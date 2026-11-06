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

use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, value};

pub const BINARIO: &str = "usr/lib/morgan/morgan-greet";
pub const ESTADO: &str = "var/lib/morgan-greet";
pub const GREETD: &str = "etc/greetd/config.toml";
const PREFIJO_RESPALDO: &str = "config.toml.morgan-";

/// Los compositores que el greeter sabe lanzar, en orden de preferencia, con el programa que
/// tiene que estar.
pub const COMPOSITORES: &[(&str, &str)] = &[("hyprland", "start-hyprland")];

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

/// El respaldo más nuevo de `dir` (el nombre lleva la fecha, así que ordena solo).
pub fn ultimo_respaldo(dir: &Path) -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(PREFIJO_RESPALDO))
        })
        .collect();
    v.sort();
    v.pop()
}

pub fn nombre_respaldo(fecha: &str) -> String {
    format!("{PREFIJO_RESPALDO}{fecha}")
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
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn la_tty_de_emergencia() {
        assert!(hay_tty_de_emergencia("[Login]\n#NAutoVTs=6\n"));
        assert!(hay_tty_de_emergencia(""));
        assert!(!hay_tty_de_emergencia("[Login]\nNAutoVTs = 0\n"));
    }

    #[test]
    fn la_fecha_ordena() {
        let f = fecha();
        assert_eq!(f.len(), 15);
        assert_eq!(&f[8..9], "-");
    }
}
