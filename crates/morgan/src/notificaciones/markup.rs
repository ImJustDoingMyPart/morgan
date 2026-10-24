//! El cuerpo de una notificación trae el subconjunto de HTML de la spec de freedesktop
//! (`b`, `i`, `u`, `a href`, `img`). GTK lo dibuja como markup de Pango, que rechaza el
//! texto entero ante un solo error. Acá se reescribe a markup siempre válido: quedan esas
//! etiquetas, balanceadas; las demás se sacan dejando su texto; `&` suelto, `<` y `>` se
//! escapan; `<br>` es salto de línea.

/// Etiquetas que pasan (las de la spec que Pango entiende; `img` no).
const SIMPLES: &[&str] = &["b", "i", "u"];

pub fn seguro(texto: &str) -> String {
    let mut out = String::with_capacity(texto.len());
    let mut abiertas: Vec<&'static str> = Vec::new();
    let mut resto = texto;
    while let Some(c) = resto.chars().next() {
        match c {
            '<' => match etiqueta(resto) {
                Some((tag, largo)) => {
                    aplicar(tag, &mut out, &mut abiertas);
                    resto = &resto[largo..];
                }
                None => {
                    out.push_str("&lt;");
                    resto = &resto[1..];
                }
            },
            '&' => {
                let largo = entidad(resto);
                if largo > 0 {
                    out.push_str(&resto[..largo]);
                    resto = &resto[largo..];
                } else {
                    out.push_str("&amp;");
                    resto = &resto[1..];
                }
            }
            '>' => {
                out.push_str("&gt;");
                resto = &resto[1..];
            }
            _ => {
                out.push(c);
                resto = &resto[c.len_utf8()..];
            }
        }
    }
    while let Some(t) = abiertas.pop() {
        out.push_str("</");
        out.push_str(t);
        out.push('>');
    }
    out
}

/// El texto de un markup sin etiquetas y con sus entidades resueltas, para copiar.
pub fn plano(m: &str) -> String {
    let mut out = String::with_capacity(m.len());
    let mut resto = m;
    while let Some(c) = resto.chars().next() {
        match c {
            '<' => match resto.find('>') {
                Some(f) => resto = &resto[f + 1..],
                None => {
                    out.push('<');
                    resto = &resto[1..];
                }
            },
            '&' => {
                let largo = entidad(resto);
                match (largo > 2).then(|| desescapar(&resto[1..largo - 1])).flatten() {
                    Some(d) => {
                        out.push(d);
                        resto = &resto[largo..];
                    }
                    None => {
                        out.push('&');
                        resto = &resto[1..];
                    }
                }
            }
            _ => {
                out.push(c);
                resto = &resto[c.len_utf8()..];
            }
        }
    }
    out
}

/// El carácter de una entidad (sin el `&` ni el `;`).
fn desescapar(nombre: &str) -> Option<char> {
    match nombre {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let n = nombre.strip_prefix('#')?;
            let v = match n.strip_prefix(['x', 'X']) {
                Some(h) => u32::from_str_radix(h, 16).ok()?,
                None => n.parse().ok()?,
            };
            char::from_u32(v)
        }
    }
}

/// Los enlaces se sacan antes de revisar la salida de [`seguro`]. El `href` va escapado, así
/// que no contiene `>`.
#[cfg(test)]
fn sin_enlaces(m: &str) -> String {
    let mut out = String::with_capacity(m.len());
    let mut resto = m;
    while let Some(i) = resto.find("<a href=\"").or_else(|| resto.find("</a>")) {
        let j = resto.find("</a>").filter(|j| *j < i).unwrap_or(i);
        out.push_str(&resto[..j]);
        let fin = resto[j..]
            .find('>')
            .map(|f| j + f + 1)
            .unwrap_or(resto.len());
        resto = &resto[fin..];
    }
    out.push_str(resto);
    out
}

#[derive(Debug, PartialEq)]
enum Tag {
    Abre(&'static str),
    Cierra(&'static str),
    Enlace(String),
    Salto,
    Otra,
}

fn etiqueta(s: &str) -> Option<(Tag, usize)> {
    let fin = s.find('>')?;
    let dentro = &s[1..fin];
    // `<` seguido de algo que no es nombre (`a < b`, `<3`) es texto.
    let cuerpo = dentro.strip_prefix('/').unwrap_or(dentro);
    if !cuerpo.starts_with(|c: char| c.is_ascii_alphabetic()) || dentro.contains('<') {
        return None;
    }
    let cierra = dentro.starts_with('/');
    let nombre: String = cuerpo
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    let tag = match (nombre.as_str(), cierra) {
        ("br", _) => Tag::Salto,
        ("a", true) => Tag::Cierra("a"),
        ("a", false) => match href(cuerpo) {
            Some(url) => Tag::Enlace(url),
            None => Tag::Otra,
        },
        (n, _) => match SIMPLES.iter().find(|s| **s == n) {
            Some(s) if cierra => Tag::Cierra(s),
            Some(s) => Tag::Abre(s),
            None => Tag::Otra,
        },
    };
    Some((tag, fin + 1))
}

fn href(atributos: &str) -> Option<String> {
    let i = atributos.find("href=")?;
    let valor = &atributos[i + 5..];
    let comilla = valor.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let valor = &valor[1..];
    let fin = valor.find(comilla)?;
    Some(valor[..fin].to_owned())
}

fn aplicar(tag: Tag, out: &mut String, abiertas: &mut Vec<&'static str>) {
    match tag {
        Tag::Abre(t) => {
            out.push('<');
            out.push_str(t);
            out.push('>');
            abiertas.push(t);
        }
        Tag::Enlace(url) => {
            out.push_str("<a href=\"");
            out.push_str(&escapar(&url));
            out.push_str("\">");
            abiertas.push("a");
        }
        // Un cierre sin su apertura se descarta; uno que cierra algo de más abajo cierra
        // también lo que quedó abierto encima (Pango no acepta cruces).
        Tag::Cierra(t) => {
            if let Some(pos) = abiertas.iter().rposition(|a| *a == t) {
                while abiertas.len() > pos {
                    let a = abiertas.pop().expect("pos < len");
                    out.push_str("</");
                    out.push_str(a);
                    out.push('>');
                }
            }
        }
        Tag::Salto => out.push('\n'),
        Tag::Otra => {}
    }
}

fn entidad(s: &str) -> usize {
    let Some(fin) = s[1..].find(';').map(|i| i + 1) else {
        return 0;
    };
    let nombre = &s[1..fin];
    let ok = match nombre.strip_prefix('#') {
        Some(n) => match n.strip_prefix(['x', 'X']) {
            Some(h) => !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit()),
            None => !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()),
        },
        None => matches!(nombre, "amp" | "lt" | "gt" | "quot" | "apos"),
    };
    if ok { fin + 1 } else { 0 }
}

fn escapar(s: &str) -> String {
    glib::markup_escape_text(s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lo que pide Pango: solo `b`, `i` y `u`, bien anidadas; las entidades, completas; ni `<`
    /// ni `>` sueltos.
    fn valido(m: &str) -> bool {
        let m = sin_enlaces(m);
        let mut abiertas: Vec<&str> = Vec::new();
        let mut resto = m.as_str();
        while let Some(c) = resto.chars().next() {
            match c {
                '<' => {
                    let Some(f) = resto.find('>') else { return false };
                    let tag = &resto[1..f];
                    match tag.strip_prefix('/') {
                        Some(t) => {
                            if abiertas.pop() != Some(t) {
                                return false;
                            }
                        }
                        None if SIMPLES.contains(&tag) => abiertas.push(tag),
                        None => return false,
                    }
                    resto = &resto[f + 1..];
                }
                '>' => return false,
                '&' => match entidad(resto) {
                    0 => return false,
                    l => resto = &resto[l..],
                },
                _ => resto = &resto[c.len_utf8()..],
            }
        }
        abiertas.is_empty()
    }

    #[test]
    fn texto_plano_sin_etiquetas() {
        assert_eq!(plano("<b>hola</b> &amp; <i>chau</i>"), "hola & chau");
        assert_eq!(plano("a &lt;b&gt; &#65;&#x42; &raro"), "a <b> AB &raro");
        assert_eq!(plano("sin cierre <"), "sin cierre <");
    }

    #[test]
    fn pasa_lo_de_la_spec() {
        assert_eq!(
            seguro("<b>hola</b> <i>y</i> <u>chau</u>"),
            "<b>hola</b> <i>y</i> <u>chau</u>"
        );
        assert_eq!(
            seguro("<a href=\"https://x.org/?a=1&b=2\">link</a>"),
            "<a href=\"https://x.org/?a=1&amp;b=2\">link</a>"
        );
    }

    #[test]
    fn saca_lo_demas_y_deja_el_texto() {
        assert_eq!(seguro("<span color='red'>rojo</span>"), "rojo");
        assert_eq!(seguro("<img src=\"x.png\" alt=\"x\"/>foto"), "foto");
        assert_eq!(seguro("uno<br>dos<br/>tres"), "uno\ndos\ntres");
    }

    #[test]
    fn escapa_lo_suelto() {
        assert_eq!(seguro("a < b && c > d"), "a &lt; b &amp;&amp; c &gt; d");
        assert_eq!(seguro("<3 te quiero"), "&lt;3 te quiero");
        assert_eq!(
            seguro("Tom &amp; Jerry &#38; &#x26;"),
            "Tom &amp; Jerry &#38; &#x26;"
        );
        assert_eq!(seguro("&nbsp;"), "&amp;nbsp;");
        assert_eq!(seguro("sin cierre <b"), "sin cierre &lt;b");
    }

    #[test]
    fn balancea() {
        assert_eq!(seguro("<b>abierta"), "<b>abierta</b>");
        assert_eq!(seguro("cerrada</b>"), "cerrada");
        assert_eq!(seguro("<b><i>cruce</b></i>"), "<b><i>cruce</i></b>");
        assert_eq!(seguro("<B>mayúscula</B>"), "<b>mayúscula</b>");
    }

    #[test]
    fn siempre_es_markup_valido() {
        for t in [
            "<b><i>x</b>",
            "</a>",
            "<a>sin href</a>",
            "<a href=\"x\">",
            "&&&;;<<>>",
            "<<b>>",
            "<b <i>",
            "ñandú <u>ü</u> 🦀",
            "<a href='y'>z</a>",
        ] {
            let m = seguro(t);
            assert!(valido(&m), "{t:?} -> {m:?}");
        }
    }
}
