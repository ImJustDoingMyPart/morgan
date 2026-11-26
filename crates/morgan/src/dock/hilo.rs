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
use super::modelo::{
    ALTO_CAPA, Disposicion, Pieza, Seccion, ZONA, corrida, destino, disponer, entran, tope, tramo,
};
use crate::deslizable::ARRANQUE;
use crate::lamina::Lamina;

const BTN_IZQUIERDO: u32 = 0x110;
const BTN_DERECHO: u32 = 0x111;
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
    },
    /// Se cerró el menú número tal (o no llegó a abrirse).
    MenuCerrado(u32),
}

/// Lo que se tocó, para el hilo de GTK.
pub enum Evento {
    Click(String),
    /// El click derecho en una app: su menú, encima de su celda.
    Menu {
        clave: String,
        salida: String,
        /// El centro de la celda, desde la izquierda de la salida.
        centro: i32,
        /// Del borde de abajo de lo que el menú puede ocupar al de abajo del menú.
        abajo: i32,
        /// Para avisar con `Orden::MenuCerrado` cuál se cerró.
        n: u32,
    },
    Enfocar(String),
    /// El click medio: le pide a la ventana que se cierre.
    Cerrar(String),
    /// Se abrió o se cerró el globo: mientras está, los títulos importan.
    Globo(bool),
    /// Se soltó una app arrastrada: las claves que se ven, en su orden nuevo.
    Ordenar(Vec<String>),
}

pub struct Arranque {
    pub paleta: Paleta,
    pub opacidad: f32,
    pub borde: Borde,
    pub mostrar: bool,
    pub ocultar: bool,
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
    /// Cuánto se corre cada celda: mientras se arrastra una, y al acomodarse después de soltar.
    corridas: Vec<Transicion>,
    arrastre: Option<Arrastre>,
    /// Las apps de `visibles`, para saber si cambiaron aunque cambien los índices.
    claves: Vec<String>,
}

/// Una app apretada con el botón izquierdo; pasado `ARRANQUE` se mueve con el puntero.
#[derive(Clone, Copy)]
struct Arrastre {
    de: usize,
    x0: f64,
    dx: f64,
    anda: bool,
    a: usize,
}

struct Globo {
    lamina: Lamina,
    capa: LayerSurface,
    clave: String,
    rotulos: Rotulos,
    encima: Option<usize>,
    tintes: Vec<Transicion>,
}

impl Globo {
    fn poner_encima(&mut self, fila: Option<usize>) {
        if fila == self.encima {
            return;
        }
        let ahora = Instant::now();
        if let Some(t) = self.encima.and_then(|i| self.tintes.get_mut(i)) {
            t.ir(0.0, TINTE, ahora);
        }
        if let Some(t) = fila.and_then(|i| self.tintes.get_mut(i)) {
            t.ir(1.0, TINTE, ahora);
        }
        self.encima = fila;
        self.lamina.sucio = true;
    }
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
    ventanas: Vec<Ventana>,
    globo: Option<Globo>,
    globo_cerrado: Option<Instant>,
    /// El menú del click derecho abierto: mientras está, el dock no se esconde.
    menu: Option<u32>,
    menus: u32,
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
        ventanas: Vec::new(),
        globo: None,
        globo_cerrado: None,
        menu: None,
        menus: 0,
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
            Orden::Config { mostrar, ocultar } => {
                let cambia = ocultar != self.ocultar;
                self.mostrar = mostrar;
                self.ocultar = ocultar;
                // La zona reservada se fija al crear la capa.
                if cambia {
                    self.quitar_capas();
                }
                self.sincronizar();
            }
            Orden::MenuCerrado(n) => {
                if self.menu == Some(n) {
                    self.menu = None;
                    self.programar_ocultar();
                }
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
        if self.globo.take().is_some() {
            let _ = self.eventos.send_blocking(Evento::Globo(false));
        }
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
            corridas: Vec::new(),
            arrastre: None,
            claves: Vec::new(),
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
                    let r = Rotulos::armar(&mut self.textos, &p, escala);
                    let tam = r.tam;
                    if let Some(g) = &mut self.globo {
                        if tam != g.rotulos.tam {
                            g.capa.set_size(tam.0, tam.1);
                            g.capa.commit();
                            g.lamina.tam = tam;
                        }
                        g.tintes.resize(r.cuantas(), Transicion::new(0.0));
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
        let claves: Vec<String> = v
            .visibles
            .iter()
            .map(|&p| piezas[p].clave.clone())
            .collect();
        // Otras apps u otro orden que el que se soltó: lo corrido ya no vale.
        if claves != v.claves {
            v.corridas.clear();
            v.arrastre = None;
            v.claves = claves;
        }
        let bandeja: Vec<bool> = v
            .visibles
            .iter()
            .map(|&p| piezas[p].seccion == Seccion::Bandeja)
            .collect();
        v.disposicion = disponer(ancho, &bandeja);
        v.tintes.resize(v.visibles.len(), Transicion::new(0.0));
        v.corridas.resize(v.visibles.len(), Transicion::new(0.0));
        v.encima = v.encima.filter(|&e| e < v.visibles.len());
        v.lamina.sucio = true;
        Self::poner_region(&self.compositor, v);
    }

    fn poner_region(compositor: &CompositorState, v: &Ventana) {
        let Ok(r) = Region::new(compositor) else {
            return;
        };
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
        let Some(v) = self.ventanas.get(i) else {
            return;
        };
        let (Some(&p), Some(c)) = (v.visibles.get(celda), v.disposicion.celdas.get(celda)) else {
            return;
        };
        let Some(pieza) = self.datos.piezas.get(p) else {
            return;
        };
        let escala = v.lamina.escala.max(1);
        let rotulos = Rotulos::armar(&mut self.textos, pieza, escala as f32);
        let tam = rotulos.tam;
        let tintes = vec![Transicion::new(0.0); rotulos.cuantas()];
        let ancho_salida = i32::try_from(v.lamina.tam.0).unwrap_or(i32::MAX);
        let izquierda =
            (c.center().x as i32 - tam.0 as i32 / 2).clamp(0, (ancho_salida - tam.0 as i32).max(0));
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
        if self.globo.is_none() {
            let _ = self.eventos.send_blocking(Evento::Globo(true));
        }
        // El anterior se va recién acá: reemplazarlo no deja un cuadro sin globo.
        self.globo = Some(Globo {
            lamina,
            capa,
            clave,
            rotulos,
            encima: None,
            tintes,
        });
        let _ = self.conn.flush();
    }

    fn cerrar_globo(&mut self) {
        if self.globo.take().is_some() {
            self.globo_cerrado = Some(Instant::now());
            let _ = self.eventos.send_blocking(Evento::Globo(false));
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
            .insert_source(
                Timer::from_duration(ESPERA_OCULTAR),
                |_, _, h: &mut Hilo| {
                    h.temporizador_ocultar = None;
                    h.ocultar_ahora();
                    TimeoutAction::Drop
                },
            )
            .ok();
    }

    fn ocultar_ahora(&mut self) {
        if self.globo.is_some() || self.menu.is_some() {
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
            let anda = v.subida.anda(ahora)
                || v.tintes.iter().any(|t| t.anda(ahora))
                || v.corridas.iter().any(|t| t.anda(ahora));
            if !(v.lamina.sucio || anda) || !v.lamina.lista() {
                continue;
            }
            let tintes: Vec<f32> = v.tintes.iter().map(|t| t.valor(ahora)).collect();
            let mut corridas: Vec<f64> = v
                .corridas
                .iter()
                .map(|t| f64::from(t.valor(ahora)))
                .collect();
            let arriba = v.arrastre.filter(|a| a.anda).map(|a| {
                if let Some(c) = corridas.get_mut(a.de) {
                    *c = a.dx;
                }
                a.de
            });
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
                &corridas,
                arriba,
                v.subida.valor(ahora),
                paleta,
                *opacidad,
                *borde,
                v.lamina.tam,
                f64::from(v.lamina.escala),
            );
            v.lamina.presentar(gpu, escena, v.capa.wl_surface(), qh);
            // Lo que sigue andando pide el próximo cuadro.
            v.lamina.sucio = v.subida.anda(ahora)
                || v.tintes.iter().any(|t| t.anda(ahora))
                || v.corridas.iter().any(|t| t.anda(ahora));
        }
        if let Some(g) = globo
            && g.lamina.sucio
            && g.lamina.lista()
        {
            let tintes: Vec<f32> = g.tintes.iter().map(|t| t.valor(ahora)).collect();
            dibujo::pintar_globo(
                escena,
                &g.rotulos,
                &tintes,
                paleta,
                *opacidad,
                f64::from(g.lamina.escala),
            );
            g.lamina.presentar(gpu, escena, g.capa.wl_surface(), qh);
            g.lamina.sucio = g.tintes.iter().any(|t| t.anda(ahora));
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
                if self.ventanas[i].arrastre.is_some() {
                    self.arrastrar(i, x);
                    return;
                }
                let celda = self.ventanas[i].disposicion.celda_en(x, y);
                self.poner_encima(i, celda);
            }
            PointerEventKind::Leave { .. } => {
                // Con el botón apretado el puntero sigue acá; si igual se va, la app vuelve.
                self.soltar(i, false);
                self.ventanas[i].adentro = false;
                self.poner_encima(i, None);
                self.programar_ocultar();
            }
            // El menú sale al apretar, como el de cualquier app: esperar a soltar se nota.
            PointerEventKind::Press { button, .. } if button == BTN_DERECHO => self.menu(i),
            PointerEventKind::Press { button, .. } if button == BTN_IZQUIERDO => {
                let v = &mut self.ventanas[i];
                v.arrastre = v.encima.map(|de| Arrastre {
                    de,
                    x0: x,
                    dx: 0.0,
                    anda: false,
                    a: de,
                });
            }
            PointerEventKind::Release { button, .. }
                if button == BTN_IZQUIERDO && self.soltar(i, true) => {}
            PointerEventKind::Release { button, .. } => {
                let v = &self.ventanas[i];
                let Some(celda) = v.encima else { return };
                let Some(p) = v
                    .visibles
                    .get(celda)
                    .and_then(|&p| self.datos.piezas.get(p))
                else {
                    return;
                };
                let evento = match button {
                    BTN_IZQUIERDO => Evento::Click(p.clave.clone()),
                    BTN_DERECHO => return,
                    // Una ventana: se cierra. Varias: el globo, ya, para elegir cuál.
                    BTN_MEDIO => match p.filas.as_slice() {
                        [una] => Evento::Cerrar(una.direccion.clone()),
                        [] => return,
                        _ => {
                            self.sin_temporizador();
                            self.abrir_globo(i, celda);
                            return;
                        }
                    },
                    _ => return,
                };
                let _ = self.eventos.send_blocking(evento);
                self.sin_temporizador();
                self.cerrar_globo();
            }
            _ => {}
        }
    }

    /// La app apretada sigue al puntero, sin salir de su tramo; las otras le hacen lugar.
    fn arrastrar(&mut self, i: usize, x: f64) {
        let ahora = Instant::now();
        let v = &mut self.ventanas[i];
        let Some(mut a) = v.arrastre else { return };
        if !a.anda && (x - a.x0).abs() < ARRANQUE {
            return;
        }
        let bandeja: Vec<bool> = v
            .visibles
            .iter()
            .map(|&p| {
                self.datos
                    .piezas
                    .get(p)
                    .is_some_and(|p| p.seccion == Seccion::Bandeja)
            })
            .collect();
        let t = tramo(&bandeja, a.de);
        let celdas = &v.disposicion.celdas;
        let (Some(primera), Some(ultima), Some(propia)) = (
            celdas.get(*t.start()),
            celdas.get(*t.end()),
            celdas.get(a.de),
        ) else {
            return;
        };
        let empezo = !a.anda;
        a.anda = true;
        a.dx = (x - a.x0).clamp(primera.x0 - propia.x0, ultima.x0 - propia.x0);
        let nuevo = destino(celdas, &t, a.de, a.dx);
        if empezo || nuevo != a.a {
            a.a = nuevo;
            for j in t.clone() {
                if j == a.de {
                    continue;
                }
                let k = corrida(j, a.de, a.a);
                let dx = celdas
                    .get(k)
                    .zip(celdas.get(j))
                    .map_or(0.0, |(k, j)| k.x0 - j.x0);
                if let Some(c) = v.corridas.get_mut(j) {
                    c.ir(dx as f32, DESLIZAR, ahora);
                }
            }
        }
        v.arrastre = Some(a);
        v.lamina.sucio = true;
        if empezo {
            self.sin_temporizador();
            self.cerrar_globo();
        }
    }

    /// Suelta lo apretado. Si se estaba arrastrando, cada app se acomoda en su celda nueva
    /// desde donde se veía y, con `vale`, el orden nuevo va a guardarse. Devuelve si había un
    /// arrastre en marcha (entonces no es un click).
    fn soltar(&mut self, i: usize, vale: bool) -> bool {
        let ahora = Instant::now();
        let v = &mut self.ventanas[i];
        let Some(a) = v.arrastre.take() else {
            return false;
        };
        if !a.anda {
            return false;
        }
        v.lamina.sucio = true;
        if !vale || a.a == a.de {
            if let Some(c) = v.corridas.get_mut(a.de) {
                *c = Transicion::new(a.dx as f32);
            }
            for c in &mut v.corridas {
                c.ir(0.0, DESLIZAR, ahora);
            }
            return true;
        }
        // Cada una arranca de donde se veía, ya en su celda nueva.
        let celdas = &v.disposicion.celdas;
        let x = |j: usize| {
            let corrida = if j == a.de {
                a.dx
            } else {
                v.corridas.get(j).map_or(0.0, |c| f64::from(c.valor(ahora)))
            };
            celdas.get(j).map_or(0.0, |c| c.x0) + corrida
        };
        let mut orden: Vec<usize> = (0..v.visibles.len()).collect();
        let movida = orden.remove(a.de);
        orden.insert(a.a, movida);
        let corridas: Vec<Transicion> = orden
            .iter()
            .enumerate()
            .map(|(k, &j)| {
                let desde = x(j) - celdas.get(k).map_or(0.0, |c| c.x0);
                let mut t = Transicion::new(desde as f32);
                t.ir(0.0, DESLIZAR, ahora);
                t
            })
            .collect();
        v.corridas = corridas;
        v.visibles = orden.iter().map(|&j| v.visibles[j]).collect();
        if a.de < v.tintes.len() && a.a < v.tintes.len() {
            let tinte = v.tintes.remove(a.de);
            v.tintes.insert(a.a, tinte);
        }
        v.encima = v.encima.map(|_| a.a);
        v.claves = v
            .visibles
            .iter()
            .filter_map(|&p| Some(self.datos.piezas.get(p)?.clave.clone()))
            .collect();
        let _ = self
            .eventos
            .send_blocking(Evento::Ordenar(v.claves.clone()));
        true
    }

    fn menu(&mut self, i: usize) {
        let v = &self.ventanas[i];
        let Some(celda) = v.encima else { return };
        let Some(p) = v
            .visibles
            .get(celda)
            .and_then(|&p| self.datos.piezas.get(p))
        else {
            return;
        };
        let salida = self
            .salidas
            .info(&v.salida)
            .and_then(|i| i.name)
            .unwrap_or_default();
        let centro = v
            .disposicion
            .celdas
            .get(celda)
            .map_or(0, |c| c.center().x.round() as i32);
        // El menú ocupa lo que dejan las zonas reservadas: si el dock reserva la suya, el borde
        // de abajo ya queda encima de la isla.
        let isla = if self.ocultar { ZONA as i32 } else { 0 };
        self.menus = self.menus.wrapping_add(1);
        self.menu = Some(self.menus);
        let _ = self.eventos.send_blocking(Evento::Menu {
            clave: p.clave.clone(),
            salida,
            centro,
            abajo: isla + ENCIMA_DE_LA_ISLA,
            n: self.menus,
        });
        self.sin_temporizador();
        self.cerrar_globo();
    }

    fn puntero_globo(&mut self, e: &PointerEvent) {
        let (x, y) = e.position;
        match e.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                self.sin_temporizador();
                if let Some(g) = &mut self.globo {
                    let fila = g.rotulos.fila_en(x, y);
                    g.poner_encima(fila);
                }
            }
            PointerEventKind::Leave { .. } => {
                if let Some(g) = &mut self.globo {
                    g.poner_encima(None);
                }
                self.temporizar(CERRAR_GLOBO, Pendiente::Cerrar);
            }
            PointerEventKind::Release { button, .. } => {
                let direccion = self.globo.as_ref().and_then(|g| {
                    let fila = g.encima?;
                    let p = self.datos.piezas.iter().find(|p| p.clave == g.clave)?;
                    Some(p.filas.get(fila)?.direccion.clone())
                });
                let Some(d) = direccion else { return };
                match button {
                    BTN_IZQUIERDO => {
                        let _ = self.eventos.send_blocking(Evento::Enfocar(d));
                        self.sin_temporizador();
                        self.cerrar_globo();
                    }
                    // El globo se queda: la fila se va cuando la ventana se cierra.
                    BTN_MEDIO => {
                        let _ = self.eventos.send_blocking(Evento::Cerrar(d));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

impl LayerShellHandler for Hilo {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, capa: &LayerSurface) {
        let wl = capa.wl_surface();
        if self
            .globo
            .as_ref()
            .is_some_and(|g| g.capa.wl_surface() == wl)
        {
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
        if let Some(v) = self
            .ventanas
            .iter_mut()
            .find(|v| v.capa.wl_surface() == surface)
        {
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
        if let Some(v) = self
            .ventanas
            .iter_mut()
            .find(|v| v.capa.wl_surface() == surface)
        {
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
