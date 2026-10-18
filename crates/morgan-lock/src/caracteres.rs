//! Solo la cuenta: el texto no sale del buffer.

use gtk::prelude::*;
use gtk::{gdk, graphene, gsk, pango};

use crate::lienzo::{ALTO, ANCHO, BORDE, Paleta, acotar, azar, con_alfa, ease_out, mezclar, seg};

const PASO: f32 = 14.0;
/// Los símbolos entre los que se revuelve cada carácter: los que JetBrains Mono dibuja a un
/// tamaño parejo (◐, ≡ o ≈ salían de otra fuente, mucho más chicos).
const GLIFOS: &[char] = &[
    '◆', '◇', '◈', '○', '◎', '●', '□', '■', '▲', '△', '▼', '▽', '◊', '※', '✦', '✧', '⊕', '⊗',
    '⊙',
];

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

    pub fn limpiar(&mut self, t: i64) {
        self.chars.retain(|c| {
            !c.muere.is_some_and(|m| seg(t - m) > 0.2)
                && !c.cae.as_ref().is_some_and(|k| seg(t - k.t0) > 1.1)
        });
    }

    fn acomodar(&mut self, dt: f32, juntar: bool) {
        let n = self.vivos() as f32;
        let k = 1.0 - (-dt / if juntar { 0.09 } else { 0.055 }).exp();
        let mut i = 0.0;
        for c in self.chars.iter_mut() {
            if !c.vivo() {
                continue;
            }
            let obj = if juntar {
                ANCHO / 2.0
            } else {
                ANCHO / 2.0 + (i - (n - 1.0) / 2.0) * PASO
            };
            i += 1.0;
            let x = c.x.get_or_insert(obj);
            *x += (obj - *x) * k;
        }
    }

    fn recortar(s: &gtk::Snapshot) {
        s.push_clip(&graphene::Rect::new(
            BORDE,
            BORDE,
            ANCHO - 2.0 * BORDE,
            ALTO - 2.0 * BORDE,
        ));
    }

    pub fn puntos(&mut self, s: &gtk::Snapshot, t: i64, dt: f32, latido: Option<f32>, p: &Paleta) {
        self.acomodar(dt, latido.is_some());
        Self::recortar(s);
        let cy = ALTO / 2.0;
        for c in self.chars.iter_mut() {
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
                esc *= 1.0 + 0.28 * (fase / 0.9 * std::f32::consts::TAU).sin();
            }
            if c.brilla {
                color = mezclar(&p.on_surface, &p.primary, 0.6);
            }
            aro(s, x, y, esc, &con_alfa(&color, alfa));
        }
        s.pop();
    }

    pub fn simbolos(
        &mut self,
        s: &gtk::Snapshot,
        w: &gtk::Widget,
        t: i64,
        dt: f32,
        latido: Option<f32>,
        p: &Paleta,
    ) {
        self.acomodar(dt, false);
        Self::recortar(s);
        let fuente = pango::FontDescription::from_string("JetBrainsMono Nerd Font 13");
        for (i, c) in self.chars.iter_mut().enumerate() {
            let Some(x) = c.x else { continue };
            let edad = seg(t - c.nace);
            let (mut alfa, mut color, mut g) = (1.0, p.on_surface, c.glifo);
            if edad < 0.16 || latido.is_some() {
                if t >= c.prox {
                    c.visto = glifo(t ^ (i as i64 * 31));
                    c.prox = t + if latido.is_some() { 140_000 } else { 35_000 };
                }
                g = c.visto;
                color = match latido {
                    Some(fase) => {
                        alfa = 0.55 + 0.35 * (fase / 0.9 * std::f32::consts::TAU).sin();
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
                g = '×';
                color = p.error;
                alfa = 1.0 - acotar(seg(t - k.t0) / 0.45, 0.0, 1.0);
            }
            if c.brilla {
                color = p.primary;
            }
            if alfa <= 0.01 {
                continue;
            }
            let l = w.create_pango_layout(Some(&g.to_string()));
            l.set_font_description(Some(&fuente));
            let (lw, lh) = l.pixel_size();
            let esc = 0.6 + 0.4 * ease_out(edad / 0.12);
            s.save();
            s.translate(&graphene::Point::new(x, ALTO / 2.0));
            s.scale(esc, esc);
            s.translate(&graphene::Point::new(-lw as f32 / 2.0, -lh as f32 / 2.0));
            s.append_layout(&l, &con_alfa(&color, alfa));
            s.restore();
        }
        s.pop();
    }

    pub fn fijos(&self, s: &gtk::Snapshot, p: &Paleta) {
        let n = self.vivos() as f32;
        for i in 0..self.vivos() {
            let x = ANCHO / 2.0 + (i as f32 - (n - 1.0) / 2.0) * PASO;
            aro(s, x, ALTO / 2.0, 1.0, &p.on_surface);
        }
    }
}

fn aro(s: &gtk::Snapshot, x: f32, y: f32, esc: f32, c: &gdk::RGBA) {
    if esc <= 0.02 || c.alpha() <= 0.01 {
        return;
    }
    let centro = graphene::Point::new(x, y);
    let b = gsk::PathBuilder::new();
    b.add_circle(&centro, 4.9 * esc);
    s.append_stroke(&b.to_path(), &gsk::Stroke::new(1.3 * esc), c);
    let b = gsk::PathBuilder::new();
    b.add_circle(&centro, 1.9 * esc);
    s.append_fill(&b.to_path(), gsk::FillRule::Winding, c);
}
