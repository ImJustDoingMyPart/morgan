//! Las formas de la hoja de GTK que se repiten: el `box-shadow` y la caja de los paneles.

use vello::Scene;
use vello::kurbo::{Affine, BezPath, Rect, RoundedRect, Shape, Vec2};
use vello::peniko::{BlendMode, Fill};

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

/// La caja de los paneles (`.control-center` de la hoja de GTK) con su borde de afuera hacia
/// adentro, en `r` (la caja del borde, en lógicos): la línea de 1 px del fondo y los dos
/// brillos por fuera, el anillo de 2 px (`primary` arriba y a la izquierda, `tertiary` abajo y
/// a la derecha, partido en diagonal en las esquinas, como GTK) y el fondo translúcido.
pub fn panel(s: &mut Scene, tr: Affine, p: &Paleta, opacidad: f32, r: Rect) {
    const RADIO: f64 = 4.0;
    const BORDE: f64 = 2.0;
    let rr = RoundedRect::from_rect(r, RADIO);
    // Las sombras van solo por fuera: el fondo es translúcido y no tienen que verse a través.
    let mut afuera = BezPath::new();
    afuera.extend(r.inflate(16.0, 16.0).path_elements(0.1));
    afuera.extend(rr.path_elements(0.1));
    s.push_layer(Fill::EvenOdd, BlendMode::default(), 1.0, tr, &afuera);
    // GTK apila la primera de la lista arriba.
    for (c, d, spread, blur) in [
        (con_alfa(&p.tertiary, 0.45), (1.0, 1.0), 2.0, 5.0),
        (con_alfa(&p.primary, 0.45), (-1.0, -1.0), 2.0, 5.0),
        (con_alfa(&p.surface, 0.85), (0.0, 0.0), 1.0, 0.0),
    ] {
        sombra(s, tr, r, RADIO, c, d, spread, blur);
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
