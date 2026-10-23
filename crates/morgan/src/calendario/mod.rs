//! Sin repetir lo que ya está a la vista: la fecha y la hora están en la barra, así que el
//! panel no las dice; el día elegido se ve en la grilla, así que el detalle no lo nombra.
//!
//! Sin GTK: lo dibuja el hilo de la barra (`barra/hilo.rs`, `dibujo.rs`), que también lo
//! cierra con Escape o con un click afuera. Acá quedan el clima y los eventos que le llegan, y
//! si está abierto, para el resto de morgan.

pub mod dibujo;
pub mod modelo;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::agenda::Agenda;
use crate::barra::hilo::Orden as OrdenBarra;
use dibujo::Orden;

#[derive(Clone)]
pub struct Calendario {
    inner: Rc<Inner>,
}

struct Inner {
    hilo: Sender<OrdenBarra>,
    ubicacion: RefCell<crate::config::Clima>,
    abierto: Cell<bool>,
}

impl Calendario {
    pub fn new(
        hilo: Sender<OrdenBarra>,
        agenda: &Agenda,
        ubicacion: &crate::config::Clima,
    ) -> Self {
        let c = Calendario {
            inner: Rc::new(Inner {
                hilo,
                ubicacion: RefCell::new(ubicacion.clone()),
                abierto: Cell::new(false),
            }),
        };
        c.mandar(Orden::Eventos(agenda.eventos()));
        let (yo, a) = (c.clone(), agenda.clone());
        agenda.al_cambiar(move || yo.mandar(Orden::Eventos(a.eventos())));
        // Con el clima ya en memoria, la primera apertura lo dibuja de entrada en vez de
        // cambiar de aspecto cuando llega.
        let yo = c.clone();
        glib::MainContext::default().spawn_local(async move { yo.leer_clima().await });
        c
    }

    fn mandar(&self, o: Orden) {
        // Si el hilo terminó (sin Wayland o sin GPU) ya lo dijo al terminar.
        let _ = self.inner.hilo.send(OrdenBarra::Calendario(o));
    }

    pub fn abierto(&self) -> bool {
        self.inner.abierto.get()
    }

    pub fn alternar(&self) {
        if self.abierto() {
            self.cerrar();
        } else {
            self.abrir();
        }
    }

    pub fn abrir(&self) {
        if self.inner.abierto.replace(true) {
            return;
        }
        self.mandar(Orden::Abrir);
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move { yo.leer_clima().await });
    }

    pub fn cerrar(&self) {
        if self.inner.abierto.replace(false) {
            self.mandar(Orden::Cerrar);
        }
    }

    /// Lo cerró el calendario mismo (Escape, un click afuera).
    pub fn cerrado(&self) {
        self.inner.abierto.set(false);
    }

    async fn leer_clima(&self) {
        let ubicacion = self.inner.ubicacion.borrow().clone();
        let p = crate::clima::pronostico(&ubicacion).await;
        self.mandar(Orden::Clima(p));
    }

    pub fn aplicar_config(&self, c: &crate::config::Clima) {
        self.inner.ubicacion.replace(c.clone());
    }
}
