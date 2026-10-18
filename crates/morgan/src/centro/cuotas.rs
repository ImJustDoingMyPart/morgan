//! Qué se muestra y por qué está en `crate::cuotas`; esto solo pinta.

use gtk::prelude::*;

use super::desplegable::{Desplegable, Grupo};
use crate::cuotas::{self, Agente, Pintura};

#[derive(Clone)]
struct Fila {
    raiz: gtk::Box,
    glifo: gtk::Button,
    nombre_boton: gtk::Button,
    nombre: gtk::Label,
    renueva: gtk::Label,
    porcentaje: gtk::Label,
    barra: gtk::ProgressBar,
}

impl Fila {
    fn new() -> Self {
        let raiz = gtk::Box::new(gtk::Orientation::Vertical, 0);
        raiz.add_css_class("cc-nivel");
        raiz.add_css_class("cc-cuota");
        let fila = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        raiz.append(&fila);

        // Un botón que no se toca, como el glifo del brillo: así toma su estilo.
        let glifo = gtk::Button::new();
        glifo.add_css_class("glifo");
        glifo.set_valign(gtk::Align::Center);
        glifo.set_can_target(false);
        glifo.set_focusable(false);
        fila.append(&glifo);

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
        // El nombre se achica antes que la renovación y el número.
        nombre_boton.set_hexpand(true);
        rotulo.append(&nombre_boton);
        let renueva = gtk::Label::new(None);
        renueva.add_css_class("renueva");
        rotulo.append(&renueva);
        let porcentaje = gtk::Label::new(None);
        porcentaje.add_css_class("porcentaje");
        rotulo.append(&porcentaje);
        columna.append(&rotulo);
        let barra = gtk::ProgressBar::new();
        columna.append(&barra);
        fila.append(&columna);

        Fila {
            raiz,
            glifo,
            nombre_boton,
            nombre,
            renueva,
            porcentaje,
            barra,
        }
    }

    fn poner(&self, p: &Pintura) {
        self.glifo.set_label(p.glifo);
        if let Some(l) = self.glifo.child().and_downcast::<gtk::Label>() {
            crate::osd::centrar_tinta(&l);
        }
        self.nombre.set_label(&p.nombre);
        self.renueva.set_label(&p.renueva);
        self.renueva.set_visible(!p.renueva.is_empty());
        self.porcentaje.set_label(&p.porcentaje);
        self.barra.set_fraction(p.fraccion);
        self.barra.set_sensitive(!p.apagada);
        if p.alerta {
            self.raiz.add_css_class("mudo");
        } else {
            self.raiz.remove_css_class("mudo");
        }
    }
}

struct Bloque {
    agente: Agente,
    fila: Fila,
    otras: gtk::Box,
    lista: Desplegable,
}

pub struct Vista {
    pub raiz: gtk::Box,
    bloques: Vec<Bloque>,
    /// Abrir una lista cierra las otras, sin cerrar la de la tarjeta (que es del grupo del
    /// panel).
    grupo: Grupo,
}

impl Vista {
    pub fn new() -> Self {
        let raiz = gtk::Box::new(gtk::Orientation::Vertical, 0);
        raiz.add_css_class("cc-niveles");
        raiz.add_css_class("cc-cuotas");
        let grupo = Grupo::default();
        let bloques = cuotas::AGENTES
            .into_iter()
            .map(|agente| {
                let fila = Fila::new();
                raiz.append(&fila.raiz);
                let lista = grupo.nueva(&fila.nombre_boton);
                // En el recuadro de las listas del panel: lo desplegado se ve como tal.
                let otras = gtk::Box::new(gtk::Orientation::Vertical, 0);
                otras.add_css_class("cc-lista");
                lista.revealer.set_child(Some(&otras));
                raiz.append(&lista.revealer);
                let l = lista.clone();
                fila.nombre_boton.connect_clicked(move |_| l.alternar());
                Bloque {
                    agente,
                    fila,
                    otras,
                    lista,
                }
            })
            .collect();
        Vista {
            raiz,
            bloques,
            grupo,
        }
    }

    pub fn refrescar(&self) -> (String, bool) {
        let ahora = cuotas::ahora();
        let lecturas: Vec<_> = self
            .bloques
            .iter()
            .map(|b| (b.agente, cuotas::leer(b.agente)))
            .collect();
        for (b, (agente, l)) in self.bloques.iter().zip(&lecturas) {
            while let Some(h) = b.otras.first_child() {
                b.otras.remove(&h);
            }
            let Some(i) = cuotas::principal(l) else {
                b.fila.poner(&cuotas::sin_datos(*agente));
                desplegable(b, false);
                continue;
            };
            b.fila.poner(&cuotas::pintar(
                &l.ventanas[i],
                Some((*agente, l.observada)),
                ahora,
            ));
            for (j, v) in l.ventanas.iter().enumerate() {
                if j != i {
                    let f = Fila::new();
                    f.poner(&cuotas::pintar(v, None, ahora));
                    b.otras.append(&f.raiz);
                }
            }
            desplegable(b, l.ventanas.len() > 1);
        }
        cuotas::resumen(&lecturas, ahora)
    }

    pub fn cerrar(&self) {
        self.grupo.cerrar();
    }
}

fn desplegable(b: &Bloque, hay: bool) {
    let n = &b.fila.nombre_boton;
    n.set_can_target(hay);
    n.set_focusable(hay);
    if hay {
        n.add_css_class("con-lista");
    } else {
        n.remove_css_class("con-lista");
        b.lista.revealer.set_reveal_child(false);
    }
}
