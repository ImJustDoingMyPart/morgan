//! Lo que importa de cómo escribe Google:
//! Lo que importa de cómo escribe Google:
//!
//! - Los recurrentes se expanden con `rrule`: sin eso, un cumpleaños cargado en 2019 no
//!   aparecería nunca. El `DTSTART` va con su zona (`TZID`), porque Google escribe el `UNTIL`
//!   en UTC y el RFC los empareja así.
//! - Una ocurrencia borrada es un `EXDATE` del evento; una movida o editada es otro `VEVENT`
//!   con el mismo `UID` y un `RECURRENCE-ID` (la ocurrencia que reemplaza). Esa sale del
//!   recurrente, y si el reemplazo está `CANCELLED` no se muestra ninguno de los dos.
//! - Lo que está adentro de un `VALARM` es de la alarma: su `SUMMARY` no es el del evento.
//! - Todo se lleva a la hora local antes de comparar y de mostrar: Google guarda en UTC los
//!   eventos creados desde otra zona o desde la web.

use std::collections::HashMap;

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use rrule::{RRule, RRuleSet, Tz, Unvalidated};

use crate::agenda::Evento;

const TOPE_OCURRENCIAS: u16 = 400;

#[derive(Debug, Default)]
struct Crudo {
    uid: String,
    inicio: Option<(DateTime<Tz>, bool)>,
    resumen: String,
    lugar: String,
    rrule: Option<String>,
    exdates: Vec<DateTime<Tz>>,
    reemplaza: Option<DateTime<Tz>>,
    cancelado: bool,
}

pub fn eventos(ics: &str, desde: DateTime<Local>, hasta: DateTime<Local>) -> Vec<Evento> {
    let crudos = leer(ics);
    let desde = desde.with_timezone(&Tz::LOCAL);
    let hasta = hasta.with_timezone(&Tz::LOCAL);

    // Las ocurrencias que tienen reemplazo, por UID: salen de su recurrente.
    let mut reemplazadas: HashMap<&str, Vec<DateTime<Tz>>> = HashMap::new();
    for c in &crudos {
        if let Some(r) = c.reemplaza {
            reemplazadas.entry(c.uid.as_str()).or_default().push(r);
        }
    }

    let mut salida: Vec<(DateTime<Tz>, Evento)> = Vec::new();
    for c in &crudos {
        let Some((inicio, dia_entero)) = c.inicio else {
            continue;
        };
        if c.cancelado {
            continue;
        }
        let fechas = match (&c.rrule, c.reemplaza) {
            (Some(regla), None) => {
                let mut ex = c.exdates.clone();
                ex.extend(reemplazadas.get(c.uid.as_str()).into_iter().flatten());
                expandir(regla, inicio, ex, desde, hasta)
            }
            _ => vec![inicio],
        };
        for f in fechas.into_iter().filter(|f| desde <= *f && *f <= hasta) {
            let local = f.with_timezone(&Local);
            salida.push((
                f,
                Evento {
                    inicio: local.to_rfc3339(),
                    dia_entero,
                    resumen: if c.resumen.is_empty() {
                        "(sin título)".into()
                    } else {
                        c.resumen.clone()
                    },
                    lugar: c.lugar.clone(),
                },
            ));
        }
    }
    salida.sort_by_key(|(f, _)| *f);
    salida.into_iter().map(|(_, e)| e).collect()
}

/// Si la regla no se deja leer, el evento queda con su fecha original.
fn expandir(
    regla: &str,
    inicio: DateTime<Tz>,
    exdates: Vec<DateTime<Tz>>,
    desde: DateTime<Tz>,
    hasta: DateTime<Tz>,
) -> Vec<DateTime<Tz>> {
    let armar = |inicio: DateTime<Tz>| -> Option<Vec<DateTime<Tz>>> {
        let r = regla.parse::<RRule<Unvalidated>>().ok()?.validate(inicio).ok()?;
        let set = RRuleSet::new(inicio)
            .rrule(r)
            .set_exdates(exdates.clone())
            .after(desde)
            .before(hasta);
        Some(set.all(TOPE_OCURRENCIAS).dates)
    };
    // Un UNTIL que no está en UTC con un DTSTART con zona no valida: se prueba con el inicio en
    // hora local, que lo admite.
    armar(inicio)
        .or_else(|| armar(inicio.with_timezone(&Tz::LOCAL)))
        .unwrap_or_else(|| vec![inicio])
}

fn leer(ics: &str) -> Vec<Crudo> {
    let mut lineas: Vec<String> = Vec::new();
    for l in ics.lines() {
        match (l.strip_prefix(' ').or_else(|| l.strip_prefix('\t')), lineas.last_mut()) {
            (Some(resto), Some(ultima)) => ultima.push_str(resto),
            _ => lineas.push(l.to_owned()),
        }
    }
    let mut crudos = Vec::new();
    let mut actual: Option<Crudo> = None;
    // Profundidad dentro del evento: lo de un VALARM (o cualquier componente anidado) no es
    // del evento.
    let mut anidado = 0u32;
    for l in &lineas {
        if l == "BEGIN:VEVENT" {
            actual = Some(Crudo::default());
            anidado = 0;
            continue;
        }
        if l == "END:VEVENT" {
            if let Some(c) = actual.take() {
                crudos.push(c);
            }
            continue;
        }
        let Some(c) = actual.as_mut() else { continue };
        if l.starts_with("BEGIN:") {
            anidado += 1;
            continue;
        }
        if l.starts_with("END:") {
            anidado = anidado.saturating_sub(1);
            continue;
        }
        if anidado > 0 {
            continue;
        }
        let Some((nombre, params, valor)) = propiedad(l) else {
            continue;
        };
        match nombre.as_str() {
            "UID" => c.uid = valor.trim().to_owned(),
            "DTSTART" => c.inicio = fecha(valor, params),
            "SUMMARY" => c.resumen = texto(valor),
            "LOCATION" => c.lugar = texto(valor),
            "RRULE" => c.rrule = Some(valor.trim().to_owned()),
            "EXDATE" => c
                .exdates
                .extend(valor.split(',').filter_map(|v| fecha(v, params)).map(|(f, _)| f)),
            "RECURRENCE-ID" => c.reemplaza = fecha(valor, params).map(|(f, _)| f),
            "STATUS" => c.cancelado = valor.trim().eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }
    crudos
}

/// Un `TZID` entre comillas puede tener `:`.
fn propiedad(l: &str) -> Option<(String, &str, &str)> {
    let mut comillas = false;
    let corte = l.char_indices().find_map(|(i, ch)| match ch {
        '"' => {
            comillas = !comillas;
            None
        }
        ':' if !comillas => Some(i),
        _ => None,
    })?;
    let (cabeza, valor) = (&l[..corte], &l[corte + 1..]);
    let (nombre, params) = cabeza.split_once(';').unwrap_or((cabeza, ""));
    Some((nombre.to_ascii_uppercase(), params, valor))
}

fn parametro<'a>(params: &'a str, clave: &str) -> Option<&'a str> {
    params.split(';').find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.eq_ignore_ascii_case(clave).then(|| v.trim_matches('"'))
    })
}

fn fecha(valor: &str, params: &str) -> Option<(DateTime<Tz>, bool)> {
    let valor = valor.trim();
    let es_dia = parametro(params, "VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE"))
        || (valor.len() == 8 && valor.bytes().all(|b| b.is_ascii_digit()));
    if es_dia {
        let d = NaiveDate::parse_from_str(valor.get(..8)?, "%Y%m%d").ok()?;
        let f = Tz::LOCAL.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).earliest()?;
        return Some((f, true));
    }
    if let Some(utc) = valor.strip_suffix('Z') {
        let n = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some((Tz::UTC.from_utc_datetime(&n), false));
    }
    let n = NaiveDateTime::parse_from_str(valor.get(..15)?, "%Y%m%dT%H%M%S").ok()?;
    let zona = parametro(params, "TZID")
        .and_then(|t| t.parse::<chrono_tz::Tz>().ok())
        .map_or(Tz::LOCAL, Tz::Tz);
    Some((zona.from_local_datetime(&n).earliest()?, false))
}

fn texto(valor: &str) -> String {
    let mut s = String::with_capacity(valor.len());
    let mut chars = valor.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            s.push(ch);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => s.push(' '),
            Some(otro) => s.push(otro),
            None => {}
        }
    }
    s.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(a: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(a, m, d, h, mi, 0).unwrap()
    }

    fn feed(eventos: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{eventos}END:VCALENDAR\r\n")
    }

    fn resumenes(v: &[Evento]) -> Vec<(String, String)> {
        v.iter()
            .map(|e| (e.inicio[..16].to_owned(), e.resumen.clone()))
            .collect()
    }

    #[test]
    fn simple_plegado_y_escapes() {
        let ics = feed(
            "BEGIN:VEVENT\r\nUID:a\r\nDTSTART;TZID=America/Argentina/Buenos_Aires:20261020T100000\r\n\
             SUMMARY:Reuni\r\n ón\\, café\\; y \\\\ algo\\nmás\r\nLOCATION:Calle 1\\, CABA\r\n\
             BEGIN:VALARM\r\nSUMMARY:no es esto\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VEVENT\r\n",
        );
        let v = eventos(&ics, local(2026, 10, 1, 0, 0), local(2026, 10, 31, 0, 0));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].resumen, "Reunión, café; y \\ algo más");
        assert_eq!(v[0].lugar, "Calle 1, CABA");
        assert!(!v[0].dia_entero);
    }

    #[test]
    fn utc_a_hora_local() {
        let ics = feed("BEGIN:VEVENT\r\nUID:b\r\nDTSTART:20261020T200000Z\r\nSUMMARY:X\r\nEND:VEVENT\r\n");
        let v = eventos(&ics, local(2026, 10, 1, 0, 0), local(2026, 10, 31, 0, 0));
        let esperado = Tz::UTC
            .with_ymd_and_hms(2026, 10, 20, 20, 0, 0)
            .unwrap()
            .with_timezone(&Local)
            .to_rfc3339();
        assert_eq!(v[0].inicio, esperado);
    }

    #[test]
    fn recurrente_con_exdate_y_ocurrencia_movida() {
        let ics = feed(
            "BEGIN:VEVENT\r\nUID:r\r\nDTSTART;VALUE=DATE:20260105\r\nRRULE:FREQ=WEEKLY;BYDAY=MO\r\n\
             EXDATE;VALUE=DATE:20261012\r\nSUMMARY:Lunes\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:r\r\nRECURRENCE-ID;VALUE=DATE:20261019\r\nDTSTART;VALUE=DATE:20261021\r\n\
             SUMMARY:Lunes (movido)\r\nEND:VEVENT\r\n",
        );
        let v = eventos(&ics, local(2026, 10, 1, 0, 0), local(2026, 10, 31, 0, 0));
        assert_eq!(
            resumenes(&v)
                .iter()
                .map(|(f, r)| format!("{} {r}", &f[..10]))
                .collect::<Vec<_>>(),
            [
                "2026-10-05 Lunes",
                // El 12 está borrado; el 19 se movió al 21.
                "2026-10-21 Lunes (movido)",
                "2026-10-26 Lunes",
            ]
        );
        assert!(v.iter().all(|e| e.dia_entero));
    }

    #[test]
    fn una_ocurrencia_cancelada_no_se_ve() {
        let ics = feed(
            "BEGIN:VEVENT\r\nUID:c\r\nDTSTART:20261001T120000Z\r\nRRULE:FREQ=DAILY;COUNT=3\r\n\
             SUMMARY:Diario\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:c\r\nRECURRENCE-ID:20261002T120000Z\r\nDTSTART:20261002T120000Z\r\n\
             STATUS:CANCELLED\r\nSUMMARY:Diario\r\nEND:VEVENT\r\n",
        );
        let v = eventos(&ics, local(2026, 9, 30, 0, 0), local(2026, 10, 31, 0, 0));
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn until_en_utc_con_dtstart_con_zona() {
        let ics = feed(
            "BEGIN:VEVENT\r\nUID:u\r\nDTSTART;TZID=America/Argentina/Buenos_Aires:20261001T090000\r\n\
             RRULE:FREQ=DAILY;UNTIL=20261003T120000Z\r\nSUMMARY:Tres\r\nEND:VEVENT\r\n",
        );
        let v = eventos(&ics, local(2026, 9, 1, 0, 0), local(2026, 10, 31, 0, 0));
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn fuera_de_la_ventana_y_roto() {
        let ics = feed(
            "BEGIN:VEVENT\r\nUID:x\r\nDTSTART:20251020T100000Z\r\nSUMMARY:Viejo\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:y\r\nDTSTART:roto\r\nSUMMARY:Roto\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:z\r\nDTSTART:20261020T100000Z\r\nRRULE:FREQ=NUNCA\r\nEND:VEVENT\r\n",
        );
        let v = eventos(&ics, local(2026, 10, 1, 0, 0), local(2026, 10, 31, 0, 0));
        // La regla ilegible queda con su fecha; sin título, el de siempre.
        assert_eq!(resumenes(&v).len(), 1);
        assert_eq!(v[0].resumen, "(sin título)");
    }
}

/// Para comparar con otra implementación sobre un feed real: `MORGAN_ICS=<archivo>
/// MORGAN_ICS_DIAS=<n> [MORGAN_ICS_ATRAS=<n>] cargo test -p morgan ical::comparar -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore]
fn comparar() {
    let ics = std::fs::read_to_string(std::env::var("MORGAN_ICS").unwrap()).unwrap();
    let dias: i64 = std::env::var("MORGAN_ICS_DIAS").unwrap().parse().unwrap();
    let atras: i64 = std::env::var("MORGAN_ICS_ATRAS").map_or(0, |a| a.parse().unwrap());
    let desde = (Local::now() - chrono::Duration::days(atras))
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|n| Local.from_local_datetime(&n).earliest())
        .unwrap();
    for e in eventos(&ics, desde, desde + chrono::Duration::days(dias)) {
        println!("OCURRENCIA\t{}\t{}\t{}", &e.inicio[..19], e.dia_entero, e.resumen);
    }
}
