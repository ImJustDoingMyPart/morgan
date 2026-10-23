//! El porcentaje, el nombre y el silencio son lo que dice el sistema (`poner`).

use gtk::prelude::*;

use super::deslizador::Deslizador;
use super::desplegable::{Desplegable, Grupo};

#[derive(Clone)]
pub struct Nivel {
    pub raiz: gtk::Box,
    pub deslizador: Deslizador,
    glifo: gtk::Button,
    nombre_boton: gtk::Button,
    nombre: gtk::Label,
    porcentaje: gtk::Label,
}

impl Nivel {
    pub fn new(
        glifo: &str,
        al_mover: impl Fn(f64) + 'static,
        al_glifo: Option<Box<dyn Fn()>>,
    ) -> Self {
        let raiz = gtk::Box::new(gtk::Orientation::Vertical, 0);
        raiz.add_css_class("cc-nivel");
        let fila = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        raiz.append(&fila);

        let boton = gtk::Button::with_label(glifo);
        boton.add_css_class("glifo");
        boton.set_valign(gtk::Align::Center);
        match al_glifo {
            Some(f) => {
                boton.connect_clicked(move |_| f());
            }
            None => {
                boton.set_can_target(false);
                boton.set_focusable(false);
            }
        }
        fila.append(&boton);

        let columna = gtk::Box::new(gtk::Orientation::Vertical, 2);
        columna.set_hexpand(true);
        columna.set_valign(gtk::Align::Center);
        let rotulo = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        rotulo.add_css_class("rotulo");
        let nombre = gtk::Label::new(None);
        nombre.set_xalign(0.0);
        nombre.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let nombre_boton = gtk::Button::new();
        nombre_boton.add_css_class("nombre");
        nombre_boton.set_child(Some(&nombre));
        nombre_boton.set_halign(gtk::Align::Start);
        nombre_boton.set_can_target(false);
        nombre_boton.set_focusable(false);
        // El resto del renglón empuja el porcentaje a la derecha.
        let hueco = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        hueco.set_hexpand(true);
        rotulo.append(&nombre_boton);
        rotulo.append(&hueco);
        let porcentaje = gtk::Label::new(None);
        porcentaje.add_css_class("porcentaje");
        rotulo.append(&porcentaje);
        columna.append(&rotulo);
        let deslizador = Deslizador::new(al_mover);
        columna.append(&deslizador.escala);
        // El número acompaña a la perilla: la del sistema, o la mano mientras arrastra.
        let (p, r) = (porcentaje.clone(), raiz.clone());
        deslizador.escala.connect_value_changed(move |e| {
            if !r.has_css_class("mudo") {
                p.set_label(&format!("{:.0}%", e.value()));
            }
        });
        fila.append(&columna);

        Nivel {
            raiz,
            deslizador,
            glifo: boton,
            nombre_boton,
            nombre,
            porcentaje,
        }
    }

    pub fn con_lista(&self, grupo: &Grupo) -> Desplegable {
        let b = &self.nombre_boton;
        b.set_can_target(true);
        b.set_focusable(true);
        b.add_css_class("con-lista");
        let d = grupo.nueva(b);
        self.raiz.append(&d.revealer);
        let d2 = d.clone();
        b.connect_clicked(move |_| d2.alternar());
        d
    }

    pub fn poner(&self, glifo: &str, nombre: &str, valor: f64, mudo: bool) {
        self.glifo.set_label(glifo);
        if let Some(l) = self.glifo.child().and_downcast::<gtk::Label>() {
            crate::icono::centrar_tinta(&l);
        }
        self.nombre.set_label(nombre);
        if mudo {
            self.raiz.add_css_class("mudo");
            self.porcentaje.set_label("mudo");
        } else {
            self.raiz.remove_css_class("mudo");
        }
        self.poner_valor(valor);
    }

    pub fn poner_valor(&self, valor: f64) {
        self.deslizador.poner(valor);
        if !self.raiz.has_css_class("mudo") {
            let v = self.deslizador.escala.value();
            self.porcentaje.set_label(&format!("{v:.0}%"));
        }
    }

    pub fn set_nombre(&self, nombre: &str) {
        self.nombre.set_label(nombre);
    }
}
