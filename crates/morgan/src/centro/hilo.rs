//! El hilo del centro: su propia conexión de Wayland, su capa y la GPU. El hilo de GTK le manda
//! lo que se ve (`vista.rs`) y recibe lo que se tocó; si se traba, el centro abierto sigue
//! respondiendo al puntero y sus animaciones siguen andando.
//!
//! La capa existe mientras el centro está abierto: se crea al abrir (con la GPU ya abierta, la
//! primera apertura cuesta lo mismo que las siguientes) y se destruye al cerrar. Cubre la
//! salida enfocada, para cerrar con un click afuera; lo que se ve baja desde su borde de abajo
//! (`bajada.rs`).
//!
//! El ecualizador del reproductor es cava (`ecualizador.rs`), que corre solo con el centro
//! abierto y algo sonando; un hilo lee sus renglones y se los pasa a este.

use std::io::BufRead;
use std::time::{Duration, Instant};

use morgan_motor::color::Paleta;
use morgan_motor::forma::Borde;
use morgan_motor::gpu::{Gpu, Lienzo};
use morgan_motor::vello::Scene;
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle, RegistrationToken};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
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
use smithay_client_toolkit::{delegate_dispatch2, delegate_registry, registry_handlers};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface};
use wayland_client::{Connection, QueueHandle};

use super::dibujo::menu::{MenuUi, Salida};
use super::dibujo::toque::{IZQUIERDO, Pedido};
use super::dibujo::{Objetivo, Ui};
use super::ecualizador;
use super::vista::{
    Contenido, Evento, Grupo, Iconos, ListaId, Menu, Nivel, Reproductor, Tarjeta,
};
use crate::lamina::Lamina;

pub enum Orden {
    /// Los colores de `theme.css`, la opacidad del fondo y lo de afuera de los paneles.
    Tema(Paleta, f32, Borde),
    /// Con `sesion`, abre con la lista de sesión desplegada y el foco en Bloquear.
    Abrir {
        sesion: bool,
    },
    Cerrar,
    Tarjetas(Vec<Tarjeta>),
    Lista(ListaId, Contenido),
    Niveles(Vec<Nivel>),
    Reproductor(Option<Reproductor>),
    Iconos(Iconos),
    Avisos(Vec<Grupo>),
    /// El foco en una opción (la sesión: «Volver» al confirmar, Bloquear al volver).
    Enfocar(ListaId, usize),
    /// Limpiar las notificaciones con la salida escalonada, como el botón.
    Limpiar,
    /// El menú de una app de la bandeja, o cerrarlo.
    Menu(Option<Menu>),
}

/// Si no hay Wayland o la GPU no abre, el hilo termina: morgan sigue sin centro.
pub fn lanzar(
    paleta: Paleta,
    opacidad: f32,
    borde: Borde,
) -> (Sender<Orden>, async_channel::Receiver<Evento>) {
    let (tx, rx) = channel::channel();
    let (etx, erx) = async_channel::unbounded();
    let lanzado = std::thread::Builder::new()
        .name("morgan-centro".into())
        .spawn(move || {
            if let Err(e) = correr(rx, etx, paleta, opacidad, borde) {
                eprintln!("morgan: centro: {e}");
            }
        });
    if let Err(e) = lanzado {
        eprintln!("morgan: centro: {e}");
    }
    (tx, erx)
}

struct Panel {
    lamina: Lamina,
    capa: LayerSurface,
    /// El primer cuadro: desde ahí baja.
    inicio: Option<Instant>,
}

/// El menú de la bandeja abierto: su capa, en la salida del ícono.
struct MenuSup {
    lamina: Lamina,
    capa: LayerSurface,
    inicio: Option<Instant>,
    ui: MenuUi,
}

/// cava corriendo, con el hilo que lee sus renglones.
struct Cava {
    proceso: std::process::Child,
}

impl Drop for Cava {
    fn drop(&mut self) {
        let _ = self.proceso.kill();
        let _ = self.proceso.wait();
    }
}

struct Hilo {
    conn: Connection,
    qh: QueueHandle<Hilo>,
    bucle: LoopHandle<'static, Hilo>,
    registro: RegistryState,
    compositor: CompositorState,
    capas: LayerShell,
    salidas: OutputState,
    asientos: SeatState,
    cursor: Option<CursorShapeManager>,
    puntero: Option<wl_pointer::WlPointer>,
    forma: Option<WpCursorShapeDeviceV1>,
    /// El serial del último `enter` del puntero, para cambiar su forma.
    serial_puntero: u32,
    forma_puesta: Option<Shape>,
    teclado: Option<wl_keyboard::WlKeyboard>,
    mods: Modifiers,
    gpu: Gpu,
    escena: Scene,
    ui: Ui,
    panel: Option<Panel>,
    menu: Option<MenuSup>,
    /// Abrir con la sesión desplegada espera a la primera hoja para enfocar.
    enfocar_al_armar: Option<Enfoque>,
    cava: Option<Cava>,
    /// El reloj de las horas relativas (cada minuto) y el del cursor de un campo.
    reloj: Option<RegistrationToken>,
    parpadeo: Option<(Instant, RegistrationToken)>,
    eventos: async_channel::Sender<Evento>,
}

#[derive(Clone, Copy)]
enum Enfoque {
    Lista,
    Opcion(ListaId, usize),
}

fn correr(
    rx: channel::Channel<Orden>,
    eventos: async_channel::Sender<Evento>,
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
        asientos: SeatState::new(&globales, &qh),
        cursor: CursorShapeManager::bind(&globales, &qh).ok(),
        puntero: None,
        forma: None,
        serial_puntero: 0,
        forma_puesta: None,
        teclado: None,
        mods: Modifiers::default(),
        escena: Scene::new(),
        ui: Ui::new(paleta, opacidad, borde),
        panel: None,
        menu: None,
        enfocar_al_armar: None,
        cava: None,
        reloj: None,
        parpadeo: None,
        eventos,
    };
    cola.roundtrip(&mut h).map_err(|e| e.to_string())?;
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

    fn ensuciar(&mut self) {
        if let Some(p) = &mut self.panel {
            p.lamina.sucio = true;
        }
        if let Some(m) = &mut self.menu {
            m.lamina.sucio = true;
        }
    }

    fn orden(&mut self, o: Orden) {
        let ahora = Instant::now();
        match o {
            Orden::Tema(p, o, b) => {
                (self.ui.paleta, self.ui.opacidad, self.ui.borde) = (p, o, b);
                if let Some(m) = &mut self.menu {
                    (m.ui.paleta, m.ui.opacidad, m.ui.borde) = (p, o, b);
                }
            }
            Orden::Abrir { sesion } => self.abrir(sesion),
            Orden::Cerrar => self.cerrar(false),
            Orden::Tarjetas(t) => self.ui.datos.tarjetas = t,
            Orden::Lista(l, c) => {
                // Un campo nuevo toma el foco.
                let nuevo = campo_de(&c).filter(|g| {
                    campo_de_lista(&self.ui, l) != Some(*g)
                });
                self.ui.datos.listas.insert(l, c);
                if let Some(g) = nuevo
                    && self.panel.is_some()
                {
                    self.ui.enfocar_campo(l, g);
                }
            }
            Orden::Niveles(n) => self.ui.datos.niveles = n,
            Orden::Reproductor(r) => {
                let titulo = r.as_ref().map(|r| r.titulo.clone());
                let antes = self.ui.datos.reproductor.as_ref().map(|r| r.titulo.clone());
                if titulo != antes {
                    self.ui.marquesina = ahora;
                }
                self.ui.datos.reproductor = r;
                self.ecualizador();
            }
            Orden::Iconos(i) => self.ui.datos.iconos = i,
            Orden::Avisos(g) => {
                self.ui.datos.grupos = g;
                self.ui.olvidar_cerrados();
            }
            Orden::Enfocar(l, i) => self.ui.enfocar_opcion(l, i),
            Orden::Limpiar => self.ui.limpiar(),
            Orden::Menu(m) => {
                self.menu = None;
                if let Some(m) = m {
                    self.abrir_menu(m);
                }
            }
        }
        self.ensuciar();
    }

    fn abrir_menu(&mut self, m: Menu) {
        let salida = self
            .salidas
            .outputs()
            .find(|o| self.salidas.info(o).and_then(|i| i.name).as_deref() == Some(&m.salida));
        let wl = self.compositor.create_surface(&self.qh);
        let capa = self.capas.create_layer_surface(
            &self.qh,
            wl,
            Layer::Top,
            Some("morgan-tray"),
            salida.as_ref(),
        );
        capa.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        capa.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        capa.commit();
        let ui = MenuUi::new(m, self.ui.paleta, self.ui.opacidad, self.ui.borde);
        self.menu = Some(MenuSup {
            lamina: Lamina::new((0, 0)),
            capa,
            inicio: None,
            ui,
        });
    }

    /// Lo que pidió el menú: se elige y se cierra, o solo se cierra.
    fn salida_menu(&mut self, s: Option<Salida>) {
        match s {
            Some(Salida::Elegido(id)) => {
                self.menu = None;
                self.avisar(Evento::Elegido(id));
                self.avisar(Evento::MenuCerrado);
            }
            Some(Salida::Cerrar) => {
                self.menu = None;
                self.avisar(Evento::MenuCerrado);
            }
            None => {}
        }
        self.ensuciar();
    }

    fn abrir(&mut self, sesion: bool) {
        let ahora = Instant::now();
        if self.panel.is_some() {
            if sesion {
                self.ui.desplegar(Some(ListaId::Sesion), ahora);
                self.ui.enfocar_opcion(ListaId::Sesion, 0);
            }
            return;
        }
        let wl = self.compositor.create_surface(&self.qh);
        let capa = self.capas.create_layer_surface(
            &self.qh,
            wl,
            Layer::Top,
            Some("morgan-center"),
            None,
        );
        capa.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        capa.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        capa.commit();
        self.panel = Some(Panel {
            lamina: Lamina::new((0, 0)),
            capa,
            inicio: None,
        });
        if sesion {
            self.ui.desplegar(Some(ListaId::Sesion), ahora);
            // Sin la animación: se abre ya desplegada.
            if let Some(r) = self.ui.revelados.get_mut(&ListaId::Sesion) {
                r.poner(true);
            }
            self.enfocar_al_armar = Some(Enfoque::Opcion(ListaId::Sesion, 0));
        } else {
            self.enfocar_al_armar = Some(Enfoque::Lista);
        }
        self.ui.marquesina = ahora;
        self.ecualizador();
        // Las horas relativas siguen andando mientras se mira.
        let timer = Timer::from_duration(Duration::from_secs(60));
        self.reloj = self
            .bucle
            .insert_source(timer, |_, _, h| {
                h.ensuciar();
                TimeoutAction::ToDuration(Duration::from_secs(60))
            })
            .ok();
    }

    /// `propio`: lo cerró el centro (Escape, un click afuera) y GTK no lo sabe.
    fn cerrar(&mut self, propio: bool) {
        if self.panel.take().is_none() {
            return;
        }
        self.ui.cerrar();
        self.enfocar_al_armar = None;
        self.cava = None;
        self.ui.ecualizador.0 = morgan_motor::curva::Transicion::new(0.0);
        self.ui.ecualizador.1.clear();
        if let Some(t) = self.reloj.take() {
            self.bucle.remove(t);
        }
        if let Some((_, t)) = self.parpadeo.take() {
            self.bucle.remove(t);
        }
        if propio {
            self.avisar(Evento::Cerrado);
        }
    }

    fn pedidos(&mut self, pedidos: Vec<Pedido>) {
        for p in pedidos {
            match p {
                Pedido::Gtk(e) => self.avisar(e),
                Pedido::Cerrar => self.cerrar(true),
                Pedido::Pegar(l) => self.pegar(l),
            }
        }
        self.ensuciar();
    }

    /// Lo que hay en el portapapeles, con `wl-paste` (sin esperar: lo trae un hilo).
    fn pegar(&mut self, l: ListaId) {
        let (tx, rx) = channel::channel::<String>();
        let leido = std::thread::Builder::new()
            .name("morgan-pegar".into())
            .spawn(move || {
                let salida = std::process::Command::new("wl-paste")
                    .args(["--no-newline", "--type", "text/plain;charset=utf-8"])
                    .stderr(std::process::Stdio::null())
                    .output();
                if let Ok(o) = salida
                    && o.status.success()
                {
                    let _ = tx.send(String::from_utf8_lossy(&o.stdout).into_owned());
                }
            });
        if leido.is_err() {
            return;
        }
        let _ = self.bucle.insert_source(rx, move |ev, _, h| {
            if let channel::Event::Msg(t) = ev {
                h.ui.pegar(l, &t);
                h.ensuciar();
            }
        });
    }

    /// cava corre con el centro abierto y algo sonando; si no, el fondo se desvanece.
    fn ecualizador(&mut self) {
        let ahora = Instant::now();
        let sonando = self.panel.is_some()
            && self
                .ui
                .datos
                .reproductor
                .as_ref()
                .is_some_and(|r| r.sonando);
        let fundido = crate::animaciones::dur(Duration::from_millis(600));
        if !sonando {
            self.cava = None;
            self.ui.ecualizador.0.ir(0.0, fundido, ahora);
            return;
        }
        self.ui.ecualizador.0.ir(1.0, fundido, ahora);
        if self.cava.is_some() {
            return;
        }
        match lanzar_cava() {
            Ok((proceso, rx)) => {
                let _ = self.bucle.insert_source(rx, |ev, _, h| {
                    if let channel::Event::Msg(n) = ev
                        && h.cava.is_some()
                    {
                        h.ui.ecualizador.1 = n;
                        h.ensuciar();
                    }
                });
                self.cava = Some(Cava { proceso });
            }
            Err(e) => eprintln!("morgan: ecualizador: {e}"),
        }
    }

    /// El cursor del campo con el foco parpadea: se despierta en cada cambio.
    fn programar_parpadeo(&mut self, ahora: Instant) {
        let proximo = self.ui.proximo_parpadeo(ahora);
        if self.parpadeo.as_ref().map(|(t, _)| *t) == proximo {
            return;
        }
        if let Some((_, t)) = self.parpadeo.take() {
            self.bucle.remove(t);
        }
        let Some(cuando) = proximo else { return };
        let timer = Timer::from_deadline(cuando);
        if let Ok(t) = self.bucle.insert_source(timer, |_, _, h| {
            h.parpadeo = None;
            h.ensuciar();
            TimeoutAction::Drop
        }) {
            self.parpadeo = Some((cuando, t));
        }
    }

    /// Después de cada vuelta del bucle: dibuja si hace falta y se puede.
    fn pintar(&mut self) {
        let ahora = Instant::now();
        let pedidos = self.ui.al_cuadro(ahora);
        if !pedidos.is_empty() {
            self.pedidos(pedidos);
        }
        self.pintar_menu(ahora);
        let Some(p) = &mut self.panel else {
            let _ = self.conn.flush();
            return;
        };
        let anda = self.ui.anda(ahora)
            || p
                .inicio
                .is_none_or(|t| ahora.saturating_duration_since(t) < crate::bajada::duracion());
        if p.lamina.lista() && (p.lamina.sucio || anda) {
            self.ui.tam = p.lamina.tam;
            let escala = p.lamina.escala;
            self.ui.armar(escala as f32, ahora);
            match self.enfocar_al_armar.take() {
                Some(Enfoque::Lista) => self.ui.enfocar_lista(),
                Some(Enfoque::Opcion(l, i)) => self.ui.enfocar_opcion(l, i),
                None => {}
            }
            let Some(p) = &mut self.panel else { return };
            let inicio = *p.inicio.get_or_insert(ahora);
            let micros = ahora.saturating_duration_since(inicio).as_micros() as i64;
            let fondo = self.ui.panel().y1 as f32;
            let bajada = crate::bajada::desplazamiento(micros, fondo);
            self.ui.pintar(&mut self.escena, f64::from(escala), f64::from(bajada));
            let wl = p.capa.wl_surface().clone();
            p.lamina.presentar(&mut self.gpu, &self.escena, &wl, &self.qh);
            self.programar_parpadeo(ahora);
        }
        let _ = self.conn.flush();
    }

    fn pintar_menu(&mut self, ahora: Instant) {
        let Some(m) = &mut self.menu else { return };
        let anda = m.ui.anda(ahora)
            || m.inicio
                .is_none_or(|t| ahora.saturating_duration_since(t) < crate::bajada::duracion());
        if !m.lamina.lista() || !(m.lamina.sucio || anda) {
            return;
        }
        m.ui.tam = m.lamina.tam;
        let escala = m.lamina.escala;
        m.ui.armar(escala as f32, ahora);
        let inicio = *m.inicio.get_or_insert(ahora);
        let micros = ahora.saturating_duration_since(inicio).as_micros() as i64;
        let bajada = crate::bajada::desplazamiento(micros, m.ui.panel().y1 as f32);
        m.ui.pintar(&mut self.escena, f64::from(escala), f64::from(bajada));
        let wl = m.capa.wl_surface().clone();
        m.lamina.presentar(&mut self.gpu, &self.escena, &wl, &self.qh);
    }

    fn puntero_menu(&mut self, e: &PointerEvent) {
        let Some(m) = &mut self.menu else { return };
        let (x, y) = e.position;
        let s = match e.kind {
            PointerEventKind::Enter { serial } => {
                self.serial_puntero = serial;
                self.forma_puesta = None;
                m.ui.mover(x, y);
                None
            }
            PointerEventKind::Motion { .. } => {
                m.ui.mover(x, y);
                None
            }
            PointerEventKind::Leave { .. } => {
                m.ui.salir();
                None
            }
            PointerEventKind::Press { .. } => {
                m.ui.apretar(x, y);
                None
            }
            PointerEventKind::Release { button, .. } => m.ui.soltar(x, y, button == IZQUIERDO),
            PointerEventKind::Axis { .. } => None,
        };
        m.lamina.sucio = true;
        if self.forma_puesta != Some(Shape::Default)
            && let Some(f) = &self.forma
        {
            f.set_shape(self.serial_puntero, Shape::Default);
            self.forma_puesta = Some(Shape::Default);
        }
        self.salida_menu(s);
    }

    fn puntero(&mut self, e: &PointerEvent) {
        let (x, y) = e.position;
        let pedidos = match e.kind {
            PointerEventKind::Enter { serial } => {
                self.serial_puntero = serial;
                self.forma_puesta = None;
                self.ui.mover_puntero(x, y, 0)
            }
            PointerEventKind::Motion { time } => self.ui.mover_puntero(x, y, time),
            PointerEventKind::Leave { .. } => {
                self.ui.salir_puntero();
                Vec::new()
            }
            PointerEventKind::Press { button, time, .. } => self.ui.apretar(x, y, button, time),
            PointerEventKind::Release { button, .. } => self.ui.soltar(x, y, button),
            PointerEventKind::Axis { vertical, .. } => {
                if vertical.value120 != 0 {
                    self.ui.rueda(f64::from(vertical.value120) / 120.0, true);
                } else if vertical.discrete != 0 {
                    self.ui.rueda(f64::from(vertical.discrete), true);
                } else if vertical.absolute != 0.0 {
                    self.ui.rueda(vertical.absolute, false);
                }
                Vec::new()
            }
        };
        self.pedidos(pedidos);
        // El cursor de texto sobre un campo.
        let forma = if self.ui.sobre_campo() {
            Shape::Text
        } else {
            Shape::Default
        };
        if self.forma_puesta != Some(forma)
            && let Some(f) = &self.forma
        {
            f.set_shape(self.serial_puntero, forma);
            self.forma_puesta = Some(forma);
        }
    }

    fn tecla(&mut self, ev: KeyEvent) {
        // El menú es lo último que se abrió: el teclado es suyo.
        if let Some(m) = &mut self.menu {
            let s = m.ui.tecla(ev.keysym, self.mods.shift);
            m.lamina.sucio = true;
            self.salida_menu(s);
            return;
        }
        if self.panel.is_none() {
            return;
        }
        let pedidos = self.ui.tecla(
            ev.keysym,
            ev.utf8.as_deref(),
            self.mods.ctrl,
            self.mods.shift,
        );
        if let Some(o) = self.ui.foco
            && matches!(o, Objetivo::Campo(_))
        {
            self.ui.mostrar(o);
        }
        self.pedidos(pedidos);
    }
}

/// El número de cada lista que pide un campo nuevo (su generación), si lleva uno.
fn campo_de(c: &Contenido) -> Option<u64> {
    match c {
        Contenido::Opciones(v) => v
            .iter()
            .find_map(|o| o.campo.as_ref().filter(|c| c.caja.is_some()).map(|c| c.generacion)),
        Contenido::Cuotas(_) => None,
    }
}

fn campo_de_lista(ui: &Ui, l: ListaId) -> Option<u64> {
    ui.datos.listas.get(&l).and_then(campo_de)
}

/// Lanza cava con su configuración y un hilo que le lee los renglones.
fn lanzar_cava() -> Result<(std::process::Child, channel::Channel<Vec<f64>>), String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .ok_or("sin XDG_RUNTIME_DIR")?
        .join("morgan");
    let ruta = dir.join("cava.conf");
    std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&ruta, ecualizador::config()))
        .map_err(|e| e.to_string())?;
    let mut proceso = std::process::Command::new("cava")
        .arg("-p")
        .arg(&ruta)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("cava: {e}"))?;
    let salida = proceso.stdout.take().ok_or("cava sin salida")?;
    let (tx, rx) = channel::channel();
    std::thread::Builder::new()
        .name("morgan-cava".into())
        .spawn(move || {
            for linea in std::io::BufReader::new(salida).lines() {
                let Ok(linea) = linea else { return };
                if let Some(n) = ecualizador::interpretar(&linea)
                    && tx.send(n).is_err()
                {
                    return;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok((proceso, rx))
}

impl LayerShellHandler for Hilo {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, capa: &LayerSurface) {
        if self
            .menu
            .as_ref()
            .is_some_and(|m| m.capa.wl_surface() == capa.wl_surface())
        {
            self.salida_menu(Some(Salida::Cerrar));
        }
        if self
            .panel
            .as_ref()
            .is_some_and(|p| p.capa.wl_surface() == capa.wl_surface())
        {
            self.cerrar(true);
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
        if let Some(m) = self.menu.as_mut().filter(|m| m.capa.wl_surface() == wl) {
            if c.new_size.0 > 0 && c.new_size.1 > 0 {
                m.lamina.tam = c.new_size;
            }
            if m.lamina.lienzo.is_none() {
                // SAFETY: el lienzo se suelta antes que la capa (orden de los campos de MenuSup).
                m.lamina.lienzo = unsafe { Lienzo::nuevo(&mut self.gpu, &self.conn, wl) };
            }
            m.lamina.sucio = true;
            return;
        }
        let Some(p) = self.panel.as_mut().filter(|p| p.capa.wl_surface() == wl) else {
            return;
        };
        if c.new_size.0 > 0 && c.new_size.1 > 0 {
            p.lamina.tam = c.new_size;
        }
        if p.lamina.lienzo.is_none() {
            // SAFETY: el lienzo se suelta antes que la capa (orden de los campos de Panel).
            p.lamina.lienzo = unsafe { Lienzo::nuevo(&mut self.gpu, &self.conn, wl) };
        }
        p.lamina.sucio = true;
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
        if let Some(p) = self.panel.as_mut().filter(|p| p.capa.wl_surface() == wl) {
            return Some(&mut p.lamina);
        }
        self.menu
            .as_mut()
            .filter(|m| m.capa.wl_surface() == wl)
            .map(|m| &mut m.lamina)
    }
}

impl OutputHandler for Hilo {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.salidas
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_output::WlOutput,
    ) {
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
                Box::new(|h, _, ev| h.tecla(ev)),
            ) {
                Ok(k) => self.teclado = Some(k),
                Err(e) => eprintln!("morgan: centro: teclado: {e}"),
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
        self.tecla(ev);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        ev: KeyEvent,
    ) {
        self.tecla(ev);
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
        m: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.mods = m;
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
            if self
                .menu
                .as_ref()
                .is_some_and(|m| m.capa.wl_surface() == &e.surface)
            {
                self.puntero_menu(e);
            } else if self
                .panel
                .as_ref()
                .is_some_and(|p| p.capa.wl_surface() == &e.surface)
            {
                self.puntero(e);
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

