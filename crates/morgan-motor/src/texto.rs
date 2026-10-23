//! Texto con parley, dibujado con vello. El pincel de cada tramo es un rol de la paleta (un
//! número), no un color: el color se resuelve al dibujar y puede animarse sin rearmar nada.

use parley::{
    FontContext, FontFamily, FontWeight, Layout, LayoutContext, PositionedLayoutItem, StyleProperty,
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
        let texto: String = partes.iter().map(|(t, _)| *t).collect();
        let mut b = self
            .cx
            .ranged_builder(&mut self.fuentes, &texto, escala, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::from(FAMILIA)));
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

/// El tamaño de la caja de `l`, en píxeles enteros.
pub fn caja(l: &Layout<u8>) -> (f64, f64) {
    (l.width().ceil() as f64, l.height().ceil() as f64)
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
    let l = t.armar(&[(texto, rol)], px, escala);
    if l.width() <= max {
        return l;
    }
    let cortes: Vec<usize> = texto.char_indices().map(|(i, _)| i).collect();
    // El corte más largo que entra con los puntos suspensivos.
    let (mut lo, mut hi) = (0, cortes.len());
    while lo < hi {
        let m = (lo + hi).div_ceil(2);
        let prueba = format!("{}…", &texto[..cortes[m]]);
        if t.armar(&[(&prueba, rol)], px, escala).width() <= max {
            lo = m;
        } else {
            hi = m - 1;
        }
    }
    let corte = cortes.get(lo).copied().unwrap_or(0);
    t.armar(&[(&format!("{}…", &texto[..corte]), rol)], px, escala)
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
