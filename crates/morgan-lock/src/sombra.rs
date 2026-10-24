//! La sombra del texto (`text-shadow: 0 0 8px` del reloj en la versión con GTK). vello no
//! desenfoca glifos: el texto se dibuja aparte en blanco, se lee su alfa, se desenfoca en
//! CPU y vuelve como una imagen negra que va debajo. Se rehace solo cuando cambia el texto
//! (una vez por minuto, o con un aviso), así que leer de la GPU no importa.

use parley::Layout;
use vello::kurbo::Affine;
use vello::peniko::{Blob, Color, ImageAlphaType, ImageData, ImageFormat};
use vello::wgpu;
use vello::{AaConfig, RenderParams, Renderer, Scene};

use crate::texto;

pub struct Sombra {
    pub imagen: ImageData,
    /// Lo que sobra por lado alrededor del texto, en píxeles del buffer.
    pub margen: f64,
}

/// `sigma` en píxeles del buffer (en GSK, sigma = radio del blur / 2); `alfa`, la del negro.
pub fn hacer(
    device: &wgpu::Device,
    cola: &wgpu::Queue,
    vello: &mut Renderer,
    l: &Layout<u8>,
    sigma: f32,
    alfa: f32,
) -> Option<Sombra> {
    let m = (3.0 * sigma).ceil() as u32;
    let (w, h) = (
        l.width().ceil() as u32 + 2 * m,
        l.height().ceil() as u32 + 2 * m,
    );
    if l.width() <= 0.0 || w > 8192 || h > 8192 {
        return None;
    }
    let mut escena = Scene::new();
    let blanco = [1.0, 1.0, 1.0, 1.0];
    texto::dibujar_en(
        &mut escena,
        l,
        Affine::translate((m as f64, m as f64)),
        |_| blanco,
        1.0,
    );

    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sombra"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let vista = tex.create_view(&Default::default());
    let params = RenderParams {
        base_color: Color::TRANSPARENT,
        width: w,
        height: h,
        antialiasing_method: AaConfig::Area,
    };
    if let Err(e) = vello.render_to_texture(device, cola, &escena, &vista, &params) {
        eprintln!("morgan-lock: sombra: {e}");
        return None;
    }
    let rgba = leer(device, cola, &tex, w, h)?;
    let mut cobertura: Vec<f32> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[3] as f32)
        .collect();
    desenfocar(&mut cobertura, w as usize, h as usize, sigma);
    let mut pixeles = Vec::with_capacity(cobertura.len() * 4);
    for a in cobertura {
        pixeles.extend([0, 0, 0, (a * alfa).round().clamp(0.0, 255.0) as u8]);
    }
    Some(Sombra {
        imagen: ImageData {
            data: Blob::new(std::sync::Arc::new(pixeles)),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: w,
            height: h,
        },
        margen: m as f64,
    })
}

/// Los píxeles de `tex` (de `w` × `h`), de la GPU a la memoria.
pub fn leer(
    device: &wgpu::Device,
    cola: &wgpu::Queue,
    tex: &wgpu::Texture,
    w: u32,
    h: u32,
) -> Option<Vec<u8>> {
    morgan_motor::gpu::leer(device, cola, tex, w, h)
}

/// Tres cajas por eje aproximan la gaussiana; afuera es transparente (por eso el margen).
pub(crate) fn desenfocar(p: &mut [f32], w: usize, h: usize, sigma: f32) {
    let r = (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0)
        .round()
        .max(1.0) as isize;
    let mut linea = Vec::new();
    for _ in 0..3 {
        for y in 0..h {
            caja(p, &mut linea, r, (0..w).map(|x| y * w + x));
        }
        for x in 0..w {
            caja(p, &mut linea, r, (0..h).map(|y| y * w + x));
        }
    }
}

fn caja(p: &mut [f32], linea: &mut Vec<f32>, r: isize, idx: impl Iterator<Item = usize> + Clone) {
    linea.clear();
    linea.extend(idx.clone().map(|i| p[i]));
    let n = linea.len() as isize;
    let en = |i: isize| {
        if (0..n).contains(&i) {
            linea[i as usize]
        } else {
            0.0
        }
    };
    let mut suma: f32 = (-r..=r).map(en).sum();
    for (k, i) in idx.enumerate() {
        p[i] = suma / (2 * r + 1) as f32;
        let k = k as isize;
        suma += en(k + r + 1) - en(k - r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_blur_conserva_lo_que_hay_y_no_se_sale() {
        let (w, h) = (40, 40);
        let mut p = vec![0.0; w * h];
        p[20 * w + 20] = 1000.0;
        desenfocar(&mut p, w, h, 2.0);
        let total: f32 = p.iter().sum();
        assert!((total - 1000.0).abs() < 1.0, "{total}");
        assert!(p[20 * w + 20] < 1000.0 && p[20 * w + 22] > 0.0);
        assert_eq!(p[0], 0.0);
    }
}
