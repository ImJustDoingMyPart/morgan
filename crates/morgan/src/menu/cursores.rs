//! Solo se lee el índice y la imagen que hace falta, no el archivo entero.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// Con una sola flecha, varios temas se confunden.
const FORMAS: [&[&str]; 3] = [
    &["default", "left_ptr", "arrow"],
    &["pointer", "hand2", "hand1", "pointing_hand"],
    &["text", "xterm", "ibeam"],
];

const MAGIA: &[u8; 4] = b"Xcur";
const IMAGEN: u32 = 0xfffd_0002;
/// Más grande no es un cursor: un archivo roto no pide memoria de más.
const LADO_MAX: u32 = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Muestra {
    pub ancho: u32,
    pub alto: u32,
    pub bgra: Vec<u8>,
}

/// Cada tema deja otro margen en su lienzo: sin recortar, a la misma altura unos se ven chicos.
pub fn muestras(temas: &[String], lado: u32) -> Vec<(String, Vec<Muestra>)> {
    temas
        .iter()
        .map(|t| {
            let m = match carpeta(t) {
                Some(dir) => FORMAS
                    .iter()
                    .filter_map(|nombres| {
                        nombres.iter().find_map(|n| {
                            let mut f = File::open(dir.join(n)).ok()?;
                            leer(&mut f, lado).and_then(recortar)
                        })
                    })
                    .collect(),
                None => Vec::new(),
            };
            (t.clone(), m)
        })
        .collect()
}

fn carpeta(tema: &str) -> Option<PathBuf> {
    crate::apariencia::cursores::bases()
        .into_iter()
        .rev()
        .map(|base| base.join(tema).join("cursors"))
        .find(|d| d.is_dir())
}

fn u32_en(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// A igual distancia, el mayor: achicar se ve mejor que agrandar.
fn leer<R: Read + Seek>(r: &mut R, lado: u32) -> Option<Muestra> {
    let mut cab = [0u8; 16];
    r.read_exact(&mut cab).ok()?;
    if &cab[..4] != MAGIA {
        return None;
    }
    let (largo_cab, entradas) = (u32_en(&cab, 4), u32_en(&cab, 12));
    if largo_cab < 16 || entradas == 0 || entradas > 4096 {
        return None;
    }
    r.seek(SeekFrom::Start(u64::from(largo_cab))).ok()?;
    let mut toc = vec![0u8; entradas as usize * 12];
    r.read_exact(&mut toc).ok()?;
    let (_, posicion) = toc
        .as_chunks::<12>()
        .0
        .iter()
        .filter(|e| u32_en(&e[..], 0) == IMAGEN)
        .map(|e| (u32_en(e, 4), u32_en(e, 8)))
        // `min_by_key` se queda con la primera de las iguales: el primer cuadro del tamaño.
        .min_by_key(|&(nominal, _)| (nominal.abs_diff(lado), std::cmp::Reverse(nominal)))?;

    r.seek(SeekFrom::Start(u64::from(posicion))).ok()?;
    let mut img = [0u8; 36];
    r.read_exact(&mut img).ok()?;
    if u32_en(&img, 4) != IMAGEN {
        return None;
    }
    let (ancho, alto) = (u32_en(&img, 16), u32_en(&img, 20));
    if ancho == 0 || alto == 0 || ancho > LADO_MAX || alto > LADO_MAX {
        return None;
    }
    // Los píxeles empiezan después de la cabecera del trozo, que dice su propio largo.
    r.seek(SeekFrom::Start(
        u64::from(posicion) + u64::from(u32_en(&img, 0)),
    ))
    .ok()?;
    let mut bgra = vec![0u8; (ancho * alto * 4) as usize];
    r.read_exact(&mut bgra).ok()?;
    Some(Muestra { ancho, alto, bgra })
}

const OPACO: u8 = 128;

fn recortar(m: Muestra) -> Option<Muestra> {
    let (an, al) = (m.ancho as usize, m.alto as usize);
    // La sombra suave no cuenta: con ella, el dibujo sale más chico.
    let alfa = |x: usize, y: usize| m.bgra[(y * an + x) * 4 + 3] >= OPACO;
    let filas: Vec<usize> = (0..al).filter(|&y| (0..an).any(|x| alfa(x, y))).collect();
    let cols: Vec<usize> = (0..an).filter(|&x| (0..al).any(|y| alfa(x, y))).collect();
    let (&y0, &y1, &x0, &x1) = (filas.first()?, filas.last()?, cols.first()?, cols.last()?);
    let bgra = (y0..=y1)
        .flat_map(|y| &m.bgra[(y * an + x0) * 4..(y * an + x1 + 1) * 4])
        .copied()
        .collect();
    Some(Muestra {
        ancho: (x1 - x0 + 1) as u32,
        alto: (y1 - y0 + 1) as u32,
        bgra,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Un XCursor con una imagen cuadrada por cada `(nominal, lado, relleno)`, en ese orden.
    fn xcursor(imagenes: &[(u32, u32, u8)]) -> Vec<u8> {
        let mut v = Vec::new();
        let n = imagenes.len() as u32;
        for x in [u32::from_le_bytes(*MAGIA), 16, 0x1_0000, n] {
            v.extend(x.to_le_bytes());
        }
        let mut pos = 16 + 12 * n;
        let mut cuerpos = Vec::new();
        for &(nominal, lado, relleno) in imagenes {
            for x in [IMAGEN, nominal, pos] {
                v.extend(x.to_le_bytes());
            }
            let mut c = Vec::new();
            for x in [36, IMAGEN, nominal, 1, lado, lado, 0, 0, 0] {
                c.extend(u32::to_le_bytes(x));
            }
            c.extend(std::iter::repeat_n(relleno, (lado * lado * 4) as usize));
            pos += c.len() as u32;
            cuerpos.push(c);
        }
        cuerpos.into_iter().for_each(|c| v.extend(c));
        v
    }

    #[test]
    fn elige_el_tamano_mas_cercano() {
        let a = xcursor(&[(16, 16, 1), (24, 24, 2), (32, 32, 3), (48, 48, 4)]);
        let m = leer(&mut Cursor::new(&a), 24).unwrap();
        assert_eq!((m.ancho, m.alto, m.bgra[0]), (24, 24, 2));
        assert_eq!(m.bgra.len(), 24 * 24 * 4);
        // 40 queda entre 32 y 48: gana el mayor.
        assert_eq!(leer(&mut Cursor::new(&a), 40).unwrap().bgra[0], 4);
        assert_eq!(leer(&mut Cursor::new(&a), 200).unwrap().bgra[0], 4);
    }

    #[test]
    fn de_un_animado_toma_el_primer_cuadro() {
        let a = xcursor(&[(24, 24, 7), (24, 24, 8), (24, 24, 9)]);
        assert_eq!(leer(&mut Cursor::new(&a), 24).unwrap().bgra[0], 7);
    }

    #[test]
    fn recorta_el_margen_transparente() {
        // 4×3 con dos píxeles dibujados: (1,1) y (2,1).
        let mut bgra = vec![0u8; 4 * 3 * 4];
        bgra[(4 + 1) * 4..(4 + 3) * 4].copy_from_slice(&[1, 2, 3, 255, 5, 6, 7, 128]);
        bgra[3] = 40; // sombra en (0,0): no cuenta
        let m = recortar(Muestra {
            ancho: 4,
            alto: 3,
            bgra,
        })
        .unwrap();
        assert_eq!((m.ancho, m.alto), (2, 1));
        assert_eq!(m.bgra, vec![1, 2, 3, 255, 5, 6, 7, 128]);
        let vacia = Muestra {
            ancho: 2,
            alto: 2,
            bgra: vec![0; 16],
        };
        assert_eq!(recortar(vacia), None);
    }

    #[test]
    fn lo_roto_no_es_muestra() {
        assert_eq!(leer(&mut Cursor::new(b"no es un cursor"), 24), None);
        let mut corto = xcursor(&[(24, 24, 1)]);
        corto.truncate(corto.len() - 10);
        assert_eq!(leer(&mut Cursor::new(&corto), 24), None);
        let enorme = xcursor(&[(24, 300, 1)]);
        assert_eq!(leer(&mut Cursor::new(&enorme), 24), None);
    }
}
