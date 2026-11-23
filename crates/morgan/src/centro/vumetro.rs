//! El nivel de lo que entra por el micrófono mientras se escucha (`escucha.rs`): el lleno del
//! slider se enciende hasta donde llega la voz. Lo mide `pw-record` en crudo (16 kHz, mono),
//! que corre solo con el centro abierto y la escucha encendida (lo lanza `hilo.rs`, que lo
//! dibuja en `dibujo/armar.rs`), 30 cuadros por segundo como el ecualizador.

pub const TASA: u32 = 16_000;
pub const POR_CUADRO: usize = (TASA / 30) as usize;
/// Lo que se ve de un cuadro al siguiente si la voz baja: sube de golpe y cae en ~0,7 s.
const CAIDA: f64 = 0.05;
/// Por debajo, nada (el ruido de fondo de un micrófono anda por −60 dBFS).
const PISO_DB: f64 = -60.0;

pub fn argumentos(nombre_app: &str, fuente: Option<&str>) -> Vec<String> {
    let mut v: Vec<String> = [
        "--raw",
        "--rate",
        "16000",
        "--channels",
        "1",
        "--format",
        "s16",
        "--latency",
        "30ms",
        "-P",
    ]
    .map(str::to_owned)
    .into();
    v.push(format!(
        "{} node.name=morgan-vumetro", // no-tr
        crate::escucha::propiedades(nombre_app, fuente.is_some())
    ));
    if let Some(f) = fuente {
        v.extend(["--target".to_owned(), f.to_owned()]);
    }
    v.push("-".to_owned());
    v
}

/// De 0 a 1, en decibeles: así se ve la voz, que en lineal quedaría en el primer tercio.
pub fn fraccion(muestras: &[i16]) -> f64 {
    let pico = muestras
        .iter()
        .map(|m| m.unsigned_abs())
        .max()
        .unwrap_or_default();
    if pico == 0 {
        return 0.0;
    }
    let db = 20.0 * (f64::from(pico) / 32768.0).log10();
    ((db - PISO_DB) / -PISO_DB).clamp(0.0, 1.0)
}

pub fn siguiente(antes: f64, medido: f64) -> f64 {
    medido.max(antes - CAIDA).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn de_la_muestra_a_la_barra() {
        assert_eq!(fraccion(&[]), 0.0);
        assert_eq!(fraccion(&[0; POR_CUADRO]), 0.0);
        assert_eq!(fraccion(&[i16::MIN, 3]), 1.0);
        // −6 dBFS (la mitad) va al 90 %; −40, a un tercio; bajo el piso, nada.
        assert!((fraccion(&[16384]) - 0.9).abs() < 0.01);
        assert!((fraccion(&[-328]) - 1.0 / 3.0).abs() < 0.01);
        assert_eq!(fraccion(&[16]), 0.0);
    }

    #[test]
    fn sube_de_golpe_y_cae_de_a_poco() {
        assert_eq!(siguiente(0.2, 0.8), 0.8);
        assert!((siguiente(0.8, 0.0) - 0.75).abs() < 1e-9);
        assert_eq!(siguiente(0.01, 0.0), 0.0);
    }

    #[test]
    fn graba_en_crudo_lo_que_se_lee() {
        let a = argumentos("Escucharme", None);
        assert!(a.contains(&"--raw".to_owned()));
        assert!(a.windows(2).any(|w| w == ["--rate", "16000"]));
        assert!(a.windows(2).any(|w| w == ["--format", "s16"]));
        assert_eq!(a.last().map(String::as_str), Some("-"));
        assert!(!a.contains(&"--target".to_owned()));
        let a = argumentos("Escucharme", Some("mic"));
        assert!(a.windows(2).any(|w| w == ["--target", "mic"]));
    }
}
