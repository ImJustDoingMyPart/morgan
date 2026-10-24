//! Texto con parley, dibujado con vello. El pincel de cada tramo es un rol de la paleta (un
//! número), no un color: el color se resuelve al dibujar y puede animarse sin rearmar nada.

use parley::{
    FontContext, FontFamily, FontStyle, FontWeight, Layout, LayoutContext, OverflowWrap,
    PositionedLayoutItem, StyleProperty,
};
use vello::Scene;
use vello::kurbo::Affine;
use vello::peniko::{Brush, Fill};

use crate::color::Color;

pub const FAMILIA: &str = "JetBrainsMono Nerd Font";

/// Los tamaños que en la hoja de GTK estaban en puntos; GTK los pasa a píxeles a 96 ppp.
pub fn pt(p: f32) -> f32 {
    p * 96.0 / 72.0
}

pub struct Textos {
    fuentes: FontContext,
    cx: LayoutContext<u8>,
}

impl Default for Textos {
    fn default() -> Self {
        Self::new()
    }
}

impl Textos {
    pub fn new() -> Self {
        Textos {
            fuentes: FontContext::new(),
            cx: LayoutContext::new(),
        }
    }

    /// Una línea con un rol por tramo. `escala` es la de la salida: el texto se arma en
    /// píxeles del buffer, para que el hinting caiga en píxeles de verdad.
    pub fn armar(&mut self, partes: &[(&str, u8)], px: f32, escala: f32) -> Layout<u8> {
        self.armar_con_peso(partes, px, 400.0, escala)
    }

    /// Con otro grosor de la misma familia (400 es la regular, 500 la medium).
    pub fn armar_con_peso(
        &mut self,
        partes: &[(&str, u8)],
        px: f32,
        peso: f32,
        escala: f32,
    ) -> Layout<u8> {
        self.armar_en(partes, FAMILIA, px, peso, escala)
    }

    /// En otra familia (la galería de fuentes del menú, cada una en la suya).
    pub fn armar_en(
        &mut self,
        partes: &[(&str, u8)],
        familia: &str,
        px: f32,
        peso: f32,
        escala: f32,
    ) -> Layout<u8> {
        let texto: String = partes.iter().map(|(t, _)| *t).collect();
        let mut b = self
            .cx
            .ranged_builder(&mut self.fuentes, &texto, escala, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::from(familia)));
        b.push_default(StyleProperty::FontSize(px));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(peso)));
        let mut desde = 0;
        for (t, rol) in partes {
            b.push(StyleProperty::Brush(*rol), desde..desde + t.len());
            desde += t.len();
        }
        let mut l = b.build(&texto);
        l.break_all_lines(None);
        l
    }

    /// Una línea con un rol y un tamaño por tramo, sobre una misma línea de base (un `span
    /// size` de Pango).
    pub fn armar_tramos(&mut self, partes: &[(&str, u8, f32)], escala: f32) -> Layout<u8> {
        let texto: String = partes.iter().map(|(t, _, _)| *t).collect();
        let mut b = self
            .cx
            .ranged_builder(&mut self.fuentes, &texto, escala, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::from(FAMILIA)));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(400.0)));
        // El tamaño de la etiqueta es el del primer tramo; sin esto, la línea se arma con el
        // de parley (16 px) y queda más alta.
        if let Some((_, _, px)) = partes.first() {
            b.push_default(StyleProperty::FontSize(*px));
        }
        let mut desde = 0;
        for (t, rol, px) in partes {
            let r = desde..desde + t.len();
            b.push(StyleProperty::Brush(*rol), r.clone());
            b.push(StyleProperty::FontSize(*px), r);
            desde += t.len();
        }
        let mut l = b.build(&texto);
        l.break_all_lines(None);
        l
    }

    /// `armar_tramos` que, si no entra en `max` (píxeles del buffer), se corta y termina en
    /// «…» con el rol y el tamaño del último tramo que queda, como una etiqueta de GTK con
    /// `ellipsize = End`.
    pub fn armar_tramos_recortado(
        &mut self,
        partes: &[(&str, u8, f32)],
        escala: f32,
        max: f32,
    ) -> Layout<u8> {
        let l = self.armar_tramos(partes, escala);
        if l.width() <= max {
            return l;
        }
        let total: usize = partes.iter().map(|(t, _, _)| t.len()).sum();
        let cortes: Vec<usize> = (0..total)
            .filter(|&i| {
                let mut i = i;
                for (t, _, _) in partes {
                    if i < t.len() {
                        return t.is_char_boundary(i);
                    }
                    i -= t.len();
                }
                false
            })
            .collect();
        let recortar = |corte: usize| {
            let mut v: Vec<(String, u8, f32)> = Vec::new();
            let mut falta = corte;
            for (t, rol, px) in partes {
                if falta == 0 {
                    break;
                }
                let n = falta.min(t.len());
                v.push((t[..n].to_owned(), *rol, *px));
                falta -= n;
            }
            while let Some((t, rol, px)) = v.pop() {
                let t = t.trim_end().to_owned();
                if !t.is_empty() {
                    v.push((t, rol, px));
                    break;
                }
            }
            let (rol, px) = v
                .last()
                .map(|(_, r, p)| (*r, *p))
                .or_else(|| partes.first().map(|(_, r, p)| (*r, *p)))
                .unwrap_or((0, 14.0));
            v.push(("…".to_owned(), rol, px));
            v
        };
        let armar = |t: &mut Textos, v: &[(String, u8, f32)]| {
            let p: Vec<(&str, u8, f32)> = v.iter().map(|(s, r, x)| (s.as_str(), *r, *x)).collect();
            t.armar_tramos(&p, escala)
        };
        // El corte más largo que entra con los puntos suspensivos.
        let (mut lo, mut hi) = (0, cortes.len().saturating_sub(1));
        while lo < hi {
            let m = (lo + hi).div_ceil(2);
            if armar(self, &recortar(cortes[m])).width() <= max {
                lo = m;
            } else {
                hi = m - 1;
            }
        }
        armar(self, &recortar(cortes.get(lo).copied().unwrap_or(0)))
    }

    /// Un párrafo con estilos por tramo, partido en renglones de `ancho` (píxeles del buffer)
    /// como una etiqueta de GTK con `wrap` y `WordChar`: por palabras, y una palabra que no
    /// entra se parte. Con más de `lineas`, el último renglón termina en «…».
    pub fn parrafo(
        &mut self,
        partes: &[(&str, Estilo)],
        px: f32,
        escala: f32,
        ancho: f32,
        lineas: usize,
    ) -> Layout<u8> {
        let l = self.armar_parrafo(partes, px, escala, ancho);
        if l.len() <= lineas || lineas == 0 {
            return l;
        }
        let total: usize = partes.iter().map(|(t, _)| t.len()).sum();
        let fin = l
            .lines()
            .nth(lineas - 1)
            .map_or(total, |r| r.text_range().end);
        // El corte más largo que, con «…», entra en `lineas` renglones.
        let cortes: Vec<usize> = (0..=fin)
            .filter(|i| es_corte(partes, *i))
            .collect();
        let (mut lo, mut hi) = (0, cortes.len().saturating_sub(1));
        while lo < hi {
            let m = (lo + hi).div_ceil(2);
            let prueba = recortar(partes, cortes[m]);
            if self.armar_parrafo(&prueba, px, escala, ancho).len() <= lineas {
                lo = m;
            } else {
                hi = m - 1;
            }
        }
        let corte = cortes.get(lo).copied().unwrap_or(0);
        self.armar_parrafo(&recortar(partes, corte), px, escala, ancho)
    }

    fn armar_parrafo(
        &mut self,
        partes: &[(&str, Estilo)],
        px: f32,
        escala: f32,
        ancho: f32,
    ) -> Layout<u8> {
        let texto: String = partes.iter().map(|(t, _)| *t).collect();
        let mut b = self
            .cx
            .ranged_builder(&mut self.fuentes, &texto, escala, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::from(FAMILIA)));
        b.push_default(StyleProperty::FontSize(px));
        b.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
        let mut desde = 0;
        for (t, e) in partes {
            let r = desde..desde + t.len();
            b.push(StyleProperty::Brush(e.rol), r.clone());
            let peso = if e.negrita { 700.0 } else { 400.0 };
            b.push(StyleProperty::FontWeight(FontWeight::new(peso)), r.clone());
            if e.cursiva {
                b.push(StyleProperty::FontStyle(FontStyle::Italic), r.clone());
            }
            if e.subrayado {
                b.push(StyleProperty::Underline(true), r);
            }
            desde += t.len();
        }
        let mut l = b.build(&texto);
        l.break_all_lines(Some(ancho));
        l
    }
}

/// Cómo va un tramo de un párrafo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Estilo {
    pub rol: u8,
    pub negrita: bool,
    pub cursiva: bool,
    pub subrayado: bool,
}

/// Si el byte `i` del texto de `partes` cae entre dos caracteres.
fn es_corte(partes: &[(&str, Estilo)], mut i: usize) -> bool {
    for (t, _) in partes {
        if i <= t.len() {
            return t.is_char_boundary(i);
        }
        i -= t.len();
    }
    false
}

/// Los tramos hasta el byte `corte`, sin el blanco del final, y «…» con el estilo del último.
fn recortar<'a>(partes: &[(&'a str, Estilo)], mut corte: usize) -> Vec<(&'a str, Estilo)> {
    let mut v: Vec<(&'a str, Estilo)> = Vec::new();
    for (t, e) in partes {
        if corte == 0 {
            break;
        }
        let n = corte.min(t.len());
        v.push((&t[..n], *e));
        corte -= n;
    }
    while let Some((t, e)) = v.pop() {
        let t = t.trim_end();
        if !t.is_empty() {
            v.push((t, e));
            break;
        }
    }
    let e = v.last().map(|(_, e)| *e).unwrap_or_default();
    v.push(("…", e));
    v
}

/// Dibuja `l` con su centro en (`cx`, `cy`), en píxeles del buffer.
pub fn dibujar(
    s: &mut Scene,
    l: &Layout<u8>,
    cx: f64,
    cy: f64,
    color: impl Fn(u8) -> Color,
    alfa: f32,
) {
    dibujar_en(s, l, Affine::translate(esquina(l, cx, cy)), color, alfa);
}

/// Dónde cae la esquina de `l` centrado en (`cx`, `cy`). Como una etiqueta de GTK: la caja
/// mide el texto redondeado hacia arriba y se centra en un píxel entero.
pub fn esquina(l: &Layout<u8>, cx: f64, cy: f64) -> (f64, f64) {
    let (w, h) = caja(l);
    ((cx - w / 2.0).floor(), (cy - h / 2.0).floor())
}

/// El tamaño de la caja de `l`, en píxeles enteros. El ancho, como el de Pango: cada letra
/// avanza en unidades de 1/1024 de píxel (redondeadas), así que un texto que mide justo un
/// número entero (5 letras de 7,2 px) se pasa por poco y su caja lleva un píxel más.
pub fn caja(l: &Layout<u8>) -> (f64, f64) {
    (ancho_pango(l).ceil(), l.height().ceil() as f64)
}

fn ancho_pango(l: &Layout<u8>) -> f64 {
    let mut ancho: f64 = 0.0;
    for linea in l.lines() {
        let mut w: i64 = 0;
        for item in linea.items() {
            let PositionedLayoutItem::GlyphRun(g) = item else {
                continue;
            };
            for gl in g.glyphs() {
                w += (f64::from(gl.advance) * 1024.0).round() as i64;
            }
        }
        ancho = ancho.max(w as f64 / 1024.0);
    }
    ancho
}

/// Con la esquina del layout donde la ponga `tr` (para escalarlo, como los símbolos).
pub fn dibujar_en(
    s: &mut Scene,
    l: &Layout<u8>,
    tr: Affine,
    color: impl Fn(u8) -> Color,
    alfa: f32,
) {
    dibujar_con(s, l, tr, |rol| {
        let c = color(rol);
        vello::peniko::Color::new([c[0], c[1], c[2], c[3] * alfa]).into()
    });
}

/// Con cualquier pincel por rol (un degradé, por ejemplo), en las coordenadas del layout.
pub fn dibujar_con(s: &mut Scene, l: &Layout<u8>, tr: Affine, pincel: impl Fn(u8) -> Brush) {
    for linea in l.lines() {
        for item in linea.items() {
            let PositionedLayoutItem::GlyphRun(g) = item else {
                continue;
            };
            let run = g.run();
            let c = pincel(g.style().brush);
            let coords: Vec<i16> = run
                .normalized_coords()
                .iter()
                .map(|c| c.to_bits())
                .collect();
            s.draw_glyphs(run.font())
                .font_size(run.font_size())
                .transform(tr)
                .normalized_coords(&coords)
                .hint(true)
                .brush(&c)
                .draw(
                    Fill::NonZero,
                    g.positioned_glyphs().map(|gl| vello::Glyph {
                        id: gl.id,
                        x: gl.x,
                        y: gl.y,
                    }),
                );
            if g.style().underline.is_some() {
                let m = run.font_metrics();
                let y = f64::from(g.baseline() - m.underline_offset);
                let x = f64::from(g.offset());
                let alto = f64::from(m.underline_size.max(1.0)).round();
                let r = vello::kurbo::Rect::new(
                    x,
                    y.round(),
                    x + f64::from(g.advance()),
                    y.round() + alto,
                );
                s.fill(Fill::NonZero, tr, &c, None, &r);
            }
        }
    }
}

/// Como una etiqueta de GTK con `ellipsize = End`: si no entra en `max` (píxeles del buffer),
/// se corta y termina en «…». Un solo tramo, un solo rol.
pub fn armar_recortado(
    t: &mut Textos,
    texto: &str,
    rol: u8,
    px: f32,
    escala: f32,
    max: f32,
) -> Layout<u8> {
    armar_recortado_con_peso(t, texto, rol, px, 400.0, escala, max)
}

/// `armar_recortado` con otro grosor (el título en negrita de un toast crítico).
pub fn armar_recortado_con_peso(
    t: &mut Textos,
    texto: &str,
    rol: u8,
    px: f32,
    peso: f32,
    escala: f32,
    max: f32,
) -> Layout<u8> {
    armar_recortado_en(t, texto, FAMILIA, rol, px, peso, escala, max)
}

/// `armar_recortado` en otra familia.
#[allow(clippy::too_many_arguments)]
pub fn armar_recortado_en(
    t: &mut Textos,
    texto: &str,
    familia: &str,
    rol: u8,
    px: f32,
    peso: f32,
    escala: f32,
    max: f32,
) -> Layout<u8> {
    let l = t.armar_en(&[(texto, rol)], familia, px, peso, escala);
    if l.width() <= max {
        return l;
    }
    let cortes: Vec<usize> = texto.char_indices().map(|(i, _)| i).collect();
    // El corte más largo que entra con los puntos suspensivos.
    let (mut lo, mut hi) = (0, cortes.len());
    while lo < hi {
        let m = (lo + hi).div_ceil(2);
        let prueba = format!("{}…", &texto[..cortes[m]]);
        if t.armar_en(&[(&prueba, rol)], familia, px, peso, escala)
            .width()
            <= max
        {
            lo = m;
        } else {
            hi = m - 1;
        }
    }
    let corte = cortes.get(lo).copied().unwrap_or(0);
    t.armar_en(&[(&format!("{}…", &texto[..corte]), rol)], familia, px, peso, escala)
}

/// La caja de la tinta de `l` (lo que se dibuja de verdad), en sus coordenadas: x0, y0, x1,
/// y1. Los glifos de la Nerd Font tienen el dibujo corrido dentro del ancho que reservan.
pub fn tinta(l: &Layout<u8>) -> Option<[f64; 4]> {
    use skrifa::MetadataProvider;
    use skrifa::instance::{LocationRef, Size};
    let mut caja: Option<[f64; 4]> = None;
    for linea in l.lines() {
        for item in linea.items() {
            let PositionedLayoutItem::GlyphRun(g) = item else {
                continue;
            };
            let run = g.run();
            let fuente = run.font();
            let Ok(f) = skrifa::FontRef::from_index(fuente.data.data(), fuente.index) else {
                continue;
            };
            let m = f.glyph_metrics(Size::new(run.font_size()), LocationRef::default());
            for gl in g.positioned_glyphs() {
                let Some(b) = m.bounds(skrifa::GlyphId::new(gl.id)) else {
                    continue;
                };
                if b.x_max <= b.x_min || b.y_max <= b.y_min {
                    continue;
                }
                let r = [
                    (gl.x + b.x_min) as f64,
                    (gl.y - b.y_max) as f64,
                    (gl.x + b.x_max) as f64,
                    (gl.y - b.y_min) as f64,
                ];
                caja = Some(match caja {
                    None => r,
                    Some(c) => [
                        c[0].min(r[0]),
                        c[1].min(r[1]),
                        c[2].max(r[2]),
                        c[3].max(r[3]),
                    ],
                });
            }
        }
    }
    caja
}

/// Dónde va la esquina de `l` para que el centro de su tinta caiga en (`cx`, `cy`), en
/// píxeles enteros. Así se dibuja todo glifo suelto en su caja: centrado por lo que se ve, no
/// por el ancho que reserva la fuente (en la Nerd Font el dibujo va corrido adentro).
pub fn esquina_por_tinta(l: &Layout<u8>, cx: f64, cy: f64) -> (f64, f64) {
    let (tx, ty) = match tinta(l) {
        Some([x0, y0, x1, y1]) => ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
        None => (l.width() as f64 / 2.0, l.height() as f64 / 2.0),
    };
    ((cx - tx).round(), (cy - ty).round())
}

/// Dónde cae la línea de base del primer renglón de `l`, desde su borde de arriba.
pub fn linea_de_base(l: &Layout<u8>) -> f64 {
    l.lines()
        .next()
        .map_or(0.0, |r| f64::from(r.metrics().baseline))
}

/// El alto de la caja de una etiqueta de GTK con el texto de `l`: ascenso más descenso de la
/// fuente, sin redondear cada uno (parley los redondea por separado y queda un píxel corto).
pub fn alto_de_linea(l: &Layout<u8>) -> f64 {
    use skrifa::MetadataProvider;
    use skrifa::instance::{LocationRef, Size};
    let alto = l.lines().next().and_then(|linea| {
        linea.items().find_map(|item| {
            let PositionedLayoutItem::GlyphRun(g) = item else {
                return None;
            };
            let run = g.run();
            let fuente = run.font();
            let f = skrifa::FontRef::from_index(fuente.data.data(), fuente.index).ok()?;
            let m = f.metrics(Size::new(run.font_size()), LocationRef::default());
            Some((m.ascent - m.descent + m.leading) as f64)
        })
    });
    alto.map_or_else(|| caja(l).1, f64::ceil)
}

