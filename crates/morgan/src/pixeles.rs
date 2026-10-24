//! Íconos e imágenes del lado de GTK, pasados a píxeles para las superficies del motor propio:
//! los íconos del tema (los simbólicos, del color que se pida), los archivos y los píxeles que
//! mandan las apps. Las superficies sin GTK los reciben como `ImageData`.

use std::sync::Arc;

use gtk::prelude::*;
use gtk::{gdk, gdk_pixbuf, graphene, gsk};
use morgan_motor::vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

#[derive(Default)]
pub struct Pintor {
    pintor: Option<gsk::Renderer>,
}

impl Pintor {
    fn pintor(&mut self) -> Option<&gsk::Renderer> {
        if self.pintor.is_none() {
            let display = gdk::Display::default()?;
            let r = gsk::CairoRenderer::new();
            if let Err(e) = r.realize_for_display(&display) {
                eprintln!("morgan: íconos: {e}");
                return None;
            }
            self.pintor = Some(r.upcast());
        }
        self.pintor.as_ref()
    }

    /// `p` de `tam` × `tam` píxeles; si es simbólico, de `color`.
    pub fn paintable(
        &mut self,
        p: &gdk::Paintable,
        tam: i32,
        color: &gdk::RGBA,
    ) -> Option<ImageData> {
        let pintor = self.pintor()?;
        pixeles(p, tam, color, pintor)
    }

    /// Un ícono del tema por nombre, a `tam` lógicos y `escala`.
    pub fn icono(
        &mut self,
        nombre: &str,
        tam: i32,
        escala: i32,
        color: &gdk::RGBA,
    ) -> Option<ImageData> {
        let display = gdk::Display::default()?;
        let tema = gtk::IconTheme::for_display(&display);
        if !tema.has_icon(nombre) {
            return None;
        }
        let p = tema.lookup_icon(
            nombre,
            &[],
            tam,
            escala,
            gtk::TextDirection::Ltr,
            gtk::IconLookupFlags::empty(),
        );
        self.paintable(p.upcast_ref(), tam * escala, color)
    }
}

impl Drop for Pintor {
    /// GSK aborta si un renderer realizado se suelta así nomás.
    fn drop(&mut self) {
        if let Some(r) = self.pintor.take() {
            r.unrealize();
        }
    }
}

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

pub fn color(c: [f32; 4]) -> gdk::RGBA {
    gdk::RGBA::new(c[0], c[1], c[2], c[3])
}

/// El ícono en píxeles RGBA sin premultiplicar, de `tam` × `tam`. Los simbólicos, con `color`.
pub fn pixeles(
    p: &gdk::Paintable,
    tam: i32,
    color: &gdk::RGBA,
    pintor: &gsk::Renderer,
) -> Option<ImageData> {
    let snap = gtk::Snapshot::new();
    let lado = f64::from(tam);
    match p.dynamic_cast_ref::<gtk::SymbolicPaintable>() {
        Some(s) => s.snapshot_symbolic(&snap, lado, lado, std::slice::from_ref(color)),
        None => p.snapshot(&snap, lado, lado),
    }
    let nodo = snap.to_node()?;
    let caja = graphene::Rect::new(0.0, 0.0, tam as f32, tam as f32);
    let textura = pintor.render_texture(&nodo, Some(&caja));
    Some(de_textura(&textura))
}

pub fn de_textura(textura: &gdk::Texture) -> ImageData {
    let mut bajada = gdk::TextureDownloader::new(textura);
    bajada.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, paso) = bajada.download_bytes();
    let (w, h) = (textura.width() as usize, textura.height() as usize);
    let mut px = Vec::with_capacity(w * h * 4);
    for fila in bytes.chunks(paso).take(h) {
        px.extend_from_slice(fila.get(..w * 4).unwrap_or_default());
    }
    rgba(px, w as u32, h as u32)
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

/// Un pixbuf en RGBA.
pub fn de_pixbuf(pb: &gdk_pixbuf::Pixbuf) -> ImageData {
    let (w, h) = (pb.width().max(0) as usize, pb.height().max(0) as usize);
    let canales = pb.n_channels() as usize;
    let paso = pb.rowstride() as usize;
    let bytes = pb.read_pixel_bytes();
    let mut px = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let fila = &bytes[y * paso..];
        for x in 0..w {
            let p = &fila[x * canales..];
            px.extend_from_slice(&[p[0], p[1], p[2], if canales == 4 { p[3] } else { 255 }]);
        }
    }
    rgba(px, w as u32, h as u32)
}

/// Un archivo de imagen que entre en `lado` × `lado` píxeles, con su proporción.
pub fn archivo(ruta: &std::path::Path, lado: i32) -> Option<ImageData> {
    let pb = gdk_pixbuf::Pixbuf::from_file_at_scale(ruta, lado, lado, true).ok()?;
    Some(de_pixbuf(&pb))
}
