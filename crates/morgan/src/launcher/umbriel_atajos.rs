//! Los atajos vigentes de Umbriel, para la página de Atajos del menú, el modo Atajos del
//! launcher y las teclas ocupadas al grabar una. Umbriel no tiene `binds` en el IPC: se leen con
//! `umbriel config get keybinds`, el `[keybinds]` ya juntado de todos los archivos incluidos.
//! No trae descripciones: salen de lo que morgan conoce (sus atajos y los de `atajos_base`), si
//! no de la app que lanza y, si no, de la acción tal cual.

use std::collections::{BTreeMap, HashMap};

use gio::prelude::*;
use serde::Deserialize;

use super::binds;
use super::fuentes::{Atajo, Disparo};
use crate::atajos_base;
use crate::config::Atajos;
use crate::setup_umbriel::normalizar;

/// Las cabeceras con que la página de Atajos agrupa (`menu::atajos::grupo_de`).
const APLICACIONES: &str = "Aplicaciones"; // no-tr
const VENTANAS: &str = "Ventanas y workspaces"; // no-tr
const MULTIMEDIA: &str = "Multimedia y brillo (morgan)"; // no-tr
const CAPTURAS: &str = "Capturas y grabación"; // no-tr
const PROPIOS: &str = "Atajos propios (menú)"; // no-tr
const OTROS: &str = "Sesión y otros"; // no-tr

#[derive(Deserialize)]
#[serde(untagged)]
enum Valor {
    Accion(String),
    Tabla {
        #[serde(default)]
        action: String,
    },
}

/// Un bind de `[keybinds]`: el submap (vacío, el de siempre), la tecla como la escribe Umbriel
/// y la acción.
#[derive(Debug, Clone, PartialEq)]
pub struct Crudo {
    pub submap: String,
    pub tecla: String,
    pub accion: String,
}

pub fn crudos(json: &str) -> Result<Vec<Crudo>, String> {
    let m: BTreeMap<String, Valor> =
        serde_json::from_str(json).map_err(|e| format!("keybinds: {e}"))?; // no-tr
    Ok(m.into_iter()
        .filter_map(|(k, v)| {
            let accion = match v {
                Valor::Accion(a) | Valor::Tabla { action: a } => a.trim().to_owned(),
            };
            let (submap, tecla) = match k.strip_prefix("submap[").and_then(|r| r.split_once("],")) {
                Some((s, t)) => (s.to_owned(), t.to_owned()),
                None => (String::new(), k),
            };
            (!accion.is_empty()).then_some(Crudo { submap, tecla, accion })
        })
        .collect())
}

/// `Mod+Shift+Return` a `SUPER + SHIFT + Return`, como la lee la página y como la compara la
/// grabación (`binds::normalizar`). `Mod` es Super, como en `setup_umbriel`.
pub fn a_hypr(tecla: &str) -> String {
    let mut mods = [false; 4];
    let mut resto = String::new();
    for p in tecla.split('+').map(str::trim) {
        match p.to_lowercase().as_str() {
            "mod" | "super" | "logo" | "win" => mods[0] = true, // no-tr
            "ctrl" | "control" => mods[1] = true,               // no-tr
            "alt" => mods[2] = true,                            // no-tr
            "shift" => mods[3] = true,                          // no-tr
            "wheelup" => resto = "mouse_up".into(),             // no-tr
            "wheeldown" => resto = "mouse_down".into(),         // no-tr
            _ => resto = p.to_owned(),
        }
    }
    binds::combinacion(mods, &resto)
}

/// La acción de un bind de `atajos_base` (lo de la derecha del `=`).
fn accion_base(valor: &str) -> Option<String> {
    let t: toml::Table = toml::from_str(&format!("v = {valor}")).ok()?; // no-tr
    match t.get("v")? {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Table(t) => t.get("action")?.as_str().map(str::to_owned),
        _ => None,
    }
}

fn cabecera_de(accion: &str) -> &'static str {
    if let Some(cmd) = accion.strip_prefix("spawn ") { // no-tr
        let c = cmd.trim_start();
        if ["morganctl volume", "morganctl player", "morganctl brightness"] // no-tr
            .iter()
            .any(|p| c.starts_with(p))
        {
            return MULTIMEDIA;
        }
        if ["grim ", "gsr-ui-cli", "morgan-captura"].iter().any(|p| c.contains(p)) { // no-tr
            // no-tr
            return CAPTURAS;
        }
        return APLICACIONES;
    }
    if accion.starts_with("screencast-") {
        return CAPTURAS;
    }
    let ventanas = ["window-", "column-", "workspace-", "output-", "layout-", "overview-", "scratchpad-"]; // no-tr
    if ventanas.iter().any(|p| accion.starts_with(p)) {
        return VENTANAS;
    }
    OTROS
}

/// Lo que se ve de un bind y la cabecera de su grupo.
fn describir(
    c: &Crudo,
    a: &Atajos,
    base: &[(String, String, String)],
    app: &dyn Fn(&str) -> Option<String>,
) -> (String, &'static str) {
    let clave = normalizar(&c.tecla);
    let accion = c.accion.as_str();
    // Los de morgan, con su nombre: así la página no los repite junto a su fila editable.
    for (cabecera, lista) in binds::acciones(a) {
        for (_, _, cmd, desc, _) in lista {
            if accion == format!("spawn {cmd}") { // no-tr
                return (desc.to_owned(), cabecera);
            }
        }
    }
    if accion == "overview-toggle" {
        // no-tr
        return (tr!("Lienzo de ventanas").to_owned(), VENTANAS);
    }
    for (_, cmd, desc, _) in binds::multimedia() {
        if accion == format!("spawn {cmd}") { // no-tr
            return (desc.to_owned(), MULTIMEDIA);
        }
    }
    if accion.contains("morgan-captura") || accion.contains("grim ") { // no-tr
        // no-tr
        if let Some((_, desc, _)) = binds::CAPTURAS.iter().find(|(t, _, _)| normalizar(t) == clave) {
            return (morgan_idioma::t(desc).to_owned(), CAPTURAS);
        }
    }
    let es_la_tecla = |t: &str| !t.is_empty() && normalizar(t) == clave;
    if let Some(p) = a.propios.iter().find(|p| es_la_tecla(&p.key)) {
        let app = (!p.app.is_empty()).then(|| crate::aplicacion::buscar(&p.app)).flatten();
        return (binds::nombre_propio(p, app.as_ref()), PROPIOS);
    }
    if let Some(m) = a.modos.iter().find(|m| es_la_tecla(&m.key)) {
        return (trf!("Modo: {}", m.name), PROPIOS);
    }
    // Los de `atajos_base`: primero la misma tecla y acción (Mod+I y la rueda hacen lo mismo con
    // otro nombre), después la acción sola.
    let en_base = base
        .iter()
        .find(|(t, ac, _)| *t == clave && ac == accion)
        .or_else(|| base.iter().find(|(_, ac, _)| ac == accion));
    if let Some((_, _, desc)) = en_base {
        return (desc.clone(), cabecera_de(accion));
    }
    if let Some(cmd) = accion.strip_prefix("spawn ") { // no-tr
        let cmd = cmd.trim();
        // La última palabra que es una app: `kitty --class btop btop` es btop.
        let nombre = cmd
            .split_whitespace()
            .rev()
            .filter(|p| !p.starts_with('-'))
            .find_map(|p| app(p.rsplit('/').next().unwrap_or(p)));
        return (nombre.unwrap_or_else(|| cmd.to_owned()), cabecera_de(accion));
    }
    (accion.to_owned(), cabecera_de(accion))
}

/// Los binds fuera de los submaps, con su cabecera.
pub fn atajos(
    crudos: &[Crudo],
    a: &Atajos,
    base: &[atajos_base::Bind],
    app: &dyn Fn(&str) -> Option<String>,
) -> Vec<(Atajo, &'static str)> {
    let base: Vec<(String, String, String)> = base
        .iter()
        .filter_map(|b| Some((normalizar(&b.tecla), accion_base(b.umbriel.as_deref()?)?, b.desc.clone())))
        .collect();
    crudos
        .iter()
        .filter(|c| c.submap.is_empty())
        .map(|c| {
            let (descripcion, cabecera) = describir(c, a, &base, app);
            let atajo = Atajo {
                descripcion,
                teclas: a_hypr(&c.tecla),
                disparo: Some(Disparo::Umbriel(c.accion.clone())),
            };
            (atajo, cabecera)
        })
        .collect()
}

/// El nombre de cada app por su id sin `.desktop`, su `StartupWMClass` y su ejecutable, en
/// minúsculas: una pasada por todas, no una por palabra.
fn nombres_de_apps() -> HashMap<String, String> {
    let mut m = HashMap::new();
    for a in gio::AppInfo::all().into_iter().filter(|a| a.should_show()) {
        let nombre = a.display_name().to_string();
        let mut claves = Vec::new();
        if let Some(id) = a.id() {
            claves.push(id.trim_end_matches(".desktop").to_lowercase()); // no-tr
        }
        if let Some(e) = a.executable().file_name() {
            claves.push(e.to_string_lossy().to_lowercase());
        }
        if let Ok(d) = a.downcast::<gio_unix::DesktopAppInfo>()
            && let Some(c) = d.startup_wm_class()
        {
            claves.push(c.to_lowercase());
        }
        for c in claves.into_iter().filter(|c| !c.is_empty()) {
            m.entry(c).or_insert_with(|| nombre.clone());
        }
    }
    m
}

pub async fn leer(a: &Atajos) -> Result<Vec<(Atajo, &'static str)>, String> {
    let json = crate::comando::salida(&["umbriel", "config", "get", "keybinds"]).await?; // no-tr
    let crudos = crudos(&json)?;
    let base = atajos_base::tabla(&atajos_base::Opciones::de_la_maquina());
    let apps = nombres_de_apps();
    Ok(atajos(&crudos, a, &base, &|p| apps.get(&p.to_lowercase()).cloned()))
}

/// `umbriel msg` con la acción partida en palabras; `spawn` lleva el resto entero, que Umbriel
/// corre como un comando de shell.
pub fn mensaje(accion: &str) -> Vec<String> {
    let mut v = vec!["umbriel".to_owned(), "msg".to_owned()]; // no-tr
    match accion.strip_prefix("spawn ") { // no-tr
        Some(cmd) => v.extend(["spawn".to_owned(), cmd.trim().to_owned()]), // no-tr
        None => v.extend(accion.split_whitespace().map(str::to_owned)),
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AtajoPropio;

    /// Recortado de `umbriel config get keybinds` (umbriel-git r1644).
    const JSON: &str = r#"{
        "Ctrl+Alt+Delete": {"action": "session-quit skip-confirmation", "repeat": false},
        "Mod+0": "workspace-switch 10",
        "Mod+1": "workspace-switch 1",
        "Mod+B": {"action": "spawn umbriel-ciclar brave-origin brave-origin", "repeat": false},
        "Mod+M": {"action": "spawn kitty --class btop btop", "repeat": false},
        "Mod+Shift+T": {"action": "spawn ~/.local/bin/tele", "repeat": false},
        "Mod+X": "window-swap-left",
        "Mod+Tab": {"action": "overview-toggle", "repeat": false},
        "Super+space": {"action": "spawn morganctl launcher toggle", "repeat": false},
        "XF86AudioMute": {"action": "spawn morganctl volume mute", "allow_when_locked": true},
        "Print": {"action": "spawn grim -g \"$(slurp)\" - | wl-copy --type image/png"},
        "Super+F1": {"action": "spawn qalc"},
        "submap[morgan-record],Super+Ctrl+Alt+Shift+F24": "submap reset",
        "Mod+Q": {"repeat": false}
    }"#;

    fn leidos(a: &Atajos) -> Vec<(String, String, &'static str)> {
        let base = atajos_base::tabla(&atajos_base::Opciones { grabador: true, uwsm: false });
        let apps = |p: &str| match p {
            "brave-origin" => Some("Brave".to_owned()), // no-tr
            "btop" => Some("btop++".to_owned()),        // no-tr
            _ => None,
        };
        atajos(&crudos(JSON).unwrap(), a, &base, &apps)
            .into_iter()
            .map(|(x, c)| (x.teclas, x.descripcion, c))
            .collect()
    }

    fn de<'a>(v: &'a [(String, String, &'static str)], teclas: &str) -> (&'a str, &'static str) {
        let (_, d, c) = v.iter().find(|(t, _, _)| t == teclas).unwrap_or_else(|| panic!("{teclas}: {v:?}"));
        (d, c)
    }

    #[test]
    fn lee_tablas_cadenas_y_submaps() {
        let c = crudos(JSON).unwrap();
        // Sin acción no cuenta.
        assert_eq!(c.len(), 13);
        let s = c.iter().find(|c| !c.submap.is_empty()).unwrap();
        assert_eq!((s.submap.as_str(), s.tecla.as_str(), s.accion.as_str()), ("morgan-record", "Super+Ctrl+Alt+Shift+F24", "submap reset"));
        assert!(crudos("no es json").is_err());
    }

    #[test]
    fn las_teclas_a_la_manera_de_hyprland() {
        assert_eq!(a_hypr("Mod+Shift+Return"), "SUPER + SHIFT + Return");
        assert_eq!(a_hypr("Super+comma"), "SUPER + comma");
        assert_eq!(a_hypr("Ctrl+Alt+Delete"), "CTRL + ALT + Delete");
        assert_eq!(a_hypr("Mod+b"), "SUPER + B");
        assert_eq!(a_hypr("Mod+WheelUp"), "SUPER + mouse_up");
        assert_eq!(binds::normalizar(&a_hypr("Shift+Mod+q")), binds::normalizar("SUPER + SHIFT + Q"));
    }

    #[test]
    fn nombra_cada_uno() {
        let mut a = Atajos::default();
        a.propios.push(AtajoPropio { key: "SUPER + F1".into(), command: "qalc".into(), ..Default::default() });
        let v = leidos(&a);
        // Fuera de los submaps.
        assert_eq!(v.len(), 12);
        assert!(v.iter().all(|(t, _, _)| !t.contains("F24")));
        assert_eq!(de(&v, "SUPER + space"), (tr!("Launcher"), "Menús y lanzadores (morgan)"));
        assert_eq!(de(&v, "XF86AudioMute"), (tr!("Silenciar audio"), MULTIMEDIA));
        assert_eq!(de(&v, "Print"), (tr!("Captura región"), CAPTURAS));
        assert_eq!(de(&v, "SUPER + Tab"), (tr!("Lienzo de ventanas"), VENTANAS));
        assert_eq!(de(&v, "SUPER + F1"), ("qalc", PROPIOS));
        assert_eq!(de(&v, "SUPER + 1"), (format!("{} 1", tr!("Workspace")).as_str(), VENTANAS));
        assert_eq!(de(&v, "SUPER + 0"), (tr!("Workspace de juegos"), VENTANAS));
        assert_eq!(de(&v, "CTRL + ALT + Delete"), (tr!("Cerrar sesión"), OTROS));
        assert_eq!(de(&v, "SUPER + B"), ("Brave (Cycle)", APLICACIONES));
        // Fuera de la tabla: la app que lanza, o el comando, o la acción.
        assert_eq!(de(&v, "SUPER + M"), ("System Monitor", APLICACIONES));
        assert_eq!(de(&v, "SUPER + SHIFT + T"), ("~/.local/bin/tele", APLICACIONES));
        assert_eq!(de(&v, "SUPER + X"), ("window-swap-left", VENTANAS));
    }

    #[test]
    fn la_app_por_la_ultima_palabra() {
        let a = Atajos::default();
        let base = Vec::new();
        let apps = |p: &str| (p == "btop").then(|| "btop++".to_owned()); // no-tr
        let c = Crudo { submap: String::new(), tecla: "Mod+M".into(), accion: "spawn kitty --class btop btop".into() };
        assert_eq!(atajos(&[c], &a, &base, &apps)[0].0.descripcion, "btop++");
    }

    #[test]
    fn manda_la_accion_partida() {
        assert_eq!(mensaje("workspace-switch 10"), ["umbriel", "msg", "workspace-switch", "10"]);
        assert_eq!(
            mensaje("spawn kitty --class btop btop"),
            ["umbriel", "msg", "spawn", "kitty --class btop btop"]
        );
    }
}
