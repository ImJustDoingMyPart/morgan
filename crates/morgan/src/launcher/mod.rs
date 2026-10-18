//! La tarjeta mide siempre lo mismo: que cambie de tamaño al tipear marea más que un hueco.
//!
//! Las apps se lanzan en su propia unidad (`uwsm app`, o un scope propio sin uwsm): hijas de
//! morgan, morirían con él.

pub(crate) mod busqueda;
pub mod binds;
mod foco;
pub mod fuentes;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use morgan_ipc::ModoLauncher as Modo;

use crate::bajada::Bajada;
use crate::{comando, icono};
use busqueda::{Entrada, Historial, Orden};
use foco::{Foco, Paso, Scroll};

/// De borde a borde del anillo. El tamaño pedido incluye el margin de la tarjeta (el lugar
/// del glow) y el anillo, que es un borde.
const ANCHO: i32 = 600;
const MARGEN: i32 = 8;

/// Una fila es el ícono de 24 px más 6 px de padding arriba y abajo, y entre filas van 4 px.
const FILAS: i32 = 6;
const ALTO_FILA: i32 = 36;
const ESPACIO: i32 = 4;

/// Lo que se espera a que el foco vuelva a la ventana de antes para escribirle un símbolo o
/// correr un atajo.
const DEVOLVER_FOCO: Duration = Duration::from_millis(250);

const PAUSA_CIUDAD: Duration = Duration::from_millis(300);

const ARCHIVOS_SIMBOLOS: &[&str] = &[
    "emojis_smileys_emotion",
    "emojis_people_body",
    "emojis_animals_nature",
    "emojis_food_drink",
    "emojis_activities",
    "emojis_travel_places",
    "emojis_objects",
    "emojis_symbols",
    "emojis_flags",
];

#[derive(Clone)]
pub struct Launcher {
    inner: Rc<Inner>,
}

struct Inner {
    window: gtk::ApplicationWindow,
    tarjeta: gtk::Box,
    bajada: Bajada,
    prompt: gtk::Label,
    texto: gtk::Text,
    lista: gtk::ListBox,
    foco: Foco,
    desliz: Scroll,
    /// Solo da el color del resaltado: Pango no sabe de roles de matugen, así que se le pregunta
    /// a GTK qué color resolvió la hoja.
    resaltado: gtk::Label,

    modo: Cell<Modo>,
    abierto: Cell<bool>,
    apps: RefCell<Catalogo>,
    apps_sucias: Cell<bool>,
    simbolos: RefCell<Catalogo>,
    portapapeles: RefCell<Catalogo>,
    atajos: RefCell<Catalogo>,
    ciudades: RefCell<Catalogo>,
    calendario: RefCell<Catalogo>,
    al_menu: RefCell<Option<AlMenu>>,
    visibles: RefCell<Vec<usize>>,
    uso_apps: RefCell<Uso>,
    uso_simbolos: RefCell<Uso>,
    /// Sube con cada carga o cálculo pedido: una respuesta vieja que llega tarde se descarta.
    turno: Cell<u64>,
    /// Sin `enter`, el primer movimiento solo marca el punto de partida.
    puntero: Cell<Option<(f64, f64)>>,
    _monitor: gio::AppInfoMonitor,
}

pub enum ParaElMenu {
    Ciudad(Option<crate::config::Clima>),
    Calendario(Option<String>),
}

type AlMenu = Rc<dyn Fn(ParaElMenu)>;

#[derive(Default)]
struct Catalogo {
    entradas: Vec<Entrada>,
    filas: Vec<Fila>,
    acciones: Vec<Accion>,
    cargado: bool,
}

impl Catalogo {
    fn agregar(&mut self, entrada: Entrada, fila: Fila, accion: Accion) {
        self.entradas.push(entrada);
        self.filas.push(fila);
        self.acciones.push(accion);
    }
}

#[derive(Clone)]
enum Accion {
    App(String),
    Simbolo(String),
    Recorte(String),
    /// La referencia del bind se vuelve a buscar al ejecutarlo.
    Atajo(String, String),
    Ciudad(crate::clima::Lugar),
    Calendario(String),
}

struct Uso {
    historial: Historial,
    archivo: Option<PathBuf>,
}

impl Uso {
    fn sumar(&mut self, id: &str) {
        self.historial.sumar(id);
        if let Some(a) = &self.archivo {
            guardar(a, &self.historial.texto());
        }
    }
}

struct Fila {
    row: gtk::ListBoxRow,
    etiqueta: gtk::Label,
    nombre: String,
    generico: Option<String>,
}

enum Icono<'a> {
    App(Option<&'a gio::Icon>),
    Nombre(&'a str),
    Ninguno,
}

impl Launcher {
    pub fn new(app: &gtk::Application) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.add_css_class("morgan");
        window.add_css_class("blank-window");
        window.init_layer_shell();
        window.set_namespace(Some("morgan-launcher"));
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        for borde in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(borde, true);
        }

        let tarjeta = gtk::Box::new(gtk::Orientation::Vertical, 0);
        tarjeta.add_css_class("launcher");
        tarjeta.set_halign(gtk::Align::Center);
        tarjeta.set_valign(gtk::Align::Center);
        tarjeta.set_size_request(ANCHO + 2 * MARGEN, -1);
        let bajada = Bajada::new(&tarjeta);
        window.set_child(Some(&bajada));

        let entrada = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        entrada.add_css_class("launcher-entrada");
        let prompt = gtk::Label::new(Some(Modo::Apps.prompt()));
        prompt.add_css_class("prompt");
        entrada.append(&prompt);
        let separador = gtk::Label::new(Some("›"));
        separador.add_css_class("separador");
        entrada.append(&separador);
        let texto = gtk::Text::new();
        texto.set_placeholder_text(Some("Buscar…"));
        texto.set_hexpand(true);
        entrada.append(&texto);
        tarjeta.append(&entrada);

        let lista = gtk::ListBox::new();
        lista.add_css_class("launcher-lista");
        // Single y no Browse: Browse no deja deseleccionar, y hace falta (ver `mostrar`).
        lista.set_selection_mode(gtk::SelectionMode::Single);
        lista.set_activate_on_single_click(true);
        lista.set_can_focus(false);
        let scroll = gtk::ScrolledWindow::new();
        // El riel va siempre a la vista, aunque todo entre: las filas no cambian de ancho al
        // filtrar.
        let alto = FILAS * ALTO_FILA + (FILAS - 1) * ESPACIO;
        scroll.set_min_content_height(alto);
        scroll.set_max_content_height(alto);
        let foco = Foco::new(&lista);
        scroll.set_child(Some(&foco));
        let desliz = Scroll::new(&scroll);
        tarjeta.append(&crate::riel::con_riel(&scroll));

        let resaltado = gtk::Label::new(None);
        resaltado.add_css_class("launcher-resaltado");
        resaltado.set_visible(false);
        tarjeta.append(&resaltado);

        let estado = crate::notificaciones::dir_from(|k| std::env::var_os(k));
        let archivo = |n: &str| estado.as_ref().map(|d| d.join(n));
        let uso_apps = Uso {
            historial: leer_historial(archivo("launcher-apps").as_deref()),
            archivo: archivo("launcher-apps"),
        };
        let uso_simbolos = Uso {
            historial: leer_historial(archivo("launcher-simbolos").as_deref()),
            archivo: archivo("launcher-simbolos"),
        };
        let monitor = gio::AppInfoMonitor::get();
        let inner = Rc::new(Inner {
            window,
            tarjeta,
            bajada,
            prompt,
            texto,
            lista,
            foco,
            desliz,
            resaltado,
            modo: Cell::new(Modo::Apps),
            abierto: Cell::new(false),
            apps: RefCell::default(),
            apps_sucias: Cell::new(true),
            simbolos: RefCell::default(),
            portapapeles: RefCell::default(),
            atajos: RefCell::default(),
            ciudades: RefCell::default(),
            calendario: RefCell::default(),
            al_menu: RefCell::default(),
            visibles: RefCell::default(),
            uso_apps: RefCell::new(uso_apps),
            uso_simbolos: RefCell::new(uso_simbolos),
            turno: Cell::new(0),
            puntero: Cell::new(None),
            _monitor: monitor.clone(),
        });
        let l = Launcher { inner };
        // Cerrado no hace falta rehacer nada: se marca y se lee al abrir.
        let weak = Rc::downgrade(&l.inner);
        monitor.connect_changed(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.apps_sucias.set(true);
                let l = Launcher { inner };
                if l.abierto() && l.inner.modo.get() == Modo::Apps {
                    l.leer_apps();
                    l.filtrar();
                }
            }
        });
        l.conectar();
        l
    }

    pub fn al_menu(&self, f: impl Fn(ParaElMenu) + 'static) {
        self.inner.al_menu.replace(Some(Rc::new(f)));
    }

    fn volver_al_menu(&self, p: ParaElMenu) {
        let f = self.inner.al_menu.borrow().clone();
        if let Some(f) = f {
            f(p);
        }
    }

    pub fn abierto(&self) -> bool {
        self.inner.abierto.get()
    }

    pub fn alternar(&self, modo: Modo) {
        if self.abierto() && self.inner.modo.get() == modo {
            self.cerrar();
        } else {
            self.abrir(modo);
        }
    }

    pub fn abrir(&self, modo: Modo) {
        let i = &self.inner;
        if i.abierto.get() && i.modo.get() == modo {
            return;
        }
        let ya_abierto = i.abierto.replace(true);
        i.modo.set(modo);
        i.prompt.set_label(modo.prompt());
        i.turno.set(i.turno.get() + 1);
        // Cada apertura (o cambio de modo) empieza de cero.
        self.vaciar();
        i.texto.set_text("");
        match modo {
            Modo::Apps => {
                if i.apps_sucias.get() {
                    self.leer_apps();
                }
                self.filtrar();
            }
            Modo::Simbolos => {
                if !i.simbolos.borrow().cargado {
                    self.leer_simbolos();
                }
                self.filtrar();
            }
            Modo::Portapapeles | Modo::Atajos => self.cargar(modo),
            // Se llenan al tipear (`buscar_ciudad`, `url_pegada`).
            Modo::Ciudad => {
                self.inner.ciudades.replace(Catalogo::default());
            }
            Modo::Calendario => {
                self.inner.calendario.replace(Catalogo::default());
            }
        }
        if !ya_abierto {
            i.bajada.arrancar();
            i.puntero.set(None);
            i.window.set_visible(true);
        }
        i.texto.grab_focus();
    }

    pub fn cerrar(&self) {
        let i = &self.inner;
        if !i.abierto.replace(false) {
            return;
        }
        i.turno.set(i.turno.get() + 1);
        i.window.set_visible(false);
    }

    fn catalogo(&self, modo: Modo) -> &RefCell<Catalogo> {
        let i = &self.inner;
        match modo {
            Modo::Apps => &i.apps,
            Modo::Simbolos => &i.simbolos,
            Modo::Portapapeles => &i.portapapeles,
            Modo::Atajos => &i.atajos,
            Modo::Ciudad => &i.ciudades,
            Modo::Calendario => &i.calendario,
        }
    }

    fn conectar(&self) {
        let i = &self.inner;

        // En captura: las flechas, Tab y Enter no le llegan al campo de texto.
        let teclas = gtk::EventControllerKey::new();
        teclas.set_propagation_phase(gtk::PropagationPhase::Capture);
        let yo = self.clone();
        teclas.connect_key_pressed(move |_, tecla, _, _| {
            let paso = match tecla {
                gdk::Key::Escape => {
                    let modo = yo.inner.modo.get();
                    yo.cerrar();
                    // Desde el menú: Escape vuelve a él, como en la banda de fondos.
                    match modo {
                        Modo::Ciudad => yo.volver_al_menu(ParaElMenu::Ciudad(None)),
                        Modo::Calendario => yo.volver_al_menu(ParaElMenu::Calendario(None)),
                        _ => {}
                    }
                    return glib::Propagation::Stop;
                }
                gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => {
                    yo.elegir_marcada();
                    return glib::Propagation::Stop;
                }
                gdk::Key::Down | gdk::Key::Tab => 1,
                gdk::Key::Up | gdk::Key::ISO_Left_Tab => -1,
                gdk::Key::Page_Down => FILAS,
                gdk::Key::Page_Up => -FILAS,
                _ => return glib::Propagation::Proceed,
            };
            yo.mover(paso);
            glib::Propagation::Stop
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
            if yo.abierto() {
                yo.filtrar();
            }
        });

        // Pasar el mouse elige y un click lanza. Solo cuenta el movimiento de verdad: cuando las
        // filas cambian bajo un puntero quieto, GTK manda movimientos sintéticos en el mismo punto, y
        // elegir ahí haría que Enter lance la fila que quedó debajo del mouse y no la primera.
        let movimiento = gtk::EventControllerMotion::new();
        let yo = self.clone();
        movimiento.connect_enter(move |_, x, y| yo.inner.puntero.set(Some((x, y))));
        let yo = self.clone();
        movimiento.connect_motion(move |_, x, y| {
            let i = &yo.inner;
            let antes = i.puntero.replace(Some((x, y)));
            if antes.is_none_or(|a| a == (x, y)) {
                return;
            }
            let Some(p) = i
                .window
                .compute_point(&i.lista, &gtk::graphene::Point::new(x as f32, y as f32))
            else {
                return;
            };
            if let Some(r) = i.lista.row_at_y(p.y() as i32)
                && !r.is_selected()
            {
                i.foco.proximo(Paso::Deslizar);
                i.lista.select_row(Some(&r));
            }
        });
        i.window.add_controller(movimiento);
        let yo = self.clone();
        i.lista
            .connect_row_activated(move |_, r| yo.elegir(r.index()));
    }

    fn display(&self) -> gdk::Display {
        WidgetExt::display(&self.inner.window)
    }

    fn leer_apps(&self) {
        let i = &self.inner;
        i.apps_sucias.set(false);
        let display = self.display();
        let mut c = Catalogo::default();
        for a in gio::AppInfo::all() {
            if !a.should_show() {
                continue;
            }
            let Ok(d) = a.downcast::<gio_unix::DesktopAppInfo>() else {
                continue;
            };
            let Some(id) = d.id() else {
                continue;
            };
            let nombre = d.display_name().to_string();
            let generico = d
                .generic_name()
                .map(|g| g.to_string())
                .filter(|g| !g.is_empty() && *g != nombre);
            let palabras: Vec<String> = d.keywords().iter().map(|k| k.to_string()).collect();
            let categorias = d.categories().map(|c| c.to_string()).unwrap_or_default();
            let ejecutable = d.executable();
            let ejecutable = ejecutable
                .file_name()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut campos: Vec<&str> =
                vec![generico.as_deref().unwrap_or(""), &categorias, &ejecutable];
            campos.extend(palabras.iter().map(String::as_str));
            c.agregar(
                Entrada::new(&id, &nombre, &campos),
                fila(&display, Icono::App(d.icon().as_ref()), nombre, generico),
                Accion::App(id.to_string()),
            );
        }
        c.cargado = true;
        i.apps.replace(c);
    }

    fn leer_simbolos(&self) {
        let display = self.display();
        let mut c = Catalogo {
            cargado: true,
            ..Default::default()
        };
        let Some(dir) = dir_rofimoji() else {
            eprintln!("morgan: launcher: no están los datos de rofimoji");
            self.inner.simbolos.replace(c);
            return;
        };
        for nombre in ARCHIVOS_SIMBOLOS {
            let Ok(texto) = std::fs::read_to_string(dir.join(format!("{nombre}.csv"))) else {
                continue;
            };
            for s in fuentes::simbolos(&texto) {
                let vista = format!("{}  {}", s.caracter, s.nombre);
                let generico = (!s.claves.is_empty()).then(|| s.claves.clone());
                c.agregar(
                    Entrada::new(&s.caracter, &vista, &[&s.claves]),
                    fila(&display, Icono::Ninguno, vista, generico),
                    Accion::Simbolo(s.caracter),
                );
            }
        }
        self.inner.simbolos.replace(c);
    }

    /// Portapapeles y atajos se releen en cada apertura: cambian todo el tiempo.
    fn cargar(&self, modo: Modo) {
        let turno = self.inner.turno.get();
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let args: &[&str] = match modo {
                Modo::Portapapeles => &["cliphist", "list"],
                _ => &["hyprctl", "-j", "binds"],
            };
            let salida = comando::salida(args).await;
            let i = &yo.inner;
            if i.turno.get() != turno {
                return;
            }
            let display = yo.display();
            let mut c = Catalogo {
                cargado: true,
                ..Default::default()
            };
            match (modo, salida) {
                (Modo::Portapapeles, Ok(t)) => {
                    for r in fuentes::recortes(&t).into_iter().take(250) {
                        c.agregar(
                            Entrada::new(&r.linea, &r.vista, &[]),
                            fila(&display, Icono::Ninguno, r.vista, None),
                            Accion::Recorte(r.linea),
                        );
                    }
                }
                (_, Ok(t)) => match fuentes::atajos(&t) {
                    Ok(v) => {
                        for a in v {
                            c.agregar(
                                Entrada::new(&a.descripcion, &a.descripcion, &[&a.teclas]),
                                fila(
                                    &display,
                                    Icono::Nombre("input-keyboard"),
                                    a.descripcion.clone(),
                                    Some(a.teclas.clone()),
                                ),
                                Accion::Atajo(a.descripcion, a.teclas),
                            );
                        }
                    }
                    Err(e) => eprintln!("morgan: launcher: {e}"),
                },
                (_, Err(e)) => eprintln!("morgan: launcher: {e}"),
            }
            yo.catalogo(modo).replace(c);
            yo.filtrar();
        });
    }

    fn filtrar(&self) {
        match self.inner.modo.get() {
            Modo::Apps => {
                let uso = self.inner.uso_apps.borrow();
                self.mostrar(Orden::Uso(&uso.historial), true);
            }
            Modo::Simbolos => {
                let uso = self.inner.uso_simbolos.borrow();
                self.mostrar(Orden::Uso(&uso.historial), true);
            }
            Modo::Portapapeles => self.mostrar(Orden::Original, true),
            // Ya vienen ordenados por descripción.
            Modo::Atajos => self.mostrar(Orden::Original, true),
            Modo::Ciudad => self.buscar_ciudad(),
            Modo::Calendario => self.url_pegada(),
        }
    }

    /// La URL completa no se muestra: es una credencial.
    fn url_pegada(&self) {
        let texto = self.inner.texto.text().trim().to_owned();
        let mut c = Catalogo {
            cargado: true,
            ..Default::default()
        };
        if let Some(dominio) = dominio_de_calendario(&texto) {
            let display = self.display();
            c.agregar(
                Entrada::new("Usar este calendario", "Usar este calendario", &[]),
                fila(
                    &display,
                    Icono::Nombre("x-office-calendar-symbolic"),
                    "Usar este calendario".into(),
                    Some(dominio),
                ),
                Accion::Calendario(texto),
            );
        }
        self.inner.calendario.replace(c);
        self.mostrar(Orden::Original, false);
    }

    /// Cuando se deja de tipear un momento: una consulta por tecla sería un pedido a la red por
    /// letra.
    fn buscar_ciudad(&self) {
        let i = &self.inner;
        i.turno.set(i.turno.get() + 1);
        let turno = i.turno.get();
        let texto = i.texto.text().to_string();
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            glib::timeout_future(PAUSA_CIUDAD).await;
            if yo.inner.turno.get() != turno {
                return;
            }
            let lugares = crate::clima::buscar(&texto).await;
            if yo.inner.turno.get() != turno {
                return;
            }
            let display = yo.display();
            let mut c = Catalogo {
                cargado: true,
                ..Default::default()
            };
            for l in lugares {
                let region = (!l.region.is_empty()).then(|| l.region.clone());
                c.agregar(
                    Entrada::new(&l.nombre, &l.nombre, &[]),
                    fila(&display, Icono::Nombre("mark-location-symbolic"), l.nombre.clone(), region),
                    Accion::Ciudad(l),
                );
            }
            yo.inner.ciudades.replace(c);
            // El geocoder ya filtró (y entiende cosas que la búsqueda local no, como «Bs As»).
            yo.mostrar(Orden::Original, false);
        });
    }

    fn vaciar(&self) {
        let i = &self.inner;
        i.lista.unselect_all();
        i.lista.remove_all();
        i.visibles.replace(Vec::new());
    }

    fn mostrar(&self, orden: Orden, filtrando: bool) {
        let i = &self.inner;
        let consulta = if filtrando {
            i.texto.text().to_string()
        } else {
            String::new()
        };
        let c = self.catalogo(i.modo.get()).borrow();
        let visibles = busqueda::filtrar(&c.entradas, &consulta, orden);
        let color = color_hex(&i.resaltado.color());
        // Las filas se reusan: una que se saca elegida vuelve con su marca, y `select_row`
        // la ignora por creerla ya elegida (la lista queda sin elegida y Enter no hace nada).
        self.vaciar();
        for &v in &visibles {
            let f = &c.filas[v];
            f.row.remove_css_class("elegida");
            f.etiqueta.set_markup(&markup(f, &consulta, &color));
            i.lista.append(&f.row);
        }
        i.visibles.replace(visibles);
        i.lista.select_row(i.lista.row_at_index(0).as_ref());
        i.desliz.poner(0.0);
    }

    /// Con vuelta al otro extremo solo cuando se avanza de a una.
    fn mover(&self, paso: i32) {
        let i = &self.inner;
        let n = i.visibles.borrow().len() as i32;
        if n == 0 {
            return;
        }
        let actual = i.lista.selected_row().map_or(0, |r| r.index());
        let nueva = if paso.abs() == 1 {
            (actual + paso).rem_euclid(n)
        } else {
            (actual + paso).clamp(0, n - 1)
        };
        // La vuelta de un extremo al otro no cruza la lista: el recuadro aparece allá.
        let vuelta = (nueva - actual).abs() > paso.abs();
        i.foco.proximo(if vuelta { Paso::Aparecer } else { Paso::Deslizar });
        if let Some(r) = i.lista.row_at_index(nueva) {
            i.lista.select_row(Some(&r));
            self.mostrar_fila(&r);
        }
    }

    fn mostrar_fila(&self, r: &gtk::ListBoxRow) {
        let i = &self.inner;
        let Some(b) = r.compute_bounds(&i.lista) else {
            return;
        };
        // Contra adónde va el scroll, no donde está: con las flechas apretadas, cada paso
        // sigue al anterior aunque el recorrido no haya terminado.
        let (valor, pagina) = (i.desliz.destino(), i.desliz.pagina());
        let (arriba, abajo) = (b.y() as f64, (b.y() + b.height()) as f64);
        if arriba < valor {
            i.desliz.ir(arriba);
        } else if abajo > valor + pagina {
            i.desliz.ir(abajo - pagina);
        }
    }

    fn elegir_marcada(&self) {
        // Enter sin coincidencias no hace nada.
        if let Some(r) = self.inner.lista.selected_row() {
            self.elegir(r.index());
        }
    }

    fn elegir(&self, posicion: i32) {
        let i = &self.inner;
        let modo = i.modo.get();
        let Some(accion) = usize::try_from(posicion)
            .ok()
            .and_then(|p| i.visibles.borrow().get(p).copied())
            .and_then(|e| self.catalogo(modo).borrow().acciones.get(e).cloned())
        else {
            return;
        };
        // El destello de la elegida (`.elegida` en la hoja) se pinta y recién ahí se cierra:
        // un cuadro, y la capa se lo lleva fundiéndose. Un segundo Enter en ese cuadro no
        // repite la acción: el launcher ya está cerrado cuando le toca.
        if let Some(r) = i.lista.row_at_index(posicion) {
            r.add_css_class("elegida");
        }
        let yo = self.clone();
        foco::despues_de_pintar(i.window.upcast_ref(), move || yo.ejecutar(accion));
    }

    fn ejecutar(&self, accion: Accion) {
        let i = &self.inner;
        if !i.abierto.get() {
            return;
        }
        self.cerrar();
        match accion {
            Accion::App(id) => {
                comando::lanzar_app(&id, &[]);
                i.uso_apps.borrow_mut().sumar(&id);
            }
            // Se escribe en la ventana que tenía el foco.
            Accion::Simbolo(s) => {
                i.uso_simbolos.borrow_mut().sumar(&s);
                glib::timeout_add_local_once(DEVOLVER_FOCO, move || {
                    comando::lanzar(&["wtype", "--", &s]);
                });
            }
            Accion::Recorte(linea) => comando::lanzar_con_entrada(
                &["sh", "-c", "cliphist decode | wl-copy"],
                linea.as_bytes(),
            ),
            Accion::Ciudad(l) => self.volver_al_menu(ParaElMenu::Ciudad(Some(crate::config::Clima {
                lugar: l.nombre,
                latitud: Some(l.latitud),
                longitud: Some(l.longitud),
            }))),
            Accion::Calendario(url) => self.volver_al_menu(ParaElMenu::Calendario(Some(url))),
            // Releídos al ejecutar: entre abrir y elegir, Hyprland pudo releer la config y
            // la referencia de entonces ser de otra acción.
            Accion::Atajo(descripcion, teclas) => {
                glib::timeout_add_local_once(DEVOLVER_FOCO, move || {
                    glib::MainContext::default().spawn_local(async move {
                        let binds = comando::salida(&["hyprctl", "-j", "binds"]).await;
                        let atajo = binds.and_then(|t| fuentes::binds(&t)).ok().and_then(|v| {
                            v.into_iter()
                                .find(|a| a.descripcion == descripcion && a.teclas == teclas)
                        });
                        match atajo.and_then(|a| a.referencia) {
                            Some(r) => comando::lanzar(&["hyprctl", "eval", &fuentes::ejecutar_bind(r)]),
                            None => eprintln!("morgan: launcher: {descripcion}: el bind ya no está"),
                        }
                    });
                });
            }
        }
    }
}

fn dominio_de_calendario(texto: &str) -> Option<String> {
    let resto = texto
        .strip_prefix("https://")
        .or_else(|| texto.strip_prefix("webcal://"))?;
    if texto.contains(char::is_whitespace) {
        return None;
    }
    let dominio = resto.split('/').next().filter(|d| d.contains('.'))?;
    Some(dominio.to_owned())
}

fn dir_rofimoji() -> Option<PathBuf> {
    std::fs::read_dir("/usr/lib")
        .ok()?
        .flatten()
        .filter(|d| d.file_name().to_string_lossy().starts_with("python3"))
        .map(|d| d.path().join("site-packages/picker/data"))
        .find(|p| p.is_dir())
}

fn fila(display: &gdk::Display, icono: Icono, nombre: String, generico: Option<String>) -> Fila {
    let caja = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    match icono {
        Icono::App(i) => caja.append(&icono::imagen(display, i, 24)),
        Icono::Nombre(n) => {
            let imagen = gtk::Image::from_icon_name(n);
            imagen.set_pixel_size(24);
            caja.append(&imagen);
        }
        Icono::Ninguno => {}
    }
    let etiqueta = gtk::Label::new(None);
    etiqueta.set_xalign(0.0);
    etiqueta.set_hexpand(true);
    etiqueta.set_ellipsize(gtk::pango::EllipsizeMode::End);
    etiqueta.set_single_line_mode(true);
    caja.append(&etiqueta);
    let row = gtk::ListBoxRow::new();
    row.set_can_focus(false);
    row.set_child(Some(&caja));
    Fila {
        row,
        etiqueta,
        nombre,
        generico,
    }
}

fn markup(f: &Fila, consulta: &str, color: &str) -> String {
    let mut m = resaltar_markup(&f.nombre, consulta, color);
    if let Some(g) = &f.generico {
        m.push_str(&format!(
            "<span size=\"small\" alpha=\"55%\">  ·  {}</span>",
            resaltar_markup(g, consulta, color)
        ));
    }
    m
}

pub(crate) fn resaltar_markup(texto: &str, consulta: &str, color: &str) -> String {
    if consulta.is_empty() {
        return glib::markup_escape_text(texto).to_string();
    }
    let intervalos = busqueda::resaltar(texto, consulta);
    if intervalos.is_empty() {
        return glib::markup_escape_text(texto).to_string();
    }
    let mut byte_offsets: Vec<usize> = texto.char_indices().map(|(b, _)| b).collect();
    byte_offsets.push(texto.len());
    let mut out = String::with_capacity(texto.len() + intervalos.len() * 40);
    let mut desde = 0;
    for (a, b) in intervalos {
        if a > desde {
            out.push_str(&glib::markup_escape_text(
                &texto[byte_offsets[desde]..byte_offsets[a]],
            ));
        }
        out.push_str(&format!(
            "<span foreground=\"{color}\">{}</span>",
            glib::markup_escape_text(&texto[byte_offsets[a]..byte_offsets[b]])
        ));
        desde = b;
    }
    if desde < byte_offsets.len() - 1 {
        out.push_str(&glib::markup_escape_text(
            &texto[byte_offsets[desde]..],
        ));
    }
    out
}

pub(crate) fn color_hex(c: &gdk::RGBA) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.red()), b(c.green()), b(c.blue()))
}

fn leer_historial(archivo: Option<&Path>) -> Historial {
    archivo
        .and_then(|a| std::fs::read_to_string(a).ok())
        .map(|t| Historial::parse(&t))
        .unwrap_or_default()
}

/// Escribe a un temporal y lo renombra: un corte a mitad de camino no deja el archivo roto.
fn guardar(archivo: &Path, texto: &str) {
    let r = (|| {
        if let Some(dir) = archivo.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = archivo.with_extension("tmp");
        std::fs::write(&tmp, texto)?;
        std::fs::rename(&tmp, archivo)
    })();
    if let Err(e) = r {
        eprintln!(
            "morgan: launcher: no se pudo guardar {}: {e}",
            archivo.display()
        );
    }
}
