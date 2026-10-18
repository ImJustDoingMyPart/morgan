//! No toma el teclado, y el mouse solo en el cuadrito del ícono: el resto deja pasar los clicks
//! a la ventana de abajo. La hoja se relee si cambió su fecha, en el próximo evento de
//! Hyprland: sin vigilarla.
//!
//! Una capa no se puede pegar a una ventana, así que se ubica con márgenes desde la esquina
//! de la salida. En la misma salida se mueve en el lugar (solo cambian los márgenes, sin
//! desmapear); a otra salida, se esconde y reaparece.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use gtk::prelude::*;
use gtk::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use serde::Deserialize;

use crate::hyprland::Hyprland;
use crate::pliegue::Pliegue;

/// `activewindowv2` también llega en cada cambio de título: ése solo cuenta si cambió la
/// ventana.
const EVENTOS: &[&str] = &[
    "activewindowv2",
    "movewindowv2",
    "changefloatingmode",
    "fullscreen",
    "openwindow",
    "closewindow",
    "workspacev2",
    "focusedmonv2",
    "monitoraddedv2",
    "monitorremovedv2",
];

const MARGEN_CSS: i32 = 6;
/// Deja libre la primera línea de una terminal, donde el shell pone el prompt derecho.
const AIRE_ARRIBA: i32 = 36;
const AIRE_DERECHA: i32 = 14;
/// En una ventana más chica, la tarjeta taparía la mitad: no aparece.
const ANCHO_MIN: i32 = 640;
const ALTO_MIN: i32 = 400;
/// El ancho de la tarjeta sale de acá.
const CARACTERES: i32 = 30;
const GLIFO: &str = "\u{f030c}";
const LADO_CUADRO: i32 = 36;
const SEPARADOR: &str = " · ";

/// Atajos y comandos van sueltos o en grupos con nombre (los dos a la vez no: TOML no mezcla
/// una lista con tablas):
///
/// ```toml
/// comandos = [["cd -", "vuelve al directorio anterior"]]
/// otras = ["lo que no es ni lo uno ni lo otro"]
///
/// [[atajos]]
/// grupo = "Cursor"
/// notas = [["Ctrl+A", "al inicio de la línea"]]
/// ```
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hoja {
    pub atajos: Vec<Entrada>,
    pub comandos: Vec<Entrada>,
    pub otras: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Entrada {
    Nota(String, String),
    Grupo(Grupo),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grupo {
    pub grupo: String,
    pub notas: Vec<(String, String)>,
}

impl Hoja {
    pub fn vacia(&self) -> bool {
        self.atajos.is_empty() && self.comandos.is_empty() && self.otras.is_empty()
    }
}

pub fn parse_hoja(texto: &str) -> Result<Hoja, toml::de::Error> {
    toml::from_str(texto)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Foco {
    pub clase: String,
    pub salida: String,
    pub x: i32,
    pub y: i32,
    pub ancho: i32,
    pub alto: i32,
    pub ancho_salida: i32,
    pub pantalla_completa: bool,
}

pub fn leer_foco(ventana: &str, monitores: &str) -> Result<Option<Foco>, serde_json::Error> {
    #[derive(Deserialize)]
    struct V {
        class: Option<String>,
        at: Option<[i32; 2]>,
        size: Option<[i32; 2]>,
        #[serde(default)]
        fullscreen: i64,
        monitor: Option<i64>,
    }
    #[derive(Deserialize)]
    struct M {
        id: i64,
        name: String,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        scale: f64,
        transform: i64,
    }
    let v: V = serde_json::from_str(ventana)?;
    let ms: Vec<M> = serde_json::from_str(monitores)?;
    let (Some(clase), Some([x, y]), Some([ancho, alto]), Some(id)) =
        (v.class, v.at, v.size, v.monitor)
    else {
        return Ok(None);
    };
    let Some(m) = ms.into_iter().find(|m| m.id == id) else {
        return Ok(None);
    };
    // Rotada un cuarto de vuelta (transform impar), el ancho lógico es el alto.
    let ancho_px = if m.transform % 2 == 1 {
        m.height
    } else {
        m.width
    };
    let escala = if m.scale > 0.0 { m.scale } else { 1.0 };
    Ok(Some(Foco {
        clase,
        salida: m.name,
        x: x - m.x,
        y: y - m.y,
        ancho,
        alto,
        ancho_salida: (f64::from(ancho_px) / escala).round() as i32,
        // Medido en la 0.56.2: maximizada es 1 y ahí la tarjeta sigue; pantalla completa es 2.
        pantalla_completa: v.fullscreen >= 2,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lugar {
    pub salida: String,
    pub arriba: i32,
    pub derecha: i32,
}

pub fn lugar(f: &Foco) -> Option<Lugar> {
    if f.pantalla_completa || f.ancho < ANCHO_MIN || f.alto < ALTO_MIN {
        return None;
    }
    // Una ventana que se sale por la derecha (un layout scrolling): la tarjeta queda en el borde
    // de la salida.
    let fuera = (f.ancho_salida - (f.x + f.ancho)).max(0);
    Some(Lugar {
        salida: f.salida.clone(),
        arriba: f.y.max(0) + AIRE_ARRIBA - MARGEN_CSS,
        derecha: fuera + AIRE_DERECHA - MARGEN_CSS,
    })
}

/// Un terminal con el título animado manda un `activewindowv2` de la misma ventana por
/// segundo.
fn cuenta(linea: &str, foco: &mut Option<String>) -> bool {
    let Some((nombre, datos)) = linea.split_once(">>") else {
        return false;
    };
    if nombre == "activewindowv2" {
        if foco.as_deref() == Some(datos) {
            return false;
        }
        *foco = Some(datos.to_owned());
    }
    EVENTOS.contains(&nombre)
}

pub fn elegida(apps: &[String], clase: &str) -> bool {
    apps.iter().any(|a| a.eq_ignore_ascii_case(clase))
}

type Leida = (Option<SystemTime>, Option<Rc<Hoja>>);

#[derive(Clone)]
pub struct Notas {
    inner: Rc<Inner>,
}

struct Inner {
    hyprland: Hyprland,
    window: gtk::ApplicationWindow,
    partes: Partes,
    minima: Cell<bool>,
    region: Cell<Option<(i32, i32, i32, i32)>>,
    reloj: RefCell<Option<(gdk::FrameClock, glib::SignalHandlerId)>>,
    apps: RefCell<Vec<String>>,
    dir: PathBuf,
    hojas: RefCell<HashMap<String, Leida>>,
    pedida: Cell<bool>,
    actual: RefCell<Option<(Lugar, Rc<Hoja>)>>,
    leer: RefCell<Option<glib::SourceId>>,
    /// La relectura que vuelve después de una más nueva no pinta.
    vuelta: Cell<u64>,
    foco: RefCell<Option<String>>,
}

impl Notas {
    pub fn new(
        app: &gtk::Application,
        hyprland: Hyprland,
        apps: Vec<String>,
        tecla: &str,
        dir: PathBuf,
    ) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.add_css_class("morgan");
        window.init_layer_shell();
        window.set_namespace(Some("morgan-notas"));
        // Encima de las ventanas y debajo de lo que se abre a propósito (launcher, centro).
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_exclusive_zone(-1);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Right, true);
        let partes = Partes::new();
        partes.poner_tecla(tecla);
        window.set_child(Some(&partes.raiz));

        let notas = Notas {
            inner: Rc::new(Inner {
                hyprland: hyprland.clone(),
                window,
                partes,
                minima: Cell::new(false),
                region: Cell::new(None),
                reloj: RefCell::default(),
                apps: RefCell::new(apps),
                dir,
                hojas: RefCell::default(),
                pedida: Cell::new(true),
                actual: RefCell::default(),
                leer: RefCell::default(),
                vuelta: Cell::new(0),
                foco: RefCell::default(),
            }),
        };
        let clic = gtk::GestureClick::new();
        let weak = Rc::downgrade(&notas.inner);
        clic.connect_released(move |_, _, _, _| {
            if let Some(inner) = weak.upgrade() {
                Notas { inner }.minimizar();
            }
        });
        notas.inner.partes.cuadro.add_controller(clic);
        // La región de entrada sigue al cuadrito, que se mueve al minimizar o al cambiar el
        // ancho de la tarjeta. Una superficie nueva (otra salida) trae otro reloj.
        let weak = Rc::downgrade(&notas.inner);
        notas.inner.window.connect_realize(move |w| {
            if let Some(inner) = weak.upgrade() {
                Notas { inner }.seguir_region(w);
            }
        });
        let weak = Rc::downgrade(&notas.inner);
        notas.inner.window.connect_unrealize(move |_| {
            if let Some((reloj, id)) = weak.upgrade().and_then(|i| i.reloj.take()) {
                reloj.disconnect(id);
            }
        });
        let weak = Rc::downgrade(&notas.inner);
        hyprland.al_evento(move |linea| {
            if let Some(inner) = weak.upgrade()
                && cuenta(linea, &mut inner.foco.borrow_mut())
            {
                Notas { inner }.programar();
            }
        });
        notas.programar();
        notas
    }

    pub fn aplicar_config(&self, n: &crate::config::Notas, tecla: &str) {
        self.inner.partes.poner_tecla(tecla);
        self.inner.apps.replace(n.apps.clone());
        self.programar();
    }

    pub fn pedir(&self, mostrar: Option<bool>) {
        let i = &self.inner;
        i.pedida.set(mostrar.unwrap_or(!i.pedida.get()));
        if i.pedida.get() {
            self.programar();
        } else {
            self.esconder();
        }
    }

    fn programar(&self) {
        let i = &self.inner;
        if i.leer.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(i);
        // Una ráfaga de eventos llega en la misma lectura del socket y se lee una vez.
        let id = glib::idle_add_local_once(move || {
            let Some(inner) = weak.upgrade() else { return };
            inner.leer.take();
            let notas = Notas { inner };
            glib::MainContext::default().spawn_local(async move { notas.releer().await });
        });
        i.leer.replace(Some(id));
    }

    async fn releer(&self) {
        let i = &self.inner;
        if !i.pedida.get() {
            return;
        }
        if i.apps.borrow().is_empty() {
            return self.esconder();
        }
        let vuelta = i.vuelta.get() + 1;
        i.vuelta.set(vuelta);
        let (v, m) = futures_lite::future::zip(
            i.hyprland.consultar("j/activewindow"),
            i.hyprland.consultar("j/monitors"),
        )
        .await;
        let (Some(v), Some(m)) = (v, m) else { return };
        let foco = match leer_foco(&v, &m) {
            Ok(f) => f,
            Err(e) => return eprintln!("morgan: notas: ventana enfocada ilegible: {e}"),
        };
        // Lo que se pidió entre tanto (esconder, otra relectura) gana.
        if !i.pedida.get() || i.vuelta.get() != vuelta {
            return;
        }
        let destino = foco
            .filter(|f| elegida(&i.apps.borrow(), &f.clase))
            .and_then(|f| Some((lugar(&f)?, self.hoja(&f.clase)?, f.clase)));
        let Some((l, hoja, clase)) = destino else {
            return self.esconder();
        };
        let (misma_salida, misma_hoja) = match &*i.actual.borrow() {
            Some((la, ha)) => (la.salida == l.salida, Rc::ptr_eq(ha, &hoja)),
            None => (false, false),
        };
        if !misma_hoja {
            i.partes.dibujar(&hoja, &clase);
            i.window.set_default_size(1, 1);
        }
        // En la misma salida, a la vista: solo los márgenes (un configure de la capa, sin
        // desmapear ni el fundido de Hyprland). Cambiar de salida sí pide desmapearla.
        let visible = i.window.is_visible();
        if !(misma_salida && visible) {
            let Some(monitor) = monitor_de(&l.salida) else {
                return self.esconder();
            };
            if visible {
                i.window.set_visible(false);
            }
            i.window.set_monitor(Some(&monitor));
        }
        i.window.set_margin(Edge::Top, l.arriba);
        i.window.set_margin(Edge::Right, l.derecha);
        i.actual.replace(Some((l, hoja)));
        i.window.set_visible(true);
    }

    /// Es solo lo que se pinta: el tamaño de la capa no cambia, así Hyprland no anima su
    /// geometría y el cuadrito no se mueve.
    fn minimizar(&self) {
        let i = &self.inner;
        let minima = !i.minima.get();
        i.minima.set(minima);
        i.partes.minimizar(minima);
    }

    /// Se mide después de pintar cada cuadro (antes, la tarjeta todavía no está medida y da cero)
    /// y se manda solo si cambió.
    fn seguir_region(&self, w: &gtk::ApplicationWindow) {
        // Hasta el primer cuadro medido, nada recibe el mouse.
        if let Some(s) = w.surface() {
            s.set_input_region(Some(&gtk::cairo::Region::create()));
        }
        let Some(reloj) = w.frame_clock() else { return };
        self.inner.region.set(None);
        let weak = Rc::downgrade(&self.inner);
        let id = reloj.connect_after_paint(move |_| {
            let Some(inner) = weak.upgrade() else { return };
            let w = &inner.window;
            let Some(r) = inner.partes.cuadro.compute_bounds(w) else {
                return;
            };
            let (dx, dy) = w.surface_transform();
            let rect = (
                (f64::from(r.x()) + dx).round() as i32,
                (f64::from(r.y()) + dy).round() as i32,
                r.width().round() as i32,
                r.height().round() as i32,
            );
            if inner.region.replace(Some(rect)) == Some(rect) {
                return;
            }
            let region = gtk::cairo::Region::create();
            let (x, y, ancho, alto) = rect;
            let _ = region.union_rectangle(&gtk::cairo::RectangleInt::new(x, y, ancho, alto));
            if let Some(s) = w.surface() {
                s.set_input_region(Some(&region));
            }
            // GDK la manda con el próximo commit, y una tarjeta quieta no tiene otro: un cuadro
            // más (el siguiente after-paint ve el mismo rectángulo y no sigue).
            w.queue_draw();
        });
        self.inner.reloj.replace(Some((reloj, id)));
    }

    fn esconder(&self) {
        self.inner.window.set_visible(false);
        self.inner.actual.take();
    }

    fn hoja(&self, clase: &str) -> Option<Rc<Hoja>> {
        let ruta = ruta_hoja(&self.inner.dir, clase);
        let fecha = std::fs::metadata(&ruta).and_then(|m| m.modified()).ok();
        let mut hojas = self.inner.hojas.borrow_mut();
        if let Some((f, h)) = hojas.get(clase)
            && *f == fecha
        {
            return h.clone();
        }
        let hoja = fecha.and_then(|_| match std::fs::read_to_string(&ruta) {
            Ok(t) => match parse_hoja(&t) {
                Ok(h) if !h.vacia() => Some(Rc::new(h)),
                Ok(_) => None,
                Err(e) => {
                    eprintln!("morgan: notas: {}: {e}", ruta.display());
                    None
                }
            },
            Err(e) => {
                eprintln!("morgan: notas: {}: {e}", ruta.display());
                None
            }
        });
        hojas.insert(clase.to_owned(), (fecha, hoja.clone()));
        hoja
    }
}

pub fn editar(dir: &Path, clase: &str) -> Result<(), String> {
    let ruta = ruta_hoja(dir, clase);
    if !ruta.exists() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::write(&ruta, hoja_nueva(clase)).map_err(|e| format!("{}: {e}", ruta.display()))?;
    }
    crate::comando::abrir(&ruta, "text/plain")
}

fn hoja_nueva(clase: &str) -> String {
    format!(
        "# {}\n# {}\n# {}\n\natajos = [\n    [\"Ctrl+S\", \"{}\"],\n]\n\ncomandos = [\n    [\"{}\", \"{}\"],\n]\n", // no-tr
        trf!("Notas de {} para la tarjeta de morgan (Mod+Alt+N la esconde o la muestra).", clase),
        tr!("Pares [teclas o comando, qué hace], sueltos o en grupos ([[atajos]] con grupo y notas);"),
        tr!("varias teclas para lo mismo, separadas por \" · \". `otras` es texto suelto."),
        tr!("guardar"),
        "--help", // no-tr
        tr!("la ayuda del programa"),
    )
}

pub fn ruta_hoja(dir: &Path, clase: &str) -> PathBuf {
    dir.join(format!("{}.toml", clase.to_lowercase()))
}

fn monitor_de(salida: &str) -> Option<gdk::Monitor> {
    let monitores = gdk::Display::default()?.monitors();
    (0..monitores.n_items())
        .filter_map(|i| monitores.item(i)?.downcast::<gdk::Monitor>().ok())
        .find(|m| m.connector().is_some_and(|c| c == salida))
}

struct Partes {
    raiz: gtk::Overlay,
    tarjeta: gtk::Box,
    pliegue: Pliegue,
    /// Va encima de la tarjeta, no adentro, para que al plegarse ella hacia él no se deforme.
    cuadro: gtk::CenterBox,
    app: gtk::Label,
    esconder: gtk::Label,
    cuerpo: gtk::Box,
}

impl Partes {
    fn new() -> Self {
        let glifo = gtk::Label::new(Some(GLIFO));
        glifo.add_css_class("glifo");
        // Centrado por la tinta, como el OSD y el centro. Al realizarse y no antes: recién ahí el
        // label tiene el tamaño de letra de la hoja, y con otro tamaño la medida no sirve.
        glifo.connect_realize(crate::osd::centrar_tinta);
        let cuadro = gtk::CenterBox::new();
        cuadro.add_css_class("icono");
        cuadro.add_css_class("notas-boton");
        cuadro.set_center_widget(Some(&glifo));
        cuadro.set_cursor_from_name(Some("pointer"));
        cuadro.set_tooltip_text(Some("Minimizar"));

        let columna = gtk::Box::new(gtk::Orientation::Vertical, 0);
        columna.set_valign(gtk::Align::Center);
        columna.set_hexpand(true);
        columna.append(&texto("Notas", "titulo"));
        let app = texto("", "estado");
        columna.append(&app);
        let esconder = tecla("");
        esconder.add_css_class("esconder");
        esconder.set_valign(gtk::Align::Center);
        // El lugar del cuadrito en la cabecera, a la derecha: minimizada queda en la misma
        // esquina y el segundo clic no pide mover el puntero.
        let hueco = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        hueco.set_size_request(LADO_CUADRO, LADO_CUADRO);
        hueco.set_valign(gtk::Align::Center);

        let cabecera = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        cabecera.add_css_class("notas-cabecera");
        cabecera.append(&columna);
        cabecera.append(&esconder);
        cabecera.append(&hueco);

        let cuerpo = gtk::Box::new(gtk::Orientation::Vertical, 8);
        cuerpo.add_css_class("notas-cuerpo");
        let tarjeta = gtk::Box::new(gtk::Orientation::Vertical, 0);
        tarjeta.add_css_class("osd");
        tarjeta.add_css_class("notas");
        // Pegada a la esquina superior derecha: con otra hoja (minimizada o no) el cuadrito
        // sigue en la esquina.
        tarjeta.set_halign(gtk::Align::End);
        tarjeta.set_valign(gtk::Align::Start);
        tarjeta.append(&cabecera);
        tarjeta.append(&cuerpo);

        let pliegue = Pliegue::new(&tarjeta);
        let raiz = gtk::Overlay::new();
        raiz.set_child(Some(&pliegue));
        raiz.add_overlay(&cuadro);
        // El cuadrito, exactamente sobre su hueco.
        raiz.connect_get_child_position(move |raiz, _| {
            let r = hueco.compute_bounds(raiz)?;
            Some(gtk::gdk::Rectangle::new(
                r.x().round() as i32,
                r.y().round() as i32,
                r.width().round() as i32,
                r.height().round() as i32,
            ))
        });
        Partes {
            raiz,
            tarjeta,
            pliegue,
            cuadro,
            app,
            esconder,
            cuerpo,
        }
    }

    /// La ventana se pide de 1×1 para achicarse y sin esto tomaría el mínimo, con cada nota en
    /// varios renglones.
    fn fijar_ancho(&self) {
        self.tarjeta.set_size_request(-1, -1);
        let (_, natural, _, _) = self.tarjeta.measure(gtk::Orientation::Horizontal, -1);
        self.tarjeta.set_size_request(natural, -1);
    }

    fn minimizar(&self, minima: bool) {
        self.cuadro.set_tooltip_text(Some(if minima {
            "Abrir las notas"
        } else {
            "Minimizar"
        }));
        let centro = self
            .cuadro
            .compute_bounds(&self.pliegue)
            .map_or((0.0, 0.0), |r| {
                (r.x() + r.width() / 2.0, r.y() + r.height() / 2.0)
            });
        self.pliegue.plegar(minima, centro);
    }

    /// El atajo que la esconde, el de la config: sin tecla, no se muestra.
    fn poner_tecla(&self, tecla: &str) {
        self.esconder.set_text(&tecla_legible(tecla));
        self.esconder.set_visible(!tecla.is_empty());
    }

    fn dibujar(&self, hoja: &Hoja, clase: &str) {
        self.app.set_text(clase);
        let cuerpo = &self.cuerpo;
        while let Some(h) = cuerpo.first_child() {
            cuerpo.remove(&h);
        }
        for (titulo, entradas) in [("Atajos", &hoja.atajos), ("Comandos", &hoja.comandos)] {
            if entradas.is_empty() {
                continue;
            }
            let (caja, grilla) = seccion(titulo);
            let mut fila = 0;
            for e in entradas {
                match e {
                    Entrada::Nota(que, dice) => {
                        nota(&grilla, fila, que, dice);
                        fila += 1;
                    }
                    Entrada::Grupo(g) => {
                        let nombre = texto(&g.grupo, "notas-grupo");
                        grilla.attach(&nombre, 0, fila, 2, 1);
                        fila += 1;
                        for (que, dice) in &g.notas {
                            nota(&grilla, fila, que, dice);
                            fila += 1;
                        }
                    }
                }
            }
            cuerpo.append(&caja);
        }
        if !hoja.otras.is_empty() {
            let (caja, grilla) = seccion("Otras");
            for (n, t) in hoja.otras.iter().enumerate() {
                grilla.attach(&texto(t, "dice"), 0, n as i32, 2, 1);
            }
            cuerpo.append(&caja);
        }
        self.fijar_ancho();
    }
}

fn seccion(titulo: &str) -> (gtk::Box, gtk::Grid) {
    let caja = gtk::Box::new(gtk::Orientation::Vertical, 4);
    caja.add_css_class("notas-seccion");
    caja.append(&texto(titulo, "notas-titulo"));
    let grilla = gtk::Grid::new();
    grilla.set_column_spacing(10);
    grilla.set_row_spacing(3);
    caja.append(&grilla);
    (caja, grilla)
}

fn nota(grilla: &gtk::Grid, fila: i32, que: &str, dice: &str) {
    let teclas = gtk::Box::new(gtk::Orientation::Horizontal, 3);
    teclas.set_valign(gtk::Align::Start);
    for t in que.split(SEPARADOR) {
        teclas.append(&tecla(t));
    }
    grilla.attach(&teclas, 0, fila, 1, 1);
    let dice = texto(dice, "dice");
    dice.set_valign(gtk::Align::Center);
    grilla.attach(&dice, 1, fila, 1, 1);
}

/// `SUPER + ALT + N` (como la escribe `hl.bind`) → `Mod+Alt+N`.
fn tecla_legible(tecla: &str) -> String {
    tecla
        .split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| match p.to_ascii_uppercase().as_str() {
            "SUPER" | "MOD4" | "WIN" | "LOGO" => "Mod".to_owned(), // no-tr
            "ALT" | "MOD1" => "Alt".to_owned(), // no-tr
            "CTRL" | "CONTROL" => "Ctrl".to_owned(), // no-tr
            "SHIFT" => "Shift".to_owned(), // no-tr
            _ if p.chars().count() == 1 => p.to_uppercase(),
            _ => p.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

fn tecla(t: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(t));
    l.add_css_class("tecla");
    l
}

fn texto(t: &str, clase: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(t));
    l.add_css_class(clase);
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l.set_max_width_chars(CARACTERES);
    l
}

#[cfg(test)]
mod tests {

    #[test]
    fn la_tecla_como_se_lee() {
        assert_eq!(tecla_legible("SUPER + ALT + n"), "Mod+Alt+N");
        assert_eq!(tecla_legible("CTRL + SHIFT + space"), "Ctrl+Shift+space");
        assert_eq!(tecla_legible(""), "");
    }

    use super::*;

    #[test]
    fn el_titulo_no_cuenta() {
        let mut f = None;
        assert!(cuenta("activewindowv2>>5572a90e2070", &mut f));
        assert!(!cuenta("activewindowv2>>5572a90e2070", &mut f), "misma ventana: título");
        assert!(!cuenta("windowtitlev2>>5572a90e2070,fish", &mut f));
        assert!(cuenta("movewindowv2>>5572a90e2070,2,2", &mut f));
        assert!(cuenta("activewindowv2>>99", &mut f));
        assert!(cuenta("activewindowv2>>", &mut f), "sin ventana enfocada también");
    }

    #[test]
    fn la_hoja_nueva_se_lee() {
        let h = parse_hoja(&hoja_nueva("foot")).unwrap();
        assert_eq!(h.atajos, [Entrada::Nota("Ctrl+S".into(), "guardar".into())]);
        assert_eq!(h.comandos.len(), 1);
        assert!(hoja_nueva("foot").starts_with("# Notas de foot "));
    }

    const MONITORES: &str = r#"[
        {"id": 0, "name": "DP-3", "x": 0, "y": 0, "width": 2560, "height": 1440,
         "scale": 1.0, "transform": 0},
        {"id": 1, "name": "HDMI-A-1", "x": 2560, "y": 0, "width": 3840, "height": 2160,
         "scale": 2.0, "transform": 0}
    ]"#;

    fn kitty(at: [i32; 2], size: [i32; 2], monitor: i64, fullscreen: i64) -> String {
        format!(
            r#"{{"class": "kitty", "at": [{}, {}], "size": [{}, {}], "monitor": {monitor},
                "fullscreen": {fullscreen}, "title": "fish"}}"#,
            at[0], at[1], size[0], size[1]
        )
    }

    #[test]
    fn hoja_de_notas() {
        let h = parse_hoja(
            r#"
atajos = [["Ctrl+U", "borra hasta el inicio"]]
comandos = [["cd -", "vuelve al directorio anterior"]]
otras = ["algo"]
"#,
        )
        .unwrap();
        let nota = |a: &str, b: &str| Entrada::Nota(a.into(), b.into());
        assert_eq!(h.atajos, [nota("Ctrl+U", "borra hasta el inicio")]);
        assert_eq!(h.comandos.len(), 1);
        assert_eq!(h.otras, ["algo"]);
        assert!(parse_hoja("").unwrap().vacia());
        assert!(parse_hoja("atajo = []").is_err());
        assert!(parse_hoja(r#"atajos = [["solo la tecla"]]"#).is_err());
    }

    #[test]
    fn grupos_en_orden() {
        let h = parse_hoja(
            r#"
[[atajos]]
grupo = "Cursor"
notas = [["Ctrl+A", "inicio"], ["Ctrl+E", "final"]]

[[atajos]]
grupo = "Borrar"
notas = [["Ctrl+K", "hasta el final"]]
"#,
        )
        .unwrap();
        let grupos: Vec<_> = h
            .atajos
            .iter()
            .map(|e| match e {
                Entrada::Grupo(g) => (g.grupo.as_str(), g.notas.len()),
                Entrada::Nota(..) => ("suelta", 0),
            })
            .collect();
        assert_eq!(grupos, [("Cursor", 2), ("Borrar", 1)]);
        assert!(parse_hoja("[[atajos]]\ngrupo = \"x\"\nnota = []\n").is_err());
    }

    #[test]
    fn foco_en_la_salida() {
        let f = leer_foco(&kitty([1290, 46], [1252, 1376], 0, 0), MONITORES)
            .unwrap()
            .unwrap();
        assert_eq!((f.clase.as_str(), f.salida.as_str()), ("kitty", "DP-3"));
        assert_eq!((f.x, f.y, f.ancho_salida), (1290, 46, 2560));
        // La segunda salida: a la derecha y con escala 2.
        let f = leer_foco(&kitty([2576, 46], [900, 1000], 1, 0), MONITORES)
            .unwrap()
            .unwrap();
        assert_eq!((f.x, f.ancho_salida), (16, 1920));
        // Sin ventana enfocada, Hyprland contesta `{}`.
        assert_eq!(leer_foco("{}", MONITORES).unwrap(), None);
        assert_eq!(
            leer_foco(&kitty([0, 0], [800, 800], 7, 0), MONITORES).unwrap(),
            None
        );
    }

    #[test]
    fn esquina_de_la_ventana() {
        let f = leer_foco(&kitty([1290, 46], [1252, 1376], 0, 0), MONITORES)
            .unwrap()
            .unwrap();
        // 2560 − (1290 + 1252) = 18 hasta el borde de la salida, más 14 − 6.
        assert_eq!(
            lugar(&f),
            Some(Lugar {
                salida: "DP-3".into(),
                arriba: 46 + 36 - 6,
                derecha: 18 + 14 - 6,
            })
        );
        // Una columna que se sale por la derecha: al borde de la salida.
        let f = leer_foco(&kitty([2000, 46], [1252, 1376], 0, 0), MONITORES)
            .unwrap()
            .unwrap();
        assert_eq!(lugar(&f).unwrap().derecha, 14 - 6);
    }

    #[test]
    fn no_aparece() {
        let lee = |at, size, fs| {
            leer_foco(&kitty(at, size, 0, fs), MONITORES)
                .unwrap()
                .unwrap()
        };
        assert_eq!(lugar(&lee([0, 0], [2560, 1440], 2)), None);
        assert!(lugar(&lee([25, 45], [2518, 1378], 1)).is_some());
        assert_eq!(lugar(&lee([100, 100], [500, 900], 0)), None);
        assert_eq!(lugar(&lee([100, 100], [900, 300], 0)), None);
    }

    #[test]
    fn apps_elegidas() {
        let apps = vec!["kitty".to_owned(), "Yazi".to_owned()];
        assert!(elegida(&apps, "kitty"));
        assert!(elegida(&apps, "yazi"));
        assert!(!elegida(&apps, "brave-origin"));
        assert_eq!(
            ruta_hoja(Path::new("/c/notas"), "Yazi"),
            Path::new("/c/notas/yazi.toml")
        );
    }
}
