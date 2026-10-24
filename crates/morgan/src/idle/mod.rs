//! El dueño del idle es quien tiene `org.freedesktop.ScreenSaver`. Si es de otro, morgan no
//! arranca nada de esto: dos idles a la vez apagarían y bloquearían por duplicado.
//!
//! Con `MORGAN_SIN_LOGIND=1` (un Hyprland anidado, cuyo bus del sistema es el de verdad) no se
//! toca logind.

mod logind;
mod screensaver;
mod wayland;

use std::cell::{Cell, RefCell};
use std::os::fd::OwnedFd;
use std::rc::Rc;
use std::time::Duration;

use zbus::Proxy;
use zbus::zvariant::OwnedObjectPath;

use crate::config;
use crate::hyprland::Hyprland;
use crate::osd::Aviso;
use logind::Logind;
use wayland::Notificador;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accion {
    Apagar,
    Bloquear,
    Suspender,
    /// De entrada, mientras las pantallas están apagadas: su `resumed` las prende.
    Despertar,
    /// De entrada, con la sesión bloqueada: apaga lo que prendió una tecla y quedó ahí.
    Bloqueada,
}

/// logind espera como mucho `InhibitDelayMaxSec` (5 s); morgan suelta antes para no llegar
/// al tope.
const ESPERA_BLOQUEO: Duration = Duration::from_secs(4);

/// Lo que tarda `Despertar` en quedar inactiva: después, cualquier tecla prende.
const DESPERTAR_MS: u32 = 100;

/// Como GNOME: bloqueada, las pantallas se apagan tras 30 s sin tocar nada.
const APAGAR_BLOQUEADA_MS: u32 = 30_000;

pub fn tiempos(c: &config::Idle, inhibido: bool) -> Vec<(Accion, u32)> {
    if inhibido {
        return Vec::new();
    }
    [
        (Accion::Apagar, c.apagar),
        (Accion::Bloquear, c.bloquear),
        (Accion::Suspender, c.suspender),
    ]
    .into_iter()
    .filter(|&(_, min)| min > 0)
    .map(|(a, min)| (a, min * 60_000))
    .collect()
}

pub fn aviso_caffeine(activo: bool) -> Aviso {
    let (texto, icono) = if activo {
        ("Activado", "󰅶")
    } else {
        ("Desactivado", "󰅶")
    };
    Aviso::estado(icono, "Caffeine", texto, activo, 2000)
}

#[derive(Clone)]
pub struct Idle {
    inner: Rc<Inner>,
}

struct Inner {
    config: RefCell<config::Idle>,
    hyprland: Hyprland,
    activo: Cell<bool>,
    caffeine: Cell<bool>,
    dbus: Cell<usize>,
    logind_inhibe: Cell<bool>,
    apagadas: Cell<bool>,
    bloqueada: Cell<bool>,
    notificador: RefCell<Option<Rc<Notificador>>>,
    logind: RefCell<Option<Rc<Logind>>>,
    inhibidor: RefCell<Option<OwnedFd>>,
    esperando: RefCell<Option<async_channel::Sender<()>>>,
    oyentes: RefCell<Vec<Rc<dyn Fn()>>>,
    _screensaver: RefCell<Option<zbus::Connection>>,
}

impl Idle {
    pub fn new(config: &config::Idle, hyprland: Hyprland) -> Self {
        let idle = Idle {
            inner: Rc::new(Inner {
                config: RefCell::new(config.clone()),
                hyprland,
                activo: Cell::new(false),
                caffeine: Cell::new(false),
                dbus: Cell::new(0),
                logind_inhibe: Cell::new(false),
                apagadas: Cell::new(false),
                bloqueada: Cell::new(false),
                notificador: RefCell::default(),
                logind: RefCell::default(),
                inhibidor: RefCell::default(),
                esperando: RefCell::default(),
                oyentes: RefCell::default(),
                _screensaver: RefCell::default(),
            }),
        };
        let yo = idle.clone();
        glib::MainContext::default().spawn_local(async move { yo.arrancar().await });
        idle
    }

    /// Si otro tiene `org.freedesktop.ScreenSaver`, caffeine queda marcado pero no frena nada.
    pub fn activo(&self) -> bool {
        self.inner.activo.get()
    }

    pub fn caffeine(&self) -> bool {
        self.inner.caffeine.get()
    }

    pub fn set_caffeine(&self, si: bool) {
        if self.inner.caffeine.replace(si) != si {
            if si {
                self.prender();
            }
            self.recalcular();
            let oyentes = self.inner.oyentes.borrow().clone();
            for f in oyentes {
                f();
            }
        }
    }

    pub fn al_cambiar(&self, f: impl Fn() + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    pub fn aplicar_config(&self, c: &config::Idle) {
        *self.inner.config.borrow_mut() = c.clone();
        self.recalcular();
    }

    async fn arrancar(&self) {
        let (tx, rx) = async_channel::unbounded();
        match screensaver::levantar(tx).await {
            Ok(c) => {
                self.inner._screensaver.replace(Some(c));
            }
            Err(e) => {
                eprintln!(
                    "morgan: idle: {} es de otro ({e}); el idle no es de morgan",
                    screensaver::NOMBRE
                );
                return;
            }
        }
        let yo = self.clone();
        let notificador = Notificador::conectar(move |a, inactivo| yo.evento(a, inactivo));
        match notificador {
            Ok(n) => {
                self.inner.notificador.replace(Some(n));
            }
            Err(e) => {
                eprintln!("morgan: idle: {e}");
                return;
            }
        }
        self.inner.activo.set(true);
        if std::env::var_os("MORGAN_SIN_LOGIND").is_none_or(|v| v.is_empty()) {
            self.conectar_logind();
        }
        self.recalcular();
        for f in self.inner.oyentes.borrow().clone() {
            f();
        }
        while let Ok(n) = rx.recv().await {
            if self.inner.dbus.replace(n) != n {
                self.recalcular();
            }
        }
    }

    fn inhibido(&self) -> bool {
        let i = &self.inner;
        i.caffeine.get() || i.dbus.get() > 0 || i.logind_inhibe.get()
    }

    /// Un inhibidor nuevo frena la cuenta pero no prende unas pantallas ya apagadas: eso es
    /// solo de la entrada (`Despertar`).
    fn recalcular(&self) {
        if let Some(n) = self.inner.notificador.borrow().as_ref() {
            n.poner(&tiempos(&self.inner.config.borrow(), self.inhibido()));
        }
    }

    fn evento(&self, accion: Accion, inactivo: bool) {
        match (accion, inactivo) {
            (Accion::Apagar | Accion::Bloqueada, true) => self.apagar(),
            // Sin notificaciones de entrada, el `resumed` es lo único que hay.
            (Accion::Apagar, false) if !self.con_entrada() => self.prender(),
            (Accion::Despertar, false) => self.prender(),
            (Accion::Bloquear, true) => self.bloquear(),
            (Accion::Suspender, true) => self.suspender_por_idle(),
            _ => {}
        }
    }

    fn con_entrada(&self) -> bool {
        let n = self.inner.notificador.borrow();
        n.as_ref().is_some_and(|n| n.con_entrada())
    }

    fn apagar(&self) {
        if self.inner.apagadas.replace(true) {
            return;
        }
        eprintln!("morgan: idle: pantallas apagadas");
        self.inner.hyprland.dpms(false);
        if let Some(n) = self.inner.notificador.borrow().as_ref() {
            n.pedir_entrada(Accion::Despertar, DESPERTAR_MS);
        }
    }

    fn prender(&self) {
        if let Some(n) = self.inner.notificador.borrow().as_ref() {
            n.soltar_entrada(Accion::Despertar);
        }
        if self.inner.apagadas.replace(false) {
            eprintln!("morgan: idle: pantallas prendidas");
            self.inner.hyprland.dpms(true);
        }
    }

    /// Sin logind no hace nada: suspendería la máquina.
    fn suspender_por_idle(&self) {
        let logind = self.inner.logind.borrow().clone();
        glib::MainContext::default().spawn_local(async move {
            if let Some(l) = logind
                && let Err(e) = l.suspender().await
            {
                eprintln!("morgan: idle: suspender: {e}");
            }
        });
    }

    /// La señal `Lock` que vuelve es la que arranca `morgan-lock`.
    pub fn bloquear(&self) {
        let logind = self.inner.logind.borrow().clone();
        glib::MainContext::default().spawn_local(async move {
            match logind {
                Some(l) => {
                    if let Err(e) = l.bloquear().await {
                        eprintln!("morgan: idle: bloquear: {e}");
                    }
                }
                None => lanzar_sin_logind(),
            }
        });
    }

    fn conectar_logind(&self) {
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let l = match Logind::conectar().await {
                Ok(l) => Rc::new(l),
                Err(e) => {
                    eprintln!("morgan: idle: logind: {e}");
                    return;
                }
            };
            yo.inner.logind.replace(Some(l.clone()));
            yo.releer_bloqueo(&l).await;
            yo.tomar_inhibidor().await;
            yo.releer_inhibidores().await;
            // Cada escucha en su tarea, con su copia de la conexión.
            let ctx = glib::MainContext::default();
            let (l1, l2, l3, l4) = (l.clone(), l.clone(), l.clone(), l);
            let (y2, y3, y4) = (yo.clone(), yo.clone(), yo);
            ctx.spawn_local(async move {
                registrar(l1.al_lock(arrancar_morgan_lock).await);
            });
            ctx.spawn_local(async move {
                registrar(l2.al_suspender(move |antes| y2.suspender(antes)).await);
            });
            ctx.spawn_local(async move {
                registrar(l3.al_cambiar_bloqueo(move || y3.cambio_bloqueo()).await);
            });
            ctx.spawn_local(async move {
                let r = l4
                    .al_cambiar_inhibidores(move || {
                        let y = y4.clone();
                        glib::MainContext::default()
                            .spawn_local(async move { y.releer_inhibidores().await });
                    })
                    .await;
                registrar(r);
            });
        });
    }

    async fn tomar_inhibidor(&self) {
        let Some(l) = self.inner.logind.borrow().clone() else {
            return;
        };
        match l.inhibir_suspension().await {
            Ok(fd) => {
                self.inner.inhibidor.replace(Some(fd));
            }
            Err(e) => eprintln!("morgan: idle: inhibidor de suspensión: {e}"),
        }
    }

    async fn releer_inhibidores(&self) {
        let Some(l) = self.inner.logind.borrow().clone() else {
            return;
        };
        let si = l.idle_inhibido().await.unwrap_or(false);
        if self.inner.logind_inhibe.replace(si) != si {
            self.recalcular();
        }
    }

    /// Soltar el inhibidor recién con la sesión bloqueada (o al tope de espera).
    fn suspender(&self, antes: bool) {
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let Some(l) = yo.inner.logind.borrow().clone() else {
                return;
            };
            if !antes {
                // Al despertar, prendidas aunque morgan las creyera prendidas.
                yo.inner.apagadas.set(true);
                yo.prender();
                yo.tomar_inhibidor().await;
                return;
            }
            if !l.bloqueada().await.unwrap_or(false) {
                let (tx, rx) = async_channel::bounded(1);
                yo.inner.esperando.replace(Some(tx));
                if let Err(e) = l.bloquear().await {
                    eprintln!("morgan: idle: bloquear antes de suspender: {e}");
                }
                // `LockedHint` pudo cambiar antes de que se armara la espera.
                if !l.bloqueada().await.unwrap_or(false) {
                    futures_lite::future::or(
                        async {
                            let _ = rx.recv().await;
                        },
                        glib::timeout_future(ESPERA_BLOQUEO),
                    )
                    .await;
                }
                yo.inner.esperando.take();
            }
            yo.inner.inhibidor.take();
        });
    }

    fn cambio_bloqueo(&self) {
        let Some(l) = self.inner.logind.borrow().clone() else {
            return;
        };
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            yo.releer_bloqueo(&l).await;
            if yo.inner.bloqueada.get()
                && let Some(tx) = yo.inner.esperando.borrow().clone()
            {
                let _ = tx.try_send(());
            }
        });
    }

    async fn releer_bloqueo(&self, l: &Logind) {
        let si = l.bloqueada().await.unwrap_or(false);
        if self.inner.bloqueada.replace(si) == si {
            return;
        }
        if let Some(n) = self.inner.notificador.borrow().as_ref() {
            if si {
                n.pedir_entrada(Accion::Bloqueada, APAGAR_BLOQUEADA_MS);
            } else {
                n.soltar_entrada(Accion::Bloqueada);
            }
        }
    }
}

fn registrar(r: zbus::Result<()>) {
    if let Err(e) = r {
        eprintln!("morgan: idle: logind: {e}");
    }
}

/// Fuera del cgroup de morgan, y arrancar una unidad activa no hace nada: dos bloqueos no se
/// apilan.
fn arrancar_morgan_lock() {
    glib::MainContext::default().spawn_local(async {
        let r: zbus::Result<OwnedObjectPath> = async {
            let c = zbus::Connection::session().await?;
            let p = Proxy::new(
                &c,
                "org.freedesktop.systemd1",
                "/org/freedesktop/systemd1",
                "org.freedesktop.systemd1.Manager",
            )
            .await?;
            p.call("StartUnit", &("morgan-lock.service", "replace"))
                .await
        }
        .await;
        if let Err(e) = r {
            eprintln!("morgan: idle: arrancar morgan-lock: {e}");
        }
    });
}

fn lanzar_sin_logind() {
    crate::comando::lanzar(&["morgan-lock", "--pam", "vlock", "--sin-logind"]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn por_defecto() {
        let t = tiempos(&config::Idle::default(), false);
        assert_eq!(
            t,
            [
                (Accion::Apagar, 600_000),
                (Accion::Bloquear, 1_200_000),
                (Accion::Suspender, 3_600_000)
            ]
        );
    }

    #[test]
    fn inhibido_no_corre_ninguno() {
        assert!(tiempos(&config::Idle::default(), true).is_empty());
    }

    #[test]
    fn cero_es_nunca() {
        let c = config::Idle {
            apagar: 0,
            bloquear: 5,
            suspender: 0,
        };
        assert_eq!(tiempos(&c, false), [(Accion::Bloquear, 300_000)]);
        let c = config::Idle { suspender: 60, ..c };
        assert_eq!(
            tiempos(&c, false),
            [(Accion::Bloquear, 300_000), (Accion::Suspender, 3_600_000)]
        );
    }
}
