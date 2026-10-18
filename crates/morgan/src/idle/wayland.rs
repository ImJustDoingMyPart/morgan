//! Una conexión Wayland propia (la de GTK no se comparte), con su fd en el main loop de glib:
//! sin hilos ni sondeo.
//!
//! `get_idle_notification` respeta los inhibidores de Wayland (`idle-inhibit`: la regla
//! `idle_inhibit` de los juegos, un video a pantalla completa). Cada tiempo es una
//! notificación; cambiar los tiempos las destruye y crea de nuevo, y la cuenta arranca de
//! cero.
//!
//! Esas notificaciones no sirven para saber si volvió el usuario: Hyprland les manda `resumed`
//! cada vez que revisa los inhibidores (abrir, cerrar o enfocar una ventana, la pantalla
//! completa, bloquear), haya entrada o no. Para eso están las de entrada
//! (`get_input_idle_notification`, versión 2), que ignoran a los inhibidores y solo cuentan
//! teclado, mouse y touchpad.

use std::cell::RefCell;
use std::os::fd::{AsFd, AsRawFd};
use std::rc::Rc;

use gtk::glib;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::ExtIdleNotifierV1,
};

use super::Accion;

#[derive(Default)]
struct Estado {
    eventos: Vec<(Accion, bool)>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Estado {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for Estado {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotifierV1, ()> for Estado {
    fn event(
        _: &mut Self,
        _: &ExtIdleNotifierV1,
        _: <ExtIdleNotifierV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotificationV1, Accion> for Estado {
    fn event(
        estado: &mut Self,
        _: &ExtIdleNotificationV1,
        evento: ext_idle_notification_v1::Event,
        accion: &Accion,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match evento {
            ext_idle_notification_v1::Event::Idled => estado.eventos.push((*accion, true)),
            ext_idle_notification_v1::Event::Resumed => estado.eventos.push((*accion, false)),
            _ => {}
        }
    }
}

type AlEvento = Box<dyn Fn(Accion, bool)>;

pub struct Notificador {
    conn: Connection,
    cola: RefCell<EventQueue<Estado>>,
    qh: QueueHandle<Estado>,
    notifier: ExtIdleNotifierV1,
    seat: wl_seat::WlSeat,
    activas: RefCell<Vec<ExtIdleNotificationV1>>,
    de_entrada: RefCell<Vec<(Accion, ExtIdleNotificationV1)>>,
    al_evento: AlEvento,
    fuente: RefCell<Option<glib::SourceId>>,
}

impl Notificador {
    pub fn conectar(al_evento: impl Fn(Accion, bool) + 'static) -> Result<Rc<Self>, String> {
        let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
        let (globals, cola) = registry_queue_init::<Estado>(&conn).map_err(|e| e.to_string())?;
        let qh = cola.handle();
        let notifier: ExtIdleNotifierV1 = globals
            .bind(&qh, 1..=2, ())
            .map_err(|e| format!("ext_idle_notifier_v1: {e}"))?;
        let seat: wl_seat::WlSeat = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| format!("wl_seat: {e}"))?;
        let n = Rc::new(Notificador {
            conn,
            cola: RefCell::new(cola),
            qh,
            notifier,
            seat,
            activas: RefCell::default(),
            de_entrada: RefCell::default(),
            al_evento: Box::new(al_evento),
            fuente: RefCell::default(),
        });
        let fd = n.conn.backend().poll_fd().as_fd().as_raw_fd();
        let debil = Rc::downgrade(&n);
        let id = glib_unix::unix_fd_add_local(
            fd,
            glib::IOCondition::IN | glib::IOCondition::HUP | glib::IOCondition::ERR,
            move |_, cond| {
                let Some(n) = debil.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if cond.intersects(glib::IOCondition::HUP | glib::IOCondition::ERR) {
                    eprintln!("morgan: idle: se cortó la conexión con el compositor");
                    n.fuente.take();
                    return glib::ControlFlow::Break;
                }
                n.leer();
                glib::ControlFlow::Continue
            },
        );
        n.fuente.replace(Some(id));
        Ok(n)
    }

    fn leer(&self) {
        let mut estado = Estado::default();
        {
            let mut cola = self.cola.borrow_mut();
            if let Some(g) = cola.prepare_read() {
                // WouldBlock es posible (otro despertar ya leyó): no es error.
                let _ = g.read();
            }
            if let Err(e) = cola.dispatch_pending(&mut estado) {
                eprintln!("morgan: idle: {e}");
            }
        }
        for (accion, inactivo) in estado.eventos {
            (self.al_evento)(accion, inactivo);
        }
    }

    pub fn poner(&self, tiempos: &[(Accion, u32)]) {
        for n in self.activas.borrow_mut().drain(..) {
            n.destroy();
        }
        let nuevas = tiempos
            .iter()
            .map(|&(a, ms)| {
                self.notifier
                    .get_idle_notification(ms, &self.seat, &self.qh, a)
            })
            .collect();
        *self.activas.borrow_mut() = nuevas;
        if let Err(e) = self.conn.flush() {
            eprintln!("morgan: idle: {e}");
        }
    }

    /// Sin la versión 2 no hay notificaciones de entrada.
    pub fn con_entrada(&self) -> bool {
        self.notifier.version() >= 2
    }

    /// Una por acción: pedirla de nuevo la reemplaza y la cuenta vuelve a cero.
    pub fn pedir_entrada(&self, accion: Accion, ms: u32) {
        if !self.con_entrada() {
            return;
        }
        self.soltar_entrada(accion);
        let n = self
            .notifier
            .get_input_idle_notification(ms, &self.seat, &self.qh, accion);
        self.de_entrada.borrow_mut().push((accion, n));
        if let Err(e) = self.conn.flush() {
            eprintln!("morgan: idle: {e}");
        }
    }

    pub fn soltar_entrada(&self, accion: Accion) {
        self.de_entrada.borrow_mut().retain(|(a, n)| {
            if *a == accion {
                n.destroy();
            }
            *a != accion
        });
        let _ = self.conn.flush();
    }
}

impl Drop for Notificador {
    fn drop(&mut self) {
        if let Some(id) = self.fuente.take() {
            id.remove();
        }
    }
}
