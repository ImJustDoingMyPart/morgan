//! Una ventana por salida, que sigue a la lista de monitores de GDK: conectar o apagar una
//! pantalla no pide reiniciar nada. Ninguna toma el teclado ni el mouse.
//!
//! La forma es la del centro de control, para que pasar de un aviso a otro cambie el contenido
//! y no la caja. Todos miden lo mismo: un ancho fijo, acotado a la salida, y dos renglones.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

const MARGEN_INFERIOR: f64 = 0.15;

pub const ANCHO: i32 = 320;
const ALTO_TEXTO: i32 = 38;

#[derive(Debug, Clone, PartialEq)]
pub struct Aviso {
    pub icono: Option<String>,
    pub titulo: String,
    pub cuerpo: Cuerpo,
    pub ms: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cuerpo {
    Estado { texto: String, encendido: bool },
    /// `apagada`: el nivel que había antes de silenciar, atenuado.
    Barra {
        fraccion: f64,
        porcentaje: f64,
        apagada: bool,
    },
}

impl Aviso {
    pub fn estado(icono: &str, titulo: &str, texto: &str, encendido: bool, ms: u32) -> Aviso {
        Aviso {
            icono: Some(icono.into()),
            titulo: titulo.into(),
            cuerpo: Cuerpo::Estado {
                texto: texto.into(),
                encendido,
            },
            ms,
        }
    }
}

/// Un par de caracteres que no son ASCII.
pub fn es_glifo(icono: &str) -> bool {
    !icono.is_ascii() && icono.chars().count() <= 2
}

pub fn correccion(logica: (i32, i32, i32, i32), tinta: (i32, i32, i32, i32)) -> (i32, i32) {
    let centro = |x: i32, w: i32| 2 * x + w;
    let dx = (centro(logica.0, logica.2) - centro(tinta.0, tinta.2)) / 2;
    let dy = (centro(logica.1, logica.3) - centro(tinta.1, tinta.3)) / 2;
    (dx, dy)
}

/// GTK centra un glifo por el ancho que reserva la fuente, no por su dibujo: los de la Nerd
/// Font tienen la tinta corrida dentro de ese ancho, y cuánto depende de la fuente que
/// resuelva fontconfig. Se mide la tinta y se compensa con márgenes.
pub fn centrar_tinta(l: &gtk::Label) {
    let (tinta, logica) = l.layout().pixel_extents();
    let r = |r: gtk::pango::Rectangle| (r.x(), r.y(), r.width(), r.height());
    let (dx, dy) = correccion(r(logica), r(tinta));
    l.set_margin_start((2 * dx).max(0));
    l.set_margin_end((-2 * dx).max(0));
    l.set_margin_top((2 * dy).max(0));
    l.set_margin_bottom((-2 * dy).max(0));
}

pub fn porcentaje(p: f64, apagada: bool) -> String {
    if apagada {
        "mudo".into()
    } else {
        format!("{p:.0}%")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tope {
    pub ancho: i32,
    pub alto: i32,
    pub margen_inferior: i32,
}

pub fn tope(ancho: i32, alto: i32) -> Tope {
    let margen = (f64::from(alto) * MARGEN_INFERIOR).round() as i32;
    Tope {
        ancho: (ancho - 2 * margen).max(1),
        alto: (alto - 2 * margen).max(1),
        margen_inferior: margen,
    }
}

#[derive(Clone)]
pub struct Osd {
    inner: Rc<Inner>,
}

struct Inner {
    app: gtk::Application,
    ventanas: RefCell<Vec<Ventana>>,
    ocultar: RefCell<Option<glib::SourceId>>,
    /// Para volver a mostrarlo si las ventanas se rearman en el medio: prender o apagar una
    /// pantalla cambia los monitores justo cuando avisa.
    actual: RefCell<Option<Aviso>>,
}

impl Osd {
    pub fn new(app: &gtk::Application, display: &gdk::Display) -> Self {
        let inner = Rc::new(Inner {
            app: app.clone(),
            ventanas: RefCell::default(),
            ocultar: RefCell::default(),
            actual: RefCell::default(),
        });
        let monitores = display.monitors();
        let weak = Rc::downgrade(&inner);
        monitores.connect_items_changed(move |lista, _, _, _| {
            if let Some(inner) = weak.upgrade() {
                inner.rearmar(lista);
            }
        });
        inner.rearmar(&monitores);
        Osd { inner }
    }

    pub fn mostrar(&self, aviso: &Aviso) {
        for v in self.inner.ventanas.borrow().iter() {
            v.mostrar(aviso);
        }
        self.inner.actual.replace(Some(aviso.clone()));
        if let Some(id) = self.inner.ocultar.take() {
            id.remove();
        }
        let weak = Rc::downgrade(&self.inner);
        let id = glib::timeout_add_local_once(
            std::time::Duration::from_millis(aviso.ms.into()),
            move || {
                if let Some(inner) = weak.upgrade() {
                    inner.ocultar.take();
                    inner.actual.take();
                    for v in inner.ventanas.borrow().iter() {
                        v.window.set_visible(false);
                    }
                }
            },
        );
        self.inner.ocultar.replace(Some(id));
    }
}

impl Inner {
    /// Se rearman todas: los cambios de monitores son raros y así no hay que seguir posiciones.
    fn rearmar(&self, lista: &gio::ListModel) {
        for v in self.ventanas.take() {
            v.window.destroy();
        }
        let nuevas = (0..lista.n_items())
            .filter_map(|i| lista.item(i)?.downcast::<gdk::Monitor>().ok())
            .map(|m| Ventana::new(&self.app, &m))
            .collect::<Vec<_>>();
        // El que estaba sigue hasta su hora, ahora también en las salidas nuevas.
        if let Some(aviso) = self.actual.borrow().as_ref() {
            for v in &nuevas {
                v.mostrar(aviso);
            }
        }
        self.ventanas.replace(nuevas);
    }
}

struct Ventana {
    monitor: gdk::Monitor,
    window: gtk::ApplicationWindow,
    tarjeta: gtk::Box,
    icono: gtk::Image,
    glifo: gtk::Label,
    titulo: gtk::Label,
    porcentaje: gtk::Label,
    estado: gtk::Label,
    barra: gtk::ProgressBar,
}

impl Ventana {
    fn new(app: &gtk::Application, monitor: &gdk::Monitor) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.add_css_class("morgan");
        window.init_layer_shell();
        window.set_namespace(Some("morgan-osd"));
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_monitor(Some(monitor));
        window.set_anchor(Edge::Bottom, true);
        window.connect_realize(|w| {
            if let Some(s) = w.surface() {
                s.set_input_region(Some(&gtk::cairo::Region::create()));
            }
        });

        let icono = gtk::Image::new();
        icono.set_pixel_size(18);
        let glifo = gtk::Label::new(None);
        glifo.add_css_class("glifo");
        let dibujo = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dibujo.append(&icono);
        dibujo.append(&glifo);
        let cuadro = gtk::CenterBox::new();
        cuadro.add_css_class("icono");
        cuadro.set_valign(gtk::Align::Center);
        cuadro.set_hexpand(false);
        cuadro.set_center_widget(Some(&dibujo));

        let titulo = gtk::Label::new(None);
        titulo.add_css_class("titulo");
        titulo.set_xalign(0.0);
        titulo.set_hexpand(true);
        titulo.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let porcentaje = gtk::Label::new(None);
        porcentaje.add_css_class("porcentaje");
        let arriba = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        arriba.append(&titulo);
        arriba.append(&porcentaje);

        let estado = gtk::Label::new(None);
        estado.add_css_class("estado");
        estado.set_xalign(0.0);
        estado.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let barra = gtk::ProgressBar::new();
        barra.set_hexpand(true);
        barra.set_valign(gtk::Align::Center);

        let columna = gtk::Box::new(gtk::Orientation::Vertical, 2);
        columna.set_hexpand(true);
        columna.set_valign(gtk::Align::Center);
        // El alto de dos renglones aunque haya uno solo: todos los avisos miden lo mismo.
        columna.set_size_request(-1, ALTO_TEXTO);
        columna.append(&arriba);
        columna.append(&estado);
        columna.append(&barra);

        let tarjeta = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        tarjeta.add_css_class("osd");
        tarjeta.append(&cuadro);
        tarjeta.append(&columna);
        window.set_child(Some(&tarjeta));

        Ventana {
            monitor: monitor.clone(),
            window,
            tarjeta,
            icono,
            glifo,
            titulo,
            porcentaje,
            estado,
            barra,
        }
    }

    fn mostrar(&self, aviso: &Aviso) {
        // Un glifo va como texto (el mismo dibujo que en el centro); un nombre, del tema.
        let glifo = aviso.icono.as_deref().filter(|i| es_glifo(i));
        self.glifo.set_text(glifo.unwrap_or_default());
        self.glifo.set_visible(glifo.is_some());
        if glifo.is_some() {
            centrar_tinta(&self.glifo);
        }
        match aviso.icono.as_deref().filter(|i| !es_glifo(i)) {
            Some(nombre) => self.icono.set_icon_name(Some(nombre)),
            None => self.icono.clear(),
        }
        self.icono.set_visible(glifo.is_none());
        self.titulo.set_text(&aviso.titulo);
        let encendido = match &aviso.cuerpo {
            Cuerpo::Barra {
                fraccion,
                porcentaje: p,
                apagada,
            } => {
                self.barra.set_fraction(fraccion.clamp(0.0, 1.0));
                self.barra.set_sensitive(!apagada);
                self.porcentaje.set_text(&porcentaje(*p, *apagada));
                self.porcentaje.set_visible(true);
                self.barra.set_visible(true);
                self.estado.set_visible(false);
                if *apagada {
                    self.tarjeta.add_css_class("mudo");
                } else {
                    self.tarjeta.remove_css_class("mudo");
                }
                false
            }
            Cuerpo::Estado { texto, encendido } => {
                self.estado.set_text(texto);
                self.estado.set_visible(!texto.is_empty());
                self.porcentaje.set_visible(false);
                self.barra.set_visible(false);
                self.tarjeta.remove_css_class("mudo");
                *encendido
            }
        };
        if encendido {
            self.tarjeta.add_css_class("encendido");
        } else {
            self.tarjeta.remove_css_class("encendido");
        }
        let geo = self.monitor.geometry();
        let tope = tope(geo.width(), geo.height());
        self.window.set_margin(Edge::Bottom, tope.margen_inferior);
        // Ancho fijo, acotado a la salida; el alto es el de dos renglones, el mismo siempre.
        let ancho = ANCHO.min(tope.ancho);
        self.tarjeta.set_size_request(ancho, -1);
        // Sin tamaño propio: la ventana toma el de la tarjeta (una ya mapeada conservaría el
        // del aviso anterior).
        self.window.set_default_size(1, 1);
        self.window.set_visible(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tope_de_la_pantalla_principal() {
        // 2560×1440.
        assert_eq!(
            tope(2560, 1440),
            Tope {
                ancho: 2560 - 432,
                alto: 1440 - 432,
                margen_inferior: 216,
            }
        );
    }

    #[test]
    fn tope_del_anidado() {
        // 1252×1376: 206 px abajo.
        assert_eq!(tope(1252, 1376).margen_inferior, 206);
    }

    #[test]
    fn el_tope_entra_en_la_salida() {
        for (w, h) in [
            (2560, 1440),
            (1920, 1080),
            (1080, 1920),
            (800, 600),
            (3840, 2160),
        ] {
            let t = tope(w, h);
            assert!(t.ancho > 0 && t.ancho < w, "{w}×{h}: {t:?}");
            assert!(
                t.alto > 0 && t.margen_inferior + t.alto < h,
                "{w}×{h}: {t:?}"
            );
        }
    }

    #[test]
    fn la_tinta_va_al_centro() {
        // Caja de 20×24 en el origen; tinta de 12×12 pegada arriba a la izquierda: hay que
        // correrla 4 a la derecha y 6 abajo.
        assert_eq!(correccion((0, 0, 20, 24), (0, 0, 12, 12)), (4, 6));
        // Ya centrada: nada.
        assert_eq!(correccion((0, 0, 20, 20), (4, 4, 12, 12)), (0, 0));
        // Corrida a la derecha: hacia la izquierda.
        assert_eq!(correccion((0, 0, 20, 20), (8, 4, 12, 12)), (-4, 0));
    }

    #[test]
    fn glifo_o_nombre() {
        assert!(es_glifo("\u{f0594}"));
        assert!(!es_glifo("osd-volume-high-symbolic"));
        assert!(!es_glifo("tv-symbolic"));
        assert!(!es_glifo(""));
    }

    #[test]
    fn salida_degenerada() {
        let t = tope(1, 1);
        assert!(t.ancho >= 1 && t.alto >= 1);
    }
}
