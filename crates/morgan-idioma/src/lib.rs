//! Los textos de morgan en el idioma de la sesión.
//!
//! El fuente está en castellano y el texto en castellano es la clave: `tr!("Apagar")` da
//! "Apagar" o, en inglés, lo que diga `en.toml` para "Apagar". Así no hay claves inventadas
//! que mantener y el código se sigue leyendo igual. La tabla va compilada en el binario.
//!
//! - `tr!("…")`: el texto traducido (`&'static str`).
//! - `trf!("… {} …", a, …)`: lo mismo con valores; los `{}` van en orden, y la traducción
//!   puede reordenarlos con `{0}`, `{1}`. Sin especificadores de formato: el valor llega ya
//!   formateado.
//! - `tr_const!("…")`: marca un texto en una constante (donde no se puede llamar a nada) y lo
//!   deja igual; se traduce al mostrarlo, con `t`.
//!
//! El test `tabla_completa` exige que cada texto marcado esté en `en.toml` con los mismos
//! `{}`, y que la tabla no tenga textos que ya nadie usa.
//!
//! El idioma se decide una vez, al primer texto: `MORGAN_IDIOMA`, después `[shell] language` de
//! la config de morgan y, si dice `auto` (o no dice), el locale (`LC_ALL`, `LC_MESSAGES`,
//! `LANG`). Lo que no es castellano es inglés.

use std::collections::HashMap;
use std::fmt::Display;
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Idioma {
    Es,
    En,
}

impl Idioma {
    pub fn de_config(v: &str) -> Option<Option<Idioma>> {
        match v {
            "" | "auto" => Some(None),
            _ => Idioma::de_codigo(v).map(Some),
        }
    }

    fn de_codigo(v: &str) -> Option<Idioma> {
        let base = v.split(['_', '.', '@', '-']).next().unwrap_or_default();
        match base {
            "es" => Some(Idioma::Es),
            "en" => Some(Idioma::En),
            _ => None,
        }
    }
}

const TABLA_EN: &str = include_str!("en.toml");

static ACTUAL: OnceLock<Idioma> = OnceLock::new();
static INGLES: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

pub fn actual() -> Idioma {
    *ACTUAL.get_or_init(|| elegir(&|v| std::env::var(v).ok(), leer_config()))
}

fn elegir(env: &dyn Fn(&str) -> Option<String>, config: Option<String>) -> Idioma {
    let puesto = |v: &str| env(v).filter(|s| !s.is_empty());
    if let Some(i) = puesto("MORGAN_IDIOMA").and_then(|v| Idioma::de_codigo(&v)) {
        return i;
    }
    if let Some(Some(i)) = config.as_deref().and_then(Idioma::de_config) {
        return i;
    }
    // `LC_ALL` tapa a `LC_MESSAGES`, que tapa a `LANG` (POSIX); el primero puesto decide.
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(puesto)
        .and_then(|v| Idioma::de_codigo(&v))
        .unwrap_or(Idioma::En)
}

/// `[shell] language` de `$XDG_CONFIG_HOME/morgan/config.toml`, sin depender del struct de
/// config de morgan: `morganctl` y `morgan-lock` también traducen.
fn leer_config() -> Option<String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    let texto = std::fs::read_to_string(base.join("morgan/config.toml")).ok()?;
    let t: toml::Table = texto.parse().ok()?;
    t.get("shell")?.get("language")?.as_str().map(str::to_owned)
}

fn ingles() -> &'static HashMap<&'static str, &'static str> {
    INGLES.get_or_init(|| {
        let tabla: toml::Table = TABLA_EN.parse().unwrap_or_default();
        tabla
            .into_iter()
            .filter_map(|(k, v)| {
                let v = v.as_str()?.to_owned();
                Some((&*Box::leak(k.into_boxed_str()), &*Box::leak(v.into_boxed_str())))
            })
            .collect()
    })
}

pub fn t(es: &'static str) -> &'static str {
    match actual() {
        Idioma::Es => es,
        Idioma::En => ingles().get(es).copied().unwrap_or(es),
    }
}

pub fn formatear(plantilla: &str, valores: &[&dyn Display]) -> String {
    let mut s = String::with_capacity(plantilla.len() + 16);
    let mut siguiente = 0;
    let mut it = plantilla.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        match c {
            '{' if it.peek().is_some_and(|&(_, d)| d == '{') => {
                it.next();
                s.push('{');
            }
            '}' if it.peek().is_some_and(|&(_, d)| d == '}') => {
                it.next();
                s.push('}');
            }
            '{' => {
                let resto = &plantilla[i + 1..];
                let Some(fin) = resto.find('}') else {
                    s.push(c);
                    continue;
                };
                let dentro = &resto[..fin];
                let n = if dentro.is_empty() {
                    siguiente += 1;
                    Some(siguiente - 1)
                } else {
                    dentro.parse::<usize>().ok()
                };
                match n.and_then(|n| valores.get(n)) {
                    Some(v) => {
                        use std::fmt::Write;
                        let _ = write!(s, "{v}");
                        for _ in 0..=fin {
                            it.next();
                        }
                    }
                    None => s.push(c),
                }
            }
            _ => s.push(c),
        }
    }
    s
}

#[macro_export]
macro_rules! tr {
    ($s:literal) => {
        $crate::t($s)
    };
}

#[macro_export]
macro_rules! trf {
    ($s:literal $(, $v:expr)* $(,)?) => {
        $crate::formatear($crate::t($s), &[$(&$v as &dyn ::std::fmt::Display),*])
    };
}

#[macro_export]
macro_rules! tr_const {
    ($s:literal) => {
        $s
    };
}

#[cfg(test)]
mod fuente;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eleccion() {
        let env = |pares: &'static [(&'static str, &'static str)]| {
            move |v: &str| pares.iter().find(|(k, _)| *k == v).map(|(_, x)| x.to_string())
        };
        let es = Idioma::Es;
        let en = Idioma::En;
        assert_eq!(elegir(&env(&[]), None), en, "sin nada, inglés");
        assert_eq!(elegir(&env(&[("LANG", "es_AR.UTF-8")]), None), es);
        assert_eq!(elegir(&env(&[("LANG", "de_DE.UTF-8")]), None), en);
        assert_eq!(elegir(&env(&[("LANG", "es_AR.UTF-8"), ("LC_ALL", "C")]), None), en);
        assert_eq!(elegir(&env(&[("LANG", "en_US"), ("LC_MESSAGES", "es_MX")]), None), es);
        assert_eq!(elegir(&env(&[("LANG", "es_AR"), ("LC_ALL", "")]), None), es, "vacía no cuenta");
        assert_eq!(elegir(&env(&[("LANG", "es_AR")]), Some("en".into())), en);
        assert_eq!(elegir(&env(&[("LANG", "es_AR")]), Some("auto".into())), es);
        assert_eq!(elegir(&env(&[("LANG", "es_AR")]), Some("xx".into())), es);
        assert_eq!(elegir(&env(&[("MORGAN_IDIOMA", "es")]), Some("en".into())), es);
    }

    #[test]
    fn formato() {
        assert_eq!(formatear("Hace {} min", &[&5]), "Hace 5 min");
        assert_eq!(formatear("{} de {}", &[&"a", &"b"]), "a de b");
        assert_eq!(formatear("{1} of {0}", &[&"a", &"b"]), "b of a");
        assert_eq!(formatear("{{x}} {}", &[&1]), "{x} 1");
        assert_eq!(formatear("{} y {}", &[&1]), "1 y {}");
        assert_eq!(formatear("ñandú {}", &[&"á"]), "ñandú á");
        assert_eq!(formatear("{nombre}", &[&1]), "{nombre}");
    }

    #[test]
    fn la_tabla_se_lee() {
        assert!(!ingles().is_empty());
        assert_eq!(ingles().get("Apagar").copied(), Some("Shut down"));
    }
}
