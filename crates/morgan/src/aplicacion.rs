//! Una app por su `.desktop`, para lo que el menú arma en la config de Hyprland (un atajo que
//! la abre o la busca, una regla para sus ventanas): su nombre, las clases con que aparecen
//! sus ventanas y el comando que la lanza.

use gio::prelude::*;

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

/// El `.desktop` de las ventanas de `clase`: por su nombre o por lo que declara.
pub fn de_clase(clase: &str) -> Option<gio_unix::DesktopAppInfo> {
    if clase.is_empty() {
        return None;
    }
    let por_nombre = [clase.to_owned(), clase.to_lowercase()]
        .into_iter()
        .find_map(|n| gio_unix::DesktopAppInfo::new(&format!("{n}.desktop")));
    por_nombre.or_else(|| {
        gio::AppInfo::all().into_iter().find_map(|a| {
            let d = a.downcast::<gio_unix::DesktopAppInfo>().ok()?;
            d.startup_wm_class()
                .filter(|c| c.eq_ignore_ascii_case(clase))
                .map(|_| d)
        })
    })
}

/// Un programa auxiliar, no una app: su `.desktop` está escondido de los menús y no tiene
/// categorías (el selector de archivos del portal, el pedido de contraseña de gcr, kwalletd,
/// zenity). Uno escondido con categorías (imv, que no se abre sin un archivo) sigue siendo una app.
pub fn es_auxiliar(d: &gio_unix::DesktopAppInfo) -> bool {
    d.is_nodisplay() && d.categories().is_none_or(|c| c.split(';').all(str::is_empty))
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

    fn desktop(extra: &str) -> gio_unix::DesktopAppInfo {
        let k = glib::KeyFile::new();
        let texto = format!("[Desktop Entry]\nType=Application\nName=X\nExec=sh\n{extra}");
        k.load_from_data(&texto, glib::KeyFileFlags::NONE).unwrap();
        gio_unix::DesktopAppInfo::from_keyfile(&k).unwrap()
    }

    #[test]
    fn un_auxiliar_es_escondido_y_sin_categorias() {
        assert!(es_auxiliar(&desktop("NoDisplay=true\n")));
        assert!(es_auxiliar(&desktop("NoDisplay=true\nCategories=;\n")));
        assert!(!es_auxiliar(&desktop("NoDisplay=true\nCategories=Graphics;Viewer;\n")));
        assert!(!es_auxiliar(&desktop("")));
    }

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
