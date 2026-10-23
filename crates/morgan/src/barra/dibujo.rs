//! La barra en una `Scene` de vello. Las medidas y los colores son los de la versión con GTK
//! (`.barra` en `estilo.css` hasta 0.1.1):
//!
//! - 28 px de alto; a la izquierda los workspaces (10 px del borde, cada uno con 3 px de relleno
//!   a los lados y 2 arriba, y 1 de margen), al medio «fecha › hora» (rellenos de 5, 4 y 5, y
//!   2 abajo) y a la derecha, desde 10 px del borde: la campana (4 y 3 de relleno, 2 de margen),
//!   el cajón (la flecha con 4 por lado y la bandeja que se desliza, con 2 y 8 de relleno e
//!   íconos de 14 cada 24), la privacidad (4 por lado) y la grabación (4 y 6);
//! - al pasar el puntero o con lo suyo abierto, un paso por el degradé del anillo en 150 ms;
//!   apretado, al 92 % de su tamaño (suelto, vuelve en 150 ms);
//! - el globo de ayuda es el de GTK con la hoja de morgan (relleno de 6 y 10, borde de 1, la
//!   etiqueta con 1 y 4 más, y la sombra).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use morgan_motor::color::{Color, Paleta, con_alfa, mezclar, pincel};
use morgan_motor::curva::{Transicion, ease_in_out};
use morgan_motor::forma::sombra;
use morgan_motor::parley::Layout;
use morgan_motor::texto::{self, Textos, pt};
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, BezPath, Point, Rect, RoundedRect, Shape};
use morgan_motor::vello::peniko::{BlendMode, Fill, ImageBrush, ImageData};

use super::modelo::{self, Boton};
use crate::hyprland::Espacios;

pub const ALTO: u32 = 28;
const ROL: u8 = 0;
const PX: f32 = 14.0;
const PX_SEPARADOR: f32 = 13.0;
const PX_GLIFO: f32 = 15.5;
const PX_GLOBO: f32 = 11.0;
const ICONO: f64 = 14.0;
const ENTRE_ICONOS: f64 = 10.0;
const APRETADO: f32 = 0.92;
pub const TRANSICION: Duration = Duration::from_millis(150);
/// El `Revealer` de la bandeja.
pub const REVELAR: Duration = Duration::from_millis(300);

const CAMPANA: &str = "󰨝";
const FLECHA: &str = "󰅁";

/// El globo: borde, relleno del globo y de la etiqueta.
const GLOBO_X: f64 = 1.0 + 10.0 + 4.0;
const GLOBO_Y: f64 = 1.0 + 6.0 + 1.0;
/// Lo que ocupa la sombra (`0 4px 12px`) alrededor del globo: izquierda, arriba, derecha, abajo.
pub const GLOBO_SOMBRA: [f64; 4] = [12.0, 8.0, 12.0, 16.0];

/// La imagen va a 14 px lógicos, sea del tamaño que sea (se arma para la escala más alta).
#[derive(Clone)]
pub struct Icono {
    pub clave: String,
    pub tooltip: String,
    pub imagen: Option<ImageData>,
}

/// Lo que se ve en la barra, igual en todas las salidas salvo los workspaces.
#[derive(Default)]
pub struct Estado {
    pub espacios: Espacios,
    pub fijos: BTreeMap<String, Vec<i64>>,
    pub fecha: String,
    pub hora: String,
    pub avisos: usize,
    pub no_molestar: bool,
    pub centro: bool,
    pub privacidad: String,
    pub tooltip_privacidad: String,
    pub grabacion: Option<String>,
    pub bandeja: Vec<Icono>,
    /// La clave del ícono con su menú abierto.
    pub menu: Option<String>,
}

pub const TOOLTIP_GRABACION: &str = "Grabando la pantalla\nClick: parar";

impl Estado {
    pub fn tooltip(&self, q: Que) -> Option<String> {
        let t = match q {
            Que::Campana => modelo::campana(self.avisos, self.no_molestar).1,
            Que::Privacidad => self.tooltip_privacidad.clone(),
            Que::Grabacion => TOOLTIP_GRABACION.into(),
            Que::Icono(i) => self.bandeja.get(i)?.tooltip.clone(),
            _ => return None,
        };
        (!t.is_empty()).then_some(t)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Que {
    Espacio(i64),
    Reloj,
    Grabacion,
    Privacidad,
    Flecha,
    Icono(usize),
    Campana,
}

impl Que {
    /// Lo que no se aprieta ni cambia con el puntero.
    pub fn quieto(self) -> bool {
        matches!(self, Que::Privacidad | Que::Flecha)
    }
}

/// Los textos de una salida, en píxeles del buffer. Se rehacen cuando cambia el estado.
pub struct Rotulos {
    pub version: u64,
    pub escala: f32,
    espacios: Vec<(Boton, Layout<u8>)>,
    fecha: Layout<u8>,
    separador: Layout<u8>,
    hora: Layout<u8>,
    campana: Layout<u8>,
    flecha: Layout<u8>,
    privacidad: Option<Layout<u8>>,
    grabacion: Option<Layout<u8>>,
}

impl Rotulos {
    pub fn armar(t: &mut Textos, e: &Estado, salida: &str, escala: f32, version: u64) -> Self {
        let fijos = e.fijos.get(salida).map(Vec::as_slice).unwrap_or_default();
        let espacios = modelo::botones(&e.espacios, salida, fijos)
            .into_iter()
            .map(|b| {
                let peso = if b.activo { 700.0 } else { 500.0 };
                let l = t.armar_con_peso(&[(&b.etiqueta, ROL)], PX, peso, escala);
                (b, l)
            })
            .collect();
        let opcional =
            |t: &mut Textos, s: &str, px| (!s.is_empty()).then(|| t.armar(&[(s, ROL)], px, escala));
        Rotulos {
            version,
            escala,
            espacios,
            fecha: t.armar_con_peso(&[(&e.fecha, ROL)], pt(10.0), 500.0, escala),
            separador: t.armar(&[("›", ROL)], PX_SEPARADOR, escala),
            hora: t.armar_con_peso(&[(&e.hora, ROL)], pt(10.0), 700.0, escala),
            campana: t.armar(&[(CAMPANA, ROL)], PX_GLIFO, escala),
            flecha: t.armar(&[(FLECHA, ROL)], PX, escala),
            privacidad: opcional(t, &e.privacidad, PX_GLIFO),
            grabacion: e
                .grabacion
                .as_deref()
                .and_then(|g| opcional(t, &format!("󰑋 {g}"), PX)),
        }
    }

    fn ancho(&self, l: &Layout<u8>) -> f64 {
        texto::caja(l).0 / f64::from(self.escala)
    }
}

pub struct Pieza {
    pub que: Que,
    /// Lo que ocupa en la barra (donde responde al puntero), en píxeles lógicos.
    pub caja: Rect,
    /// Sin el relleno: donde va el texto o la imagen.
    pub contenido: Rect,
}

/// Dónde cae cada cosa en una barra de `ancho` píxeles lógicos.
pub struct Lugar {
    pub piezas: Vec<Pieza>,
    /// La flecha y lo que se ve de la bandeja: pasar por acá la abre.
    pub cajon: Rect,
    /// Lo que se ve de la bandeja mientras se desliza.
    pub revelador: Rect,
    reloj: [Rect; 3],
}

/// `revelado`: cuánto se ve de la bandeja, de 0 a 1, ya con su curva.
pub fn ubicar(ancho: f64, rot: &Rotulos, iconos: usize, revelado: f64) -> Lugar {
    let h = f64::from(ALTO);
    let mut piezas = Vec::new();
    let mut x = 10.0;
    for (b, l) in &rot.espacios {
        x += 1.0;
        let w = 3.0 + rot.ancho(l) + 3.0;
        piezas.push(Pieza {
            que: Que::Espacio(b.id),
            caja: Rect::new(x, 0.0, x + w, h),
            contenido: Rect::new(x + 3.0, 2.0, x + w - 3.0, h),
        });
        x += w + 1.0;
    }

    let (fw, sw, hw) = (
        rot.ancho(&rot.fecha),
        rot.ancho(&rot.separador),
        rot.ancho(&rot.hora),
    );
    let rw = fw + sw + hw + 28.0;
    // `GtkCenterBox` centra en píxeles enteros.
    let x0 = ((ancho - rw) / 2.0).floor();
    piezas.push(Pieza {
        que: Que::Reloj,
        caja: Rect::new(x0, 0.0, x0 + rw, h),
        contenido: Rect::new(x0, 0.0, x0 + rw, h - 2.0),
    });
    let xs = x0 + 10.0 + fw + 4.0;
    let xh = xs + sw + 4.0 + 5.0;
    let reloj = [
        Rect::new(x0 + 5.0, 0.0, x0 + 5.0 + fw, h - 2.0),
        Rect::new(xs, 0.0, xs + sw, h - 2.0),
        Rect::new(xh, 0.0, xh + hw, h - 2.0),
    ];

    let mut x = ancho - 10.0;
    let cw = 4.0 + rot.ancho(&rot.campana) + 3.0;
    piezas.push(Pieza {
        que: Que::Campana,
        caja: Rect::new(x - cw, 0.0, x, h),
        contenido: Rect::new(x - cw + 4.0, 0.0, x - 3.0, h),
    });
    x -= cw + 2.0;

    // La bandeja entra desde la derecha (`SlideLeft`): su borde derecho queda fijo y el
    // revelador la recorta a la izquierda.
    let bw = 10.0 + iconos as f64 * ICONO + iconos.saturating_sub(1) as f64 * ENTRE_ICONOS;
    let rv = (bw * revelado.clamp(0.0, 1.0)).ceil();
    let revelador = Rect::new(x - rv, 0.0, x, h);
    let origen = x - bw + 2.0;
    let y = ((h - ICONO) / 2.0).floor();
    for i in 0..iconos {
        let ix = origen + i as f64 * (ICONO + ENTRE_ICONOS);
        if ix + ICONO <= revelador.x0 {
            continue;
        }
        piezas.push(Pieza {
            que: Que::Icono(i),
            caja: Rect::new(ix.max(revelador.x0), 0.0, ix + ICONO, h),
            contenido: Rect::new(ix, y, ix + ICONO, y + ICONO),
        });
    }
    x -= rv;
    let flw = 4.0 + rot.ancho(&rot.flecha) + 4.0;
    piezas.push(Pieza {
        que: Que::Flecha,
        caja: Rect::new(x - flw, 0.0, x, h),
        contenido: Rect::new(x - flw + 4.0, 0.0, x - 4.0, h),
    });
    let cajon = Rect::new(x - flw, 0.0, revelador.x1, h);
    x -= flw;

    if let Some(l) = &rot.privacidad {
        let w = 4.0 + rot.ancho(l) + 4.0;
        piezas.push(Pieza {
            que: Que::Privacidad,
            caja: Rect::new(x - w, 0.0, x, h),
            contenido: Rect::new(x - w + 4.0, 0.0, x - 4.0, h),
        });
        x -= w;
    }
    if let Some(l) = &rot.grabacion {
        let w = 4.0 + rot.ancho(l) + 6.0;
        piezas.push(Pieza {
            que: Que::Grabacion,
            caja: Rect::new(x - w, 0.0, x, h),
            contenido: Rect::new(x - w + 4.0, 0.0, x - 6.0, h),
        });
    }
    Lugar {
        piezas,
        cajon,
        revelador,
        reloj,
    }
}

impl Lugar {
    pub fn bajo(&self, x: f64, y: f64) -> Option<Que> {
        let p = Point::new(x, y);
        self.piezas
            .iter()
            .find(|q| q.caja.contains(p))
            .map(|q| q.que)
    }

    pub fn pieza(&self, q: Que) -> Option<&Pieza> {
        self.piezas.iter().find(|p| p.que == q)
    }
}

/// El puntero encima (o lo suyo abierto) y lo apretado de cada pieza, con sus transiciones.
#[derive(Default)]
pub struct Animos(Vec<(Que, Transicion, Transicion)>);

impl Animos {
    fn entrada(&mut self, q: Que) -> &mut (Que, Transicion, Transicion) {
        let i = match self.0.iter().position(|(x, ..)| *x == q) {
            Some(i) => i,
            None => {
                self.0.push((q, Transicion::new(0.0), Transicion::new(1.0)));
                self.0.len() - 1
            }
        };
        &mut self.0[i]
    }

    /// Hacia encendido (1) o no (0).
    pub fn encender(&mut self, q: Que, si: bool, ahora: Instant) {
        let v = if si { 1.0 } else { 0.0 };
        self.entrada(q)
            .1
            .ir(v, crate::animaciones::dur(TRANSICION), ahora);
    }

    /// Apretar achica de golpe; soltar vuelve con su transición.
    pub fn apretar(&mut self, q: Que, si: bool, ahora: Instant) {
        let e = self.entrada(q);
        if si {
            e.2.ir(APRETADO, Duration::ZERO, ahora);
        } else {
            e.2.ir(1.0, crate::animaciones::dur(TRANSICION), ahora);
        }
    }

    fn valores(&self, q: Que, ahora: Instant) -> (f32, f32) {
        self.0
            .iter()
            .find(|(x, ..)| *x == q)
            .map_or((0.0, 1.0), |(_, h, e)| (h.valor(ahora), e.valor(ahora)))
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        self.0
            .iter()
            .any(|(_, h, e)| h.anda(ahora) || e.anda(ahora))
    }
}

/// La opacidad del latido de la grabación (`grabando`: 2 s, de 1 a 0,55 y de vuelta).
pub fn latido(desde: Instant, ahora: Instant) -> f32 {
    let f = (ahora.saturating_duration_since(desde).as_secs_f32() / 2.0).fract();
    let (a, b, t) = if f < 0.5 {
        (1.0, 0.55, f * 2.0)
    } else {
        (0.55, 1.0, f * 2.0 - 1.0)
    };
    a + (b - a) * ease_in_out(t)
}

/// Un color con la opacidad del elemento encima de la suya.
fn opaco(c: &Color, op: f32) -> Color {
    con_alfa(c, c[3] * op)
}

pub struct Pintura<'a> {
    pub estado: &'a Estado,
    pub rotulos: &'a Rotulos,
    pub lugar: &'a Lugar,
    pub animos: &'a Animos,
    pub paleta: &'a Paleta,
    pub opacidad: f32,
    pub ahora: Instant,
    /// La opacidad de la grabación en este cuadro.
    pub latido: f32,
}

impl Pintura<'_> {
    /// La barra entera, de `ancho` × `alto` lógicos, en píxeles del buffer.
    pub fn pintar(&self, s: &mut Scene, (ancho, alto): (u32, u32), escala: f64) {
        s.reset();
        let p = self.paleta;
        let tr = Affine::scale(escala);
        let fondo = con_alfa(&p.surface, p.surface[3] * self.opacidad);
        s.fill(
            Fill::NonZero,
            tr,
            pincel(&fondo),
            None,
            &Rect::new(0.0, 0.0, f64::from(ancho), f64::from(alto)),
        );
        for pieza in &self.lugar.piezas {
            let (hv, k) = self.animos.valores(pieza.que, self.ahora);
            // Apretado: todo lo de la pieza se achica hacia su centro.
            let c = pieza.caja.center();
            let k = f64::from(k);
            let t = Affine::translate((c.x * escala, c.y * escala))
                * Affine::scale(k)
                * Affine::translate((-c.x * escala, -c.y * escala));
            match pieza.que {
                Que::Espacio(id) => {
                    let Some((b, l)) = self.rotulos.espacios.iter().find(|(b, _)| b.id == id)
                    else {
                        continue;
                    };
                    let base = if b.urgente {
                        p.error
                    } else if b.activo {
                        p.primary
                    } else {
                        p.on_surface
                    };
                    let encima = if b.urgente {
                        p.error
                    } else if b.activo {
                        p.tertiary
                    } else {
                        p.primary
                    };
                    let op = if b.urgente || b.activo || !b.vacio {
                        1.0
                    } else {
                        0.4
                    };
                    let color = opaco(&mezclar(&base, &encima, hv), op + (1.0 - op) * hv);
                    texto_en(s, l, pieza.contenido, escala, t, color, false);
                }
                Que::Reloj => {
                    let medio = mezclar(&p.primary, &p.tertiary, 0.5);
                    let r = self.rotulos;
                    let [f, sep, h] = self.lugar.reloj;
                    texto_en(
                        s,
                        &r.fecha,
                        f,
                        escala,
                        t,
                        mezclar(&p.on_surface, &p.primary, hv),
                        false,
                    );
                    texto_en(
                        s,
                        &r.separador,
                        sep,
                        escala,
                        t,
                        opaco(&mezclar(&p.primary, &medio, hv), 0.45 + 0.45 * hv),
                        false,
                    );
                    texto_en(
                        s,
                        &r.hora,
                        h,
                        escala,
                        t,
                        mezclar(&p.primary, &p.tertiary, hv),
                        false,
                    );
                }
                Que::Campana => {
                    let e = self.estado;
                    let con_avisos = e.avisos > 0;
                    let base = if e.no_molestar {
                        opaco(&p.on_surface_variant, 0.5)
                    } else if con_avisos {
                        p.primary
                    } else {
                        opaco(&p.on_surface, 0.9)
                    };
                    let encima = if con_avisos && !e.no_molestar {
                        p.tertiary
                    } else {
                        p.primary
                    };
                    texto_en(
                        s,
                        &self.rotulos.campana,
                        pieza.contenido,
                        escala,
                        t,
                        mezclar(&base, &encima, hv),
                        true,
                    );
                }
                Que::Flecha => {
                    let c = opaco(
                        &mezclar(&p.on_surface_variant, &p.primary, hv),
                        0.65 + 0.35 * hv,
                    );
                    texto_en(s, &self.rotulos.flecha, pieza.contenido, escala, t, c, true);
                }
                Que::Icono(i) => {
                    let Some(img) = self.estado.bandeja.get(i).and_then(|b| b.imagen.as_ref())
                    else {
                        continue;
                    };
                    s.push_clip_layer(Fill::NonZero, tr, &self.lugar.revelador);
                    let r = pieza.contenido;
                    let e = ICONO / f64::from(img.width.max(1));
                    let pos = tr
                        * Affine::translate((c.x, c.y))
                        * Affine::scale(k)
                        * Affine::translate((r.x0 - c.x, r.y0 - c.y))
                        * Affine::scale(e);
                    let brocha = ImageBrush::new(img.clone()).with_alpha(0.8 + 0.2 * hv);
                    s.draw_image(&brocha, pos);
                    s.pop_layer();
                }
                Que::Privacidad => {
                    if let Some(l) = &self.rotulos.privacidad {
                        texto_en(s, l, pieza.contenido, escala, t, p.error, false);
                    }
                }
                Que::Grabacion => {
                    if let Some(l) = &self.rotulos.grabacion {
                        let c = opaco(&p.error, self.latido);
                        texto_en(s, l, pieza.contenido, escala, t, c, false);
                    }
                }
            }
        }
    }
}

/// `l` en `r` (lógicos), como una etiqueta de GTK; un glifo suelto, centrado por su tinta.
fn texto_en(s: &mut Scene, l: &Layout<u8>, r: Rect, escala: f64, t: Affine, c: Color, glifo: bool) {
    let centro = r.center();
    let (cx, cy) = (centro.x * escala, centro.y * escala);
    // De alto, la caja de una etiqueta de GTK (parley la da un píxel corta).
    let (x, y) = if glifo {
        texto::esquina_por_tinta(l, cx, cy)
    } else {
        let (x, _) = texto::esquina(l, cx, cy);
        (x, (cy - texto::alto_de_linea(l) / 2.0).floor())
    };
    texto::dibujar_en(s, l, t * Affine::translate((x, y)), |_| c, 1.0);
}

/// El texto de un globo, en píxeles del buffer.
pub fn armar_globo(t: &mut Textos, texto: &str, escala: f32) -> Layout<u8> {
    t.armar(&[(texto, ROL)], PX_GLOBO, escala)
}

/// El globo sin la sombra, en lógicos.
pub fn globo_caja(l: &Layout<u8>, escala: f32) -> (f64, f64) {
    let e = f64::from(escala);
    let renglones = l.lines().count().max(1) as f64;
    let w = texto::caja(l).0 / e;
    let h = texto::alto_de_linea(l) * renglones / e;
    ((w + 2.0 * GLOBO_X).ceil(), (h + 2.0 * GLOBO_Y).ceil())
}

/// El tamaño de la superficie del globo, con la sombra.
pub fn globo_tam(l: &Layout<u8>, escala: f32) -> (u32, u32) {
    let (w, h) = globo_caja(l, escala);
    let [i, a, d, b] = GLOBO_SOMBRA;
    ((w + i + d) as u32, (h + a + b) as u32)
}

pub fn pintar_globo(s: &mut Scene, l: &Layout<u8>, p: &Paleta, escala: f64) {
    s.reset();
    let tr = Affine::scale(escala);
    let (w, h) = globo_caja(l, escala as f32);
    let [i, a, ..] = GLOBO_SOMBRA;
    let r = Rect::new(i, a, i + w, a + h);
    let rr = RoundedRect::from_rect(r, 4.0);
    // La sombra solo afuera: el fondo es apenas translúcido.
    let mut afuera = BezPath::new();
    afuera.extend(r.inflate(20.0, 20.0).path_elements(0.1));
    afuera.extend(rr.path_elements(0.1));
    s.push_layer(Fill::EvenOdd, BlendMode::default(), 1.0, tr, &afuera);
    sombra(s, tr, r, 4.0, [0.0, 0.0, 0.0, 0.5], (0.0, 4.0), 0.0, 12.0);
    s.pop_layer();
    // `background-clip: padding-box`: el fondo adentro del borde.
    let adentro = RoundedRect::from_rect(r.inset(-1.0), 3.0);
    s.fill(
        Fill::NonZero,
        tr,
        pincel(&opaco(&p.surface_container, 0.95)),
        None,
        &adentro,
    );
    let mut borde = BezPath::new();
    borde.extend(rr.path_elements(0.1));
    borde.extend(adentro.path_elements(0.1));
    s.fill(
        Fill::EvenOdd,
        tr,
        pincel(&opaco(&p.primary, 0.4)),
        None,
        &borde,
    );
    let (x, y) = ((r.x0 + GLOBO_X) * escala, (r.y0 + GLOBO_Y) * escala);
    texto::dibujar_en(
        s,
        l,
        Affine::translate((x.round(), y.round())),
        |_| p.on_surface,
        1.0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_latido_va_y_vuelve() {
        let t0 = Instant::now();
        let s = Duration::from_secs_f32;
        assert!((latido(t0, t0) - 1.0).abs() < 1e-4);
        assert!((latido(t0, t0 + s(1.0)) - 0.55).abs() < 1e-4);
        assert!((latido(t0, t0 + s(2.0)) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn apretar_achica_de_golpe_y_soltar_vuelve() {
        let t0 = Instant::now();
        let mut a = Animos::default();
        a.apretar(Que::Reloj, true, t0);
        assert_eq!(a.valores(Que::Reloj, t0).1, APRETADO);
        a.apretar(Que::Reloj, false, t0);
        assert!(a.anda(t0));
        assert_eq!(a.valores(Que::Reloj, t0 + Duration::from_secs(1)).1, 1.0);
        assert_eq!(a.valores(Que::Campana, t0), (0.0, 1.0));
    }
}
