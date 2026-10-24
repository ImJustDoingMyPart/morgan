//! Las cuentas de una fila que se arrastra de costado (las usan el centro y los toasts). Al
//! pasar el umbral la acción se hace sin esperar a soltar: llevar la tarjeta hasta el borde
//! cuesta. Un tirón corto y rápido antes del umbral también cuenta.
//!
//! El arrastre arranca recién cuando el movimiento es más horizontal que vertical y pasa
//! unos píxeles: así el scroll del panel y el click siguen andando.

use std::time::Duration;

pub const ARRANQUE: f64 = 20.0;
pub const UMBRAL: f64 = 0.5;
const TIRON_MIN: f64 = 100.0;
const TIRON_VELOCIDAD: f64 = 2.0;
const ANIMACION: Duration = Duration::from_millis(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lado {
    Derecha,
    Izquierda,
}

fn lado(dx: f64) -> Lado {
    if dx > 0.0 {
        Lado::Derecha
    } else {
        Lado::Izquierda
    }
}

pub fn tiron(dx: f64, velocidad: f64) -> Option<Lado> {
    let mismo_lado = dx * velocidad > 0.0;
    (mismo_lado && dx.abs() >= TIRON_MIN && velocidad.abs() >= TIRON_VELOCIDAD).then(|| lado(dx))
}

pub fn decidir(dx: f64, ancho: f64) -> Option<Lado> {
    if ancho <= 0.0 || dx.abs() < ancho * UMBRAL {
        return None;
    }
    Some(if dx > 0.0 {
        Lado::Derecha
    } else {
        Lado::Izquierda
    })
}

pub fn asomo(dx: f64, ancho: f64) -> f64 {
    if ancho <= 0.0 {
        return 0.0;
    }
    (dx.abs() / (ancho * UMBRAL)).min(1.0)
}

pub fn opacidad(dx: f64, ancho: f64) -> f64 {
    if ancho <= 0.0 {
        return 1.0;
    }
    let u = ancho * UMBRAL;
    let pasado = (dx.abs() - u).max(0.0) / (ancho - u).max(1.0);
    (1.0 - pasado).clamp(0.0, 1.0)
}

pub fn avance(micros: i64) -> f64 {
    (micros as f64 / crate::animaciones::dur(ANIMACION).as_micros() as f64).clamp(0.0, 1.0)
}

pub fn suavizar(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_pasado_el_umbral() {
        assert_eq!(decidir(0.0, 300.0), None);
        assert_eq!(decidir(140.0, 300.0), None);
        assert_eq!(decidir(150.0, 300.0), Some(Lado::Derecha));
        assert_eq!(decidir(-150.0, 300.0), Some(Lado::Izquierda));
        assert_eq!(decidir(50.0, 0.0), None);
    }

    #[test]
    fn asoma_y_se_apaga() {
        assert_eq!(asomo(0.0, 300.0), 0.0);
        assert!((asomo(75.0, 300.0) - 0.5).abs() < 1e-9);
        assert_eq!(asomo(-200.0, 300.0), 1.0);
        assert_eq!(opacidad(140.0, 300.0), 1.0);
        assert_eq!(opacidad(300.0, 300.0), 0.0);
        assert!(opacidad(-200.0, 300.0) < 1.0);
    }

    #[test]
    fn un_tiron_corto_y_rapido_cuenta() {
        assert_eq!(tiron(110.0, 2.5), Some(Lado::Derecha));
        assert_eq!(tiron(-110.0, -2.1), Some(Lado::Izquierda));
        // Lento, corto o volviendo: no.
        assert_eq!(tiron(110.0, 1.5), None);
        assert_eq!(tiron(80.0, 4.0), None);
        assert_eq!(tiron(110.0, -2.5), None);
    }

    #[test]
    fn la_animacion_avanza_y_termina() {
        assert_eq!(avance(0), 0.0);
        assert!((avance(90_000) - 0.5).abs() < 1e-9);
        assert_eq!(avance(180_000), 1.0);
        assert_eq!(avance(500_000), 1.0);
        assert_eq!(suavizar(0.0), 0.0);
        assert_eq!(suavizar(1.0), 1.0);
        assert!(suavizar(0.5) > 0.5);
    }
}
