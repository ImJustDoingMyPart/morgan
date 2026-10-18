
use std::sync::OnceLock;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};

pub async fn salida(args: &[&str]) -> Result<String, String> {
    salidas(&[args])
        .await
        .pop()
        .unwrap_or_else(|| Err(format!("{}: sin resultado", args[0])))
}

/// Todo en otro hilo: lanzar un proceso desde el principal (`gio::Subprocess`) cuesta ~1,5 ms
/// de fork, y el menú lanza decenas al abrirse. `std::process` además lanza con
/// `posix_spawn`, más barato.
pub async fn salidas(lista: &[&[&str]]) -> Vec<Result<String, String>> {
    let lista: Vec<Vec<String>> = lista
        .iter()
        .map(|a| a.iter().map(|s| (*s).to_owned()).collect())
        .collect();
    let n = lista.len();
    gio::spawn_blocking(move || correr(&lista))
        .await
        .unwrap_or_else(|_| vec![Err("comando: el hilo se cayó".into()); n])
}

fn correr(lista: &[Vec<String>]) -> Vec<Result<String, String>> {
    let hijos: Vec<_> = lista
        .iter()
        .map(|a| {
            std::process::Command::new(&a[0])
                .args(&a[1..])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| format!("{}: {e}", a[0]))
        })
        .collect();
    hijos
        .into_iter()
        .zip(lista)
        .map(|(hijo, a)| {
            let fin = hijo?
                .wait_with_output()
                .map_err(|e| format!("{}: {e}", a[0]))?;
            if !fin.status.success() {
                return Err(format!("{}: terminó con error", a[0]));
            }
            Ok(String::from_utf8_lossy(&fin.stdout).trim_end().to_owned())
        })
        .collect()
}

/// Un motor colgado (la red, en `qalc`) no deja procesos acumulados.
pub async fn salida_cortable(
    args: &[&str],
    tope: Duration,
    cancelar: &gio::Cancellable,
) -> Result<String, String> {
    let p = con_salida(args)?;
    let fin = async { Some(p.communicate_utf8_future(None).await) };
    let corte = async {
        futures_lite::future::or(glib::timeout_future(tope), cancelar.future()).await;
        None
    };
    match futures_lite::future::or(fin, corte).await {
        Some(r) => leer(args, &p, r),
        None => {
            p.force_exit();
            Err(format!("{}: cortado", args[0]))
        }
    }
}

fn con_salida(args: &[&str]) -> Result<gio::Subprocess, String> {
    let argv: Vec<&std::ffi::OsStr> = args.iter().map(|a| std::ffi::OsStr::new(*a)).collect();
    gio::Subprocess::newv(
        &argv,
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE,
    )
    .map_err(|e| format!("{}: {e}", args[0]))
}

fn leer(
    args: &[&str],
    p: &gio::Subprocess,
    r: Result<(Option<glib::GString>, Option<glib::GString>), glib::Error>,
) -> Result<String, String> {
    let (out, _) = r.map_err(|e| format!("{}: {e}", args[0]))?;
    if !p.is_successful() {
        return Err(format!("{}: terminó con error", args[0]));
    }
    Ok(out.map(|s| s.trim_end().to_owned()).unwrap_or_default())
}

static UWSM: OnceLock<bool> = OnceLock::new();

/// uwsm es Python y tarda ~100 ms: `main` lo pregunta al arrancar, en otro hilo.
pub fn con_uwsm() -> bool {
    *UWSM.get_or_init(|| {
        gtk::glib::find_program_in_path("uwsm").is_some()
            && std::process::Command::new("uwsm")
                .args(["check", "is-active"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
    })
}

/// Fuera del cgroup de `morgan.service`: lanzada como hija moriría con un reinicio de morgan.
/// Con uwsm, `uwsm app`; si no, un scope propio de systemd con `gio launch`.
pub fn lanzar_app(id: &str, archivos: &[&str]) {
    let desktop;
    let mut args: Vec<&str> = if con_uwsm() {
        vec!["uwsm", "app", "--", id]
    } else {
        let Some(ruta) = gio_unix::DesktopAppInfo::new(id).and_then(|d| d.filename()) else {
            eprintln!("morgan: {id}: no está el .desktop");
            return;
        };
        desktop = ruta.to_string_lossy().into_owned();
        vec!["systemd-run", "--user", "--scope", "--collect", "--quiet", "--", "gio", "launch", &desktop]
    };
    args.extend_from_slice(archivos);
    lanzar(&args);
}

/// `launch_default_for_uri` la lanzaría como hija y moriría con un reinicio de morgan.
pub fn abrir(ruta: &std::path::Path, otro_tipo: &str) -> Result<(), String> {
    let tipo = if ruta.is_dir() {
        "inode/directory".into()
    } else {
        gio::content_type_guess(Some(ruta), None).0
    };
    let app = gio::AppInfo::default_for_type(&tipo, false)
        .or_else(|| gio::AppInfo::default_for_type(otro_tipo, false))
        .ok_or_else(|| format!("no hay app predeterminada para {tipo}"))?;
    let id = app.id().ok_or("la app predeterminada no tiene .desktop")?;
    lanzar_app(&id, &[&ruta.to_string_lossy()]);
    Ok(())
}

/// Con uwsm, `uwsm stop` (su doc pide que la termine él).
pub fn salir_de_la_sesion() -> &'static [&'static str] {
    if con_uwsm() {
        &["uwsm", "stop"]
    } else {
        &["hyprctl", "eval", "hl.dispatch(hl.dsp.exit())"]
    }
}

pub fn lanzar(args: &[&str]) {
    let argv: Vec<&std::ffi::OsStr> = args.iter().map(|a| std::ffi::OsStr::new(*a)).collect();
    match gio::Subprocess::newv(&argv, gio::SubprocessFlags::NONE) {
        Ok(p) => p.wait_async(gio::Cancellable::NONE, |_| {}),
        Err(e) => eprintln!("morgan: {}: {e}", args[0]),
    }
}

pub fn lanzar_con_entrada(args: &[&str], entrada: &[u8]) {
    let argv: Vec<&std::ffi::OsStr> = args.iter().map(|a| std::ffi::OsStr::new(*a)).collect();
    let p = match gio::Subprocess::newv(&argv, gio::SubprocessFlags::STDIN_PIPE) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("morgan: {}: {e}", args[0]);
            return;
        }
    };
    let nombre = args[0].to_owned();
    let datos = gtk::glib::Bytes::from(entrada);
    gtk::glib::MainContext::default().spawn_local(async move {
        if let Err(e) = p.communicate_future(Some(&datos)).await {
            eprintln!("morgan: {nombre}: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn corre_en_orden_y_separa_los_errores() {
        let r = correr(&[
            v(&["sh", "-c", "sleep 0.05; printf 'lento\\n\\n'"]),
            v(&["printf", "rápido"]),
            v(&["false"]),
            v(&["morgan-no-existe"]),
            v(&["sh", "-c", "echo afuera >&2"]),
        ]);
        assert_eq!(r[0], Ok("lento".into()));
        assert_eq!(r[1], Ok("rápido".into()));
        assert_eq!(r[2], Err("false: terminó con error".into()));
        assert!(
            r[3].as_ref()
                .is_err_and(|e| e.starts_with("morgan-no-existe: "))
        );
        assert_eq!(r[4], Ok("".into()));
    }
}
