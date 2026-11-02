//! El dock y su globo en una `Scene` de vello: la tarjeta de las otras superficies, los íconos
//! de las apps y sus marcas.

use morgan_motor::color::{Color, Paleta, con_alfa, pincel};
use morgan_motor::forma::{self, Anillo, Borde, degrade_horizontal, sombra, tarjeta};
use morgan_motor::parley::Layout;
use morgan_motor::texto::{self, Textos};
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Circle, Rect, RoundedRect};
use morgan_motor::vello::peniko::{Fill, ImageData};

use super::modelo::{ALTO_MARCAS, Disposicion, ICONO, Marca, Pieza, marcas_en};

const RADIO_CELDA: f64 = 6.0;
const ALTO_PUNTO: f64 = 5.0;
const ROL: u8 = 0;

const PX_GLOBO: f32 = 12.0;
/// El lugar de la sombra alrededor de la caja.
pub const GLOBO_MARGEN: f64 = 8.0;
const GLOBO_RELLENO: f64 = 6.0;
const GLOBO_RADIO: f64 = 6.0;
const ALTO_TITULO: f64 = 24.0;
const ALTO_FILA: f64 = 24.0;
const COLUMNA_ESPACIO: f64 = 34.0;
const SANGRIA: f64 = 6.0;
const ANCHO_MIN: f64 = 150.0;
const ANCHO_MAX: f64 = 420.0;
/// Más ventanas de una app no entran en un globo.
pub const TOPE_FILAS: usize = 10;

/// El fondo deja ver más que las otras superficies: está siempre a la vista, y lo que hay atrás
/// es el fondo de pantalla. Sobre el 0,5 de `ignore_alpha` (la regla de la capa), o se queda
/// sin blur.
fn opacidad_isla(opacidad: f32) -> f32 {
    (opacidad * 0.7).max(0.55)
}

/// `arriba`: cuánto de la isla ya subió, de 0 (afuera, abajo) a 1.
#[allow(clippy::too_many_arguments)]
pub fn pintar_dock(
    s: &mut Scene,
    d: &Disposicion,
    piezas: &[(&Pieza, &ImageData)],
    encima: &[f32],
    arriba: f64,
    p: &Paleta,
    opacidad: f32,
    borde: Borde,
    (_, alto): (u32, u32),
    escala: f64,
) {
    s.reset();
    let Some(isla) = d.isla else { return };
    if arriba <= 0.0 {
        return;
    }
    let bajar = (1.0 - arriba) * f64::from(alto);
    let tr = Affine::scale(escala) * Affine::translate((0.0, bajar));
    tarjeta(s, tr, p, opacidad_isla(opacidad), borde, isla, Anillo::Bicolor);
    if let Some(x) = d.raya {
        let y0 = d.celdas[0].y0 + 8.0;
        let y1 = d.celdas[0].y1 - 8.0;
        s.fill(
            Fill::NonZero,
            tr,
            pincel(&p.outline_variant),
            None,
            &Rect::new(x, y0, x + 1.0, y1),
        );
    }
    for (i, (celda, (pieza, icono))) in d.celdas.iter().zip(piezas).enumerate() {
        let tinte = if pieza.foco {
            Some(con_alfa(&p.primary, 0.14))
        } else {
            let hv = encima.get(i).copied().unwrap_or(0.0);
            (hv > 0.0).then(|| con_alfa(&p.on_surface, 0.08 * hv))
        };
        if let Some(c) = tinte {
            s.fill(
                Fill::NonZero,
                tr,
                pincel(&c),
                None,
                &RoundedRect::from_rect(*celda, RADIO_CELDA),
            );
        }
        let c = celda.center();
        let m = ICONO / 2.0;
        forma::imagen(
            s,
            tr,
            icono,
            Rect::new(c.x - m, c.y - m, c.x + m, c.y + m),
            1.0,
        );
        marcas(s, tr, c.x, celda.y1 + 4.0, &pieza.marcas, p);
    }
}

fn marcas(s: &mut Scene, tr: Affine, cx: f64, y: f64, m: &[Marca], p: &Paleta) {
    let yc = y + ALTO_MARCAS / 2.0;
    for (marca, (x, ancho)) in m.iter().zip(marcas_en(cx, m)) {
        match marca {
            Marca::Foco => {
                let r = Rect::new(x, yc - ALTO_PUNTO / 2.0, x + ancho, yc + ALTO_PUNTO / 2.0);
                s.fill(
                    Fill::NonZero,
                    tr,
                    &degrade_horizontal(r, p.primary, p.tertiary),
                    None,
                    &RoundedRect::from_rect(r, ALTO_PUNTO / 2.0),
                );
            }
            Marca::Aca | Marca::Otro => {
                let c: Color = if *marca == Marca::Aca {
                    p.primary
                } else {
                    con_alfa(&p.on_surface_variant, 0.5)
                };
                s.fill(
                    Fill::NonZero,
                    tr,
                    pincel(&c),
                    None,
                    &Circle::new((x + ancho / 2.0, yc), ALTO_PUNTO / 2.0),
                );
            }
        }
    }
}

/// El nombre de la app y, debajo, cada ventana con su workspace; o, sin ventanas, qué hace el
/// click. En píxeles del buffer, como los demás textos.
pub struct Rotulos {
    titulo: Layout<u8>,
    nota: Option<Layout<u8>>,
    filas: Vec<(Layout<u8>, Layout<u8>)>,
    foco: Option<usize>,
    /// El tamaño de la capa, en lógicos.
    pub tam: (u32, u32),
}

impl Rotulos {
    pub fn armar(t: &mut Textos, pieza: &Pieza, nota: &str, escala: f32) -> Self {
        let e = f64::from(escala);
        let interno = 2.0 * (GLOBO_MARGEN + GLOBO_RELLENO + SANGRIA);
        let max = ((ANCHO_MAX - interno) * e) as f32;
        let titulo =
            texto::armar_recortado_con_peso(t, &pieza.nombre, ROL, PX_GLOBO, 700.0, escala, max);
        let mut ancho = texto::caja(&titulo).0 / e;
        let nota = pieza.filas.is_empty().then(|| {
            let l = texto::armar_recortado(t, nota, ROL, PX_GLOBO, escala, max);
            ancho = ancho.max(texto::caja(&l).0 / e);
            l
        });
        let filas: Vec<_> = pieza
            .filas
            .iter()
            .take(TOPE_FILAS)
            .map(|f| {
                let peso = if f.foco { 700.0 } else { 500.0 };
                let esp = t.armar_con_peso(&[(&f.espacio, ROL)], PX_GLOBO, peso, escala);
                let libre = max - (COLUMNA_ESPACIO * e) as f32;
                let tit = texto::armar_recortado(t, &f.titulo, ROL, PX_GLOBO, escala, libre);
                ancho = ancho.max(COLUMNA_ESPACIO + texto::caja(&tit).0 / e);
                (esp, tit)
            })
            .collect();
        let foco = pieza.filas.iter().take(TOPE_FILAS).position(|f| f.foco);
        let w = (ancho + interno).clamp(ANCHO_MIN + 2.0 * GLOBO_MARGEN, ANCHO_MAX);
        let renglones = if nota.is_some() { 1 } else { filas.len() };
        let h = 2.0 * (GLOBO_MARGEN + GLOBO_RELLENO) + ALTO_TITULO + renglones as f64 * ALTO_FILA;
        Rotulos {
            titulo,
            nota,
            filas,
            foco,
            tam: (w.ceil() as u32, h.ceil() as u32),
        }
    }

    fn fila(&self, i: usize) -> Rect {
        let x0 = GLOBO_MARGEN + GLOBO_RELLENO;
        let y0 = x0 + ALTO_TITULO + i as f64 * ALTO_FILA;
        Rect::new(x0, y0, f64::from(self.tam.0) - x0, y0 + ALTO_FILA)
    }

    /// La fila bajo el puntero.
    pub fn fila_en(&self, x: f64, y: f64) -> Option<usize> {
        (0..self.filas.len()).find(|&i| self.fila(i).contains((x, y)))
    }
}

pub fn pintar_globo(
    s: &mut Scene,
    r: &Rotulos,
    encima: Option<usize>,
    p: &Paleta,
    opacidad: f32,
    escala: f64,
) {
    s.reset();
    let tr = Affine::scale(escala);
    let (w, h) = (f64::from(r.tam.0), f64::from(r.tam.1));
    let caja = Rect::new(GLOBO_MARGEN, GLOBO_MARGEN, w - GLOBO_MARGEN, h - GLOBO_MARGEN);
    let negro = |a| [0.0, 0.0, 0.0, a];
    sombra(s, tr, caja, GLOBO_RADIO, negro(0.5), (0.0, 2.0), 0.0, 8.0);
    sombra(s, tr, caja, GLOBO_RADIO, negro(1.0), (0.0, 0.0), 1.0, 0.0);
    let fondo = con_alfa(&p.surface_container, p.surface_container[3] * opacidad);
    s.fill(
        Fill::NonZero,
        tr,
        pincel(&fondo),
        None,
        &RoundedRect::from_rect(caja, GLOBO_RADIO),
    );

    let en = |x: f64, y: f64, l: &Layout<u8>| {
        let alto = texto::alto_de_linea(l);
        (
            (x * escala).floor(),
            ((y * escala) + (ALTO_FILA * escala - alto) / 2.0).floor(),
        )
    };
    let x0 = GLOBO_MARGEN + GLOBO_RELLENO + SANGRIA;
    let y0 = GLOBO_MARGEN + GLOBO_RELLENO;
    let escribir = |s: &mut Scene, l: &Layout<u8>, (x, y): (f64, f64), c: Color| {
        texto::dibujar_en(s, l, Affine::translate((x, y)), |_| c, 1.0);
    };
    escribir(s, &r.titulo, en(x0, y0, &r.titulo), p.on_surface);
    if let Some(l) = &r.nota {
        escribir(s, l, en(x0, y0 + ALTO_TITULO, l), p.on_surface_variant);
    }
    for (i, (esp, tit)) in r.filas.iter().enumerate() {
        let caja = r.fila(i);
        if encima == Some(i) {
            s.fill(
                Fill::NonZero,
                tr,
                pincel(&con_alfa(&p.primary, 0.14)),
                None,
                &RoundedRect::from_rect(caja, 4.0),
            );
        }
        let color_esp = if r.foco == Some(i) { p.primary } else { p.on_surface_variant };
        escribir(s, esp, en(x0, caja.y0, esp), color_esp);
        escribir(s, tit, en(x0 + COLUMNA_ESPACIO, caja.y0, tit), p.on_surface);
    }
}
