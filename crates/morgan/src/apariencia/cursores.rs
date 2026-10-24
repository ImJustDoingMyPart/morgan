//! El cursor no tiene una sola fuente de verdad, así que se escribe en cuatro lugares: el env
//! de uwsm si se usa (lo heredan Hyprland y todo lo que se lance, completo en el próximo login),
//! `hyprctl setcursor` (el compositor, en caliente), gsettings (las apps GTK, en caliente) y los
//! `settings.ini` de GTK (una app en Flatpak puede ver `~/.config` sin ver dconf).

use std::path::PathBuf;

use gio::prelude::*;

use super::archivos::{self, reescribir};

fn uwsm_env() -> PathBuf {
    glib::user_config_dir().join("uwsm/env")
}

/// La del usuario pisa a la del sistema, como en la spec de íconos.
pub fn bases() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = glib::system_data_dirs()
        .into_iter()
        .rev()
        .map(|d| d.join("icons"))
        .collect();
    if !v.iter().any(|d| d == std::path::Path::new("/usr/share/icons")) {
        v.insert(0, PathBuf::from("/usr/share/icons"));
    }
    let home = glib::home_dir();
    v.push(home.join(".icons"));
    v.push(glib::user_data_dir().join("icons"));
    v
}

pub fn listar() -> Vec<String> {
    let mut v: Vec<String> = bases()
        .iter()
        .filter_map(|b| std::fs::read_dir(b).ok())
        .flatten()
        .flatten()
        .filter(|e| {
            std::fs::read_dir(e.path().join("cursors")).is_ok_and(|mut d| d.next().is_some())
        })
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .collect();
    v.sort();
    v.dedup();
    v
}

fn interfaz() -> gio::Settings {
    gio::Settings::new("org.gnome.desktop.interface")
}

pub fn actual() -> (String, u32) {
    let env = std::fs::read_to_string(uwsm_env()).unwrap_or_default();
    let s = interfaz();
    let tema = archivos::env_leer(&env, "XCURSOR_THEME").unwrap_or_else(|| s.string("cursor-theme").into());
    let tamano = archivos::env_leer(&env, "XCURSOR_SIZE")
        .and_then(|t| t.parse().ok())
        .unwrap_or_else(|| u32::try_from(s.int("cursor-size")).unwrap_or(24));
    (tema, tamano)
}

pub async fn aplicar(tema: &str, tamano: u32) -> Result<(), String> {
    if !listar().iter().any(|t| t == tema) {
        return Err(format!("no existe el tema de cursor «{tema}»"));
    }
    if !(8..=256).contains(&tamano) {
        return Err(format!("tamaño de cursor fuera de rango: {tamano}"));
    }
    let n = tamano.to_string();
    reescribir(&uwsm_env(), |t| {
        archivos::env(
            t,
            &[("XCURSOR_THEME", tema), ("XCURSOR_SIZE", &n), ("HYPRCURSOR_SIZE", &n)],
        )
    });
    if let Err(e) = crate::comando::salida(&["hyprctl", "setcursor", tema, &n]).await {
        eprintln!("morgan: apariencia: {e}");
    }
    let s = interfaz();
    if let Err(e) = s
        .set_string("cursor-theme", tema)
        .and_then(|()| s.set_int("cursor-size", tamano as i32))
    {
        eprintln!("morgan: apariencia: gsettings cursor: {e}");
    }
    for ini in ["gtk-3.0/settings.ini", "gtk-4.0/settings.ini"] {
        let ruta = glib::user_config_dir().join(ini);
        reescribir(&ruta, |t| {
            archivos::ini(t, "gtk-cursor-theme-name", tema)
                .and_then(|t| archivos::ini(&t, "gtk-cursor-theme-size", &n))
        });
    }
    Ok(())
}
