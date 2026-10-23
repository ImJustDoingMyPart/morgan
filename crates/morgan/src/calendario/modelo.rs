use gtk::glib;

use crate::agenda::Evento;

pub const MESES: [&str; 12] = [
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clima {
    pub descripcion: String,
    pub temperaturas: String,
    pub glifo: String,
}

/// Lo que no se puede leer queda vacío: el calendario se dibuja sin clima.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Pronostico {
    pub ahora: Option<String>,
    pub dias: Vec<Clima>,
}

/// La semana empieza el lunes.
pub fn mes(año: i32, mes: i32) -> (u32, u32) {
    let zona = glib::TimeZone::utc();
    let Ok(primero) = glib::DateTime::new(&zona, año, mes, 1, 12, 0, 0.0) else {
        return (0, 30);
    };
    let dias = primero
        .add_months(1)
        .and_then(|s| s.add_days(-1))
        .map(|u| u.day_of_month() as u32)
        .unwrap_or(30);
    ((primero.day_of_week() - 1) as u32, dias)
}

pub fn distancia(desde: (i32, i32, i32), hasta: (i32, i32, i32)) -> i64 {
    let dia = |(a, m, d): (i32, i32, i32)| {
        glib::DateTime::from_utc(a, m, d, 0, 0, 0.0)
            .map(|u| u.to_unix() / 86_400)
            .unwrap_or(0)
    };
    dia(hasta) - dia(desde)
}

pub fn correr(dia: (i32, i32, i32), n: i32) -> (i32, i32, i32) {
    glib::DateTime::from_utc(dia.0, dia.1, dia.2, 12, 0, 0.0)
        .and_then(|d| d.add_days(n))
        .map(|d| (d.year(), d.month(), d.day_of_month()))
        .unwrap_or(dia)
}

pub fn correr_mes((año, mes): (i32, i32), n: i32) -> (i32, i32) {
    let i = año * 12 + (mes - 1) + n;
    (i.div_euclid(12), i.rem_euclid(12) + 1)
}

pub fn del_mes<'a>(evs: &'a [Evento], mes: (i32, i32), zona: &glib::TimeZone) -> Vec<&'a Evento> {
    let mut v: Vec<(&Evento, i64)> = evs
        .iter()
        .filter_map(|e| {
            let i = e.comienzo(zona)?;
            ((i.year(), i.month()) == mes).then(|| (e, i.to_unix()))
        })
        .collect();
    v.sort_by_key(|(_, t)| *t);
    v.into_iter().map(|(e, _)| e).collect()
}

pub fn dia(e: &Evento, zona: &glib::TimeZone) -> Option<(i32, i32, i32)> {
    e.comienzo(zona)
        .map(|i| (i.year(), i.month(), i.day_of_month()))
}

pub fn dias_con_eventos(evs: &[Evento], mes: (i32, i32), zona: &glib::TimeZone) -> Vec<u32> {
    let mut v: Vec<u32> = evs
        .iter()
        .filter_map(|e| {
            let i = e.comienzo(zona)?;
            ((i.year(), i.month()) == mes).then(|| i.day_of_month() as u32)
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

const DIAS: [&str; 7] = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];

/// El mes ya está en el rótulo.
pub fn cuando(e: &Evento, zona: &glib::TimeZone) -> String {
    let Some(i) = e.comienzo(zona) else {
        return String::new();
    };
    let dia = format!(
        "{} {}",
        DIAS[(i.day_of_week() - 1) as usize],
        i.day_of_month()
    );
    if e.dia_entero {
        dia
    } else {
        format!("{dia} · {:02}:{:02}", i.hour(), i.minute())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(inicio: &str, dia_entero: bool, resumen: &str) -> Evento {
        Evento {
            inicio: inicio.into(),
            dia_entero,
            resumen: resumen.into(),
            lugar: String::new(),
        }
    }

    #[test]
    fn forma_del_mes() {
        // Octubre de 2026 empieza en jueves; febrero de 2028 es bisiesto.
        assert_eq!(mes(2026, 10), (3, 31));
        assert_eq!(mes(2028, 2), (1, 29));
        assert_eq!(mes(2026, 6), (0, 30));
    }

    #[test]
    fn correr_dias_y_meses() {
        assert_eq!(correr((2026, 10, 31), 1), (2026, 11, 1));
        assert_eq!(correr((2026, 1, 3), -7), (2025, 12, 27));
        assert_eq!(correr_mes((2026, 12), 1), (2027, 1));
        assert_eq!(correr_mes((2026, 1), -1), (2025, 12));
        assert_eq!(distancia((2026, 10, 4), (2026, 10, 10)), 6);
        assert_eq!(distancia((2026, 10, 4), (2026, 9, 30)), -4);
    }

    #[test]
    fn eventos_del_mes() {
        let zona = glib::TimeZone::from_offset(-3 * 3600);
        let evs = vec![
            ev("2026-10-17T15:00:00-03:00", false, "tarde"),
            ev("2026-10-17T00:00:00-03:00", true, "cumplemes"),
            ev("2026-10-18T09:00:00-03:00", false, "otro día"),
            // 02:00 UTC del 18 es el 17 a las 23 en UTC-3.
            ev("2026-10-18T02:00:00Z", false, "noche"),
        ];
        let r: Vec<&str> = del_mes(&evs, (2026, 10), &zona)
            .iter()
            .map(|e| e.resumen.as_str())
            .collect();
        assert_eq!(r, ["cumplemes", "tarde", "noche", "otro día"]);
        assert_eq!(dia(&evs[3], &zona), Some((2026, 10, 17)));
        assert_eq!(dias_con_eventos(&evs, (2026, 10), &zona), [17, 18]);
        assert!(dias_con_eventos(&evs, (2026, 11), &zona).is_empty());
        assert_eq!(cuando(&evs[0], &zona), "sáb 17 · 15:00");
        assert_eq!(cuando(&evs[1], &zona), "sáb 17");
    }
}
