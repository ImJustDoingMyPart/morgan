//! El cuadrito del ícono va encima, en otra capa del `Overlay`, así que no se deforma. El
//! tamaño no cambia (la capa de Hyprland no se entera y no anima nada): es solo lo que se pinta.

use std::cell::{Cell, RefCell};

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, graphene};

use crate::bajada::duracion;

pub fn pliegue(micros: i64, desde: f64, hacia: f64) -> f64 {
    let t = (micros as f64 / duracion().as_micros() as f64).clamp(0.0, 1.0);
    // Al final, el destino tal cual: con flotantes, `desde + (hacia - desde)` puede no serlo
    // y la animación no terminaría.
    if t >= 1.0 {
        return hacia;
    }
    let e = 1.0 - (1.0 - t).powi(3);
    desde + (hacia - desde) * e
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Pliegue {
        pub hijo: RefCell<Option<gtk::Widget>>,
        pub valor: Cell<f64>,
        pub desde: Cell<f64>,
        pub hacia: Cell<f64>,
        pub punto: Cell<(f32, f32)>,
        pub inicio: Cell<Option<i64>>,
        pub animando: RefCell<Option<gtk::TickCallbackId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Pliegue {
        const NAME: &'static str = "MorganPliegue";
        type Type = super::Pliegue;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Pliegue {
        fn dispose(&self) {
            if let Some(id) = self.animando.take() {
                id.remove();
            }
            if let Some(h) = self.hijo.take() {
                h.unparent();
            }
        }
    }

    impl WidgetImpl for Pliegue {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            self.hijo
                .borrow()
                .as_ref()
                .map_or(gtk::SizeRequestMode::ConstantSize, |h| h.request_mode())
        }

        fn measure(&self, o: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            match self.hijo.borrow().as_ref() {
                Some(h) => h.measure(o, for_size),
                None => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, ancho: i32, alto: i32, baseline: i32) {
            if let Some(h) = self.hijo.borrow().as_ref() {
                h.allocate(ancho, alto, baseline, None);
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let Some(h) = self.hijo.borrow().clone() else {
                return;
            };
            let obj = self.obj();
            let p = self.valor.get();
            if p <= 0.0 {
                return obj.snapshot_child(&h, snapshot);
            }
            if p >= 1.0 {
                return;
            }
            let (x, y) = self.punto.get();
            let escala = (1.0 - p) as f32;
            snapshot.push_opacity(1.0 - p);
            snapshot.save();
            snapshot.translate(&graphene::Point::new(x, y));
            snapshot.scale(escala, escala);
            snapshot.translate(&graphene::Point::new(-x, -y));
            obj.snapshot_child(&h, snapshot);
            snapshot.restore();
            snapshot.pop();
        }
    }
}

glib::wrapper! {
    pub struct Pliegue(ObjectSubclass<imp::Pliegue>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Pliegue {
    pub fn new(hijo: &impl IsA<gtk::Widget>) -> Self {
        let e: Pliegue = glib::Object::new();
        hijo.set_parent(&e);
        e.imp().hijo.replace(Some(hijo.clone().upcast()));
        e
    }

    /// A mitad de camino sigue desde donde está, en vez de saltar al principio.
    pub fn plegar(&self, plegada: bool, punto: (f32, f32)) {
        let imp = self.imp();
        if let Some(id) = imp.animando.take() {
            id.remove();
        }
        imp.punto.set(punto);
        imp.desde.set(imp.valor.get());
        imp.hacia.set(if plegada { 1.0 } else { 0.0 });
        imp.inicio.set(None);
        let id = self.add_tick_callback(|e, reloj| {
            let imp = e.imp();
            let ahora = reloj.frame_time();
            let inicio = *imp.inicio.get().get_or_insert(ahora);
            imp.inicio.set(Some(inicio));
            let v = pliegue(ahora - inicio, imp.desde.get(), imp.hacia.get());
            imp.valor.set(v);
            e.queue_draw();
            if v == imp.hacia.get() {
                imp.animando.take();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
        imp.animando.replace(Some(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pliega_y_abre_en_la_duracion_de_la_bajada() {
        let fin = duracion().as_micros() as i64;
        assert_eq!(pliegue(0, 0.0, 1.0), 0.0);
        assert_eq!(pliegue(fin, 0.0, 1.0), 1.0);
        assert_eq!(pliegue(fin * 2, 0.0, 1.0), 1.0);
        assert_eq!(pliegue(fin, 1.0, 0.0), 0.0);
        // Salida suave: a mitad de tiempo ya hizo más de la mitad.
        assert!(pliegue(fin / 2, 0.0, 1.0) > 0.8);
        // Dada vuelta a mitad de camino, sale de donde estaba.
        assert_eq!(pliegue(0, 0.6, 0.0), 0.6);
        assert_eq!(pliegue(fin, 0.3, 1.0), 1.0);
    }
}
