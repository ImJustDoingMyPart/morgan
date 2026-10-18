//! Una app por su `.desktop`, para lo que el menú arma en la config de Hyprland (un atajo que
//! la abre o la busca, una regla para sus ventanas): su nombre, las clases con que aparecen
//! sus ventanas y el comando que la lanza.

use gio::prelude::*;
use gtk::gio;

#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub nombre: String,
    /// `StartupWMClass` primero (lo que la app declara), después el id sin `.desktop`.
    pub clases: Vec<String>,
    pub lanzar: String,
}

/// Las que el launcher muestra, por nombre.
pub fn todas() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = gio::AppInfo::all()
        .into_iter()
        .filter(|a| a.should_show())
        .filter_map(|a| Some((a.id()?.to_string(), a.display_name().to_string())))
        .collect();
    v.sort_by_key(|(_, n)| n.to_lowercase());
    v.dedup_by(|a, b| a.0 == b.0);
    v
}

pub fn buscar(id: &str) -> Option<App> {
    let d = gio_unix::DesktopAppInfo::new(id)?;
    let ruta = d.filename()?.to_string_lossy().into_owned();
    Some(App {
        nombre: d.display_name().to_string(),
        clases: clases(id, d.startup_wm_class().as_deref()),
        lanzar: comando(id, &ruta, crate::comando::con_uwsm()),
    })
}

pub fn clases(id: &str, wm_class: Option<&str>) -> Vec<String> {
    let base = id.strip_suffix(".desktop").unwrap_or(id).to_owned();
    let mut v: Vec<String> = wm_class
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
        .into_iter()
        .collect();
    if !v.iter().any(|c| c.eq_ignore_ascii_case(&base)) {
        v.push(base);
    }
    v
}

/// Como `comando::lanzar_app`: fuera del cgroup de quien la lanza.
pub fn comando(id: &str, ruta: &str, uwsm: bool) -> String {
    let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    if uwsm {
        format!("uwsm app -- {}", q(id)) // no-tr
    } else {
        format!("systemd-run --user --scope --collect --quiet -- gio launch {}", q(ruta)) // no-tr
    }
}

/// Para el `match` de Hyprland: cualquiera de las clases, entera.
pub fn patron(clases: &[String]) -> String {
    let escapar = |c: &String| {
        c.chars()
            .flat_map(|ch| {
                let especial = "\\.^$|?*+()[]{}".contains(ch);
                especial.then_some('\\').into_iter().chain(std::iter::once(ch))
            })
            .collect::<String>()
    };
    format!("^({})$", clases.iter().map(escapar).collect::<Vec<_>>().join("|"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clases_de_una_app() {
        assert_eq!(clases("firefox.desktop", None), ["firefox"]);
        assert_eq!(
            clases("org.gnome.Nautilus.desktop", Some("org.gnome.Nautilus")),
            ["org.gnome.Nautilus"]
        );
        assert_eq!(clases("code.desktop", Some("Code")), ["Code"]);
        assert_eq!(clases("brave-browser.desktop", Some("brave")), ["brave", "brave-browser"]);
    }

    #[test]
    fn patron_escapado() {
        assert_eq!(
            patron(&["org.gnome.Nautilus".into(), "x+y".into()]),
            r"^(org\.gnome\.Nautilus|x\+y)$"
        );
    }

    #[test]
    fn comando_con_y_sin_uwsm() {
        assert_eq!(comando("kitty.desktop", "/a/kitty.desktop", true), "uwsm app -- 'kitty.desktop'");
        assert_eq!(
            comando("kitty.desktop", "/a/kitty.desktop", false),
            "systemd-run --user --scope --collect --quiet -- gio launch '/a/kitty.desktop'"
        );
    }
}
