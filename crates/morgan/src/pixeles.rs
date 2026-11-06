//! Imágenes en píxeles para las superficies del motor propio: los archivos (una foto, una
//! carátula, una captura) y los píxeles que mandan las apps. Los íconos del tema están en
//! `icono.rs`. Todo sale premultiplicado o no según lo diga su `ImageData`.

use std::sync::Arc;

use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::compositor::Salida;
use morgan_motor::vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

static ESCALA: AtomicI32 = AtomicI32::new(1);
/// El ancho y el alto de la salida más grande, en píxeles, en una sola palabra.
static GRANDE: AtomicU64 = AtomicU64::new(0);

/// La escala más alta de las salidas, redondeada para arriba: las imágenes se arman para ella.
pub fn escala() -> i32 {
    ESCALA.load(Ordering::Relaxed).max(1)
}

/// El tamaño en píxeles de la salida más grande.
pub fn salida_grande() -> Option<(i32, i32)> {
    let v = GRANDE.load(Ordering::Relaxed);
    (v != 0).then_some(((v >> 32) as i32, v as u32 as i32))
}

pub fn poner_salidas(salidas: &[Salida]) {
    let escala = salidas
        .iter()
        .map(|s| s.escala.ceil() as i32)
        .max()
        .unwrap_or(1);
    let grande = salidas
        .iter()
        .max_by_key(|s| i64::from(s.ancho) * i64::from(s.alto))
        .map_or(0, |s| (u64::from(s.ancho as u32) << 32) | u64::from(s.alto as u32));
    ESCALA.store(escala.max(1), Ordering::Relaxed);
    GRANDE.store(grande, Ordering::Relaxed);
}

/// Al arrancar, antes de que nada pida una escala: el compositor contesta en milisegundos.
pub fn leer_salidas() {
    match crate::compositor::salidas_ya() {
        Some(s) => poner_salidas(&s),
        None => eprintln!("morgan: no se pudo leer las salidas del compositor"),
    }
}

pub fn rgba(px: Vec<u8>, w: u32, h: u32) -> ImageData {
    ImageData {
        data: Blob::new(Arc::new(px)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: w,
        height: h,
    }
}

/// `i` achicada (nunca agrandada) para entrar en `ancho` × `alto`, con su proporción.
pub fn achicar(i: image::RgbaImage, ancho: u32, alto: u32) -> ImageData {
    let (w, h) = i.dimensions();
    let k = (f64::from(ancho.max(1)) / f64::from(w.max(1)))
        .min(f64::from(alto.max(1)) / f64::from(h.max(1)));
    let i = if k < 1.0 {
        let (nw, nh) = (
            ((f64::from(w) * k).round() as u32).max(1),
            ((f64::from(h) * k).round() as u32).max(1),
        );
        image::imageops::resize(&i, nw, nh, image::imageops::FilterType::CatmullRom)
    } else {
        i
    };
    let (w, h) = i.dimensions();
    rgba(i.into_raw(), w, h)
}

/// Un archivo de imagen que entre en `lado` × `lado` píxeles, con su proporción.
pub fn archivo(ruta: &std::path::Path, lado: i32) -> Option<ImageData> {
    archivo_en(ruta, lado.max(1) as u32, lado.max(1) as u32)
}

/// Un archivo de imagen que entre en `ancho` × `alto` píxeles, con su proporción.
pub fn archivo_en(ruta: &std::path::Path, ancho: u32, alto: u32) -> Option<ImageData> {
    let i = image::open(ruta)
        .map_err(|e| eprintln!("morgan: imagen {}: {e}", ruta.display()))
        .ok()?
        .into_rgba8();
    Some(achicar(i, ancho, alto))
}

/// El ancho y el alto de una imagen, sin leerla entera.
pub fn medidas(ruta: &std::path::Path) -> Option<(u32, u32)> {
    image::image_dimensions(ruta).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn achica_con_su_proporcion_y_no_agranda() {
        let i = image::RgbaImage::new(400, 200);
        let img = achicar(i, 100, 100);
        assert_eq!((img.width, img.height), (100, 50));
        let i = image::RgbaImage::new(40, 20);
        let img = achicar(i, 100, 100);
        assert_eq!((img.width, img.height), (40, 20));
    }
}

#[cfg(test)]
mod salidas {
    use super::*;

    #[test]
    fn la_escala_y_la_salida_grande() {
        let json = r#"[
            {"name":"DP-3","width":2560,"height":1440,"scale":1.0},
            {"name":"HDMI-A-1","width":3840,"height":2160,"scale":1.5},
            {"name":"X","width":7680,"height":4320,"scale":3.0,"disabled":true}
        ]"#;
        poner_salidas(&crate::compositor::hyprland::leer_salidas(json).unwrap());
        assert_eq!(escala(), 2);
        assert_eq!(salida_grande(), Some((3840, 2160)));
        poner_salidas(&[]);
        assert_eq!(escala(), 1);
    }
}
