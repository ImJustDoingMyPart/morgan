//! Las listas desplegables con GTK, que siguen usando el menú de sistema y los toasts (el
//! centro ya no: `dibujo/armar.rs`). Todas las listas son del mismo `Grupo`: abrir una cierra
//! las otras. Una opción marcada lo está porque el sistema lo dice (`Item::marcada`); el click
//! solo pide.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

pub struct Item {
    pub glifo: String,
    pub texto: String,
    pub marcada: Option<bool>,
    pub accion: Rc<dyn Fn()>,
}

impl Item {
    pub fn new(
        glifo: impl Into<String>,
        texto: impl Into<String>,
        marcada: Option<bool>,
        f: impl Fn() + 'static,
    ) -> Self {
        Item {
            glifo: glifo.into(),
            texto: texto.into(),
            marcada,
            accion: Rc::new(f),
        }
    }
}

/// Sin retenerlas: el disparador retiene su lista (por su señal) y la lista retiene el
/// grupo, así que con referencias fuertes una página del menú que se repinta no se liberaría.
#[derive(Clone, Default)]
pub struct Grupo {
    listas: Rc<RefCell<Vec<glib::WeakRef<gtk::Revealer>>>>,
}

impl Grupo {
    pub fn nueva(&self, disparador: &impl IsA<gtk::Widget>) -> Desplegable {
        let opciones = gtk::Box::new(gtk::Orientation::Vertical, 0);
        opciones.add_css_class("cc-lista");
        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        revealer.set_transition_duration(crate::animaciones::ms(200));
        revealer.set_child(Some(&opciones));
        let d = disparador.upcast_ref::<gtk::Widget>().downgrade();
        revealer.connect_reveal_child_notify(move |r| {
            let Some(d) = d.upgrade() else { return };
            if r.reveals_child() {
                d.add_css_class("abierto");
            } else {
                d.remove_css_class("abierto");
            }
        });
        let mut listas = self.listas.borrow_mut();
        listas.retain(|r| r.upgrade().is_some());
        listas.push(revealer.downgrade());
        Desplegable {
            revealer,
            opciones,
            grupo: self.clone(),
        }
    }

    pub fn cerrar(&self) {
        let vivas: Vec<gtk::Revealer> = self.listas.borrow().iter().filter_map(|r| r.upgrade()).collect();
        for r in vivas {
            r.set_reveal_child(false);
        }
    }
}

#[derive(Clone)]
pub struct Desplegable {
    pub revealer: gtk::Revealer,
    opciones: gtk::Box,
    grupo: Grupo,
}

impl Desplegable {
    pub fn alternar(&self) {
        let abrir = !self.revealer.reveals_child();
        self.grupo.cerrar();
        self.revealer.set_reveal_child(abrir);
    }

    pub fn llenar(&self, items: Vec<Item>) -> Vec<gtk::Button> {
        while let Some(h) = self.opciones.first_child() {
            self.opciones.remove(&h);
        }
        // Sin opciones (una sola salida, y es la puesta), no hay nada que desplegar.
        self.revealer.set_visible(!items.is_empty());
        let mut botones = Vec::new();
        for it in items {
            let b = opcion(it);
            self.opciones.append(&b);
            botones.push(b);
        }
        botones
    }
}

pub fn opcion(it: Item) -> gtk::Button {
    let fila = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let glifo = gtk::Label::new(Some(&it.glifo));
    glifo.add_css_class("glifo");
    fila.append(&glifo);
    let texto = gtk::Label::new(Some(&it.texto));
    texto.set_xalign(0.0);
    texto.set_hexpand(true);
    texto.set_ellipsize(gtk::pango::EllipsizeMode::End);
    fila.append(&texto);
    if it.marcada == Some(true) {
        let tilde = gtk::Label::new(Some("󰄬"));
        tilde.add_css_class("tilde");
        fila.append(&tilde);
    }
    let b = gtk::Button::new();
    b.set_child(Some(&fila));
    b.add_css_class("cc-opcion");
    if it.marcada == Some(true) {
        b.add_css_class("marcada");
    }
    let f = it.accion.clone();
    b.connect_clicked(move |_| f());
    b
}
