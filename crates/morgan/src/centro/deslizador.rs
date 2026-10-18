//! Lo que muestra el slider es lo que el sistema responde, no lo pedido. Mientras se
//! arrastra, lo que llega no mueve la perilla: pelearía con la mano.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;

#[derive(Clone)]
pub struct Deslizador {
    pub escala: gtk::Scale,
    /// Mientras se pone un valor desde el estado, el cambio no vuelve como pedido.
    poniendo: Rc<Cell<bool>>,
}

impl Deslizador {
    pub fn new(al_mover: impl Fn(f64) + 'static) -> Self {
        let escala = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        escala.set_draw_value(false);
        escala.set_hexpand(true);
        escala.set_round_digits(0);
        let poniendo = Rc::new(Cell::new(false));
        let p = poniendo.clone();
        escala.connect_value_changed(move |e| {
            if !p.get() {
                al_mover(e.value().round());
            }
        });
        Deslizador { escala, poniendo }
    }

    pub fn poner(&self, v: f64) {
        if self.escala.state_flags().contains(gtk::StateFlags::ACTIVE) {
            return;
        }
        self.poniendo.set(true);
        self.escala.set_value(v.clamp(0.0, 100.0));
        self.poniendo.set(false);
    }
}
