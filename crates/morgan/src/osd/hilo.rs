//! El hilo del OSD: su propia conexión de Wayland, sus superficies (una por salida) y la GPU.
//! El hilo de GTK solo le manda órdenes; si se traba, el aviso que ya está sigue en pantalla
//! y se va a su hora.
//!
//! Las superficies existen mientras hay un aviso: al irse se destruyen (Hyprland anima la
//! entrada de cada una nueva, como con las ventanas de GTK). Ninguna toma el teclado ni el
//! mouse.

use std::time::Duration;

use morgan_motor::color::Paleta;
use morgan_motor::forma::Borde;
use morgan_motor::gpu::{Gpu, Lienzo};
use morgan_motor::texto::Textos;
use morgan_motor::vello::Scene;
use smithay_client_toolkit::compositor::{
    CompositorHandler, CompositorState, FrameCallbackData, Region,
};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle, RegistrationToken};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::{delegate_dispatch2, delegate_registry, registry_handlers};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_output, wl_surface};
use wayland_client::{Connection, QueueHandle};

use super::dibujo::{self, Rotulos};
use super::{ALTO, ANCHO, Aviso, MARGEN_EXTRA, tope};

pub enum Orden {
    Mostrar(Aviso),
    /// Los colores de `theme.css`, la opacidad del fondo de las tarjetas y lo de afuera del
    /// anillo (sigue a la sombra de las ventanas).
    Tema(Paleta, f32, Borde),
}

/// Si no hay Wayland o la GPU no abre, el hilo termina y las órdenes se pierden: morgan
/// sigue sin OSD.
pub fn lanzar(paleta: Paleta, opacidad: f32, borde: Borde) -> Sender<Orden> {
    let (tx, rx) = channel::channel();
    let lanzado = std::thread::Builder::new()
        .name("morgan-osd".into())
        .spawn(move || {
            if let Err(e) = correr(rx, paleta, opacidad, borde) {
                eprintln!("morgan: osd: {e}");
            }
        });
    if let Err(e) = lanzado {
        eprintln!("morgan: osd: {e}");
    }
    tx
}

struct Ventana {
    /// Antes que `capa`: lo de wgpu se suelta antes de que se destruya la superficie.
    lienzo: Option<Lienzo>,
    capa: LayerSurface,
    salida: wl_output::WlOutput,
    tam: (u32, u32),
    escala: i32,
    /// Hay un frame callback pendiente: el compositor todavía no quiere otro cuadro.
    esperando: bool,
    sucio: bool,
    rotulos: Option<Rotulos>,
}

struct Hilo {
    conn: Connection,
    qh: QueueHandle<Hilo>,
    bucle: LoopHandle<'static, Hilo>,
    registro: RegistryState,
    compositor: CompositorState,
    capas: LayerShell,
    salidas: OutputState,
    gpu: Gpu,
    textos: Textos,
    escena: Scene,
    paleta: Paleta,
    opacidad: f32,
    borde: Borde,
    aviso: Option<Aviso>,
    /// Sube con cada aviso: los textos de cada ventana se rearman cuando cambia.
    version: u64,
    ocultar: Option<RegistrationToken>,
    ventanas: Vec<Ventana>,
}

fn correr(
    rx: channel::Channel<Orden>,
    paleta: Paleta,
    opacidad: f32,
    borde: Borde,
) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
    let (globales, mut cola) = registry_queue_init::<Hilo>(&conn).map_err(|e| e.to_string())?;
    let qh = cola.handle();
    let compositor = CompositorState::bind(&globales, &qh).map_err(|e| e.to_string())?;
    let capas = LayerShell::bind(&globales, &qh).map_err(|e| e.to_string())?;
    let mut bucle: EventLoop<Hilo> = EventLoop::try_new().map_err(|e| e.to_string())?;
    bucle
        .handle()
        .insert_source(rx, |ev, _, h: &mut Hilo| {
            if let channel::Event::Msg(o) = ev {
                h.orden(o);
            }
        })
        .map_err(|e| e.to_string())?;
    let mut h = Hilo {
        gpu: Gpu::con_display("morgan", &conn),
        conn,
        qh: qh.clone(),
        bucle: bucle.handle(),
        registro: RegistryState::new(&globales),
        compositor,
        capas,
        salidas: OutputState::new(&globales, &qh),
        textos: Textos::new(),
        escena: Scene::new(),
        paleta,
        opacidad,
        borde,
        aviso: None,
        version: 0,
        ocultar: None,
        ventanas: Vec::new(),
    };
    // El tamaño de cada salida llega después del registro (xdg-output): sin esperarlo, el
    // primer aviso se ubicaría con un tamaño inventado.
    for _ in 0..2 {
        cola.roundtrip(&mut h).map_err(|e| e.to_string())?;
    }
    WaylandSource::new(h.conn.clone(), cola)
        .insert(bucle.handle())
        .map_err(|e| e.to_string())?;
    bucle
        .run(None, &mut h, |h| h.pintar())
        .map_err(|e| e.to_string())
}

impl Hilo {
    fn orden(&mut self, o: Orden) {
        match o {
            Orden::Mostrar(a) => self.mostrar(a),
            Orden::Tema(p, o, b) => {
                (self.paleta, self.opacidad, self.borde) = (p, o, b);
                for v in &mut self.ventanas {
                    v.sucio = true;
                }
            }
        }
    }

    fn mostrar(&mut self, aviso: Aviso) {
        let ms = aviso.ms;
        self.aviso = Some(aviso);
        self.version += 1;
        for salida in self.salidas.outputs().collect::<Vec<_>>() {
            self.cubrir(salida);
        }
        for v in &mut self.ventanas {
            v.sucio = true;
        }
        if let Some(t) = self.ocultar.take() {
            self.bucle.remove(t);
        }
        let timer = Timer::from_duration(Duration::from_millis(ms.into()));
        self.ocultar = self
            .bucle
            .insert_source(timer, |_, _, h| {
                h.ocultar = None;
                h.aviso = None;
                h.ventanas.clear();
                let _ = h.conn.flush();
                TimeoutAction::Drop
            })
            .ok();
    }

    /// Una ventana en `salida`, si no la tiene. El ancho fijo, acotado a la salida; el alto,
    /// el de dos renglones, el mismo siempre.
    fn cubrir(&mut self, salida: wl_output::WlOutput) {
        if self.ventanas.iter().any(|v| v.salida == salida) {
            return;
        }
        let (w, h) = self
            .salidas
            .info(&salida)
            .and_then(|i| {
                i.logical_size.or_else(|| {
                    let m = i.modes.iter().find(|m| m.current)?;
                    let e = i.scale_factor.max(1);
                    Some((m.dimensions.0 / e, m.dimensions.1 / e))
                })
            })
            .unwrap_or((ANCHO, ALTO * 4));
        let t = tope(w, h);
        let ancho = ANCHO.min(t.ancho) as u32;
        let wl = self.compositor.create_surface(&self.qh);
        let capa = self.capas.create_layer_surface(
            &self.qh,
            wl,
            Layer::Overlay,
            Some("morgan-osd"),
            Some(&salida),
        );
        capa.set_anchor(Anchor::BOTTOM);
        capa.set_size(ancho, ALTO as u32);
        capa.set_margin(0, 0, (t.margen_inferior - MARGEN_EXTRA).max(0), 0);
        capa.set_keyboard_interactivity(KeyboardInteractivity::None);
        if let Ok(r) = Region::new(&self.compositor) {
            capa.wl_surface().set_input_region(Some(r.wl_region()));
        }
        capa.commit();
        self.ventanas.push(Ventana {
            lienzo: None,
            capa,
            salida,
            tam: (ancho, ALTO as u32),
            escala: 1,
            esperando: false,
            sucio: false,
            rotulos: None,
        });
    }

    /// Después de cada vuelta del bucle: dibuja las ventanas que lo necesitan y pueden.
    fn pintar(&mut self) {
        let Some(aviso) = &self.aviso else { return };
        for v in &mut self.ventanas {
            if v.esperando || !v.sucio {
                continue;
            }
            let Some(lienzo) = v.lienzo.as_mut() else {
                continue;
            };
            let escala = v.escala as f32;
            if v.rotulos
                .as_ref()
                .is_none_or(|r| r.version != self.version || r.escala != escala)
            {
                v.rotulos = Some(Rotulos::armar(
                    &mut self.textos,
                    aviso,
                    v.tam.0,
                    escala,
                    self.version,
                ));
            }
            let Some(rot) = &v.rotulos else { continue };
            dibujo::pintar(
                &mut self.escena,
                aviso,
                rot,
                &self.paleta,
                self.opacidad,
                self.borde,
                v.tam,
                v.escala as f64,
            );
            let wl = v.capa.wl_surface();
            let qh = &self.qh;
            // El pedido del próximo cuadro va antes del commit que hace `present`.
            if lienzo.presentar(&mut self.gpu, wl, &self.escena, v.tam, v.escala, || {
                wl.frame(qh, FrameCallbackData(wl.clone()));
            }) {
                v.esperando = true;
                v.sucio = false;
            }
        }
        let _ = self.conn.flush();
    }

    fn ventana(&mut self, wl: &wl_surface::WlSurface) -> Option<&mut Ventana> {
        self.ventanas.iter_mut().find(|v| v.capa.wl_surface() == wl)
    }
}

impl LayerShellHandler for Hilo {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, capa: &LayerSurface) {
        self.ventanas
            .retain(|v| v.capa.wl_surface() != capa.wl_surface());
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        capa: &LayerSurface,
        c: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Hilo {
            ventanas,
            gpu,
            conn,
            ..
        } = self;
        let Some(v) = ventanas
            .iter_mut()
            .find(|v| v.capa.wl_surface() == capa.wl_surface())
        else {
            return;
        };
        if c.new_size.0 > 0 && c.new_size.1 > 0 {
            v.tam = c.new_size;
        }
        if v.lienzo.is_none() {
            // SAFETY: el lienzo se suelta antes que la capa (orden de los campos de Ventana).
            v.lienzo = unsafe { Lienzo::nuevo(gpu, conn, v.capa.wl_surface()) };
        }
        v.sucio = true;
    }
}

impl CompositorHandler for Hilo {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if let Some(v) = self.ventana(surface) {
            v.escala = factor.max(1);
            v.sucio = true;
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if let Some(v) = self.ventana(surface) {
            v.esperando = false;
        }
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Hilo {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.salidas
    }

    /// El aviso que está sigue hasta su hora, ahora también en la salida nueva.
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, salida: wl_output::WlOutput) {
        if self.aviso.is_some() {
            self.cubrir(salida);
        }
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        salida: wl_output::WlOutput,
    ) {
        self.ventanas.retain(|v| v.salida != salida);
    }
}

impl ProvidesRegistryState for Hilo {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registro
    }

    registry_handlers![OutputState];
}

delegate_registry!(Hilo);
delegate_dispatch2!(Hilo);
