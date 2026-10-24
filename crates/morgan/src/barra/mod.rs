//! La barra, sin GTK: la dibuja su propio hilo con el motor de `morgan-motor` (`hilo.rs`,
//! `dibujo.rs`), junto con el calendario, que se abre desde la fecha. Acá, en el hilo de GTK,
//! queda lo que la alimenta (workspaces, reloj, avisos, bandeja) y lo que hace cada click.
//!
//! Los pedidos que también tienen `morganctl` (centro, No molestar) pasan por el mismo
//! `dispatch` que el socket, así que cierran las otras superficies y muestran el OSD igual.
//!
//! Cero sondeo: el contador de la grabación corre solo mientras se graba; el reloj se
//! despierta al cambiar el minuto y al volver de una suspensión (el temporizador de glib no
//! cuenta el tiempo dormido).

mod bandeja;
mod dibujo;
pub mod hilo;
mod modelo;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use morgan_ipc::Request;
use morgan_motor::color::Paleta;
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::bandeja::Bandeja;
use crate::calendario::Calendario;
use crate::hyprland::Hyprland;
use crate::notificaciones::Notificaciones;
use crate::privacidad::{self, Privacidad};
use crate::sistema;
use crate::tema::Tema;
use hilo::{Evento, Orden};

pub struct Servicios {
    pub hyprland: Hyprland,
    pub notificaciones: Notificaciones,
    pub bandeja: Bandeja,
    pub privacidad: Privacidad,
    pub iconos_bandeja: BTreeMap<String, String>,
    pub tele: String,
    pub tema: Tema,
    pub calendario: Calendario,
    pub centro: crate::centro::Centro,
    pub pedir: Rc<dyn Fn(Request)>,
}

/// Lanza el hilo de la barra y el calendario, que sigue los colores de `tema`.
pub fn lanzar(
    tema: &Tema,
    fijos: &BTreeMap<String, Vec<NonZeroU32>>,
) -> (Sender<Orden>, async_channel::Receiver<Evento>) {
    let (css, opacidad, borde) = tema.actual();
    let fijos = fijos
        .iter()
        .map(|(m, v)| (m.clone(), v.iter().map(|i| i64::from(i.get())).collect()))
        .collect();
    let (tx, rx) = hilo::lanzar(Paleta::desde_css(&css), opacidad as f32, borde, fijos);
    let t = tx.clone();
    tema.al_cambiar(move |css, opacidad, borde| {
        let _ = t.send(Orden::Tema(Paleta::desde_css(css), opacidad as f32, borde));
    });
    (tx, rx)
}

#[derive(Clone)]
pub struct Barra {
    inner: Rc<Inner>,
}

struct Inner {
    hilo: Sender<Orden>,
    s: Servicios,
    cajon: Rc<bandeja::Contexto>,
    reloj: RefCell<Option<glib::SourceId>>,
    grabacion: RefCell<Option<(std::time::Instant, glib::SourceId)>>,
    despertar: RefCell<Option<glib::JoinHandle<()>>>,
    escuchar: RefCell<Option<glib::JoinHandle<()>>>,
}

impl Barra {
    pub fn new(
        hilo: Sender<Orden>,
        eventos: async_channel::Receiver<Evento>,
        s: Servicios,
    ) -> Self {
        let cajon = bandeja::Contexto::new(
            &s.centro,
            s.bandeja.clone(),
            s.iconos_bandeja.clone(),
            hilo.clone(),
            &s.tema,
        );
        let inner = Rc::new(Inner {
            hilo,
            cajon,
            s,
            reloj: RefCell::default(),
            grabacion: RefCell::default(),
            despertar: RefCell::default(),
            escuchar: RefCell::default(),
        });

        let weak = Rc::downgrade(&inner);
        inner.s.hyprland.al_cambiar_espacios(move |e| {
            if let Some(inner) = weak.upgrade() {
                let _ = inner.hilo.send(Orden::Espacios(e.clone()));
                inner.privacidad();
            }
        });
        let weak = Rc::downgrade(&inner);
        inner.s.bandeja.al_cambiar(move || {
            if let Some(inner) = weak.upgrade() {
                inner.cajon.enviar();
            }
        });
        let weak = Rc::downgrade(&inner);
        inner.s.privacidad.al_cambiar(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.privacidad();
                inner.grabacion();
            }
        });
        let weak = Rc::downgrade(&inner);
        inner.s.notificaciones.al_cambiar(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.campana();
            }
        });
        let _ = inner
            .hilo
            .send(Orden::Espacios(inner.s.hyprland.espacios()));
        inner.campana();
        inner.privacidad();
        inner.grabacion();
        inner.cajon.enviar();

        // Relee al empezar (eso arranca el reloj) y cada vez que la máquina se despierta.
        let weak = Rc::downgrade(&inner);
        let despertar = sistema::vigilar(
            dormir(),
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.reloj();
                }
                Box::pin(async {})
            }),
        );
        inner.despertar.replace(Some(despertar));

        let weak = Rc::downgrade(&inner);
        let escuchar = glib::MainContext::default().spawn_local(async move {
            while let Ok(e) = eventos.recv().await {
                let Some(inner) = weak.upgrade() else { return };
                inner.evento(e);
            }
        });
        inner.escuchar.replace(Some(escuchar));
        Barra { inner }
    }

    pub fn marcar_centro(&self, abierto: bool) {
        let _ = self.inner.hilo.send(Orden::Centro(abierto));
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        for h in [self.despertar.take(), self.escuchar.take()]
            .into_iter()
            .flatten()
        {
            h.abort();
        }
    }
}

impl Inner {
    fn evento(&self, e: Evento) {
        match e {
            Evento::Pedir(r) => (self.s.pedir)(r),
            Evento::IrA(id) => self.s.hyprland.ir_a(id),
            Evento::Grabar => crate::comando::lanzar(&["gsr-ui-cli", "toggle-record"]),
            Evento::Bandeja {
                clave,
                boton,
                salida,
                derecha,
            } => self.cajon.click(&clave, boton, &salida, derecha),
            Evento::Desplazar { clave, delta } => self.s.bandeja.desplazar(&clave, delta),
            Evento::CalendarioCerrado => self.s.calendario.cerrado(),
        }
    }

    fn grabacion(self: &Rc<Self>) {
        let grabando = self.s.privacidad.estado().grabando;
        let corriendo = self.grabacion.borrow().is_some();
        if grabando && !corriendo {
            let weak = Rc::downgrade(self);
            let id = glib::timeout_add_seconds_local(1, move || match weak.upgrade() {
                Some(inner) => {
                    inner.pintar_grabacion();
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            });
            self.grabacion
                .replace(Some((std::time::Instant::now(), id)));
        } else if !grabando && let Some((_, id)) = self.grabacion.take() {
            id.remove();
        }
        self.pintar_grabacion();
    }

    fn pintar_grabacion(&self) {
        let desde = self.grabacion.borrow().as_ref().map(|(t, _)| *t);
        let texto = desde.map(|t| modelo::duracion(t.elapsed().as_secs()));
        let _ = self.hilo.send(Orden::Grabacion(texto));
    }

    fn reloj(self: &Rc<Self>) {
        if let Some(id) = self.reloj.take() {
            id.remove();
        }
        let espera = self.pintar_reloj();
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(espera, move || {
            if let Some(inner) = weak.upgrade() {
                inner.reloj.take();
                inner.reloj();
            }
        });
        self.reloj.replace(Some(id));
    }

    fn pintar_reloj(&self) -> Duration {
        let ahora = glib::DateTime::now_local().expect("reloj local");
        let fecha = modelo::fecha(ahora.day_of_month(), ahora.month());
        let hora = ahora.format("%H:%M").map(String::from).unwrap_or_default();
        let _ = self.hilo.send(Orden::Reloj { fecha, hora });
        // Unos milisegundos de más: despertar justo antes del cambio pintaría el minuto viejo.
        let falta = (60.0 - ahora.seconds()).max(0.0);
        Duration::from_secs_f64(falta) + Duration::from_millis(20)
    }

    fn privacidad(&self) {
        let e = self.s.privacidad.estado();
        let mut glifos = Vec::new();
        if !e.microfono.is_empty() {
            glifos.push("󰍬");
        }
        if e.pantalla {
            glifos.push("󰹑");
        }
        let tele = self.s.hyprland.espacios().prendida(&self.s.tele);
        if tele {
            glifos.push("󰔂");
        }
        let mut tooltip = privacidad::tooltip(&e.microfono, e.pantalla);
        if tele {
            if !tooltip.is_empty() {
                tooltip.push('\n');
            }
            tooltip.push_str("TV prendida");
        }
        let _ = self.hilo.send(Orden::Privacidad {
            glifos: glifos.join(" "),
            tooltip,
        });
    }

    fn campana(&self) {
        let n = &self.s.notificaciones;
        let _ = self.hilo.send(Orden::Campana {
            avisos: n.lista().len(),
            no_molestar: n.no_molestar(),
        });
    }
}

fn dormir() -> zbus::MatchRule<'static> {
    zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.login1")
        .and_then(|b| b.path("/org/freedesktop/login1"))
        .and_then(|b| b.interface("org.freedesktop.login1.Manager"))
        .and_then(|b| b.member("PrepareForSleep"))
        .map(|b| b.build())
        .expect("regla fija")
}
