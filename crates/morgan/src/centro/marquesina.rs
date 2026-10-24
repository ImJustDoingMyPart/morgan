//! Un texto que no entra va y viene: quieto 2 s, avanza a 30 px/s hasta el final, 2 s y vuelve.
//! Lo usan el título del reproductor del centro y los del calendario.

const PAUSA: f64 = 2.0;
const VELOCIDAD: f64 = 30.0;

pub fn posicion(t: f64, recorrido: f64) -> f64 {
    if recorrido <= 0.0 {
        return 0.0;
    }
    let ida = recorrido / VELOCIDAD;
    let ciclo = 2.0 * (PAUSA + ida);
    let t = t.rem_euclid(ciclo);
    if t < PAUSA {
        0.0
    } else if t < PAUSA + ida {
        (t - PAUSA) * VELOCIDAD
    } else if t < 2.0 * PAUSA + ida {
        recorrido
    } else {
        recorrido - (t - 2.0 * PAUSA - ida) * VELOCIDAD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quieta_si_entra() {
        assert_eq!(posicion(5.0, 0.0), 0.0);
        assert_eq!(posicion(5.0, -10.0), 0.0);
    }

    #[test]
    fn pausa_ida_pausa_vuelta() {
        // 60 px a 30 px/s: 2 s de ida. Ciclo: 2 + 2 + 2 + 2 = 8 s.
        assert_eq!(posicion(1.0, 60.0), 0.0);
        assert_eq!(posicion(3.0, 60.0), 30.0);
        assert_eq!(posicion(5.0, 60.0), 60.0);
        assert_eq!(posicion(7.0, 60.0), 30.0);
        assert_eq!(posicion(8.5, 60.0), 0.0);
    }
}
