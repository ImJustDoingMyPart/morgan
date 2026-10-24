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

use super::dibujo::ajustes::{self, AjustesUi};
use super::dibujo::calculadora::CalculadoraUi;
use super::dibujo::fondos::{FondosUi, Mini};
use super::dibujo::ventanas::VentanasUi;
use super::dibujo::launcher::{self, LauncherUi};
use super::dibujo::menu::{MenuUi, Salida};
use super::dibujo::toasts::{self, ToastsUi};
use super::dibujo::toque::{IZQUIERDO, Pedido};
use super::dibujo::{Objetivo, Ui};
use super::ecualizador;
use super::vista::{
    Aviso, Contenido, Evento, EventoAjustes, EventoCalculadora, EventoFondos, EventoVentanas, VistaCalculadora, VistaFondos, VistaVentanas, EventoLauncher, Grupo, Iconos, IndiceAjustes, ListaId,
    ListaLauncher, Lugar, Menu, Nivel, PaginaAjustes, Reproductor, Tarjeta,
};
use crate::lamina::Lamina;

// Llegan de a una y de vez en cuando: el tamaño de la más grande no pesa.
#[allow(clippy::large_enum_variant)]
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
    Menu(Option<Box<Menu>>),
    /// Los toasts que flotan, el más nuevo primero; sin ninguno, su capa se va.
    Toasts(Vec<Aviso>),
    /// Abre el launcher (o cambia de modo), con su prompt.
    LauncherAbrir {
        prompt: String,
        simbolos: bool,
    },
    LauncherLista(ListaLauncher),
    LauncherCerrar,
    /// El menú de sistema: abrirlo (con el buscador vacío), lo que muestra el índice y la
    /// página, adónde ir en ella, lo aplicado (y si salió) y si se graba un atajo.
    AjustesAbrir,
    AjustesCerrar,
    AjustesIndice(IndiceAjustes),
    AjustesPagina(PaginaAjustes),
    AjustesIr {
        seccion: usize,
        fila: Option<usize>,
        suave: bool,
    },
    AjustesHecho {
        lugar: Lugar,
        ok: bool,
    },
    AjustesGrabando(bool),
    /// Una página armada una vez con el menú cerrado: las fuentes de la galería se cargan
    /// antes de la primera apertura.
    AjustesPrecalentar(PaginaAjustes),
    /// La calculadora: abrirla, cerrarla y lo que muestra.
    CalculadoraAbrir,
    CalculadoraCerrar,
    Calculadora(VistaCalculadora),
    /// La banda de fondos: abrirla (con las imágenes de la carpeta) y cerrarla.
    FondosAbrir(Box<VistaFondos>),
    FondosCerrar,
    /// El lienzo de ventanas: el estado se arma al abrir; la capa se pone al mostrarse (un
    /// toque no la dibuja).
    VentanasAbrir(Box<VistaVentanas>),
    VentanasMostrar,
    /// Un paso más en orden de uso reciente (y mostrarlo).
    VentanasReciente,
    /// Enfocar la elegida (se soltó `Mod` antes de mostrarse).
    VentanasConfirmar,
    VentanasFoto(String, morgan_motor::vello::peniko::ImageData),
    VentanasFondo(morgan_motor::vello::peniko::ImageData),
    VentanasCerrar,
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

/// La columna de los toasts, mientras hay alguno: en la salida enfocada, arriba de todo.
struct ToastsSup {
    lamina: Lamina,
    capa: LayerSurface,
    inicio: Option<Instant>,
}

/// El launcher abierto: cubre la salida enfocada, para cerrar con un click afuera.
struct LauncherSup {
    lamina: Lamina,
    capa: LayerSurface,
    inicio: Option<Instant>,
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
    /// Para pasar una tecla a la que es sin modificadores: el mapa y la distribución en uso.
    keymap: Option<xkbcommon::xkb::Keymap>,
    layout: u32,
    gpu: Gpu,
    escena: Scene,
    ui: Ui,
    panel: Option<Panel>,
    menu: Option<MenuSup>,
    toasts: Option<ToastsSup>,
    toasts_ui: ToastsUi,
    launcher: Option<LauncherSup>,
    launcher_ui: LauncherUi,
    ajustes: Option<LauncherSup>,
    ajustes_ui: AjustesUi,
    calculadora: Option<LauncherSup>,
    calculadora_ui: CalculadoraUi,
    fondos: Option<LauncherSup>,
    fondos_ui: FondosUi,
    ventanas: Option<LauncherSup>,
    ventanas_ui: VentanasUi,
    ventanas_salida: Option<String>,
    /// Por donde llegan las miniaturas que lee el hilo de la banda.
    minis: Sender<Mini>,
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
    let (minis, rx_minis) = channel::channel::<Mini>();
    bucle
        .handle()
        .insert_source(rx_minis, |ev, _, h: &mut Hilo| {
            if let channel::Event::Msg(m) = ev {
                h.mini(m);
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
        keymap: None,
        layout: 0,
        escena: Scene::new(),
        ui: Ui::new(paleta, opacidad, borde),
        panel: None,
        menu: None,
        toasts: None,
        toasts_ui: ToastsUi::new(paleta, opacidad, borde),
        launcher: None,
        launcher_ui: LauncherUi::new(paleta, opacidad, borde),
        ajustes: None,
        ajustes_ui: AjustesUi::new(paleta, opacidad, borde),
        calculadora: None,
        calculadora_ui: CalculadoraUi::new(paleta, opacidad, borde),
        fondos: None,
        fondos_ui: FondosUi::new(paleta, opacidad, borde),
        ventanas: None,
        ventanas_ui: VentanasUi::new(paleta, opacidad, borde),
        ventanas_salida: None,
        minis,
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
        if let Some(t) = &mut self.toasts {
            t.lamina.sucio = true;
        }
        if let Some(l) = &mut self.launcher {
            l.lamina.sucio = true;
        }
        if let Some(a) = &mut self.ajustes {
            a.lamina.sucio = true;
        }
        if let Some(c) = &mut self.calculadora {
            c.lamina.sucio = true;
        }
        if let Some(f) = &mut self.fondos {
            f.lamina.sucio = true;
        }
        if let Some(v) = &mut self.ventanas {
            v.lamina.sucio = true;
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
                let t = &mut self.toasts_ui;
                (t.paleta, t.opacidad, t.borde) = (p, o, b);
                let l = &mut self.launcher_ui;
                (l.paleta, l.opacidad, l.borde) = (p, o, b);
                let a = &mut self.ajustes_ui;
                (a.paleta, a.opacidad, a.borde) = (p, o, b);
                let c = &mut self.calculadora_ui;
                (c.paleta, c.opacidad, c.borde) = (p, o, b);
                let f = &mut self.fondos_ui;
                (f.paleta, f.opacidad, f.borde) = (p, o, b);
                let v = &mut self.ventanas_ui;
                (v.paleta, v.opacidad, v.borde) = (p, o, b);
            }
            Orden::Abrir { sesion } => self.abrir(sesion),
            Orden::Cerrar => self.cerrar(false),
            Orden::Tarjetas(t) => self.ui.datos.tarjetas = t,
            Orden::Lista(l, c) => {
                // Un campo nuevo toma el foco.
                let nuevo = campo_de(&c).filter(|g| campo_de_lista(&self.ui, l) != Some(*g));
                self.ui.datos.listas.insert(l, c);
                self.ui.soltar_deslices(l);
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
                    self.abrir_menu(*m);
                }
            }
            Orden::Toasts(v) => self.poner_toasts(v),
            Orden::LauncherAbrir { prompt, simbolos } => self.abrir_launcher(prompt, simbolos),
            Orden::LauncherLista(l) => {
                let pedidos = self.launcher_ui.poner_lista(l);
                self.pedidos_launcher(pedidos);
            }
            Orden::LauncherCerrar => self.cerrar_launcher(),
            Orden::AjustesAbrir => self.abrir_ajustes(),
            Orden::AjustesCerrar => self.cerrar_ajustes(),
            Orden::AjustesIndice(i) => self.ajustes_ui.poner_indice(i),
            Orden::AjustesPagina(p) => self.ajustes_ui.poner_pagina(p),
            Orden::AjustesIr {
                seccion,
                fila,
                suave,
            } => self.ajustes_ui.ir_a(seccion, fila, suave),
            Orden::AjustesHecho { lugar, ok } => {
                self.ajustes_ui.hecho(lugar, ok);
                // Hyprland 0.56, tras `setcursor`, no redibuja la forma ya pedida: una ida y
                // vuelta a otra la vuelve a pedir, y sale del tema nuevo.
                if self.ajustes.is_some()
                    && let Some(f) = &self.forma
                {
                    f.set_shape(self.serial_puntero, Shape::Progress);
                    f.set_shape(
                        self.serial_puntero,
                        self.forma_puesta.unwrap_or(Shape::Default),
                    );
                }
            }
            Orden::AjustesGrabando(g) => self.ajustes_ui.grabando = g,
            Orden::CalculadoraAbrir => self.abrir_calculadora(),
            Orden::CalculadoraCerrar => self.calculadora = None,
            Orden::Calculadora(v) => self.calculadora_ui.poner(v),
            Orden::FondosAbrir(v) => self.abrir_fondos(*v),
            Orden::FondosCerrar => self.cerrar_fondos(),
            Orden::VentanasAbrir(v) => {
                self.ventanas = None;
                self.ventanas_salida = v.salida.clone();
                self.ventanas_ui.abrir(*v);
            }
            Orden::VentanasMostrar => self.mostrar_ventanas(),
            Orden::VentanasReciente => {
                self.ventanas_ui.reciente();
                self.mostrar_ventanas();
            }
            Orden::VentanasConfirmar => {
                let v = self.ventanas_ui.confirmar(false);
                self.eventos_ventanas(v);
            }
            Orden::VentanasFoto(d, img) => self.ventanas_ui.foto(&d, img),
            Orden::VentanasFondo(img) => self.ventanas_ui.fondo(img),
            Orden::VentanasCerrar => self.cerrar_ventanas(),
            Orden::AjustesPrecalentar(p) => {
                if self.ajustes.is_none() {
                    self.ajustes_ui.precalentar(p);
                }
            }
        }
        self.ensuciar();
    }

    fn poner_toasts(&mut self, v: Vec<Aviso>) {
        self.toasts_ui.avisos = v;
        if self.toasts_ui.avisos.is_empty() {
            self.toasts = None;
            self.toasts_ui.vaciar();
            return;
        }
        if self.toasts.is_some() {
            return;
        }
        let wl = self.compositor.create_surface(&self.qh);
        let capa = self.capas.create_layer_surface(
            &self.qh,
            wl,
            Layer::Overlay,
            Some("morgan-notifications"),
            None,
        );
        // El alto lo pone la salida: lo que no entra se scrollea adentro.
        capa.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::RIGHT);
        capa.set_size(toasts::ANCHO, 0);
        capa.set_keyboard_interactivity(KeyboardInteractivity::None);
        // Hasta el primer cuadro no responde nada: la columna no tapa los clicks.
        if let Ok(r) = Region::new(&self.compositor) {
            capa.wl_surface().set_input_region(Some(r.wl_region()));
        }
        capa.commit();
        self.toasts = Some(ToastsSup {
            lamina: Lamina::new((0, 0)),
            capa,
            inicio: None,
        });
    }

    fn abrir_launcher(&mut self, prompt: String, simbolos: bool) {
        self.launcher_ui.abrir(prompt, simbolos);
        if self.launcher.is_some() {
            return;
        }
        let wl = self.compositor.create_surface(&self.qh);
        let capa = self.capas.create_layer_surface(
            &self.qh,
            wl,
            Layer::Overlay,
            Some("morgan-launcher"),
            None,
        );
        capa.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        capa.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        capa.commit();
        self.launcher = Some(LauncherSup {
            lamina: Lamina::new((0, 0)),
            capa,
            inicio: None,
        });
    }

    fn abrir_calculadora(&mut self) {
        if self.calculadora.is_some() {
            return;
        }
        self.calculadora_ui.abrir();
        let wl = self.compositor.create_surface(&self.qh);
        let capa = self.capas.create_layer_surface(
            &self.qh,
            wl,
            Layer::Top,
            Some("morgan-calculator"),
            None,
        );
        capa.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        capa.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        capa.commit();
        self.calculadora = Some(LauncherSup {
            lamina: Lamina::new((0, 0)),
            capa,
            inicio: None,
        });
    }

    fn abrir_fondos(&mut self, v: VistaFondos) {
        if self.fondos.is_some() {
            return;
        }
        let salida = v.salida.as_deref().and_then(|n| {
            self.salidas
                .outputs()
                .find(|o| self.salidas.info(o).and_then(|i| i.name).as_deref() == Some(n))
        });
        // Sin la salida enfocada a mano, la primera: el alto es una estimación.
        let alto_salida = salida
            .clone()
            .or_else(|| self.salidas.outputs().next())
            .and_then(|o| self.salidas.info(&o))
            .and_then(|i| i.logical_size)
            .map_or(1080, |(_, h)| h);
        self.fondos_ui.abrir(v, alto_salida, self.minis.clone());
        let wl = self.compositor.create_surface(&self.qh);
        let capa = self.capas.create_layer_surface(
            &self.qh,
            wl,
            Layer::Overlay,
            Some("morgan-wallpapers"),
            salida.as_ref(),
        );
        capa.set_anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        capa.set_size(0, FondosUi::alto(alto_salida));
        capa.set_exclusive_zone(-1);
        capa.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        capa.commit();
        self.fondos = Some(LauncherSup {
            lamina: Lamina::new((0, 0)),
            capa,
            inicio: None,
        });
    }

    /// La capa recién se pone al mostrarse: hasta entonces el teclado sigue en la ventana
    /// enfocada.
    fn mostrar_ventanas(&mut self) {
        if self.ventanas.is_some() || !self.ventanas_ui.abierta() {
            return;
        }
        let salida = self.ventanas_salida.as_deref().and_then(|n| {
            self.salidas
                .outputs()
                .find(|o| self.salidas.info(o).and_then(|i| i.name).as_deref() == Some(n))
        });
        let wl = self.compositor.create_surface(&self.qh);
        let capa = self.capas.create_layer_surface(
            &self.qh,
            wl,
            Layer::Overlay,
            Some("morgan-windows"),
            salida.as_ref(),
        );
        capa.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        capa.set_exclusive_zone(-1);
        capa.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        capa.commit();
        self.ventanas = Some(LauncherSup {
            lamina: Lamina::new((0, 0)),
            capa,
            inicio: None,
        });
    }

    fn cerrar_ventanas(&mut self) {
        self.ventanas = None;
        self.ventanas_ui.cerrar();
    }

    fn eventos_ventanas(&mut self, v: Vec<EventoVentanas>) {
        for e in v {
            // Enfocar o cerrar: la capa se va, y con ella el estado.
            self.avisar(Evento::Ventanas(e));
            self.cerrar_ventanas();
        }
        if let Some(v) = &mut self.ventanas {
            v.lamina.sucio = true;
        }
    }

    fn cerrar_fondos(&mut self) {
        self.fondos = None;
        self.fondos_ui.cerrar();
    }

    fn mini(&mut self, m: Mini) {
        self.fondos_ui.mini(m);
        if let Some(f) = &mut self.fondos {
            f.lamina.sucio = true;
        }
    }

    fn eventos_fondos(&mut self, v: Vec<EventoFondos>) {
        for e in v {
            let cierra = matches!(e, EventoFondos::Cerrada { .. });
            self.avisar(Evento::Fondos(e));
            if cierra {
                self.cerrar_fondos();
            }
        }
        if let Some(f) = &mut self.fondos {
            f.lamina.sucio = true;
        }
    }

    fn eventos_calculadora(&mut self, v: Vec<EventoCalculadora>) {
        for e in v {
            let cierra = matches!(e, EventoCalculadora::Cerrada);
            self.avisar(Evento::Calculadora(e));
            if cierra {
                self.calculadora = None;
            }
        }
        if let Some(c) = &mut self.calculadora {
            c.lamina.sucio = true;
        }
    }

    fn abrir_ajustes(&mut self) {
        self.ajustes_ui.abrir();
        if self.ajustes.is_some() {
            return;
        }
        let wl = self.compositor.create_surface(&self.qh);
        let capa = self.capas.create_layer_surface(
            &self.qh,
            wl,
            Layer::Overlay,
            Some("morgan-menu"),
            None,
        );
        capa.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        capa.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        capa.commit();
        self.ajustes = Some(LauncherSup {
            lamina: Lamina::new((0, 0)),
            capa,
            inicio: None,
        });
    }

    fn cerrar_ajustes(&mut self) {
        if self.ajustes.take().is_none() {
            return;
        }
        self.ajustes_ui.cerrar();
        if self.panel.is_none()
            && self.launcher.is_none()
            && let Some((_, t)) = self.parpadeo.take()
        {
            self.bucle.remove(t);
        }
    }

    fn pedidos_ajustes(&mut self, pedidos: Vec<ajustes::Pedido>) {
        for p in pedidos {
            match p {
                ajustes::Pedido::Gtk(e) => {
                    let cierra = matches!(e, EventoAjustes::Cerrado);
                    self.avisar(Evento::Ajustes(e));
                    if cierra {
                        self.cerrar_ajustes();
                    }
                }
                ajustes::Pedido::Pegar => self.pegar(|h, t| {
                    let pedidos = h.ajustes_ui.pegar(t);
                    h.pedidos_ajustes(pedidos);
                }),
            }
        }
        if let Some(a) = &mut self.ajustes {
            a.lamina.sucio = true;
        }
    }

    fn cerrar_launcher(&mut self) {
        if self.launcher.take().is_none() {
            return;
        }
        self.launcher_ui.cerrar();
        if self.panel.is_none()
            && let Some((_, t)) = self.parpadeo.take()
        {
            self.bucle.remove(t);
        }
    }

    fn pedidos_launcher(&mut self, pedidos: Vec<launcher::Pedido>) {
        for p in pedidos {
            match p {
                launcher::Pedido::Gtk(e) => {
                    let cierra = matches!(e, EventoLauncher::Cerrado { .. });
                    self.avisar(Evento::Launcher(e));
                    if cierra {
                        self.cerrar_launcher();
                    }
                }
                launcher::Pedido::Pegar => self.pegar(|h, t| {
                    let pedidos = h.launcher_ui.pegar(t);
                    h.pedidos_launcher(pedidos);
                }),
            }
        }
        if let Some(l) = &mut self.launcher {
            l.lamina.sucio = true;
        }
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
        let capa =
            self.capas
                .create_layer_surface(&self.qh, wl, Layer::Top, Some("morgan-center"), None);
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
                Pedido::Pegar(l) => self.pegar(move |h, t| h.ui.pegar(l, t)),
            }
        }
        self.ensuciar();
    }

    /// Lo que hay en el portapapeles, con `wl-paste` (sin esperar: lo trae un hilo).
    fn pegar(&mut self, al_llegar: impl Fn(&mut Hilo, &str) + 'static) {
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
                al_llegar(h, &t);
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
        let proximo = if self.launcher.is_some() {
            self.launcher_ui.proximo_parpadeo(ahora)
        } else if self.ajustes.is_some() {
            self.ajustes_ui.proximo_parpadeo(ahora)
        } else {
            self.ui.proximo_parpadeo(ahora)
        };
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
        self.pintar_toasts(ahora);
        self.pintar_launcher(ahora);
        self.pintar_ajustes(ahora);
        self.pintar_calculadora(ahora);
        self.pintar_fondos(ahora);
        self.pintar_ventanas(ahora);
        let Some(p) = &mut self.panel else {
            let _ = self.conn.flush();
            return;
        };
        let anda = self.ui.anda(ahora)
            || p.inicio
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
            self.ui
                .pintar(&mut self.escena, f64::from(escala), f64::from(bajada));
            let wl = p.capa.wl_surface().clone();
            p.lamina
                .presentar(&mut self.gpu, &self.escena, &wl, &self.qh);
            self.programar_parpadeo(ahora);
        }
        let _ = self.conn.flush();
    }

    /// Baja desde su borde de abajo al abrir (`bajada.rs`), como el centro.
    fn pintar_launcher(&mut self, ahora: Instant) {
        let Some(l) = &mut self.launcher else { return };
        if !l.lamina.lista() {
            return;
        }
        // El cuadro del destello ya se vio: se hace y se cierra.
        let hechos = self.launcher_ui.al_cuadro();
        if !hechos.is_empty() {
            self.pedidos_launcher(hechos.into_iter().map(launcher::Pedido::Gtk).collect());
            return;
        }
        let ui = &mut self.launcher_ui;
        let anda = ui.anda(ahora)
            || l.inicio
                .is_none_or(|t| ahora.saturating_duration_since(t) < crate::bajada::duracion());
        if !(l.lamina.sucio || anda) {
            return;
        }
        ui.tam = l.lamina.tam;
        let escala = l.lamina.escala;
        ui.armar(escala as f32, ahora);
        let inicio = *l.inicio.get_or_insert(ahora);
        let micros = ahora.saturating_duration_since(inicio).as_micros() as i64;
        let bajada = crate::bajada::desplazamiento(micros, ui.panel().y1 as f32);
        ui.pintar(&mut self.escena, f64::from(escala), f64::from(bajada));
        let wl = l.capa.wl_surface().clone();
        l.lamina
            .presentar(&mut self.gpu, &self.escena, &wl, &self.qh);
        self.programar_parpadeo(ahora);
    }

    /// Baja desde su borde de abajo al abrir (`bajada.rs`), como el centro.
    fn pintar_ajustes(&mut self, ahora: Instant) {
        let Some(a) = &mut self.ajustes else { return };
        let ui = &mut self.ajustes_ui;
        let anda = ui.anda(ahora)
            || a.inicio
                .is_none_or(|t| ahora.saturating_duration_since(t) < crate::bajada::duracion());
        if !a.lamina.lista() || !(a.lamina.sucio || anda) {
            return;
        }
        ui.tam = a.lamina.tam;
        let escala = a.lamina.escala;
        ui.armar(escala as f32, ahora);
        let inicio = *a.inicio.get_or_insert(ahora);
        let micros = ahora.saturating_duration_since(inicio).as_micros() as i64;
        let bajada = crate::bajada::desplazamiento(micros, ui.panel().y1 as f32);
        ui.pintar(&mut self.escena, f64::from(escala), f64::from(bajada));
        let wl = a.capa.wl_surface().clone();
        a.lamina
            .presentar(&mut self.gpu, &self.escena, &wl, &self.qh);
        self.programar_parpadeo(ahora);
    }

    /// Baja desde su borde de abajo al abrir (`bajada.rs`), como el centro.
    fn pintar_calculadora(&mut self, ahora: Instant) {
        let Some(c) = &mut self.calculadora else { return };
        let ui = &mut self.calculadora_ui;
        let anda = ui.anda(ahora)
            || c.inicio
                .is_none_or(|t| ahora.saturating_duration_since(t) < crate::bajada::duracion());
        if !c.lamina.lista() || !(c.lamina.sucio || anda) {
            return;
        }
        ui.tam = c.lamina.tam;
        let escala = c.lamina.escala;
        ui.armar(escala as f32, ahora);
        let inicio = *c.inicio.get_or_insert(ahora);
        let micros = ahora.saturating_duration_since(inicio).as_micros() as i64;
        let bajada = crate::bajada::desplazamiento(micros, ui.panel().y1 as f32);
        ui.pintar(&mut self.escena, f64::from(escala), f64::from(bajada));
        let wl = c.capa.wl_surface().clone();
        c.lamina
            .presentar(&mut self.gpu, &self.escena, &wl, &self.qh);
    }

    /// Sin bajada: Hyprland la sube desde el borde (`slide bottom`).
    fn pintar_fondos(&mut self, ahora: Instant) {
        let Some(f) = &mut self.fondos else { return };
        let ui = &mut self.fondos_ui;
        if !f.lamina.lista() || !(f.lamina.sucio || ui.anda(ahora)) {
            return;
        }
        ui.tam = f.lamina.tam;
        let escala = f.lamina.escala;
        ui.armar(escala as f32, ahora);
        ui.pintar(&mut self.escena, f64::from(escala));
        let wl = f.capa.wl_surface().clone();
        f.lamina
            .presentar(&mut self.gpu, &self.escena, &wl, &self.qh);
    }

    /// Sin bajada propia: sube 10 px mientras Hyprland funde la capa.
    fn pintar_ventanas(&mut self, ahora: Instant) {
        let Some(v) = &mut self.ventanas else { return };
        let ui = &mut self.ventanas_ui;
        if !v.lamina.lista() || !(v.lamina.sucio || ui.anda(ahora)) {
            return;
        }
        ui.tam = v.lamina.tam;
        let escala = v.lamina.escala;
        ui.armar(escala as f32, ahora);
        ui.pintar(&mut self.escena, f64::from(escala));
        let wl = v.capa.wl_surface().clone();
        v.lamina
            .presentar(&mut self.gpu, &self.escena, &wl, &self.qh);
    }

    fn puntero_ventanas(&mut self, e: &PointerEvent) {
        let (x, y) = e.position;
        let ui = &mut self.ventanas_ui;
        let v = match e.kind {
            PointerEventKind::Enter { serial } => {
                self.serial_puntero = serial;
                self.forma_puesta = None;
                ui.mover(x, y);
                Vec::new()
            }
            PointerEventKind::Motion { .. } => {
                ui.mover(x, y);
                Vec::new()
            }
            PointerEventKind::Leave { .. } => {
                ui.salir();
                Vec::new()
            }
            PointerEventKind::Press { .. } => {
                ui.apretar(x, y);
                Vec::new()
            }
            PointerEventKind::Release { .. } => ui.soltar(x, y),
            PointerEventKind::Axis { .. } => Vec::new(),
        };
        self.eventos_ventanas(v);
        if self.forma_puesta != Some(Shape::Default)
            && let Some(f) = &self.forma
        {
            f.set_shape(self.serial_puntero, Shape::Default);
            self.forma_puesta = Some(Shape::Default);
        }
    }

    fn puntero_fondos(&mut self, e: &PointerEvent) {
        let (x, y) = e.position;
        let ui = &mut self.fondos_ui;
        let v = match e.kind {
            PointerEventKind::Enter { serial } => {
                self.serial_puntero = serial;
                self.forma_puesta = None;
                Vec::new()
            }
            PointerEventKind::Motion { .. } | PointerEventKind::Leave { .. } => Vec::new(),
            PointerEventKind::Press { .. } => {
                ui.apretar(x, y);
                Vec::new()
            }
            PointerEventKind::Release { .. } => ui.soltar(x, y),
            PointerEventKind::Axis {
                horizontal,
                vertical,
                ..
            } => {
                // El eje que más se movió, como la banda con GTK.
                let eje = |a: smithay_client_toolkit::seat::pointer::AxisScroll| {
                    if a.value120 != 0 {
                        (f64::from(a.value120) / 120.0, true)
                    } else if a.discrete != 0 {
                        (f64::from(a.discrete), true)
                    } else {
                        (a.absolute, false)
                    }
                };
                let (v, h) = (eje(vertical), eje(horizontal));
                let (paso, discreto) = if v.0.abs() >= h.0.abs() { v } else { h };
                if paso != 0.0 {
                    ui.rueda(paso, discreto);
                }
                Vec::new()
            }
        };
        self.eventos_fondos(v);
        if self.forma_puesta != Some(Shape::Default)
            && let Some(f) = &self.forma
        {
            f.set_shape(self.serial_puntero, Shape::Default);
            self.forma_puesta = Some(Shape::Default);
        }
    }

    fn puntero_calculadora(&mut self, e: &PointerEvent) {
        let (x, y) = e.position;
        let ui = &mut self.calculadora_ui;
        let v = match e.kind {
            PointerEventKind::Enter { serial } => {
                self.serial_puntero = serial;
                self.forma_puesta = None;
                ui.mover(x, y);
                Vec::new()
            }
            PointerEventKind::Motion { .. } => {
                ui.mover(x, y);
                Vec::new()
            }
            PointerEventKind::Leave { .. } => {
                ui.salir();
                Vec::new()
            }
            PointerEventKind::Press { .. } => {
                ui.apretar(x, y);
                Vec::new()
            }
            PointerEventKind::Release { button, .. } => ui.soltar(x, y, button == IZQUIERDO),
            PointerEventKind::Axis { vertical, .. } => {
                if vertical.value120 != 0 {
                    ui.rueda(f64::from(vertical.value120) / 120.0, true);
                } else if vertical.discrete != 0 {
                    ui.rueda(f64::from(vertical.discrete), true);
                } else if vertical.absolute != 0.0 {
                    ui.rueda(vertical.absolute, false);
                }
                Vec::new()
            }
        };
        self.eventos_calculadora(v);
        if self.forma_puesta != Some(Shape::Default)
            && let Some(f) = &self.forma
        {
            f.set_shape(self.serial_puntero, Shape::Default);
            self.forma_puesta = Some(Shape::Default);
        }
    }

    fn puntero_ajustes(&mut self, e: &PointerEvent) {
        let (x, y) = e.position;
        let ui = &mut self.ajustes_ui;
        let pedidos = match e.kind {
            PointerEventKind::Enter { serial } => {
                self.serial_puntero = serial;
                self.forma_puesta = None;
                ui.mover(x, y);
                Vec::new()
            }
            PointerEventKind::Motion { .. } => {
                ui.mover(x, y);
                Vec::new()
            }
            PointerEventKind::Leave { .. } => {
                ui.salir();
                Vec::new()
            }
            PointerEventKind::Press { .. } => {
                ui.apretar(x, y);
                Vec::new()
            }
            PointerEventKind::Release { button, .. } => ui.soltar(x, y, button == IZQUIERDO),
            PointerEventKind::Axis { vertical, .. } => {
                if vertical.value120 != 0 {
                    ui.rueda(f64::from(vertical.value120) / 120.0, true);
                } else if vertical.discrete != 0 {
                    ui.rueda(f64::from(vertical.discrete), true);
                } else if vertical.absolute != 0.0 {
                    ui.rueda(vertical.absolute, false);
                }
                Vec::new()
            }
        };
        let forma = match self.ajustes_ui.forma_puntero() {
            ajustes::Forma::Normal => Shape::Default,
            ajustes::Forma::Texto => Shape::Text,
            ajustes::Forma::Mano => Shape::Grab,
        };
        self.pedidos_ajustes(pedidos);
        if self.forma_puesta != Some(forma)
            && let Some(f) = &self.forma
        {
            f.set_shape(self.serial_puntero, forma);
            self.forma_puesta = Some(forma);
        }
    }

    fn puntero_launcher(&mut self, e: &PointerEvent) {
        let (x, y) = e.position;
        let ui = &mut self.launcher_ui;
        let pedidos = match e.kind {
            PointerEventKind::Enter { serial } => {
                self.serial_puntero = serial;
                self.forma_puesta = None;
                ui.mover_puntero(x, y, false);
                Vec::new()
            }
            PointerEventKind::Motion { .. } => {
                ui.mover_puntero(x, y, true);
                Vec::new()
            }
            PointerEventKind::Leave { .. } => {
                ui.salir_puntero();
                Vec::new()
            }
            PointerEventKind::Press { .. } => {
                ui.apretar(x, y);
                Vec::new()
            }
            PointerEventKind::Release { button, .. } => ui.soltar(x, y, button == IZQUIERDO),
            PointerEventKind::Axis { vertical, .. } => {
                if vertical.value120 != 0 {
                    ui.rueda(f64::from(vertical.value120) / 120.0, true);
                } else if vertical.discrete != 0 {
                    ui.rueda(f64::from(vertical.discrete), true);
                } else if vertical.absolute != 0.0 {
                    ui.rueda(vertical.absolute, false);
                }
                Vec::new()
            }
        };
        self.pedidos_launcher(pedidos);
        if self.forma_puesta != Some(Shape::Default)
            && let Some(f) = &self.forma
        {
            f.set_shape(self.serial_puntero, Shape::Default);
            self.forma_puesta = Some(Shape::Default);
        }
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
        m.lamina
            .presentar(&mut self.gpu, &self.escena, &wl, &self.qh);
    }

    /// Los toasts bajan desde su borde de abajo al aparecer la columna (`bajada.rs`); solo
    /// responden las tarjetas.
    fn pintar_toasts(&mut self, ahora: Instant) {
        let terminados = self.toasts_ui.al_cuadro(ahora);
        for e in terminados {
            self.avisar(Evento::Toast(e));
        }
        let Some(t) = &mut self.toasts else { return };
        let ui = &mut self.toasts_ui;
        let anda = ui.anda(ahora)
            || t.inicio
                .is_none_or(|i| ahora.saturating_duration_since(i) < crate::bajada::duracion());
        if !t.lamina.lista() || !(t.lamina.sucio || anda) {
            return;
        }
        ui.tam = t.lamina.tam;
        let escala = t.lamina.escala;
        ui.armar(escala as f32, ahora);
        let encima = ui.encima_cambio();
        let inicio = *t.inicio.get_or_insert(ahora);
        let micros = ahora.saturating_duration_since(inicio).as_micros() as i64;
        let bajada = f64::from(crate::bajada::desplazamiento(micros, ui.fondo() as f32));
        ui.pintar(&mut self.escena, f64::from(escala), bajada);
        let wl = t.capa.wl_surface().clone();
        if let Ok(r) = Region::new(&self.compositor) {
            for c in ui.region(bajada) {
                r.add(
                    c.x0.floor() as i32,
                    c.y0.floor() as i32,
                    c.width().ceil() as i32,
                    c.height().ceil() as i32,
                );
            }
            wl.set_input_region(Some(r.wl_region()));
        }
        t.lamina
            .presentar(&mut self.gpu, &self.escena, &wl, &self.qh);
        if let Some(e) = encima {
            self.avisar(Evento::Toast(e));
        }
    }

    fn puntero_toasts(&mut self, e: &PointerEvent) {
        let (x, y) = e.position;
        let ui = &mut self.toasts_ui;
        let eventos = match e.kind {
            PointerEventKind::Enter { serial } => {
                self.serial_puntero = serial;
                self.forma_puesta = None;
                ui.mover(x, y, 0)
            }
            PointerEventKind::Motion { time } => ui.mover(x, y, time),
            PointerEventKind::Leave { .. } => {
                ui.salir();
                Vec::new()
            }
            PointerEventKind::Press { button, time, .. } => {
                ui.apretar(x, y, button, time);
                Vec::new()
            }
            PointerEventKind::Release { button, .. } => ui.soltar(x, y, button),
            PointerEventKind::Axis { vertical, .. } => {
                if vertical.value120 != 0 {
                    ui.rueda(f64::from(vertical.value120) / 120.0, true);
                } else if vertical.discrete != 0 {
                    ui.rueda(f64::from(vertical.discrete), true);
                } else if vertical.absolute != 0.0 {
                    ui.rueda(vertical.absolute, false);
                }
                Vec::new()
            }
        };
        let encima = ui.encima_cambio();
        for e in eventos.into_iter().chain(encima) {
            self.avisar(Evento::Toast(e));
        }
        if let Some(t) = &mut self.toasts {
            t.lamina.sucio = true;
        }
        if self.forma_puesta != Some(Shape::Default)
            && let Some(f) = &self.forma
        {
            f.set_shape(self.serial_puntero, Shape::Default);
            self.forma_puesta = Some(Shape::Default);
        }
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

    /// El keysym de la tecla `codigo` (de xkb) sin modificadores, en la distribución en uso; sin
    /// mapa, el mismo `keysym`.
    fn base_de(&self, codigo: u32, keysym: Keysym) -> u32 {
        self.keymap
            .as_ref()
            .and_then(|k| {
                k.key_get_syms_by_level(xkbcommon::xkb::Keycode::new(codigo), self.layout, 0)
                    .first()
                    .copied()
            })
            .unwrap_or(keysym)
            .raw()
    }

    fn tecla(&mut self, ev: KeyEvent) {
        // El menú es lo último que se abrió: el teclado es suyo.
        if let Some(m) = &mut self.menu {
            let s = m.ui.tecla(ev.keysym, self.mods.shift);
            m.lamina.sucio = true;
            self.salida_menu(s);
            return;
        }
        if self.ventanas.is_some() {
            let v = self.ventanas_ui.tecla(ev.keysym);
            self.eventos_ventanas(v);
            return;
        }
        if self.fondos.is_some() {
            let v = self
                .fondos_ui
                .tecla(ev.keysym, ev.utf8.as_deref(), self.mods.ctrl);
            self.eventos_fondos(v);
            return;
        }
        if self.calculadora.is_some() {
            let m = self.mods;
            let v = self
                .calculadora_ui
                .tecla(ev.keysym, ev.utf8.as_deref(), m.ctrl, m.alt);
            self.eventos_calculadora(v);
            return;
        }
        if self.ajustes.is_some() {
            let m = self.mods;
            let pedidos = self.ajustes_ui.tecla(
                ev.keysym,
                ev.utf8.as_deref(),
                self.base_de(ev.raw_code + 8, ev.keysym),
                [m.logo, m.ctrl, m.alt, m.shift],
            );
            self.pedidos_ajustes(pedidos);
            return;
        }
        if self.launcher.is_some() {
            let pedidos = self
                .launcher_ui
                .tecla(ev.keysym, ev.utf8.as_deref(), self.mods.ctrl);
            self.pedidos_launcher(pedidos);
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
        Contenido::Opciones(v) => v.iter().find_map(|o| {
            o.campo
                .as_ref()
                .filter(|c| c.caja.is_some())
                .map(|c| c.generacion)
        }),
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
            .ventanas
            .as_ref()
            .is_some_and(|v| v.capa.wl_surface() == capa.wl_surface())
        {
            self.eventos_ventanas(vec![EventoVentanas::Cerrada]);
        }
        if self
            .fondos
            .as_ref()
            .is_some_and(|f| f.capa.wl_surface() == capa.wl_surface())
        {
            self.eventos_fondos(vec![EventoFondos::Cerrada { volver: false }]);
        }
        if self
            .calculadora
            .as_ref()
            .is_some_and(|c| c.capa.wl_surface() == capa.wl_surface())
        {
            self.eventos_calculadora(vec![EventoCalculadora::Cerrada]);
        }
        if self
            .ajustes
            .as_ref()
            .is_some_and(|a| a.capa.wl_surface() == capa.wl_surface())
        {
            self.pedidos_ajustes(vec![ajustes::Pedido::Gtk(EventoAjustes::Cerrado)]);
        }
        if self
            .launcher
            .as_ref()
            .is_some_and(|l| l.capa.wl_surface() == capa.wl_surface())
        {
            self.pedidos_launcher(vec![launcher::Pedido::Gtk(EventoLauncher::Cerrado {
                escape: false,
            })]);
        }
        // La salida se fue: la próxima lista la vuelve a crear en otra.
        if self
            .toasts
            .as_ref()
            .is_some_and(|t| t.capa.wl_surface() == capa.wl_surface())
        {
            self.toasts = None;
        }
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
        if let Some(l) = self
            .ajustes
            .as_mut()
            .filter(|l| l.capa.wl_surface() == wl)
            .or(self.launcher.as_mut().filter(|l| l.capa.wl_surface() == wl))
            .or(self.calculadora.as_mut().filter(|l| l.capa.wl_surface() == wl))
            .or(self.fondos.as_mut().filter(|l| l.capa.wl_surface() == wl))
            .or(self.ventanas.as_mut().filter(|l| l.capa.wl_surface() == wl))
        {
            if c.new_size.0 > 0 && c.new_size.1 > 0 {
                l.lamina.tam = c.new_size;
            }
            if l.lamina.lienzo.is_none() {
                // SAFETY: el lienzo se suelta antes que la capa (orden de los campos de LauncherSup).
                l.lamina.lienzo = unsafe { Lienzo::nuevo(&mut self.gpu, &self.conn, wl) };
            }
            l.lamina.sucio = true;
            return;
        }
        if let Some(t) = self.toasts.as_mut().filter(|t| t.capa.wl_surface() == wl) {
            if c.new_size.0 > 0 && c.new_size.1 > 0 {
                t.lamina.tam = c.new_size;
            }
            if t.lamina.lienzo.is_none() {
                // SAFETY: el lienzo se suelta antes que la capa (orden de los campos de ToastsSup).
                t.lamina.lienzo = unsafe { Lienzo::nuevo(&mut self.gpu, &self.conn, wl) };
            }
            t.lamina.sucio = true;
            return;
        }
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
        if let Some(t) = self.toasts.as_mut().filter(|t| t.capa.wl_surface() == wl) {
            return Some(&mut t.lamina);
        }
        if let Some(l) = self.launcher.as_mut().filter(|l| l.capa.wl_surface() == wl) {
            return Some(&mut l.lamina);
        }
        if let Some(a) = self.ajustes.as_mut().filter(|a| a.capa.wl_surface() == wl) {
            return Some(&mut a.lamina);
        }
        if let Some(c) = self.calculadora.as_mut().filter(|c| c.capa.wl_surface() == wl) {
            return Some(&mut c.lamina);
        }
        if let Some(f) = self.fondos.as_mut().filter(|f| f.capa.wl_surface() == wl) {
            return Some(&mut f.lamina);
        }
        if let Some(v) = self.ventanas.as_mut().filter(|v| v.capa.wl_surface() == wl) {
            return Some(&mut v.lamina);
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

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
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
        ev: KeyEvent,
    ) {
        // Soltar `Mod` confirma el lienzo de ventanas.
        if self.ventanas.is_some() {
            let v = self.ventanas_ui.soltada(ev.keysym);
            self.eventos_ventanas(v);
        }
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        m: Modifiers,
        _: RawModifiers,
        layout: u32,
    ) {
        self.mods = m;
        self.layout = layout;
    }

    fn update_keymap(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        keymap: smithay_client_toolkit::seat::keyboard::Keymap<'_>,
    ) {
        use xkbcommon::xkb;
        self.keymap = xkb::Keymap::new_from_string(
            &xkb::Context::new(xkb::CONTEXT_NO_FLAGS),
            keymap.as_string(),
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        );
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
                .toasts
                .as_ref()
                .is_some_and(|t| t.capa.wl_surface() == &e.surface)
            {
                self.puntero_toasts(e);
            } else if self
                .launcher
                .as_ref()
                .is_some_and(|l| l.capa.wl_surface() == &e.surface)
            {
                self.puntero_launcher(e);
            } else if self
                .ajustes
                .as_ref()
                .is_some_and(|a| a.capa.wl_surface() == &e.surface)
            {
                self.puntero_ajustes(e);
            } else if self
                .calculadora
                .as_ref()
                .is_some_and(|c| c.capa.wl_surface() == &e.surface)
            {
                self.puntero_calculadora(e);
            } else if self
                .ventanas
                .as_ref()
                .is_some_and(|v| v.capa.wl_surface() == &e.surface)
            {
                self.puntero_ventanas(e);
            } else if self
                .fondos
                .as_ref()
                .is_some_and(|f| f.capa.wl_surface() == &e.surface)
            {
                self.puntero_fondos(e);
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
