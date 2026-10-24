//! El tema es el de GNOME (`org.gnome.desktop.sound theme-name`, el que leen las apps GTK) y
//! morgan se lo pasa a canberra en cada sonido: así suena el puesto sin depender de cómo lo lee
//! GTK, y la vista previa de otro no lo toca.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use futures_lite::StreamExt;
use gio::prelude::*;
use libpulse_binding as pulse;
use pulse::context::subscribe::{Facility, InterestMaskSet, Operation};
use zbus::Proxy;
use zbus::zvariant::OwnedObjectPath;

use crate::audio::Conexion;
use crate::config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evento {
    Volumen,
    Conectado,
    Desconectado,
    RedConectada,
    RedPerdida,
    Bloqueo,
    Desbloqueo,
    GrabacionGuardada,
}

impl Evento {
    pub fn nombre(self) -> &'static str {
        match self {
            Evento::Volumen => "audio-volume-change",
            Evento::Conectado => "device-added",
            Evento::Desconectado => "device-removed",
            Evento::RedConectada => "network-connectivity-established",
            Evento::RedPerdida => "network-connectivity-lost",
            Evento::Bloqueo => "service-logout",
            Evento::Desbloqueo => "service-login",
            Evento::GrabacionGuardada => "complete",
        }
    }

    pub fn prendido(self, c: &config::Sonido) -> bool {
        match self {
            Evento::Volumen => c.al_cambiar_volumen,
            Evento::Conectado | Evento::Desconectado => c.dispositivos,
            Evento::RedConectada | Evento::RedPerdida => c.red,
            Evento::Bloqueo | Evento::Desbloqueo => c.bloqueo,
            Evento::GrabacionGuardada => c.grabacion,
        }
    }
}

pub const MUESTRAS: &[(&str, &str)] = &[
    ("message-new-instant", "Notificación"),
    ("dialog-warning", "Notificación crítica"),
    ("audio-volume-change", "Volumen"),
    ("device-added", "Dispositivo conectado"),
    ("device-removed", "Dispositivo desconectado"),
    ("network-connectivity-established", "Red conectada"),
    ("network-connectivity-lost", "Red perdida"),
    ("service-logout", "Bloqueo"),
    ("service-login", "Desbloqueo"),
    ("complete", "Grabación guardada"),
];

const DEMO: &[&str] = &[
    "service-login",
    "device-added",
    "message-new-instant",
    "complete",
];

/// canberra cae a él con lo que a otro le falta.
pub const TEMA_BASE: &str = "freedesktop";

#[derive(Debug, Clone, PartialEq)]
pub struct TemaSonido {
    pub carpeta: String,
    pub nombre: String,
    pub propios: usize,
    pub demo: &'static str,
}

pub fn nombre_tema(index: &str) -> Option<String> {
    let mut en_tema = false;
    let mut nombre = None;
    for l in index.lines().map(str::trim) {
        if l.starts_with('[') {
            en_tema = l == "[Sound Theme]";
        } else if en_tema {
            match l.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
                Some(("Name", v)) if !v.is_empty() => nombre = Some(v.to_owned()),
                Some(("Hidden", "true")) => return None,
                _ => {}
            }
        }
    }
    nombre
}

fn trae(dir: &Path, id: &str) -> bool {
    let en = |d: &Path| {
        ["oga", "ogg", "wav"]
            .iter()
            .any(|ext| d.join(format!("{id}.{ext}")).is_file())
    };
    en(dir)
        || std::fs::read_dir(dir).is_ok_and(|r| {
            r.flatten()
                .any(|e| e.file_type().is_ok_and(|t| t.is_dir()) && en(&e.path()))
        })
}

pub fn leer_tema(dir: &Path) -> Option<TemaSonido> {
    let nombre = nombre_tema(&std::fs::read_to_string(dir.join("index.theme")).ok()?)?;
    let propios = MUESTRAS.iter().filter(|(id, _)| trae(dir, id)).count();
    let demo = DEMO
        .iter()
        .chain(MUESTRAS.iter().map(|(id, _)| id))
        .find(|id| trae(dir, id))
        .copied()
        .unwrap_or(DEMO[0]);
    Some(TemaSonido {
        carpeta: dir.file_name()?.to_string_lossy().into_owned(),
        nombre,
        propios,
        demo,
    })
}

/// Si una carpeta se repite, vale la primera, como en canberra.
pub fn temas() -> Vec<TemaSonido> {
    let mut v: Vec<TemaSonido> = Vec::new();
    let bases = std::iter::once(glib::user_data_dir()).chain(glib::system_data_dirs());
    for base in bases {
        let Ok(r) = std::fs::read_dir(base.join("sounds")) else {
            continue;
        };
        for e in r.flatten() {
            if let Some(t) = leer_tema(&e.path())
                && !v.iter().any(|o| o.carpeta == t.carpeta)
            {
                v.push(t);
            }
        }
    }
    v.sort_by_key(|t| t.nombre.to_lowercase());
    v
}

fn ajustes() -> Option<gio::Settings> {
    const ESQUEMA: &str = "org.gnome.desktop.sound";
    gio::SettingsSchemaSource::default()?.lookup(ESQUEMA, true)?;
    Some(gio::Settings::new(ESQUEMA))
}

pub fn tema_puesto() -> String {
    ajustes()
        .map(|s| s.string("theme-name").to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| TEMA_BASE.into())
}

pub fn poner_tema(carpeta: &str) {
    let Some(s) = ajustes() else {
        eprintln!("morgan: sonidos: falta el esquema org.gnome.desktop.sound");
        return;
    };
    if let Err(e) = s.set_string("theme-name", carpeta) {
        eprintln!("morgan: sonidos: tema {carpeta}: {e}");
    }
    gio::Settings::sync();
}

pub fn sonar(id: &str, tema: Option<&str>) {
    let tema = tema.map_or_else(tema_puesto, str::to_owned);
    crate::comando::lanzar(&[
        "canberra-gtk-play",
        "-i",
        id,
        "-d",
        "morgan",
        &format!("--property=canberra.xdg-theme.name={tema}"),
    ]);
}

const NM_CONECTADO: u32 = 70;

pub fn evento_red(antes: Option<u32>, ahora: u32) -> Option<Evento> {
    let antes = antes?;
    match (antes >= NM_CONECTADO, ahora >= NM_CONECTADO) {
        (false, true) => Some(Evento::RedConectada),
        (true, false) => Some(Evento::RedPerdida),
        _ => None,
    }
}

#[derive(Clone)]
pub struct Sonidos {
    inner: Rc<Inner>,
}

struct Inner {
    config: RefCell<config::Sonido>,
    /// El monitor se pide en ocioso, después de armar todo: crearlo es una llamada D-Bus
    /// síncrona.
    _monitor: RefCell<Option<gio::VolumeMonitor>>,
    pulse: RefCell<Option<Conexion>>,
}

impl Sonidos {
    pub fn new(c: &config::Sonido) -> Self {
        let s = Sonidos {
            inner: Rc::new(Inner {
                config: RefCell::new(c.clone()),
                _monitor: RefCell::default(),
                pulse: RefCell::default(),
            }),
        };
        let weak = Rc::downgrade(&s.inner);
        glib::idle_add_local_once(move || {
            let monitor = gio::VolumeMonitor::get();
            if let Some(inner) = weak.upgrade() {
                let yo = Sonidos { inner: inner.clone() };
                monitor.connect_drive_connected(move |_, _| yo.tocar(Evento::Conectado));
                let yo = Sonidos { inner: inner.clone() };
                monitor.connect_drive_disconnected(move |_, _| yo.tocar(Evento::Desconectado));
                inner._monitor.replace(Some(monitor));
            }
        });
        s.seguir_pulse();
        s.seguir_red();
        s.seguir_bloqueo();
        s
    }

    pub fn aplicar_config(&self, c: &config::Sonido) {
        self.inner.config.replace(c.clone());
    }

    pub fn tocar(&self, e: Evento) {
        if e.prendido(&self.inner.config.borrow()) {
            sonar(e.nombre(), None);
        }
    }

    fn seguir_pulse(&self) {
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let c = match Conexion::abrir().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("morgan: sonidos: {e}");
                    return;
                }
            };
            let weak = Rc::downgrade(&yo.inner);
            {
                let mut ctx = c.context.borrow_mut();
                ctx.set_subscribe_callback(Some(Box::new(move |facility, op, _| {
                    let Some(inner) = weak.upgrade() else { return };
                    let e = match (facility, op) {
                        (Some(Facility::Sink), Some(Operation::New)) => Evento::Conectado,
                        (Some(Facility::Sink), Some(Operation::Removed)) => Evento::Desconectado,
                        _ => return,
                    };
                    Sonidos { inner }.tocar(e);
                })));
                ctx.subscribe(InterestMaskSet::SINK, |_| {});
            }
            yo.inner.pulse.replace(Some(c));
        });
    }

    fn seguir_red(&self) {
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = yo.escuchar_red().await {
                eprintln!("morgan: sonidos: NetworkManager: {e}");
            }
        });
    }

    async fn escuchar_red(&self) -> zbus::Result<()> {
        let conn = crate::sistema::bus().await?;
        let nm = Proxy::new(
            &conn,
            "org.freedesktop.NetworkManager",
            "/org/freedesktop/NetworkManager",
            "org.freedesktop.NetworkManager",
        )
        .await?;
        let mut antes: Option<u32> = nm.get_property("State").await.ok();
        let mut cambios = nm.receive_signal("StateChanged").await?;
        while let Some(m) = cambios.next().await {
            let Ok(ahora) = m.body().deserialize::<u32>() else {
                continue;
            };
            if let Some(e) = evento_red(antes, ahora) {
                self.tocar(e);
            }
            antes = Some(ahora);
        }
        Ok(())
    }

    fn seguir_bloqueo(&self) {
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = yo.escuchar_bloqueo().await {
                eprintln!("morgan: sonidos: logind: {e}");
            }
        });
    }

    /// La ruta de verdad, no `auto`, que no emite.
    async fn escuchar_bloqueo(&self) -> zbus::Result<()> {
        if std::env::var_os("MORGAN_SIN_LOGIND").is_some() {
            return Ok(());
        }
        const LOGIND: &str = "org.freedesktop.login1";
        const SESION: &str = "org.freedesktop.login1.Session";
        let conn = crate::sistema::bus().await?;
        let auto = Proxy::new(
            &conn,
            LOGIND,
            "/org/freedesktop/login1/session/auto",
            SESION,
        )
        .await?;
        let id: String = auto.get_property("Id").await?;
        let manager = Proxy::new(
            &conn,
            LOGIND,
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .await?;
        let ruta: OwnedObjectPath = manager.call("GetSession", &(id,)).await?;
        let sesion = Proxy::new(&conn, LOGIND, ruta, SESION).await?;
        let mut antes: bool = sesion.get_property("LockedHint").await.unwrap_or(false);
        let mut cambios = sesion.receive_property_changed::<bool>("LockedHint").await;
        while let Some(c) = cambios.next().await {
            let Ok(ahora) = c.get().await else { continue };
            if ahora != antes {
                self.tocar(if ahora {
                    Evento::Bloqueo
                } else {
                    Evento::Desbloqueo
                });
            }
            antes = ahora;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_suena_solo_al_cambiar() {
        assert_eq!(evento_red(None, 70), None);
        assert_eq!(evento_red(Some(20), 70), Some(Evento::RedConectada));
        assert_eq!(evento_red(Some(70), 20), Some(Evento::RedPerdida));
        // Conectando (40) o sitio sin internet (60) no es "conectado".
        assert_eq!(evento_red(Some(70), 60), Some(Evento::RedPerdida));
        assert_eq!(evento_red(Some(20), 40), None);
        assert_eq!(evento_red(Some(70), 70), None);
    }

    #[test]
    fn cada_evento_tiene_su_muestra() {
        use Evento::*;
        for e in [
            Volumen,
            Conectado,
            Desconectado,
            RedConectada,
            RedPerdida,
            Bloqueo,
            Desbloqueo,
            GrabacionGuardada,
        ] {
            assert!(MUESTRAS.iter().any(|(id, _)| *id == e.nombre()), "{e:?}");
        }
    }

    #[test]
    fn el_nombre_sale_de_su_seccion() {
        let index =
            "[Sound Theme]\nName=Ocean\nComment=x\nDirectories=stereo\n\n[stereo]\nName=Otro\n";
        assert_eq!(nombre_tema(index).as_deref(), Some("Ocean"));
        assert_eq!(nombre_tema("[Sound Theme]\nName=X\nHidden=true\n"), None);
        assert_eq!(nombre_tema("[Icon Theme]\nName=X\n"), None);
    }

    #[test]
    fn un_tema_cuenta_lo_que_trae() {
        let dir = std::env::temp_dir().join(format!("morgan-sonidos-{}", std::process::id()));
        let tema = dir.join("prueba");
        std::fs::create_dir_all(tema.join("stereo")).unwrap();
        std::fs::write(tema.join("index.theme"), "[Sound Theme]\nName=Prueba\n").unwrap();
        std::fs::write(tema.join("stereo/device-added.oga"), "").unwrap();
        std::fs::write(tema.join("stereo/complete.ogg"), "").unwrap();
        std::fs::write(tema.join("stereo/action.oga"), "").unwrap();
        let t = leer_tema(&tema).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(t.carpeta, "prueba");
        assert_eq!(t.nombre, "Prueba");
        assert_eq!(t.propios, 2);
        assert_eq!(t.demo, "device-added");
    }

    #[test]
    fn cada_evento_tiene_su_casilla() {
        let mut c = config::Sonido::default();
        assert!(Evento::Volumen.prendido(&c));
        c.red = false;
        assert!(!Evento::RedPerdida.prendido(&c));
        assert!(Evento::Bloqueo.prendido(&c));
    }
}
