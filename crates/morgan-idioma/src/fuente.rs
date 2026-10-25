//! Un lexer chico de Rust para los tests: no entiende la gramática; alcanza con saber qué es
//! un literal, en qué línea está, qué macro lo envuelve y si ya empezó el módulo de tests.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Los archivos (relativos a `crates/`) donde todo texto visible ya pasa por la tabla. Uno
/// que se traduce entra acá, y desde entonces el test no deja agregarle textos sueltos.
const TRADUCIDOS: &[&str] = &[
    "morgan/src/menu/arbol.rs",
    "morgan/src/launcher/binds.rs",
    "morgan/src/centro/mod.rs",
    "morgan/src/setup.rs",
    "morgan/src/polkit/mod.rs",
    "morgan/src/polkit/ayudante.rs",
    "morgan/src/polkit/secreto.rs",
    "morgan/src/centro/dibujo/permiso.rs",
];

/// Lo que va dentro de estas macros no se traduce: es para el journal o para quien programa.
const SIN_TRADUCIR: &[&str] = &[
    "eprintln", "eprint", "panic", "unreachable", "todo", "unimplemented", "assert",
    "assert_eq", "assert_ne", "debug_assert", "debug_assert_eq", "debug_assert_ne", "expect",
    "include_str", "concat", "env", "matches",
];

const MARCAS: &[&str] = &["tr", "trf", "tr_const"];

#[derive(Debug, Clone)]
pub struct Literal {
    pub linea: usize,
    pub texto: String,
    pub marca: Option<&'static str>,
    pub exento: bool,
    pub en_tests: bool,
    pub no_tr: bool,
    pub crudo: bool,
}

fn es_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn string(c: &[char], mut i: usize, linea: &mut usize) -> (String, usize) {
    let mut s = String::new();
    while i < c.len() {
        match c[i] {
            '"' => return (s, i + 1),
            '\\' if i + 1 < c.len() => {
                i += 1;
                match c[i] {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    'r' => s.push('\r'),
                    '0' => s.push('\0'),
                    '\\' => s.push('\\'),
                    '"' => s.push('"'),
                    '\'' => s.push('\''),
                    '\n' => {
                        // Continuación de línea: se come el blanco de la siguiente.
                        *linea += 1;
                        while i + 1 < c.len() && c[i + 1].is_whitespace() {
                            if c[i + 1] == '\n' {
                                *linea += 1;
                            }
                            i += 1;
                        }
                    }
                    'u' => {
                        let fin = (i..c.len()).find(|&j| c[j] == '}').unwrap_or(i);
                        let hex: String = c[i + 2..fin].iter().collect();
                        if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            s.push(ch);
                        }
                        i = fin;
                    }
                    'x' => {
                        let hex: String = c[i + 1..(i + 3).min(c.len())].iter().collect();
                        if let Ok(b) = u8::from_str_radix(&hex, 16) {
                            s.push(b as char);
                        }
                        i += 2;
                    }
                    otro => s.push(otro),
                }
            }
            '\n' => {
                *linea += 1;
                s.push('\n');
            }
            ch => s.push(ch),
        }
        i += 1;
    }
    (s, i)
}

struct Abierta {
    nivel: usize,
    nombre: String,
}

pub fn literales(fuente: &str) -> Vec<Literal> {
    let c: Vec<char> = fuente.chars().collect();
    let lineas: Vec<&str> = fuente.lines().collect();
    let no_tr = |l: usize| lineas.get(l - 1).is_some_and(|s| s.trim_end().ends_with("// no-tr"));
    let mut v = Vec::new();
    let (mut i, mut linea, mut nivel) = (0, 1, 0usize);
    let mut macros: Vec<Abierta> = Vec::new();
    let mut pendiente: Option<String> = None;
    let mut en_atributo: Option<usize> = None;
    let mut en_tests = false;
    let mut cfg_test = false;
    let mut ultimo_ident = String::new();
    while i < c.len() {
        let ch = c[i];
        match ch {
            '\n' => linea += 1,
            '/' if c.get(i + 1) == Some(&'/') => {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' if c.get(i + 1) == Some(&'*') => {
                let mut prof = 0;
                while i < c.len() {
                    if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                        prof += 1;
                        i += 1;
                    } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                        prof -= 1;
                        i += 1;
                        if prof == 0 {
                            break;
                        }
                    } else if c[i] == '\n' {
                        linea += 1;
                    }
                    i += 1;
                }
            }
            '"' => {
                let inicio = linea;
                let (texto, sig) = string(&c, i + 1, &mut linea);
                empujar(&mut v, &macros, nivel, en_atributo.is_some(), en_tests, inicio, texto, false, &no_tr);
                i = sig;
                pendiente = None;
                continue;
            }
            '\'' => {
                // Un char (`'x'`, `'\n'`, `'\u{1F600}'`) o un lifetime (`'a`).
                if c.get(i + 1) == Some(&'\\') {
                    let mut j = i + 3;
                    while j < c.len() && c[j] != '\'' {
                        j += 1;
                    }
                    i = j + 1;
                    continue;
                }
                if c.get(i + 2) == Some(&'\'') {
                    i += 3;
                    continue;
                }
            }
            '#' if c.get(i + 1) == Some(&'[') || (c.get(i + 1) == Some(&'!') && c.get(i + 2) == Some(&'[')) => {
                let resto: String = c[i..(i + 14).min(c.len())].iter().collect();
                cfg_test |= resto.starts_with("#[cfg(test)]");
                en_atributo = Some(nivel);
            }
            '(' | '[' | '{' => {
                nivel += 1;
                if let Some(n) = pendiente.take() {
                    macros.push(Abierta { nivel, nombre: n });
                }
            }
            ')' | ']' | '}' => {
                if macros.last().is_some_and(|m| m.nivel == nivel) {
                    macros.pop();
                }
                nivel = nivel.saturating_sub(1);
                if ch == ']' && en_atributo == Some(nivel) {
                    en_atributo = None;
                }
            }
            '!' if !ultimo_ident.is_empty() && c.get(i + 1) != Some(&'=') => {
                pendiente = Some(std::mem::take(&mut ultimo_ident));
                i += 1;
                continue;
            }
            ch if es_ident(ch) && (i == 0 || !es_ident(c[i - 1])) => {
                let mut j = i;
                while j < c.len() && es_ident(c[j]) {
                    j += 1;
                }
                let palabra: String = c[i..j].iter().collect();
                // Raw strings y byte strings: `r"…"`, `r#"…"#`, `b"…"`, `br"…"`.
                if matches!(palabra.as_str(), "r" | "br") && matches!(c.get(j), Some('"' | '#')) {
                    let mut hashes = 0;
                    while c.get(j + hashes) == Some(&'#') {
                        hashes += 1;
                    }
                    if c.get(j + hashes) == Some(&'"') {
                        let inicio = linea;
                        let mut k = j + hashes + 1;
                        let cierre: Vec<char> = std::iter::once('"').chain(std::iter::repeat_n('#', hashes)).collect();
                        while k < c.len() && !c[k..].starts_with(&cierre) {
                            if c[k] == '\n' {
                                linea += 1;
                            }
                            k += 1;
                        }
                        let texto: String = c[j + hashes + 1..k.min(c.len())].iter().collect();
                        if palabra == "r" {
                            empujar(&mut v, &macros, nivel, en_atributo.is_some(), en_tests, inicio, texto, true, &no_tr);
                        }
                        i = k + cierre.len();
                        pendiente = None;
                        continue;
                    }
                }
                if palabra == "b" && c.get(j) == Some(&'"') {
                    let (_, sig) = string(&c, j + 1, &mut linea);
                    i = sig;
                    continue;
                }
                if en_atributo.is_none() {
                    en_tests |= palabra == "mod" && cfg_test;
                    cfg_test = false;
                }
                pendiente = None;
                ultimo_ident = palabra;
                // Una llamada a método `.expect(…)` cuenta como macro exenta.
                if ultimo_ident == "expect" && i > 0 && c[i - 1] == '.' {
                    pendiente = Some("expect".into());
                }
                i = j;
                continue;
            }
            ch if ch.is_whitespace() => {
                ultimo_ident.clear();
                i += 1;
                continue;
            }
            _ => {}
        }
        if !matches!(ch, '!' | '(' | '[' | '{') {
            pendiente = pendiente.filter(|_| ch.is_whitespace());
        }
        ultimo_ident.clear();
        i += 1;
    }
    v
}

#[allow(clippy::too_many_arguments)]
fn empujar(
    v: &mut Vec<Literal>,
    macros: &[Abierta],
    nivel: usize,
    atributo: bool,
    en_tests: bool,
    linea: usize,
    texto: String,
    crudo: bool,
    no_tr: &dyn Fn(usize) -> bool,
) {
    let directa = macros.last().filter(|m| m.nivel == nivel);
    let marca = directa.and_then(|m| MARCAS.iter().find(|n| **n == m.nombre)).copied();
    let exento = atributo || macros.iter().any(|m| SIN_TRADUCIR.contains(&m.nombre.as_str()));
    v.push(Literal { linea, texto, marca, exento, en_tests, no_tr: no_tr(linea), crudo });
}

/// ¿Parece un texto para una persona? Con espacio y letras, con letras del castellano, o
/// una palabra con mayúscula inicial ("Apagar"). Rutas, clases de CSS, nombres de íconos y
/// comandos de una palabra no.
pub fn parece_texto(s: &str) -> bool {
    let letras = s.chars().filter(|c| c.is_alphabetic()).count();
    if letras < 2 {
        return false;
    }
    if s.starts_with(['/', '~', '.', '#', '-', '$', '@']) || s.contains("::") || s.contains("://") {
        return false;
    }
    if s.contains(['á', 'é', 'í', 'ó', 'ú', 'ñ', 'ü', '¿', '¡', 'Á', 'É', 'Í', 'Ó', 'Ú', 'Ñ']) {
        return true;
    }
    if s.contains(' ') {
        return true;
    }
    let mut cs = s.chars();
    cs.next().is_some_and(|c| c.is_uppercase()) && cs.clone().count() > 0 && cs.all(|c| c.is_lowercase())
}

pub fn marcadores(s: &str) -> Result<BTreeMap<usize, usize>, String> {
    let mut v = BTreeMap::new();
    let mut siguiente = 0;
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        match c {
            '{' if it.peek().is_some_and(|&(_, d)| d == '{') => {
                it.next();
            }
            '}' if it.peek().is_some_and(|&(_, d)| d == '}') => {
                it.next();
            }
            '{' => {
                let fin = s[i..].find('}').ok_or("llave sin cerrar")?;
                let dentro = &s[i + 1..i + fin];
                let n = if dentro.is_empty() {
                    siguiente += 1;
                    siguiente - 1
                } else {
                    dentro.parse().map_err(|_| format!("`{{{dentro}}}`: solo `{{}}` o `{{N}}`"))?
                };
                *v.entry(n).or_insert(0) += 1;
                while it.peek().is_some_and(|&(j, _)| j <= i + fin) {
                    it.next();
                }
            }
            _ => {}
        }
    }
    Ok(v)
}

fn archivos(dir: &Path, v: &mut Vec<PathBuf>) {
    let Ok(it) = std::fs::read_dir(dir) else { return };
    for e in it.flatten() {
        let p = e.path();
        if p.is_dir() {
            archivos(&p, v);
        } else if p.extension().is_some_and(|x| x == "rs") {
            v.push(p);
        }
    }
}

fn crates() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn fuentes() -> Vec<(String, String)> {
    let raiz = crates();
    let mut v = Vec::new();
    for e in std::fs::read_dir(&raiz).unwrap().flatten() {
        archivos(&e.path().join("src"), &mut v);
    }
    v.sort();
    v.into_iter()
        .map(|p| {
            let rel = p.strip_prefix(&raiz).unwrap().to_string_lossy().into_owned();
            (rel, std::fs::read_to_string(&p).unwrap())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexer() {
        let f = r##"
// tr!("comentario")
/* tr!("bloque") */
fn f<'a>(x: &'a str) -> char {
    let a = tr!("Apagar");
    let b = trf!("Hace {} min", 5);
    eprintln!("morgan: {}", tr!("dentro"));
    let c = '"';
    let d = r#"crudo "con" comillas"#;
    let e = "Bloquear \
             pantalla";
    let g = x.expect("no puede fallar");
    h(tr_const!("Constante"), "Suelto"); // no-tr
    '\''
}
#[derive(Debug)]
struct S;
#[cfg(test)]
mod tests {
    const X: &str = "en tests";
}
"##;
        let v = literales(f);
        let textos: Vec<&str> = v.iter().map(|l| l.texto.as_str()).collect();
        assert_eq!(
            textos,
            [
                "Apagar", "Hace {} min", "morgan: {}", "dentro", "crudo \"con\" comillas",
                "Bloquear pantalla", "no puede fallar", "Constante", "Suelto", "en tests"
            ]
        );
        let m: Vec<Option<&str>> = v.iter().map(|l| l.marca).collect();
        assert_eq!(m[0], Some("tr"));
        assert_eq!(m[1], Some("trf"));
        assert_eq!(m[2], None);
        assert_eq!(m[3], Some("tr"), "una marca dentro de eprintln sigue siendo marca");
        assert!(v[2].exento && v[6].exento);
        assert!(v[4].crudo && !v[4].exento);
        assert_eq!(v[5].linea, 10);
        assert_eq!(m[7], Some("tr_const"));
        assert!(v[8].no_tr && v[8].marca.is_none());
        assert!(v[9].en_tests && !v[8].en_tests);
    }

    #[test]
    fn heuristica() {
        for s in ["Apagar", "Cerrar sesión", "fondo de las superficies · {}", "días"] {
            assert!(parece_texto(s), "{s}");
        }
        for s in ["swaync-close-symbolic", "morgan", "/usr/share", "hypr/cfg/morgan.lua", "ok", "SIGUSR1", "x"] {
            assert!(!parece_texto(s), "{s}");
        }
    }

    #[test]
    fn llaves() {
        assert_eq!(marcadores("{} de {}").unwrap(), BTreeMap::from([(0, 1), (1, 1)]));
        assert_eq!(marcadores("{1} of {0}").unwrap(), BTreeMap::from([(0, 1), (1, 1)]));
        assert_eq!(marcadores("{{literal}}").unwrap(), BTreeMap::new());
        assert!(marcadores("{:.0}").is_err());
        assert!(marcadores("{nombre}").is_err());
    }

    #[test]
    fn tabla_completa() {
        let tabla: toml::Table = crate::TABLA_EN.parse().expect("en.toml no es TOML válido");
        let mut usados = BTreeSet::new();
        let mut errores = Vec::new();
        for (rel, texto) in fuentes() {
            if rel.starts_with("morgan-idioma/") {
                continue;
            }
            for l in literales(&texto) {
                let Some(marca) = l.marca else { continue };
                let donde = format!("{rel}:{}", l.linea);
                if l.crudo {
                    errores.push(format!("{donde}: {marca}! con un raw string"));
                    continue;
                }
                usados.insert(l.texto.clone());
                let es = match marcadores(&l.texto) {
                    Ok(m) => m,
                    Err(e) => {
                        errores.push(format!("{donde}: {e}"));
                        continue;
                    }
                };
                if marca != "trf" && !es.is_empty() {
                    errores.push(format!("{donde}: {{}} en {marca}!, va trf!"));
                }
                match tabla.get(&l.texto).and_then(toml::Value::as_str) {
                    None => errores.push(format!("{donde}: falta en en.toml: {:?}", l.texto)),
                    Some("") => errores.push(format!("{donde}: vacío en en.toml: {:?}", l.texto)),
                    Some(en) => {
                        let posiciones: BTreeSet<usize> = es.keys().copied().collect();
                        let en_pos = marcadores(en).map(|m| m.keys().copied().collect::<BTreeSet<_>>());
                        if en_pos.as_ref() != Ok(&posiciones) {
                            errores.push(format!("{donde}: los {{}} no coinciden: {:?} → {en:?}", l.texto));
                        }
                    }
                }
            }
        }
        for k in tabla.keys() {
            if !usados.contains(k) {
                errores.push(format!("en.toml: {k:?} ya no se usa"));
            }
        }
        assert!(errores.is_empty(), "\n{}\n", errores.join("\n"));
    }

    /// Cuántos textos sueltos quedan en cada archivo que todavía no está en `TRADUCIDOS`:
    /// `cargo test -p morgan-idioma inventario -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn inventario() {
        let mut total = 0;
        for (rel, texto) in fuentes() {
            if TRADUCIDOS.contains(&rel.as_str()) || rel.starts_with("morgan-idioma/") {
                continue;
            }
            let n = literales(&texto)
                .iter()
                .filter(|l| l.marca.is_none() && !l.exento && !l.en_tests && !l.no_tr && parece_texto(&l.texto))
                .count();
            if n > 0 {
                println!("{n:5}  {rel}");
                total += n;
            }
        }
        println!("{total:5}  en total");
    }

    /// En los archivos de `TRADUCIDOS`, ningún texto visible queda sin marcar. Uno que no es
    /// para mostrar (un comando, una línea de Lua) lleva `// no-tr` al final de su línea.
    #[test]
    fn sin_textos_sueltos() {
        let mut errores = Vec::new();
        let todos = fuentes();
        for rel in TRADUCIDOS {
            let Some((_, texto)) = todos.iter().find(|(r, _)| r == rel) else {
                errores.push(format!("{rel}: no existe"));
                continue;
            };
            for l in literales(texto) {
                if l.marca.is_none() && !l.exento && !l.en_tests && !l.no_tr && parece_texto(&l.texto) {
                    errores.push(format!("{rel}:{}: {:?}", l.linea, l.texto));
                }
            }
        }
        assert!(errores.is_empty(), "textos sin tr!/trf!/tr_const! (o `// no-tr`):\n{}\n", errores.join("\n"));
    }
}
