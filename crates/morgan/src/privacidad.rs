//! - **Micrófono**: se cuenta lo que graba de una fuente que no es el monitor de una salida (un
//!   visualizador de audio no es el micrófono) y no está en pausa. A diferencia del mezclador
//!   del centro, esta conexión vive siempre: el indicador tiene que estar al día con todo
//!   cerrado.
//! - **Grabación** (GPU Screen Recorder): captura por KMS, sin el portal, así que Hyprland no
//!   la ve. Lo que sí se ve es su audio en pulse. El replay (`-r`) no cuenta. Límites: una
//!   grabación sin pistas de audio no se ve, y una que se arranca con el replay corriendo (la
//!   hace el mismo proceso, con una señal) tampoco.
//! - **Pantalla**: el evento `screencast` de Hyprland no se puede consultar, así que lo lleva
//!   `hyprland.rs`. Cuenta solo si dura `COMPARTIR_MINIMO`: las fotos de ventanas de morgan
//!   también lo disparan.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::State;
use pulse::context::subscribe::InterestMaskSet;

use crate::audio::Conexion;
use crate::hyprland::Hyprland;

const REINTENTO: Duration = Duration::from_secs(2);

/// Las fotos de ventanas de morgan también llegan como `screencast` y duran menos de un
/// segundo; compartir de verdad dura mucho más.
const COMPARTIR_MINIMO: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grabacion {
    pub fuente: u32,
    pub app: String,
    pub pausada: bool,
    pub binario: String,
    pub pid: Option<u32>,
}

const GRABADOR: &str = "gpu-screen-recorder";

pub fn es_replay<S: AsRef<str>>(argumentos: &[S]) -> bool {
    argumentos.iter().any(|a| a.as_ref() == "-r")
}

pub fn grabando(grabaciones: &[Grabacion], argumentos: impl Fn(u32) -> Vec<String>) -> bool {
    grabaciones
        .iter()
        .any(|g| g.binario == GRABADOR && g.pid.is_none_or(|pid| !es_replay(&argumentos(pid))))
}

fn argumentos(pid: u32) -> Vec<String> {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|b| {
            b.split(|c| *c == 0)
                .filter(|a| !a.is_empty())
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect()
        })
        .unwrap_or_default()
}

pub fn usando_microfono(grabaciones: &[Grabacion], monitores: &HashSet<u32>) -> Vec<String> {
    let mut v: Vec<String> = grabaciones
        .iter()
        .filter(|g| !g.pausada && !monitores.contains(&g.fuente))
        .map(|g| g.app.clone())
        .collect();
    v.sort();
    v.dedup();
    v
}

pub fn tooltip(microfono: &[String], pantalla: bool) -> String {
    let mut l = Vec::new();
    if !microfono.is_empty() {
        l.push(format!("Micrófono: {}", microfono.join(", ")));
    }
    if pantalla {
        l.push("Pantalla compartida".to_owned());
    }
    l.join("\n")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Estado {
    pub microfono: Vec<String>,
    pub pantalla: bool,
    pub grabando: bool,
}

#[derive(Clone)]
pub struct Privacidad {
    inner: Rc<Inner>,
}

type Oyentes = RefCell<Vec<Rc<dyn Fn(&Estado)>>>;

#[derive(Default)]
struct Inner {
    estado: RefCell<Estado>,
    oyentes: Oyentes,
    conexion: RefCell<Option<Conexion>>,
    pendiente: Cell<bool>,
    compartir: RefCell<Option<glib::SourceId>>,
}

impl Privacidad {
    pub fn new(hyprland: &Hyprland) -> Self {
        let p = Privacidad {
            inner: Rc::default(),
        };
        let yo = p.clone();
        hyprland.al_compartir(move |v| yo.compartiendo(v));
        p.conectar();
        p
    }

    fn compartiendo(&self, v: bool) {
        if let Some(id) = self.inner.compartir.take() {
            id.remove();
        }
        if !v {
            let mut e = self.estado();
            e.pantalla = false;
            self.publicar(e);
            return;
        }
        let weak = Rc::downgrade(&self.inner);
        let id = glib::timeout_add_local_once(COMPARTIR_MINIMO, move || {
            if let Some(inner) = weak.upgrade() {
                inner.compartir.take();
                let p = Privacidad { inner };
                let mut e = p.estado();
                e.pantalla = true;
                p.publicar(e);
            }
        });
        self.inner.compartir.replace(Some(id));
    }

    pub fn estado(&self) -> Estado {
        self.inner.estado.borrow().clone()
    }

    pub fn al_cambiar(&self, f: impl Fn(&Estado) + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    fn publicar(&self, e: Estado) {
        if *self.inner.estado.borrow() == e {
            return;
        }
        self.inner.estado.replace(e.clone());
        let oyentes = self.inner.oyentes.borrow().clone();
        for f in oyentes {
            f(&e);
        }
    }

    /// Si la conexión se cae, lo que había deja de contar y se reintenta.
    fn conectar(&self) {
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let c = match Conexion::abrir().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("morgan: privacidad: {e}");
                    yo.reintentar();
                    return;
                }
            };
            {
                let mut ctx = c.context.borrow_mut();
                let weak = Rc::downgrade(&yo.inner);
                ctx.set_subscribe_callback(Some(Box::new(move |_, _, _| {
                    if let Some(inner) = weak.upgrade() {
                        Privacidad { inner }.programar();
                    }
                })));
                let weak = Rc::downgrade(&yo.inner);
                let ctx_debil = Rc::downgrade(&c.context);
                ctx.set_state_callback(Some(Box::new(move || {
                    let caida = ctx_debil.upgrade().is_some_and(|c| {
                        matches!(
                            c.try_borrow().map(|c| c.get_state()),
                            Ok(State::Failed | State::Terminated)
                        )
                    });
                    if caida && let Some(inner) = weak.upgrade() {
                        let p = Privacidad { inner };
                        // Soltar el contexto dentro de su propio callback no: después.
                        glib::idle_add_local_once(move || {
                            p.inner.conexion.take();
                            let mut e = p.estado();
                            e.microfono.clear();
                            e.grabando = false;
                            p.publicar(e);
                            p.reintentar();
                        });
                    }
                })));
                ctx.subscribe(
                    InterestMaskSet::SOURCE_OUTPUT | InterestMaskSet::SOURCE,
                    |_| {},
                );
            }
            yo.inner.conexion.replace(Some(c));
            yo.programar();
        });
    }

    fn reintentar(&self) {
        let yo = self.clone();
        glib::timeout_add_local_once(REINTENTO, move || yo.conectar());
    }

    fn programar(&self) {
        if self.inner.pendiente.replace(true) {
            return;
        }
        let yo = self.clone();
        glib::idle_add_local_once(move || {
            yo.inner.pendiente.set(false);
            glib::MainContext::default().spawn_local(async move { yo.releer().await });
        });
    }

    async fn releer(&self) {
        let introspect = match self.inner.conexion.borrow().as_ref() {
            Some(c) => c.context.borrow().introspect(),
            None => return,
        };
        let (tx, rx) = async_channel::bounded(1);
        let mut monitores = HashSet::new();
        introspect.get_source_info_list(move |r| match r {
            ListResult::Item(s) => {
                if s.monitor_of_sink.is_some() {
                    monitores.insert(s.index);
                }
            }
            ListResult::End | ListResult::Error => {
                let _ = tx.try_send(std::mem::take(&mut monitores));
            }
        });
        let Ok(monitores) = rx.recv().await else {
            return;
        };

        let (tx, rx) = async_channel::bounded(1);
        let mut grabaciones = Vec::new();
        introspect.get_source_output_info_list(move |r| match r {
            ListResult::Item(o) => {
                let app = o
                    .proplist
                    .get_str(pulse::proplist::properties::APPLICATION_NAME)
                    .or_else(|| o.name.as_ref().map(|n| n.to_string()))
                    .unwrap_or_default();
                let p = &o.proplist;
                grabaciones.push(Grabacion {
                    fuente: o.source,
                    app,
                    pausada: o.corked,
                    binario: p
                        .get_str(pulse::proplist::properties::APPLICATION_PROCESS_BINARY)
                        .unwrap_or_default(),
                    pid: p
                        .get_str(pulse::proplist::properties::APPLICATION_PROCESS_ID)
                        .and_then(|v| v.parse().ok()),
                });
            }
            ListResult::End | ListResult::Error => {
                let _ = tx.try_send(std::mem::take(&mut grabaciones));
            }
        });
        let Ok(grabaciones) = rx.recv().await else {
            return;
        };
        let mut e = self.estado();
        e.microfono = usando_microfono(&grabaciones, &monitores);
        e.grabando = grabando(&grabaciones, argumentos);
        self.publicar(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(fuente: u32, app: &str, pausada: bool) -> Grabacion {
        Grabacion {
            fuente,
            app: app.into(),
            pausada,
            binario: String::new(),
            pid: None,
        }
    }

    #[test]
    fn grabando_sin_el_replay() {
        let gsr = |pid| Grabacion {
            binario: GRABADOR.into(),
            pid: Some(pid),
            ..g(7, "gsr-default_output", false)
        };
        let args = |pid: u32| -> Vec<String> {
            let mut v = vec!["gpu-screen-recorder", "-w", "DP-3", "-a", "default_output"];
            if pid == 2 {
                v.extend(["-r", "60"]);
            }
            v.extend(["-o", "/x"]);
            v.into_iter().map(String::from).collect()
        };
        assert!(grabando(&[gsr(1)], args));
        assert!(!grabando(&[gsr(2)], args));
        assert!(grabando(&[gsr(2), gsr(1)], args));
        assert!(!grabando(&[g(3, "Discord", false)], args));
        // Sin pid no hay cómo descartar el replay: se avisa.
        assert!(grabando(
            &[Grabacion {
                pid: None,
                ..gsr(2)
            }],
            args
        ));
        // "-r" tiene que ser un argumento entero, no parte de una ruta.
        assert!(!es_replay(&["-o", "/videos/-r/x.mp4"]));
    }

    #[test]
    fn el_monitor_no_es_el_microfono() {
        let monitores: HashSet<u32> = [7].into();
        let v = [
            g(3, "Discord", false),
            g(7, "cava", false),
            g(3, "OBS", true),
            g(4, "Discord", false),
        ];
        assert_eq!(usando_microfono(&v, &monitores), ["Discord"]);
        assert!(usando_microfono(&[g(7, "cava", false)], &monitores).is_empty());
    }

    #[test]
    fn texto_del_tooltip() {
        assert_eq!(tooltip(&["Discord".into()], false), "Micrófono: Discord");
        assert_eq!(
            tooltip(&["A".into(), "B".into()], true),
            "Micrófono: A, B\nPantalla compartida"
        );
        assert_eq!(tooltip(&[], true), "Pantalla compartida");
        assert_eq!(tooltip(&[], false), "");
    }
}
