//! La superficie cubre la salida para cerrar con un click afuera, así que el `slide top` de
//! Hyprland arrastra la capa entera: un panel pegado arriba asoma recién en el último tercio y
//! parece que tarda. Por eso la capa entra con `fade` y lo que se ve baja acá, desde su propio
//! borde de abajo.

use std::time::Duration;

/// Lo que tarda el fundido de las capas a velocidad media.
const BASE: Duration = Duration::from_millis(150);

pub fn duracion() -> Duration {
    crate::animaciones::dur(BASE)
}

pub fn desplazamiento(micros: i64, fondo: f32) -> f32 {
    let t = (micros as f64 / duracion().as_micros() as f64).clamp(0.0, 1.0);
    let falta = (1.0 - t).powi(3);
    (falta * f64::from(fondo)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arranca_con_todo_el_alto_afuera_y_termina_en_su_lugar() {
        assert_eq!(desplazamiento(0, 600.0), 600.0);
        assert_eq!(desplazamiento(duracion().as_micros() as i64, 600.0), 0.0);
        assert_eq!(
            desplazamiento(10 * duracion().as_micros() as i64, 600.0),
            0.0
        );
    }

    #[test]
    fn baja_mas_rapido_al_principio() {
        let mitad = desplazamiento(duracion().as_micros() as i64 / 2, 600.0);
        assert!(
            mitad < 300.0 * 0.5,
            "a la mitad del tiempo ya bajó más de la mitad"
        );
    }
}
