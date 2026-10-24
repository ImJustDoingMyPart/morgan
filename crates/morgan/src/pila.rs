//! Un grupo de notificaciones cerrado, apilado: las de atrás asoman 8 px por abajo, cada una
//! un poco más chica (`centro/dibujo/avisos.rs`).

pub const APILADAS: usize = 3;
pub const DESPLAZAMIENTO: i32 = 8;
const ESCALA: f32 = 0.95;

pub fn ubicacion(i: usize, ancho: i32, alto: i32) -> (f32, f32, f32) {
    let escala = ESCALA.powi(i as i32);
    let x = (ancho as f32 - ancho as f32 * escala) * 0.5;
    let y = alto as f32 * (1.0 - escala) + (DESPLAZAMIENTO * i as i32) as f32;
    (x, y, escala)
}

/// Una más alta (una captura grande detrás de una chica) no se puede asignar más corta: su
/// contenido no achica y se derramaría debajo de la pila. De atrás solo se ve el fondo, así que
/// aplastarla no se nota.
pub fn aplastado(alto: i32, alto_propio: i32) -> f32 {
    if alto_propio <= alto {
        1.0
    } else {
        alto as f32 / alto_propio as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ancladas_abajo_y_centradas() {
        assert_eq!(ubicacion(0, 300, 60), (0.0, 0.0, 1.0));
        let (x, y, e) = ubicacion(1, 300, 60);
        assert!((e - 0.95).abs() < 1e-6);
        assert!((x - 7.5).abs() < 1e-4);
        // El borde de abajo de la de atrás queda 8 px más abajo que el de la de adelante.
        assert!((y + 60.0 * e - 68.0).abs() < 1e-3);
        let (_, y, e) = ubicacion(2, 300, 60);
        assert!((y + 60.0 * e - 76.0).abs() < 1e-3);
    }

    #[test]
    fn una_de_atras_mas_alta_no_se_derrama() {
        assert_eq!(aplastado(60, 60), 1.0);
        assert_eq!(aplastado(60, 40), 1.0);
        // Una captura de 180 detrás de una de 120: se ve de 120, no se sale por abajo.
        let (_, y, e) = ubicacion(1, 300, 120);
        let visto = 180.0 * e * aplastado(120, 180);
        assert!((y + visto - 128.0).abs() < 1e-3);
    }
}
