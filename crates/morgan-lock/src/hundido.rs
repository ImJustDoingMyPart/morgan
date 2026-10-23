//! El interior del campo, apenas hundido: una sombra hacia adentro desde el borde de arriba
//! y una luz finísima en el de abajo (el `box-shadow: inset` de CSS). La caja mide siempre
//! lo mismo, así que se calcula una vez por escala, en CPU, y va como imagen.

use std::sync::Arc;

use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

use crate::sombra::desenfocar;

#[derive(Clone, Copy, Debug)]
pub struct Hundido {
    /// (sigma, alfa, cuánto baja) de la sombra negra que entra desde arriba.
    pub sombra: (f32, f32, f64),
    /// (sigma, alfa, cuánto sube) de la luz blanca que entra desde abajo.
    pub luz: (f32, f32, f64),
}

/// Las dos capas (sombra y luz) para un rectángulo redondeado de `w`×`h` y radio `r`, en
/// píxeles lógicos; las imágenes salen en píxeles del buffer.
pub fn hacer(w: f64, h: f64, r: f64, escala: f64, hd: &Hundido) -> (ImageData, ImageData) {
    let (bw, bh) = ((w * escala).ceil() as usize, (h * escala).ceil() as usize);
    let rr = r * escala;
    // Cuánto de cada píxel cae dentro, con un píxel de transición.
    let dentro = |x: f64, y: f64| -> f32 {
        let (cx, cy) = (bw as f64 / 2.0, bh as f64 / 2.0);
        let (qx, qy) = ((x - cx).abs() - (cx - rr), (y - cy).abs() - (cy - rr));
        let fuera = (qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0)) - rr;
        (0.5 - fuera).clamp(0.0, 1.0) as f32
    };
    let capa = |(sigma, alfa, corre): (f32, f32, f64), color: u8| {
        let s = sigma * escala as f32;
        let m = (3.0 * s).ceil() as usize + (corre.abs() * escala).ceil() as usize + 1;
        let (pw, ph) = (bw + 2 * m, bh + 2 * m);
        // Lo de afuera, corrido: lo que entra por el borde opuesto al corrimiento.
        let mut p = vec![0.0f32; pw * ph];
        for y in 0..ph {
            for x in 0..pw {
                let (fx, fy) = (
                    x as f64 - m as f64 + 0.5,
                    y as f64 - m as f64 + 0.5 - corre * escala,
                );
                p[y * pw + x] = 1.0 - dentro(fx, fy);
            }
        }
        desenfocar(&mut p, pw, ph, s);
        let mut px = Vec::with_capacity(bw * bh * 4);
        for y in 0..bh {
            for x in 0..bw {
                let a = p[(y + m) * pw + x + m] * dentro(x as f64 + 0.5, y as f64 + 0.5) * alfa;
                px.extend([
                    color,
                    color,
                    color,
                    (a * 255.0).round().clamp(0.0, 255.0) as u8,
                ]);
            }
        }
        ImageData {
            data: Blob::new(Arc::new(px)),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: bw as u32,
            height: bh as u32,
        }
    };
    let (ls, la, sube) = hd.luz;
    (capa(hd.sombra, 0), capa((ls, la, -sube), 255))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oscurece_arriba_aclara_abajo_y_no_sale_de_la_caja() {
        let hd = Hundido {
            sombra: (2.0, 0.5, 2.0),
            luz: (1.0, 0.3, 1.0),
        };
        let (sombra, luz) = hacer(100.0, 40.0, 4.0, 1.0, &hd);
        let a = |img: &ImageData, x: usize, y: usize| img.data.data()[(y * 100 + x) * 4 + 3];
        assert!(
            a(&sombra, 50, 1) > a(&sombra, 50, 38),
            "la sombra entra por arriba"
        );
        assert!(a(&luz, 50, 38) > a(&luz, 50, 1), "la luz entra por abajo");
        assert_eq!(a(&sombra, 50, 20), 0, "el centro queda limpio");
        assert_eq!(a(&sombra, 0, 0), 0, "fuera de la esquina redondeada, nada");
    }
}
