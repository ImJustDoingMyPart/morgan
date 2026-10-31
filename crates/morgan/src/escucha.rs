//! «Escucharme» (la lista del micrófono del centro): lo que entra por el micrófono sale por la
//! salida, con `pw-loopback`. Sin destino fijo: WirePlumber lo lleva a la entrada y a la salida
//! predeterminadas y lo mueve si cambia la entrada.
//!
//! Se apaga solo si cambia la salida, otra predeterminada u otro puerto de la misma (de los
//! auriculares a los parlantes vendría el acople), y nunca arranca encendida: no se guarda. Es
//! hija de morgan y muere con él (el cgroup del servicio y `PR_SET_PDEATHSIG`), así que cerrar
//! sesión también la apaga. Mientras suena, el indicador de privacidad la cuenta: el micrófono
//! está en uso de verdad.

use std::cell::{Cell, RefCell};
use std::os::unix::process::CommandExt;
use std::rc::Rc;

use libpulse_binding as pulse;
use pulse::context::subscribe::InterestMaskSet;

use crate::audio::Conexion;

/// El nombre de sus nodos: `pw-loopback` les pone `input.` y `output.` adelante.
pub const NODO: &str = "morgan-escucha";

/// La salida que oye: la predeterminada y su puerto.
pub type Salida = (String, Option<String>);

/// Los dispositivos de una prueba (`MORGAN_ESCUCHA_PRUEBA=fuente,salida`). En el anidado
/// (`MORGAN_DEV`) son obligatorios: sin ellos no se oye ni se mide nada, porque se usarían el
/// micrófono y la salida de verdad.
pub fn destinos() -> Result<Option<(String, String)>, &'static str> {
    destinos_de(
        std::env::var("MORGAN_ESCUCHA_PRUEBA").ok(),
        std::env::var_os("MORGAN_DEV").is_some(),
    )
}

fn destinos_de(v: Option<String>, dev: bool) -> Result<Option<(String, String)>, &'static str> {
    match v.as_deref().and_then(|v| v.split_once(',')) {
        Some((f, s)) => Ok(Some((f.to_owned(), s.to_owned()))),
        None if dev => Err("MORGAN_DEV sin MORGAN_ESCUCHA_PRUEBA=fuente,salida"),
        None => Ok(None),
    }
}

/// Propiedades de los streams; en una prueba, sin caer al predeterminado si falta el destino.
pub fn propiedades(nombre_app: &str, prueba: bool) -> String {
    let mut p =
        format!("application.name=\"{nombre_app}\" application.icon_name=audio-input-microphone");
    if prueba {
        p.push_str(" node.dont-fallback=true");
    }
    p
}

pub fn argumentos(nombre_app: &str, destinos: Option<&(String, String)>) -> Vec<String> {
    let props = propiedades(nombre_app, destinos.is_some());
    let mut v = vec![
        "-n".to_owned(),
        NODO.to_owned(),
        "-i".to_owned(),
        props.clone(),
        "-o".to_owned(),
        props,
    ];
    if let Some((fuente, salida)) = destinos {
        v.extend([
            "-C".to_owned(),
            fuente.clone(),
            "-P".to_owned(),
            salida.clone(),
        ]);
    }
    v
}

/// La primera lectura fija la salida; después, cualquier diferencia apaga.
pub fn cambio(inicial: Option<&Salida>, ahora: &Salida) -> bool {
    inicial.is_some_and(|i| i != ahora)
}

pub fn aviso(activa: bool) -> crate::osd::Aviso {
    let texto = if activa { "Activado" } else { "Desactivado" };
    crate::osd::Aviso::estado("󰋋", tr!("Escucharme"), texto, activa, 2000)
}

#[derive(Clone, Default)]
pub struct Escucha {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    hija: RefCell<Option<Hija>>,
    vigia: RefCell<Option<Conexion>>,
    salida: RefCell<Option<Salida>>,
    pendiente: Cell<bool>,
    /// Sube con cada encendido: una lectura o una salida tardía no apaga la de ahora.
    vuelta: Cell<u64>,
    oyentes: RefCell<Vec<Rc<dyn Fn()>>>,
}

/// `pw-loopback` corriendo; la espera glib (`vigilada`) hasta que se apaga.
struct Hija {
    pid: libc::pid_t,
    /// `None`: ya terminó y glib la esperó.
    vigilada: Option<glib::SourceId>,
}

impl Drop for Hija {
    fn drop(&mut self) {
        let Some(v) = self.vigilada.take() else {
            return;
        };
        v.remove();
        // glib ya no la espera: la espera otro hilo, sin bloquear (sale enseguida).
        let pid = self.pid;
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        std::thread::spawn(move || unsafe {
            libc::waitpid(pid, std::ptr::null_mut(), 0);
        });
    }
}

impl Escucha {
    pub fn activa(&self) -> bool {
        self.inner.hija.borrow().is_some()
    }

    pub fn al_cambiar(&self, f: impl Fn() + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    fn avisar(&self) {
        let oyentes = self.inner.oyentes.borrow().clone();
        for f in oyentes {
            f();
        }
    }

    pub fn alternar(&self) {
        if self.activa() {
            self.apagar();
        } else {
            self.encender();
        }
    }

    fn encender(&self) {
        let i = &self.inner;
        let vuelta = i.vuelta.get() + 1;
        i.vuelta.set(vuelta);
        let destinos = match destinos() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("morgan: escucha: {e}");
                return;
            }
        };
        let args = argumentos(tr!("Escucharme"), destinos.as_ref());
        let mut cmd = std::process::Command::new("pw-loopback");
        cmd.args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null());
        // Si morgan muere sin pasar por Drop, pw-loopback se va con él.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let pid = match cmd.spawn() {
            Ok(h) => h.id() as libc::pid_t,
            Err(e) => {
                eprintln!("morgan: escucha: pw-loopback: {e}");
                return;
            }
        };
        let weak = Rc::downgrade(&self.inner);
        let vigilada = glib::child_watch_add_local(glib::Pid(pid), move |_, estado| {
            let Some(inner) = weak.upgrade() else { return };
            if inner.vuelta.get() != vuelta {
                return;
            }
            eprintln!("morgan: escucha: pw-loopback terminó ({estado})");
            if let Some(mut h) = inner.hija.take() {
                h.vigilada = None;
            }
            let yo = Escucha { inner };
            yo.soltar_vigia();
            yo.avisar();
        });
        i.hija.replace(Some(Hija {
            pid,
            vigilada: Some(vigilada),
        }));
        self.vigilar(vuelta);
        self.avisar();
    }

    fn apagar(&self) {
        let i = &self.inner;
        i.vuelta.set(i.vuelta.get() + 1);
        if i.hija.take().is_none() {
            return;
        }
        self.soltar_vigia();
        self.avisar();
    }

    fn soltar_vigia(&self) {
        let i = &self.inner;
        i.salida.replace(None);
        if let Some(c) = i.vigia.take() {
            c.context.borrow_mut().set_subscribe_callback(None);
            c.context.borrow_mut().disconnect();
        }
    }

    /// Una conexión de pulse propia, solo mientras está encendida: la del mezclador vive con el
    /// centro abierto.
    fn vigilar(&self, vuelta: u64) {
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let c = match Conexion::abrir().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("morgan: escucha: {e}");
                    return;
                }
            };
            if yo.inner.vuelta.get() != vuelta {
                return;
            }
            let weak = Rc::downgrade(&yo.inner);
            {
                let mut ctx = c.context.borrow_mut();
                ctx.set_subscribe_callback(Some(Box::new(move |_, _, _| {
                    if let Some(inner) = weak.upgrade() {
                        Escucha { inner }.programar(vuelta);
                    }
                })));
                ctx.subscribe(InterestMaskSet::SINK | InterestMaskSet::SERVER, |_| {});
            }
            yo.inner.vigia.replace(Some(c));
            yo.programar(vuelta);
        });
    }

    fn programar(&self, vuelta: u64) {
        if self.inner.pendiente.replace(true) {
            return;
        }
        let yo = self.clone();
        glib::idle_add_local_once(move || {
            yo.inner.pendiente.set(false);
            glib::MainContext::default().spawn_local(async move { yo.releer(vuelta).await });
        });
    }

    async fn releer(&self, vuelta: u64) {
        let introspect = match self.inner.vigia.borrow().as_ref() {
            Some(c) => c.context.borrow().introspect(),
            None => return,
        };
        let (tx, rx) = async_channel::bounded(1);
        introspect.get_server_info(move |i| {
            let _ = tx.try_send(i.default_sink_name.as_ref().map(|n| n.to_string()));
        });
        let Ok(Some(nombre)) = rx.recv().await else {
            return;
        };
        let (tx, rx) = async_channel::unbounded();
        introspect.get_sink_info_by_name(&nombre, move |r| {
            if let pulse::callbacks::ListResult::Item(s) = r {
                let _ = tx.try_send(
                    s.active_port
                        .as_ref()
                        .and_then(|p| p.name.as_deref().map(str::to_owned)),
                );
            }
        });
        let puerto = rx.recv().await.unwrap_or_default();
        if self.inner.vuelta.get() != vuelta {
            return;
        }
        let ahora = (nombre, puerto);
        if cambio(self.inner.salida.borrow().as_ref(), &ahora) {
            self.apagar();
            return;
        }
        self.inner.salida.replace(Some(ahora));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_apaga_solo_si_cambia_la_salida() {
        let s = |n: &str, p: Option<&str>| (n.to_owned(), p.map(str::to_owned));
        let auris = s("alsa_output.pci", Some("analog-output-headphones"));
        // La primera lectura no apaga: fija la de ahora.
        assert!(!cambio(None, &auris));
        assert!(!cambio(Some(&auris), &auris));
        // De los auriculares a los parlantes de la misma placa: otro puerto.
        assert!(cambio(
            Some(&auris),
            &s("alsa_output.pci", Some("analog-output-speaker"))
        ));
        // Otra salida predeterminada.
        assert!(cambio(Some(&auris), &s("alsa_output.usb", None)));
    }

    #[test]
    fn en_el_anidado_nunca_el_microfono_real() {
        assert_eq!(destinos_de(None, false), Ok(None));
        assert!(destinos_de(None, true).is_err());
        assert!(destinos_de(Some("sin coma".into()), true).is_err());
        assert_eq!(
            destinos_de(Some("mic,sal".into()), true),
            Ok(Some(("mic".into(), "sal".into())))
        );
        assert!(propiedades("x", true).contains("node.dont-fallback=true"));
        assert!(!propiedades("x", false).contains("dont-fallback"));
    }

    #[test]
    fn sin_destino_fijo_salvo_en_la_prueba() {
        let a = argumentos("Escucharme", None);
        assert!(!a.iter().any(|x| x == "-C" || x == "-P"));
        assert_eq!(a[..2], ["-n", NODO]);
        let d = ("mic".to_owned(), "sal".to_owned());
        let a = argumentos("Escucharme", Some(&d));
        assert!(a.windows(2).any(|w| w == ["-C", "mic"]));
        assert!(a.windows(2).any(|w| w == ["-P", "sal"]));
    }
}
