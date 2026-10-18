//! La superficie cubre la salida para cerrar con un click afuera, así que el `slide top` de
//! Hyprland arrastra la capa entera: un panel pegado arriba asoma recién en el último tercio y
//! parece que tarda. Por eso la capa entra con `fade` y lo que se ve baja acá, desde su propio
//! borde de abajo.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, graphene};

/// Lo que tarda el fundido de las capas a velocidad media.
const BASE: Duration = Duration::from_millis(150);

pub fn duracion() -> Duration {
    crate::animaciones::dur(BASE)
}

pub fn desplazamiento(micros: i64, fondo: f32) -> f32 {
    let t = (micros as f64 / duracion().as_micros() as f64).clamp(0.0, 1.0);
    let falta = (1.0 - t).powi(3);
    (falta * f64::from(fondo)) as f32
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Bajada {
        pub hijo: RefCell<Option<gtk::Widget>>,
        pub inicio: Cell<Option<i64>>,
        pub ahora: Cell<i64>,
        pub animando: RefCell<Option<gtk::TickCallbackId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Bajada {
        const NAME: &'static str = "MorganBajada";
        type Type = super::Bajada;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Bajada {
        fn dispose(&self) {
            if let Some(id) = self.animando.take() {
                id.remove();
            }
            if let Some(h) = self.hijo.take() {
                h.unparent();
            }
        }
    }

    impl WidgetImpl for Bajada {
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
            let dy = if let Some(inicio) = self.inicio.get() {
                let fondo = h
                    .compute_bounds(&*obj)
                    .map_or(0.0, |r| r.y() + r.height());
                desplazamiento(self.ahora.get() - inicio, fondo)
            } else {
                0.0
            };
            if dy > 0.0 {
                snapshot.save();
                snapshot.translate(&graphene::Point::new(0.0, -dy));
                obj.snapshot_child(&h, snapshot);
                snapshot.restore();
            } else {
                obj.snapshot_child(&h, snapshot);
            }
        }
    }
}

glib::wrapper! {
    pub struct Bajada(ObjectSubclass<imp::Bajada>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Bajada {
    pub fn new(hijo: &impl IsA<gtk::Widget>) -> Self {
        let e: Bajada = glib::Object::new();
        hijo.set_parent(&e);
        e.imp().hijo.replace(Some(hijo.clone().upcast()));
        e
    }

    /// Antes de mostrar la ventana: el primer cuadro ya sale con el panel arriba, afuera.
    pub fn arrancar(&self) {
        let imp = self.imp();
        if let Some(id) = imp.animando.take() {
            id.remove();
        }
        // `Some(0)`: todavía no hubo cuadro; hasta entonces falta todo.
        imp.inicio.set(Some(0));
        imp.ahora.set(0);
        self.queue_draw();
        let id = self.add_tick_callback(|e, reloj| {
            let imp = e.imp();
            let ahora = reloj.frame_time();
            let inicio = match imp.inicio.get() {
                Some(i) if i != 0 => i,
                _ => {
                    imp.inicio.set(Some(ahora));
                    ahora
                }
            };
            imp.ahora.set(ahora);
            e.queue_draw();
            if ahora - inicio >= duracion().as_micros() as i64 {
                imp.inicio.set(None);
                imp.animando.take();
                e.queue_draw();
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
    fn arranca_con_todo_el_alto_afuera_y_termina_en_su_lugar() {
        assert_eq!(desplazamiento(0, 600.0), 600.0);
        assert_eq!(desplazamiento(duracion().as_micros() as i64, 600.0), 0.0);
        assert_eq!(
            desplazamiento(10 * duracion().as_micros() as i64, 600.0),
            0.0
        );
    }

    #[test]
    fn baja_mas_rapido_al_principio() {
        let mitad = desplazamiento(duracion().as_micros() as i64 / 2, 600.0);
        assert!(
            mitad < 300.0 * 0.5,
            "a la mitad del tiempo ya bajó más de la mitad"
        );
    }
}
