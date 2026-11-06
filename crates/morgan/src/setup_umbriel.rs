//! La parte de Umbriel de `morgan setup`: que su config incluya `morgan.toml` (opcional: si
//! todavía no existe, Umbriel arranca igual y lo carga cuando morgan lo escriba).
//!
//! Sin config propia se parte de la plantilla del paquete, sin los atajos que llaman a Noctalia
//! (no está) ni los que chocan con los de morgan: el archivo que incluye se aplica último, y
//! en la misma tecla ganaría sobre morgan.

use std::path::Path;

use morgan_idioma::trf;

pub const ARCHIVO: &str = "morgan.toml"; // no-tr
const PLANTILLA: &str = "/usr/share/umbriel/config.toml"; // no-tr

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
    con_include(&out).unwrap_or(out)
}

/// El texto con `morgan.toml` entre los opcionales; `None` si ya está.
pub fn con_include(texto: &str) -> Option<String> {
    if texto
        .lines()
        .any(|l| !l.trim_start().starts_with('#') && l.contains(ARCHIVO))
    {
        return None;
    }
    let lineas: Vec<&str> = texto.lines().collect();
    if let Some(i) = lineas.iter().position(|l| l.trim() == "[include.optional]") {
        // no-tr
        // El `files = [` de esa tabla, antes de la próxima.
        let fin = lineas[i + 1..]
            .iter()
            .position(|l| l.trim_start().starts_with('['))
            .map_or(lineas.len(), |p| i + 1 + p);
        if let Some(j) = (i + 1..fin).find(|&j| lineas[j].trim_start().starts_with("files")) {
            // no-tr
            let l = lineas[j];
            let abre = l.find('[')?;
            let resto = l[abre + 1..].trim_start();
            let sep = if resto.starts_with(']') { "" } else { ", " };
            let nueva = format!("{}[\"{ARCHIVO}\"{sep}{}", &l[..abre], resto);
            let mut v: Vec<String> = lineas.iter().map(|s| (*s).to_owned()).collect();
            v[j] = nueva;
            return Some(v.join("\n") + "\n");
        }
        let mut v: Vec<String> = lineas.iter().map(|s| (*s).to_owned()).collect();
        v.insert(i + 1, format!("files = [\"{ARCHIVO}\"]")); // no-tr
        return Some(v.join("\n") + "\n");
    }
    // Al final: arriba quedaría por encima de claves sueltas y se las llevaría a esta tabla.
    let mut t = texto.to_owned();
    if !t.is_empty() && !t.ends_with('\n') {
        t.push('\n');
    }
    t.push_str(&format!("\n# morgan: sus atajos y las reglas de sus capas.\n[include.optional]\nfiles = [\"{ARCHIVO}\"]\n")); // no-tr
    Some(t)
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

pub fn correr(c: &crate::config::Config) -> Result<(), String> {
    let dir = glib::user_config_dir().join("umbriel");
    let ruta = dir.join("config.toml");
    let (nuevo, que) = match std::fs::read_to_string(&ruta) {
        Ok(t) => match con_include(&t) {
            Some(n) => (
                n,
                trf!("Agregada la config de morgan a {}.", ruta.display()),
            ),
            None => {
                println!("{}", trf!("{} ya carga morgan.", ruta.display()));
                return Ok(());
            }
        },
        Err(_) => {
            let plantilla = std::fs::read_to_string(PLANTILLA).unwrap_or_default();
            (
                desde_plantilla(&plantilla, &teclas_de_morgan(c)),
                trf!("Creada {} con la config de morgan.", ruta.display()),
            )
        }
    };
    escribir(&dir, &ruta, &nuevo)?;
    println!("{que}");
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
    fn la_plantilla_sin_noctalia_ni_choques_y_con_el_include() {
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
        assert!(t.contains("[include.optional]\nfiles = [\"morgan.toml\"]"));
        let v: toml::Table = toml::from_str(&t).unwrap();
        assert_eq!(v["layout"]["mode"].as_str(), Some("scrolling"));
        assert_eq!(con_include(&t), None);
    }

    #[test]
    fn el_include_en_una_config_propia() {
        let con_lista = "[include.optional]\nfiles = [\"otro.toml\"]\n[general]\n";
        let t = con_include(con_lista).unwrap();
        assert!(t.contains("files = [\"morgan.toml\", \"otro.toml\"]"));
        let sin = "[general]\nxwayland = false\n";
        let t = con_include(sin).unwrap();
        let v: toml::Table = toml::from_str(&t).unwrap();
        assert_eq!(
            v["include"]["optional"]["files"][0].as_str(),
            Some("morgan.toml")
        );
        assert_eq!(v["general"]["xwayland"].as_bool(), Some(false));
        // Uno comentado no cuenta.
        assert!(con_include("# files = [\"morgan.toml\"]\n").is_some());
    }
}
