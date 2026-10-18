//! Cada palabra tipeada tiene que aparecer en algún campo de la entrada, sin distinguir
//! mayúsculas ni tildes («musica» encuentra «Música»). El orden no depende de lo tipeado: lo
//! decide cada modo.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Entrada {
    pub id: String,
    #[allow(dead_code)]
    pub nombre: String,
    pub nombre_plegado: String,
    pub pajar: String,
}

impl Entrada {
    pub fn new(id: &str, nombre: &str, campos: &[&str]) -> Self {
        let nombre_plegado = plegar(nombre);
        let mut pajar = nombre_plegado.clone();
        for c in campos {
            pajar.push('\n');
            pajar.push_str(&plegar(c));
        }
        Entrada {
            id: id.into(),
            nombre: nombre.into(),
            nombre_plegado,
            pajar,
        }
    }

    fn coincide(&self, palabras: &[String]) -> bool {
        palabras.iter().all(|p| self.pajar.contains(p.as_str()))
    }
}

pub fn plegar_char(c: char) -> char {
    match c {
        'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
        'É' | 'È' | 'Ê' | 'Ë' | 'é' | 'è' | 'ê' | 'ë' => 'e',
        'Í' | 'Ì' | 'Î' | 'Ï' | 'í' | 'ì' | 'î' | 'ï' => 'i',
        'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
        'Ú' | 'Ù' | 'Û' | 'Ü' | 'ú' | 'ù' | 'û' | 'ü' => 'u',
        'Ç' | 'ç' => 'c',
        '\n' => ' ',
        c => c.to_lowercase().next().unwrap_or(c),
    }
}

/// El salto de línea separa campos: una palabra no puede empezar en uno y terminar en el
/// siguiente.
pub fn plegar(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            '\n' => ' ',
            c => c,
        })
        .collect()
}

#[derive(Clone, Copy)]
pub enum Orden<'a> {
    Uso(&'a Historial),
    Original,
}

pub fn filtrar(entradas: &[Entrada], consulta: &str, orden: Orden) -> Vec<usize> {
    let palabras: Vec<String> = plegar(consulta)
        .split_whitespace()
        .map(String::from)
        .collect();
    let mut r: Vec<usize> = (0..entradas.len())
        .filter(|&i| entradas[i].coincide(&palabras))
        .collect();
    if let Orden::Uso(historial) = orden {
        r.sort_by_cached_key(|&i| {
            let e = &entradas[i];
            (
                std::cmp::Reverse(historial.usos(&e.id)),
                &e.nombre_plegado,
                &e.id,
            )
        });
    }
    r
}

pub fn resaltar(texto: &str, consulta: &str) -> Vec<(usize, usize)> {
    // Plegado carácter a carácter, para que las posiciones sigan siendo las de `texto`.
    let t: Vec<char> = texto.chars().map(plegar_char).collect();
    let mut rangos = Vec::new();
    for p in plegar(consulta).split_whitespace() {
        let p: Vec<char> = p.chars().collect();
        if p.len() > t.len() {
            continue;
        }
        for i in 0..=t.len() - p.len() {
            if t[i..i + p.len()] == p[..] {
                rangos.push((i, i + p.len()));
            }
        }
    }
    rangos.sort_unstable();
    let mut juntos: Vec<(usize, usize)> = Vec::new();
    for (a, b) in rangos {
        match juntos.last_mut() {
            Some(u) if a <= u.1 => u.1 = u.1.max(b),
            _ => juntos.push((a, b)),
        }
    }
    juntos
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Historial {
    usos: HashMap<String, u32>,
}

impl Historial {
    /// Las líneas que no se entienden se saltean: el historial ordena, no es crítico.
    pub fn parse(texto: &str) -> Self {
        let mut usos = HashMap::new();
        for linea in texto.lines() {
            let Some((n, id)) = linea.trim().split_once(' ') else {
                continue;
            };
            let (Ok(n), id) = (n.parse::<u32>(), id.trim()) else {
                continue;
            };
            if n > 0 && !id.is_empty() {
                usos.insert(id.to_owned(), n);
            }
        }
        Historial { usos }
    }

    pub fn usos(&self, id: &str) -> u32 {
        self.usos.get(id).copied().unwrap_or(0)
    }

    pub fn sumar(&mut self, id: &str) {
        let n = self.usos.entry(id.to_owned()).or_default();
        *n = n.saturating_add(1);
    }

    /// A igual uso, por id, para que el archivo no cambie sin motivo.
    pub fn texto(&self) -> String {
        let mut v: Vec<_> = self.usos.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        v.into_iter().map(|(id, n)| format!("{n} {id}\n")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apps() -> Vec<Entrada> {
        vec![
            Entrada::new(
                "org.gnome.Nautilus.desktop",
                "Archivos",
                &["Gestor de archivos", "nautilus"],
            ),
            Entrada::new("vesktop.desktop", "Vesktop", &["Discord", "vesktop"]),
            Entrada::new("btop.desktop", "btop++", &["Monitor del sistema", "btop"]),
            Entrada::new(
                "musica.desktop",
                "Música",
                &["Reproductor", "Audio;Player", "lollypop"],
            ),
            Entrada::new("obsidian.desktop", "Obsidian", &["", "obsidian"]),
        ]
    }

    fn nombres(e: &[Entrada], r: &[usize]) -> Vec<String> {
        r.iter().map(|&i| e[i].nombre.clone()).collect()
    }

    #[test]
    fn sin_consulta_va_todo_alfabetico() {
        let e = apps();
        let r = filtrar(&e, "", Orden::Uso(&Historial::default()));
        assert_eq!(
            nombres(&e, &r),
            ["Archivos", "btop++", "Música", "Obsidian", "Vesktop"]
        );
    }

    #[test]
    fn lo_mas_usado_va_primero() {
        let e = apps();
        let h = Historial::parse("44 vesktop.desktop\n22 obsidian.desktop\n");
        let r = filtrar(&e, "", Orden::Uso(&h));
        assert_eq!(
            nombres(&e, &r),
            ["Vesktop", "Obsidian", "Archivos", "btop++", "Música"]
        );
    }

    #[test]
    fn el_orden_no_depende_de_lo_tipeado() {
        // Filtrar no reordena.
        let e = apps();
        let h = Historial::parse("3 obsidian.desktop\n");
        assert_eq!(
            nombres(&e, &filtrar(&e, "o", Orden::Uso(&h)))[0],
            "Obsidian"
        );
    }

    #[test]
    fn original_no_reordena() {
        let e = apps();
        let r = filtrar(&e, "", Orden::Original);
        assert_eq!(r, [0, 1, 2, 3, 4]);
        assert_eq!(filtrar(&e, "a", Orden::Original), [0, 2, 3, 4]);
    }

    #[test]
    fn busca_en_todos_los_campos() {
        let e = apps();
        let h = Historial::default();
        assert_eq!(
            nombres(&e, &filtrar(&e, "discord", Orden::Uso(&h))),
            ["Vesktop"]
        );
        assert_eq!(
            nombres(&e, &filtrar(&e, "nautilus", Orden::Uso(&h))),
            ["Archivos"]
        );
        assert_eq!(
            nombres(&e, &filtrar(&e, "player", Orden::Uso(&h))),
            ["Música"]
        );
    }

    #[test]
    fn sin_mayusculas_ni_tildes() {
        let e = apps();
        let h = Historial::default();
        assert_eq!(
            nombres(&e, &filtrar(&e, "MUSICA", Orden::Uso(&h))),
            ["Música"]
        );
        assert_eq!(
            nombres(&e, &filtrar(&e, "múSica", Orden::Uso(&h))),
            ["Música"]
        );
    }

    #[test]
    fn cada_palabra_tiene_que_estar() {
        let e = apps();
        let h = Historial::default();
        assert_eq!(
            nombres(&e, &filtrar(&e, "monitor btop", Orden::Uso(&h))),
            ["btop++"]
        );
        assert!(filtrar(&e, "monitor discord", Orden::Uso(&h)).is_empty());
    }

    #[test]
    fn una_palabra_no_cruza_campos() {
        // "vesktop" + "discord" pegados no tienen que encontrar "pdis".
        let e = apps();
        assert!(filtrar(&e, "pdis", Orden::Uso(&Historial::default())).is_empty());
    }

    #[test]
    fn resalta_por_caracteres_y_junta() {
        assert_eq!(resaltar("Música", "SICA"), [(2, 6)]);
        assert_eq!(resaltar("Música", "mu ica"), [(0, 2), (3, 6)]);
        assert_eq!(resaltar("Música", "usi sic"), [(1, 5)]);
        assert_eq!(resaltar("aaa", "a"), [(0, 3)]);
        assert!(resaltar("Música", "").is_empty());
        assert!(resaltar("ab", "abc").is_empty());
    }

    #[test]
    fn historial_lee_sus_lineas() {
        let h = Historial::parse(
            "44 vesktop.desktop\n0 nada.desktop\nbasura\nx y.desktop\n2 a b.desktop\n",
        );
        assert_eq!(h.usos("vesktop.desktop"), 44);
        assert_eq!(h.usos("nada.desktop"), 0);
        assert_eq!(h.usos("a b.desktop"), 2);
        assert_eq!(h.usos("otra.desktop"), 0);
    }

    #[test]
    fn historial_ida_y_vuelta() {
        let mut h = Historial::parse("2 b.desktop\n5 a.desktop\n");
        h.sumar("c.desktop");
        h.sumar("b.desktop");
        assert_eq!(h.texto(), "5 a.desktop\n3 b.desktop\n1 c.desktop\n");
        assert_eq!(Historial::parse(&h.texto()), h);
    }
}
