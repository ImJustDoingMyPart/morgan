
/// (segundo, brillo): entre dos puntos el brillo va con ease-out cúbica.
const BRILLO: [(f32, f32); 8] = [
    (0.0, 0.0),
    (0.6, 1.0),
    (3.0, 1.0),
    (3.8, 0.0), // ficha técnica
    (7.2, 0.0),
    (8.0, 1.0),
    (8.8, 1.0),
    (9.5, 0.0), // in memory of Morgan
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Texto {
    Ficha,
    Memoria,
}

const TEXTOS: [(Texto, f32, f32); 2] = [(Texto::Ficha, 3.8, 7.2), (Texto::Memoria, 9.5, 12.6)];
const FUNDIDO: f32 = 0.4;

pub const FIN: f32 = 13.0;

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

pub fn brillo(t: f32) -> f32 {
    let Some(i) = BRILLO.windows(2).position(|w| t < w[1].0) else {
        return BRILLO[BRILLO.len() - 1].1;
    };
    let ((t0, b0), (t1, b1)) = (BRILLO[i], BRILLO[i + 1]);
    if t < t0 {
        return b0;
    }
    b0 + (b1 - b0) * ease_out((t - t0) / (t1 - t0))
}

pub fn texto(t: f32) -> Option<(Texto, f32)> {
    TEXTOS.iter().find(|(_, entra, sale)| (*entra..*sale).contains(&t)).map(|&(texto, entra, sale)| {
        let alfa = ((t - entra) / FUNDIDO).min((sale - t) / FUNDIDO).min(1.0);
        (texto, ease_out(alfa))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasos() -> impl Iterator<Item = f32> {
        (0..=(FIN * 100.0) as u32).map(|i| i as f32 / 100.0)
    }

    #[test]
    fn el_texto_va_solo_sobre_oscuro() {
        for t in pasos() {
            if texto(t).is_some() {
                assert_eq!(brillo(t), 0.0, "texto con video a la vista en {t} s");
            }
        }
    }

    #[test]
    fn el_video_se_ve_dos_veces() {
        assert_eq!(brillo(2.0), 1.0);
        assert_eq!(brillo(5.0), 0.0);
        assert_eq!(brillo(8.5), 1.0);
        assert_eq!(brillo(11.0), 0.0);
        assert_eq!(brillo(FIN + 1.0), 0.0);
    }

    #[test]
    fn los_textos_en_orden_y_enteros_un_rato() {
        assert_eq!(texto(5.5), Some((Texto::Ficha, 1.0)));
        assert_eq!(texto(11.0), Some((Texto::Memoria, 1.0)));
        assert_eq!(texto(8.5), None);
        assert!(texto(FIN).is_none());
    }

    #[test]
    fn la_memoria_sigue_despues_del_sonido() {
        // El sonido dura lo que el video (10 s): el texto se queda un rato en silencio.
        assert_eq!(texto(12.0), Some((Texto::Memoria, 1.0)));
    }

    #[test]
    fn el_brillo_no_salta() {
        let mut antes = brillo(0.0);
        for t in pasos() {
            let ahora = brillo(t);
            assert!((ahora - antes).abs() < 0.1, "salto en {t} s");
            antes = ahora;
        }
    }
}
