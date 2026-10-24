//! El motor es cava (no se reimplementa un analizador). Corre solo con el centro abierto y el
//! reproductor sonando (lo lanza `hilo.rs`, que lo dibuja en `dibujo/armar.rs`). cava escucha
//! el monitor de la salida, no el micrófono: el indicador de privacidad no lo cuenta.

pub const BARRAS: usize = 48;
const RANGO: f64 = 100.0;

/// Solo claves del `example_files/config` de cava 1.0.0.
pub fn config() -> String {
    format!(
        "[general]\n\
         framerate = 30\n\
         bars = {BARRAS}\n\
         autosens = 1\n\
         lower_cutoff_freq = 50\n\
         higher_cutoff_freq = 10000\n\
         \n\
         [input]\n\
         method = pipewire\n\
         source = auto\n\
         \n\
         [output]\n\
         method = raw\n\
         raw_target = /dev/stdout\n\
         data_format = ascii\n\
         ascii_max_range = {RANGO}\n\
         bar_delimiter = 59\n\
         frame_delimiter = 10\n\
         channels = stereo\n\
         \n\
         [smoothing]\n\
         noise_reduction = 77\n"
    )
}

pub fn interpretar(linea: &str) -> Option<Vec<f64>> {
    let niveles = linea
        .trim()
        .split(';')
        .filter(|v| !v.is_empty())
        .map(|v| {
            v.trim()
                .parse::<f64>()
                .ok()
                .map(|v| (v / RANGO).clamp(0.0, 1.0))
        })
        .collect::<Option<Vec<f64>>>()?;
    (niveles.len() == BARRAS).then_some(niveles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_renglon_de_cava() {
        let renglon = |v: &[u32]| v.iter().map(|x| format!("{x};")).collect::<String>();
        let mut v = vec![0u32; BARRAS];
        v[0] = 50;
        v[1] = 250;
        let n = interpretar(&(renglon(&v) + "\n")).unwrap();
        assert_eq!(n.len(), BARRAS);
        assert_eq!(n[0], 0.5);
        // Lo que se pasa del rango se recorta.
        assert_eq!(n[1], 1.0);
        // Sin el ; final también.
        assert!(interpretar(renglon(&v).trim_end_matches(';')).is_some());
        // Cantidad equivocada o basura: nada.
        assert!(interpretar("1;2;3;").is_none());
        assert!(interpretar(&renglon(&[1; BARRAS + 1])).is_none());
        assert!(interpretar(&"a;".repeat(BARRAS)).is_none());
        assert!(interpretar("").is_none());
    }

    #[test]
    fn la_config_pide_lo_que_se_lee() {
        let c = config();
        assert!(c.contains(&format!("bars = {BARRAS}")));
        assert!(c.contains("method = raw"));
        assert!(c.contains("data_format = ascii"));
        assert!(c.contains("ascii_max_range = 100"));
        assert!(c.contains("channels = stereo"));
    }
}
