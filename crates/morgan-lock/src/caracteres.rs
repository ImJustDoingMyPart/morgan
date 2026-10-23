//! Las escrituras que no son la onda: puntos, símbolos y fijo. Solo la cuenta: el texto no
//! sale de `clave.rs`. El modelo (nacer, morir, caer, acomodarse, revolver el símbolo) lo
//! camina el hilo de render en `avanzar`; el dibujo, abajo, no cambia nada.

use std::f32::consts::TAU;

use vello::Scene;
use vello::kurbo::{Affine, Circle, Rect, Stroke};
use vello::peniko::Fill;

use crate::escena::{ALTO, ANCHO, Color, Paleta, acotar, azar, con_alfa, ease_out, mezclar, seg};

const PASO: f32 = 14.0;
const BORDE: f64 = 2.0;
/// Los símbolos entre los que se revuelve cada carácter: los que JetBrains Mono dibuja a un
/// tamaño parejo (◐, ≡ o ≈ salían de otra fuente, mucho más chicos).
pub const GLIFOS: &[char] = &[
    '◆', '◇', '◈', '○', '◎', '●', '□', '■', '▲', '△', '▼', '▽', '◊', '※', '✦', '✧', '⊕', '⊗', '⊙',
];
/// El de la clave mala.
pub const CRUZ: char = '×';

struct Caida {
    t0: i64,
    vx: f32,
    vy: f32,
}

struct Caracter {
    nace: i64,
    muere: Option<i64>,
    cae: Option<Caida>,
    brilla: bool,
    x: Option<f32>,
    glifo: char,
    visto: char,
    prox: i64,
}

impl Caracter {
    fn vivo(&self) -> bool {
        self.muere.is_none() && self.cae.is_none()
    }
}

#[derive(Default)]
pub struct Caracteres {
    chars: Vec<Caracter>,
}

fn glifo(semilla: i64) -> char {
    GLIFOS[(azar(semilla) * GLIFOS.len() as f32) as usize % GLIFOS.len()]
}

impl Caracteres {
    pub fn vivos(&self) -> usize {
        self.chars.iter().filter(|c| c.vivo()).count()
    }

    pub fn hay(&self) -> bool {
        !self.chars.is_empty()
    }

    pub fn contar(&mut self, total: usize, t: i64) {
        let vivos = self.vivos();
        for i in vivos..total {
            let g = glifo(t ^ (i as i64 * 7919));
            self.chars.push(Caracter {
                nace: t,
                muere: None,
                cae: None,
                brilla: false,
                x: None,
                glifo: g,
                visto: g,
                prox: 0,
            });
        }
        for c in self
            .chars
            .iter_mut()
            .rev()
            .filter(|c| c.vivo())
            .take(vivos.saturating_sub(total))
        {
            c.muere = Some(t);
        }
    }

    pub fn vaciar(&mut self, t: i64) {
        self.contar(0, t);
    }

    pub fn caer(&mut self, t: i64) {
        for (i, c) in self.chars.iter_mut().enumerate().filter(|(_, c)| c.vivo()) {
            let a = azar(t ^ (i as i64 * 104_729));
            let b = azar(t ^ (i as i64 * 15_485_863));
            c.cae = Some(Caida {
                t0: t,
                vx: (a - 0.5) * 120.0,
                vy: -120.0 - b * 140.0,
            });
        }
    }

    pub fn brillar(&mut self) {
        for c in &mut self.chars {
            c.brilla = true;
        }
    }

    /// Una vuelta del render: suelta lo que terminó, acomoda las posiciones (con `dt`, que
    /// es lo único que depende del cuadro) y revuelve los símbolos que todavía cambian.
    pub fn avanzar(&mut self, t: i64, dt: f32, juntar: bool, latido: bool) {
        self.chars.retain(|c| {
            !c.muere.is_some_and(|m| seg(t - m) > 0.2)
                && !c.cae.as_ref().is_some_and(|k| seg(t - k.t0) > 1.1)
        });
        let n = self.vivos() as f32;
        let k = 1.0 - (-dt / if juntar { 0.09 } else { 0.055 }).exp();
        let mut i = 0.0;
        for c in self.chars.iter_mut().filter(|c| c.vivo()) {
            let obj = if juntar {
                ANCHO / 2.0
            } else {
                ANCHO / 2.0 + (i - (n - 1.0) / 2.0) * PASO
            };
            i += 1.0;
            let x = c.x.get_or_insert(obj);
            *x += (obj - *x) * k;
        }
        for (i, c) in self.chars.iter_mut().enumerate() {
            if (seg(t - c.nace) < 0.16 || latido) && t >= c.prox {
                c.visto = glifo(t ^ (i as i64 * 31));
                c.prox = t + if latido { 140_000 } else { 35_000 };
            }
        }
    }
}

/// Lo de adentro del borde: los caracteres que caen no lo pisan.
fn recortar(s: &mut Scene, tr: Affine) {
    let r = Rect::new(BORDE, BORDE, ANCHO as f64 - BORDE, ALTO as f64 - BORDE);
    s.push_clip_layer(Fill::NonZero, tr, &r);
}

pub fn puntos(s: &mut Scene, tr: Affine, f: &Caracteres, t: i64, latido: Option<f32>, p: &Paleta) {
    recortar(s, tr);
    let cy = ALTO / 2.0;
    for c in &f.chars {
        let Some(mut x) = c.x else { continue };
        let mut esc = ease_out(seg(t - c.nace) / 0.1);
        let (mut alfa, mut color, mut y) = (1.0, p.on_surface, cy);
        if let Some(m) = c.muere {
            let f = seg(t - m) / 0.12;
            esc *= 1.0 - ease_out(f);
            alfa = 1.0 - f;
        }
        if let Some(k) = &c.cae {
            let e = seg(t - k.t0);
            y = cy + k.vy * e + 900.0 * e * e;
            x += k.vx * e;
            color = p.error;
            alfa = 1.0 - acotar(e / 0.55, 0.0, 1.0);
        }
        if let Some(fase) = latido {
            esc *= 1.0 + 0.28 * (fase / 0.9 * TAU).sin();
        }
        if c.brilla {
            color = mezclar(&p.on_surface, &p.primary, 0.6);
        }
        aro(s, tr, x, y, esc, &con_alfa(&color, alfa));
    }
    s.pop_layer();
}

/// `dibujar(s, glifo, x, y, escala, color)` pone un símbolo con su centro en (x, y), en las
/// coordenadas del campo: el texto lo arma quien tiene parley.
pub fn simbolos(
    s: &mut Scene,
    tr: Affine,
    f: &Caracteres,
    t: i64,
    latido: Option<f32>,
    p: &Paleta,
    mut dibujar: impl FnMut(&mut Scene, char, f32, f32, f32, Color),
) {
    recortar(s, tr);
    for c in &f.chars {
        let Some(x) = c.x else { continue };
        let edad = seg(t - c.nace);
        let (mut alfa, mut color, mut g) = (1.0, p.on_surface, c.glifo);
        if edad < 0.16 || latido.is_some() {
            g = c.visto;
            color = match latido {
                Some(fase) => {
                    alfa = 0.55 + 0.35 * (fase / 0.9 * TAU).sin();
                    mezclar(&p.on_surface, &p.primary, 0.5)
                }
                None if (edad / 0.035) as u32 % 2 == 1 => p.primary,
                None => p.tertiary,
            };
        }
        if let Some(m) = c.muere {
            alfa = 1.0 - seg(t - m) / 0.14;
        }
        if let Some(k) = &c.cae {
            g = CRUZ;
            color = p.error;
            alfa = 1.0 - acotar(seg(t - k.t0) / 0.45, 0.0, 1.0);
        }
        if c.brilla {
            color = p.primary;
        }
        if alfa <= 0.01 {
            continue;
        }
        let esc = 0.6 + 0.4 * ease_out(edad / 0.12);
        dibujar(s, g, x, ALTO / 2.0, esc, con_alfa(&color, alfa));
    }
    s.pop_layer();
}

pub fn fijos(s: &mut Scene, tr: Affine, f: &Caracteres, p: &Paleta) {
    let n = f.vivos() as f32;
    for i in 0..f.vivos() {
        let x = ANCHO / 2.0 + (i as f32 - (n - 1.0) / 2.0) * PASO;
        aro(s, tr, x, ALTO / 2.0, 1.0, &p.on_surface);
    }
}

fn aro(s: &mut Scene, tr: Affine, x: f32, y: f32, esc: f32, c: &Color) {
    if esc <= 0.02 || c[3] <= 0.01 {
        return;
    }
    let pincel = vello::peniko::Color::new(*c);
    let centro = (x as f64, y as f64);
    let anillo = Circle::new(centro, 4.9 * esc as f64);
    s.stroke(&Stroke::new(1.3 * esc as f64), tr, pincel, None, &anillo);
    s.fill(
        Fill::NonZero,
        tr,
        pincel,
        None,
        &Circle::new(centro, 1.9 * esc as f64),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contar_agrega_y_mata_los_ultimos() {
        let mut f = Caracteres::default();
        f.contar(3, 0);
        assert_eq!(f.vivos(), 3);
        f.contar(1, 10);
        assert_eq!(f.vivos(), 1);
        assert_eq!(f.chars.len(), 3, "los que mueren se ven irse");
        f.avanzar(10 + 300_000, 0.016, false, false);
        assert_eq!(f.chars.len(), 1);
    }

    #[test]
    fn se_acomodan_centrados() {
        let mut f = Caracteres::default();
        f.contar(2, 0);
        for i in 0..200 {
            f.avanzar(i * 16_000, 0.016, false, false);
        }
        let xs: Vec<f32> = f.chars.iter().filter_map(|c| c.x).collect();
        assert!((xs[0] + xs[1] - ANCHO).abs() < 0.01);
        assert!((xs[1] - xs[0] - PASO).abs() < 0.01);
    }
}
