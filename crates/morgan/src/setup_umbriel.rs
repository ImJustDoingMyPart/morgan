//! La parte de Umbriel de `morgan setup`: que su `config.toml` sea un cargador que incluye
//! `user.toml` (la config escrita a mano) y después `morgan.toml` (la de morgan). Umbriel
//! aplica los incluidos en el orden de la lista y el último que pone una clave gana
//! (`configuration.md`, «Include»): así lo cambiado desde el menú de morgan pisa lo del
//! usuario, como `morgan.lua`, que `hyprland.lua` carga al final. Los dos son opcionales: si
//! uno todavía no existe, Umbriel arranca igual y lo carga cuando aparece.
//!
//! Una config que ya existe se mueve entera a `user.toml`. Sin config se parte de la
//! plantilla del paquete, con comentados los atajos que llaman a Noctalia (no está) y los que
//! chocan con los de morgan.

use std::path::Path;

use morgan_idioma::trf;

pub const ARCHIVO: &str = "morgan.toml"; // no-tr
pub const USUARIO: &str = "user.toml"; // no-tr
const PLANTILLA: &str = "/usr/share/umbriel/config.toml"; // no-tr

/// El `config.toml` que escribe `morgan setup`: solo la lista, en orden.
pub const CARGADOR: &str = concat!(
    "# Cargador: solo lista los archivos, en orden. Umbriel aplica los incluidos en el orden de la\n", // no-tr
    "# lista y el último que pone una clave gana (docs/user/configuration.md, «Include»):\n", // no-tr
    "#   morgan-atajos.toml  los atajos de ventanas de morgan, si se eligieron en morgan setup.\n", // no-tr
    "#   user.toml   la config escrita a mano; pisa a los atajos.\n", // no-tr
    "#   morgan.toml la escribe morgan: sus atajos, sus capas, el tema y lo cambiado desde su menú.\n", // no-tr
    "# Así morgan pisa solo lo que se cambió desde el menú, como morgan.lua en Hyprland.\n\n", // no-tr
    "[include.optional]\n", // no-tr
    "files = [\"morgan-atajos.toml\", \"user.toml\", \"morgan.toml\"]\n", // no-tr
);

/// `Mod+Space` y `Super+space` son la misma.
pub fn normalizar(tecla: &str) -> String {
    let mut mods = Vec::new();
    let mut resto = String::new();
    for p in tecla.split('+').map(str::trim) {
        match p.to_lowercase().as_str() {
            "mod" | "super" | "logo" | "win" => mods.push("super"), // no-tr
            "ctrl" | "control" => mods.push("ctrl"),                // no-tr
            "alt" => mods.push("alt"),                              // no-tr
            "shift" => mods.push("shift"),                          // no-tr
            otra => resto = otra.to_owned(),
        }
    }
    mods.sort_unstable();
    mods.push(&resto);
    mods.join("+")
}

/// La tecla de una línea `"Mod+Q" = …` de `[keybinds]`.
fn tecla_de(linea: &str) -> Option<&str> {
    let l = linea.trim_start().strip_prefix('"')?;
    let (tecla, resto) = l.split_once('"')?;
    resto.trim_start().starts_with('=').then_some(tecla)
}

/// La plantilla sin lo que no sirve con morgan: queda comentado, con el porqué.
pub fn desde_plantilla(plantilla: &str, de_morgan: &[String]) -> String {
    let ocupadas: Vec<String> = de_morgan.iter().map(|t| normalizar(t)).collect();
    let mut out = String::new();
    let mut en_atajos = false;
    for linea in plantilla.lines() {
        let t = linea.trim_start();
        if t.starts_with('[') {
            en_atajos = t.starts_with("[keybinds]"); // no-tr
        }
        let tecla = en_atajos.then(|| tecla_de(linea)).flatten();
        let choca = tecla.is_some_and(|k| ocupadas.contains(&normalizar(k)));
        if tecla.is_some() && (choca || linea.contains("noctalia")) {
            let porque = if choca { "es de morgan" } else { "Noctalia" }; // no-tr
            out.push_str(&format!("# ({porque}) {linea}\n"));
        } else {
            out.push_str(linea);
            out.push('\n');
        }
    }
    out
}

/// Un cargador sin los atajos de morgan, con el archivo agregado antes que `user.toml`; `None`
/// si ya lo tiene o no hay dónde ponerlo.
pub fn con_atajos(texto: &str) -> Option<String> {
    let atajos = format!("\"{}\"", crate::setup_atajos::TOML);
    if texto.contains(&atajos) || !texto.contains(&format!("\"{USUARIO}\"")) {
        return None;
    }
    Some(texto.replacen(&format!("\"{USUARIO}\""), &format!("{atajos}, \"{USUARIO}\""), 1)) // no-tr
}

/// Ya es el cargador: solo incluye, y `morgan.toml` va último.
pub fn es_cargador(texto: &str) -> bool {
    let Ok(t) = toml::from_str::<toml::Table>(texto) else {
        return false;
    };
    let ultimo = t
        .get("include")
        .and_then(|i| i.get("optional"))
        .and_then(|o| o.get("files"))
        .and_then(|f| f.as_array())
        .and_then(|f| f.last())
        .and_then(|f| f.as_str());
    t.keys().all(|k| k == "include") && ultimo == Some(ARCHIVO) // no-tr
}

/// La config del usuario sin `morgan.toml` entre sus includes: ahora lo incluye el cargador.
pub fn sin_morgan(texto: &str) -> String {
    let entrada = format!("\"{ARCHIVO}\"");
    texto
        .lines()
        .map(|l| {
            if l.trim_start().starts_with("files") && l.contains(&entrada) { // no-tr
                l.replace(&format!("{entrada}, "), "") // no-tr
                    .replace(&format!(", {entrada}"), "") // no-tr
                    .replace(&entrada, "")
            } else {
                l.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Las teclas que morgan ocupa en Umbriel, de lo que escribiría ahora.
fn teclas_de_morgan(c: &crate::config::Config) -> Vec<String> {
    let texto = crate::umbriel_cfg::toml(c, true);
    toml::from_str::<toml::Table>(&texto)
        .ok()
        .and_then(|t| t.get("keybinds").and_then(|k| k.as_table()).cloned())
        .map(|k| k.keys().cloned().collect())
        .unwrap_or_default()
}

/// `extra` son teclas de más que la plantilla deja comentadas (las de los atajos de morgan).
pub fn correr(c: &crate::config::Config, extra: &[String]) -> Result<(), String> {
    let dir = glib::user_config_dir().join("umbriel");
    let ruta = dir.join("config.toml");
    let usuario = dir.join(USUARIO);
    let propia = std::fs::read_to_string(&ruta).ok();
    if propia.as_deref().is_some_and(es_cargador) {
        println!("{}", trf!("{} ya carga morgan.", ruta.display()));
        return Ok(());
    }
    let texto = match &propia {
        Some(t) => sin_morgan(t),
        None => desde_plantilla(
            &std::fs::read_to_string(PLANTILLA).unwrap_or_default(),
            &[teclas_de_morgan(c), extra.to_vec()].concat(),
        ),
    };
    if toml::from_str::<toml::Table>(&texto).is_ok_and(|t| t.contains_key("drm")) {
        // La doc pide `[drm]` también en el archivo principal si un incluido lo define.
        return Err(trf!(
            "{} tiene [drm]: hay que pasarla a mano a {} y declarar [drm] en el cargador.",
            ruta.display(),
            usuario.display()
        ));
    }
    if usuario.exists() {
        return Err(trf!(
            "Ya existe {}: no se pisa. Juntalo a mano con {} y volvé a correr morgan setup.",
            usuario.display(),
            ruta.display()
        ));
    }
    escribir(&dir, &usuario, &texto)?;
    escribir(&dir, &ruta, CARGADOR)?;
    if propia.is_some() {
        println!(
            "{}",
            trf!(
                "Tu config de Umbriel pasó a {}; {} la carga y después la de morgan.",
                usuario.display(),
                ruta.display()
            )
        );
    } else {
        println!(
            "{}",
            trf!(
                "Creadas {} (de la plantilla) y {}, que la carga y después la de morgan.",
                usuario.display(),
                ruta.display()
            )
        );
    }
    Ok(())
}

fn escribir(dir: &Path, ruta: &Path, texto: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(ruta, texto).map_err(|e| format!("{}: {e}", ruta.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn misma_tecla_con_otro_nombre() {
        assert_eq!(normalizar("Mod+Space"), normalizar("Super+space"));
        assert_eq!(normalizar("Shift+Mod+Q"), normalizar("Super+Shift+q"));
        assert_ne!(normalizar("Mod+Q"), normalizar("Mod+Shift+Q"));
    }

    #[test]
    fn la_plantilla_sin_noctalia_ni_choques() {
        let plantilla = "[include]\nfiles = []\n\n[include.optional]\nfiles = []\n\n[keybinds]\n\
            \"Mod+Return\" = { action = \"spawn kitty\", repeat = false }\n\
            \"Mod\" = \"spawn noctalia msg panel-toggle launcher\"\n\
            \"Mod+Space\" = \"scratchpad-toggle\"\n\
            \"Mod+Tab\" = \"scratchpad-focus-next\"\n\n[layout]\nmode = \"scrolling\"\n";
        let t = desde_plantilla(plantilla, &["Super+space".into(), "Super+Tab".into()]);
        assert!(t.contains("\n\"Mod+Return\""));
        assert!(t.contains("# (Noctalia) \"Mod\" ="));
        assert!(t.contains("# (es de morgan) \"Mod+Space\""));
        assert!(t.contains("# (es de morgan) \"Mod+Tab\""));
        assert!(!t.contains(ARCHIVO));
        let v: toml::Table = toml::from_str(&t).unwrap();
        assert_eq!(v["layout"]["mode"].as_str(), Some("scrolling"));
    }

    #[test]
    fn el_cargador_y_la_config_propia() {
        assert!(es_cargador(CARGADOR));
        let v: toml::Table = toml::from_str(CARGADOR).unwrap();
        assert_eq!(
            v["include"]["optional"]["files"].as_array().unwrap(),
            &["morgan-atajos.toml", "user.toml", "morgan.toml"].map(toml::Value::from)
        );
        // Un cargador anterior a los atajos los recibe antes que user.toml, y una sola vez.
        let viejo = "[include.optional]\nfiles = [\"user.toml\", \"morgan.toml\"]\n";
        let nuevo = con_atajos(viejo).unwrap();
        assert!(nuevo.contains("[\"morgan-atajos.toml\", \"user.toml\", \"morgan.toml\"]"));
        assert!(es_cargador(&nuevo));
        assert_eq!(con_atajos(&nuevo), None);
        // Una config con valores propios no es el cargador, aunque incluya a morgan.
        let propia = "[include.optional]\nfiles = [\"morgan.toml\", \"otro.toml\"]\n[general]\nxwayland = false\n";
        assert!(!es_cargador(propia));
        // Con morgan primero, morgan no gana.
        assert!(!es_cargador("[include.optional]\nfiles = [\"morgan.toml\", \"user.toml\"]\n"));
        let sin = sin_morgan(propia);
        let v: toml::Table = toml::from_str(&sin).unwrap();
        assert_eq!(
            v["include"]["optional"]["files"].as_array().unwrap(),
            &vec![toml::Value::from("otro.toml")]
        );
        assert_eq!(v["general"]["xwayland"].as_bool(), Some(false));
        let solo = sin_morgan("[include.optional]\nfiles = [\"morgan.toml\"]\n");
        assert!(toml::from_str::<toml::Table>(&solo).is_ok());
        assert!(!solo.contains(ARCHIVO));
    }
}
