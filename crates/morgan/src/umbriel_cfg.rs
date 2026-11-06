//! La config de morgan para Umbriel: sus atajos y las reglas de sus capas, en
//! `~/.config/umbriel/morgan.toml`, que la config de Umbriel incluye (`morgan setup`). Umbriel
//! la relee sola al cambiar el archivo y, si queda inválida, sigue con la última buena; igual
//! se valida antes con `umbriel config validate` y, si falla, queda la de antes.
//!
//! Los archivos incluidos se aplican antes que el que los incluye (`configuration.md`): un
//! atajo que el usuario ponga en su `config.toml` en la misma tecla gana sobre el de morgan.

use std::path::{Path, PathBuf};

use crate::config::{AtajoPropio, Capturas, Config};
use crate::launcher::binds;

/// `MORGAN_UMBRIEL_CFG` lo cambia, para que un Umbriel anidado de pruebas no toque la config
/// real.
pub fn ruta() -> PathBuf {
    std::env::var_os("MORGAN_UMBRIEL_CFG")
        .map(PathBuf::from)
        .unwrap_or_else(|| glib::user_config_dir().join("umbriel/morgan.toml"))
}

/// Las capas que se abren sobre ventanas desenfocan lo que tienen detrás; la barra, solo el
/// fondo (el desenfoque optimizado, más barato).
const CAPAS: &str = r#"
# Las capas de morgan: desenfoque recortado a lo que se ve.
[[layer_rule]]
match.namespace = "^morgan-"
blur = true
blur_popups = true
blur_ignore_alpha = 0.5

[[layer_rule]]
match.namespace = "^morgan-(center|calendar|tray|calculator|notifications|osd|launcher|polkit|menu|wallpapers|windows|dock|dock-globo)$"
blur_optimized = false
"#;

/// `SUPER + ALT + L` de Hyprland a `Super+Alt+L`. `None` para lo que Umbriel no tiene igual
/// (un código de tecla, un botón del mouse con nombre de Hyprland).
pub fn tecla(hypr: &str) -> Option<String> {
    let mut partes = Vec::new();
    for p in hypr.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        let nombre = match p.to_uppercase().as_str() {
            "SUPER" | "MOD4" | "WIN" | "META" => "Super".to_owned(), // no-tr
            "CTRL" | "CONTROL" => "Ctrl".to_owned(),                 // no-tr
            "ALT" | "MOD1" => "Alt".to_owned(),                      // no-tr
            "SHIFT" => "Shift".to_owned(),                           // no-tr
            _ if p.contains(':') => return None,
            _ => p.to_owned(),
        };
        partes.push(nombre);
    }
    (!partes.is_empty()).then(|| partes.join("+"))
}

/// Una cadena básica de TOML en una línea (las de varias no van en una tabla en línea). Las
/// secuencias de escape de JSON valen en TOML.
fn cadena(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

fn entre_comillas(ruta: &str) -> String {
    format!("'{}'", ruta.replace('\'', "'\\''"))
}

/// Sin hyprshot (habla con Hyprland): grim y slurp. Como hyprshot, guarda y copia, o solo
/// copia; morgan avisa con la vista previa (`morgan-captura`).
fn comando_captura(c: &Capturas, que: &str) -> String {
    let fuente = match que {
        "region" => "-g \"$(slurp)\"".to_owned(), // no-tr
        "output" => {
            "-o \"$(umbriel outputs --json | jq -r '.[] | select(.focused) | .name')\"".to_owned()
        } // no-tr
        _ => "-g \"$(umbriel focused-window --json | jq -r '\"\\(.x),\\(.y) \\(.w)x\\(.h)\"')\""
            .to_owned(), // no-tr
    };
    if c.portapapeles {
        return format!("grim {fuente} - | wl-copy --type image/png"); // no-tr
    }
    let carpeta = if c.carpeta.is_empty() {
        binds::carpeta_capturas()
    } else {
        c.carpeta.clone()
    };
    format!(
        "mkdir -p {d} && f={d}/\"$(date +%Y-%m-%d-%H%M%S)_morgan.png\" && grim {fuente} \"$f\" && wl-copy --type image/png < \"$f\" && morgan-captura \"$f\"", // no-tr
        d = entre_comillas(&carpeta)
    )
}

fn bind(tecla_hypr: &str, accion: &str, extra: &[&str]) -> Option<String> {
    let t = tecla(tecla_hypr)?;
    let mut campos = vec![format!("action = {}", cadena(accion))]; // no-tr
    campos.extend(extra.iter().map(|e| (*e).to_owned()));
    Some(format!("{} = {{ {} }}\n", cadena(&t), campos.join(", ")))
}

fn accion_propio(p: &AtajoPropio) -> Option<String> {
    if !p.app.is_empty() {
        let app = crate::aplicacion::buscar(&p.app)?;
        return Some(format!("spawn {}", app.lanzar)); // no-tr
    }
    let cmd = p.command.trim();
    (!cmd.is_empty()).then(|| format!("spawn {cmd}")) // no-tr
}

fn extra_propio(p: &AtajoPropio) -> Vec<&'static str> {
    let mut v = Vec::new();
    if !p.repeat {
        v.push("repeat = false"); // no-tr
    }
    if p.locked {
        v.push("allow_when_locked = true"); // no-tr
    }
    v
}

pub fn toml(c: &Config, con_grim: bool) -> String {
    let a = &c.atajos;
    let mut out = String::from(concat!(
        "# morgan: sus atajos y las reglas de sus capas. Lo escribe morgan con cada cambio de su\n", // no-tr
        "# config (morgan/config.toml, [keybinds] y [screenshots]): no editar a mano.\n", // no-tr
        "\n[keybinds]\n",                                                                 // no-tr
    ));
    for (cabecera, lista) in binds::acciones(a) {
        out.push_str(&format!("# {cabecera}\n"));
        for (_, t, cmd, _, locked) in lista {
            let extra: &[&str] = if locked {
                &["repeat = false", "allow_when_locked = true"] // no-tr
            } else {
                &["repeat = false"] // no-tr
            };
            if let Some(b) = bind(t, &format!("spawn {cmd}"), extra) {
                out.push_str(&b);
            }
        }
    }
    // El lienzo: cada toque de la tecla pide un paso; soltar el modificador lo ve el lienzo
    // mismo, que tiene el teclado.
    if let Some(b) = bind(&a.ventanas, "spawn morganctl windows", &["repeat = false"]) {
        // no-tr
        out.push_str("# Lienzo de ventanas (morgan)\n");
        out.push_str(&b);
    }
    if a.multimedia {
        out.push_str("# Multimedia y brillo (morgan)\n");
        for (t, cmd, _, repite) in binds::multimedia() {
            let extra: &[&str] = if repite {
                &["allow_when_locked = true"] // no-tr
            } else {
                &["allow_when_locked = true", "repeat = false"] // no-tr
            };
            if let Some(b) = bind(t, &format!("spawn {cmd}"), extra) {
                out.push_str(&b);
            }
        }
    }
    if con_grim {
        out.push_str("# Capturas de pantalla (grim y slurp)\n");
        for (t, que) in [
            ("Print", "region"),
            ("CTRL + Print", "output"),
            ("ALT + Print", "window"),
        ] {
            // no-tr
            if let Some(b) = bind(
                t,
                &format!("spawn {}", comando_captura(&c.capturas, que)),
                &["repeat = false"],
            ) {
                // no-tr
                out.push_str(&b);
            }
        }
    }
    let propios: Vec<String> = a
        .propios
        .iter()
        .filter(|p| !p.key.is_empty())
        .filter_map(|p| bind(&p.key, &accion_propio(p)?, &extra_propio(p)))
        .collect();
    if !propios.is_empty() {
        out.push_str("# Atajos propios (menú)\n");
        out.extend(propios);
    }
    for (m, modo) in a
        .modos
        .iter()
        .enumerate()
        .filter(|(_, m)| !m.key.is_empty())
    {
        let nombre = binds::submap_modo(m);
        out.push_str(&format!("# Modo: {}\n", modo.name));
        if let Some(b) = bind(&modo.key, &format!("submap {nombre}"), &["repeat = false"]) {
            // no-tr
            out.push_str(&b);
        }
        // Escape siempre sale: un modo sin él dejaría el teclado atrapado.
        out.push_str(&format!(
            "{} = \"submap reset\"\n",
            cadena(&format!("submap[{nombre}],Escape"))
        )); // no-tr
        for p in &modo.binds {
            let (Some(t), Some(accion)) = (tecla(&p.key), accion_propio(p)) else {
                continue;
            };
            let mut extra: Vec<&str> = extra_propio(p);
            if modo.once {
                extra.push("submap = \"reset\""); // no-tr
            }
            let mut campos = vec![format!("action = {}", cadena(&accion))]; // no-tr
            campos.extend(extra.iter().map(|e| (*e).to_owned()));
            out.push_str(&format!(
                "{} = {{ {} }}\n",
                cadena(&format!("submap[{nombre}],{t}")),
                campos.join(", ")
            ));
        }
    }
    // Mientras el menú graba una tecla (Atajos): un submap sin nada, para que la combinación le
    // llegue entera. Umbriel no conoce un submap sin binds: lleva uno de relleno.
    out.push_str(&format!(
        "# Mientras el menú graba una tecla\n{} = \"submap reset\"\n", // no-tr
        cadena(&format!(
            "submap[{}],Super+Ctrl+Alt+Shift+F24",
            binds::SUBMAP_GRABAR
        ))  // no-tr
    ));
    out.push_str(CAPAS);
    out
}

pub async fn aplicar(c: &Config) -> Result<(), String> {
    let destino = ruta();
    let con_grim = ["grim", "slurp", "wl-copy", "jq"] // no-tr
        .iter()
        .all(|p| glib::find_program_in_path(p).is_some());
    let nuevo = toml(c, con_grim);
    if std::fs::read_to_string(&destino).ok().as_deref() == Some(nuevo.as_str()) {
        return Ok(());
    }
    // Se valida aparte, antes de ponerlo donde Umbriel lo mira.
    let prueba = destino.with_extension("prueba.toml");
    escribir(&prueba, &nuevo)?;
    let valida = crate::comando::salida(&[
        "umbriel",
        "config",
        "validate",
        "-c",
        &prueba.to_string_lossy(),
    ])
    .await; // no-tr
    let _ = std::fs::remove_file(&prueba);
    valida.map_err(|e| {
        format!("la config de morgan para Umbriel quedó inválida ({e}); se dejó la de antes")
    })?;
    escribir(&destino, &nuevo)
}

fn escribir(ruta: &Path, texto: &str) -> Result<(), String> {
    if let Some(dir) = ruta.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // Primero a un temporal y después el cambio de nombre: Umbriel nunca lee uno a medias.
    let tmp = ruta.with_extension("tmp");
    std::fs::write(&tmp, texto).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, ruta).map_err(|e| format!("{}: {e}", ruta.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teclas_de_hyprland_a_umbriel() {
        assert_eq!(tecla("SUPER + space").as_deref(), Some("Super+space"));
        assert_eq!(
            tecla("SUPER + ALT + SHIFT + L").as_deref(),
            Some("Super+Alt+Shift+L")
        );
        assert_eq!(tecla("CTRL + Print").as_deref(), Some("Ctrl+Print"));
        assert_eq!(tecla("XF86AudioMute").as_deref(), Some("XF86AudioMute"));
        assert_eq!(tecla("SUPER + code:10"), None);
        assert_eq!(tecla(""), None);
    }

    #[test]
    fn los_defaults_son_toml_valido_con_las_capas() {
        let t = toml(&Config::default(), true);
        let v: toml::Table = toml::from_str(&t).expect("TOML válido");
        let binds = v["keybinds"].as_table().unwrap();
        assert_eq!(
            binds["Super+space"]["action"].as_str(),
            Some("spawn morganctl launcher toggle")
        );
        assert_eq!(
            binds["XF86AudioRaiseVolume"]["allow_when_locked"].as_bool(),
            Some(true)
        );
        assert!(binds["Print"]["action"].as_str().unwrap().contains("slurp"));
        assert_eq!(v["layer_rule"].as_array().unwrap().len(), 2);
        // Sin grim, sin capturas.
        let t = toml(&Config::default(), false);
        assert!(!t.contains("Print"));
    }

    #[test]
    fn un_modo_es_un_submap_con_escape() {
        let mut c = Config::default();
        c.atajos.modos.push(crate::config::Modo {
            name: "Ventanas".into(),
            key: "SUPER + R".into(),
            once: true,
            binds: vec![AtajoPropio {
                key: "H".into(),
                command: "umbriel msg window-focus-left".into(),
                ..Default::default()
            }],
        });
        let v: toml::Table = toml::from_str(&toml(&c, false)).unwrap();
        let binds = v["keybinds"].as_table().unwrap();
        assert_eq!(
            binds["Super+R"]["action"].as_str(),
            Some("submap morgan-mode-0")
        );
        assert_eq!(
            binds["submap[morgan-mode-0],Escape"].as_str(),
            Some("submap reset")
        );
        let h = &binds["submap[morgan-mode-0],H"];
        assert_eq!(
            h["action"].as_str(),
            Some("spawn umbriel msg window-focus-left")
        );
        assert_eq!(h["submap"].as_str(), Some("reset"));
    }
}
