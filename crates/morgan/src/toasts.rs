//! Una sola ventana, anclada arriba, abajo y a la derecha: el alto lo pone la salida, así que
//! lo que no entra se scrollea adentro sin medir nada. Solo las tarjetas reciben el mouse: la
//! región de input se rehace en cada cuadro pintado (sin cuadros, sin trabajo).
//!
//! Con un juego en foco la ventana no cambia: abrir o cerrar una capa sobre el juego le suelta
//! el puntero. Lo que ya flotaba queda quieto, sin cuenta, y lo que había que sacar se saca al
//! salir del juego.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::bajada::Bajada;
use crate::columna::Columna;
use crate::hyprland::Hyprland;
use crate::notificaciones::modelo;
use crate::notificaciones::{Cambio, Notificaciones};
use crate::tarjeta::Tarjeta;

const ANCHO: i32 = 304;

#[derive(Clone)]
pub struct Toasts {
    inner: Rc<Inner>,
}

struct Inner {
    window: gtk::ApplicationWindow,
    bajada: Bajada,
    lista: Columna,
    notificaciones: Notificaciones,
    vivas: RefCell<HashMap<u32, Viva>>,
    callados: Cell<bool>,
    hyprland: Hyprland,
    diferidas: RefCell<Vec<u32>>,
}

struct Viva {
    tarjeta: Tarjeta,
    cuenta: Option<glib::SourceId>,
}

impl Toasts {
    pub fn new(
        app: &gtk::Application,
        notificaciones: &Notificaciones,
        hyprland: &Hyprland,
    ) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.add_css_class("morgan");
        window.add_css_class("floating-notifications");
        window.init_layer_shell();
        window.set_namespace(Some("morgan-notifications"));
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::None);
        for borde in [Edge::Top, Edge::Bottom, Edge::Right] {
            window.set_anchor(borde, true);
        }
        window.set_default_size(ANCHO, -1);

        let lista = Columna::new();
        lista.set_valign(gtk::Align::Start);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_propagate_natural_height(true);
        scroll.set_valign(gtk::Align::Start);
        scroll.set_has_frame(false);
        scroll.set_child(Some(&lista));
        let bajada = Bajada::new(&scroll);
        window.set_child(Some(&bajada));

        let inner = Rc::new(Inner {
            window: window.clone(),
            bajada,
            lista,
            notificaciones: notificaciones.clone(),
            vivas: RefCell::default(),
            callados: Cell::new(false),
            hyprland: hyprland.clone(),
            diferidas: RefCell::default(),
        });

        // La región de input sigue a las tarjetas: se rehace después de cada cuadro.
        window.connect_realize(|w| {
            let w = w.clone();
            if let Some(reloj) = w.frame_clock() {
                reloj.connect_after_paint(move |_| region(&w));
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
            let ids: Vec<u32> = self.inner.vivas.borrow().keys().copied().collect();
            for id in ids {
                self.inner.dejar(id);
            }
        }
    }
}

impl Inner {
    fn juego(self: &Rc<Self>, juego: bool) {
        if juego {
            for v in self.vivas.borrow_mut().values_mut() {
                if let Some(c) = v.cuenta.take() {
                    c.remove();
                }
            }
            return;
        }
        let diferidas = std::mem::take(&mut *self.diferidas.borrow_mut());
        for id in diferidas {
            self.quitar(id);
        }
        let ids: Vec<u32> = self.vivas.borrow().keys().copied().collect();
        for id in ids {
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
                let ids: Vec<u32> = self.vivas.borrow().keys().copied().collect();
                for id in ids {
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
        let Some(n) = self.notificaciones.get(id) else {
            return;
        };
        let weak = Rc::downgrade(self);
        let al_tocar: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(inner) = weak.upgrade() {
                inner.quitar(id);
            }
        });
        let t = Tarjeta::new(&n, &self.notificaciones, al_tocar);

        let hover = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(self);
        hover.connect_enter(move |_, _, _| {
            if let Some(inner) = weak.upgrade()
                && let Some(v) = inner.vivas.borrow_mut().get_mut(&id)
                && let Some(c) = v.cuenta.take()
            {
                c.remove();
            }
        });
        let weak = Rc::downgrade(self);
        hover.connect_leave(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.contar(id);
            }
        });
        t.fondo.add_controller(hover);

        self.lista.prepend(&t.raiz);
        self.vivas.borrow_mut().insert(
            id,
            Viva {
                tarjeta: t,
                cuenta: None,
            },
        );
        self.contar(id);
        if !self.window.is_visible() {
            self.bajada.arrancar();
        }
        self.window.set_visible(true);
    }

    fn contar(self: &Rc<Self>, id: u32) {
        if self.hyprland.juego_en_foco() {
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
                if let Some(v) = inner.vivas.borrow_mut().get_mut(&id) {
                    v.cuenta = None;
                }
                inner.dejar(id);
            }
        });
        if let Some(v) = self.vivas.borrow_mut().get_mut(&id)
            && let Some(vieja) = v.cuenta.replace(fuente)
        {
            vieja.remove();
        }
    }

    fn dejar(self: &Rc<Self>, id: u32) {
        self.quitar(id);
        self.notificaciones.vencio(id);
    }

    fn quitar(&self, id: u32) {
        if self.hyprland.juego_en_foco() && self.vivas.borrow().contains_key(&id) {
            self.diferidas.borrow_mut().push(id);
            return;
        }
        let Some(v) = self.vivas.borrow_mut().remove(&id) else {
            return;
        };
        if let Some(c) = v.cuenta {
            c.remove();
        }
        self.lista.remove(&v.tarjeta.raiz);
        if self.vivas.borrow().is_empty() {
            self.window.set_visible(false);
        }
    }
}

fn region(w: &gtk::ApplicationWindow) {
    let Some(surface) = w.surface() else {
        return;
    };
    let region = gtk::cairo::Region::create();
    let mut hijo = w
        .child()
        .and_then(|b| b.first_child())
        .and_then(|s| s.downcast::<gtk::ScrolledWindow>().ok())
        .and_then(|s| s.child())
        .and_then(|v| v.first_child())
        .and_then(|l| l.first_child());
    while let Some(fila) = hijo {
        if let Some(b) = fila.compute_bounds(w) {
            let _ = region.union_rectangle(&gtk::cairo::RectangleInt::new(
                b.x() as i32,
                b.y() as i32,
                b.width().ceil() as i32,
                b.height().ceil() as i32,
            ));
        }
        hijo = fila.next_sibling();
    }
    surface.set_input_region(Some(&region));
}
