//! El hilo de la barra y el calendario: su propia conexión de Wayland, sus superficies (una
//! barra por salida, el globo de ayuda y el calendario abierto) y la GPU. El hilo de GTK le
//! manda el estado y recibe lo que se tocó; si se traba, la barra sigue respondiendo al puntero
//! y el calendario abierto se sigue moviendo.
//!
//! El calendario existe mientras está abierto: al cerrarse se destruye (Hyprland anima la
//! entrada de cada una nueva, como con las ventanas de GTK). Lo que se ve baja desde su borde
//! de abajo (`bajada.rs`).

use std::time::{Duration, Instant};

use morgan_ipc::{AccionInterruptor, AccionSuperficie, Request};
use morgan_motor::color::Paleta;
use morgan_motor::curva::Revelado;
use morgan_motor::forma::Borde;
use morgan_motor::gpu::{Gpu, Lienzo};
use morgan_motor::parley::Layout;
use morgan_motor::texto::Textos;
use morgan_motor::vello::Scene;
use smithay_client_toolkit::compositor::{
    CompositorHandler, CompositorState, Region,
};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle, RegistrationToken};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_positioner::{
    Anchor as Ancla, ConstraintAdjustment, Gravity,
};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
};
use smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::xdg::XdgPositioner;
use smithay_client_toolkit::error::GlobalError;
use smithay_client_toolkit::globals::{GlobalData, ProvidesBoundGlobal};
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_wm_base::XdgWmBase;
use smithay_client_toolkit::shell::xdg::popup::{Popup, PopupConfigure, PopupHandler};
use smithay_client_toolkit::{delegate_dispatch2, delegate_registry, registry_handlers};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface};
use wayland_client::{Connection, QueueHandle};

use super::dibujo::{self, ALTO, Animos, Estado, Icono, Lugar, Pintura, Que, Rotulos};
use crate::calendario::dibujo::{self as cal, Cal, Hoja, Objetivo};
use crate::hyprland::Espacios;
use crate::lamina::Lamina;

/// Lo que GTK espera del puntero quieto antes de mostrar el globo; si otro globo se cerró hace
/// menos, el siguiente sale enseguida.
const ESPERA_GLOBO: Duration = Duration::from_millis(500);

pub enum Orden {
    /// Los colores de `theme.css`, la opacidad del fondo y lo de afuera de los paneles (que
    /// sigue a la sombra de las ventanas).
    Tema(Paleta, f32, Borde),
    Espacios(Espacios),
    Reloj {
        fecha: String,
        hora: String,
    },
    Campana {
        avisos: usize,
        no_molestar: bool,
    },
    Centro(bool),
    Privacidad {
        glifos: String,
        tooltip: String,
    },
    /// El tiempo que lleva grabando, o `None` si no se graba.
    Grabacion(Option<String>),
    Bandeja(Vec<Icono>),
    /// El ícono de la bandeja con su menú abierto.
    Menu(Option<String>),
    Calendario(cal::Orden),
}

/// Lo que se tocó, para el hilo de GTK.
pub enum Evento {
    Pedir(Request),
    IrA(i64),
    Grabar,
    /// `boton` como en GTK (1, 2, 3); `derecha`: del borde derecho del ícono al de la salida.
    Bandeja {
        clave: String,
        boton: u32,
        salida: String,
        derecha: i32,
    },
    Desplazar {
        clave: String,
        delta: i32,
    },
    CalendarioCerrado,
}

/// Si no hay Wayland o la GPU no abre, el hilo termina: morgan sigue sin barra.
pub fn lanzar(
    paleta: Paleta,
    opacidad: f32,
    borde: Borde,
    fijos: std::collections::BTreeMap<String, Vec<i64>>,
) -> (Sender<Orden>, async_channel::Receiver<Evento>) {
    let (tx, rx) = channel::channel();
    let (etx, erx) = async_channel::unbounded();
    let lanzado = std::thread::Builder::new()
        .name("morgan-barra".into())
        .spawn(move || {
            if let Err(e) = correr(rx, etx, paleta, opacidad, borde, fijos) {
                eprintln!("morgan: barra: {e}");
            }
        });
    if let Err(e) = lanzado {
        eprintln!("morgan: barra: {e}");
    }
    (tx, erx)
}

struct Barra {
    lamina: Lamina,
    capa: LayerSurface,
    salida: wl_output::WlOutput,
    nombre: String,
    rotulos: Option<Rotulos>,
    animos: Animos,
    revelado: Revelado,
    /// Lo que tiene el puntero encima.
    encima: Option<Que>,
    en_cajon: bool,
}

struct Globo {
    lamina: Lamina,
    popup: Popup,
    texto: Layout<u8>,
    /// De qué pieza y de qué barra.
    de: (wl_surface::WlSurface, Que),
}

struct Calendario {
    lamina: Lamina,
    capa: LayerSurface,
    hoja: Option<Hoja>,
    animos: cal::Animos,
    /// El primer cuadro: desde ahí baja.
    inicio: Option<Instant>,
    marquesina: Instant,
    encima: Option<Objetivo>,
}

struct Hilo {
    conn: Connection,
    qh: QueueHandle<Hilo>,
    bucle: LoopHandle<'static, Hilo>,
    registro: RegistryState,
    compositor: CompositorState,
    capas: LayerShell,
    xdg: Option<WmBase>,
    salidas: OutputState,
    asientos: SeatState,
    cursor: Option<CursorShapeManager>,
    puntero: Option<wl_pointer::WlPointer>,
    forma: Option<WpCursorShapeDeviceV1>,
    teclado: Option<wl_keyboard::WlKeyboard>,
    gpu: Gpu,
    textos: Textos,
    escena: Scene,
    paleta: Paleta,
    opacidad: f32,
    borde: Borde,
    estado: Estado,
    /// Sube con cada cambio del estado: los textos de cada barra se rearman.
    version: u64,
    grabando_desde: Option<Instant>,
    barras: Vec<Barra>,
    globo: Option<Globo>,
    espera_globo: Option<RegistrationToken>,
    /// Cuándo se fue el último globo.
    sin_globo: Option<Instant>,
    /// Lo apretado: en qué superficie, y qué de la barra o del calendario.
    apretado: Option<(wl_surface::WlSurface, Option<Que>, Option<Objetivo>)>,
    cal: Cal,
    calendario: Option<Calendario>,
    eventos: async_channel::Sender<Evento>,
}

fn correr(
    rx: channel::Channel<Orden>,
    eventos: async_channel::Sender<Evento>,
    paleta: Paleta,
    opacidad: f32,
    borde: Borde,
    fijos: std::collections::BTreeMap<String, Vec<i64>>,
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
        xdg: globales
            .bind::<XdgWmBase, _, _>(&qh, 5..=6, GlobalData)
            .ok()
            .map(WmBase),
        salidas: OutputState::new(&globales, &qh),
        asientos: SeatState::new(&globales, &qh),
        cursor: CursorShapeManager::bind(&globales, &qh).ok(),
        puntero: None,
        forma: None,
        teclado: None,
        textos: Textos::new(),
        escena: Scene::new(),
        paleta,
        opacidad,
        borde,
        estado: Estado {
            fijos,
            ..Estado::default()
        },
        version: 0,
        grabando_desde: None,
        barras: Vec::new(),
        globo: None,
        espera_globo: None,
        sin_globo: None,
        apretado: None,
        cal: Cal::default(),
        calendario: None,
        eventos,
    };
    // El nombre y el tamaño de cada salida llegan después del registro (xdg-output).
    for _ in 0..2 {
        cola.roundtrip(&mut h).map_err(|e| e.to_string())?;
    }
    for salida in h.salidas.outputs().collect::<Vec<_>>() {
        h.cubrir(salida);
    }
    WaylandSource::new(h.conn.clone(), cola)
        .insert(bucle.handle())
        .map_err(|e| e.to_string())?;
    bucle
        .run(None, &mut h, |h| h.pintar())
        .map_err(|e| e.to_string())
}

impl Hilo {
    fn avisar(&self, e: Evento) {
        let _ = self.eventos.try_send(e);
    }

    fn orden(&mut self, o: Orden) {
        let e = &mut self.estado;
        match o {
            Orden::Tema(p, o, b) => {
                (self.paleta, self.opacidad, self.borde) = (p, o, b);
                self.ensuciar();
                return;
            }
            Orden::Espacios(x) => e.espacios = x,
            Orden::Reloj { fecha, hora } => (e.fecha, e.hora) = (fecha, hora),
            Orden::Campana {
                avisos,
                no_molestar,
            } => {
                (e.avisos, e.no_molestar) = (avisos, no_molestar);
                self.refrescar_globo();
            }
            Orden::Centro(abierto) => e.centro = abierto,
            Orden::Privacidad { glifos, tooltip } => {
                (e.privacidad, e.tooltip_privacidad) = (glifos, tooltip);
                self.refrescar_globo();
            }
            Orden::Grabacion(g) => {
                if g.is_some() != e.grabacion.is_some() {
                    self.grabando_desde = g.as_ref().map(|_| Instant::now());
                }
                self.estado.grabacion = g;
            }
            Orden::Bandeja(iconos) => {
                e.bandeja = iconos;
                self.refrescar_globo();
            }
            Orden::Menu(m) => e.menu = m,
            Orden::Calendario(o) => {
                self.orden_calendario(o);
                return;
            }
        }
        self.version += 1;
        self.ensuciar();
        self.objetivos();
    }

    fn ensuciar(&mut self) {
        for b in &mut self.barras {
            b.lamina.sucio = true;
        }
        if let Some(c) = &mut self.calendario {
            c.lamina.sucio = true;
        }
        if let Some(g) = &mut self.globo {
            g.lamina.sucio = true;
        }
    }

    /// Una barra en `salida`, si no la tiene.
    fn cubrir(&mut self, salida: wl_output::WlOutput) {
        if self.barras.iter().any(|b| b.salida == salida) {
            return;
        }
        let nombre = self
            .salidas
            .info(&salida)
            .and_then(|i| i.name)
            .unwrap_or_default();
        let wl = self.compositor.create_surface(&self.qh);
        let capa = self.capas.create_layer_surface(
            &self.qh,
            wl,
            Layer::Top,
            Some("morgan-bar"),
            Some(&salida),
        );
        capa.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
        capa.set_size(0, ALTO);
        capa.set_exclusive_zone(ALTO as i32);
        capa.set_keyboard_interactivity(KeyboardInteractivity::None);
        capa.commit();
        self.barras.push(Barra {
            lamina: Lamina::new((0, ALTO)),
            capa,
            salida,
            nombre,
            rotulos: None,
            animos: Animos::default(),
            revelado: Revelado::new(false),
            encima: None,
            en_cajon: false,
        });
    }

    fn lugar(b: &Barra, estado: &Estado, ahora: Instant) -> Option<Lugar> {
        let rot = b.rotulos.as_ref()?;
        Some(dibujo::ubicar(
            f64::from(b.lamina.tam.0),
            rot,
            estado.bandeja.len(),
            b.revelado.valor(ahora),
        ))
    }

    /// Hacia dónde va cada pieza: encendida con el puntero encima o con lo suyo abierto.
    fn objetivos(&mut self) {
        let ahora = Instant::now();
        let cal_abierto = self.calendario.is_some();
        let e = &self.estado;
        for b in &mut self.barras {
            let abrir = b.en_cajon || e.menu.is_some();
            b.revelado
                .ir(abrir, crate::animaciones::dur(dibujo::REVELAR), ahora);
            let Some(lugar) = Self::lugar(b, e, ahora) else {
                continue;
            };
            for p in &lugar.piezas {
                let abierto = match p.que {
                    Que::Reloj => cal_abierto,
                    Que::Campana => e.centro,
                    Que::Icono(i) => e
                        .menu
                        .as_ref()
                        .is_some_and(|m| e.bandeja.get(i).is_some_and(|x| &x.clave == m)),
                    Que::Flecha => b.en_cajon,
                    _ => false,
                };
                let si = abierto || (b.encima == Some(p.que) && !p.que.quieto());
                b.animos.encender(p.que, si, ahora);
            }
        }
    }

    /// Después de cada vuelta del bucle: dibuja lo que lo necesita y puede.
    fn pintar(&mut self) {
        let ahora = Instant::now();
        let latido = self
            .grabando_desde
            .map_or(1.0, |d| dibujo::latido(d, ahora));
        for b in &mut self.barras {
            let anda =
                b.animos.anda(ahora) || b.revelado.anda(ahora) || self.grabando_desde.is_some();
            if !b.lamina.lista() || !(b.lamina.sucio || anda) {
                continue;
            }
            let escala = b.lamina.escala as f32;
            if b.rotulos
                .as_ref()
                .is_none_or(|r| r.version != self.version || r.escala != escala)
            {
                b.rotulos = Some(Rotulos::armar(
                    &mut self.textos,
                    &self.estado,
                    &b.nombre,
                    escala,
                    self.version,
                ));
            }
            let Some(lugar) = Self::lugar(b, &self.estado, ahora) else {
                continue;
            };
            let Some(rotulos) = &b.rotulos else { continue };
            Pintura {
                estado: &self.estado,
                rotulos,
                lugar: &lugar,
                animos: &b.animos,
                paleta: &self.paleta,
                opacidad: self.opacidad,
                ahora,
                latido,
            }
            .pintar(&mut self.escena, b.lamina.tam, f64::from(escala));
            let wl = b.capa.wl_surface().clone();
            b.lamina
                .presentar(&mut self.gpu, &self.escena, &wl, &self.qh);
        }

        if let Some(g) = &mut self.globo
            && g.lamina.lista()
            && g.lamina.sucio
        {
            dibujo::pintar_globo(
                &mut self.escena,
                &g.texto,
                &self.paleta,
                f64::from(g.lamina.escala),
            );
            let wl = g.popup.wl_surface().clone();
            g.lamina
                .presentar(&mut self.gpu, &self.escena, &wl, &self.qh);
        }

        if let Some(c) = &mut self.calendario {
            let entrando = c
                .inicio
                .is_none_or(|t| ahora.saturating_duration_since(t) < crate::bajada::duracion());
            let anda =
                c.animos.anda(ahora) || entrando || c.hoja.as_ref().is_some_and(Hoja::marquesina);
            if c.lamina.lista() && (c.lamina.sucio || anda) {
                let escala = c.lamina.escala as f32;
                if c.hoja.as_ref().is_none_or(|h| {
                    h.version != self.cal.version || h.escala != escala || h.ancho != c.lamina.tam.0
                }) {
                    c.hoja = Some(Hoja::armar(
                        &mut self.textos,
                        &self.cal,
                        c.lamina.tam.0,
                        escala,
                    ));
                    c.marquesina = ahora;
                }
                // La entrada cuenta desde el primer cuadro.
                let inicio = *c.inicio.get_or_insert(ahora);
                let micros = ahora.saturating_duration_since(inicio).as_micros() as i64;
                if let Some(hoja) = &c.hoja {
                    let bajada = crate::bajada::desplazamiento(micros, hoja.panel.y1 as f32);
                    cal::Pintura {
                        hoja,
                        animos: &c.animos,
                        paleta: &self.paleta,
                        opacidad: self.opacidad,
                        borde: self.borde,
                        ahora,
                        bajada: f64::from(bajada),
                        marquesina: ahora.saturating_duration_since(c.marquesina).as_secs_f64(),
                    }
                    .pintar(&mut self.escena, f64::from(escala));
                    let wl = c.capa.wl_surface().clone();
                    c.lamina
                        .presentar(&mut self.gpu, &self.escena, &wl, &self.qh);
                }
            }
        }
        let _ = self.conn.flush();
    }

    // ── El calendario ──

    fn orden_calendario(&mut self, o: cal::Orden) {
        match o {
            cal::Orden::Abrir => self.abrir_calendario(),
            cal::Orden::Cerrar => self.cerrar_calendario(false),
            cal::Orden::Clima(p) => {
                self.cal.clima = p.dias;
                self.cal.ahora = p.ahora;
                self.cal.version += 1;
            }
            cal::Orden::Eventos(e) => {
                self.cal.eventos = e;
                self.cal.version += 1;
            }
        }
        if let Some(c) = &mut self.calendario {
            c.lamina.sucio = true;
        }
    }

    fn abrir_calendario(&mut self) {
        if self.calendario.is_some() {
            return;
        }
        self.cal.elegir(cal::hoy());
        let wl = self.compositor.create_surface(&self.qh);
        let capa = self.capas.create_layer_surface(
            &self.qh,
            wl,
            Layer::Top,
            Some("morgan-calendar"),
            None,
        );
        capa.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        capa.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        capa.commit();
        self.calendario = Some(Calendario {
            lamina: Lamina::new((0, 0)),
            capa,
            hoja: None,
            animos: cal::Animos::default(),
            inicio: None,
            marquesina: Instant::now(),
            encima: None,
        });
        self.objetivos();
        self.ensuciar();
    }

    /// `propio`: lo cerró el calendario (Escape, un click afuera) y GTK no lo sabe.
    fn cerrar_calendario(&mut self, propio: bool) {
        if self.calendario.take().is_none() {
            return;
        }
        if self
            .apretado
            .as_ref()
            .is_some_and(|(_, q, o)| q.is_none() && o.is_some())
        {
            self.apretado = None;
        }
        if propio {
            self.avisar(Evento::CalendarioCerrado);
        }
        self.objetivos();
        self.ensuciar();
    }

    fn tecla_calendario(&mut self, ev: KeyEvent) {
        if self.calendario.is_none() {
            return;
        }
        let d = self.cal.elegido;
        use crate::calendario::modelo::correr;
        match ev.keysym {
            Keysym::Escape => return self.cerrar_calendario(true),
            Keysym::Left => self.cal.elegir(correr(d, -1)),
            Keysym::Right => self.cal.elegir(correr(d, 1)),
            Keysym::Up => self.cal.elegir(correr(d, -7)),
            Keysym::Down => self.cal.elegir(correr(d, 7)),
            Keysym::Page_Up => self.cal.correr_mes(-1),
            Keysym::Page_Down => self.cal.correr_mes(1),
            Keysym::Home => self.cal.elegir(cal::hoy()),
            _ => return,
        }
        if let Some(c) = &mut self.calendario {
            c.lamina.sucio = true;
        }
    }

    fn puntero_calendario(&mut self, e: &PointerEvent) {
        let ahora = Instant::now();
        let (x, y) = e.position;
        let Some(c) = &mut self.calendario else {
            return;
        };
        let bajo = c.hoja.as_ref().and_then(|h| h.bajo(x, y));
        match e.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                if bajo != c.encima {
                    if let Some(o) = c.encima {
                        c.animos.encima(o, false, ahora);
                    }
                    if let Some(o) = bajo {
                        c.animos.encima(o, true, ahora);
                    }
                    c.encima = bajo;
                    c.lamina.sucio = true;
                }
            }
            PointerEventKind::Leave { .. } => {
                if let Some(o) = c.encima.take() {
                    c.animos.encima(o, false, ahora);
                    c.lamina.sucio = true;
                }
            }
            PointerEventKind::Press { button, .. } if button == BTN_IZQUIERDO => {
                if let Some(o) = bajo {
                    c.animos.apretar(o, true, ahora);
                    c.lamina.sucio = true;
                }
                self.apretado = Some((e.surface.clone(), None, bajo));
            }
            // Cualquier botón afuera del panel lo cierra, como el click de GTK.
            PointerEventKind::Release { button, .. } => {
                let apretado = self
                    .apretado
                    .take()
                    .and_then(|(s, _, o)| (s == e.surface).then_some(o).flatten());
                if let Some(o) = apretado {
                    c.animos.apretar(o, false, ahora);
                    c.lamina.sucio = true;
                    let sigue =
                        c.hoja.as_ref().and_then(|h| h.caja(o)).is_some_and(|r| {
                            r.contains(morgan_motor::vello::kurbo::Point::new(x, y))
                        });
                    if sigue && button == BTN_IZQUIERDO {
                        self.cal.tocar(o);
                    }
                    return;
                }
                let dentro = c.hoja.as_ref().is_some_and(|h| h.dentro(x, y));
                if !dentro {
                    self.cerrar_calendario(true);
                }
            }
            _ => {}
        }
    }

    // ── La barra ──

    fn barra(&mut self, wl: &wl_surface::WlSurface) -> Option<usize> {
        self.barras.iter().position(|b| b.capa.wl_surface() == wl)
    }

    fn puntero_barra(&mut self, i: usize, e: &PointerEvent) {
        let ahora = Instant::now();
        let (x, y) = e.position;
        let b = &self.barras[i];
        let lugar = Self::lugar(b, &self.estado, ahora);
        let bajo = lugar.as_ref().and_then(|l| l.bajo(x, y));
        let en_cajon = lugar.as_ref().is_some_and(|l| {
            l.cajon
                .contains(morgan_motor::vello::kurbo::Point::new(x, y))
        });
        match e.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                self.mover(i, bajo, en_cajon);
            }
            PointerEventKind::Leave { .. } => {
                self.mover(i, None, false);
                self.cerrar_globo();
            }
            PointerEventKind::Press { .. } => {
                self.cerrar_globo();
                self.cancelar_globo();
                let b = &mut self.barras[i];
                if let Some(q) = bajo.filter(|q| !q.quieto()) {
                    b.animos.apretar(q, true, ahora);
                    b.lamina.sucio = true;
                }
                self.apretado = Some((e.surface.clone(), bajo, None));
            }
            PointerEventKind::Release { button, .. } => {
                let Some((s, Some(q), _)) = self.apretado.take() else {
                    return;
                };
                if s != e.surface {
                    return;
                }
                let b = &mut self.barras[i];
                b.animos.apretar(q, false, ahora);
                b.lamina.sucio = true;
                if bajo == Some(q) {
                    self.tocar(i, q, boton_gtk(button));
                }
            }
            PointerEventKind::Axis { vertical, .. } => {
                let Some(Que::Icono(n)) = bajo else { return };
                let pasos = if vertical.value120 != 0 {
                    f64::from(vertical.value120) / 120.0
                } else if vertical.discrete != 0 {
                    f64::from(vertical.discrete)
                } else {
                    vertical.absolute / 10.0
                };
                let delta = pasos.round() as i32;
                if delta != 0
                    && let Some(icono) = self.estado.bandeja.get(n)
                {
                    self.avisar(Evento::Desplazar {
                        clave: icono.clave.clone(),
                        delta,
                    });
                }
            }
        }
    }

    fn mover(&mut self, i: usize, bajo: Option<Que>, en_cajon: bool) {
        let b = &mut self.barras[i];
        if b.encima == bajo && b.en_cajon == en_cajon {
            return;
        }
        b.encima = bajo;
        b.en_cajon = en_cajon;
        b.lamina.sucio = true;
        let wl = b.capa.wl_surface().clone();
        self.objetivos();
        // El globo sigue a la pieza.
        if self.globo.as_ref().is_some_and(|g| Some(g.de.1) != bajo) {
            self.cerrar_globo();
        }
        self.cancelar_globo();
        let Some(q) = bajo else { return };
        if self.estado.tooltip(q).is_none() || self.globo.is_some() {
            return;
        }
        let reciente = self.sin_globo.is_some_and(|t| t.elapsed() < ESPERA_GLOBO);
        if reciente {
            self.abrir_globo(wl, q);
            return;
        }
        let timer = Timer::from_duration(ESPERA_GLOBO);
        self.espera_globo = self
            .bucle
            .insert_source(timer, move |_, _, h| {
                h.espera_globo = None;
                h.abrir_globo(wl.clone(), q);
                TimeoutAction::Drop
            })
            .ok();
    }

    fn tocar(&mut self, i: usize, q: Que, boton: u32) {
        let e = match (q, boton) {
            (Que::Espacio(id), 1) => Evento::IrA(id),
            (Que::Reloj, 1) => Evento::Pedir(Request::Reloj {
                accion: AccionSuperficie::Alternar,
            }),
            // Izquierdo en el reloj: el calendario; derecho: el centro. En la campana, al revés
            // el derecho: No molestar.
            (Que::Reloj, 3) | (Que::Campana, 1) => Evento::Pedir(Request::Centro {
                accion: AccionSuperficie::Alternar,
            }),
            (Que::Campana, 3) => Evento::Pedir(Request::NoMolestar {
                accion: AccionInterruptor::Alternar,
            }),
            (Que::Grabacion, 1) => Evento::Grabar,
            (Que::Icono(n), 1..=3) => {
                let b = &self.barras[i];
                let Some(icono) = self.estado.bandeja.get(n) else {
                    return;
                };
                let derecha = Self::lugar(b, &self.estado, Instant::now())
                    .and_then(|l| l.pieza(q).map(|p| p.contenido.x1))
                    .map_or(0, |x| (f64::from(b.lamina.tam.0) - x).round() as i32);
                Evento::Bandeja {
                    clave: icono.clave.clone(),
                    boton,
                    salida: b.nombre.clone(),
                    derecha,
                }
            }
            _ => return,
        };
        self.avisar(e);
    }

    // ── El globo ──

    fn cancelar_globo(&mut self) {
        if let Some(t) = self.espera_globo.take() {
            self.bucle.remove(t);
        }
    }

    fn cerrar_globo(&mut self) {
        if self.globo.take().is_some() {
            self.sin_globo = Some(Instant::now());
        }
    }

    /// El texto del globo abierto cambió (la cantidad de avisos, por ejemplo).
    fn refrescar_globo(&mut self) {
        let Some(g) = &self.globo else { return };
        let (wl, q) = g.de.clone();
        self.globo = None;
        if self.estado.tooltip(q).is_some() {
            self.abrir_globo(wl, q);
        }
    }

    fn abrir_globo(&mut self, wl: wl_surface::WlSurface, q: Que) {
        let Some(i) = self.barra(&wl) else { return };
        let Some(texto) = self.estado.tooltip(q) else {
            return;
        };
        let Some(xdg) = &self.xdg else { return };
        let b = &self.barras[i];
        let Some(caja) =
            Self::lugar(b, &self.estado, Instant::now()).and_then(|l| l.pieza(q).map(|p| p.caja))
        else {
            return;
        };
        let escala = b.lamina.escala as f32;
        let l = dibujo::armar_globo(&mut self.textos, &texto, escala);
        let (gw, gh) = dibujo::globo_caja(&l, escala);
        let tam = dibujo::globo_tam(&l, escala);
        let Ok(pos) = XdgPositioner::new(xdg) else {
            return;
        };
        pos.set_size(gw as i32, gh as i32);
        pos.set_anchor_rect(
            caja.x0.round() as i32,
            caja.y0.round() as i32,
            caja.width().round().max(1.0) as i32,
            caja.height().round().max(1.0) as i32,
        );
        pos.set_anchor(Ancla::Bottom);
        pos.set_gravity(Gravity::Bottom);
        pos.set_constraint_adjustment(ConstraintAdjustment::SlideX | ConstraintAdjustment::FlipY);
        let superficie = self.compositor.create_surface(&self.qh);
        let Ok(popup) = Popup::from_surface(None, &pos, &self.qh, superficie, xdg) else {
            return;
        };
        b.capa.get_popup(popup.xdg_popup());
        let [izq, arriba, ..] = dibujo::GLOBO_SOMBRA;
        popup
            .xdg_surface()
            .set_window_geometry(izq as i32, arriba as i32, gw as i32, gh as i32);
        if let Ok(r) = Region::new(&self.compositor) {
            popup.wl_surface().set_input_region(Some(r.wl_region()));
        }
        popup.wl_surface().commit();
        let mut lamina = Lamina::new(tam);
        lamina.escala = b.lamina.escala;
        self.globo = Some(Globo {
            lamina,
            popup,
            texto: l,
            de: (wl, q),
        });
    }
}

const BTN_IZQUIERDO: u32 = 0x110;

/// `xdg_wm_base` solo para los globos: el `XdgShell` de sctk pide además manejar ventanas.
struct WmBase(XdgWmBase);

impl ProvidesBoundGlobal<XdgWmBase, 5> for WmBase {
    fn bound_global(&self) -> Result<XdgWmBase, GlobalError> {
        Ok(self.0.clone())
    }
}

impl ProvidesBoundGlobal<XdgWmBase, 6> for WmBase {
    fn bound_global(&self) -> Result<XdgWmBase, GlobalError> {
        Ok(self.0.clone())
    }
}

/// Los botones de Linux como los cuenta GTK: 1 el izquierdo, 2 el del medio, 3 el derecho.
fn boton_gtk(b: u32) -> u32 {
    match b {
        0x110 => 1,
        0x112 => 2,
        0x111 => 3,
        _ => 0,
    }
}

impl LayerShellHandler for Hilo {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, capa: &LayerSurface) {
        self.barras
            .retain(|b| b.capa.wl_surface() != capa.wl_surface());
        if self
            .calendario
            .as_ref()
            .is_some_and(|c| c.capa.wl_surface() == capa.wl_surface())
        {
            self.cerrar_calendario(true);
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
        let lamina = if let Some(b) = self.barras.iter_mut().find(|b| b.capa.wl_surface() == wl) {
            &mut b.lamina
        } else if let Some(k) = self
            .calendario
            .as_mut()
            .filter(|k| k.capa.wl_surface() == wl)
        {
            &mut k.lamina
        } else {
            return;
        };
        if c.new_size.0 > 0 && c.new_size.1 > 0 {
            lamina.tam = c.new_size;
        }
        if lamina.lienzo.is_none() {
            // SAFETY: el lienzo se suelta antes que la capa (orden de los campos).
            lamina.lienzo = unsafe { Lienzo::nuevo(&mut self.gpu, &self.conn, wl) };
        }
        lamina.sucio = true;
        self.objetivos();
    }
}

impl PopupHandler for Hilo {
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        popup: &Popup,
        _: PopupConfigure,
    ) {
        let Some(g) = self.globo.as_mut().filter(|g| &g.popup == popup) else {
            return;
        };
        if g.lamina.lienzo.is_none() {
            // SAFETY: el lienzo se suelta antes que el popup (orden de los campos de Globo).
            g.lamina.lienzo =
                unsafe { Lienzo::nuevo(&mut self.gpu, &self.conn, popup.wl_surface()) };
        }
        g.lamina.sucio = true;
    }

    fn done(&mut self, _: &Connection, _: &QueueHandle<Self>, popup: &Popup) {
        if self.globo.as_ref().is_some_and(|g| &g.popup == popup) {
            self.cerrar_globo();
        }
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
        if let Some(l) = self.lamina(surface) {
            l.escala = factor.max(1);
            l.sucio = true;
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
        if let Some(l) = self.lamina(surface) {
            l.esperando = false;
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

impl Hilo {
    fn lamina(&mut self, wl: &wl_surface::WlSurface) -> Option<&mut Lamina> {
        if let Some(b) = self.barras.iter_mut().find(|b| b.capa.wl_surface() == wl) {
            return Some(&mut b.lamina);
        }
        if let Some(c) = self
            .calendario
            .as_mut()
            .filter(|c| c.capa.wl_surface() == wl)
        {
            return Some(&mut c.lamina);
        }
        self.globo
            .as_mut()
            .filter(|g| g.popup.wl_surface() == wl)
            .map(|g| &mut g.lamina)
    }
}

impl OutputHandler for Hilo {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.salidas
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, salida: wl_output::WlOutput) {
        self.cubrir(salida);
    }

    /// El nombre puede llegar después del alta (xdg-output).
    fn update_output(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        salida: wl_output::WlOutput,
    ) {
        let nombre = self.salidas.info(&salida).and_then(|i| i.name);
        if let Some(b) = self.barras.iter_mut().find(|b| b.salida == salida)
            && let Some(n) = nombre
            && n != b.nombre
        {
            b.nombre = n;
            b.rotulos = None;
            b.lamina.sucio = true;
        }
    }

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        salida: wl_output::WlOutput,
    ) {
        if let Some(g) = &self.globo
            && self
                .barras
                .iter()
                .any(|b| b.salida == salida && b.capa.wl_surface() == &g.de.0)
        {
            self.globo = None;
        }
        self.barras.retain(|b| b.salida != salida);
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
        if c == Capability::Keyboard && self.teclado.is_none() {
            match self.asientos.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                self.bucle.clone(),
                Box::new(|h, _, ev| h.tecla_calendario(ev)),
            ) {
                Ok(k) => self.teclado = Some(k),
                Err(e) => eprintln!("morgan: barra: teclado: {e}"),
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
        if c == Capability::Keyboard
            && let Some(k) = self.teclado.take()
        {
            k.release();
        }
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

impl KeyboardHandler for Hilo {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        ev: KeyEvent,
    ) {
        self.tecla_calendario(ev);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        ev: KeyEvent,
    ) {
        self.tecla_calendario(ev);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
    }
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
            if let Some(i) = self.barra(&e.surface) {
                self.puntero_barra(i, e);
            } else if self
                .calendario
                .as_ref()
                .is_some_and(|c| c.capa.wl_surface() == &e.surface)
            {
                self.puntero_calendario(e);
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
