//! El dock y su globo en una `Scene` de vello: la tarjeta de las otras superficies, los íconos
//! de las apps y sus marcas; el globo, con el marco y las medidas del menú de la bandeja.

use morgan_motor::color::{Color, Paleta, con_alfa, pincel};
use morgan_motor::forma::menu::{
    BORDE_Y_RELLENO, BOTON_X, ENTRE, ENTRE_BLOQUES, FILA_X, FILA_Y, LISTA, MIN, PX, RADIO_FILA,
    RADIO_RECUADRO, TINTE_FILA, TINTE_RECUADRO,
};
use morgan_motor::forma::{self, Anillo, Borde, degrade_horizontal, marco, tarjeta};
use morgan_motor::parley::Layout;
use morgan_motor::texto::{self, Textos};
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Circle, Rect, RoundedRect};
use morgan_motor::vello::peniko::{Fill, ImageData};

use super::modelo::{ALTO_MARCAS, Disposicion, ICONO, Marca, Pieza, marcas_en};

const RADIO_CELDA: f64 = 6.0;
const ALTO_PUNTO: f64 = 5.0;
const ROL: u8 = 0;

/// El lugar de la sombra alrededor de la caja.
pub const GLOBO_MARGEN: f64 = 8.0;
/// Lo que ocupa el workspace, si todos son cortos.
const COLUMNA_MIN: f64 = 24.0;
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

/// El nombre de la app y, debajo, cada ventana con su workspace en un recuadro; o, sin ventanas,
/// qué hace el click. En píxeles del buffer, como los demás textos.
pub struct Rotulos {
    titulo: Layout<u8>,
    nota: Option<Layout<u8>>,
    filas: Vec<(Layout<u8>, Layout<u8>)>,
    foco: Option<usize>,
    /// Del inicio del workspace al del título.
    columna: f64,
    alto_fila: f64,
    /// El tamaño de la capa, en lógicos.
    pub tam: (u32, u32),
}

impl Rotulos {
    pub fn armar(t: &mut Textos, pieza: &Pieza, nota: &str, escala: f32) -> Self {
        let e = f64::from(escala);
        let interno = 2.0 * (GLOBO_MARGEN + BORDE_Y_RELLENO + BOTON_X);
        let max = ((ANCHO_MAX - interno) * e) as f32;
        let titulo = texto::armar_recortado_con_peso(t, &pieza.nombre, ROL, PX, 700.0, escala, max);
        let alto_fila = FILA_Y * 2.0 + (texto::alto_de_linea(&titulo) / e).ceil();
        let mut ancho = texto::caja(&titulo).0 / e;
        let nota = pieza.filas.is_empty().then(|| {
            let l = texto::armar_recortado(t, nota, ROL, PX, escala, max);
            ancho = ancho.max(texto::caja(&l).0 / e);
            l
        });
        let visibles = || pieza.filas.iter().take(TOPE_FILAS);
        let espacios: Vec<_> = visibles()
            .map(|f| {
                let peso = if f.foco { 700.0 } else { 500.0 };
                t.armar_con_peso(&[(&f.espacio, ROL)], PX, peso, escala)
            })
            .collect();
        let columna = espacios
            .iter()
            .map(|l| texto::caja(l).0 / e)
            .fold(COLUMNA_MIN, f64::max)
            .ceil()
            + ENTRE;
        let libre = max - (columna * e) as f32;
        let filas: Vec<_> = espacios
            .into_iter()
            .zip(visibles())
            .map(|(esp, f)| {
                let tit = texto::armar_recortado(t, &f.titulo, ROL, PX, escala, libre);
                ancho = ancho.max(columna + texto::caja(&tit).0 / e);
                (esp, tit)
            })
            .collect();
        let foco = visibles().position(|f| f.foco);
        let w = (ancho + interno).clamp(MIN + 2.0 * GLOBO_MARGEN, ANCHO_MAX);
        let abajo = if nota.is_some() {
            alto_fila
        } else {
            ENTRE_BLOQUES + 2.0 * LISTA + filas.len() as f64 * alto_fila
        };
        let h = 2.0 * (GLOBO_MARGEN + BORDE_Y_RELLENO) + alto_fila + abajo;
        Rotulos {
            titulo,
            nota,
            filas,
            foco,
            columna,
            alto_fila,
            tam: (w.ceil() as u32, h.ceil() as u32),
        }
    }

    pub fn cuantas(&self) -> usize {
        self.filas.len()
    }

    fn fila(&self, i: usize) -> Rect {
        let x0 = GLOBO_MARGEN + BORDE_Y_RELLENO + LISTA;
        let y0 = GLOBO_MARGEN
            + BORDE_Y_RELLENO
            + self.alto_fila
            + ENTRE_BLOQUES
            + LISTA
            + i as f64 * self.alto_fila;
        Rect::new(x0, y0, f64::from(self.tam.0) - x0, y0 + self.alto_fila)
    }

    /// La fila bajo el puntero.
    pub fn fila_en(&self, x: f64, y: f64) -> Option<usize> {
        (0..self.filas.len()).find(|&i| self.fila(i).contains((x, y)))
    }
}

/// `tintes`: cuánto se ilumina cada fila, de 0 a 1.
pub fn pintar_globo(
    s: &mut Scene,
    r: &Rotulos,
    tintes: &[f32],
    p: &Paleta,
    opacidad: f32,
    escala: f64,
) {
    s.reset();
    let tr = Affine::scale(escala);
    let (w, h) = (f64::from(r.tam.0), f64::from(r.tam.1));
    let caja = Rect::new(GLOBO_MARGEN, GLOBO_MARGEN, w - GLOBO_MARGEN, h - GLOBO_MARGEN);
    marco(s, tr, escala, p, opacidad, caja);

    let en = |x: f64, y: f64, l: &Layout<u8>| {
        let alto = texto::alto_de_linea(l);
        (
            (x * escala).floor(),
            ((y * escala) + (r.alto_fila * escala - alto) / 2.0).floor(),
        )
    };
    let x0 = GLOBO_MARGEN + BORDE_Y_RELLENO + BOTON_X;
    let y0 = GLOBO_MARGEN + BORDE_Y_RELLENO;
    let escribir = |s: &mut Scene, l: &Layout<u8>, (x, y): (f64, f64), c: Color| {
        texto::dibujar_en(s, l, Affine::translate((x, y)), |_| c, 1.0);
    };
    escribir(s, &r.titulo, en(x0, y0, &r.titulo), p.on_surface);
    if let Some(l) = &r.nota {
        escribir(s, l, en(x0, y0 + r.alto_fila, l), p.on_surface_variant);
    }
    if let Some(ultima) = r.filas.len().checked_sub(1) {
        let (a, b) = (r.fila(0), r.fila(ultima));
        s.fill(
            Fill::NonZero,
            tr,
            pincel(&con_alfa(&p.on_surface, p.on_surface[3] * TINTE_RECUADRO)),
            None,
            &RoundedRect::new(
                a.x0 - LISTA,
                a.y0 - LISTA,
                a.x1 + LISTA,
                b.y1 + LISTA,
                RADIO_RECUADRO,
            ),
        );
    }
    for (i, (esp, tit)) in r.filas.iter().enumerate() {
        let caja = r.fila(i);
        let luz = tintes.get(i).copied().unwrap_or(0.0);
        if luz > 0.0 {
            s.fill(
                Fill::NonZero,
                tr,
                pincel(&con_alfa(&p.primary, p.primary[3] * TINTE_FILA * luz)),
                None,
                &RoundedRect::from_rect(caja, RADIO_FILA),
            );
        }
        let x = caja.x0 + FILA_X;
        let color_esp = if r.foco == Some(i) { p.primary } else { p.on_surface_variant };
        escribir(s, esp, en(x, caja.y0, esp), color_esp);
        escribir(s, tit, en(x + r.columna, caja.y0, tit), p.on_surface);
    }
}

#[cfg(test)]
mod tests {
    use super::super::modelo::Fila;
    use super::*;

    fn pieza(espacios: &[&str]) -> Pieza {
        Pieza {
            clave: "firefox".to_owned(),
            nombre: "Firefox".to_owned(),
            filas: espacios
                .iter()
                .map(|e| Fila {
                    direccion: "0x1".to_owned(),
                    espacio: (*e).to_owned(),
                    titulo: "Wiki".to_owned(),
                    foco: false,
                })
                .collect(),
            marcas: Vec::new(),
            foco: false,
            usada: false,
        }
    }

    #[test]
    fn un_globo_corto_mide_lo_mismo_que_un_menu_corto() {
        let r = Rotulos::armar(&mut Textos::new(), &pieza(&["I"]), "Abrir", 1.0);
        assert_eq!(f64::from(r.tam.0), MIN + 2.0 * GLOBO_MARGEN);
    }

    #[test]
    fn solo_las_filas_reciben_el_puntero() {
        let r = Rotulos::armar(&mut Textos::new(), &pieza(&["I", "II"]), "Abrir", 1.0);
        let (a, b) = (r.fila(0), r.fila(1));
        assert_eq!(a.y1, b.y0, "sin huecos entre filas");
        assert_eq!(r.fila_en(a.center().x, a.center().y), Some(0));
        assert_eq!(r.fila_en(b.center().x, b.center().y), Some(1));
        assert_eq!(r.fila_en(a.center().x, a.y0 - 1.0), None, "el título");
        assert_eq!(r.fila_en(a.x0 - 1.0, a.center().y), None, "el borde del recuadro");
    }

    #[test]
    fn un_workspace_largo_no_pisa_el_titulo() {
        let r = Rotulos::armar(&mut Textos::new(), &pieza(&["XXVIII"]), "Abrir", 1.0);
        let ancho = texto::caja(&r.filas[0].0).0;
        assert!(r.columna >= ancho + ENTRE, "{} {ancho}", r.columna);
    }

    #[test]
    fn sin_ventanas_no_hay_filas() {
        let r = Rotulos::armar(&mut Textos::new(), &pieza(&[]), "Abrir", 1.0);
        assert_eq!(r.cuantas(), 0);
        assert_eq!(r.fila_en(50.0, 50.0), None);
    }
}
