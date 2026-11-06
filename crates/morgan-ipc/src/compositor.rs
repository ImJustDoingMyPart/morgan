//! Qué compositor corre la sesión. Primero `XDG_CURRENT_DESKTOP`: un compositor anidado hereda
//! las variables del de afuera (un Umbriel adentro de Hyprland ve también
//! `HYPRLAND_INSTANCE_SIGNATURE`), pero esa la pisa siempre el de adentro. Después, el socket
//! de cada uno.

use std::ffi::OsString;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tipo {
    Hyprland,
    Umbriel,
    /// Ninguno conocido: morgan anda sin lo que depende del compositor.
    Otro,
}

pub fn detectar(env: impl Fn(&str) -> Option<OsString>) -> Tipo {
    let hay = |k: &str| env(k).is_some_and(|v| !v.is_empty());
    let escritorio = env("XDG_CURRENT_DESKTOP")
        .map(|v| v.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    // Puede venir como lista: `Hyprland:GNOME`.
    for nombre in escritorio.split(':') {
        match nombre {
            "hyprland" if hay("HYPRLAND_INSTANCE_SIGNATURE") => return Tipo::Hyprland,
            "umbriel" => return Tipo::Umbriel,
            _ => {}
        }
    }
    if hay("UMBRIEL_SOCKET") {
        Tipo::Umbriel
    } else if hay("HYPRLAND_INSTANCE_SIGNATURE") {
        Tipo::Hyprland
    } else {
        Tipo::Otro
    }
}

/// El de este proceso.
pub fn actual() -> Tipo {
    detectar(|k| std::env::var_os(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pares: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| {
            pares
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn el_anidado_gana_al_de_afuera() {
        // Umbriel anidado en Hyprland: hereda la firma de Hyprland.
        let umbriel_en_hyprland = [
            ("XDG_CURRENT_DESKTOP", "umbriel"),
            ("UMBRIEL_SOCKET", "/run/user/1000/umbriel-wayland-0.sock"),
            ("HYPRLAND_INSTANCE_SIGNATURE", "abc"),
        ];
        assert_eq!(detectar(env(&umbriel_en_hyprland)), Tipo::Umbriel);
        let hyprland = [
            ("XDG_CURRENT_DESKTOP", "Hyprland"),
            ("HYPRLAND_INSTANCE_SIGNATURE", "abc"),
        ];
        assert_eq!(detectar(env(&hyprland)), Tipo::Hyprland);
    }

    #[test]
    fn sin_escritorio_por_el_socket() {
        assert_eq!(detectar(env(&[("UMBRIEL_SOCKET", "/x")])), Tipo::Umbriel);
        assert_eq!(
            detectar(env(&[("HYPRLAND_INSTANCE_SIGNATURE", "abc")])),
            Tipo::Hyprland
        );
        assert_eq!(
            detectar(env(&[("XDG_CURRENT_DESKTOP", "GNOME")])),
            Tipo::Otro
        );
        assert_eq!(
            detectar(env(&[("HYPRLAND_INSTANCE_SIGNATURE", "")])),
            Tipo::Otro
        );
        // Dice Hyprland pero sin su firma no hay con quién hablar.
        assert_eq!(
            detectar(env(&[("XDG_CURRENT_DESKTOP", "Hyprland:GNOME")])),
            Tipo::Otro
        );
    }
}
