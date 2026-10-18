//! El formato lo escribe `dev/acerca-cuadros.py`, que lo documenta.

pub struct Clip {
    pub lado: usize,
    pub fps: f32,
    pub paleta: Vec<[u8; 3]>,
    pub cuadros: Vec<Vec<u8>>,
}

impl Clip {
    #[cfg(test)]
    pub fn duracion(&self) -> f32 {
        self.cuadros.len() as f32 / self.fps
    }

    pub fn claro(&self) -> [u8; 3] {
        let luz = |c: &[u8; 3]| 299 * c[0] as u32 + 587 * c[1] as u32 + 114 * c[2] as u32;
        self.paleta.iter().copied().max_by_key(luz).unwrap_or([255; 3])
    }
}

pub fn leer(datos: &[u8]) -> Result<Clip, String> {
    let resto = datos.strip_prefix(b"MRGN1").ok_or("no es un clip de morgan")?;
    let [ancho, alto, c0, c1, fps, colores, resto @ ..] = resto else {
        return Err("cabecera corta".into());
    };
    if ancho != alto {
        return Err("el clip no es cuadrado".into());
    }
    let lado = *ancho as usize;
    let total = u16::from_le_bytes([*c0, *c1]) as usize;
    // 0 colores es 256: el script escribe el número módulo 256.
    let colores = if *colores == 0 { 256 } else { *colores as usize };
    if resto.len() < colores * 3 {
        return Err("paleta corta".into());
    }
    let (paleta, mut resto) = resto.split_at(colores * 3);
    let paleta: Vec<[u8; 3]> = paleta.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();

    let area = lado * lado;
    let mut cuadros = Vec::with_capacity(total);
    let mut anterior = vec![0u8; area];
    for _ in 0..total {
        let mut cuadro = anterior.clone();
        let mut p = 0;
        while p < area {
            let [op, tras @ ..] = resto else {
                return Err("cuadro cortado".into());
            };
            resto = tras;
            if *op < 0x80 {
                p += *op as usize + 1;
            } else {
                let n = (*op - 0x7F) as usize;
                if resto.len() < n || p + n > area {
                    return Err("cuadro cortado".into());
                }
                cuadro[p..p + n].copy_from_slice(&resto[..n]);
                resto = &resto[n..];
                p += n;
            }
        }
        if p != area {
            return Err("un cuadro se pasa del área".into());
        }
        if cuadro.iter().any(|&i| i as usize >= paleta.len()) {
            return Err("índice fuera de la paleta".into());
        }
        cuadros.push(cuadro);
        anterior = cuadros.last().unwrap().clone();
    }
    if !resto.is_empty() {
        return Err("sobran bytes después del último cuadro".into());
    }
    Ok(Clip { lado, fps: *fps as f32, paleta, cuadros })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2×2, dos colores, dos cuadros: el segundo cambia solo el último píxel.
    const CHICO: &[u8] = &[
        b'M', b'R', b'G', b'N', b'1', 2, 2, 2, 0, 24, 2, //
        0, 0, 0, 255, 255, 255, //
        0x80 + 3, 1, 0, 1, 1, // cuatro literales
        0x02, 0x80, 0, // tres iguales, un literal
    ];

    #[test]
    fn lee_diferencias() {
        let c = leer(CHICO).unwrap();
        assert_eq!(c.lado, 2);
        assert_eq!(c.cuadros, vec![vec![1, 0, 1, 1], vec![1, 0, 1, 0]]);
        assert_eq!(c.claro(), [255, 255, 255]);
    }

    #[test]
    fn rechaza_cortado_y_sobrante() {
        assert!(leer(&CHICO[..CHICO.len() - 1]).is_err());
        let mut largo = CHICO.to_vec();
        largo.push(0);
        assert!(leer(&largo).is_err());
    }

    #[test]
    fn el_clip_del_binario_se_lee_entero() {
        let c = leer(super::super::CUADROS).unwrap();
        assert_eq!(c.lado, 108);
        assert!(c.duracion() >= 10.0, "el guion cuenta con 10 s de video");
    }
}
