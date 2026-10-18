//! Los pedidos que también tienen `morganctl` (centro, No molestar) pasan por el mismo
//! `dispatch` que el socket, así que cierran las otras superficies y muestran el OSD igual.
//!
//! Cero sondeo: el contador de la grabación corre solo mientras se graba; el reloj se
//! despierta al cambiar el minuto y al volver de una suspensión (el temporizador de glib no
//! cuenta el tiempo dormido).

mod bandeja;
mod menu_bandeja;
mod modelo;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use morgan_ipc::{AccionInterruptor, AccionSuperficie, Request};

use crate::bandeja::Bandeja;
use crate::hyprland::{Espacios, Hyprland};
use crate::notificaciones::Notificaciones;
use crate::privacidad::{self, Privacidad};
use crate::sistema;

const ALTO: i32 = 28;

pub struct Servicios {
    pub hyprland: Hyprland,
    pub notificaciones: Notificaciones,
    pub fijos: BTreeMap<String, Vec<NonZeroU32>>,
    pub bandeja: Bandeja,
    pub privacidad: Privacidad,
    pub iconos_bandeja: BTreeMap<String, String>,
    pub tele: String,
    pub pedir: Rc<dyn Fn(Request)>,
}

#[derive(Clone)]
pub struct Barra {
    inner: Rc<Inner>,
}

struct Inner {
    app: gtk::Application,
    s: Servicios,
    cajon: Rc<bandeja::Contexto>,
    ventanas: RefCell<Vec<Ventana>>,
    reloj: RefCell<Option<glib::SourceId>>,
    centro_abierto: std::cell::Cell<bool>,
    calendario_abierto: std::cell::Cell<bool>,
    grabacion: RefCell<Option<(std::time::Instant, glib::SourceId)>>,
    despertar: RefCell<Option<glib::JoinHandle<()>>>,
}

impl Barra {
    pub fn new(app: &gtk::Application, display: &gdk::Display, s: Servicios) -> Self {
        let cajon = Rc::new(bandeja::Contexto::new(
            s.bandeja.clone(),
            s.iconos_bandeja.clone(),
        ));
        let inner = Rc::new(Inner {
            app: app.clone(),
            cajon,
            s,
            ventanas: RefCell::default(),
            reloj: RefCell::default(),
            centro_abierto: Default::default(),
            calendario_abierto: Default::default(),
            grabacion: RefCell::default(),
            despertar: RefCell::default(),
        });

        let monitores = display.monitors();
        let weak = Rc::downgrade(&inner);
        monitores.connect_items_changed(move |lista, _, _, _| {
            if let Some(inner) = weak.upgrade() {
                inner.rearmar(lista);
            }
        });
        inner.rearmar(&monitores);

        let weak = Rc::downgrade(&inner);
        inner.s.hyprland.al_cambiar_espacios(move |e| {
            if let Some(inner) = weak.upgrade() {
                for v in inner.ventanas.borrow().iter() {
                    v.espacios(e, &inner.s);
                }
                inner.privacidad();
            }
        });
        let weak = Rc::downgrade(&inner);
        inner.s.bandeja.al_cambiar(move || {
            if let Some(inner) = weak.upgrade() {
                for v in inner.ventanas.borrow().iter() {
                    v.cajon.pintar(&inner.cajon, &v.window);
                }
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
        Barra { inner }
    }

    pub fn marcar_centro(&self, abierto: bool) {
        self.inner.centro_abierto.set(abierto);
        self.inner.campana();
    }

    pub fn marcar_calendario(&self, abierto: bool) {
        self.inner.calendario_abierto.set(abierto);
        self.inner.marcar_reloj();
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(h) = self.despertar.take() {
            h.abort();
        }
    }
}

impl Inner {
    fn rearmar(self: &Rc<Self>, lista: &gio::ListModel) {
        for v in self.ventanas.take() {
            v.window.destroy();
        }
        let nuevas: Vec<Ventana> = (0..lista.n_items())
            .filter_map(|i| lista.item(i)?.downcast::<gdk::Monitor>().ok())
            .map(|m| Ventana::new(self, &m))
            .collect();
        let espacios = self.s.hyprland.espacios();
        for v in &nuevas {
            let weak = Rc::downgrade(self);
            v.monitor.connect_connector_notify(move |_| {
                if let Some(inner) = weak.upgrade() {
                    let espacios = inner.s.hyprland.espacios();
                    for v in inner.ventanas.borrow().iter() {
                        v.espacios(&espacios, &inner.s);
                    }
                }
            });
            v.espacios(&espacios, &self.s);
            v.cajon.pintar(&self.cajon, &v.window);
        }
        self.ventanas.replace(nuevas);
        self.campana();
        self.marcar_reloj();
        self.privacidad();
        self.pintar_grabacion();
        self.pintar_reloj();
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
        let texto = desde
            .map(|t| modelo::duracion(t.elapsed().as_secs()))
            .unwrap_or_default();
        for v in self.ventanas.borrow().iter() {
            v.grabacion.set_label(&format!("󰑋 {texto}"));
            v.grabacion.set_visible(desde.is_some());
        }
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
        for v in self.ventanas.borrow().iter() {
            v.fecha.set_label(&fecha);
            v.hora.set_label(&hora);
        }
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
        let texto = glifos.join(" ");
        let mut tooltip = privacidad::tooltip(&e.microfono, e.pantalla);
        if tele {
            if !tooltip.is_empty() {
                tooltip.push('\n');
            }
            tooltip.push_str("TV prendida");
        }
        for v in self.ventanas.borrow().iter() {
            v.privacidad.set_label(&texto);
            v.privacidad.set_tooltip_text(Some(&tooltip));
            v.privacidad.set_visible(!glifos.is_empty());
        }
    }

    fn marcar_reloj(&self) {
        let abierto = self.calendario_abierto.get();
        for v in self.ventanas.borrow().iter() {
            if abierto {
                v.reloj.add_css_class("abierto");
            } else {
                v.reloj.remove_css_class("abierto");
            }
        }
    }

    fn campana(&self) {
        let n = &self.s.notificaciones;
        let (estado, tooltip) = modelo::campana(n.lista().len(), n.no_molestar());
        let abierto = self.centro_abierto.get().then_some("abierto");
        let clases: Vec<&str> = ["campana", "boton"]
            .into_iter()
            .chain(estado)
            .chain(abierto)
            .collect();
        for v in self.ventanas.borrow().iter() {
            let viejas = v.campana.css_classes();
            let iguales = viejas.len() == clases.len()
                && viejas.iter().zip(&clases).all(|(vi, c)| vi.as_str() == *c);
            if !iguales {
                v.campana.set_css_classes(&clases);
            }
            if v.campana.tooltip_text().as_deref() != Some(&tooltip) {
                v.campana.set_tooltip_text(Some(&tooltip));
            }
        }
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

struct Ventana {
    window: gtk::ApplicationWindow,
    /// Se lee al pintar: GDK puede anunciar el monitor antes de saber el conector, y copiarlo al
    /// crear la ventana dejaría la barra sin workspaces para siempre.
    monitor: gdk::Monitor,
    espacios: gtk::Box,
    botones: RefCell<Vec<(i64, gtk::Label)>>,
    fecha: gtk::Label,
    hora: gtk::Label,
    reloj: gtk::Box,
    campana: gtk::Label,
    cajon: bandeja::Cajon,
    privacidad: gtk::Label,
    grabacion: gtk::Label,
}

impl Ventana {
    fn new(inner: &Rc<Inner>, monitor: &gdk::Monitor) -> Self {
        let window = gtk::ApplicationWindow::new(&inner.app);
        window.add_css_class("morgan");
        window.add_css_class("barra");
        window.init_layer_shell();
        window.set_namespace(Some("morgan-bar"));
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_monitor(Some(monitor));
        for borde in [Edge::Top, Edge::Left, Edge::Right] {
            window.set_anchor(borde, true);
        }
        window.auto_exclusive_zone_enable();
        window.set_default_size(-1, ALTO);

        let espacios = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        espacios.add_css_class("espacios");

        let fecha = gtk::Label::new(None);
        fecha.add_css_class("fecha");
        let separador = gtk::Label::new(Some("›"));
        separador.add_css_class("separador");
        let hora = gtk::Label::new(None);
        hora.add_css_class("hora");
        let centro = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        centro.add_css_class("reloj");
        centro.add_css_class("boton");
        centro.append(&fecha);
        centro.append(&separador);
        centro.append(&hora);
        // Izquierdo: el calendario. Derecho: el centro.
        al_click(&centro, &inner.s.pedir, |boton| match boton {
            1 => Some(Request::Reloj {
                accion: AccionSuperficie::Alternar,
            }),
            3 => Some(centro_alternar()),
            _ => None,
        });

        let campana = gtk::Label::new(Some("󰨝"));
        campana.add_css_class("campana");
        let derecha = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        derecha.add_css_class("derecha");
        let grabacion = gtk::Label::new(None);
        grabacion.add_css_class("grabacion");
        grabacion.add_css_class("boton");
        grabacion.set_tooltip_text(Some("Grabando la pantalla\nClick: parar"));
        grabacion.set_visible(false);
        let click = gtk::GestureClick::new();
        apretable(&click);
        click.connect_released(|_, _, _, _| {
            crate::comando::lanzar(&["gsr-ui-cli", "toggle-record"]);
        });
        grabacion.add_controller(click);
        derecha.append(&grabacion);
        let privacidad = gtk::Label::new(None);
        privacidad.add_css_class("privacidad");
        privacidad.set_visible(false);
        derecha.append(&privacidad);
        let cajon = bandeja::Cajon::new();
        derecha.append(&cajon.raiz);
        derecha.append(&campana);
        // Izquierdo: el centro. Derecho: No molestar.
        al_click(&campana, &inner.s.pedir, |boton| match boton {
            1 => Some(centro_alternar()),
            3 => Some(Request::NoMolestar {
                accion: AccionInterruptor::Alternar,
            }),
            _ => None,
        });

        let caja = gtk::CenterBox::new();
        caja.set_start_widget(Some(&espacios));
        caja.set_center_widget(Some(&centro));
        caja.set_end_widget(Some(&derecha));
        window.set_child(Some(&caja));
        window.present();

        Ventana {
            window,
            monitor: monitor.clone(),
            espacios,
            botones: RefCell::default(),
            fecha,
            hora,
            reloj: centro,
            campana,
            cajon,
            privacidad,
            grabacion,
        }
    }

    fn espacios(&self, e: &Espacios, s: &Servicios) {
        let monitor = self
            .monitor
            .connector()
            .map(String::from)
            .unwrap_or_default();
        let fijos: Vec<i64> = s
            .fijos
            .get(&monitor)
            .map(|v| v.iter().map(|i| i64::from(i.get())).collect())
            .unwrap_or_default();
        let nuevos = modelo::botones(e, &monitor, &fijos);
        let mut botones = self.botones.borrow_mut();
        let mismos = botones.len() == nuevos.len()
            && botones.iter().zip(&nuevos).all(|((id, _), b)| *id == b.id);
        if !mismos {
            for (_, l) in botones.drain(..) {
                self.espacios.remove(&l);
            }
            for b in &nuevos {
                let l = gtk::Label::new(Some(&b.etiqueta));
                let hyprland = s.hyprland.clone();
                let id = b.id;
                let click = gtk::GestureClick::new();
                apretable(&click);
                click.connect_released(move |_, _, _, _| hyprland.ir_a(id));
                l.add_controller(click);
                self.espacios.append(&l);
                botones.push((b.id, l));
            }
        }
        for ((_, l), b) in botones.iter().zip(&nuevos) {
            let clases = [
                (b.activo, "activo"),
                (b.vacio, "vacio"),
                (b.urgente, "urgente"),
            ];
            let nuevas: Vec<&str> = std::iter::once("boton")
                .chain(clases.iter().filter(|(si, _)| *si).map(|(_, c)| *c))
                .collect();
            let viejas = l.css_classes();
            let iguales = viejas.len() == nuevas.len()
                && viejas.iter().zip(&nuevas).all(|(v, n)| v.as_str() == *n);
            if !iguales {
                l.set_css_classes(&nuevas);
            }
        }
    }
}

fn centro_alternar() -> Request {
    Request::Centro {
        accion: AccionSuperficie::Alternar,
    }
}

/// GTK pone `:active` solo a los `Button`: lo que se toca en la barra son etiquetas, cajas e
/// imágenes, y la hoja los hunde.
fn apretable(click: &gtk::GestureClick) {
    click.connect_pressed(|g, _, _, _| {
        if let Some(w) = g.widget() {
            w.set_state_flags(gtk::StateFlags::ACTIVE, false);
        }
    });
    // `stopped` llega al soltar y también si el gesto se cancela (el puntero se va).
    click.connect_stopped(|g| {
        if let Some(w) = g.widget() {
            w.unset_state_flags(gtk::StateFlags::ACTIVE);
        }
    });
}

fn al_click(
    w: &impl IsA<gtk::Widget>,
    pedir: &Rc<dyn Fn(Request)>,
    que: impl Fn(u32) -> Option<Request> + 'static,
) {
    let click = gtk::GestureClick::new();
    click.set_button(0);
    let pedir = pedir.clone();
    apretable(&click);
    click.connect_released(move |g, _, _, _| {
        if let Some(r) = que(g.current_button()) {
            pedir(r);
        }
    });
    w.add_controller(click);
}
