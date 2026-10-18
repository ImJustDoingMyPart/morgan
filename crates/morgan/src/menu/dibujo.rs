//! Los monitores a escala, para ubicarlos con el mouse: cada uno es una tarjeta del menú que
//! se arrastra y, al soltarla, se pega al lado más cercano de otro (`monitores::encajar`).
//! La apagada se ve atenuada en el lugar que va a tener al prenderse.

use std::cell::Cell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use morgan_idioma::tr;

use super::monitores::{Campo, Pantalla, encajar};

const ALTO: i32 = 180;
/// Aire alrededor, para poder arrastrar una pantalla más allá de las otras.
const AIRE: f64 = 28.0;
/// Menos que esto es un toque, no un arrastre.
const TOQUE: f64 = 3.0;

struct Dibujo {
    pantallas: Vec<Pantalla>,
    botones: Vec<gtk::Button>,
    fijo: gtk::Fixed,
    /// Píxeles de pantalla por píxel del dibujo, y dónde cae el origen del escritorio.
    escala: Cell<f64>,
    origen: Cell<(f64, f64)>,
}

impl Dibujo {
    fn a_dibujo(&self, x: i32, y: i32) -> (f64, f64) {
        let (ox, oy) = self.origen.get();
        let e = self.escala.get();
        (ox + x as f64 * e, oy + y as f64 * e)
    }

    fn a_escritorio(&self, x: f64, y: f64) -> (f64, f64) {
        let (ox, oy) = self.origen.get();
        let e = self.escala.get();
        ((x - ox) / e, (y - oy) / e)
    }

    /// Todas en su lugar, a la escala que entra en `ancho × alto`.
    fn acomodar(&self, ancho: i32, alto: i32) {
        let p = &self.pantallas;
        let x0 = p.iter().map(|p| p.x).min().unwrap_or(0) as f64;
        let y0 = p.iter().map(|p| p.y).min().unwrap_or(0) as f64;
        let x1 = p.iter().map(|p| p.x + p.ancho).max().unwrap_or(1) as f64;
        let y1 = p.iter().map(|p| p.y + p.alto).max().unwrap_or(1) as f64;
        let e = ((ancho as f64 - 2.0 * AIRE) / (x1 - x0))
            .min((alto as f64 - 2.0 * AIRE) / (y1 - y0))
            .max(0.001);
        self.escala.set(e);
        // Centrado.
        let ox = (ancho as f64 - (x1 - x0) * e) / 2.0 - x0 * e;
        let oy = (alto as f64 - (y1 - y0) * e) / 2.0 - y0 * e;
        self.origen.set((ox, oy));
        for (p, b) in p.iter().zip(&self.botones) {
            let (x, y) = self.a_dibujo(p.x, p.y);
            b.set_size_request(
                (p.ancho as f64 * e).round() as i32,
                (p.alto as f64 * e).round() as i32,
            );
            self.fijo.move_(b, x.round(), y.round());
        }
    }
}

/// `al_soltar` recibe la tarjeta soltada, la salida y lo que hay que guardar.
pub fn nuevo(
    pantallas: &[Pantalla],
    al_soltar: impl Fn(&gtk::Widget, &str, Campo) + 'static,
) -> gtk::Widget {
    let fijo = gtk::Fixed::new();
    let botones: Vec<gtk::Button> = pantallas.iter().map(tarjeta).collect();
    for b in &botones {
        fijo.put(b, 0.0, 0.0);
    }
    let d = Rc::new(Dibujo {
        pantallas: pantallas.to_vec(),
        botones,
        fijo: fijo.clone(),
        escala: Cell::new(1.0),
        origen: Cell::new((0.0, 0.0)),
    });

    // El área vacía da el tamaño disponible; las tarjetas van encima.
    let area = gtk::DrawingArea::new();
    area.set_hexpand(true);
    area.set_content_height(ALTO);
    let caja = gtk::Overlay::new();
    caja.add_css_class("menu-dibujo");
    caja.set_overflow(gtk::Overflow::Hidden);
    caja.set_child(Some(&area));
    caja.add_overlay(&fijo);
    let dd = d.clone();
    area.connect_resize(move |_, ancho, alto| {
        // Mover hijos durante la asignación de tamaño la vuelve a pedir: al cuadro siguiente.
        let dd = dd.clone();
        glib::idle_add_local_once(move || dd.acomodar(ancho, alto));
    });

    let al_soltar = Rc::new(al_soltar);
    for (n, b) in d.botones.iter().enumerate() {
        let arrastre = gtk::GestureDrag::new();
        arrastre.set_propagation_phase(gtk::PropagationPhase::Capture);
        let desde = Rc::new(Cell::new((0.0, 0.0)));
        let (dd, de) = (d.clone(), desde.clone());
        arrastre.connect_drag_begin(move |_, _, _| {
            let p = &dd.pantallas[n];
            de.set(dd.a_dibujo(p.x, p.y));
            dd.botones[n].add_css_class("abierto");
        });
        let (dd, de) = (d.clone(), desde.clone());
        arrastre.connect_drag_update(move |_, dx, dy| {
            let (x, y) = de.get();
            dd.fijo.move_(&dd.botones[n], x + dx, y + dy);
        });
        let (dd, de, soltar) = (d.clone(), desde, al_soltar.clone());
        arrastre.connect_drag_end(move |_, dx, dy| {
            let b = &dd.botones[n];
            b.remove_css_class("abierto");
            let p = &dd.pantallas[n];
            let (x0, y0) = de.get();
            let quieta = dx.hypot(dy) < TOQUE;
            let (lx, ly) = dd.a_escritorio(x0 + dx, y0 + dy);
            let (x, y) = if quieta { (p.x, p.y) } else { encajar(&dd.pantallas, n, lx, ly) };
            let (px, py) = dd.a_dibujo(x, y);
            dd.fijo.move_(b, px.round(), py.round());
            if (x, y) != (p.x, p.y) {
                soltar(b.upcast_ref(), &p.nombre, p.lugar(x, y));
            }
        });
        b.add_controller(arrastre);
    }
    caja.upcast()
}

fn tarjeta(p: &Pantalla) -> gtk::Button {
    let textos = gtk::Box::new(gtk::Orientation::Vertical, 2);
    textos.set_valign(gtk::Align::Center);
    let t = gtk::Label::new(Some(&p.titulo));
    t.add_css_class("titulo");
    t.set_ellipsize(gtk::pango::EllipsizeMode::End);
    textos.append(&t);
    let sub = if p.apagada {
        tr!("apagado").to_owned()
    } else {
        format!("{} × {}", p.ancho, p.alto) // no-tr
    };
    let s = gtk::Label::new(Some(&sub));
    s.add_css_class("estado");
    s.set_ellipsize(gtk::pango::EllipsizeMode::End);
    textos.append(&s);
    let b = gtk::Button::new();
    b.set_child(Some(&textos));
    b.add_css_class("cc-tarjeta");
    if p.apagada {
        b.add_css_class("apagada");
    }
    b.set_can_focus(false);
    b.set_overflow(gtk::Overflow::Hidden);
    b.set_cursor_from_name(Some("grab"));
    b.set_tooltip_text(Some(tr!("Arrastralo para ubicarlo")));
    b
}
