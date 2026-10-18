//! Es un widget propio porque GTK no lo puede hacer con CSS: no hay `background-attachment:
//! fixed`, y un degradé en la perilla mide la perilla, no el riel.

use std::cell::RefCell;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk};

const ANCHO: i32 = 8;
const ANCHO_RIEL: f32 = 4.0;
const SEPARACION: i32 = 4;
const TINTE: f32 = 0x30 as f32 / 255.0;
const PERILLA_MIN: f32 = 8.0;
const GLOW: f32 = 0.3;
const GLOW_BLUR: f32 = 2.0;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Riel {
        pub adj: RefCell<Option<gtk::Adjustment>>,
        pub fin: RefCell<Option<gtk::Box>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Riel {
        const NAME: &'static str = "MorganRiel";
        type Type = super::Riel;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Riel {
        fn dispose(&self) {
            if let Some(f) = self.fin.take() {
                f.unparent();
            }
        }
    }

    impl WidgetImpl for Riel {
        fn measure(&self, o: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match o {
                gtk::Orientation::Horizontal => (ANCHO, ANCHO, -1, -1),
                _ => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, _ancho: i32, _alto: i32, _baseline: i32) {
            if let Some(f) = self.fin.borrow().as_ref() {
                f.allocate(0, 0, -1, None);
            }
        }

        fn snapshot(&self, s: &gtk::Snapshot) {
            let obj = self.obj();
            let (ancho, alto) = (obj.width() as f32, obj.height() as f32);
            if ancho <= 0.0 || alto <= 0.0 {
                return;
            }
            let principio = obj.color();
            let fin = self.fin.borrow().as_ref().map_or(principio, |f| f.color());
            let degrade = |s: &gtk::Snapshot, rect: &graphene::Rect, alfa: f32| {
                s.append_linear_gradient(
                    rect,
                    &graphene::Point::new(0.0, 0.0),
                    &graphene::Point::new(0.0, alto),
                    &[
                        gsk::ColorStop::new(0.0, con_alfa(&principio, alfa)),
                        gsk::ColorStop::new(1.0, con_alfa(&fin, alfa)),
                    ],
                );
            };
            let todo = graphene::Rect::new(0.0, 0.0, ancho, alto);

            let x = (ancho - ANCHO_RIEL) / 2.0;
            let riel = graphene::Rect::new(x, 0.0, ANCHO_RIEL, alto);
            s.push_rounded_clip(&redondeado(riel));
            degrade(s, &todo, TINTE);
            s.pop();

            let Some((y, h)) = self.perilla(alto) else {
                return;
            };
            let perilla = redondeado(graphene::Rect::new(0.0, y, ancho, h));
            let medio = mezcla(&principio, &fin, (y + h / 2.0) / alto);
            s.append_outset_shadow(&perilla, &con_alfa(&medio, GLOW), 0.0, 0.0, 0.0, GLOW_BLUR);
            s.push_rounded_clip(&perilla);
            degrade(s, &todo, 1.0);
            s.pop();
        }
    }

    impl Riel {
        pub fn perilla(&self, alto: f32) -> Option<(f32, f32)> {
            let adj = self.adj.borrow();
            let a = adj.as_ref()?;
            let total = (a.upper() - a.lower()) as f32;
            if total <= 0.0 {
                return Some((0.0, alto));
            }
            let h = ((a.page_size() as f32 / total).min(1.0) * alto).max(PERILLA_MIN.min(alto));
            let recorrido = total - a.page_size() as f32;
            let y = if recorrido > 0.0 {
                ((a.value() - a.lower()) as f32 / recorrido).clamp(0.0, 1.0) * (alto - h)
            } else {
                0.0
            };
            Some((y, h))
        }
    }
}

glib::wrapper! {
    pub struct Riel(ObjectSubclass<imp::Riel>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Riel {
    fn new(adj: &gtk::Adjustment) -> Self {
        let r: Riel = glib::Object::new();
        r.add_css_class("riel");
        r.set_vexpand(true);
        let fin = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        fin.add_css_class("fin");
        fin.set_parent(&r);
        r.imp().fin.replace(Some(fin));
        r.imp().adj.replace(Some(adj.clone()));

        let w = r.downgrade();
        adj.connect_changed(move |_| {
            if let Some(r) = w.upgrade() {
                r.queue_draw();
            }
        });
        let w = r.downgrade();
        adj.connect_value_changed(move |_| {
            if let Some(r) = w.upgrade() {
                r.queue_draw();
            }
        });

        // Arrastrar (o un click) lleva el medio de la perilla al puntero.
        let arrastre = gtk::GestureDrag::new();
        let w = r.downgrade();
        arrastre.connect_drag_begin(move |_, _, y| {
            if let Some(r) = w.upgrade() {
                r.llevar(y);
            }
        });
        let w = r.downgrade();
        arrastre.connect_drag_update(move |g, _, dy| {
            if let (Some(r), Some((_, y0))) = (w.upgrade(), g.start_point()) {
                r.llevar(y0 + dy);
            }
        });
        r.add_controller(arrastre);

        let rueda = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let a = adj.clone();
        rueda.connect_scroll(move |_, _, dy| {
            a.set_value(a.value() + dy * a.step_increment().max(1.0));
            glib::Propagation::Stop
        });
        r.add_controller(rueda);
        r
    }

    fn llevar(&self, y: f64) {
        let Some(a) = self.imp().adj.borrow().clone() else {
            return;
        };
        let alto = self.height() as f64;
        if alto <= 0.0 {
            return;
        }
        let total = a.upper() - a.lower();
        let v = a.lower() + y / alto * total - a.page_size() / 2.0;
        a.set_value(v.clamp(a.lower(), (a.upper() - a.page_size()).max(a.lower())));
    }
}

/// El scroll deja de dibujar su barra (`External`), pero sigue desplazándose.
pub fn con_riel(scroll: &gtk::ScrolledWindow) -> gtk::Box {
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::External);
    scroll.set_hexpand(true);
    let caja = gtk::Box::new(gtk::Orientation::Horizontal, SEPARACION);
    caja.append(scroll);
    caja.append(&Riel::new(&scroll.vadjustment()));
    caja
}

fn redondeado(r: graphene::Rect) -> gsk::RoundedRect {
    let radio = r.width() / 2.0;
    let esquina = graphene::Size::new(radio, radio);
    gsk::RoundedRect::new(r, esquina, esquina, esquina, esquina)
}

fn con_alfa(c: &gdk::RGBA, alfa: f32) -> gdk::RGBA {
    gdk::RGBA::new(c.red(), c.green(), c.blue(), c.alpha() * alfa)
}

fn mezcla(a: &gdk::RGBA, b: &gdk::RGBA, t: f32) -> gdk::RGBA {
    let t = t.clamp(0.0, 1.0);
    let m = |x: f32, y: f32| x + (y - x) * t;
    gdk::RGBA::new(
        m(a.red(), b.red()),
        m(a.green(), b.green()),
        m(a.blue(), b.blue()),
        m(a.alpha(), b.alpha()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mezcla_en_las_puntas_y_el_medio() {
        let a = gdk::RGBA::new(0.0, 0.0, 0.0, 1.0);
        let b = gdk::RGBA::new(1.0, 0.5, 0.0, 1.0);
        assert_eq!(mezcla(&a, &b, 0.0), a);
        assert_eq!(mezcla(&a, &b, 1.0), b);
        assert_eq!(mezcla(&a, &b, 0.5), gdk::RGBA::new(0.5, 0.25, 0.0, 1.0));
        assert_eq!(mezcla(&a, &b, 2.0), b);
    }
}
