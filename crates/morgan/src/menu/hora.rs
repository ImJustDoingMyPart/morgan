//! Fecha, hora y zona horaria, por timedated (`org.freedesktop.timedate1`, que arranca por
//! activación de D-Bus). Cambiar algo pasa por polkit (`interactive`): si hace falta, el
//! agente pide la contraseña.
//!
//! La zona se elige en dos listas, región y ciudad: una sola con las ~600 de la base no se
//! podría recorrer. Elegir una región no cambia nada; solo muestra sus ciudades.

use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
use zbus::zvariant::OwnedValue;

use super::arbol::{Control, Fila, Que, ajuste, opcion};
use crate::sistema;

const TD: &str = "org.freedesktop.timedate1";
const RUTA: &str = "/org/freedesktop/timedate1";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lectura {
    pub zona: String,
    pub ntp: bool,
    pub puede_ntp: bool,
    pub sincronizada: bool,
    pub zonas: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cambio {
    Ntp(bool),
    Zona(String),
    /// Solo la región que se mira: no toca el sistema.
    Region(String),
    /// `AAAA-MM-DD`, con la hora de ahora.
    Fecha(String),
    /// `HH:MM`, con la fecha de hoy.
    Reloj(String),
}

pub async fn leer() -> Lectura {
    let r = async {
        let p = sistema::proxy(TD, RUTA, TD).await?;
        let prop = |k: &'static str| {
            let p = p.clone();
            async move { p.get_property::<OwnedValue>(k).await }
        };
        let bool_de = |v: zbus::Result<OwnedValue>| v.ok().and_then(|v| bool::try_from(v).ok());
        Ok::<_, zbus::Error>(Lectura {
            zona: prop("Timezone")
                .await
                .ok()
                .and_then(|v| String::try_from(v).ok())
                .unwrap_or_default(),
            ntp: bool_de(prop("NTP").await).unwrap_or(false),
            puede_ntp: bool_de(prop("CanNTP").await).unwrap_or(false),
            sincronizada: bool_de(prop("NTPSynchronized").await).unwrap_or(false),
            zonas: p.call("ListTimezones", &()).await.unwrap_or_default(),
        })
    };
    r.await.unwrap_or_else(|e| {
        eprintln!("morgan: menú: timedated: {e}");
        Lectura::default()
    })
}

pub async fn aplicar(c: Cambio) -> Result<(), String> {
    let p = sistema::proxy(TD, RUTA, TD)
        .await
        .map_err(|e| format!("timedated: {e}"))?;
    let zona: String = p
        .get_property("Timezone")
        .await
        .map_err(|e| format!("timedated: {e}"))?;
    let zona = zona.as_str();
    let r = match c {
        Cambio::Ntp(si) => p.call::<_, _, ()>("SetNTP", &(si, true)).await,
        Cambio::Zona(z) => p.call::<_, _, ()>("SetTimezone", &(z.as_str(), true)).await,
        Cambio::Region(_) => return Ok(()),
        Cambio::Fecha(_) | Cambio::Reloj(_) => {
            let usec = instante(&c, zona, Utc::now())?;
            p.call::<_, _, ()>("SetTime", &(usec, false, true)).await
        }
    };
    r.map_err(|e| format!("timedated: {e}"))
}

/// Microsegundos desde 1970 de la fecha o la hora escrita, completada con la otra mitad de
/// `ahora` en la zona `zona`.
pub fn instante(c: &Cambio, zona: &str, ahora: chrono::DateTime<Utc>) -> Result<i64, String> {
    let tz: chrono_tz::Tz = zona
        .parse()
        .map_err(|_| format!("zona desconocida: {zona}"))?;
    let local = ahora.with_timezone(&tz).naive_local();
    let nueva = match c {
        Cambio::Fecha(s) => NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
            .map_err(|_| format!("fecha inválida: {s}"))?
            .and_time(local.time()),
        Cambio::Reloj(s) => local.date().and_time(
            NaiveTime::parse_from_str(s.trim(), "%H:%M")
                .map_err(|_| format!("hora inválida: {s}"))?,
        ),
        _ => return Err("no es una fecha ni una hora".into()),
    };
    tz.from_local_datetime(&nueva)
        .earliest()
        .map(|t| t.timestamp_micros())
        .ok_or_else(|| format!("esa hora no existe en {zona}"))
}

/// La región de una zona: lo que va antes de la primera barra («America»); las sueltas
/// («UTC») y las de `Etc` van juntas.
pub fn region(zona: &str) -> &str {
    match zona.split_once('/') {
        Some(("Etc", _)) | None => "Etc",
        Some((r, _)) => r,
    }
}

pub fn nombre_region(r: &str) -> &'static str {
    match r {
        "Africa" => tr!("África"),
        "America" => tr!("América"),
        "Antarctica" => tr!("Antártida"),
        "Arctic" => tr!("Ártico"),
        "Asia" => tr!("Asia"),
        "Atlantic" => tr!("Atlántico"),
        "Australia" => tr!("Australia"),
        "Europe" => tr!("Europa"),
        "Indian" => tr!("Índico"),
        "Pacific" => tr!("Pacífico"),
        _ => tr!("Otras"),
    }
}

/// «America/Argentina/Buenos_Aires» → «Buenos Aires (Argentina)».
pub fn ciudad(zona: &str) -> String {
    let partes: Vec<&str> = zona.split('/').collect();
    let limpio = |s: &str| s.replace('_', " ");
    match partes.as_slice() {
        [_, medio @ .., ultima] if !medio.is_empty() => {
            format!(
                "{} ({})",
                limpio(ultima),
                medio
                    .iter()
                    .map(|m| limpio(m))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        [_, ultima] => limpio(ultima),
        _ => limpio(zona),
    }
}

pub fn filas(l: &Lectura, mirada: Option<&str>, ahora: chrono::DateTime<Utc>) -> Vec<Fila> {
    if l.zona.is_empty() {
        return vec![ajuste(
            tr!("Sin datos"),
            tr!("timedated no respondió"),
            Control::Nada,
        )];
    }
    let hora = |c: Cambio| Que::Hora(c);
    let local = l
        .zona
        .parse::<chrono_tz::Tz>()
        .ok()
        .map(|tz| ahora.with_timezone(&tz).naive_local());
    let mut v = Vec::new();
    if l.puede_ntp {
        v.push(ajuste(
            tr!("Hora automática"),
            if !l.ntp {
                tr!("apagada · la fecha y la hora se ponen a mano")
            } else if l.sincronizada {
                tr!("por internet · sincronizada")
            } else {
                tr!("por internet · todavía sin sincronizar")
            },
            Control::Casilla {
                activa: l.ntp,
                que: hora(Cambio::Ntp(!l.ntp)),
            },
        ));
    }
    if !l.ntp
        && let Some(local) = local
    {
        let fecha = local.format("%Y-%m-%d").to_string();
        let reloj = local.format("%H:%M").to_string();
        v.push(ajuste(
            tr!("Fecha"),
            &fecha,
            Control::Texto {
                valor: fecha.clone(),
                pista: "AAAA-MM-DD".into(),
                que: hora(Cambio::Fecha(fecha.clone())),
            },
        ));
        v.push(ajuste(
            tr!("Hora"),
            &reloj,
            Control::Texto {
                valor: reloj.clone(),
                pista: "HH:MM".into(),
                que: hora(Cambio::Reloj(reloj.clone())),
            },
        ));
    }
    let puesta = region(&l.zona);
    let mirada = mirada.unwrap_or(puesta);
    let mut regiones: Vec<&str> = l.zonas.iter().map(|z| region(z)).collect();
    regiones.sort_by_key(|r| nombre_region(r));
    regiones.dedup();
    v.push(ajuste(
        tr!("Región"),
        nombre_region(mirada),
        Control::Lista(
            regiones
                .iter()
                .map(|r| {
                    opcion(nombre_region(r), "", hora(Cambio::Region((*r).to_owned())))
                        .activa(*r == mirada)
                })
                .collect(),
        ),
    ));
    let mut ciudades: Vec<(String, &String)> = l
        .zonas
        .iter()
        .filter(|z| region(z) == mirada)
        .map(|z| (ciudad(z), z))
        .collect();
    ciudades.sort();
    v.push(ajuste(
        tr!("Ciudad"),
        &if mirada == puesta {
            ciudad(&l.zona)
        } else {
            tr!("elegir una para cambiar la zona").to_owned()
        },
        Control::Lista(
            ciudades
                .into_iter()
                .map(|(c, z)| opcion(&c, "", hora(Cambio::Zona(z.clone()))).activa(*z == l.zona))
                .collect(),
        ),
    ));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lectura(ntp: bool) -> Lectura {
        Lectura {
            zona: "America/Argentina/Buenos_Aires".into(),
            ntp,
            puede_ntp: true,
            sincronizada: true,
            zonas: [
                "America/Argentina/Buenos_Aires",
                "America/New_York",
                "Europe/Madrid",
                "UTC",
                "Etc/GMT+3",
            ]
            .map(String::from)
            .to_vec(),
        }
    }

    fn titulos(f: &[Fila]) -> Vec<String> {
        f.iter()
            .map(|f| match f {
                Fila::Ajuste { titulo, .. } => titulo.clone(),
                _ => String::new(),
            })
            .collect()
    }

    #[test]
    fn nombres_de_zonas() {
        assert_eq!(region("America/Argentina/Buenos_Aires"), "America");
        assert_eq!(region("UTC"), "Etc");
        assert_eq!(region("Etc/GMT+3"), "Etc");
        assert_eq!(
            ciudad("America/Argentina/Buenos_Aires"),
            "Buenos Aires (Argentina)"
        );
        assert_eq!(ciudad("America/New_York"), "New York");
        assert_eq!(ciudad("UTC"), "UTC");
    }

    #[test]
    fn a_mano_solo_sin_hora_automatica() {
        let ahora = Utc::now();
        assert_eq!(
            titulos(&filas(&lectura(true), None, ahora)),
            ["Hora automática", "Región", "Ciudad"]
        );
        assert_eq!(
            titulos(&filas(&lectura(false), None, ahora)),
            ["Hora automática", "Fecha", "Hora", "Región", "Ciudad"]
        );
    }

    #[test]
    fn la_region_mirada_muestra_sus_ciudades() {
        let f = filas(&lectura(true), Some("Europe"), Utc::now());
        let Fila::Ajuste {
            control: Control::Lista(o),
            ..
        } = &f[2]
        else {
            panic!("ciudad");
        };
        let n: Vec<_> = o.iter().map(|o| o.titulo.as_str()).collect();
        assert_eq!(n, ["Madrid"]);
        assert!(!o[0].activa);
    }

    #[test]
    fn fecha_y_hora_en_la_zona() {
        // 2026-10-18 15:00 UTC = 12:00 en Buenos Aires (UTC-3).
        let ahora = Utc.with_ymd_and_hms(2026, 10, 18, 15, 0, 0).unwrap();
        let z = "America/Argentina/Buenos_Aires";
        let t = instante(&Cambio::Reloj("09:30".into()), z, ahora).unwrap();
        assert_eq!(
            t,
            Utc.with_ymd_and_hms(2026, 10, 18, 12, 30, 0)
                .unwrap()
                .timestamp_micros()
        );
        let t = instante(&Cambio::Fecha("2026-12-31".into()), z, ahora).unwrap();
        assert_eq!(
            t,
            Utc.with_ymd_and_hms(2026, 12, 31, 15, 0, 0)
                .unwrap()
                .timestamp_micros()
        );
        assert!(instante(&Cambio::Reloj("25:00".into()), z, ahora).is_err());
        assert!(instante(&Cambio::Fecha("ayer".into()), z, ahora).is_err());
    }
}
