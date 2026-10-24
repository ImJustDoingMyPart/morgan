//! Los textos armados, guardados entre cuadros: cada cuadro rearma la hoja entera (posiciones y
//! colores), pero un texto se arma con parley solo la primera vez que aparece. Lo que no se usó
//! en un cuadro se suelta al terminarlo.

use std::collections::HashMap;
use std::rc::Rc;

use morgan_motor::parley::Layout;
use morgan_motor::texto::{self, Estilo, Textos};

#[derive(Clone, PartialEq, Eq, Hash)]
enum Clave {
    Linea {
        texto: String,
        px: u32,
        peso: u16,
        /// Con «…» si no entra (en píxeles del buffer), o entero.
        max: Option<u32>,
        /// Otra familia que la del shell.
        familia: Option<String>,
        /// El «…» al principio.
        al_inicio: bool,
    },
    /// Tramos con su rol y su tamaño, con «…» si no entran.
    Tramos {
        partes: Vec<(String, u8, u32)>,
        max: u32,
    },
    Parrafo {
        markup: String,
        px: u32,
        ancho: u32,
        lineas: usize,
    },
}

pub struct Rotulos {
    textos: Textos,
    escala: f32,
    guardados: HashMap<Clave, (Rc<Layout<u8>>, bool)>,
}

impl Rotulos {
    pub fn new() -> Self {
        Rotulos {
            textos: Textos::new(),
            escala: 1.0,
            guardados: HashMap::new(),
        }
    }

    pub fn escala(&self) -> f32 {
        self.escala
    }

    /// Otra escala: todo se rearma.
    pub fn poner_escala(&mut self, escala: f32) {
        if escala != self.escala {
            self.escala = escala;
            self.guardados.clear();
        }
    }

    fn buscar(
        &mut self,
        c: Clave,
        armar: impl FnOnce(&mut Textos, f32) -> Layout<u8>,
    ) -> Rc<Layout<u8>> {
        if let Some((l, usado)) = self.guardados.get_mut(&c) {
            *usado = true;
            return l.clone();
        }
        let l = Rc::new(armar(&mut self.textos, self.escala));
        self.guardados.insert(c, (l.clone(), true));
        l
    }

    /// Una línea entera.
    pub fn linea(&mut self, s: &str, px: f32, peso: f32) -> Rc<Layout<u8>> {
        let c = Clave::Linea {
            texto: s.to_owned(),
            px: (px * 100.0) as u32,
            peso: peso as u16,
            max: None,
            familia: None,
            al_inicio: false,
        };
        self.buscar(c, |t, e| t.armar_con_peso(&[(s, 0)], px, peso, e))
    }

    /// Una línea que termina en «…» si no entra en `max` lógicos (una etiqueta de GTK con
    /// `ellipsize`).
    pub fn recortada(&mut self, s: &str, px: f32, max: f64) -> Rc<Layout<u8>> {
        self.recortada_con_peso(s, px, 400.0, max)
    }

    pub fn recortada_con_peso(&mut self, s: &str, px: f32, peso: f32, max: f64) -> Rc<Layout<u8>> {
        let max_b = (max * f64::from(self.escala)).max(0.0) as u32;
        let c = Clave::Linea {
            texto: s.to_owned(),
            px: (px * 100.0) as u32,
            peso: peso as u16,
            max: Some(max_b),
            familia: None,
            al_inicio: false,
        };
        self.buscar(c, |t, e| {
            texto::armar_recortado_con_peso(t, s, 0, px, peso, e, max_b as f32)
        })
    }

    /// Una línea que empieza con «…» si no entra en `max` lógicos (`ellipsize = Start`).
    pub fn recortada_al_inicio(&mut self, s: &str, px: f32, max: f64) -> Rc<Layout<u8>> {
        let max_b = (max * f64::from(self.escala)).max(0.0) as u32;
        let c = Clave::Linea {
            texto: s.to_owned(),
            px: (px * 100.0) as u32,
            peso: 400,
            max: Some(max_b),
            familia: None,
            al_inicio: true,
        };
        self.buscar(c, |t, e| texto::armar_recortado_al_inicio(t, s, 0, px, e, max_b as f32))
    }

    /// `recortada` en otra familia (la galería de fuentes del menú).
    pub fn recortada_en(&mut self, s: &str, familia: &str, px: f32, max: f64) -> Rc<Layout<u8>> {
        let max_b = (max * f64::from(self.escala)).max(0.0) as u32;
        let c = Clave::Linea {
            texto: s.to_owned(),
            px: (px * 100.0) as u32,
            peso: 400,
            max: Some(max_b),
            familia: Some(familia.to_owned()),
            al_inicio: false,
        };
        self.buscar(c, |t, e| {
            texto::armar_recortado_en(t, s, familia, 0, px, 400.0, e, max_b as f32)
        })
    }

    /// Una etiqueta con tramos de distinto rol y tamaño (lo resaltado de una búsqueda, el
    /// genérico más chico), con «…» si no entra en `max` lógicos.
    pub fn tramos(&mut self, partes: &[(&str, u8, f32)], max: f64) -> Rc<Layout<u8>> {
        let max_b = (max * f64::from(self.escala)).max(0.0) as u32;
        let c = Clave::Tramos {
            partes: partes
                .iter()
                .map(|(s, r, px)| ((*s).to_owned(), *r, (px * 100.0) as u32))
                .collect(),
            max: max_b,
        };
        self.buscar(c, |t, e| t.armar_tramos_recortado(partes, e, max_b as f32))
    }

    /// El cuerpo de una notificación: markup seguro (`b`, `i`, `u`), en renglones de `ancho`
    /// lógicos, como mucho `lineas`.
    pub fn parrafo(&mut self, markup: &str, px: f32, ancho: f64, lineas: usize) -> Rc<Layout<u8>> {
        let ancho_b = (ancho * f64::from(self.escala)).max(1.0) as u32;
        let c = Clave::Parrafo {
            markup: markup.to_owned(),
            px: (px * 100.0) as u32,
            ancho: ancho_b,
            lineas,
        };
        self.buscar(c, |t, e| {
            let tramos = tramos(markup);
            let partes: Vec<(&str, Estilo)> =
                tramos.iter().map(|(s, e)| (s.as_str(), *e)).collect();
            t.parrafo(&partes, px, e, ancho_b as f32, lineas)
        })
    }

    /// Al terminar un cuadro: suelta lo que no se usó.
    pub fn barrer(&mut self) {
        self.guardados.retain(|_, (_, usado)| std::mem::take(usado));
    }
}

/// El markup que deja `notificaciones::markup::seguro` en tramos con su estilo: `b`, `i`, `u`
/// (`a` sin estilo) y las entidades de XML.
pub fn tramos(markup: &str) -> Vec<(String, Estilo)> {
    let mut v: Vec<(String, Estilo)> = Vec::new();
    let (mut negrita, mut cursiva, mut subrayado) = (0u32, 0u32, 0u32);
    let mut actual = String::new();
    let estilo = |n: u32, c: u32, s: u32| Estilo {
        rol: 0,
        negrita: n > 0,
        cursiva: c > 0,
        subrayado: s > 0,
    };
    let mut resto = markup;
    while let Some(ch) = resto.chars().next() {
        if ch == '<'
            && let Some(fin) = resto.find('>')
        {
            let tag = resto[1..fin].trim();
            let (cierra, nombre) = match tag.strip_prefix('/') {
                Some(n) => (true, n.trim()),
                None => (false, tag.split_whitespace().next().unwrap_or("")),
            };
            let nombre = nombre.to_ascii_lowercase();
            if matches!(nombre.as_str(), "b" | "i" | "u") {
                if !actual.is_empty() {
                    let e = estilo(negrita, cursiva, subrayado);
                    v.push((std::mem::take(&mut actual), e));
                }
                let n = match nombre.as_str() {
                    "b" => &mut negrita,
                    "i" => &mut cursiva,
                    _ => &mut subrayado,
                };
                if cierra {
                    *n = n.saturating_sub(1);
                } else {
                    *n += 1;
                }
            }
            resto = &resto[fin + 1..];
            continue;
        }
        if ch == '&'
            && let Some(fin) = resto.find(';')
            && fin <= 10
        {
            let ent = &resto[1..fin];
            let c = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => ent
                    .strip_prefix("#x")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            if let Some(c) = c {
                actual.push(c);
                resto = &resto[fin + 1..];
                continue;
            }
        }
        actual.push(ch);
        resto = &resto[ch.len_utf8()..];
    }
    if !actual.is_empty() {
        v.push((actual, estilo(negrita, cursiva, subrayado)));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tramos_con_estilo_y_entidades() {
        let t = tramos("<b>Martín</b> está jugando <i>Portal &amp; 2</i>");
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].0, "Martín");
        assert!(t[0].1.negrita && !t[0].1.cursiva);
        assert_eq!(t[1].0, " está jugando ");
        assert!(!t[1].1.negrita);
        assert_eq!(t[2].0, "Portal & 2");
        assert!(t[2].1.cursiva);
        assert_eq!(tramos("a &lt;b&gt;\nc")[0].0, "a <b>\nc");
        assert_eq!(tramos("<a href=\"x\">link</a>")[0].0, "link");
    }
}
