//! `morgan-greet install` y `restore`: poner el greeter en greetd y sacarlo. Como el de
//! Mandara: primero el plan, y nada cambia sin `--apply`; la config de greetd se respalda con
//! fecha antes de tocarla y `restore` vuelve al último respaldo. Nunca reinicia greetd: vale
//! desde el próximo arranque, y la sesión de ahora sigue.
//!
//! Qué hace (como root):
//! 1. Copia el binario a `/usr/lib/morgan/morgan-greet`: el usuario `greeter` no ejecuta nada
//!    de un home. Con el paquete (ya en `/usr`), usa ese.
//! 2. Crea `/var/lib/morgan-greet` (del usuario `greeter`, para el recuerdo) y adentro
//!    `usuarios/` con sticky bit, donde morgan deja las copias de cada cuenta.
//! 3. En `/etc/greetd/config.toml`, el `command` de `[default_session]` pasa a ser el greeter
//!    y se saca `[initial_session]` (el inicio automático; Roberto, 2026-11-05). El resto del
//!    archivo, comentarios incluidos, queda igual.
//! 4. Si está `pam_gnome_keyring`, lo suma a `/etc/pam.d/greetd` (respaldado junto a la config de greetd): la clave
//!    del greeter abre el llavero al entrar (Roberto, 2026-11-07). Sin inicio automático,
//!    nadie más lo abría.

use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, value};

pub const BINARIO: &str = "usr/lib/morgan/morgan-greet";
pub const ESTADO: &str = "var/lib/morgan-greet";
pub const GREETD: &str = "etc/greetd/config.toml";
pub const PAM: &str = "etc/pam.d/greetd";
pub const PAM_LLAVERO: &str = "usr/lib/security/pam_gnome_keyring.so";
const PREFIJO_RESPALDO: &str = "config.toml.morgan-";
/// Junto a la config de greetd, no en `/etc/pam.d`: ahí cada archivo es un servicio de PAM.
const PREFIJO_RESPALDO_PAM: &str = "pam.d-greetd.morgan-";

/// Los compositores que el greeter sabe lanzar, en orden de preferencia, con el programa que
/// tiene que estar. Con varios, Hyprland (Roberto, 2026-11-06); otro, con `--compositor`.
pub const COMPOSITORES: &[(&str, &str)] = &[("hyprland", "start-hyprland"), ("umbriel", "umbriel")];

/// El primero de `COMPOSITORES` cuyo programa está.
pub fn instalado(esta: impl Fn(&str) -> bool) -> Option<&'static str> {
    COMPOSITORES
        .iter()
        .find(|(_, prog)| esta(prog))
        .map(|(n, _)| *n)
}

/// El `command` de greetd: el greeter con su compositor fijo, así no depende de qué haya
/// instalado al arrancar.
pub fn comando(binario: &str, compositor: &str) -> String {
    format!("{binario} --compositor {compositor}")
}

/// Si el `command` de `[default_session]` ya es el greeter (con cualquier compositor): cambiarlo
/// no lleva respaldo, así `restore` sigue volviendo a la config de antes del greeter.
pub fn tiene_greeter(actual: &str, binario: &str) -> bool {
    actual
        .parse::<DocumentMut>()
        .ok()
        .and_then(|d| {
            d.get("default_session")?
                .get("command")?
                .as_str()
                .map(str::to_owned)
        })
        .is_some_and(|c| c == binario || c.starts_with(&format!("{binario} ")))
}

/// Si greetd no está: el greeter corre sobre él. `None` si está (su config existe).
pub fn falta_greetd(raiz: &Path) -> Option<String> {
    (!raiz.join(GREETD).is_file()).then(|| {
        "greetd no está instalado: el greeter corre sobre él. Instalalo (en Arch, `sudo pacman -S greetd`) y volvé a correr esto.".to_owned()
    })
}

/// El gestor de inicio que arranca la máquina, por el alias `display-manager.service` que
/// `systemctl enable` deja en `/etc/systemd/system`: `None` si es greetd.
pub fn aviso_gestor(raiz: &Path) -> Option<String> {
    let alias = raiz.join("etc/systemd/system/display-manager.service");
    let destino = std::fs::read_link(&alias).ok();
    let nombre = destino
        .as_deref()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(".service"))
        .map(str::to_owned);
    match nombre.as_deref() {
        Some("greetd") => None,
        Some(otro) => Some(format!(
            "Ojo: el inicio de sesión lo maneja {otro}, no greetd. Para ver el greeter: sudo systemctl disable {otro} && sudo systemctl enable greetd"
        )),
        None => Some(
            "Ojo: greetd no está habilitado. Para ver el greeter: sudo systemctl enable greetd"
                .to_owned(),
        ),
    }
}

/// La config de greetd con el greeter: `command` del greeter y sin inicio automático.
pub fn config_nueva(actual: &str, command: &str) -> Result<String, String> {
    let mut doc: DocumentMut = actual
        .parse()
        .map_err(|e| format!("{GREETD} no es TOML válido: {e}"))?;
    let sesion = doc
        .get_mut("default_session")
        .and_then(|t| t.as_table_mut())
        .ok_or_else(|| format!("{GREETD} no tiene [default_session]"))?;
    sesion["command"] = value(command);
    if !sesion.contains_key("user") {
        sesion["user"] = value("greeter");
    }
    doc.remove("initial_session");
    Ok(doc.to_string())
}

/// `/etc/pam.d/greetd` con el llavero: `auth` guarda la clave y `session` abre el llavero con
/// ella (`pam_gnome_keyring(8)`), cada una después de las de su tipo. `None` si ya está.
pub fn pam_nuevo(actual: &str) -> Option<String> {
    if actual.contains("pam_gnome_keyring") {
        return None;
    }
    let tipo = |l: &str| l.split_whitespace().next().map(str::to_owned);
    let mut v: Vec<String> = actual.lines().map(str::to_owned).collect();
    for (t, linea) in [
        ("auth", "auth       optional     pam_gnome_keyring.so"),
        (
            "session",
            "session    optional     pam_gnome_keyring.so auto_start",
        ),
    ] {
        let i = v
            .iter()
            .rposition(|l| tipo(l).as_deref() == Some(t))
            .map_or(v.len(), |i| i + 1);
        v.insert(i, linea.to_owned());
    }
    let mut s = v.join("\n");
    s.push('\n');
    Some(s)
}

/// El respaldo más nuevo de `dir` (el nombre lleva la fecha, así que ordena solo) que no
/// tiene el greeter: con `install` corrido dos veces, el segundo respaldo ya lo trae, y volver
/// a ese no sería volver.
pub fn ultimo_respaldo(dir: &Path) -> Option<PathBuf> {
    let mut v = respaldos(dir, PREFIJO_RESPALDO);
    while let Some(p) = v.pop() {
        if std::fs::read_to_string(&p).is_ok_and(|t| !t.contains("morgan-greet")) {
            return Some(p);
        }
    }
    None
}

/// El respaldo más nuevo de `/etc/pam.d/greetd` (en `dir`, el de la config de greetd).
pub fn ultimo_respaldo_pam(dir: &Path) -> Option<PathBuf> {
    ultimo_con(dir, PREFIJO_RESPALDO_PAM)
}

fn ultimo_con(dir: &Path, prefijo: &str) -> Option<PathBuf> {
    respaldos(dir, prefijo).pop()
}

fn respaldos(dir: &Path, prefijo: &str) -> Vec<PathBuf> {
    let Ok(d) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = d
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefijo))
        })
        .collect();
    v.sort();
    v
}

pub fn nombre_respaldo(fecha: &str) -> String {
    format!("{PREFIJO_RESPALDO}{fecha}")
}

pub fn nombre_respaldo_pam(fecha: &str) -> String {
    format!("{PREFIJO_RESPALDO_PAM}{fecha}")
}

/// La fecha local para el respaldo: `20261106-174512`.
pub fn fecha() -> String {
    let mut ts = 0 as libc::time_t;
    // SAFETY: punteros a locales del tamaño que piden.
    let tm = unsafe {
        libc::time(&mut ts);
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&ts, &mut tm);
        tm
    };
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

/// logind abre terminales de texto a pedido (`NAutoVTs`, 6 si nadie lo cambió): con 0, si el
/// greeter no arranca no queda Ctrl+Alt+F2 para entrar.
pub fn hay_tty_de_emergencia(logind_conf: &str) -> bool {
    !logind_conf.lines().any(|l| {
        let l = l.trim();
        l.strip_prefix("NAutoVTs")
            .and_then(|r| r.trim_start().strip_prefix('='))
            .is_some_and(|v| v.trim() == "0")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greetd_instalado_y_habilitado() {
        let raiz = std::env::temp_dir().join(format!("morgan-greet-gestor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let sys = raiz.join("etc/systemd/system");
        std::fs::create_dir_all(&sys).unwrap();
        assert!(falta_greetd(&raiz).unwrap().contains("pacman -S greetd"));
        assert!(
            aviso_gestor(&raiz)
                .unwrap()
                .contains("systemctl enable greetd")
        );
        let alias = sys.join("display-manager.service");
        std::os::unix::fs::symlink("/usr/lib/systemd/system/sddm.service", &alias).unwrap();
        assert!(aviso_gestor(&raiz).unwrap().contains("disable sddm"));
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink("/usr/lib/systemd/system/greetd.service", &alias).unwrap();
        assert_eq!(aviso_gestor(&raiz), None);
        std::fs::create_dir_all(raiz.join("etc/greetd")).unwrap();
        std::fs::write(raiz.join(GREETD), "").unwrap();
        assert_eq!(falta_greetd(&raiz), None);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    const ESTA_MAQUINA: &str = r#"[terminal]
# The VT to run the greeter on. Can be "next", "current" or a number
# designating the VT.
vt = 1
# Autologin.
[initial_session]
command = "uwsm start hyprland.desktop"
user = "anon"

# The default session, also known as the greeter.
[default_session]

# `agreety` is the bundled agetty/login-lookalike.
command = "agreety --cmd 'uwsm start hyprland.desktop'"

# The user to run the command as.
user = "greeter"
"#;

    #[test]
    fn cambia_el_command_saca_el_inicio_automatico_y_deja_el_resto() {
        let n = config_nueva(ESTA_MAQUINA, "/usr/lib/morgan/morgan-greet").unwrap();
        let doc: DocumentMut = n.parse().unwrap();
        assert_eq!(
            doc["default_session"]["command"].as_str(),
            Some("/usr/lib/morgan/morgan-greet")
        );
        assert_eq!(doc["default_session"]["user"].as_str(), Some("greeter"));
        assert!(doc.get("initial_session").is_none());
        assert_eq!(doc["terminal"]["vt"].as_integer(), Some(1));
        assert!(n.contains("# The VT to run the greeter on."), "{n}");
        assert!(n.contains("# The user to run the command as."), "{n}");
    }

    #[test]
    fn el_compositor_queda_en_el_command() {
        let b = "/usr/lib/morgan/morgan-greet";
        assert_eq!(comando(b, "umbriel"), format!("{b} --compositor umbriel"));
        assert_eq!(instalado(|p| p == "umbriel"), Some("umbriel"));
        assert_eq!(
            instalado(|_| true),
            Some("hyprland"),
            "con los dos, Hyprland"
        );
        assert_eq!(instalado(|_| false), None);
        let con = |c: &str| format!("[default_session]\ncommand = \"{c}\"\n");
        assert!(tiene_greeter(&con(b), b), "el de antes, sin --compositor");
        assert!(tiene_greeter(&con(&comando(b, "hyprland")), b));
        assert!(!tiene_greeter(&con("agreety --cmd x"), b));
        assert!(!tiene_greeter(&con("/usr/lib/morgan/morgan-greeter"), b));
    }

    #[test]
    fn sin_default_session_no_se_inventa() {
        assert!(config_nueva("[terminal]\nvt = 1\n", "x").is_err());
        assert!(config_nueva("no es [toml", "x").is_err());
    }

    #[test]
    fn el_ultimo_respaldo_es_el_mas_nuevo() {
        let d = std::env::temp_dir().join(format!("morgan-respaldos-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        for f in [
            "config.toml",
            "config.toml.backup-20260403-145646",
            &nombre_respaldo("20261106-101010"),
            &nombre_respaldo("20261107-090000"),
        ] {
            std::fs::write(d.join(f), "").unwrap();
        }
        assert_eq!(
            ultimo_respaldo(&d).unwrap(),
            d.join(nombre_respaldo("20261107-090000"))
        );
        // El de un segundo `install` ya trae el greeter: se saltea.
        std::fs::write(
            d.join(nombre_respaldo("20261108-080000")),
            "command = \"/usr/lib/morgan/morgan-greet\"\n",
        )
        .unwrap();
        assert_eq!(
            ultimo_respaldo(&d).unwrap(),
            d.join(nombre_respaldo("20261107-090000"))
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn la_tty_de_emergencia() {
        assert!(hay_tty_de_emergencia("[Login]\n#NAutoVTs=6\n"));
        assert!(hay_tty_de_emergencia(""));
        assert!(!hay_tty_de_emergencia("[Login]\nNAutoVTs = 0\n"));
    }

    const PAM_DE_ARCH: &str = "#%PAM-1.0\n\n\
        auth       required     pam_securetty.so\n\
        auth       requisite    pam_nologin.so\n\
        auth       include      system-local-login\n\
        account    include      system-local-login\n\
        session    include      system-local-login\n";

    #[test]
    fn el_llavero_va_al_final_de_cada_tipo() {
        let n = pam_nuevo(PAM_DE_ARCH).unwrap();
        assert_eq!(
            n,
            "#%PAM-1.0\n\n\
             auth       required     pam_securetty.so\n\
             auth       requisite    pam_nologin.so\n\
             auth       include      system-local-login\n\
             auth       optional     pam_gnome_keyring.so\n\
             account    include      system-local-login\n\
             session    include      system-local-login\n\
             session    optional     pam_gnome_keyring.so auto_start\n"
        );
        assert_eq!(pam_nuevo(&n), None);
    }

    #[test]
    fn la_fecha_ordena() {
        let f = fecha();
        assert_eq!(f.len(), 15);
        assert_eq!(&f[8..9], "-");
    }
}
