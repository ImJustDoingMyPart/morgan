//! La caja que se abre debajo de un renglón de las listas de red y bluetooth: la del
//! launcher, del ancho del renglón y con sus columnas (glifo, texto en la de los nombres,
//! un botón en la de los tildes), y un aviso de 12 px debajo. Sirve para escribir una clave,
//! para mostrar un código y para avisar cómo va algo.

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

const OJO: &str = "\u{F0208}";
const OJO_TACHADO: &str = "\u{F0209}";
const TILDE: &str = "󰄬";

#[derive(Clone)]
pub struct Campo {
    pub raiz: gtk::Box,
    entrada: gtk::Box,
    glifo: gtk::Label,
    pub texto: gtk::Text,
    boton: gtk::Button,
    aviso: gtk::Label,
}

impl Campo {
    pub fn new(glifo: &str) -> Self {
        let raiz = gtk::Box::new(gtk::Orientation::Vertical, 0);
        raiz.add_css_class("cc-clave");
        let entrada = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        entrada.add_css_class("launcher-entrada");
        let g = gtk::Label::new(Some(glifo));
        g.add_css_class("glifo");
        entrada.append(&g);
        let texto = gtk::Text::new();
        texto.set_hexpand(true);
        entrada.append(&texto);
        let boton = gtk::Button::new();
        boton.add_css_class("glifo");
        boton.set_visible(false);
        entrada.append(&boton);
        raiz.append(&entrada);
        let aviso = gtk::Label::new(None);
        aviso.add_css_class("aviso");
        aviso.set_xalign(0.0);
        aviso.set_wrap(true);
        aviso.set_visible(false);
        raiz.append(&aviso);
        Campo {
            raiz,
            entrada,
            glifo: g,
            texto,
            boton,
            aviso,
        }
    }

    /// Para una clave: tapada con `•` (el `●` de GTK ocupa la celda entera de la
    /// monoespaciada y se pegan) y un ojo para verla.
    pub fn clave(placeholder: &str) -> Self {
        let c = Campo::new("\u{F033E}");
        c.texto.set_placeholder_text(Some(placeholder));
        c.texto.set_visibility(false);
        c.texto.set_invisible_char('•');
        c.boton.set_label(OJO);
        c.boton.set_focus_on_click(false);
        c.boton.set_visible(true);
        let t = c.texto.clone();
        c.boton.connect_clicked(move |b| {
            let ver = !t.is_visible();
            t.set_visibility(ver);
            b.set_label(if ver { OJO_TACHADO } else { OJO });
        });
        c
    }

    /// Enter en el texto.
    pub fn al_enviar(&self, f: impl Fn() + 'static) {
        self.texto.connect_activate(move |_| f());
    }

    /// Escape con el foco en cualquier parte de la caja; no llega al panel, que se cerraría.
    pub fn al_escape(&self, f: impl Fn() + 'static) {
        let teclas = gtk::EventControllerKey::new();
        teclas.connect_key_pressed(move |_, tecla, _, _| {
            if tecla != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            f();
            glib::Propagation::Stop
        });
        self.raiz.add_controller(teclas);
    }

    pub fn enfocar(&self) {
        if self.boton.is_visible() && !self.texto.is_editable() {
            self.boton.grab_focus();
        } else {
            self.texto.grab_focus();
        }
    }

    pub fn avisar(&self, texto: &str, error: bool) {
        self.aviso.set_label(texto);
        self.aviso.set_visible(true);
        if error {
            self.aviso.add_css_class("error");
        } else {
            self.aviso.remove_css_class("error");
        }
    }

    /// Solo el aviso, sin caja: algo que pasa solo («Emparejando…») o cómo terminó.
    pub fn solo_aviso(&self, texto: &str, error: bool) {
        self.entrada.set_visible(false);
        self.avisar(texto, error);
    }

    /// Un código para leer; con `confirmar`, el tilde lo acepta (y tiene el foco: Enter).
    pub fn codigo(&self, codigo: &str, confirmar: Option<Rc<dyn Fn()>>) {
        self.entrada.set_visible(true);
        self.glifo.set_label("\u{F0306}");
        self.texto.set_visibility(true);
        self.texto.set_editable(false);
        self.texto.set_can_focus(false);
        self.texto.buffer().set_text(codigo);
        match confirmar {
            Some(f) => {
                self.boton.set_label(TILDE);
                self.boton.set_visible(true);
                self.boton.connect_clicked(move |_| f());
            }
            None => self.boton.set_visible(false),
        }
    }

    /// Para escribir un código: a la vista, sin ojo.
    pub fn pedir(&self, placeholder: &str) {
        self.entrada.set_visible(true);
        self.glifo.set_label("\u{F0306}");
        self.texto.set_visibility(true);
        self.texto.set_editable(true);
        self.texto.set_can_focus(true);
        self.texto.buffer().set_text("");
        self.texto.set_placeholder_text(Some(placeholder));
        self.boton.set_visible(false);
    }

    /// Mientras algo está en curso, no se escribe.
    pub fn ocupar(&self, si: bool) {
        self.texto.set_sensitive(!si);
        self.boton.set_sensitive(!si);
    }
}
