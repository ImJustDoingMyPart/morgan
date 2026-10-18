//! Sin flecha: en dos columnas de monoespaciada no hay lugar para ella y el título.
//!
//! El click pide; lo que se ve encendido lo pone quien sabe el estado (`poner`).

use std::cell::Cell;

use gtk::prelude::*;

use super::desplegable::{Desplegable, Grupo};
use super::resumen::Resumen;

pub struct Tarjetas {
    pub raiz: gtk::Box,
    grilla: gtk::Grid,
    cuantas: Cell<i32>,
    grupo: Grupo,
}

#[derive(Clone)]
pub struct Tarjeta {
    boton: gtk::Button,
    glifo: gtk::Label,
    estado: gtk::Label,
}

impl Tarjetas {
    pub fn new(grupo: &Grupo) -> Self {
        let raiz = gtk::Box::new(gtk::Orientation::Vertical, 0);
        raiz.add_css_class("cc-tarjetas");
        let grilla = gtk::Grid::new();
        grilla.set_column_homogeneous(true);
        grilla.set_row_spacing(8);
        grilla.set_column_spacing(8);
        raiz.append(&grilla);
        Tarjetas {
            raiz,
            grilla,
            cuantas: Cell::new(0),
            grupo: grupo.clone(),
        }
    }

    pub fn interruptor(&self, glifo: &str, titulo: &str, f: impl Fn() + 'static) -> Tarjeta {
        let t = self.tarjeta(glifo, titulo);
        t.boton.connect_clicked(move |_| f());
        t
    }

    pub fn con_lista(&self, glifo: &str, titulo: &str) -> (Tarjeta, Desplegable) {
        let t = self.tarjeta(glifo, titulo);
        let d = self.grupo.nueva(&t.boton);
        self.raiz.append(&d.revealer);
        let d2 = d.clone();
        t.boton.connect_clicked(move |_| d2.alternar());
        (t, d)
    }

    fn tarjeta(&self, glifo: &str, titulo: &str) -> Tarjeta {
        let fila = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let icono = gtk::Label::new(Some(glifo));
        icono.add_css_class("icono");
        icono.set_valign(gtk::Align::Center);
        fila.append(&icono);
        let textos = gtk::Box::new(gtk::Orientation::Vertical, 0);
        textos.set_valign(gtk::Align::Center);
        textos.set_hexpand(true);
        let t = gtk::Label::new(Some(titulo));
        t.add_css_class("titulo");
        t.set_xalign(0.0);
        t.set_ellipsize(gtk::pango::EllipsizeMode::End);
        textos.append(&t);
        let estado = gtk::Label::new(None);
        estado.add_css_class("estado");
        estado.set_xalign(0.0);
        estado.set_ellipsize(gtk::pango::EllipsizeMode::End);
        textos.append(&estado);
        fila.append(&textos);
        let boton = gtk::Button::new();
        boton.add_css_class("cc-tarjeta");
        boton.set_child(Some(&fila));
        let n = self.cuantas.get();
        self.grilla.attach(&boton, n % 2, n / 2, 1, 1);
        self.cuantas.set(n + 1);
        Tarjeta {
            boton,
            glifo: icono,
            estado,
        }
    }
}

impl Tarjeta {
    pub fn poner(&self, r: &Resumen) {
        self.glifo.set_label(r.glifo);
        self.estado.set_label(&r.estado);
        if r.encendida {
            self.boton.add_css_class("encendida");
        } else {
            self.boton.remove_css_class("encendida");
        }
    }

    pub fn alertar(&self, si: bool) {
        if si {
            self.boton.add_css_class("alerta");
        } else {
            self.boton.remove_css_class("alerta");
        }
    }
}
