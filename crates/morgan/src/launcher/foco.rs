//! Nada de esto demora: Enter actúa aunque haya un movimiento a mitad de camino.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, graphene, gsk};

const RECUADRO_US: i64 = 100_000;
const SCROLL_US: i64 = 120_000;
const RADIO: f32 = 4.0;

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Paso {
    #[default]
    Saltar,
    Deslizar,
    Aparecer,
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Foco {
        pub lista: RefCell<Option<gtk::ListBox>>,
        pub proximo: Cell<Paso>,
        pub paso: Cell<Paso>,
        pub desde: Cell<Option<(f32, f32)>>,
        pub t0: Cell<i64>,
        pub dibujado: Cell<Option<(f32, f32)>>,
        pub corriendo: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Foco {
        const NAME: &'static str = "MorganLauncherFoco";
        type Type = super::Foco;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Foco {
        fn dispose(&self) {
            if let Some(l) = self.lista.take() {
                l.unparent();
            }
        }
    }

    impl WidgetImpl for Foco {
        fn measure(&self, o: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            self.lista
                .borrow()
                .as_ref()
                .map_or((0, 0, -1, -1), |l| l.measure(o, for_size))
        }

        fn size_allocate(&self, ancho: i32, alto: i32, base: i32) {
            if let Some(l) = self.lista.borrow().as_ref() {
                l.allocate(ancho, alto, base, None);
            }
        }

        fn snapshot(&self, s: &gtk::Snapshot) {
            let obj = self.obj();
            let Some(lista) = self.lista.borrow().clone() else {
                return;
            };
            if let Some((y, alto, alfa)) = self.recuadro(&obj, &lista) {
                let c = obj.color();
                let c = gtk::gdk::RGBA::new(c.red(), c.green(), c.blue(), c.alpha() * alfa);
                let r = graphene::Rect::new(0.0, y, obj.width() as f32, alto);
                s.push_rounded_clip(&gsk::RoundedRect::from_rect(r, RADIO));
                s.append_color(&c, &r);
                s.pop();
            }
            obj.snapshot_child(&lista, s);
        }
    }

    impl Foco {
        fn recuadro(&self, obj: &super::Foco, lista: &gtk::ListBox) -> Option<(f32, f32, f32)> {
            let fila = lista.selected_row()?;
            let b = fila.compute_bounds(obj)?;
            let destino = (b.y(), b.height());
            let t = glib::monotonic_time() - self.t0.get();
            let f = ease_out(t as f64 / crate::animaciones::us(RECUADRO_US) as f64) as f32;
            let (y, alto, alfa) = match (self.paso.get(), self.desde.get()) {
                (Paso::Deslizar, Some((y0, h0))) => (
                    y0 + (destino.0 - y0) * f,
                    h0 + (destino.1 - h0) * f,
                    1.0,
                ),
                (Paso::Aparecer, _) => (destino.0, destino.1, f),
                _ => (destino.0, destino.1, 1.0),
            };
            self.dibujado.set(Some((y, alto)));
            Some((y, alto, alfa))
        }
    }
}

glib::wrapper! {
    pub struct Foco(ObjectSubclass<imp::Foco>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Foco {
    pub fn new(lista: &gtk::ListBox) -> Self {
        let f: Foco = glib::Object::new();
        f.add_css_class("launcher-foco");
        lista.add_css_class("con-foco");
        lista.set_parent(&f);
        f.imp().lista.replace(Some(lista.clone()));
        let yo = f.downgrade();
        lista.connect_row_selected(move |_, _| {
            if let Some(f) = yo.upgrade() {
                f.seguir();
            }
        });
        f
    }

    pub fn proximo(&self, paso: Paso) {
        self.imp().proximo.set(paso);
    }

    fn seguir(&self) {
        let imp = self.imp();
        let paso = imp.proximo.take();
        imp.paso.set(paso);
        imp.desde.set(imp.dibujado.get());
        imp.t0.set(glib::monotonic_time());
        self.queue_draw();
        if paso == Paso::Saltar || imp.corriendo.replace(true) {
            return;
        }
        self.add_tick_callback(|f, _| {
            f.queue_draw();
            if glib::monotonic_time() - f.imp().t0.get() < crate::animaciones::us(RECUADRO_US) {
                glib::ControlFlow::Continue
            } else {
                f.imp().corriendo.set(false);
                glib::ControlFlow::Break
            }
        });
    }
}

#[derive(Clone)]
pub struct Scroll {
    scroll: gtk::ScrolledWindow,
    recorrido: Rc<Cell<Option<(f64, f64, i64)>>>,
    corriendo: Rc<Cell<bool>>,
}

impl Scroll {
    pub fn new(scroll: &gtk::ScrolledWindow) -> Self {
        Scroll {
            scroll: scroll.clone(),
            recorrido: Rc::default(),
            corriendo: Rc::default(),
        }
    }

    pub fn destino(&self) -> f64 {
        self.recorrido
            .get()
            .map_or_else(|| self.scroll.vadjustment().value(), |(_, a, _)| a)
    }

    pub fn pagina(&self) -> f64 {
        self.scroll.vadjustment().page_size()
    }

    pub fn poner(&self, valor: f64) {
        self.recorrido.set(None);
        self.scroll.vadjustment().set_value(valor);
    }

    pub fn ir(&self, valor: f64) {
        let adj = self.scroll.vadjustment();
        self.recorrido
            .set(Some((adj.value(), valor, glib::monotonic_time())));
        if self.corriendo.replace(true) {
            return;
        }
        let (recorrido, corriendo) = (self.recorrido.clone(), self.corriendo.clone());
        self.scroll.add_tick_callback(move |s, _| {
            let Some((desde, hasta, t0)) = recorrido.get() else {
                corriendo.set(false);
                return glib::ControlFlow::Break;
            };
            let f = ease_out(
                (glib::monotonic_time() - t0) as f64 / crate::animaciones::us(SCROLL_US) as f64,
            );
            s.vadjustment().set_value(desde + (hasta - desde) * f);
            if f < 1.0 {
                glib::ControlFlow::Continue
            } else {
                recorrido.set(None);
                corriendo.set(false);
                glib::ControlFlow::Break
            }
        });
    }
}

/// Con un tope, por si no se pinta.
pub fn despues_de_pintar(w: &gtk::Widget, f: impl FnOnce() + 'static) {
    let f = Rc::new(RefCell::new(Some(f)));
    let correr = {
        let f = f.clone();
        move || {
            if let Some(f) = f.borrow_mut().take() {
                f();
            }
        }
    };
    let Some(reloj) = w.frame_clock() else {
        correr();
        return;
    };
    let id: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::default();
    let (c, id2) = (correr.clone(), id.clone());
    let h = reloj.connect_after_paint(move |r| {
        if let Some(h) = id2.borrow_mut().take() {
            r.disconnect(h);
        }
        c();
    });
    id.replace(Some(h));
    w.queue_draw();
    glib::timeout_add_local_once(std::time::Duration::from_millis(100), correr);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_out_va_de_cero_a_uno() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_out(2.0), 1.0);
        assert!(ease_out(0.5) > 0.5);
    }
}
