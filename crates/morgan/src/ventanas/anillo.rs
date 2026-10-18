//! Dibujado encima de las celdas para que se deslice de una a otra en vez de saltar. No
//! recibe clicks, y el borde es la máscara del degradé: lo de adentro queda a la vista.

use std::cell::{Cell, RefCell};

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk};

/// Más que el del launcher: los saltos son más largos.
const DESLIZAR_US: i64 = 160_000;
const GROSOR: f32 = 3.0;
const RADIO: f32 = 6.0;

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

pub fn interpolar(a: [f32; 4], b: [f32; 4], f: f32) -> [f32; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * f)
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Anillo {
        /// Solo da el color del final del degradé: Pango y gsk no saben de los roles de matugen.
        pub fin: RefCell<Option<gtk::Widget>>,
        pub desde: Cell<Option<[f32; 4]>>,
        pub hasta: Cell<Option<[f32; 4]>>,
        pub t0: Cell<i64>,
        pub dibujado: Cell<Option<[f32; 4]>>,
        pub corriendo: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Anillo {
        const NAME: &'static str = "MorganVentanasAnillo";
        type Type = super::Anillo;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Anillo {
        fn constructed(&self) {
            self.parent_constructed();
            let fin = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            fin.add_css_class("fin");
            fin.set_parent(&*self.obj());
            self.fin.replace(Some(fin.upcast()));
        }

        fn dispose(&self) {
            if let Some(f) = self.fin.take() {
                f.unparent();
            }
        }
    }

    impl WidgetImpl for Anillo {
        fn size_allocate(&self, _: i32, _: i32, _: i32) {
            // El de `fin` no se dibuja: solo tiene que existir para que tenga estilo.
            if let Some(f) = self.fin.borrow().as_ref() {
                f.allocate(0, 0, -1, None);
            }
        }

        fn snapshot(&self, s: &gtk::Snapshot) {
            let Some(hasta) = self.hasta.get() else {
                return;
            };
            let f = ease_out(
                (glib::monotonic_time() - self.t0.get()) as f64
                    / crate::animaciones::us(DESLIZAR_US) as f64,
            );
            let [x, y, w, h] = match self.desde.get() {
                Some(desde) => interpolar(desde, hasta, f as f32),
                None => hasta,
            };
            self.dibujado.set(Some([x, y, w, h]));
            let obj = self.obj();
            let primero = obj.color();
            let ultimo = self
                .fin
                .borrow()
                .as_ref()
                .map_or(primero, |f| f.color());
            let rect = graphene::Rect::new(x, y, w, h);
            let borde = gsk::RoundedRect::from_rect(rect, RADIO);
            let glow = gdk::RGBA::new(primero.red(), primero.green(), primero.blue(), 0.45);
            s.append_outset_shadow(&borde, &glow, 0.0, 0.0, 1.0, 8.0);
            // El degradé a 45°, enmascarado por el borde: de abajo a la izquierda a arriba a la
            // derecha, como el `linear-gradient(45deg, …)` de la hoja.
            s.push_mask(gsk::MaskMode::Alpha);
            let blanco = gdk::RGBA::WHITE;
            s.append_border(&borde, &[GROSOR; 4], &[blanco; 4]);
            s.pop();
            s.append_linear_gradient(
                &rect,
                &graphene::Point::new(x, y + h),
                &graphene::Point::new(x + w, y),
                &[
                    gsk::ColorStop::new(0.0, primero),
                    gsk::ColorStop::new(1.0, ultimo),
                ],
            );
            s.pop();
        }
    }
}

glib::wrapper! {
    pub struct Anillo(ObjectSubclass<imp::Anillo>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Anillo {
    pub fn new() -> Self {
        let a: Anillo = glib::Object::new();
        a.add_css_class("anillo");
        a.set_can_target(false);
        a
    }

    pub fn ir(&self, rect: [f32; 4], deslizar: bool) {
        let imp = self.imp();
        imp.desde
            .set(if deslizar { imp.dibujado.get() } else { None });
        imp.hasta.set(Some(rect));
        imp.t0.set(glib::monotonic_time());
        self.queue_draw();
        if !deslizar || imp.corriendo.replace(true) {
            return;
        }
        self.add_tick_callback(|a, _| {
            a.queue_draw();
            if glib::monotonic_time() - a.imp().t0.get() < crate::animaciones::us(DESLIZAR_US) {
                glib::ControlFlow::Continue
            } else {
                a.imp().corriendo.set(false);
                glib::ControlFlow::Break
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolar_va_de_un_rectangulo_al_otro() {
        let a = [0.0, 0.0, 10.0, 10.0];
        let b = [100.0, 50.0, 20.0, 30.0];
        assert_eq!(interpolar(a, b, 0.0), a);
        assert_eq!(interpolar(a, b, 1.0), b);
        assert_eq!(interpolar(a, b, 0.5), [50.0, 25.0, 15.0, 20.0]);
    }

    #[test]
    fn ease_out_satura() {
        assert_eq!(ease_out(-1.0), 0.0);
        assert_eq!(ease_out(1.5), 1.0);
        assert!(ease_out(0.25) > 0.25);
    }
}
