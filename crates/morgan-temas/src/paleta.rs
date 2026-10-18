//! De la paleta de un tema a los roles de Material que piden los templates.
//!
//! Un tema curado no trae los ~40 roles de Material 3: trae su paleta oficial en un
//! vocabulario chico (`PALETA`), y los roles se derivan acá con una regla para todos. Lo que
//! la paleta no tiene (contenedores, "fixed") se mezcla entre colores de la propia paleta:
//! ningún hex sale de la nada. Los 16 ANSI son la excepción: salen de la paleta de terminal
//! oficial del tema (`terminal`), que es lo que se espera al elegir Dracula.
//!
//! El tema generado trae los roles de matugen; acá solo se le agregan los 16 ANSI.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// Lo que un `paleta.toml` tiene que definir. Los fondos van del más oscuro al más claro.
pub const PALETA: [&str; 15] = [
    "fondo0", "fondo1", "fondo", "fondo2", "fondo3", "fondo4", "texto", "texto2", "apagado",
    "primario", "secundario", "terciario", "rojo", "amarillo", "verde",
];

/// Los 16 ANSI del tema generado (Material no tiene "rojo" ni "verde": van los roles de
/// contraste parecido).
const TERM_MATUGEN: [&str; 16] = [
    "surface_container_highest", "error", "primary", "tertiary", "secondary",
    "primary_fixed_dim", "secondary_fixed_dim", "on_surface", "outline", "error", "primary",
    "tertiary", "secondary", "primary_fixed_dim", "secondary_fixed_dim", "on_surface",
];

type Rgb = [u8; 3];

fn rgb(hex: &str) -> Option<Rgb> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([c(0)?, c(2)?, c(4)?])
}

pub fn mezcla(a: &str, b: &str, t: f64) -> String {
    let (a, b) = (rgb(a).unwrap_or_default(), rgb(b).unwrap_or_default());
    let mut s = String::from("#");
    for i in 0..3 {
        let v = (f64::from(a[i]) * t + f64::from(b[i]) * (1.0 - t)).round_ties_even();
        s.push_str(&format!("{:02x}", v as u8));
    }
    s
}

/// Un valor de la paleta: `"#hex"`, el nombre de otro color, o `[a, b, t]` (una mezcla).
fn resolver(p: &toml::Table, v: &toml::Value, vistos: &[&str]) -> Result<String, String> {
    match v {
        toml::Value::Array(m) if m.len() == 3 => {
            let t = match &m[2] {
                toml::Value::Float(f) => *f,
                toml::Value::Integer(i) => *i as f64,
                otro => return Err(format!("proporción inválida: {otro}")),
            };
            Ok(mezcla(&resolver(p, &m[0], vistos)?, &resolver(p, &m[1], vistos)?, t))
        }
        toml::Value::String(s) if s.starts_with('#') => match rgb(s) {
            Some(_) => Ok(s.to_ascii_lowercase()),
            None => Err(format!("color inválido: {s:?}")),
        },
        toml::Value::String(s) => match p.get(s) {
            Some(otro) if !vistos.contains(&s.as_str()) => {
                let mut v = vistos.to_vec();
                v.push(s);
                resolver(p, otro, &v)
            }
            _ => Err(format!("color sin resolver: {s:?}")),
        },
        otro => Err(format!("color sin resolver: {otro}")),
    }
}

pub fn roles_curado(texto: &str) -> Result<BTreeMap<String, String>, String> {
    let p: toml::Table = texto.parse().map_err(|e| format!("paleta.toml: {e}"))?;
    let faltan: Vec<&str> = PALETA.into_iter().filter(|k| !p.contains_key(*k)).collect();
    if !faltan.is_empty() {
        return Err(format!("a paleta.toml le faltan: {}", faltan.join(", ")));
    }
    let c = |k: &str| resolver(&p, &p[k], &[]);
    let (f0, f, tx) = (c("fondo0")?, c("fondo")?, c("texto")?);
    let primario = c("primario")?;
    let mut r: BTreeMap<String, String> = BTreeMap::new();
    let mut poner = |k: &str, v: &str| r.insert(k.to_owned(), v.to_owned());
    for (k, v) in [
        ("background", f.as_str()),
        ("surface", &f),
        ("surface_container_lowest", &f0),
        ("surface_dim", &f0),
        ("surface_container_low", &c("fondo1")?),
        ("surface_container", &c("fondo2")?),
        ("surface_container_high", &c("fondo3")?),
        ("surface_variant", &c("fondo3")?),
        ("surface_container_highest", &c("fondo4")?),
        ("surface_bright", &c("fondo4")?),
        ("on_background", &tx),
        ("on_surface", &tx),
        ("on_surface_variant", &c("texto2")?),
        ("outline", &c("apagado")?),
        ("outline_variant", &c("fondo4")?),
        ("inverse_surface", &tx),
        ("inverse_on_surface", &f),
        ("inverse_primary", &mezcla(&primario, &f, 0.5)),
        ("shadow", "#000000"),
        ("scrim", "#000000"),
        ("surface_tint", &primario),
        ("source_color", &primario),
        ("quota_ok", &c("verde")?),
        ("quota_warn", &c("amarillo")?),
        ("quota_crit", &c("rojo")?),
    ] {
        poner(k, v);
    }
    // La misma regla para los cuatro acentos, con los tonos de M3 como guía: el contenedor
    // es el acento hundido en el fondo; "fixed", el acento aclarado hacia el texto.
    for (rol, col) in [
        ("primary", primario.clone()),
        ("secondary", c("secundario")?),
        ("tertiary", c("terciario")?),
        ("error", c("rojo")?),
    ] {
        poner(rol, &col);
        poner(&format!("on_{rol}"), &f0);
        poner(&format!("{rol}_container"), &mezcla(&col, &f, 0.35));
        poner(&format!("on_{rol}_container"), &mezcla(&col, &tx, 0.5));
        if rol != "error" {
            poner(&format!("{rol}_fixed"), &mezcla(&col, &tx, 0.6));
            poner(&format!("{rol}_fixed_dim"), &col);
            poner(&format!("on_{rol}_fixed"), &f0);
            poner(&format!("on_{rol}_fixed_variant"), &mezcla(&col, &f0, 0.45));
        }
    }
    let term = match p.get("terminal") {
        Some(toml::Value::Array(t)) if t.len() == 16 => t,
        _ => return Err("`terminal` tiene que traer los 16 colores ANSI".into()),
    };
    for (i, v) in term.iter().enumerate() {
        poner(&format!("term{i}"), &resolver(&p, v, &[])?);
    }
    Ok(r)
}

/// Un tema curado es oscuro: las variantes `light` que piden algunos templates reciben los
/// mismos colores.
pub fn json_curado(roles: &BTreeMap<String, String>) -> Value {
    let colores: serde_json::Map<String, Value> = roles
        .iter()
        .map(|(k, v)| {
            let c = json!({ "color": v });
            (k.clone(), json!({ "default": c, "dark": c, "light": c }))
        })
        .collect();
    json!({ "colors": colores })
}

pub fn json_matugen(mut volcado: Value) -> Result<Value, String> {
    let colores = volcado
        .get_mut("colors")
        .and_then(Value::as_object_mut)
        .ok_or("el volcado de matugen no trae `colors`")?;
    for (i, rol) in TERM_MATUGEN.iter().enumerate() {
        let v = colores
            .get(*rol)
            .cloned()
            .ok_or_else(|| format!("el volcado de matugen no trae `{rol}`"))?;
        colores.insert(format!("term{i}"), v);
    }
    Ok(volcado)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NORD: &str = r##"
fondo0 = "#2e3440"
fondo1 = "#2e3440"
fondo  = "#2e3440"
fondo2 = "#3b4252"
fondo3 = "#434c5e"
fondo4 = "#4c566a"
texto   = "#D8DEE9"
texto2  = ["#d8dee9", "#4c566a", 0.7]
apagado = "fondo4"
primario   = "#88c0d0"
secundario = "#8fbcbb"
terciario  = "#81a1c1"
rojo     = "#bf616a"
amarillo = "#ebcb8b"
verde    = "#a3be8c"
terminal = [
  "#3b4252", "rojo", "verde", "amarillo", "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
  "#4c566a", "#bf616a", "#a3be8c", "#d08770", "#5e81ac", "#b48ead", "#8fbcbb", "#eceff4",
]
"##;

    #[test]
    fn mezcla_redondea_al_par_como_python() {
        // 64.5 se redondea al par.
        assert_eq!(mezcla("#800000", "#010000", 0.5), "#400000");
        assert_eq!(mezcla("#ffffff", "#000000", 1.0), "#ffffff");
        assert_eq!(mezcla("#ffffff", "#000000", 0.0), "#000000");
    }

    #[test]
    fn curado() {
        let r = roles_curado(NORD).unwrap();
        assert_eq!(r["on_surface"], "#d8dee9", "en minúsculas");
        assert_eq!(r["outline"], "#4c566a", "por nombre");
        assert_eq!(r["term1"], "#bf616a");
        // Valores de referencia para Nord.
        assert_eq!(r["on_surface_variant"], "#aeb5c3");
        assert_eq!(r["primary_container"], "#4e6572");
        assert_eq!(r["inverse_primary"], "#5b7a88");
        assert_eq!(r.len(), 69);
    }

    #[test]
    fn errores() {
        assert!(roles_curado("fondo0 = \"#000000\"").unwrap_err().contains("faltan"));
        let ciclo = NORD.replace("apagado = \"fondo4\"", "apagado = \"apagado\"");
        assert!(roles_curado(&ciclo).unwrap_err().contains("sin resolver"));
        let corto = NORD.replace("\"#eceff4\",", "");
        assert!(roles_curado(&corto).unwrap_err().contains("16"));
    }

    #[test]
    fn matugen_agrega_los_ansi() {
        let roles: serde_json::Map<String, Value> = TERM_MATUGEN
            .iter()
            .map(|r| (r.to_string(), json!({ "default": { "color": format!("#{r}") } })))
            .collect();
        let v = json_matugen(json!({ "colors": roles })).unwrap();
        assert_eq!(v["colors"]["term1"], v["colors"]["error"]);
        assert_eq!(v["colors"]["term15"], v["colors"]["on_surface"]);
        assert!(json_matugen(json!({})).is_err());
    }
}
