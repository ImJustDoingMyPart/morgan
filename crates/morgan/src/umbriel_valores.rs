//! Lo que vale cada clave de la config de Umbriel. Umbriel no tiene un `getoption` (ni su CLI
//! ni `umbriel msg` devuelven el valor vigente de una clave), así que se deduce de los mismos
//! archivos que lee él, con sus reglas (`docs/user/configuration.md`, «Include»):
//!
//! - los incluidos se aplican en el orden de la lista y el que los incluye, al final;
//! - las tablas se juntan por clave, las listas de reglas (`[[window_rule]]` y parientes)
//!   suman sus entradas, y los valores sueltos y las listas comunes los pone el último.
//!
//! La doc no dice en qué orden van `[include]` y `[include.optional]` entre sí: se toman en
//! ese, el de su ejemplo. Lo que ningún archivo pone es el default de `umbriel config schema`.
//!
//! Es una deducción, no una respuesta de Umbriel: si su config dejó de valer, él sigue con la
//! última buena y esto muestra la nueva.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::menu::compositor::Valor;

/// Un include que vuelve sobre sí mismo no da vueltas para siempre.
const PROFUNDIDAD: u32 = 8;

/// El archivo que Umbriel abre: el `config.toml` al lado de `morgan.toml`.
pub fn ruta_config() -> PathBuf {
    crate::umbriel_cfg::ruta().with_file_name("config.toml")
}

/// La config entera, ya juntada. Un archivo que falta o no es TOML no suma nada.
pub fn cargar(ruta: &Path) -> toml::Table {
    cargar_en(ruta, 0)
}

fn cargar_en(ruta: &Path, nivel: u32) -> toml::Table {
    let Some(mut propia) = std::fs::read_to_string(ruta)
        .ok()
        .and_then(|t| toml::from_str::<toml::Table>(&t).ok())
    else {
        return toml::Table::new();
    };
    let include = propia.remove("include");
    let mut out = toml::Table::new();
    if nivel < PROFUNDIDAD
        && let Some(toml::Value::Table(inc)) = include
    {
        let dir = ruta.parent().unwrap_or(Path::new("/"));
        let listas = [
            inc.get("files"),
            inc.get("optional")
                .and_then(|o| o.as_table())
                .and_then(|o| o.get("files")),
        ];
        for archivo in listas
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_array())
            .flatten()
            .filter_map(|v| v.as_str())
        {
            juntar(&mut out, cargar_en(&dir.join(expandir(archivo)), nivel + 1));
        }
    }
    juntar(&mut out, propia);
    out
}

/// `~`, `$VAR` y `${VAR}`, como Umbriel.
fn expandir(ruta: &str) -> String {
    let ruta = match ruta.strip_prefix("~/") {
        Some(resto) => format!("{}/{resto}", glib::home_dir().display()),
        None if ruta == "~" => glib::home_dir().display().to_string(),
        None => ruta.to_owned(),
    };
    let mut out = String::new();
    let mut resto = ruta.as_str();
    while let Some(i) = resto.find('$') {
        out.push_str(&resto[..i]);
        let tras = &resto[i + 1..];
        let (nombre, sigue) = if let Some(t) = tras.strip_prefix('{') {
            match t.find('}') {
                Some(f) => (&t[..f], &t[f + 1..]),
                None => (t, ""),
            }
        } else {
            let f = tras
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(tras.len());
            (&tras[..f], &tras[f..])
        };
        out.push_str(&std::env::var(nombre).unwrap_or_default());
        resto = sigue;
    }
    out.push_str(resto);
    out
}

/// `encima` sobre `base`: las tablas se juntan, las listas de tablas suman y el resto lo
/// reemplaza el de encima.
pub fn juntar(base: &mut toml::Table, encima: toml::Table) {
    for (k, v) in encima {
        match (base.get_mut(&k), v) {
            (Some(toml::Value::Table(b)), toml::Value::Table(e)) => juntar(b, e),
            (Some(toml::Value::Array(b)), toml::Value::Array(e))
                if es_de_reglas(b) && (e.is_empty() || es_de_reglas(&e)) =>
            {
                // `[]` descarta lo juntado antes.
                if e.is_empty() {
                    b.clear();
                } else {
                    b.extend(e);
                }
            }
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
}

fn es_de_reglas(v: &[toml::Value]) -> bool {
    !v.is_empty() && v.iter().all(|x| x.is_table())
}

/// El valor en `a.b.c`, si algún archivo lo pone.
pub fn buscar<'a>(t: &'a toml::Table, clave: &str) -> Option<&'a toml::Value> {
    let mut partes = clave.split('.');
    let mut v = t.get(partes.next()?)?;
    for p in partes {
        v = v.as_table()?.get(p)?;
    }
    Some(v)
}

pub fn valor(v: &toml::Value) -> Option<Valor> {
    match v {
        toml::Value::Boolean(b) => Some(Valor::Bool(*b)),
        toml::Value::Integer(i) => Some(Valor::Int(*i)),
        toml::Value::Float(f) => Some(Valor::Float(*f)),
        toml::Value::String(s) => Some(Valor::Str(s.clone())),
        _ => None,
    }
}

/// Los defaults de `umbriel config schema --json`, por clave.
pub fn defaults(schema: &str) -> BTreeMap<String, Valor> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(schema) else {
        return BTreeMap::new();
    };
    v.get("options")
        .and_then(|o| o.as_array())
        .into_iter()
        .flatten()
        .filter_map(|o| {
            let clave = o.get("path")?.as_str()?.to_owned();
            let d = o.get("default")?;
            let v = if let Some(b) = d.as_bool() {
                Valor::Bool(b)
            } else if let Some(i) = d.as_i64() {
                // Un float con default entero (`0.0`) llega como entero en JSON.
                if o.get("type").and_then(|t| t.as_str()) == Some("float") {
                    Valor::Float(i as f64)
                } else {
                    Valor::Int(i)
                }
            } else if let Some(f) = d.as_f64() {
                Valor::Float(f)
            } else {
                Valor::Str(d.as_str()?.to_owned())
            };
            Some((clave, v))
        })
        .collect()
}

/// Lo que vale cada una de `claves`: lo de los archivos, si no el default.
pub fn vigentes<'a>(
    config: &toml::Table,
    defaults: &BTreeMap<String, Valor>,
    claves: impl Iterator<Item = &'a str>,
) -> BTreeMap<String, Valor> {
    claves
        .filter_map(|c| {
            let v = buscar(config, c)
                .and_then(valor)
                .or_else(|| defaults.get(c).cloned())?;
            Some((c.to_owned(), v))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escribir(dir: &Path, nombre: &str, texto: &str) {
        std::fs::write(dir.join(nombre), texto).unwrap();
    }

    #[test]
    fn el_ultimo_de_la_lista_gana_y_el_que_incluye_mas() {
        let dir = std::env::temp_dir().join(format!("morgan-umbriel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        escribir(
            &dir,
            "config.toml",
            "[include.optional]\nfiles = [\"user.toml\", \"morgan.toml\", \"falta.toml\"]\n",
        );
        escribir(
            &dir,
            "user.toml",
            "[appearance]\nborder_width = 2\ncorner_radius = 4\n\n[[window_rule]]\nmatch.app_id = \"a\"\n",
        );
        escribir(
            &dir,
            "morgan.toml",
            "[include.optional]\nfiles = [\"tema.toml\"]\n\n[appearance]\nborder_width = 3\n\n[[window_rule]]\nmatch.app_id = \"b\"\n",
        );
        escribir(&dir, "tema.toml", "[appearance]\ncorner_radius = 9\nborder_width = 7\n");
        let t = cargar(&dir.join("config.toml"));
        std::fs::remove_dir_all(&dir).unwrap();
        // tema.toml va antes que morgan.toml (que lo incluye), y user.toml antes que los dos.
        assert_eq!(buscar(&t, "appearance.border_width").and_then(valor), Some(Valor::Int(3)));
        assert_eq!(buscar(&t, "appearance.corner_radius").and_then(valor), Some(Valor::Int(9)));
        assert_eq!(t["window_rule"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn defaults_del_schema() {
        let schema = r#"{"options":[
            {"path":"appearance.border_width","type":"int","default":2},
            {"path":"animation.dim_unfocused.dim","type":"float","default":0},
            {"path":"input.mouse.natural_scroll","type":"bool"},
            {"path":"layout.mode","type":"enum","default":"scrolling"}]}"#;
        let d = defaults(schema);
        assert_eq!(d["appearance.border_width"], Valor::Int(2));
        assert_eq!(d["animation.dim_unfocused.dim"], Valor::Float(0.0));
        assert_eq!(d["layout.mode"], Valor::Str("scrolling".into()));
        assert!(!d.contains_key("input.mouse.natural_scroll"));
        let v = vigentes(
            &toml::from_str("[layout]\nmode = \"dwindle\"\n").unwrap(),
            &d,
            ["layout.mode", "appearance.border_width", "input.mouse.natural_scroll"].into_iter(),
        );
        assert_eq!(v["layout.mode"], Valor::Str("dwindle".into()));
        assert_eq!(v["appearance.border_width"], Valor::Int(2));
        assert!(!v.contains_key("input.mouse.natural_scroll"));
    }

    #[test]
    fn variables_en_las_rutas() {
        // SAFETY: los tests de este módulo no leen esta variable desde otro hilo.
        unsafe { std::env::set_var("MORGAN_PRUEBA_DIR", "/x") };
        assert_eq!(expandir("$MORGAN_PRUEBA_DIR/a.toml"), "/x/a.toml");
        assert_eq!(expandir("${MORGAN_PRUEBA_DIR}b/c"), "/xb/c");
        assert!(expandir("~/u.toml").ends_with("/u.toml"));
        assert!(!expandir("~/u.toml").starts_with('~'));
    }
}
