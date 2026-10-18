//! Binario aparte de morgan: si morgan se cae, el bloqueo sigue.
//!
//!   morgan-lock [--pam SERVICIO] [--sin-logind]
//!
//! `--pam vlock` y `--sin-logind` son para probarlo en un Hyprland anidado: un fallo no suma
//! a faillock y no se toca el `LockedHint` de la sesión real.

mod caracteres;
mod fondo;
mod lienzo;
mod logind;
mod pam;
mod vista;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gtk::{gdk, glib};
use gtk4_session_lock as lock;

use morgan_ipc::bloqueo::Bloqueo;

use crate::logind::Logind;
use crate::pam::{Resultado, Secreto};
use crate::vista::{Campo, Vista};

struct Opciones {
    servicio: String,
    logind: bool,
}

fn opciones() -> Result<Opciones, String> {
    let mut o = Opciones {
        servicio: "morgan-lock".into(),
        logind: true,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--pam" => o.servicio = args.next().ok_or("--pam necesita un servicio")?,
            "--sin-logind" => o.logind = false,
            otro => return Err(format!("argumento desconocido: {otro}")),
        }
    }
    Ok(o)
}

fn main() -> ExitCode {
    let o = match opciones() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("morgan-lock: {e}\nuso: morgan-lock [--pam SERVICIO] [--sin-logind]");
            return ExitCode::FAILURE;
        }
    };
    // Antes de pedir el bloqueo: sin servicio PAM no habría forma de salir.
    if !pam::servicio_existe(&o.servicio) {
        eprintln!(
            "morgan-lock: no existe el servicio PAM «{}» (/etc/pam.d): no se bloquea",
            o.servicio
        );
        return ExitCode::FAILURE;
    }
    let Some(usuario) = pam::usuario() else {
        eprintln!("morgan-lock: no se encontró el usuario de este proceso");
        return ExitCode::FAILURE;
    };
    if let Err(e) = gtk::init() {
        eprintln!("morgan-lock: {e}");
        return ExitCode::FAILURE;
    }
    if !lock::is_supported() {
        eprintln!("morgan-lock: el compositor no tiene ext-session-lock");
        return ExitCode::FAILURE;
    }
    let display = gdk::Display::default().expect("display de gtk::init");
    let paleta = lienzo::Paleta::desde_css(&cargar_estilo(&display));

    let fondo = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .map(|d| d.join("wallpaper-lock.png"))
        .and_then(|p| gdk::Texture::from_filename(p).ok());
    let vista = Vista::new(&display, fondo, paleta, estilo());
    let instancia = lock::Instance::new();
    let bucle = glib::MainLoop::new(None, false);
    let salida = Rc::new(Cell::new(ExitCode::SUCCESS));
    let logind: Rc<RefCell<Option<Rc<Logind>>>> = Rc::default();

    let v = vista.clone();
    instancia.connect_monitor(move |inst, monitor| {
        inst.assign_window_to_monitor(&v.ventana(), monitor);
    });
    let (b, s) = (bucle.clone(), salida.clone());
    instancia.connect_failed(move |_| {
        eprintln!("morgan-lock: no se pudo bloquear (¿otro bloqueo activo?)");
        s.set(ExitCode::FAILURE);
        b.quit();
    });
    let l = logind.clone();
    instancia.connect_locked(move |_| {
        if let Some(l) = l.borrow().clone() {
            marcar(l, true);
        }
    });
    let (l, b) = (logind.clone(), bucle.clone());
    instancia.connect_unlocked(move |_| {
        let l = l.borrow().clone();
        let b = b.clone();
        glib::spawn_future_local(async move {
            if let Some(l) = l
                && let Err(e) = l.marcar_bloqueada(false).await
            {
                eprintln!("morgan-lock: logind: {e}");
            }
            b.quit();
        });
    });

    verificar(&vista, &instancia, o.servicio, usuario);
    // SIGUSR1 desbloquea: quien puede mandarla ya puede `loginctl unlock-session`.
    let (v, inst) = (vista.clone(), instancia.clone());
    glib_unix::unix_signal_add_local(10, move || {
        desbloquear(&v, &inst);
        glib::ControlFlow::Continue
    });
    let temporizador = Temporizador::default();
    reloj(&vista, &temporizador);

    if !instancia.lock() {
        eprintln!("morgan-lock: no se pudo pedir el bloqueo");
        return ExitCode::FAILURE;
    }
    if o.logind {
        conectar_logind(&logind, &instancia, &vista, &temporizador);
    }
    bucle.run();
    salida.get()
}

fn cargar_estilo(display: &gdk::Display) -> String {
    let colores = config_dir()
        .map(|d| d.join("morgan/theme.css"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let provider = gtk::CssProvider::new();
    provider.connect_parsing_error(|_, section, error| {
        eprintln!("morgan-lock: estilo: {section}: {error}");
    });
    provider.load_from_string(&format!("{colores}\n{}", include_str!("estilo.css")));
    gtk::style_context_add_provider_for_display(
        display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    colores
}

/// `[lock]` de la config de morgan. Cualquier problema (sin archivo, TOML roto, un valor
/// desconocido) deja los defaults y bloquea igual: la config no puede impedir el bloqueo.
fn estilo() -> Bloqueo {
    let Some(ruta) = config_dir().map(|d| d.join("morgan/config.toml")) else {
        return Bloqueo::default();
    };
    let Ok(texto) = std::fs::read_to_string(&ruta) else {
        return Bloqueo::default();
    };
    match leer_estilo(&texto) {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "morgan-lock: {}: [lock]: {e}; con los valores de fábrica",
                ruta.display()
            );
            Bloqueo::default()
        }
    }
}

fn leer_estilo(texto: &str) -> Result<Bloqueo, String> {
    let tabla: toml::Table = texto.parse().map_err(|e: toml::de::Error| e.to_string())?;
    match tabla.get("lock") {
        Some(v) => v
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| e.to_string()),
        None => Ok(Bloqueo::default()),
    }
}

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

fn verificar(vista: &Vista, instancia: &lock::Instance, servicio: String, usuario: String) {
    let ocupado = Rc::new(Cell::new(false));
    let fallos = Rc::new(Cell::new(0u32));
    let v = vista.clone();
    let inst = instancia.clone();
    vista.al_enviar(move |texto| {
        if ocupado.replace(true) {
            return;
        }
        v.campo(Campo::Verificando);
        let clave = Secreto::new(texto);
        let (tx, rx) = async_channel::bounded(1);
        let (s, u) = (servicio.clone(), usuario.clone());
        std::thread::spawn(move || {
            let _ = tx.send_blocking(pam::autenticar(&s, &u, clave));
        });
        let (v, inst, ocupado, fallos) = (v.clone(), inst.clone(), ocupado.clone(), fallos.clone());
        glib::spawn_future_local(async move {
            let r = rx
                .recv()
                .await
                .unwrap_or_else(|_| Resultado::Error("el hilo de PAM no respondió".into()));
            ocupado.set(false);
            match r {
                Resultado::Bien => desbloquear(&v, &inst),
                Resultado::Fallo(mensajes) => {
                    fallos.set(fallos.get() + 1);
                    v.mensaje(mensajes.into_iter().next());
                    v.campo(Campo::Fallo(fallos.get()));
                }
                Resultado::Error(e) => {
                    eprintln!("morgan-lock: {e}");
                    fallos.set(fallos.get() + 1);
                    v.campo(Campo::Fallo(fallos.get()));
                }
            }
        });
    });
}

fn desbloquear(vista: &Vista, instancia: &lock::Instance) {
    let inst = instancia.clone();
    vista.salir(move || inst.unlock());
}

type Temporizador = Rc<RefCell<Option<glib::SourceId>>>;

/// El reloj cambia al cambiar el minuto: un temporizador al próximo, no un sondeo. Llamarlo
/// de nuevo (al despertar: el monotónico no corre suspendido) reemplaza al pendiente.
fn reloj(vista: &Vista, t: &Temporizador) {
    vista.refrescar_reloj();
    let falta = glib::DateTime::now_local()
        .map(|t| 60_000 - (t.second() as u64 * 1000 + t.microsecond() as u64 / 1000))
        .unwrap_or(60_000);
    let (v, t2) = (vista.clone(), t.clone());
    let id = glib::timeout_add_local_once(Duration::from_millis(falta.max(1)), move || {
        t2.borrow_mut().take();
        reloj(&v, &t2);
    });
    if let Some(viejo) = t.borrow_mut().replace(id) {
        viejo.remove();
    }
}

fn conectar_logind(
    celda: &Rc<RefCell<Option<Rc<Logind>>>>,
    instancia: &lock::Instance,
    vista: &Vista,
    temporizador: &Temporizador,
) {
    let (celda, inst, v, t) = (
        celda.clone(),
        instancia.clone(),
        vista.clone(),
        temporizador.clone(),
    );
    glib::spawn_future_local(async move {
        let l = match Logind::conectar().await {
            Ok(l) => Rc::new(l),
            Err(e) => {
                eprintln!("morgan-lock: logind: {e}");
                return;
            }
        };
        *celda.borrow_mut() = Some(l.clone());
        // Si `locked` llegó antes que la conexión, se marca ahora.
        if inst.is_locked() {
            marcar(l.clone(), true);
        }
        let (l2, inst2, v2) = (l.clone(), inst.clone(), v.clone());
        glib::spawn_future_local(async move {
            if l2.esperar_unlock().await.is_ok() {
                desbloquear(&v2, &inst2);
            }
        });
        if let Err(e) = l.al_despertar(|| reloj(&v, &t)).await {
            eprintln!("morgan-lock: logind: {e}");
        }
    });
}

fn marcar(l: Rc<Logind>, si: bool) {
    glib::spawn_future_local(async move {
        if let Err(e) = l.marcar_bloqueada(si).await {
            eprintln!("morgan-lock: logind: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use morgan_ipc::bloqueo::Escritura;

    #[test]
    fn el_estilo_sale_de_su_seccion_y_lo_roto_no_impide_leer() {
        let b =
            leer_estilo("[shell]\nopacity = 0.9\n[lock]\ntyping = \"dots\"\n").unwrap();
        assert_eq!(b.escritura, Escritura::Puntos);
        assert!(b.chispa);
        assert_eq!(leer_estilo("[shell]\n").unwrap(), Bloqueo::default());
        assert!(leer_estilo("[lock]\ntyping = \"nada\"\n").is_err());
        assert!(leer_estilo("no es toml [").is_err());
    }
}
