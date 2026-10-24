//! Imágenes en píxeles para las superficies del motor propio: los archivos (una foto, una
//! carátula, una captura) y los píxeles que mandan las apps. Los íconos del tema están en
//! `icono.rs`. Todo sale premultiplicado o no según lo diga su `ImageData`.

use std::sync::Arc;

use gtk::gdk;
use gtk::prelude::*;
use morgan_motor::vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

/// La escala más alta de las salidas: las imágenes se arman para ella.
pub fn escala() -> i32 {
    let Some(display) = gdk::Display::default() else {
        return 1;
    };
    let monitores = display.monitors();
    (0..monitores.n_items())
        .filter_map(|i| monitores.item(i)?.downcast::<gdk::Monitor>().ok())
        .map(|m| m.scale_factor())
        .max()
        .unwrap_or(1)
        .max(1)
}

/// RGBA sin premultiplicar.
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
