//! Donde se tocan los glows de dos tarjetas seguidas tiene que quedar encima el de la de
//! arriba, y un `GtkBox` pinta en el orden de sus hijos, o sea al revés.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Columna;

    #[glib::object_subclass]
    impl ObjectSubclass for Columna {
        const NAME: &'static str = "MorganColumna";
        type Type = super::Columna;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for Columna {}

    impl WidgetImpl for Columna {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let mut hijo = obj.last_child();
            while let Some(h) = hijo {
                obj.snapshot_child(&h, snapshot);
                hijo = h.prev_sibling();
            }
        }
    }

    impl BoxImpl for Columna {}
}

glib::wrapper! {
    pub struct Columna(ObjectSubclass<imp::Columna>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl Columna {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("orientation", gtk::Orientation::Vertical)
            .build()
    }
}
