//! `morgan setup`: lo que cada usuario hace una vez y un paquete no puede hacer por él (la
//! línea de `hyprland.lua` y la unidad de systemd --user). Se puede correr de nuevo: lo que ya
//! está no se repite.

use std::path::Path;
use std::process::{Command, ExitCode};

use morgan_idioma::{tr, trf};

pub const LINEA: &str = "pcall(require, \"morgan\")"; // no-tr

/// La primera línea del comentario es la que busca `install.sh --desinstalar`.
const COMENTARIO: &str = "-- morgan: sus binds, los colores del tema, las reglas de sus capas y lo que se cambia\n-- desde su menú. Va al final, para pisar lo de arriba.\n"; // no-tr

/// El texto con la línea de morgan al final; `None` si ya la tiene.
pub fn con_linea(texto: &str) -> Option<String> {
    if texto.lines().any(|l| l.trim() == LINEA) {
        return None;
    }
    let mut t = texto.to_owned();
    if !t.is_empty() && !t.ends_with('\n') {
        t.push('\n');
    }
    t.push('\n');
    t.push_str(COMENTARIO);
    t.push_str(LINEA);
    t.push('\n');
    Some(t)
}

/// Lo que se para antes de arrancar morgan: hace lo mismo y se dibujaría encima o se quedaría
/// con el nombre de las notificaciones. Los shells de Quickshell (Noctalia, DankMaterialShell,
/// Caelestia) corren como `quickshell`, o `qs` si se lanzaron por ese nombre.
const COMPETIDORES: &[&str] = &[
    "waybar", "swaync", "mako", "dunst", "swayosd-server", "hypridle", "hyprlock", // no-tr
    "swayidle", "swaylock", "quickshell", "qs", "ags", "eww", // no-tr
];

pub fn correr() -> ExitCode {
    let config = glib::user_config_dir();
    let hypr = config.join("hypr/hyprland.lua");
    if !hypr.is_file() {
        if config.join("hypr/hyprland.conf").is_file() {
            eprintln!("{}", trf!("morgan necesita la config de Hyprland en Lua ({}); hay una hyprland.conf.", hypr.display()));
        } else {
            eprintln!("{}", trf!("No está {}: morgan necesita la config de Hyprland en Lua.", hypr.display()));
        }
        return ExitCode::FAILURE;
    }
    if let Err(e) = agregar_linea(&hypr) {
        eprintln!("{}", trf!("No se pudo escribir {}: {}", hypr.display(), e));
        return ExitCode::FAILURE;
    }
    if !systemctl(&["enable", "morgan.service"]) {
        return ExitCode::FAILURE;
    }
    let ajenas = notificaciones_ajenas();
    for p in COMPETIDORES.iter().filter(|p| corre(p)) {
        println!("  ! {}", trf!("{} está corriendo: morgan hace lo mismo; conviene pararlo y deshabilitarlo.", p));
    }
    if let Some(quien) = &ajenas {
        println!("  ! {}", trf!("{} tiene las notificaciones: hay que pararlo antes de arrancar morgan.", quien));
    }
    let en_hyprland = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some();
    if en_hyprland && ajenas.is_none() {
        let _ = Command::new("systemctl")
            .args(["--user", "import-environment"])
            .args(VARIABLES)
            .status();
        if systemctl(&["start", "morgan.service"]) {
            println!("{}", tr!("morgan arrancó. El menú de sistema se abre con Super+coma."));
        }
    } else {
        println!("{}", tr!("Listo: morgan arranca la próxima vez que entres a Hyprland."));
    }
    ExitCode::SUCCESS
}

/// Lo que la unidad necesita de la sesión gráfica; sin uwsm, el gestor de systemd no lo tiene.
pub const VARIABLES: &[&str] = &[
    "WAYLAND_DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE", "XDG_CURRENT_DESKTOP", "XDG_SESSION_TYPE", "DISPLAY", // no-tr
];

fn agregar_linea(hypr: &Path) -> std::io::Result<()> {
    let texto = std::fs::read_to_string(hypr)?;
    match con_linea(&texto) {
        Some(nuevo) => {
            std::fs::write(hypr, nuevo)?;
            println!("{}", trf!("Agregada la línea de morgan al final de {}.", hypr.display()));
        }
        None => println!("{}", trf!("{} ya carga morgan.", hypr.display())),
    }
    Ok(())
}

fn systemctl(args: &[&str]) -> bool {
    match Command::new("systemctl").arg("--user").args(args).status() {
        Ok(s) if s.success() => true,
        _ => {
            eprintln!("{}", trf!("Falló systemctl --user {}.", args.join(" ")));
            false
        }
    }
}

fn corre(programa: &str) -> bool {
    Command::new("pgrep")
        .args(["-x", programa])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// El proceso que tiene `org.freedesktop.Notifications`, si no es morgan.
fn notificaciones_ajenas() -> Option<String> {
    let o = Command::new("busctl")
        .args(["--user", "status", "org.freedesktop.Notifications"]) // no-tr
        .output()
        .ok()?;
    let texto = String::from_utf8_lossy(&o.stdout);
    let quien = texto.lines().find_map(|l| l.strip_prefix("Comm="))?.trim().to_owned();
    (quien != "morgan").then_some(quien)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_linea_va_una_vez_al_final() {
        let t = con_linea("hl.config({})").unwrap();
        assert!(t.starts_with("hl.config({})\n\n-- morgan:"));
        assert!(t.ends_with("pcall(require, \"morgan\")\n"));
        assert_eq!(con_linea(&t), None);
        assert_eq!(con_linea("  pcall(require, \"morgan\")  \n"), None);
    }
}
