//! Las formas de la hoja de GTK que se repiten: el `box-shadow`.

use vello::Scene;
use vello::kurbo::{Affine, Rect, RoundedRect, Vec2};
use vello::peniko::Fill;

use crate::color::{Color, pincel};

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
