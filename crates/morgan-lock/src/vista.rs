//! Las ventanas de todas las salidas comparten el buffer del campo y el estado.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, glib};

use crate::fondo::Fondo;
use crate::lienzo::{Lienzo, Paleta};
use morgan_ipc::bloqueo::Bloqueo;

const FALLO_MS: u64 = 2000;
const SALIDA_MS: u64 = 200;
const DESTELLO_MS: u64 = 380;

pub enum Campo {
    Normal,
    Verificando,
    Fallo(u32),
}

type Enviar = Box<dyn Fn(String)>;

#[derive(Clone)]
pub struct Vista {
    inner: Rc<Inner>,
}

struct Inner {
    buffer: gtk::PasswordEntryBuffer,
    fondo: Option<gdk::Texture>,
    paleta: Paleta,
    estilo: Bloqueo,
    /// El campo se vacía por el envío, no por un borrado: sin golpe de onda.
    enviando: Cell<bool>,
    salidas: RefCell<Vec<Salida>>,
    enviar: RefCell<Option<Enviar>>,
    mayus: Cell<bool>,
    mensaje: RefCell<Option<String>>,
    /// Sube con cada cambio del campo: un temporizador viejo no pisa uno nuevo.
    generacion: Cell<u64>,
    /// Ya empezó el fundido de salida: un segundo pedido no desbloquea dos veces.
    saliendo: Cell<bool>,
}

struct Salida {
    contenido: glib::WeakRef<gtk::Overlay>,
    lienzo: glib::WeakRef<Lienzo>,
    entrada: glib::WeakRef<gtk::Entry>,
    hora: glib::WeakRef<gtk::Label>,
    minutos: glib::WeakRef<gtk::Label>,
    fecha: glib::WeakRef<gtk::Label>,
    aviso: glib::WeakRef<gtk::Label>,
}

impl Vista {
    pub fn new(
        display: &gdk::Display,
        fondo: Option<gdk::Texture>,
        paleta: Paleta,
        estilo: Bloqueo,
    ) -> Self {
        let vista = Vista {
            inner: Rc::new(Inner {
                buffer: gtk::PasswordEntryBuffer::new(),
                fondo,
                paleta,
                estilo,
                enviando: Cell::new(false),
                salidas: RefCell::default(),
                enviar: RefCell::default(),
                mayus: Cell::new(false),
                mensaje: RefCell::default(),
                generacion: Cell::new(0),
                saliendo: Cell::new(false),
            }),
        };
        if let Some(teclado) = display.default_seat().and_then(|s| s.keyboard()) {
            vista.inner.mayus.set(teclado.is_caps_locked());
            let v = vista.clone();
            teclado.connect_caps_lock_state_notify(move |t| {
                v.inner.mayus.set(t.is_caps_locked());
                v.pintar_aviso();
                v.pintar_mayus();
            });
        }
        // La onda y las chispas siguen al buffer, que es uno para todas las salidas.
        let v = vista.clone();
        let previo = Cell::new(0usize);
        vista.inner.buffer.connect_length_notify(move |b| {
            let n = b.length() as usize;
            let antes = previo.replace(n);
            if v.inner.enviando.get() {
                v.lienzos(|l| l.vaciar());
            } else if n > antes {
                v.lienzos(|l| l.agregar(n));
            } else if n < antes {
                let todo = n == 0 && antes > 1;
                v.lienzos(|l| l.quitar(n, todo));
            }
        });
        vista
    }

    pub fn al_enviar(&self, f: impl Fn(String) + 'static) {
        *self.inner.enviar.borrow_mut() = Some(Box::new(f));
    }

    pub fn ventana(&self) -> gtk::Window {
        let overlay = gtk::Overlay::new();
        overlay.add_css_class("contenido");
        match &self.inner.fondo {
            Some(t) => overlay.set_child(Some(&Fondo::new(t.clone()))),
            None => {
                let liso = gtk::Box::new(gtk::Orientation::Vertical, 0);
                liso.add_css_class("liso");
                overlay.set_child(Some(&liso));
            }
        }

        let hora = gtk::Label::new(None);
        hora.add_css_class("hora");
        let dos = gtk::Label::new(Some(":"));
        dos.add_css_class("dospuntos");
        let minutos = gtk::Label::new(None);
        minutos.add_css_class("minutos");
        let reloj = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        reloj.add_css_class("reloj");
        reloj.append(&hora);
        reloj.append(&dos);
        reloj.append(&minutos);
        en(&overlay, &reloj, 170);

        let fecha = gtk::Label::new(None);
        fecha.add_css_class("fecha");
        en(&overlay, &fecha, 70);

        // El Entry recibe el teclado y no se ve: lo que se ve lo dibuja el lienzo encima.
        let entrada = gtk::Entry::with_buffer(&self.inner.buffer);
        entrada.add_css_class("campo");
        entrada.set_visibility(false);
        let lienzo = Lienzo::new(self.inner.paleta, self.inner.estilo);
        let campo = gtk::Overlay::new();
        campo.set_child(Some(&entrada));
        campo.add_overlay(&lienzo);
        let v = self.clone();
        entrada.connect_activate(move |e| {
            let texto = e.text().to_string();
            if texto.is_empty() || !e.is_editable() {
                return;
            }
            v.inner.enviando.set(true);
            v.inner.buffer.delete_text(0, None);
            v.inner.enviando.set(false);
            if let Some(f) = v.inner.enviar.borrow().as_ref() {
                f(texto);
            }
        });
        let v = self.clone();
        entrada.connect_changed(move |e| {
            if !e.text().is_empty() && e.has_css_class("fallo") {
                v.campo(Campo::Normal);
            }
        });
        let teclas = gtk::EventControllerKey::new();
        let v = self.clone();
        teclas.connect_key_pressed(move |_, tecla, _, _| {
            if tecla == gdk::Key::Escape {
                v.inner.buffer.delete_text(0, None);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        entrada.add_controller(teclas);
        en(&overlay, &campo, -50);

        let aviso = gtk::Label::new(None);
        aviso.add_css_class("aviso");
        en(&overlay, &aviso, -105);

        let w = gtk::Window::new();
        w.add_css_class("morgan-lock");
        w.set_child(Some(&overlay));
        w.set_cursor_from_name(Some("none"));
        GtkWindowExt::set_focus(&w, Some(&entrada));

        self.inner.salidas.borrow_mut().push(Salida {
            contenido: overlay.downgrade(),
            lienzo: lienzo.downgrade(),
            entrada: entrada.downgrade(),
            hora: hora.downgrade(),
            minutos: minutos.downgrade(),
            fecha: fecha.downgrade(),
            aviso: aviso.downgrade(),
        });
        self.refrescar_reloj();
        self.pintar_aviso();
        self.pintar_mayus();
        w
    }

    pub fn refrescar_reloj(&self) {
        let Ok(ahora) = glib::DateTime::now_local() else {
            return;
        };
        let h = ahora.format("%H").unwrap_or_default();
        let m = ahora.format("%M").unwrap_or_default();
        let f = ahora.format("%A %-d de %B").unwrap_or_default();
        self.cada(|s| {
            if let Some(l) = s.hora.upgrade() {
                l.set_label(&h);
            }
            if let Some(l) = s.minutos.upgrade() {
                l.set_label(&m);
            }
            if let Some(l) = s.fecha.upgrade() {
                l.set_label(&f);
            }
        });
    }

    pub fn campo(&self, c: Campo) {
        let gen_ = self.inner.generacion.get() + 1;
        self.inner.generacion.set(gen_);
        let (clase, editable) = match c {
            Campo::Normal => (None, true),
            Campo::Verificando => (Some("verificando"), false),
            Campo::Fallo(_) => (Some("fallo"), true),
        };
        let es_fallo = clase == Some("fallo");
        self.cada(|s| {
            if let Some(l) = s.lienzo.upgrade() {
                match c {
                    Campo::Normal => l.normal(),
                    Campo::Verificando => l.verificando(),
                    Campo::Fallo(n) => l.fallo(n),
                }
            }
            if let Some(e) = s.entrada.upgrade() {
                e.set_editable(editable);
                for c in ["verificando", "fallo"] {
                    if Some(c) == clase {
                        e.add_css_class(c);
                    } else {
                        e.remove_css_class(c);
                    }
                }
            }
        });
        if es_fallo {
            let v = self.clone();
            glib::timeout_add_local_once(Duration::from_millis(FALLO_MS), move || {
                if v.inner.generacion.get() == gen_ {
                    v.campo(Campo::Normal);
                }
            });
        }
    }

    pub fn salir(&self, f: impl FnOnce() + 'static) {
        if self.inner.saliendo.replace(true) {
            return;
        }
        self.cada(|s| {
            if let Some(e) = s.entrada.upgrade() {
                e.set_editable(false);
            }
            if let Some(l) = s.lienzo.upgrade() {
                l.acierto();
            }
        });
        let v = self.clone();
        let destello = if self.inner.estilo.destello {
            DESTELLO_MS
        } else {
            0
        };
        glib::timeout_add_local_once(Duration::from_millis(destello), move || {
            v.cada(|s| {
                if let Some(c) = s.contenido.upgrade() {
                    c.add_css_class("saliendo");
                }
            });
            glib::timeout_add_local_once(Duration::from_millis(SALIDA_MS), f);
        });
    }

    pub fn mensaje(&self, m: Option<String>) {
        *self.inner.mensaje.borrow_mut() = m;
        self.pintar_aviso();
    }

    fn pintar_aviso(&self) {
        let texto = if self.inner.mayus.get() {
            Some("󰘲  Bloq Mayús activado".to_owned())
        } else {
            self.inner.mensaje.borrow().clone()
        };
        self.cada(|s| {
            if let Some(l) = s.aviso.upgrade() {
                l.set_label(texto.as_deref().unwrap_or(""));
                l.set_visible(texto.is_some());
            }
        });
    }

    fn pintar_mayus(&self) {
        let si = self.inner.mayus.get();
        self.cada(|s| {
            if let Some(l) = s.lienzo.upgrade() {
                l.mayus(si);
            }
        });
    }

    fn lienzos(&self, f: impl Fn(&Lienzo)) {
        self.cada(|s| {
            if let Some(l) = s.lienzo.upgrade() {
                f(&l);
            }
        });
    }

    fn cada(&self, mut f: impl FnMut(&Salida)) {
        // Las ventanas de salidas que se fueron ya no están: se limpian de paso.
        self.inner
            .salidas
            .borrow_mut()
            .retain(|s| s.entrada.upgrade().is_some());
        for s in self.inner.salidas.borrow().iter() {
            f(s);
        }
    }
}

/// El margen doble de un lado deja el centro de `hijo` `y` px arriba del de la salida.
fn en(overlay: &gtk::Overlay, hijo: &impl IsA<gtk::Widget>, y: i32) {
    hijo.set_halign(gtk::Align::Center);
    hijo.set_valign(gtk::Align::Center);
    if y > 0 {
        hijo.set_margin_bottom(2 * y);
    } else {
        hijo.set_margin_top(-2 * y);
    }
    overlay.add_overlay(hijo);
}
