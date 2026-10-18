//! El teclado tiene dos zonas, índice y página. El buscador tiene el foco siempre: lo elegido
//! de cada zona es una marca propia, como en el launcher.
//!
//! El menú lee lo puesto de la fuente real, aplica y vuelve a leer para marcar. Lo de morgan se
//! escribe en `config.toml`, se relee y se aplica en vivo.

pub(crate) mod arbol;
pub mod atajos;
pub mod compositor;
pub mod monitores;
mod cursores;
mod dibujo;
mod lectura;
mod sistema;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use morgan_ipc::{ModoLauncher, NivelMenu};

use crate::bajada::Bajada;
use crate::centro::desplegable;
use crate::comando;
use crate::config::{self, Config};
use crate::idle::Idle;
use crate::launcher::{binds, color_hex, resaltar_markup};
use crate::notificaciones::Notificaciones;
use crate::sonidos;
use crate::tema::Tema;
use arbol::{Categoria, Control, Fila, Que, Resultado, Seccion};
use lectura::leer;

const ANCHO_INDICE: i32 = 240;
const COLUMNAS: i32 = 3;
const DESLIZ: Duration = Duration::from_millis(200);
const DESPLIEGUE: u32 = 150;
const SEGUIR: Duration = Duration::from_millis(120);
type AlLauncher = Rc<dyn Fn(ModoLauncher)>;

const AVISO: Duration = Duration::from_secs(4);

pub struct Servicios {
    pub config: Config,
    pub ruta_config: PathBuf,
    pub tema: Tema,
    pub notificaciones: Notificaciones,
    pub idle: Idle,
    pub audio: crate::audio::Audio,
    pub sonidos: crate::sonidos::Sonidos,
    pub luz: crate::luz::Luz,
    pub fondos: crate::fondos::Fondos,
    pub calendario: crate::calendario::Calendario,
    pub centro: crate::centro::Centro,
}

#[derive(Debug, Clone, PartialEq)]
enum Entrada {
    Categoria(Categoria),
    Seccion(Categoria, usize),
    Resultado(Resultado),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Zona {
    Indice,
    Pagina,
}

#[derive(Clone)]
pub struct Menu {
    inner: Rc<Inner>,
}

/// Los cambios a prueba (salida y campo) y el temporizador que los deshace.
type Prueba = (Vec<(String, monitores::Campo)>, glib::SourceId);

struct Inner {
    /// Los cambios de monitor a prueba y la vuelta automática.
    prueba: RefCell<Option<Prueba>>,
    al_fondos: RefCell<Option<Rc<dyn Fn()>>>,
    al_launcher: RefCell<Option<AlLauncher>>,
    window: gtk::ApplicationWindow,
    tarjeta: gtk::Box,
    bajada: Bajada,
    texto: gtk::Text,
    indice: gtk::ListBox,
    scroll_indice: gtk::ScrolledWindow,
    pagina: gtk::Box,
    scroll_pagina: gtk::ScrolledWindow,
    resaltado: gtk::Label,

    s: Servicios,
    config: RefCell<Config>,
    abierto: Cell<bool>,
    zona: Cell<Zona>,
    categoria: Cell<Categoria>,
    /// Sobrevive al cierre: al abrir se muestra al instante y se vuelve a leer todo atrás.
    paginas: RefCell<HashMap<Categoria, Vec<Seccion>>>,
    /// Una lectura que llega cuando ya se pidió otra de la misma página se descarta, así no pisa
    /// algo más nuevo.
    revisiones: RefCell<HashMap<Categoria, u64>>,
    pedidos: Cell<u64>,
    listas: RefCell<Option<Rc<lectura::Listas>>>,
    /// La página sigue al resultado elegido un momento después de la última tecla, no en cada una.
    seguir: RefCell<Option<glib::SourceId>>,
    seguido: RefCell<Option<Entrada>>,
    /// Propia, abierta mientras el menú lo está: la del centro se abre solo con el centro.
    mezclador: crate::mezclador::Mezclador,
    salidas: RefCell<Vec<(String, String)>>,
    entradas: RefCell<Vec<Entrada>>,
    pintando: Cell<bool>,
    cabeceras: RefCell<Vec<gtk::Widget>>,
    filas: RefCell<Vec<Vec<gtk::Widget>>>,
    elegido: RefCell<Option<gtk::Widget>>,
    desplegables: RefCell<desplegable::Grupo>,
    ir_a: Cell<Option<(usize, Option<usize>)>>,
    ir_suave: Cell<bool>,
    deslizando: RefCell<Option<gtk::TickCallbackId>>,
    indice_abierta: Cell<Option<Categoria>>,
    grabando: RefCell<Option<Grabacion>>,
    grabaciones: Cell<u64>,
}

/// Hyprland resuelve los binds antes que el menú: mientras dure, está en el submap de grabar,
/// que no tiene ninguno, y sale siempre (al guardar, al cancelar, al cerrar el menú y al
/// arrancar).
struct Grabacion {
    id: String,
    numero: u64,
    fila: glib::WeakRef<gtk::Widget>,
    sub_antes: String,
    ocupadas: Option<Vec<crate::launcher::fuentes::Atajo>>,
    pendiente: Option<(String, Option<String>)>,
    nota: Option<gtk::Label>,
}

impl Menu {
    pub fn new(app: &gtk::Application, s: Servicios) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.add_css_class("morgan");
        window.add_css_class("blank-window");
        window.init_layer_shell();
        window.set_namespace(Some("morgan-menu"));
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        for borde in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(borde, true);
        }

        let tarjeta = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        tarjeta.add_css_class("launcher");
        tarjeta.add_css_class("menu");
        tarjeta.set_halign(gtk::Align::Center);
        tarjeta.set_valign(gtk::Align::Center);
        let bajada = Bajada::new(&tarjeta);
        window.set_child(Some(&bajada));

        // Ni el índice ni la página llevan riel: entran o se scrollean con la rueda.
        let izquierda = gtk::Box::new(gtk::Orientation::Vertical, 0);
        izquierda.add_css_class("menu-lateral");
        izquierda.set_size_request(ANCHO_INDICE, -1);
        izquierda.set_hexpand(false);
        let entrada = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        entrada.add_css_class("launcher-entrada");
        let lupa = gtk::Label::new(Some("\u{F0349}"));
        lupa.add_css_class("prompt");
        entrada.append(&lupa);
        let texto = gtk::Text::new();
        texto.set_placeholder_text(Some("Buscar…"));
        texto.set_hexpand(true);
        entrada.append(&texto);
        izquierda.append(&entrada);
        let indice = gtk::ListBox::new();
        indice.add_css_class("launcher-lista");
        indice.add_css_class("menu-indice");
        indice.set_selection_mode(gtk::SelectionMode::Single);
        indice.set_activate_on_single_click(true);
        indice.set_can_focus(false);
        let scroll_indice = gtk::ScrolledWindow::new();
        scroll_indice.set_policy(gtk::PolicyType::Never, gtk::PolicyType::External);
        scroll_indice.set_child(Some(&indice));
        scroll_indice.set_vexpand(true);
        izquierda.append(&scroll_indice);
        tarjeta.append(&izquierda);

        let pagina = gtk::Box::new(gtk::Orientation::Vertical, 8);
        pagina.add_css_class("menu-pagina");
        let scroll_pagina = gtk::ScrolledWindow::new();
        scroll_pagina.set_policy(gtk::PolicyType::Never, gtk::PolicyType::External);
        scroll_pagina.set_child(Some(&pagina));
        scroll_pagina.set_vexpand(true);
        scroll_pagina.set_hexpand(true);
        tarjeta.append(&scroll_pagina);

        let resaltado = gtk::Label::new(None);
        resaltado.add_css_class("launcher-resaltado");
        resaltado.set_visible(false);
        izquierda.append(&resaltado);

        let config = RefCell::new(s.config.clone());
        let inner = Rc::new(Inner {
            al_fondos: RefCell::default(),
            al_launcher: RefCell::default(),
            window,
            tarjeta,
            bajada,
            texto,
            indice,
            scroll_indice,
            pagina,
            scroll_pagina,
            resaltado,
            s,
            config,
            abierto: Cell::new(false),
            zona: Cell::new(Zona::Indice),
            categoria: Cell::new(Categoria::Apariencia),
            paginas: RefCell::default(),
            revisiones: RefCell::default(),
            pedidos: Cell::new(0),
            listas: RefCell::default(),
            seguir: RefCell::default(),
            seguido: RefCell::default(),
            mezclador: crate::mezclador::Mezclador::new(),
            salidas: RefCell::default(),
            entradas: RefCell::default(),
            pintando: Cell::new(false),
            cabeceras: RefCell::default(),
            filas: RefCell::default(),
            elegido: RefCell::default(),
            desplegables: RefCell::default(),
            ir_a: Cell::new(None),
            ir_suave: Cell::new(false),
            deslizando: RefCell::default(),
            indice_abierta: Cell::new(None),
            grabando: RefCell::new(None),
            grabaciones: Cell::new(0),
            prueba: RefCell::new(None),
        });
        let m = Menu { inner };
        m.conectar();
        // Por si morgan se cayó grabando una tecla: en el submap de grabar no anda ningún bind.
        comando::lanzar(&["hyprctl", "eval", &binds::submap_grabar(false)]);
        m
    }

    pub fn al_fondos(&self, f: impl Fn() + 'static) {
        self.inner.al_fondos.replace(Some(Rc::new(f)));
    }

    pub fn al_launcher(&self, f: impl Fn(ModoLauncher) + 'static) {
        self.inner.al_launcher.replace(Some(Rc::new(f)));
    }

    pub fn poner_ubicacion(&self, u: Option<crate::config::Clima>) {
        if let Some(u) = u
            && let Err(e) = self.cambiar_config(&arbol::Cambio::Ubicacion(u))
        {
            eprintln!("morgan: menú: {e}");
        }
        self.abrir(NivelMenu::Clima);
    }

    pub fn abierto(&self) -> bool {
        self.inner.abierto.get()
    }

    /// La galería de Fuentes dibuja cada familia con su tipografía, y cargarlas en frío son
    /// tirones en la bajada de la primera apertura. Medir carga las mismas que va a pedir la
    /// página, y Pango las guarda.
    pub fn precalentar(&self) {
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            glib::timeout_future(Duration::from_secs(3)).await;
            yo.refrescar(Categoria::Apariencia, None).await;
            let i = &yo.inner;
            if i.abierto.get() || i.categoria.get() != Categoria::Apariencia {
                return;
            }
            yo.pintar_pagina(false);
            yo.pintar_indice(Some(Entrada::Categoria(Categoria::Apariencia)));
            // La página va adentro de un scroll, que no siempre mide entero a su hijo.
            if let Some(c) = i.window.child() {
                c.measure(gtk::Orientation::Horizontal, -1);
                c.measure(gtk::Orientation::Vertical, -1);
            }
            i.pagina.measure(gtk::Orientation::Vertical, -1);
        });
    }

    pub fn alternar(&self, nivel: NivelMenu) {
        if self.abierto() {
            self.cerrar();
        } else {
            self.abrir(nivel);
        }
    }

    pub fn abrir(&self, nivel: NivelMenu) {
        let i = &self.inner;
        let ya_abierto = i.abierto.replace(true);
        let (cat, seccion) = match nivel {
            NivelMenu::Raiz => (Categoria::Apariencia, None),
            NivelMenu::Fondos => (Categoria::Apariencia, Some("Fondos")),
            NivelMenu::Clima => (Categoria::Shell, Some("General")),
        };
        let seccion = seccion.and_then(|n| cat.secciones().iter().position(|s| *s == n));
        if !ya_abierto {
            i.bajada.arrancar();
            i.indice_abierta.set(None);
            i.seguido.replace(None);
            i.mezclador.abrir();
            i.listas.replace(None);
            // Se muestra lo de la vez anterior y se relee todo: la búsqueda lo necesita.
            for c in Categoria::TODAS {
                self.pedir(c);
            }
        }
        i.zona.set(Zona::Indice);
        i.pintando.set(true);
        i.texto.set_text("");
        i.pintando.set(false);
        self.ver_categoria(cat, seccion.map(|s| (s, None)));
        self.pintar_indice(Some(match seccion {
            Some(s) => Entrada::Seccion(cat, s),
            None => Entrada::Categoria(cat),
        }));
        if !ya_abierto {
            i.window.set_visible(true);
        }
        i.texto.grab_focus();
    }

    pub fn cerrar(&self) {
        let i = &self.inner;
        if !i.abierto.replace(false) {
            return;
        }
        self.dejar_de_grabar();
        if let Some(s) = i.seguir.take() {
            s.remove();
        }
        i.mezclador.cerrar();
        i.window.set_visible(false);
    }

    fn conectar(&self) {
        let i = &self.inner;

        let yo = self.clone();
        i.window.connect_map(move |w| {
            let Some(superficie) = w.surface() else {
                return;
            };
            let Some(monitor) = WidgetExt::display(w).monitor_at_surface(&superficie) else {
                return;
            };
            let g = monitor.geometry();
            let (ancho, alto) = arbol::tamano(g.width(), g.height());
            yo.inner.tarjeta.set_size_request(ancho, alto);
        });

        let teclas = gtk::EventControllerKey::new();
        teclas.set_propagation_phase(gtk::PropagationPhase::Capture);
        let yo = self.clone();
        teclas.connect_key_pressed(move |_, tecla, codigo, estado| {
            if yo.tecla_grabada(tecla, codigo, estado) || yo.tecla(tecla) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        i.window.add_controller(teclas);

        let click = gtk::GestureClick::new();
        let yo = self.clone();
        click.connect_released(move |g, _, x, y| {
            let t = &yo.inner.tarjeta;
            let dentro = yo
                .inner
                .window
                .pick(x, y, gtk::PickFlags::DEFAULT)
                .is_some_and(|p| p == *t.upcast_ref::<gtk::Widget>() || p.is_ancestor(t));
            if !dentro {
                g.set_state(gtk::EventSequenceState::Claimed);
                yo.cerrar();
            }
        });
        i.window.add_controller(click);

        let yo = self.clone();
        i.texto.connect_changed(move |_| {
            let i = &yo.inner;
            if !i.abierto.get() || i.pintando.get() {
                return;
            }
            let buscando = !i.texto.text().trim().is_empty();
            i.zona.set(Zona::Indice);
            yo.marcar_elegido(None);
            // Al borrar la búsqueda, el índice vuelve a la categoría que se está viendo.
            let actual = Entrada::Categoria(i.categoria.get());
            yo.pintar_indice((!buscando).then_some(actual));
        });

        // El índice cambia solo con un click o el teclado: pasar el puntero no saca de donde se
        // estaba.
        let yo = self.clone();
        i.indice.connect_row_selected(move |_, r| {
            if let Some(r) = r
                && !yo.inner.pintando.get()
            {
                yo.navegar(r.index());
            }
        });

        // Sonido se relee cuando cambian las salidas (no en cada paso de volumen).
        let yo = self.clone();
        i.mezclador.al_cambiar(move || {
            let i = &yo.inner;
            let ahora: Vec<(String, String)> = i
                .mezclador
                .estado()
                .salidas
                .into_iter()
                .map(|s| (s.nombre, s.descripcion))
                .collect();
            if *i.salidas.borrow() != ahora {
                i.salidas.replace(ahora);
                yo.pedir(Categoria::Sonido);
            }
        });

        let yo = self.clone();
        i.scroll_pagina
            .vadjustment()
            .connect_value_changed(move |_| yo.marcar_a_la_vista());
    }

    fn tecla(&self, tecla: gdk::Key) -> bool {
        let i = &self.inner;
        let en_pagina = i.zona.get() == Zona::Pagina;
        // Escribiendo en una caja de la página (un nombre), las teclas son de ella; Escape la
        // deja y vuelve al buscador.
        if let Some(f) = GtkWindowExt::focus(&i.window)
            && f.is::<gtk::Text>()
            && f != *i.texto.upcast_ref::<gtk::Widget>()
        {
            if tecla != gdk::Key::Escape {
                return false;
            }
            i.texto.grab_focus();
            return true;
        }
        // Con algo tipeado, Izquierda y Derecha mueven el cursor del buscador.
        let tipeando = !en_pagina && !i.texto.text().is_empty();
        match tecla {
            gdk::Key::Left | gdk::Key::Right if tipeando => return false,
            gdk::Key::Escape if en_pagina => self.a_la_zona(Zona::Indice),
            gdk::Key::Escape if !i.texto.text().is_empty() => i.texto.set_text(""),
            gdk::Key::Escape => self.cerrar(),
            gdk::Key::Left | gdk::Key::ISO_Left_Tab if en_pagina => self.a_la_zona(Zona::Indice),
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter | gdk::Key::space
                if en_pagina =>
            {
                let e = i.elegido.borrow().clone();
                if let Some(b) = e.and_then(|w| w.downcast::<gtk::Button>().ok()) {
                    b.emit_clicked();
                }
            }
            gdk::Key::Down | gdk::Key::Tab if en_pagina => self.mover_en_pagina(1),
            gdk::Key::Up if en_pagina => self.mover_en_pagina(-1),
            gdk::Key::Page_Down | gdk::Key::Page_Up => {
                let adj = i.scroll_pagina.vadjustment();
                let paso = if tecla == gdk::Key::Page_Down {
                    1.0
                } else {
                    -1.0
                };
                adj.set_value(adj.value() + paso * adj.page_size() * 0.9);
            }
            gdk::Key::Right
            | gdk::Key::Tab
            | gdk::Key::Return
            | gdk::Key::KP_Enter
            | gdk::Key::ISO_Enter => self.a_la_zona(Zona::Pagina),
            gdk::Key::Left => {
                // De una sección, a su categoría.
                if let Some(Entrada::Seccion(c, _)) = self.entrada_elegida() {
                    self.elegir_entrada(&Entrada::Categoria(c));
                }
            }
            gdk::Key::Down => self.mover_en_indice(1),
            gdk::Key::Up | gdk::Key::ISO_Left_Tab => self.mover_en_indice(-1),
            _ => return false,
        }
        true
    }

    fn a_la_zona(&self, zona: Zona) {
        let i = &self.inner;
        i.zona.set(zona);
        match zona {
            Zona::Indice => self.marcar_elegido(None),
            Zona::Pagina => {
                // Lo primero que se toca de la sección (o la fila) elegida en el índice.
                let destino = match self.entrada_elegida() {
                    Some(Entrada::Seccion(_, s)) => self.fila(s, Some(0)),
                    Some(Entrada::Resultado(r)) => self.fila(r.seccion, Some(r.fila.unwrap_or(0))),
                    _ => None,
                };
                let elegibles = self.elegibles();
                let primero = destino
                    .and_then(|d| {
                        elegibles
                            .iter()
                            .find(|e| *e == &d || e.is_ancestor(&d))
                            .cloned()
                    })
                    .or_else(|| elegibles.first().cloned());
                if primero.is_none() {
                    i.zona.set(Zona::Indice);
                }
                self.marcar_elegido(primero);
            }
        }
    }

    fn entrada_elegida(&self) -> Option<Entrada> {
        let i = &self.inner;
        let r = i.indice.selected_row()?;
        i.entradas
            .borrow()
            .get(usize::try_from(r.index()).ok()?)
            .cloned()
    }

    fn elegir_entrada(&self, e: &Entrada) {
        let i = &self.inner;
        let posicion = i.entradas.borrow().iter().position(|x| x == e);
        if let Some(r) = posicion.and_then(|p| i.indice.row_at_index(p as i32)) {
            i.indice.select_row(Some(&r));
        }
    }

    fn mover_en_indice(&self, paso: i32) {
        let i = &self.inner;
        let n = i.entradas.borrow().len() as i32;
        if n == 0 {
            return;
        }
        let actual = i.indice.selected_row().map_or(-1, |r| r.index());
        let nueva = (actual + paso).rem_euclid(n);
        if let Some(r) = i.indice.row_at_index(nueva) {
            i.indice.select_row(Some(&r));
        }
    }

    fn navegar(&self, posicion: i32) {
        let i = &self.inner;
        let Some(e) = usize::try_from(posicion)
            .ok()
            .and_then(|p| i.entradas.borrow().get(p).cloned())
        else {
            return;
        };
        if let Some(r) = i.indice.row_at_index(posicion) {
            mostrar_en(&i.scroll_indice, &i.indice, r.upcast_ref());
        }
        i.seguido.replace(Some(e.clone()));
        match e {
            Entrada::Categoria(c) => {
                if c != i.categoria.get() {
                    self.ver_categoria(c, None);
                    self.pintar_indice(Some(Entrada::Categoria(c)));
                } else {
                    self.pedir_ir_a(0, None, true);
                }
            }
            Entrada::Seccion(c, s) => {
                if c != i.categoria.get() {
                    self.ver_categoria(c, Some((s, None)));
                } else {
                    self.pedir_ir_a(s, None, true);
                }
            }
            Entrada::Resultado(r) => {
                if r.categoria != i.categoria.get() {
                    self.ver_categoria(r.categoria, Some((r.seccion, r.fila)));
                } else {
                    self.pedir_ir_a(r.seccion, r.fila, true);
                }
            }
        }
    }

    fn pintar_indice(&self, elegir: Option<Entrada>) {
        let i = &self.inner;
        let consulta = i.texto.text().to_string();
        let buscando = !consulta.trim().is_empty();
        let mut desplegar = false;
        let entradas: Vec<Entrada> = if buscando {
            let paginas = i.paginas.borrow();
            let leidas: Vec<(Categoria, &[Seccion])> = Categoria::TODAS
                .iter()
                .filter_map(|c| paginas.get(c).map(|p| (*c, &p[..])))
                .collect();
            arbol::buscar(&leidas, &consulta)
                .into_iter()
                .map(Entrada::Resultado)
                .collect()
        } else {
            let abierta = i.categoria.get();
            desplegar = i
                .indice_abierta
                .replace(Some(abierta))
                .is_some_and(|a| a != abierta);
            let mut v = Vec::new();
            for c in Categoria::TODAS {
                v.push(Entrada::Categoria(c));
                if c == abierta && c.secciones().len() > 1 {
                    v.extend((0..c.secciones().len()).map(|s| Entrada::Seccion(c, s)));
                }
            }
            v
        };
        let color = color_hex(&i.resaltado.color());
        i.pintando.set(true);
        i.indice.unselect_all();
        i.indice.remove_all();
        for e in &entradas {
            i.indice
                .append(&fila_indice(e, &consulta, &color, desplegar));
        }
        let posicion = elegir
            .and_then(|el| entradas.iter().position(|e| *e == el))
            .unwrap_or(0);
        let hay = !entradas.is_empty();
        i.entradas.replace(entradas);
        let fila = i.indice.row_at_index(posicion as i32);
        if let Some(r) = &fila {
            i.indice.select_row(Some(r));
        }
        i.pintando.set(false);
        self.marcar_a_la_vista();
        if hay && buscando {
            self.seguir_busqueda();
        }
        if let Some(r) = fila {
            let yo = self.clone();
            glib::idle_add_local_once(move || {
                let i = &yo.inner;
                mostrar_en(&i.scroll_indice, &i.indice, r.upcast_ref());
            });
        }
    }

    /// Rehacer una página como Apariencia en cada tecla traba.
    fn seguir_busqueda(&self) {
        let i = &self.inner;
        if let Some(s) = i.seguir.take() {
            s.remove();
        }
        let yo = self.clone();
        let id = glib::timeout_add_local_once(SEGUIR, move || {
            let i = &yo.inner;
            i.seguir.take();
            let Some(e) = yo.entrada_elegida() else {
                return;
            };
            if i.seguido.borrow().as_ref() == Some(&e) {
                return;
            }
            if let Some(r) = i.indice.selected_row() {
                yo.navegar(r.index());
            }
        });
        i.seguir.replace(Some(id));
    }

    fn pedir(&self, cat: Categoria) {
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move { yo.refrescar(cat, None).await });
    }

    /// Si llega tarde (se pidió otra lectura de la misma página), se descarta.
    async fn refrescar(&self, cat: Categoria, elegido: Option<usize>) {
        let i = &self.inner;
        let revision = i.pedidos.get() + 1;
        i.pedidos.set(revision);
        i.revisiones.borrow_mut().insert(cat, revision);
        let secciones = self.leer(cat).await;
        if i.revisiones.borrow().get(&cat) != Some(&revision) {
            return;
        }
        let antes = i.paginas.borrow_mut().insert(cat, secciones);
        let cambio = antes.as_ref() != i.paginas.borrow().get(&cat);
        if !i.abierto.get() || !cambio {
            return;
        }
        if i.categoria.get() == cat {
            if antes.is_none() {
                self.pintar_pagina(true);
            } else {
                self.repintar(elegido);
            }
        }
        // Una búsqueda incluye lo que llegó.
        if !i.texto.text().trim().is_empty() {
            let elegida = self.entrada_elegida();
            self.pintar_indice(elegida);
        }
    }

    async fn leer(&self, cat: Categoria) -> Vec<Seccion> {
        let c = self.inner.config.borrow().clone();
        match cat {
            Categoria::Shell
            | Categoria::Pantalla
            | Categoria::Notificaciones => {
                let apps = self.inner.s.notificaciones.apps();
                let mut p = arbol::pagina_fija(cat, &c, &apps).unwrap_or_default();
                if cat == Categoria::Pantalla
                    && let Some(m) = p.first_mut()
                {
                    let mut salidas = monitores::de_json(&leer(&["hyprctl", "-j", "monitors", "all"]).await);
                    let reglas = monitores::Reglas::leer(&self.ruta_monitores()).unwrap_or_else(|e| {
                        eprintln!("morgan: menú: {e}");
                        Default::default()
                    });
                    let ruta = glib::user_cache_dir().join("morgan/monitores-vistos.toml");
                    let mut vistos = monitores::Vistos::leer(&ruta);
                    if vistos.anotar(&salidas)
                        && let Err(e) = vistos.guardar(&ruta)
                    {
                        eprintln!("morgan: menú: {e}");
                    }
                    monitores::completar(&mut salidas, &reglas, &vistos);
                    m.filas = monitores::filas(&salidas, &reglas, self.inner.prueba.borrow().is_some());
                }
                p
            }
            Categoria::Apariencia => {
                let listas = self.inner.listas.borrow().clone();
                let (lecturas, nuevas) = lectura::apariencia(listas, &c.fondos).await;
                if let Some(n) = nuevas {
                    self.inner.listas.replace(Some(Rc::new(n)));
                }
                arbol::apariencia(&lecturas, &c)
            }
            Categoria::Compositor => lectura::compositor(&self.ruta_cambios_hypr()).await,
            Categoria::Atajos => lectura::atajos(&c.atajos).await,
            Categoria::Aplicaciones => lectura::aplicaciones(&c.capturas),
            Categoria::Sistema => lectura::sistema().await,
            Categoria::Sonido => {
                let salidas = self.inner.salidas.borrow().clone();
                arbol::sonido(&c, &sonidos::temas(), &sonidos::tema_puesto(), &salidas)
            }
        }
    }

    fn ruta_monitores(&self) -> PathBuf {
        crate::hypr_cfg::ruta_monitores(&self.inner.s.ruta_config)
    }

    /// Prueba en vivo: si nadie la mantiene, a los `SEGUNDOS_PRUEBA` vuelve sola, con el menú
    /// abierto o no. Una segunda prueba se suma a la primera y reinicia la cuenta.
    async fn probar_monitor(&self, salida: String, campo: monitores::Campo) -> Result<(), String> {
        comando::salida(&["hyprctl", "eval", &monitores::prueba(&salida, &campo)]).await?;
        let i = &self.inner;
        let mut cambios = i.prueba.borrow_mut().take().map(|(c, id)| {
            id.remove();
            c
        });
        cambios.get_or_insert_with(Vec::new).push((salida, campo));
        let yo = self.clone();
        let id = glib::timeout_add_seconds_local_once(monitores::SEGUNDOS_PRUEBA, move || {
            yo.inner.prueba.borrow_mut().take();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = yo.volver_monitores().await {
                    eprintln!("morgan: menú: {e}");
                }
                yo.refrescar(Categoria::Pantalla, None).await;
            });
        });
        i.prueba.replace(cambios.map(|c| (c, id)));
        Ok(())
    }

    async fn mantener_monitores(&self) -> Result<(), String> {
        let Some((cambios, id)) = self.inner.prueba.borrow_mut().take() else {
            return Ok(());
        };
        id.remove();
        let ruta = self.ruta_monitores();
        let mut r = monitores::Reglas::leer(&ruta)?;
        for (salida, campo) in &cambios {
            r.poner(salida, campo);
        }
        r.guardar(&ruta)?;
        self.escribir_hypr(None).await
    }

    /// Lo de los archivos vuelve con la recarga: la prueba no se guardó en ninguno.
    async fn volver_monitores(&self) -> Result<(), String> {
        if let Some((_, id)) = self.inner.prueba.borrow_mut().take() {
            id.remove();
        }
        comando::salida(&["hyprctl", "reload", "config-only"]).await.map(drop)
    }

    async fn guardar_monitor(&self, salida: &str, campo: &monitores::Campo) -> Result<(), String> {
        let ruta = self.ruta_monitores();
        let mut r = monitores::Reglas::leer(&ruta)?;
        r.poner(salida, campo);
        r.guardar(&ruta)?;
        // El nombre es solo de morgan: Hyprland no tiene nada que releer.
        if matches!(campo, monitores::Campo::Apodo(_)) {
            return Ok(());
        }
        self.escribir_hypr(None).await
    }

    fn ruta_cambios_hypr(&self) -> PathBuf {
        crate::hypr_cfg::ruta_cambios(&self.inner.s.ruta_config)
    }

    fn ver_categoria(&self, cat: Categoria, ir_a: Option<(usize, Option<usize>)>) {
        let i = &self.inner;
        i.categoria.set(cat);
        i.ir_a.set(Some(ir_a.unwrap_or((0, None))));
        self.pintar_pagina(true);
    }

    fn pintar_pagina(&self, entra: bool) {
        let i = &self.inner;
        let cat = i.categoria.get();
        let pagina = &i.pagina;
        while let Some(h) = pagina.first_child() {
            pagina.remove(&h);
        }
        if let Some(t) = i.deslizando.take() {
            t.remove();
        }
        let agregar = |w: &gtk::Widget| {
            if entra {
                w.add_css_class("entra");
            }
            pagina.append(w);
        };
        i.cabeceras.borrow_mut().clear();
        i.filas.borrow_mut().clear();
        i.elegido.replace(None);
        i.desplegables.replace(desplegable::Grupo::default());

        let titulo = gtk::Label::new(Some(cat.titulo()));
        titulo.add_css_class("menu-titulo");
        titulo.set_xalign(0.0);
        agregar(titulo.upcast_ref());

        let paginas = i.paginas.borrow();
        let Some(secciones) = paginas.get(&cat) else {
            let leyendo = gtk::Label::new(Some("Leyendo…"));
            leyendo.add_css_class("menu-leyendo");
            leyendo.set_xalign(0.0);
            agregar(leyendo.upcast_ref());
            return;
        };
        let grupo = i.desplegables.borrow().clone();
        for s in secciones {
            let cabecera = gtk::Label::new(Some(s.titulo));
            cabecera.add_css_class("menu-seccion");
            cabecera.set_xalign(0.0);
            agregar(cabecera.upcast_ref());
            i.cabeceras.borrow_mut().push(cabecera.upcast());
            let mut filas = Vec::new();
            for f in &s.filas {
                let w = self.widget_fila(f, &grupo);
                agregar(&w);
                filas.push(w);
            }
            i.filas.borrow_mut().push(filas);
        }
        drop(paginas);

        if let Some((s, f)) = i.ir_a.take() {
            self.pedir_ir_a(s, f, false);
        }
    }

    /// Se espera al cuadro siguiente, no a un idle: el idle puede correr antes de que GTK mida la
    /// página nueva, y el scroll todavía tiene el tope de la anterior. El tick de un cuadro corre
    /// antes de su layout; en el segundo, la página ya está medida.
    fn pedir_ir_a(&self, seccion: usize, fila: Option<usize>, suave: bool) {
        let i = &self.inner;
        i.ir_suave.set(suave);
        if i.ir_a.replace(Some((seccion, fila))).is_some() {
            return;
        }
        let yo = self.clone();
        let cuadros = Cell::new(0);
        i.scroll_pagina.add_tick_callback(move |_, _| {
            cuadros.set(cuadros.get() + 1);
            if cuadros.get() < 2 {
                return glib::ControlFlow::Continue;
            }
            if let Some((s, f)) = yo.inner.ir_a.take() {
                yo.ir_a(s, f);
            }
            glib::ControlFlow::Break
        });
    }

    fn ir_a(&self, seccion: usize, fila: Option<usize>) {
        let i = &self.inner;
        let adj = i.scroll_pagina.vadjustment();
        let destino = self.fila(seccion, fila);
        let suave = i.ir_suave.replace(false);
        let ir = |v: f64| {
            if suave {
                self.deslizar(v);
            } else {
                adj.set_value(v);
            }
        };
        let Some(destino) = destino.filter(|_| fila.is_some() || seccion > 0) else {
            ir(0.0);
            return;
        };
        if let Some(b) = destino.compute_bounds(&i.pagina) {
            // Con aire arriba: el título de la sección queda a la vista.
            let aire = if fila.is_some() { 40.0 } else { 8.0 };
            let tope = (adj.upper() - adj.page_size()).max(0.0);
            ir((b.y() as f64 - aire).clamp(0.0, tope));
        }
        if fila.is_some() {
            destino.add_css_class("resalte");
            let d = destino.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(900), move || {
                d.remove_css_class("resalte")
            });
        }
    }

    /// Un desliz nuevo corta el anterior donde esté.
    fn deslizar(&self, destino: f64) {
        let i = &self.inner;
        if let Some(t) = i.deslizando.take() {
            t.remove();
        }
        let adj = i.scroll_pagina.vadjustment();
        let desde = adj.value();
        if (destino - desde).abs() < 1.0 {
            adj.set_value(destino);
            return;
        }
        let inicio = Cell::new(None::<i64>);
        let inner = Rc::downgrade(&self.inner);
        let id = i.scroll_pagina.add_tick_callback(move |_, reloj| {
            let ahora = reloj.frame_time();
            let t0 = inicio.get().unwrap_or(ahora);
            inicio.set(Some(t0));
            let t =
                ((ahora - t0) as f64 / crate::animaciones::dur(DESLIZ).as_micros() as f64).min(1.0);
            let avance = 1.0 - (1.0 - t).powi(3);
            adj.set_value(desde + (destino - desde) * avance);
            if t < 1.0 {
                return glib::ControlFlow::Continue;
            }
            // Terminó solo: el id ya no sirve para sacarlo.
            if let Some(i) = inner.upgrade() {
                i.deslizando.take();
            }
            glib::ControlFlow::Break
        });
        i.deslizando.replace(Some(id));
    }

    fn fila(&self, seccion: usize, fila: Option<usize>) -> Option<gtk::Widget> {
        let i = &self.inner;
        match fila {
            None => i.cabeceras.borrow().get(seccion).cloned(),
            Some(f) => i.filas.borrow().get(seccion)?.get(f).cloned(),
        }
    }

    fn marcar_a_la_vista(&self) {
        let i = &self.inner;
        let adj = i.scroll_pagina.vadjustment();
        let valor = adj.value();
        // Al fondo, la última sección a la vista: puede que su título nunca llegue arriba.
        let al_fondo = valor > 1.0 && valor >= adj.upper() - adj.page_size() - 1.0;
        let limite = if al_fondo {
            valor + adj.page_size() - 24.0
        } else {
            valor + 24.0
        };
        // Arriba de todo es la primera (y una página recién puesta todavía no se midió).
        let actual = if valor < 1.0 {
            0
        } else {
            i.cabeceras
                .borrow()
                .iter()
                .rposition(|c| {
                    c.compute_bounds(&i.pagina)
                        .is_some_and(|b| b.y() as f64 <= limite)
                })
                .unwrap_or(0)
        };
        let cat = i.categoria.get();
        for (n, e) in i.entradas.borrow().iter().enumerate() {
            let Some(r) = i.indice.row_at_index(n as i32) else {
                continue;
            };
            if *e == Entrada::Seccion(cat, actual) {
                r.add_css_class("a-la-vista");
            } else {
                r.remove_css_class("a-la-vista");
            }
        }
    }

    fn widget_fila(&self, f: &Fila, grupo: &desplegable::Grupo) -> gtk::Widget {
        match f {
            Fila::Ajuste {
                titulo,
                sub,
                control,
                nota,
            } => {
                let (boton, derecha) = tarjeta(titulo, sub, None, None);
                boton.add_css_class("menu-ajuste");
                if !nota.is_empty() {
                    poner_nota(&boton, nota);
                }
                let accion = |que: &Que| {
                    let yo = self.clone();
                    let que = que.clone();
                    boton.connect_clicked(move |b| yo.ejecutar(b.upcast_ref(), &que));
                };
                match control {
                    Control::Casilla { activa, que } => {
                        let casilla = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        casilla.add_css_class("casilla");
                        derecha.append(&casilla);
                        if *activa {
                            boton.add_css_class("encendida");
                        }
                        boton.add_css_class("elegible");
                        accion(que);
                        boton.upcast()
                    }
                    Control::Boton(que) => {
                        derecha.append(&flecha("\u{F0142}"));
                        boton.add_css_class("elegible");
                        accion(que);
                        boton.upcast()
                    }
                    Control::Confirmar(que) => {
                        // El primer toque pide confirmar (por 4 s); el segundo lo hace.
                        let aviso = flecha("\u{F0A7A}");
                        derecha.append(&aviso);
                        boton.add_css_class("elegible");
                        boton.add_css_class("peligro");
                        let yo = self.clone();
                        let que = que.clone();
                        let titulo = titulo.clone();
                        boton.connect_clicked(move |b| {
                            if b.has_css_class("confirmando") {
                                // Una vez: un tercer toque vuelve a pedir confirmar.
                                b.remove_css_class("confirmando");
                                if let Some(l) = titulo_de(b.upcast_ref()) {
                                    l.set_label(&glib::markup_escape_text(&titulo));
                                }
                                yo.ejecutar(b.upcast_ref(), &que);
                                return;
                            }
                            titulo_por_un_rato(
                                b.upcast_ref(),
                                "¿Seguro? Tocá de nuevo para borrar",
                                "confirmando",
                            );
                        });
                        boton.upcast()
                    }
                    Control::Texto { valor, pista, que } => {
                        // Como la clave de una Wi-Fi nueva en el centro: la caja del buscador
                        // debajo de la fila, con el foco mientras se escribe.
                        derecha.append(&flecha("\u{F03EB}"));
                        boton.add_css_class("elegible");
                        let caja = gtk::Box::new(gtk::Orientation::Vertical, 0);
                        caja.append(&boton);
                        let entrada = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        entrada.add_css_class("launcher-entrada");
                        entrada.add_css_class("menu-texto");
                        let lapiz = gtk::Label::new(Some("\u{F03EB}"));
                        lapiz.add_css_class("prompt");
                        entrada.append(&lapiz);
                        let texto = gtk::Text::new();
                        texto.set_placeholder_text(Some(pista));
                        texto.set_hexpand(true);
                        entrada.append(&texto);
                        entrada.set_visible(false);
                        caja.append(&entrada);
                        let (yo, e, t, v) =
                            (self.clone(), entrada.clone(), texto.clone(), valor.clone());
                        boton.connect_clicked(move |b| {
                            yo.inner.desplegables.borrow().cerrar();
                            yo.inner.zona.set(Zona::Pagina);
                            yo.marcar_elegido(Some(b.clone().upcast()));
                            e.set_visible(true);
                            t.set_text(&v);
                            t.grab_focus();
                            t.select_region(0, -1);
                        });
                        // Escape lo atiende `tecla`: vuelve el foco al buscador, y salir de la
                        // caja la cierra sin guardar.
                        let salir = gtk::EventControllerFocus::new();
                        let e = entrada.clone();
                        salir.connect_leave(move |_| e.set_visible(false));
                        texto.add_controller(salir);
                        let (yo, b, que, antes) =
                            (self.clone(), boton.downgrade(), que.clone(), valor.clone());
                        texto.connect_activate(move |t| {
                            let nuevo = t.text().trim().to_owned();
                            yo.inner.texto.grab_focus();
                            if let Some(b) = b.upgrade()
                                && nuevo != antes
                            {
                                yo.ejecutar(b.upcast_ref(), &que.con_texto(&nuevo));
                            }
                        });
                        caja.upcast()
                    }
                    Control::Nada => {
                        // Solo se lee: no se toca, pero se lee entero (no atenuado).
                        boton.set_sensitive(false);
                        boton.add_css_class("dato");
                        boton.upcast()
                    }
                    Control::Lista(opciones) => {
                        // El valor puesto ya está en el subtítulo; a la derecha, la flecha.
                        derecha.append(&flecha("\u{F0140}"));
                        boton.add_css_class("elegible");
                        let caja = gtk::Box::new(gtk::Orientation::Vertical, 0);
                        caja.append(&boton);
                        let d = grupo.nueva(&boton);
                        caja.append(&d.revealer);
                        // La opción guarda su fila sin retenerla: con una referencia fuerte, fila → lista → opción
                        // → fila no se liberaría nunca al repintar.
                        let items = opciones
                            .iter()
                            .map(|o| {
                                let yo = self.clone();
                                let que = o.que.clone();
                                let b = boton.downgrade();
                                desplegable::Item::new("", &o.titulo, Some(o.activa), move || {
                                    if let Some(b) = b.upgrade() {
                                        yo.ejecutar(b.upcast_ref(), &que)
                                    }
                                })
                            })
                            .collect();
                        for b in d.llenar(items) {
                            b.add_css_class("elegible");
                            // El foco sigue en el buscador.
                            b.set_can_focus(false);
                        }
                        let yo = self.clone();
                        boton.connect_clicked(move |b| {
                            yo.inner.zona.set(Zona::Pagina);
                            yo.marcar_elegido(Some(b.clone().upcast()));
                            d.alternar();
                        });
                        caja.upcast()
                    }
                }
            }
            Fila::Disposicion(pantallas) => {
                let yo = self.clone();
                dibujo::nuevo(pantallas, move |w, salida, campo| {
                    yo.ejecutar(w, &Que::Monitor(salida.to_owned(), campo))
                })
            }
            Fila::Galeria { titulo, opciones } => {
                let caja = gtk::Box::new(gtk::Orientation::Vertical, 8);
                if let Some(t) = titulo {
                    let l = gtk::Label::new(Some(t));
                    l.add_css_class("menu-subtitulo");
                    l.set_xalign(0.0);
                    caja.append(&l);
                }
                let grilla = gtk::Grid::new();
                grilla.set_column_spacing(8);
                grilla.set_row_spacing(8);
                grilla.set_column_homogeneous(true);
                for (n, o) in opciones.iter().enumerate() {
                    let (boton, derecha) =
                        tarjeta(&o.titulo, &o.sub, o.fuente.as_deref(), o.cursor.as_deref());
                    boton.add_css_class("elegible");
                    if o.activa {
                        // El tilde se lee sin el tema.
                        boton.add_css_class("encendida");
                        let tilde = gtk::Label::new(Some("\u{F012C}"));
                        tilde.add_css_class("tilde");
                        derecha.append(&tilde);
                    }
                    let yo = self.clone();
                    let que = o.que.clone();
                    boton.connect_clicked(move |b| yo.ejecutar(b.upcast_ref(), &que));
                    let n = n as i32;
                    grilla.attach(&boton, n % COLUMNAS, n / COLUMNAS, 1, 1);
                }
                caja.append(&grilla);
                caja.upcast()
            }
        }
    }

    fn elegibles(&self) -> Vec<gtk::Widget> {
        fn juntar(w: &gtk::Widget, v: &mut Vec<gtk::Widget>) {
            if !w.is_visible() || !w.is_child_visible() {
                return;
            }
            if w.has_css_class("elegible") && w.is_sensitive() {
                v.push(w.clone());
                return;
            }
            let mut h = w.first_child();
            while let Some(c) = h {
                juntar(&c, v);
                h = c.next_sibling();
            }
        }
        let mut v = Vec::new();
        juntar(self.inner.pagina.upcast_ref(), &mut v);
        v
    }

    fn mover_en_pagina(&self, paso: i32) {
        let elegibles = self.elegibles();
        if elegibles.is_empty() {
            return;
        }
        let actual = self.inner.elegido.borrow().clone();
        let n = elegibles.len() as i32;
        let nueva = match actual.and_then(|a| elegibles.iter().position(|e| *e == a)) {
            Some(p) => (p as i32 + paso).clamp(0, n - 1),
            None => 0,
        };
        self.marcar_elegido(Some(elegibles[nueva as usize].clone()));
    }

    fn marcar_elegido(&self, w: Option<gtk::Widget>) {
        let i = &self.inner;
        if let Some(antes) = i.elegido.replace(w.clone()) {
            antes.remove_css_class("elegida");
        }
        if let Some(w) = w {
            w.add_css_class("elegida");
            let yo = self.clone();
            glib::idle_add_local_once(move || {
                let i = &yo.inner;
                mostrar_en(&i.scroll_pagina, &i.pagina, &w);
            });
        }
    }

    /// Lo elegido sigue en el mismo lugar después de que la página se relea, para seguir con el
    /// teclado.
    fn ejecutar(&self, desde: &gtk::Widget, que: &Que) {
        let i = &self.inner;
        i.zona.set(Zona::Pagina);
        // Tocar otra cosa con el mouse mientras se graba deja la grabación.
        self.dejar_de_grabar();
        match que {
            Que::GrabarTecla(id) => {
                self.grabar(desde, id);
                return;
            }
            Que::Fondos => {
                self.cerrar();
                let f = i.al_fondos.borrow().clone();
                if let Some(f) = f {
                    f();
                }
                return;
            }
            Que::Ubicacion | Que::Calendario => {
                self.cerrar();
                let modo = if *que == Que::Ubicacion {
                    ModoLauncher::Ciudad
                } else {
                    ModoLauncher::Calendario
                };
                let f = i.al_launcher.borrow().clone();
                if let Some(f) = f {
                    f(modo);
                }
                return;
            }
            Que::Carpetas(actual) => {
                self.examinar_carpeta(actual);
                return;
            }
            Que::Sonar(id) => {
                sonidos::sonar(id, None);
                return;
            }
            _ => {}
        }
        let mut posicion = self.elegibles().iter().position(|e| e == desde);
        // Tras probar un monitor, lo elegido es «Mantener»: la fila tocada ya no está, y lo que
        // quedara en su lugar se tocaría con el próximo Enter.
        if matches!(que, Que::Monitor(_, c) if c.a_prueba()) {
            posicion = Some(0);
        }
        // La página que se relee es la de la fila, aunque se cambie de página mientras.
        let cat = i.categoria.get();
        // Lo marcado cambia recién cuando el sistema lo dice.
        desde.add_css_class("aplicando");
        let fila = desde.downgrade();
        let yo = self.clone();
        let que = que.clone();
        let es_cursor =
            matches!(&que, Que::Apariencia(crate::apariencia::Cambio::Cursor(..)));
        glib::MainContext::default().spawn_local(async move {
            let hecho = yo.aplicar(que).await;
            if let Some(w) = fila.upgrade() {
                w.remove_css_class("aplicando");
                if let Err(e) = &hecho {
                    eprintln!("morgan: menú: {e}");
                    avisar_fallo(&w);
                }
                // GTK pide el cursor por forma (cursor-shape-v1) y no la repite si no cambia;
                // Hyprland 0.56, tras `setcursor`, no redibuja la forma ya pedida. Sin esto el
                // tema nuevo recién se ve al salir del menú. Ida y vuelta a otra forma la
                // vuelve a pedir, y ya sale del tema nuevo.
                if es_cursor && let Some(r) = w.root() {
                    r.set_cursor_from_name(Some("progress"));
                    r.set_cursor(None);
                }
            }
            yo.refrescar(cat, posicion).await;
        });
    }

    /// Hyprland entra al submap sin binds después de leer cuáles teclas están ocupadas.
    fn grabar(&self, fila: &gtk::Widget, id: &str) {
        let i = &self.inner;
        i.desplegables.borrow().cerrar();
        let Some(sub) = etiqueta(fila, "estado") else {
            return;
        };
        let numero = i.grabaciones.get() + 1;
        i.grabaciones.set(numero);
        i.grabando.replace(Some(Grabacion {
            id: id.to_owned(),
            numero,
            fila: fila.downgrade(),
            sub_antes: sub.label().into(),
            ocupadas: None,
            pendiente: None,
            nota: None,
        }));
        sub.set_label("Apretá la combinación · Escape cancela");
        fila.add_css_class("aplicando");
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let ocupadas = comando::salida(&["hyprctl", "-j", "binds"])
                .await
                .and_then(|j| crate::launcher::fuentes::ocupadas(&j))
                .unwrap_or_else(|e| {
                    eprintln!("morgan: menú: {e}");
                    Vec::new()
                });
            let entrar = binds::submap_grabar(true);
            if let Err(e) = comando::salida(&["hyprctl", "eval", &entrar]).await {
                eprintln!("morgan: menú: submap de grabar: {e}");
            }
            let sigue = match &mut *yo.inner.grabando.borrow_mut() {
                Some(g) if g.numero == numero => {
                    g.ocupadas = Some(ocupadas);
                    true
                }
                _ => false,
            };
            // Se canceló mientras entraba: se sale de nuevo.
            if !sigue {
                comando::lanzar(&["hyprctl", "eval", &binds::submap_grabar(false)]);
            }
        });
    }

    fn dejar_de_grabar(&self) {
        let Some(g) = self.inner.grabando.take() else {
            return;
        };
        comando::lanzar(&["hyprctl", "eval", &binds::submap_grabar(false)]);
        if let Some(n) = &g.nota {
            n.unparent();
        }
        if let Some(f) = g.fila.upgrade() {
            f.remove_css_class("aplicando");
            if let Some(sub) = etiqueta(&f, "estado") {
                sub.set_label(&g.sub_antes);
            }
        }
    }

    fn tecla_grabada(&self, tecla: gdk::Key, codigo: u32, estado: gdk::ModifierType) -> bool {
        let i = &self.inner;
        let mods = [
            gdk::ModifierType::SUPER_MASK,
            gdk::ModifierType::CONTROL_MASK,
            gdk::ModifierType::ALT_MASK,
            gdk::ModifierType::SHIFT_MASK,
        ]
        .map(|m| estado.contains(m));
        let sin_mods = !mods.contains(&true);
        let (fila, pendiente, cargada) = match &*i.grabando.borrow() {
            None => return false,
            Some(g) => (g.fila.upgrade(), g.pendiente.clone(), g.ocupadas.is_some()),
        };
        let Some(fila) = fila else {
            // La página se repintó: la fila ya no está.
            self.dejar_de_grabar();
            return true;
        };
        use gdk::Key;
        match tecla {
            Key::Escape if sin_mods => {
                self.dejar_de_grabar();
                return true;
            }
            Key::Return | Key::KP_Enter | Key::ISO_Enter if sin_mods && pendiente.is_some() => {
                let (t, libera) = pendiente.unwrap_or_default();
                self.guardar_tecla(&fila, t, libera);
                return true;
            }
            Key::Shift_L
            | Key::Shift_R
            | Key::Control_L
            | Key::Control_R
            | Key::Alt_L
            | Key::Alt_R
            | Key::Super_L
            | Key::Super_R
            | Key::Meta_L
            | Key::Meta_R
            | Key::Hyper_L
            | Key::Hyper_R
            | Key::ISO_Level3_Shift
            | Key::Caps_Lock
            | Key::Num_Lock => return true,
            _ if !cargada => return true,
            _ => {}
        }
        // Hyprland compara la tecla sin los modificadores puestos (Shift + 1 es `1`, no `!`).
        let base = WidgetExt::display(&fila)
            .translate_key(codigo, gdk::ModifierType::empty(), 0)
            .map_or(tecla, |(k, ..)| k);
        let Some(nombre) = base.name() else {
            return true;
        };
        let nueva = binds::combinacion(mods, &nombre);
        let comparable = binds::normalizar(&nueva);
        let (id, choque) = {
            let g = i.grabando.borrow();
            let Some(g) = g.as_ref() else { return true };
            let choque = g
                .ocupadas
                .iter()
                .flatten()
                .find(|a| binds::normalizar(&a.teclas) == comparable)
                .cloned();
            (g.id.clone(), choque)
        };
        let config = i.config.borrow().atajos.clone();
        let propia = config.tecla(&id).unwrap_or_default();
        if binds::normalizar(&propia) == comparable {
            // La que ya tenía: nada que cambiar.
            self.dejar_de_grabar();
            return true;
        }
        let Some(choque) = choque else {
            self.guardar_tecla(&fila, nueva, None);
            return true;
        };
        // La usa otro: si es de morgan, al reasignar queda sin tecla.
        let libera = binds::propios(&config)
            .into_iter()
            .find(|p| p.id != id && binds::normalizar(&p.tecla) == comparable)
            .map(|p| p.id.to_owned());
        let quien = if choque.descripcion.is_empty() {
            "un bind sin descripción".to_owned()
        } else {
            format!("«{}»", choque.descripcion)
        };
        if let Some(sub) = etiqueta(&fila, "estado") {
            sub.set_label(&nueva);
        }
        let mut g = i.grabando.borrow_mut();
        let Some(g) = g.as_mut() else { return true };
        if let Some(n) = g.nota.take() {
            n.unparent();
        }
        if let Ok(b) = fila.clone().downcast::<gtk::Button>() {
            g.nota = poner_nota(&b, &format!("Ya la usa {quien}: Enter la reasigna, Escape cancela"));
        }
        g.pendiente = Some((nueva, libera));
        true
    }

    fn guardar_tecla(&self, fila: &gtk::Widget, tecla: String, libera: Option<String>) {
        let Some(g) = self.inner.grabando.borrow().as_ref().map(|g| g.id.clone()) else {
            return;
        };
        let mut v = vec![(g, tecla)];
        v.extend(libera.map(|l| (l, String::new())));
        // `ejecutar` deja de grabar (y sale del submap) antes de aplicar.
        self.ejecutar(fila, &Que::Config(arbol::Cambio::Teclas(v)));
    }

    /// El `Err` va al journal, y la fila lo avisa.
    async fn aplicar(&self, que: Que) -> Result<(), String> {
        match que {
            Que::Aplicar(args) => {
                let a: Vec<&str> = args.iter().map(String::as_str).collect();
                comando::salida(&a).await.map(drop)
            }
            Que::Config(c) => {
                self.cambiar_config(&c)?;
                match c {
                    arbol::Cambio::Capturas(_)
                    | arbol::Cambio::Sombra(_)
                    | arbol::Cambio::Teclas(_)
                    | arbol::Cambio::Multimedia(_) => {
                        return self.escribir_hypr(None).await;
                    }
                    arbol::Cambio::Animacion(_) => return self.escribir_hypr(None).await,
                    // El shell sigue a la velocidad: el ritmo y la hoja con sus tiempos.
                    arbol::Cambio::Velocidad(id) => {
                        crate::animaciones::poner(&id);
                        let i = &self.inner;
                        i.s.tema.set_shell(&i.config.borrow().shell);
                        return self.escribir_hypr(None).await;
                    }
                    _ => {}
                }
                despues_de_cambiar(&c).await;
                Ok(())
            }
            Que::Hypr(clave, valor) => self.escribir_hypr(Some((clave, valor))).await,
            Que::Monitor(salida, campo) if campo.a_prueba() => self.probar_monitor(salida, campo).await,
            Que::Monitor(salida, campo) => self.guardar_monitor(&salida, &campo).await,
            Que::MantenerMonitores => self.mantener_monitores().await,
            Que::VolverMonitores => self.volver_monitores().await,
            Que::Apariencia(c) => crate::apariencia::aplicar(c).await,
            Que::Predeterminada(id, mimes) => {
                let app = gio_unix::DesktopAppInfo::new(&id).ok_or(format!("no existe {id}"))?;
                mimes.iter().try_for_each(|m| {
                    app.set_as_default_for_type(m)
                        .map_err(|e| format!("{id} para {m}: {e}"))
                })
            }
            Que::ElegirTemaSonido(carpeta, demo) => {
                sonidos::poner_tema(&carpeta);
                sonidos::sonar(&demo, Some(&carpeta));
                Ok(())
            }
            Que::BorrarRecientes => gtk::RecentManager::default()
                .purge_items()
                .map(drop)
                .map_err(|e| format!("recientes: {e}")),
            Que::Autoarranque(archivo, activar) => lectura::poner_autoarranque(&archivo, activar)
                .map_err(|e| format!("autoarranque de {archivo}: {e}")),
            Que::Fondos
            | Que::Ubicacion
            | Que::Calendario
            | Que::Carpetas(_)
            | Que::Sonar(_)
            | Que::GrabarTecla(_) => Ok(()),
        }
    }

    /// Va sin padre y con el menú cerrado: una capa overlay con el teclado exclusivo lo taparía y
    /// le robaría las teclas.
    fn examinar_carpeta(&self, actual: &std::path::Path) {
        self.cerrar();
        let yo = self.clone();
        let actual = actual.to_owned();
        glib::MainContext::default().spawn_local(async move {
            let dialogo = gtk::FileDialog::new();
            dialogo.set_title("Carpeta de fondos");
            dialogo.set_accept_label(Some("Elegir"));
            if actual.is_dir() {
                dialogo.set_initial_folder(Some(&gtk::gio::File::for_path(&actual)));
            }
            let elegida = dialogo.select_folder_future(gtk::Window::NONE).await;
            if let Some(ruta) = elegida.ok().and_then(|f| f.path()) {
                let c = arbol::Cambio::CarpetaFondos(ruta.to_string_lossy().into_owned());
                if let Err(e) = yo.cambiar_config(&c) {
                    eprintln!("morgan: menú: {e}");
                }
            }
            yo.abrir(NivelMenu::Fondos);
        });
    }

    async fn escribir_hypr(
        &self,
        cambio: Option<(String, Option<compositor::Valor>)>,
    ) -> Result<(), String> {
        let ruta = self.ruta_cambios_hypr();
        if let Some((clave, valor)) = cambio {
            let mut c = compositor::Cambios::leer(&ruta)?;
            c.poner(&clave, valor);
            c.guardar(&ruta)?;
        }
        let config = self.inner.config.borrow().clone();
        crate::hypr_cfg::aplicar(&config, &self.inner.s.ruta_config).await
    }

    fn cambiar_config(&self, cambio: &arbol::Cambio) -> Result<(), String> {
        let i = &self.inner;
        let mut c = i.config.borrow().clone();
        cambio.aplicar(&mut c);
        config::guardar(&i.s.ruta_config, &c).map_err(|e| e.to_string())?;
        // Lo que vale es lo que quedó en el archivo, releído.
        let c = config::load(&i.s.ruta_config).map_err(|e| e.to_string())?;
        i.s.tema.set_shell(&c.shell);
        i.s.notificaciones.aplicar_config(&c);
        i.s.idle.aplicar_config(&c.idle);
        i.s.audio.aplicar_config(&c.sonido);
        i.s.sonidos.aplicar_config(&c.sonido);
        i.s.luz.aplicar_config(&c.luz);
        i.s.fondos.aplicar_config(&c.fondos);
        i.s.calendario.aplicar_config(&c.clima);
        i.s.centro.aplicar_config(&c.sonido);
        i.config.replace(c);
        Ok(())
    }

    fn repintar(&self, elegido: Option<usize>) {
        let i = &self.inner;
        let elegido = elegido.or_else(|| {
            let e = i.elegido.borrow().clone()?;
            self.elegibles().iter().position(|x| *x == e)
        });
        let valor = i.scroll_pagina.vadjustment().value();
        self.pintar_pagina(false);
        let yo = self.clone();
        glib::idle_add_local_once(move || {
            let i = &yo.inner;
            i.scroll_pagina.vadjustment().set_value(valor);
            if let Some(p) = elegido
                && i.zona.get() == Zona::Pagina
            {
                let e = yo.elegibles();
                yo.marcar_elegido(e.get(p).or(e.last()).cloned());
            }
        });
    }
}

async fn despues_de_cambiar(c: &arbol::Cambio) {
    use arbol::Cambio::*;
    match c {
        // Regenera el tema si el activo es el del wallpaper; si no, no hace nada.
        MatugenTipo(_) | MatugenModo(_) | MatugenContraste(_) => {
            leer(&["morganctl", "theme", "wallpaper"]).await;
        }
        _ => {}
    }
}

fn titulo_de(w: &gtk::Widget) -> Option<gtk::Label> {
    etiqueta(w, "titulo")
}

fn etiqueta(w: &gtk::Widget, clase: &str) -> Option<gtk::Label> {
    if w.has_css_class(clase)
        && let Some(l) = w.downcast_ref::<gtk::Label>()
    {
        return Some(l.clone());
    }
    let mut h = w.first_child();
    while let Some(c) = h {
        if let Some(l) = etiqueta(&c, clase) {
            return Some(l);
        }
        h = c.next_sibling();
    }
    None
}

/// Las fuentes llevan el título con markup: se vuelve a poner tal cual estaba.
fn titulo_por_un_rato(fila: &gtk::Widget, texto: &str, clase: &'static str) {
    let Some(l) = titulo_de(fila) else { return };
    let antes = l.label();
    let escapado = glib::markup_escape_text(texto);
    l.set_label(if l.uses_markup() { &escapado } else { texto });
    fila.add_css_class(clase);
    let f = fila.downgrade();
    glib::timeout_add_local_once(AVISO, move || {
        if let Some(f) = f.upgrade() {
            f.remove_css_class(clase);
            l.set_label(&antes);
        }
    });
}

fn avisar_fallo(fila: &gtk::Widget) {
    titulo_por_un_rato(fila, "No se pudo: el detalle está en el journal", "fallo");
}

fn flecha(glifo: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(glifo));
    l.add_css_class("flecha");
    l
}

fn mostrar_en(scroll: &gtk::ScrolledWindow, contenido: &impl IsA<gtk::Widget>, w: &gtk::Widget) {
    let Some(b) = w.compute_bounds(contenido) else {
        return;
    };
    let adj = scroll.vadjustment();
    let (arriba, abajo) = (b.y() as f64, (b.y() + b.height()) as f64);
    if arriba < adj.value() {
        adj.set_value(arriba);
    } else if abajo > adj.value() + adj.page_size() {
        adj.set_value(abajo - adj.page_size());
    }
}

fn fila_indice(e: &Entrada, consulta: &str, color: &str, desplegar: bool) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_can_focus(false);
    let caja = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let renglones: Vec<(String, bool)> = match e {
        Entrada::Categoria(c) => {
            let g = gtk::Label::new(Some(&c.glifo().to_string()));
            g.add_css_class("glifo");
            caja.append(&g);
            row.add_css_class("categoria");
            vec![(c.titulo().into(), false)]
        }
        Entrada::Seccion(c, s) => {
            row.add_css_class("seccion");
            vec![(morgan_idioma::t(c.secciones()[*s]).into(), false)]
        }
        Entrada::Resultado(r) => {
            row.add_css_class("resultado");
            vec![(r.titulo.clone(), false), (r.donde.clone(), true)]
        }
    };
    let textos = gtk::Box::new(gtk::Orientation::Vertical, 0);
    textos.set_hexpand(true);
    textos.set_valign(gtk::Align::Center);
    for (t, sub) in renglones {
        let l = gtk::Label::new(None);
        l.set_xalign(0.0);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        l.set_single_line_mode(true);
        let m = resaltar_markup(&t, consulta, color);
        if sub {
            l.set_markup(&format!("<span size=\"small\" alpha=\"55%\">{m}</span>"));
        } else {
            l.set_markup(&m);
        }
        textos.append(&l);
    }
    caja.append(&textos);
    if let Entrada::Seccion(..) = e {
        let r = gtk::Revealer::new();
        r.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        r.set_transition_duration(crate::animaciones::ms(DESPLIEGUE));
        r.set_child(Some(&caja));
        if desplegar {
            // Recién mapeado: antes, el revelador salta sin animar.
            r.connect_map(|r| r.set_reveal_child(true));
        } else {
            r.set_reveal_child(true);
        }
        row.set_child(Some(&r));
    } else {
        row.set_child(Some(&caja));
    }
    row
}

fn tarjeta(
    titulo: &str,
    sub: &str,
    fuente: Option<&str>,
    cursor: Option<&[cursores::Muestra]>,
) -> (gtk::Button, gtk::Box) {
    let caja = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let textos = gtk::Box::new(gtk::Orientation::Vertical, 2);
    textos.set_hexpand(true);
    textos.set_valign(gtk::Align::Center);
    let t = gtk::Label::new(None);
    t.add_css_class("titulo");
    t.set_xalign(0.0);
    t.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let escapado = glib::markup_escape_text(titulo);
    match fuente {
        Some(f) => t.set_markup(&format!(
            "<span font_family=\"{}\">{escapado}</span>",
            glib::markup_escape_text(f)
        )),
        None => t.set_markup(&escapado),
    }
    textos.append(&t);
    if let Some(formas) = cursor {
        textos.append(&muestras_cursor(formas));
    } else if !sub.is_empty() {
        let s = gtk::Label::new(Some(sub));
        s.add_css_class("estado");
        s.set_xalign(0.0);
        s.set_ellipsize(gtk::pango::EllipsizeMode::End);
        textos.append(&s);
    }
    caja.append(&textos);
    let derecha = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    derecha.set_valign(gtk::Align::Center);
    caja.append(&derecha);
    let b = gtk::Button::new();
    b.set_child(Some(&caja));
    b.add_css_class("cc-tarjeta");
    b.set_can_focus(false);
    (b, derecha)
}

/// En varios renglones si no entra: una advertencia cortada no sirve.
fn poner_nota(boton: &gtk::Button, texto: &str) -> Option<gtk::Label> {
    let textos = boton
        .child()
        .and_then(|caja| caja.first_child())
        .and_downcast::<gtk::Box>()?;
    let l = gtk::Label::new(Some(&format!("\u{F0026}  {texto}")));
    l.add_css_class("estado");
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    textos.append(&l);
    Some(l)
}

fn muestras_cursor(formas: &[cursores::Muestra]) -> gtk::Box {
    let tira = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    tira.add_css_class("estado");
    let alto = lectura::ALTO_MUESTRA as i32;
    for m in formas {
        let textura = gdk::MemoryTexture::new(
            m.ancho as i32,
            m.alto as i32,
            gdk::MemoryFormat::B8g8r8a8Premultiplied,
            &glib::Bytes::from(&m.bgra[..]),
            m.ancho as usize * 4,
        );
        let p = gtk::Picture::for_paintable(&textura);
        p.set_content_fit(gtk::ContentFit::Contain);
        p.set_can_shrink(true);
        p.set_size_request((alto * m.ancho as i32 + m.alto as i32 - 1) / m.alto as i32, alto);
        tira.append(&p);
    }
    tira
}

