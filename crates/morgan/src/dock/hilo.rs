//! El hilo del dock: su propia conexión de Wayland, una capa por salida, el globo y la GPU. El
//! hilo de GTK le manda las apps y recibe los clicks; si se traba, el dock sigue respondiendo.
//!
//! Cada capa cubre el ancho de la salida y recibe el puntero solo en la isla. Con un juego en
//! foco o el dock apagado no hay capas: no reservan lugar ni molestan al juego.

use std::time::{Duration, Instant};

use morgan_motor::color::Paleta;
use morgan_motor::curva::{Revelado, Transicion};
use morgan_motor::forma::Borde;
use morgan_motor::gpu::{Gpu, Lienzo};
use morgan_motor::texto::Textos;
use morgan_motor::vello::Scene;
use morgan_motor::vello::peniko::ImageData;
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle, RegistrationToken};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::{delegate_dispatch2, delegate_registry, registry_handlers};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_output, wl_pointer, wl_seat, wl_surface};
use wayland_client::{Connection, QueueHandle};

use super::dibujo::{self, GLOBO_MARGEN, Rotulos};
use super::modelo::{ALTO_CAPA, Disposicion, Pieza, ZONA, disponer, entran, tope};
use crate::lamina::Lamina;

const BTN_IZQUIERDO: u32 = 0x110;
const BTN_MEDIO: u32 = 0x112;

/// Como los globos de la barra: si otro se cerró hace menos de `ENSEGUIDA`, sale enseguida.
const ESPERA_GLOBO: Duration = Duration::from_millis(500);
const ENSEGUIDA: Duration = Duration::from_millis(400);
/// Para llegar del ícono al globo sin que se cierre.
const CERRAR_GLOBO: Duration = Duration::from_millis(200);
const ESPERA_OCULTAR: Duration = Duration::from_millis(400);
const DESLIZAR: Duration = Duration::from_millis(220);
const TINTE: Duration = Duration::from_millis(150);
/// El globo, por encima de la isla.
const ENCIMA_DE_LA_ISLA: i32 = 4;

pub struct Datos {
    pub piezas: Vec<Pieza>,
    pub iconos: Vec<ImageData>,
}

pub enum Orden {
    Tema(Paleta, f32, Borde),
    Apps(Datos),
    /// Hay un juego en foco.
    Juego(bool),
    Config {
        mostrar: bool,
        ocultar: bool,
        nota: String,
    },
}

/// Lo que se tocó, para el hilo de GTK.
pub enum Evento {
    Click(String),
    Nueva(String),
    Enfocar(String),
}

pub struct Arranque {
    pub paleta: Paleta,
    pub opacidad: f32,
    pub borde: Borde,
    pub mostrar: bool,
    pub ocultar: bool,
    pub nota: String,
}

/// Si no hay Wayland o la GPU no abre, el hilo termina: morgan sigue sin dock.
pub fn lanzar(a: Arranque) -> (Sender<Orden>, async_channel::Receiver<Evento>) {
    let (tx, rx) = channel::channel();
    let (etx, erx) = async_channel::unbounded();
    let lanzado = std::thread::Builder::new()
        .name("morgan-dock".into())
        .spawn(move || {
            if let Err(e) = correr(rx, etx, a) {
                eprintln!("morgan: dock: {e}");
            }
        });
    if let Err(e) = lanzado {
        eprintln!("morgan: dock: {e}");
    }
    (tx, erx)
}

struct Ventana {
    /// Antes que `capa`: lo de wgpu se suelta antes de que se destruya la superficie.
    lamina: Lamina,
    capa: LayerSurface,
    salida: wl_output::WlOutput,
    disposicion: Disposicion,
    /// Los índices de `Datos::piezas` que entran.
    visibles: Vec<usize>,
    tintes: Vec<Transicion>,
    encima: Option<usize>,
    /// El puntero está sobre la capa (sin importar si hay una celda debajo).
    adentro: bool,
    subida: Revelado,
}

struct Globo {
    lamina: Lamina,
    capa: LayerSurface,
    clave: String,
    rotulos: Rotulos,
    encima: Option<usize>,
}

#[derive(Clone, Copy)]
enum Pendiente {
    Abrir(usize, usize),
    Cerrar,
}

struct Hilo {
    conn: Connection,
    qh: QueueHandle<Hilo>,
    bucle: LoopHandle<'static, Hilo>,
    registro: RegistryState,
    asientos: SeatState,
    compositor: CompositorState,
    capas: LayerShell,
    salidas: OutputState,
    puntero: Option<wl_pointer::WlPointer>,
    cursor: Option<CursorShapeManager>,
    forma: Option<WpCursorShapeDeviceV1>,
    gpu: Gpu,
    textos: Textos,
    escena: Scene,
    paleta: Paleta,
    opacidad: f32,
    borde: Borde,
    datos: Datos,
    mostrar: bool,
    ocultar: bool,
    juego: bool,
    nota: String,
    ventanas: Vec<Ventana>,
    globo: Option<Globo>,
    globo_cerrado: Option<Instant>,
    temporizador: Option<RegistrationToken>,
    temporizador_ocultar: Option<RegistrationToken>,
    eventos: async_channel::Sender<Evento>,
}

fn correr(
    rx: channel::Channel<Orden>,
    eventos: async_channel::Sender<Evento>,
    a: Arranque,
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
        asientos: SeatState::new(&globales, &qh),
        compositor,
        capas,
        salidas: OutputState::new(&globales, &qh),
        puntero: None,
        cursor: CursorShapeManager::bind(&globales, &qh).ok(),
        forma: None,
        textos: Textos::new(),
        escena: Scene::new(),
        paleta: a.paleta,
        opacidad: a.opacidad,
        borde: a.borde,
        datos: Datos {
            piezas: Vec::new(),
            iconos: Vec::new(),
        },
        mostrar: a.mostrar,
        ocultar: a.ocultar,
        juego: false,
        nota: a.nota,
        ventanas: Vec::new(),
        globo: None,
        globo_cerrado: None,
        temporizador: None,
        temporizador_ocultar: None,
        eventos,
    };
    for _ in 0..2 {
        cola.roundtrip(&mut h).map_err(|e| e.to_string())?;
    }
    WaylandSource::new(h.conn.clone(), cola)
        .insert(bucle.handle())
        .map_err(|e| e.to_string())?;
    h.sincronizar();
    bucle
        .run(None, &mut h, |h| h.pintar())
        .map_err(|e| e.to_string())
}

impl Hilo {
    fn orden(&mut self, o: Orden) {
        match o {
            Orden::Tema(p, o, b) => {
                (self.paleta, self.opacidad, self.borde) = (p, o, b);
                self.ensuciar();
            }
            Orden::Apps(d) => {
                self.datos = d;
                self.rehacer();
            }
            Orden::Juego(j) => {
                self.juego = j;
                self.sincronizar();
            }
            Orden::Config {
                mostrar,
                ocultar,
                nota,
            } => {
                let cambia = ocultar != self.ocultar;
                self.nota = nota;
                self.mostrar = mostrar;
                self.ocultar = ocultar;
                // La zona reservada se fija al crear la capa.
                if cambia {
                    self.quitar_capas();
                }
                self.sincronizar();
            }
        }
    }

    fn ensuciar(&mut self) {
        for v in &mut self.ventanas {
            v.lamina.sucio = true;
        }
        if let Some(g) = &mut self.globo {
            g.lamina.sucio = true;
        }
    }

    fn quitar_capas(&mut self) {
        self.globo = None;
        self.ventanas.clear();
    }

    /// Crea o destruye las capas según corresponda.
    fn sincronizar(&mut self) {
        if self.mostrar && !self.juego {
            for salida in self.salidas.outputs().collect::<Vec<_>>() {
                self.cubrir(salida);
            }
        } else {
            self.quitar_capas();
        }
        let _ = self.conn.flush();
    }

    fn cubrir(&mut self, salida: wl_output::WlOutput) {
        if self.ventanas.iter().any(|v| v.salida == salida) {
            return;
        }
        let wl = self.compositor.create_surface(&self.qh);
        let capa = self.capas.create_layer_surface(
            &self.qh,
            wl,
            Layer::Top,
            Some("morgan-dock"),
            Some(&salida),
        );
        capa.set_anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        capa.set_size(0, ALTO_CAPA);
        capa.set_exclusive_zone(if self.ocultar { -1 } else { ZONA as i32 });
        capa.set_keyboard_interactivity(KeyboardInteractivity::None);
        // Hasta saber el ancho, no recibe nada.
        if let Ok(r) = Region::new(&self.compositor) {
            capa.wl_surface().set_input_region(Some(r.wl_region()));
        }
        capa.commit();
        self.ventanas.push(Ventana {
            lamina: Lamina::new((1, ALTO_CAPA)),
            capa,
            salida,
            disposicion: Disposicion::default(),
            visibles: Vec::new(),
            tintes: Vec::new(),
            encima: None,
            adentro: false,
            subida: Revelado::new(!self.ocultar),
        });
    }

    /// Después de cambiar las apps o el tamaño: qué entra, dónde, y lo que recibe el puntero.
    fn rehacer(&mut self) {
        for i in 0..self.ventanas.len() {
            self.disponer(i);
        }
        // El globo abierto se rearma con lo nuevo; si su app ya no está, se va.
        if let Some(g) = &self.globo {
            let clave = g.clave.clone();
            let pieza = self.datos.piezas.iter().find(|p| p.clave == clave).cloned();
            match pieza {
                Some(p) => {
                    let escala = self.globo.as_ref().map_or(1.0, |g| g.lamina.escala as f32);
                    let r = Rotulos::armar(&mut self.textos, &p, &self.nota, escala);
                    let tam = r.tam;
                    if let Some(g) = &mut self.globo {
                        if tam != g.rotulos.tam {
                            g.capa.set_size(tam.0, tam.1);
                            g.capa.commit();
                            g.lamina.tam = tam;
                        }
                        g.rotulos = r;
                        g.lamina.sucio = true;
                    }
                }
                None => self.cerrar_globo(),
            }
        }
        let _ = self.conn.flush();
    }

    fn disponer(&mut self, i: usize) {
        let piezas = &self.datos.piezas;
        let v = &mut self.ventanas[i];
        let ancho = f64::from(v.lamina.tam.0);
        v.visibles = entran(piezas, tope(ancho));
        let usadas: Vec<bool> = v.visibles.iter().map(|&p| piezas[p].usada).collect();
        v.disposicion = disponer(ancho, &usadas);
        v.tintes.resize(v.visibles.len(), Transicion::new(0.0));
        v.encima = v.encima.filter(|&e| e < v.visibles.len());
        v.lamina.sucio = true;
        Self::poner_region(&self.compositor, v);
    }

    fn poner_region(compositor: &CompositorState, v: &Ventana) {
        let Ok(r) = Region::new(compositor) else { return };
        if let Some(c) = v
            .disposicion
            .region(f64::from(v.lamina.tam.1), v.subida.abierto())
        {
            r.add(
                c.x0 as i32,
                c.y0 as i32,
                c.width() as i32,
                c.height() as i32,
            );
        }
        v.capa.wl_surface().set_input_region(Some(r.wl_region()));
    }

    fn temporizar(&mut self, dura: Duration, que: Pendiente) {
        if let Some(t) = self.temporizador.take() {
            self.bucle.remove(t);
        }
        self.temporizador = self
            .bucle
            .insert_source(Timer::from_duration(dura), move |_, _, h: &mut Hilo| {
                h.temporizador = None;
                match que {
                    Pendiente::Abrir(i, celda) => h.abrir_globo(i, celda),
                    Pendiente::Cerrar => h.cerrar_globo(),
                }
                TimeoutAction::Drop
            })
            .ok();
    }

    fn sin_temporizador(&mut self) {
        if let Some(t) = self.temporizador.take() {
            self.bucle.remove(t);
        }
    }

    fn abrir_globo(&mut self, i: usize, celda: usize) {
        let Some(v) = self.ventanas.get(i) else { return };
        let (Some(&p), Some(c)) = (v.visibles.get(celda), v.disposicion.celdas.get(celda)) else {
            return;
        };
        let Some(pieza) = self.datos.piezas.get(p) else { return };
        let escala = v.lamina.escala.max(1);
        let rotulos = Rotulos::armar(&mut self.textos, pieza, &self.nota, escala as f32);
        let tam = rotulos.tam;
        let ancho_salida = i32::try_from(v.lamina.tam.0).unwrap_or(i32::MAX);
        let izquierda = (c.center().x as i32 - tam.0 as i32 / 2)
            .clamp(0, (ancho_salida - tam.0 as i32).max(0));
        let abajo = ZONA as i32 + ENCIMA_DE_LA_ISLA - GLOBO_MARGEN as i32;
        let salida = v.salida.clone();
        let clave = pieza.clave.clone();

        let wl = self.compositor.create_surface(&self.qh);
        let capa = self.capas.create_layer_surface(
            &self.qh,
            wl,
            Layer::Top,
            Some("morgan-dock-globo"),
            Some(&salida),
        );
        capa.set_anchor(Anchor::BOTTOM | Anchor::LEFT);
        capa.set_size(tam.0, tam.1);
        // Lo mide desde el borde de la salida, pase lo que pase con la zona del dock.
        capa.set_exclusive_zone(-1);
        capa.set_margin(0, 0, abajo, izquierda);
        capa.set_keyboard_interactivity(KeyboardInteractivity::None);
        capa.commit();
        let mut lamina = Lamina::new(tam);
        lamina.escala = escala;
        // El anterior se va recién acá: reemplazarlo no deja un cuadro sin globo.
        self.globo = Some(Globo {
            lamina,
            capa,
            clave,
            rotulos,
            encima: None,
        });
        let _ = self.conn.flush();
    }

    fn cerrar_globo(&mut self) {
        if self.globo.take().is_some() {
            self.globo_cerrado = Some(Instant::now());
            let _ = self.conn.flush();
        }
        self.programar_ocultar();
    }

    /// Con `ocultar`, baja las islas que ya nadie está mirando.
    fn programar_ocultar(&mut self) {
        if !self.ocultar {
            return;
        }
        if let Some(t) = self.temporizador_ocultar.take() {
            self.bucle.remove(t);
        }
        self.temporizador_ocultar = self
            .bucle
            .insert_source(Timer::from_duration(ESPERA_OCULTAR), |_, _, h: &mut Hilo| {
                h.temporizador_ocultar = None;
                h.ocultar_ahora();
                TimeoutAction::Drop
            })
            .ok();
    }

    fn ocultar_ahora(&mut self) {
        if self.globo.is_some() {
            return;
        }
        let ahora = Instant::now();
        for v in &mut self.ventanas {
            if !v.adentro && v.subida.abierto() {
                v.subida.ir(false, DESLIZAR, ahora);
                v.lamina.sucio = true;
                Self::poner_region(&self.compositor, v);
            }
        }
        let _ = self.conn.flush();
    }

    fn pintar(&mut self) {
        let ahora = Instant::now();
        let Hilo {
            ventanas,
            globo,
            gpu,
            escena,
            paleta,
            opacidad,
            borde,
            datos,
            qh,
            conn,
            ..
        } = self;
        for v in ventanas.iter_mut() {
            let anda = v.subida.anda(ahora) || v.tintes.iter().any(|t| t.anda(ahora));
            if !(v.lamina.sucio || anda) || !v.lamina.lista() {
                continue;
            }
            let tintes: Vec<f32> = v.tintes.iter().map(|t| t.valor(ahora)).collect();
            let piezas: Vec<(&Pieza, &ImageData)> = v
                .visibles
                .iter()
                .filter_map(|&i| Some((datos.piezas.get(i)?, datos.iconos.get(i)?)))
                .collect();
            dibujo::pintar_dock(
                escena,
                &v.disposicion,
                &piezas,
                &tintes,
                v.subida.valor(ahora),
                paleta,
                *opacidad,
                *borde,
                v.lamina.tam,
                f64::from(v.lamina.escala),
            );
            v.lamina.presentar(gpu, escena, v.capa.wl_surface(), qh);
            // Lo que sigue andando pide el próximo cuadro.
            v.lamina.sucio = v.subida.anda(ahora) || v.tintes.iter().any(|t| t.anda(ahora));
        }
        if let Some(g) = globo
            && g.lamina.sucio
            && g.lamina.lista()
        {
            dibujo::pintar_globo(
                escena,
                &g.rotulos,
                g.encima,
                paleta,
                *opacidad,
                f64::from(g.lamina.escala),
            );
            g.lamina.presentar(gpu, escena, g.capa.wl_surface(), qh);
        }
        let _ = conn.flush();
    }

    fn poner_encima(&mut self, i: usize, celda: Option<usize>) {
        let ahora = Instant::now();
        let v = &mut self.ventanas[i];
        if v.encima == celda {
            return;
        }
        if let Some(o) = v.encima.and_then(|o| v.tintes.get_mut(o)) {
            o.ir(0.0, TINTE, ahora);
        }
        if let Some(n) = celda.and_then(|n| v.tintes.get_mut(n)) {
            n.ir(1.0, TINTE, ahora);
        }
        v.encima = celda;
        v.lamina.sucio = true;
        match celda {
            Some(c) => {
                let enseguida = self.globo.is_some()
                    || self
                        .globo_cerrado
                        .is_some_and(|t| ahora.duration_since(t) < ENSEGUIDA);
                if enseguida {
                    self.sin_temporizador();
                    self.abrir_globo(i, c);
                } else {
                    self.temporizar(ESPERA_GLOBO, Pendiente::Abrir(i, c));
                }
            }
            None if self.globo.is_some() => self.temporizar(CERRAR_GLOBO, Pendiente::Cerrar),
            None => self.sin_temporizador(),
        }
    }

    fn puntero_dock(&mut self, i: usize, e: &PointerEvent) {
        let (x, y) = e.position;
        match e.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                let v = &mut self.ventanas[i];
                v.adentro = true;
                if !v.subida.abierto() {
                    v.subida.ir(true, DESLIZAR, Instant::now());
                    v.lamina.sucio = true;
                    Self::poner_region(&self.compositor, v);
                    let _ = self.conn.flush();
                }
                let celda = self.ventanas[i].disposicion.celda_en(x, y);
                self.poner_encima(i, celda);
            }
            PointerEventKind::Leave { .. } => {
                self.ventanas[i].adentro = false;
                self.poner_encima(i, None);
                self.programar_ocultar();
            }
            PointerEventKind::Release { button, .. } => {
                let v = &self.ventanas[i];
                let Some(p) = v
                    .encima
                    .and_then(|c| v.visibles.get(c))
                    .and_then(|&p| self.datos.piezas.get(p))
                else {
                    return;
                };
                let evento = match button {
                    BTN_IZQUIERDO => Evento::Click(p.clave.clone()),
                    BTN_MEDIO => Evento::Nueva(p.clave.clone()),
                    _ => return,
                };
                let _ = self.eventos.send_blocking(evento);
                self.sin_temporizador();
                self.cerrar_globo();
            }
            _ => {}
        }
    }

    fn puntero_globo(&mut self, e: &PointerEvent) {
        let (x, y) = e.position;
        match e.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                self.sin_temporizador();
                if let Some(g) = &mut self.globo {
                    let fila = g.rotulos.fila_en(x, y);
                    if fila != g.encima {
                        g.encima = fila;
                        g.lamina.sucio = true;
                    }
                }
            }
            PointerEventKind::Leave { .. } => {
                if let Some(g) = &mut self.globo {
                    g.encima = None;
                    g.lamina.sucio = true;
                }
                self.temporizar(CERRAR_GLOBO, Pendiente::Cerrar);
            }
            PointerEventKind::Release {
                button: BTN_IZQUIERDO,
                ..
            } => {
                let direccion = self.globo.as_ref().and_then(|g| {
                    let fila = g.encima?;
                    let p = self.datos.piezas.iter().find(|p| p.clave == g.clave)?;
                    Some(p.filas.get(fila)?.direccion.clone())
                });
                if let Some(d) = direccion {
                    let _ = self.eventos.send_blocking(Evento::Enfocar(d));
                    self.sin_temporizador();
                    self.cerrar_globo();
                }
            }
            _ => {}
        }
    }
}

impl LayerShellHandler for Hilo {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, capa: &LayerSurface) {
        let wl = capa.wl_surface();
        if self.globo.as_ref().is_some_and(|g| g.capa.wl_surface() == wl) {
            self.globo = None;
            return;
        }
        if self.ventanas.iter().any(|v| v.capa.wl_surface() == wl) {
            self.globo = None;
            self.ventanas.retain(|v| v.capa.wl_surface() != wl);
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        capa: &LayerSurface,
        c: LayerSurfaceConfigure,
        _: u32,
    ) {
        let wl = capa.wl_surface();
        if let Some(g) = &mut self.globo
            && g.capa.wl_surface() == wl
        {
            if c.new_size.0 > 0 && c.new_size.1 > 0 {
                g.lamina.tam = c.new_size;
            }
            if g.lamina.lienzo.is_none() {
                // SAFETY: la lámina se suelta antes que la capa (orden de los campos de Globo).
                g.lamina.lienzo = unsafe { Lienzo::nuevo(&mut self.gpu, &self.conn, wl) };
            }
            g.lamina.sucio = true;
            return;
        }
        let Some(i) = self.ventanas.iter().position(|v| v.capa.wl_surface() == wl) else {
            return;
        };
        let v = &mut self.ventanas[i];
        if c.new_size.0 > 0 && c.new_size.1 > 0 {
            v.lamina.tam = c.new_size;
        }
        if v.lamina.lienzo.is_none() {
            // SAFETY: la lámina se suelta antes que la capa (orden de los campos de Ventana).
            v.lamina.lienzo = unsafe { Lienzo::nuevo(&mut self.gpu, &self.conn, wl) };
        }
        self.disponer(i);
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
        let factor = factor.max(1);
        if let Some(g) = &mut self.globo
            && g.capa.wl_surface() == surface
        {
            g.lamina.escala = factor;
            g.lamina.sucio = true;
        }
        if let Some(v) = self.ventanas.iter_mut().find(|v| v.capa.wl_surface() == surface) {
            v.lamina.escala = factor;
            v.lamina.sucio = true;
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
        if let Some(g) = &mut self.globo
            && g.capa.wl_surface() == surface
        {
            g.lamina.esperando = false;
        }
        if let Some(v) = self.ventanas.iter_mut().find(|v| v.capa.wl_surface() == surface) {
            v.lamina.esperando = false;
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

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, salida: wl_output::WlOutput) {
        if self.mostrar && !self.juego {
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
        // El globo guarda el índice de su ventana en el temporizador: se va con ella.
        self.sin_temporizador();
        self.globo = None;
        self.ventanas.retain(|v| v.salida != salida);
    }
}

impl SeatHandler for Hilo {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.asientos
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        c: Capability,
    ) {
        if c == Capability::Pointer && self.puntero.is_none() {
            self.puntero = self.asientos.get_pointer(qh, &seat).ok();
            if let (Some(m), Some(p)) = (&self.cursor, &self.puntero) {
                self.forma = Some(m.get_shape_device(p, qh));
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        c: Capability,
    ) {
        if c == Capability::Pointer {
            if let Some(f) = self.forma.take() {
                f.destroy();
            }
            if let Some(p) = self.puntero.take() {
                p.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Hilo {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        eventos: &[PointerEvent],
    ) {
        for e in eventos {
            if let PointerEventKind::Enter { serial } = e.kind
                && let Some(f) = &self.forma
            {
                f.set_shape(serial, Shape::Default);
            }
            if self
                .globo
                .as_ref()
                .is_some_and(|g| g.capa.wl_surface() == &e.surface)
            {
                self.puntero_globo(e);
            } else if let Some(i) = self
                .ventanas
                .iter()
                .position(|v| v.capa.wl_surface() == &e.surface)
            {
                self.puntero_dock(i, e);
            }
        }
    }
}

impl ProvidesRegistryState for Hilo {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registro
    }

    registry_handlers![OutputState, SeatState];
}

delegate_registry!(Hilo);
delegate_dispatch2!(Hilo);
