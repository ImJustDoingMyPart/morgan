//! Las formas de la hoja de GTK que se repiten: el `box-shadow` y la caja de los paneles.

use vello::Scene;
use vello::kurbo::{Affine, BezPath, Point, Rect, RoundedRect, Shape, Vec2};
use vello::peniko::{BlendMode, Compose, Fill, Gradient, ImageBrush, ImageData, Mix};

use crate::color::{Color, Paleta, con_alfa, pincel};

/// `box-shadow` de GTK sobre la caja `r` de radio `radio`: agrandada en `spread` (el radio
/// también), desplazada y desenfocada con sigma = blur / 2. Sin blur, un relleno liso.
#[allow(clippy::too_many_arguments)]
pub fn sombra(
    s: &mut Scene,
    tr: Affine,
    r: Rect,
    radio: f64,
    c: Color,
    (dx, dy): (f64, f64),
    spread: f64,
    blur: f64,
) {
    let r = r.inflate(spread, spread) + Vec2::new(dx, dy);
    let radio = (radio + spread).max(0.0);
    if blur <= 0.0 {
        s.fill(
            Fill::NonZero,
            tr,
            pincel(&c),
            None,
            &RoundedRect::from_rect(r, radio),
        );
    } else {
        s.draw_blurred_rounded_rect(tr, r, pincel(&c), radio, blur / 2.0);
    }
}

/// `linear-gradient(135deg, a, b)` de CSS sobre `r`.
pub fn degrade_135(r: Rect, a: Color, b: Color) -> Gradient {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let largo = (r.width() + r.height()) * s;
    let c = r.center();
    let d = s * largo / 2.0;
    Gradient::new_linear(Point::new(c.x - d, c.y - d), Point::new(c.x + d, c.y + d))
        .with_stops([pincel(&a), pincel(&b)].as_slice())
}

/// `linear-gradient(to right, a, b)` de CSS sobre `r`.
pub fn degrade_horizontal(r: Rect, a: Color, b: Color) -> Gradient {
    Gradient::new_linear(Point::new(r.x0, r.y0), Point::new(r.x1, r.y0))
        .with_stops([pincel(&a), pincel(&b)].as_slice())
}

/// Una imagen en `r` (lógicos, bajo `tr`), estirada a su tamaño.
pub fn imagen(s: &mut Scene, tr: Affine, img: &ImageData, r: Rect, alfa: f32) {
    let (w, h) = (f64::from(img.width.max(1)), f64::from(img.height.max(1)));
    let pos = tr * Affine::translate((r.x0, r.y0)) * Affine::scale_non_uniform(r.width() / w, r.height() / h);
    s.draw_image(&ImageBrush::new(img.clone()).with_alpha(alfa), pos);
}

/// Un ícono simbólico (lo que importa es su alfa) pintado de `c`, en `r`.
pub fn mascara(s: &mut Scene, tr: Affine, img: &ImageData, r: Rect, c: Color) {
    s.push_layer(Fill::NonZero, BlendMode::default(), 1.0, tr, &r);
    s.fill(Fill::NonZero, tr, pincel(&c), None, &r);
    s.push_layer(
        Fill::NonZero,
        BlendMode::new(Mix::Normal, Compose::DestIn),
        1.0,
        tr,
        &r,
    );
    imagen(s, tr, img, r, 1.0);
    s.pop_layer();
    s.pop_layer();
}

/// Lo de afuera del anillo de los paneles. Sigue a la sombra de las ventanas (`[appearance]
/// shadow`): negra o el glow del tema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Borde {
    /// Una línea negra de 1 px y una sombra negra suave, centrada: la de las ventanas,
    /// achicada al tamaño de un panel.
    Sombra,
    /// Los dos brillos del anillo, `primary` arriba a la izquierda y `tertiary` abajo a la
    /// derecha, sin línea.
    Brillo,
}

impl Borde {
    /// El id de la sombra de la config; uno que no se conoce, la negra.
    pub fn de(id: &str) -> Borde {
        if id == "glow" {
            Borde::Brillo
        } else {
            Borde::Sombra
        }
    }
}

/// La caja de los paneles (`.control-center` de la hoja de GTK) con su borde de afuera hacia
/// adentro, en `r` (la caja del borde, en lógicos): lo de afuera según `borde`, el anillo de
/// 2 px (`primary` arriba y a la izquierda, `tertiary` abajo y a la derecha, partido en
/// diagonal en las esquinas, como GTK) y el fondo translúcido.
///
/// Todo lo de afuera queda por debajo de 0,5 de alfa (salvo la línea): el blur de la capa
/// (`ignore_alpha` 0,5) no lo alcanza y no se ve una franja desenfocada alrededor.
pub fn panel(s: &mut Scene, tr: Affine, p: &Paleta, opacidad: f32, r: Rect, borde: Borde) {
    const RADIO: f64 = 4.0;
    const BORDE: f64 = 2.0;
    let rr = RoundedRect::from_rect(r, RADIO);
    // Las sombras van solo por fuera: el fondo es translúcido y no tienen que verse a través.
    let mut afuera = BezPath::new();
    afuera.extend(r.inflate(16.0, 16.0).path_elements(0.1));
    afuera.extend(rr.path_elements(0.1));
    s.push_layer(Fill::EvenOdd, BlendMode::default(), 1.0, tr, &afuera);
    let negro = |a| [0.0, 0.0, 0.0, a];
    let capas: &[(Color, (f64, f64), f64, f64)] = match borde {
        Borde::Sombra => &[
            (negro(0.5), (0.0, 0.0), 1.0, 6.0),
            (negro(1.0), (0.0, 0.0), 1.0, 0.0),
        ],
        Borde::Brillo => &[
            (con_alfa(&p.tertiary, 0.3), (1.0, 1.0), 1.0, 5.0),
            (con_alfa(&p.primary, 0.3), (-1.0, -1.0), 1.0, 5.0),
        ],
    };
    for (c, d, spread, blur) in capas {
        sombra(s, tr, r, RADIO, *c, *d, *spread, *blur);
    }
    s.pop_layer();
    let adentro = RoundedRect::from_rect(r.inflate(-BORDE, -BORDE), RADIO - BORDE);
    let fondo = con_alfa(
        &p.surface_container_low,
        p.surface_container_low[3] * opacidad,
    );
    s.fill(Fill::NonZero, tr, pincel(&fondo), None, &adentro);
    let mut anillo = BezPath::new();
    anillo.extend(rr.path_elements(0.1));
    anillo.extend(adentro.path_elements(0.1));
    // Arriba a la izquierda de la diagonal que une las esquinas de adentro: `primary`.
    let mut arriba = BezPath::new();
    arriba.move_to((r.x0, r.y0));
    arriba.line_to((r.x1, r.y0));
    arriba.line_to((r.x1 - BORDE, r.y0 + BORDE));
    arriba.line_to((r.x0 + BORDE, r.y1 - BORDE));
    arriba.line_to((r.x0, r.y1));
    arriba.close_path();
    let mut abajo = BezPath::new();
    abajo.move_to((r.x1, r.y0));
    abajo.line_to((r.x1, r.y1));
    abajo.line_to((r.x0, r.y1));
    abajo.line_to((r.x0 + BORDE, r.y1 - BORDE));
    abajo.line_to((r.x1 - BORDE, r.y0 + BORDE));
    abajo.close_path();
    for (lado, c) in [(arriba, p.primary), (abajo, p.tertiary)] {
        s.push_clip_layer(Fill::NonZero, tr, &lado);
        s.fill(Fill::EvenOdd, tr, pincel(&c), None, &anillo);
        s.pop_layer();
    }
}
