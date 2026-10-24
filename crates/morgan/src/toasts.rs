//! Las notificaciones que flotan: cuáles, en qué orden y cuánto duran. Las dibuja el hilo del
//! centro (`centro/dibujo/toasts.rs`) en su propia capa, que existe mientras hay alguna; acá se
//! le manda la lista cada vez que cambia y se lleva la cuenta de cada una (el puntero encima la
//! frena).
//!
//! Con un juego en foco la columna no cambia: abrir o cerrar una capa sobre el juego le suelta
//! el puntero. Lo que ya flotaba queda quieto, sin cuenta, y lo que había que sacar se saca al
//! salir del juego.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use gtk::glib;
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::centro::hilo::Orden;
use crate::centro::lista::{self, Lista};
use crate::hyprland::Hyprland;
use crate::notificaciones::modelo;
use crate::notificaciones::{Cambio, Notificaciones};

#[derive(Clone)]
pub struct Toasts {
    inner: Rc<Inner>,
}

struct Inner {
    hilo: Sender<Orden>,
    lista: Rc<Lista>,
    notificaciones: Notificaciones,
    /// Las que flotan, la más nueva primero, con su cuenta.
    vivas: RefCell<Vec<(u32, Option<glib::SourceId>)>>,
    /// La que tiene el puntero encima: no cuenta.
    encima: Cell<Option<u32>>,
    callados: Cell<bool>,
    hyprland: Hyprland,
    diferidas: RefCell<Vec<u32>>,
}

impl Toasts {
    pub fn new(
        hilo: Sender<Orden>,
        notificaciones: &Notificaciones,
        hyprland: &Hyprland,
        color: [f32; 4],
    ) -> Self {
        let inner = Rc::new(Inner {
            hilo,
            lista: Lista::new(notificaciones, color, lista::TOAST),
            notificaciones: notificaciones.clone(),
            vivas: RefCell::default(),
            encima: Cell::new(None),
            callados: Cell::new(false),
            hyprland: hyprland.clone(),
            diferidas: RefCell::default(),
        });
        // Una vista previa llega después de decodificarse.
        let weak = Rc::downgrade(&inner);
        inner.lista.al_llegar(move || {
            if let Some(inner) = weak.upgrade() {
                inner.mandar();
            }
        });
        let weak = Rc::downgrade(&inner);
        notificaciones.al_cambiar(move |c| {
            if let Some(inner) = weak.upgrade() {
                inner.cambio(c);
            }
        });
        let weak = Rc::downgrade(&inner);
        hyprland.al_cambiar(move |juego| {
            if let Some(inner) = weak.upgrade() {
                inner.juego(juego);
            }
        });
        Toasts { inner }
    }

    pub fn callar(&self, v: bool) {
        self.inner.callados.set(v);
        if v {
            for id in self.inner.ids() {
                self.inner.dejar(id);
            }
        }
    }

    /// Otro tema: los íconos simbólicos cambian de color.
    pub fn poner_color(&self, c: [f32; 4]) {
        self.inner.lista.poner_color(c);
        self.inner.mandar();
    }

    /// Se tocó: el toast se va (la acción ya la hizo quien llama).
    pub fn quitar(&self, id: u32) {
        self.inner.quitar(id);
    }

    /// El puntero pasó a estar sobre `id` (o sobre ninguno): esa se frena y la de antes vuelve
    /// a contar.
    pub fn encima(&self, id: Option<u32>) {
        let i = &self.inner;
        let antes = i.encima.replace(id);
        if antes == id {
            return;
        }
        if let Some(id) = id {
            i.parar(id);
        }
        if let Some(a) = antes {
            i.contar(a);
        }
    }
}

impl Inner {
    fn ids(&self) -> Vec<u32> {
        self.vivas.borrow().iter().map(|(id, _)| *id).collect()
    }

    fn mandar(&self) {
        let ids = self.ids();
        let avisos = if ids.is_empty() {
            Vec::new()
        } else {
            self.lista.sueltas(&ids)
        };
        // Si el hilo terminó (sin Wayland o sin GPU) ya lo dijo al terminar.
        let _ = self.hilo.send(Orden::Toasts(avisos));
    }

    fn juego(self: &Rc<Self>, juego: bool) {
        if juego {
            for id in self.ids() {
                self.parar(id);
            }
            return;
        }
        let diferidas = std::mem::take(&mut *self.diferidas.borrow_mut());
        for id in diferidas {
            self.quitar(id);
        }
        for id in self.ids() {
            self.contar(id);
        }
    }

    fn cambio(self: &Rc<Self>, c: Cambio) {
        match c {
            Cambio::Llego { id, flota } => {
                // Un reemplazo saca la vieja aunque la nueva no flote.
                self.quitar(id);
                if flota && !self.callados.get() {
                    self.mostrar(id);
                } else if flota {
                    // No flotó: para una transitoria, eso es vencer.
                    self.notificaciones.vencio(id);
                }
            }
            Cambio::Cerrada(id) => self.quitar(id),
            Cambio::NoMolestar(true) => {
                // Prender No molestar se lleva lo que flotaba, salvo las críticas.
                for id in self.ids() {
                    let critica = self
                        .notificaciones
                        .get(id)
                        .is_some_and(|n| n.urgencia == modelo::Urgencia::Critica);
                    if !critica {
                        self.dejar(id);
                    }
                }
            }
            Cambio::NoMolestar(false) => {}
        }
    }

    fn mostrar(self: &Rc<Self>, id: u32) {
        if self.notificaciones.get(id).is_none() {
            return;
        }
        self.vivas.borrow_mut().insert(0, (id, None));
        self.contar(id);
        self.mandar();
    }

    fn parar(&self, id: u32) {
        if let Some((_, c)) = self.vivas.borrow_mut().iter_mut().find(|(i, _)| *i == id)
            && let Some(c) = c.take()
        {
            c.remove();
        }
    }

    fn contar(self: &Rc<Self>, id: u32) {
        if self.hyprland.juego_en_foco() || self.encima.get() == Some(id) {
            return;
        }
        let Some(dur) = self
            .notificaciones
            .get(id)
            .as_ref()
            .and_then(modelo::duracion)
        else {
            return;
        };
        let weak: Weak<Inner> = Rc::downgrade(self);
        let fuente = glib::timeout_add_local_once(dur, move || {
            if let Some(inner) = weak.upgrade() {
                if let Some((_, c)) = inner.vivas.borrow_mut().iter_mut().find(|(i, _)| *i == id) {
                    *c = None;
                }
                inner.dejar(id);
            }
        });
        let mut vivas = self.vivas.borrow_mut();
        match vivas.iter_mut().find(|(i, _)| *i == id) {
            Some((_, c)) => {
                if let Some(vieja) = c.replace(fuente) {
                    vieja.remove();
                }
            }
            None => fuente.remove(),
        }
    }

    fn dejar(&self, id: u32) {
        self.quitar(id);
        self.notificaciones.vencio(id);
    }

    fn quitar(&self, id: u32) {
        if self.hyprland.juego_en_foco() && self.ids().contains(&id) {
            self.diferidas.borrow_mut().push(id);
            return;
        }
        let quitada = {
            let mut vivas = self.vivas.borrow_mut();
            vivas
                .iter()
                .position(|(i, _)| *i == id)
                .map(|p| vivas.remove(p))
        };
        let Some((_, cuenta)) = quitada else {
            return;
        };
        if let Some(c) = cuenta {
            c.remove();
        }
        if self.encima.get() == Some(id) {
            self.encima.set(None);
        }
        self.mandar();
    }
}
