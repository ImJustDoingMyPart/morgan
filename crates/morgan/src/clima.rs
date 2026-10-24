//! Open-Meteo: sin API key ni cuenta. Se pide `hourly` y no `current` a propósito: con una caché
//! de 6 horas, `current` quedaría hasta 6 horas viejo. El array por hora se indexa por la hora
//! local de ahora, así que una descarga alimenta una temperatura que se refresca sola. La caché
//! guarda la ubicación con la que se pidió: cambiar de ciudad no muestra la anterior.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::calendario::modelo::{Clima, Pronostico};
use crate::comando;
use crate::config;

const API: &str = "https://api.open-meteo.com/v1/forecast";
const GEOCODER: &str = "https://geocoding-api.open-meteo.com/v1/search";
/// La red se toca 4 veces por día.
const CADUCA: Duration = Duration::from_secs(6 * 3600);
const DIAS: usize = 7;

/// El sol despejado es U+F05A8 y no U+F0599: ese dibuja 10 px de tinta fina contra los 13×13
/// del resto.
const CODIGOS: &[(i64, &str, &str, &str)] = &[
    (0, "Despejado", "󰖨", "󰖔"),
    (1, "Mayormente despejado", "󰖕", "󰼱"),
    (2, "Parcialmente nublado", "󰖕", "󰼱"),
    (3, "Nublado", "󰖐", "󰖐"),
    (45, "Niebla", "󰖑", "󰖑"),
    (48, "Niebla con escarcha", "󰖑", "󰖑"),
    (51, "Llovizna leve", "󰖗", "󰖗"),
    (53, "Llovizna", "󰖗", "󰖗"),
    (55, "Llovizna intensa", "󰖗", "󰖗"),
    (56, "Llovizna helada", "󰙿", "󰙿"),
    (57, "Llovizna helada intensa", "󰙿", "󰙿"),
    (61, "Lluvia leve", "󰖗", "󰖗"),
    (63, "Lluvia", "󰖖", "󰖖"),
    (65, "Lluvia intensa", "󰖖", "󰖖"),
    (66, "Lluvia helada", "󰙿", "󰙿"),
    (67, "Lluvia helada intensa", "󰙿", "󰙿"),
    (71, "Nevada leve", "󰖘", "󰖘"),
    (73, "Nevada", "󰖘", "󰖘"),
    (75, "Nevada intensa", "󰖘", "󰖘"),
    (77, "Granos de nieve", "󰖘", "󰖘"),
    (80, "Chaparrones leves", "󰖗", "󰖗"),
    (81, "Chaparrones", "󰖖", "󰖖"),
    (82, "Chaparrones fuertes", "󰖖", "󰖖"),
    (85, "Chubascos de nieve", "󰖘", "󰖘"),
    (86, "Chubascos de nieve fuertes", "󰖘", "󰖘"),
    (95, "Tormenta", "󰖓", "󰖓"),
    (96, "Tormenta con granizo", "󰙾", "󰙾"),
    (99, "Tormenta con granizo fuerte", "󰙾", "󰙾"),
];

const DESCONOCIDO: (&str, &str) = ("Sin datos", "󰼯");

fn codigo(c: i64, de_dia: bool) -> (&'static str, &'static str) {
    CODIGOS
        .iter()
        .find(|x| x.0 == c)
        .map_or(DESCONOCIDO, |x| (x.1, if de_dia { x.2 } else { x.3 }))
}

/// Con el medio hacia arriba: una máxima de 12,5 es 13 (redondear al par la mostraría como 12).
fn temp(v: f64) -> i64 {
    (v + 0.5).floor() as i64
}

fn cache() -> PathBuf {
    glib::user_cache_dir().join("morgan/clima.json")
}

pub async fn pronostico(c: &config::Clima) -> Pronostico {
    let (Some(lat), Some(lon)) = (c.latitud, c.longitud) else {
        return Pronostico::default();
    };
    let Some(datos) = datos(lat, lon).await else {
        return Pronostico::default();
    };
    let ahora = glib::DateTime::now_local()
        .and_then(|d| d.format("%Y-%m-%dT%H"))
        .map(|s| s.to_string())
        .unwrap_or_default();
    armar(&datos, &ahora)
}

/// Si la red falla, la caché vieja de esta ubicación: un pronóstico de ayer sirve más que
/// ninguno.
async fn datos(lat: f64, lon: f64) -> Option<Value> {
    let guardado = std::fs::read_to_string(cache())
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v["latitud"].as_f64() == Some(lat) && v["longitud"].as_f64() == Some(lon));
    let fresca = std::fs::metadata(cache())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|edad| edad < CADUCA);
    if let Some(g) = &guardado
        && fresca
    {
        return Some(g["datos"].clone());
    }
    let url = format!(
        "{API}?latitude={lat}&longitude={lon}&hourly=temperature_2m,weather_code,is_day\
         &daily=weather_code,temperature_2m_max,temperature_2m_min&timezone=auto\
         &forecast_days={DIAS}"
    );
    // --max-time: mejor la caché vieja (o nada) que dejar el calendario esperando a la red.
    let red = comando::salida(&["curl", "-fsSL", "--max-time", "8", &url])
        .await
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v.get("daily").is_some() && v.get("hourly").is_some());
    match red {
        Some(d) => {
            let todo = serde_json::json!({ "latitud": lat, "longitud": lon, "datos": d });
            if let Err(e) = guardar(&todo.to_string()) {
                eprintln!("morgan: clima: {e}");
            }
            Some(d)
        }
        None => guardado.map(|g| g["datos"].clone()),
    }
}

fn guardar(texto: &str) -> std::io::Result<()> {
    let ruta = cache();
    if let Some(d) = ruta.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = ruta.with_extension("tmp");
    std::fs::write(&tmp, texto)?;
    std::fs::rename(&tmp, &ruta)
}

/// Las horas vienen en hora local (`timezone=auto`), sin offset: se comparan como texto.
pub fn armar(d: &Value, ahora: &str) -> Pronostico {
    let h = &d["hourly"];
    let i = h["time"]
        .as_array()
        .and_then(|horas| horas.iter().position(|t| t.as_str().is_some_and(|t| t.starts_with(ahora))));
    let ahora = i.and_then(|i| h["temperature_2m"][i].as_f64()).map(|t| format!("{}°", temp(t)));
    let dia = &d["daily"];
    let dias = (0..DIAS)
        .map_while(|k| {
            // Para los días, siempre el glifo de día: una luna al lado de un resumen de 24
            // horas no significa nada.
            let (desc, glifo) = codigo(dia["weather_code"][k].as_i64()?, true);
            Some(Clima {
                descripcion: desc.to_owned(),
                temperaturas: format!(
                    "{}° / {}°",
                    temp(dia["temperature_2m_max"][k].as_f64()?),
                    temp(dia["temperature_2m_min"][k].as_f64()?)
                ),
                glifo: glifo.to_owned(),
            })
        })
        .collect();
    Pronostico { ahora, dias }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lugar {
    pub nombre: String,
    pub region: String,
    pub latitud: f64,
    pub longitud: f64,
}

/// Con menos de dos letras el geocoder no contesta nada, así que ni se pregunta.
pub async fn buscar(nombre: &str) -> Vec<Lugar> {
    let nombre = nombre.trim();
    if nombre.chars().count() < 2 {
        return Vec::new();
    }
    let idioma = glib::language_names()
        .first()
        .map(|l| l.chars().take(2).collect::<String>())
        .filter(|l| l.len() == 2 && l.chars().all(|c| c.is_ascii_lowercase()))
        .unwrap_or_else(|| "en".into());
    let url = format!(
        "{GEOCODER}?name={}&count=10&language={idioma}&format=json",
        glib::Uri::escape_string(nombre, None, false)
    );
    match comando::salida(&["curl", "-fsSL", "--max-time", "8", &url]).await {
        Ok(t) => lugares(&t),
        Err(e) => {
            eprintln!("morgan: clima: {e}");
            Vec::new()
        }
    }
}

pub fn lugares(json: &str) -> Vec<Lugar> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    v["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let region: Vec<&str> = ["admin1", "country"]
                .iter()
                .filter_map(|k| r[*k].as_str())
                .collect();
            Some(Lugar {
                nombre: r["name"].as_str()?.to_owned(),
                region: region.join(", "),
                latitud: r["latitude"].as_f64()?,
                longitud: r["longitude"].as_f64()?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn muestra() -> Value {
        serde_json::json!({
            "hourly": {
                "time": ["2026-10-07T00:00", "2026-10-07T01:00", "2026-10-07T02:00"],
                "temperature_2m": [12.4, 12.5, 11.0],
                "weather_code": [0, 3, 61],
                "is_day": [0, 0, 0]
            },
            "daily": {
                "time": ["2026-10-07", "2026-10-08"],
                "weather_code": [81, 1],
                "temperature_2m_max": [17.2, 22.5],
                "temperature_2m_min": [12.0, -0.6]
            }
        })
    }

    #[test]
    fn la_hora_de_ahora_y_los_dias() {
        let p = armar(&muestra(), "2026-10-07T01");
        assert_eq!(p.ahora.as_deref(), Some("13°"));
        assert_eq!(p.dias.len(), 2);
        assert_eq!(p.dias[0].descripcion, "Chaparrones");
        assert_eq!(p.dias[0].temperaturas, "17° / 12°");
        // El medio redondea hacia arriba, también bajo cero: −0,6 es −1.
        assert_eq!(p.dias[1].temperaturas, "23° / -1°");
        assert_eq!(p.dias[1].glifo, "󰖕");
    }

    #[test]
    fn una_hora_que_no_esta_no_inventa_la_temperatura() {
        let p = armar(&muestra(), "2026-10-20T10");
        assert_eq!(p.ahora, None);
        assert_eq!(p.dias.len(), 2);
    }

    #[test]
    fn sin_datos_o_rotos() {
        assert_eq!(armar(&Value::Null, "2026-10-07T01"), Pronostico::default());
        let mut roto = muestra();
        roto["daily"]["temperature_2m_min"] = Value::Null;
        assert!(armar(&roto, "2026-10-07T01").dias.is_empty());
    }

    #[test]
    fn resultados_del_geocoder() {
        let json = r#"{"results": [
            {"name": "Rosario", "admin1": "Provincia de Santa Fe", "country": "Argentina",
             "latitude": -32.94682, "longitude": -60.63932},
            {"name": "Sin región", "latitude": 1.0, "longitude": 2.0},
            {"name": "Sin coordenadas", "country": "X"}
        ], "generationtime_ms": 0.5}"#;
        let l = lugares(json);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].region, "Provincia de Santa Fe, Argentina");
        assert_eq!(l[0].longitud, -60.63932);
        assert_eq!(l[1].region, "");
        assert!(lugares(r#"{"generationtime_ms": 0.2}"#).is_empty());
        assert!(lugares("roto").is_empty());
    }

    #[test]
    fn codigos() {
        assert_eq!(codigo(0, true).1, "󰖨");
        assert_eq!(codigo(0, false).1, "󰖔");
        assert_eq!(codigo(1234, true), DESCONOCIDO);
    }
}
