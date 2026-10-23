//! El fondo es estático: se desenfoca una vez, en CPU, al arrancar. vello no tiene blur de
//! imagen, y no hace falta: el mismo resultado sale de achicar la imagen, desenfocarla chica
//! y dejar que la GPU la agrande con filtro bilineal (desenfocada no se nota).
//!
//! El aspecto es el de `morgan-lock`: blur de radio 34 (en GSK, sigma = radio / 2), con el
//! borde extendido en vez de mezclar con transparente, y `brightness(0.77) contrast(1.1)`.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Cuánto se achica por lado antes de desenfocar.
const ACHICAR: usize = 4;
/// Sigma del blur de `morgan-lock` (radio 34 de GSK), en píxeles de la imagen original.
const SIGMA: f32 = 17.0;
const BRILLO: f32 = 0.77;
const CONTRASTE: f32 = 1.1;

pub struct Imagen {
    pub ancho: u32,
    pub alto: u32,
    /// RGBA8, opaco.
    pub pixeles: Vec<u8>,
    /// Cuánto hay que agrandarla para volver al tamaño original.
    pub achicada: f32,
}

pub fn cargar(ruta: &Path) -> Result<Imagen, String> {
    let archivo = File::open(ruta).map_err(|e| e.to_string())?;
    let mut d = png::Decoder::new(BufReader::new(archivo));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut lector = d.read_info().map_err(|e| e.to_string())?;
    let tam = lector
        .output_buffer_size()
        .ok_or("imagen demasiado grande")?;
    let mut buf = vec![0u8; tam];
    let info = lector.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let canales = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return Err("PNG indexado sin expandir".into()),
    };
    let (w, h) = (info.width as usize, info.height as usize);
    let rgb = |i: usize| -> [f32; 3] {
        let p = &buf[i * canales..];
        match canales {
            1 | 2 => [p[0] as f32; 3],
            _ => [p[0] as f32, p[1] as f32, p[2] as f32],
        }
    };
    let img = achicar(w, h, info.line_size / canales, rgb);
    drop(buf);
    Ok(procesar(img))
}

/// Promedio de cada bloque de `ACHICAR`×`ACHICAR`.
fn achicar(w: usize, h: usize, paso: usize, rgb: impl Fn(usize) -> [f32; 3]) -> Plano {
    let (cw, ch) = (w.div_ceil(ACHICAR), h.div_ceil(ACHICAR));
    let mut px = vec![[0.0f32; 3]; cw * ch];
    let mut cuenta = vec![0u32; cw * ch];
    for y in 0..h {
        for x in 0..w {
            let i = (y / ACHICAR) * cw + x / ACHICAR;
            let c = rgb(y * paso + x);
            for k in 0..3 {
                px[i][k] += c[k];
            }
            cuenta[i] += 1;
        }
    }
    for (p, n) in px.iter_mut().zip(cuenta) {
        for v in p.iter_mut() {
            *v /= n.max(1) as f32;
        }
    }
    Plano { w: cw, h: ch, px }
}

struct Plano {
    w: usize,
    h: usize,
    px: Vec<[f32; 3]>,
}

fn procesar(mut p: Plano) -> Imagen {
    // Tres cajas seguidas aproximan la gaussiana: cada una suma (ancho² - 1) / 12 a la varianza.
    let sigma = SIGMA / ACHICAR as f32;
    let radio = (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0).round().max(1.0) as usize;
    for _ in 0..3 {
        caja(&mut p, radio, true);
        caja(&mut p, radio, false);
    }
    let filtro = |v: f32| ((v / 255.0 * BRILLO - 0.5) * CONTRASTE + 0.5).clamp(0.0, 1.0);
    let mut pixeles = Vec::with_capacity(p.w * p.h * 4);
    for c in &p.px {
        pixeles.extend(c.map(|v| (filtro(v) * 255.0).round() as u8));
        pixeles.push(255);
    }
    Imagen {
        ancho: p.w as u32,
        alto: p.h as u32,
        pixeles,
        achicada: ACHICAR as f32,
    }
}

/// Blur de caja en una dirección. Más allá del borde se repite el último píxel: el borde no
/// se oscurece (el problema que `morgan-lock/src/fondo.rs` resolvía con una copia agrandada).
fn caja(p: &mut Plano, r: usize, horizontal: bool) {
    let (largo, lineas) = if horizontal { (p.w, p.h) } else { (p.h, p.w) };
    let idx = |linea: usize, i: usize| {
        if horizontal {
            linea * p.w + i
        } else {
            i * p.w + linea
        }
    };
    let n = (2 * r + 1) as f32;
    let mut fila = vec![[0.0f32; 3]; largo];
    for linea in 0..lineas {
        for (i, f) in fila.iter_mut().enumerate() {
            *f = p.px[idx(linea, i)];
        }
        let en = |i: isize| fila[i.clamp(0, largo as isize - 1) as usize];
        let mut suma = [0.0f32; 3];
        for j in -(r as isize)..=(r as isize) {
            let c = en(j);
            for k in 0..3 {
                suma[k] += c[k];
            }
        }
        for i in 0..largo {
            p.px[idx(linea, i)] = suma.map(|s| s / n);
            let (sale, entra) = (en(i as isize - r as isize), en(i as isize + r as isize + 1));
            for k in 0..3 {
                suma[k] += entra[k] - sale[k];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_color_liso_sigue_liso_y_con_el_filtro() {
        let p = Plano {
            w: 30,
            h: 20,
            px: vec![[200.0, 100.0, 0.0]; 600],
        };
        let img = procesar(p);
        let esperado = |v: f32| (((v / 255.0 * BRILLO - 0.5) * CONTRASTE + 0.5) * 255.0).round() as u8;
        for c in img.pixeles.chunks(4) {
            assert_eq!(c, [esperado(200.0), esperado(100.0), 0, 255]);
        }
    }

    #[test]
    fn el_blur_conserva_el_promedio_y_no_oscurece_el_borde() {
        let mut p = Plano {
            w: 40,
            h: 1,
            px: (0..40).map(|i| [if i < 20 { 0.0 } else { 255.0 }; 3]).collect(),
        };
        caja(&mut p, 3, true);
        assert_eq!(p.px[0], [0.0; 3]);
        assert_eq!(p.px[39], [255.0; 3]);
        assert!(p.px[19][0] > 0.0 && p.px[20][0] < 255.0);
    }

    #[test]
    fn achicar_promedia_bloques() {
        let p = achicar(8, 4, 8, |i| [if i % 8 < 4 { 0.0 } else { 100.0 }; 3]);
        assert_eq!((p.w, p.h), (2, 1));
        assert_eq!(p.px, vec![[0.0; 3], [100.0; 3]]);
    }
}
