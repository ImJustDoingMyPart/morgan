//! Solo anda mientras se ve: GTK no llama a un tick callback sin dibujar.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

const PAUSA: f64 = 2.0;
const VELOCIDAD: f64 = 30.0;

pub fn posicion(t: f64, recorrido: f64) -> f64 {
    if recorrido <= 0.0 {
        return 0.0;
    }
    let ida = recorrido / VELOCIDAD;
    let ciclo = 2.0 * (PAUSA + ida);
    let t = t.rem_euclid(ciclo);
    if t < PAUSA {
        0.0
    } else if t < PAUSA + ida {
        (t - PAUSA) * VELOCIDAD
    } else if t < 2.0 * PAUSA + ida {
        recorrido
    } else {
        recorrido - (t - 2.0 * PAUSA - ida) * VELOCIDAD
    }
}

#[derive(Clone)]
pub struct Marquesina {
    pub raiz: gtk::ScrolledWindow,
    pub etiqueta: gtk::Label,
    inicio: Rc<Cell<Option<i64>>>,
    tick: Rc<RefCell<Option<gtk::TickCallbackId>>>,
}

impl Marquesina {
    pub fn new() -> Self {
        let etiqueta = gtk::Label::new(None);
        etiqueta.set_xalign(0.0);
        etiqueta.set_single_line_mode(true);
        let raiz = gtk::ScrolledWindow::new();
        // Se scrollea por código; sin barra, sin rueda.
        raiz.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
        raiz.set_propagate_natural_height(true);
        raiz.set_hexpand(true);
        raiz.set_can_target(false);
        raiz.set_child(Some(&etiqueta));
        let m = Marquesina {
            raiz,
            etiqueta,
            inicio: Rc::default(),
            tick: Rc::default(),
        };
        // Al volver a mostrarse, de nuevo desde el principio.
        let yo = m.clone();
        m.raiz.connect_map(move |_| yo.arrancar());
        m
    }

    pub fn set_text(&self, texto: &str) {
        if self.etiqueta.text() != texto {
            self.etiqueta.set_text(texto);
            self.arrancar();
        }
    }

    fn arrancar(&self) {
        self.inicio.set(None);
        self.raiz.hadjustment().set_value(0.0);
        if self.tick.borrow().is_some() {
            return;
        }
        let (inicio, tick) = (self.inicio.clone(), self.tick.clone());
        let id = self.raiz.add_tick_callback(move |sw, reloj| {
            let ajuste = sw.hadjustment();
            let recorrido = ajuste.upper() - ajuste.page_size();
            // Ya medido y entra: no hay nada que mover.
            if ajuste.page_size() > 0.0 && recorrido <= 0.0 {
                tick.take();
                return gtk::glib::ControlFlow::Break;
            }
            let ahora = reloj.frame_time();
            let t0 = match inicio.get() {
                Some(t0) => t0,
                None => {
                    inicio.set(Some(ahora));
                    ahora
                }
            };
            ajuste.set_value(posicion((ahora - t0) as f64 / 1e6, recorrido));
            gtk::glib::ControlFlow::Continue
        });
        self.tick.replace(Some(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quieta_si_entra() {
        assert_eq!(posicion(5.0, 0.0), 0.0);
        assert_eq!(posicion(5.0, -10.0), 0.0);
    }

    #[test]
    fn pausa_ida_pausa_vuelta() {
        // 60 px a 30 px/s: 2 s de ida. Ciclo: 2 + 2 + 2 + 2 = 8 s.
        assert_eq!(posicion(1.0, 60.0), 0.0);
        assert_eq!(posicion(3.0, 60.0), 30.0);
        assert_eq!(posicion(5.0, 60.0), 60.0);
        assert_eq!(posicion(7.0, 60.0), 30.0);
        assert_eq!(posicion(8.5, 60.0), 0.0);
    }
}
