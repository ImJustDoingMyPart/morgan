use crate::hyprland::Espacios;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boton {
    pub id: i64,
    pub etiqueta: String,
    pub activo: bool,
    pub vacio: bool,
    pub urgente: bool,
}

/// Un fijo que vive en otra salida se ve allá.
pub fn botones(e: &Espacios, monitor: &str, fijos: &[i64]) -> Vec<Boton> {
    let activo = e
        .activos
        .iter()
        .find(|(m, _)| m == monitor)
        .map(|(_, id)| *id);
    let mut v: Vec<Boton> = e
        .espacios
        .iter()
        .filter(|w| w.monitor == monitor)
        .map(|w| (w.id, w.ventanas == 0))
        .chain(
            fijos
                .iter()
                .filter(|id| !e.espacios.iter().any(|w| w.id == **id))
                .map(|id| (*id, true)),
        )
        .map(|(id, vacio)| Boton {
            id,
            etiqueta: etiqueta(id),
            activo: activo == Some(id),
            vacio,
            urgente: e.urgentes.contains(&id),
        })
        .collect();
    v.sort_by_key(|b| b.id);
    v.dedup_by_key(|b| b.id);
    v
}

pub fn etiqueta(id: i64) -> String {
    const ROMANOS: [&str; 10] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"];
    usize::try_from(id - 1)
        .ok()
        .and_then(|i| ROMANOS.get(i))
        .map_or_else(|| id.to_string(), |r| (*r).to_owned())
}

/// Sin depender del locale del proceso.
pub fn fecha(dia: i32, mes: i32) -> String {
    const MESES: [&str; 12] = [
        "enero",
        "febrero",
        "marzo",
        "abril",
        "mayo",
        "junio",
        "julio",
        "agosto",
        "septiembre",
        "octubre",
        "noviembre",
        "diciembre",
    ];
    let mes = usize::try_from(mes - 1)
        .ok()
        .and_then(|i| MESES.get(i))
        .copied()
        .unwrap_or("?");
    format!("{dia} de {mes}")
}

pub fn campana(cantidad: usize, no_molestar: bool) -> (Vec<&'static str>, String) {
    let mut clases = Vec::new();
    if cantidad > 0 {
        clases.push("con-avisos");
    }
    if no_molestar {
        clases.push("no-molestar");
    }
    let tooltip = match cantidad {
        0 => "Sin notificaciones".to_owned(),
        1 => "1 notificación".to_owned(),
        n => format!("{n} notificaciones"),
    };
    (clases, tooltip)
}

pub fn duracion(segundos: u64) -> String {
    let (h, m, s) = (segundos / 3600, segundos / 60 % 60, segundos % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hyprland::Espacio;

    fn espacio(id: i64, monitor: &str, ventanas: u32) -> Espacio {
        Espacio {
            id,
            monitor: monitor.into(),
            ventanas,
        }
    }

    fn ids(v: &[Boton]) -> Vec<i64> {
        v.iter().map(|b| b.id).collect()
    }

    #[test]
    fn workspaces_de_la_salida_mas_los_fijos() {
        let e = Espacios {
            espacios: vec![
                espacio(1, "DP-3", 8),
                espacio(5, "DP-3", 1),
                espacio(4, "HDMI-A-1", 2),
            ],
            activos: vec![("DP-3".into(), 5), ("HDMI-A-1".into(), 4)],
            urgentes: [10].into(),
            enfocada: None,
        };
        let dp = botones(&e, "DP-3", &[1, 2, 3]);
        assert_eq!(ids(&dp), [1, 2, 3, 5]);
        assert!(!dp[0].vacio && dp[1].vacio && dp[2].vacio && !dp[3].vacio);
        assert_eq!(dp.iter().filter(|b| b.activo).count(), 1);
        assert!(dp[3].activo);
        // El 4 vive en la TV: no se repite en DP-3 aunque sea fijo de otro lado.
        let tv = botones(&e, "HDMI-A-1", &[4, 5, 6]);
        assert_eq!(ids(&tv), [4, 6]);
        assert!(tv[0].activo);
        // Una salida sin fijos muestra solo lo que tiene.
        assert!(botones(&e, "WAYLAND-1", &[]).is_empty());
    }

    #[test]
    fn el_juego_llama() {
        let e = Espacios {
            espacios: vec![espacio(1, "DP-3", 1), espacio(10, "DP-3", 1)],
            activos: vec![("DP-3".into(), 1)],
            urgentes: [10].into(),
            enfocada: None,
        };
        let b = botones(&e, "DP-3", &[1]);
        assert_eq!(ids(&b), [1, 10]);
        assert!(b[1].urgente && !b[0].urgente);
        assert_eq!(b[1].etiqueta, "X");
    }

    #[test]
    fn etiquetas() {
        assert_eq!(etiqueta(1), "I");
        assert_eq!(etiqueta(4), "IV");
        assert_eq!(etiqueta(9), "IX");
        assert_eq!(etiqueta(10), "X");
        assert_eq!(etiqueta(11), "11");
        assert_eq!(etiqueta(0), "0");
    }

    #[test]
    fn fecha_en_castellano() {
        assert_eq!(fecha(3, 10), "3 de octubre");
        assert_eq!(fecha(20, 9), "20 de septiembre");
        assert_eq!(fecha(1, 1), "1 de enero");
        assert_eq!(fecha(31, 12), "31 de diciembre");
    }

    #[test]
    fn campana_como_swaync() {
        assert_eq!(campana(0, false), (vec![], "Sin notificaciones".into()));
        assert_eq!(
            campana(1, false),
            (vec!["con-avisos"], "1 notificación".into())
        );
        assert_eq!(
            campana(29, true),
            (
                vec!["con-avisos", "no-molestar"],
                "29 notificaciones".into()
            )
        );
        assert_eq!(campana(0, true).0, ["no-molestar"]);
    }

    #[test]
    fn duracion_de_una_grabacion() {
        assert_eq!(duracion(7), "0:07");
        assert_eq!(duracion(754), "12:34");
        assert_eq!(duracion(3723), "1:02:03");
    }
}
