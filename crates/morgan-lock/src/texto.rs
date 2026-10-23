//! Texto con parley, dibujado con vello. El pincel de cada tramo es un rol de la paleta (un
//! número), no un color: el color se resuelve al dibujar y puede animarse sin rearmar nada.

use parley::{FontContext, FontFamily, Layout, LayoutContext, PositionedLayoutItem, StyleProperty};
use vello::Scene;
use vello::kurbo::Affine;
use vello::peniko::Fill;

use crate::escena::Color;

const FAMILIA: &str = "JetBrainsMono Nerd Font";

/// Los tamaños de `morgan-lock` están en puntos; GTK los pasa a píxeles a 96 ppp.
pub fn pt(p: f32) -> f32 {
    p * 96.0 / 72.0
}

pub struct Textos {
    fuentes: FontContext,
    cx: LayoutContext<u8>,
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
        let texto: String = partes.iter().map(|(t, _)| *t).collect();
        let mut b = self
            .cx
            .ranged_builder(&mut self.fuentes, &texto, escala, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::from(FAMILIA)));
        b.push_default(StyleProperty::FontSize(px));
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
    let x0 = (cx - l.width() as f64 / 2.0).round();
    let y0 = (cy - l.height() as f64 / 2.0).round();
    dibujar_en(s, l, Affine::translate((x0, y0)), color, alfa);
}

/// Con la esquina del layout donde la ponga `tr` (para escalarlo, como los símbolos).
pub fn dibujar_en(
    s: &mut Scene,
    l: &Layout<u8>,
    tr: Affine,
    color: impl Fn(u8) -> Color,
    alfa: f32,
) {
    for linea in l.lines() {
        for item in linea.items() {
            let PositionedLayoutItem::GlyphRun(g) = item else {
                continue;
            };
            let run = g.run();
            let c = color(g.style().brush);
            let c = vello::peniko::Color::new([c[0], c[1], c[2], c[3] * alfa]);
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
                .brush(c)
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
