//! Solo fuentes documentadas: el payload de la status line de Claude Code y el de Antigravity
//! CLI (los guarda `dist/morgan-cuota`) y el `rate_limits` que Codex escribe en sus sesiones.
//!
//! - Un dato que falta no es un cero: `restante: None` se pinta «—».
//! - Cada lectura dice cuándo la observó el agente, no cuándo la leyó morgan.
//! - Una ventana que pasó su renovación queda pendiente: no se llena sola.
//! - Las ventanas no se suman.

use std::path::{Path, PathBuf};

use serde_json::Value;

pub const UMBRAL: f64 = 0.2;

const VIEJO: i64 = 3600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agente {
    Claude,
    Codex,
    Gemini,
}

pub const AGENTES: [Agente; 3] = [Agente::Claude, Agente::Codex, Agente::Gemini];

impl Agente {
    pub fn nombre(self) -> &'static str {
        match self {
            Agente::Claude => "Claude Code",
            Agente::Codex => "Codex",
            Agente::Gemini => "Gemini",
        }
    }

    /// Gemini no tiene logo en la Nerd Font: va su estrella de cuatro puntas.
    pub fn glifo(self) -> &'static str {
        match self {
            Agente::Claude => "\u{ec82}",
            Agente::Codex => "\u{ec81}",
            Agente::Gemini => "\u{f0ae2}",
        }
    }

    pub fn corto(self) -> &'static str {
        match self {
            Agente::Claude => "Claude",
            Agente::Codex => "Codex",
            Agente::Gemini => "Gemini",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ventana {
    pub grupo: Option<String>,
    pub minutos: Option<u32>,
    pub clave: String,
    pub restante: Option<f64>,
    pub renueva: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lectura {
    pub observada: Option<i64>,
    pub ventanas: Vec<Ventana>,
}


fn porcentaje_usado(v: &Value) -> Option<f64> {
    let p = v.as_f64()?;
    (p.is_finite() && (0.0..=100.0).contains(&p)).then(|| 1.0 - p / 100.0)
}

fn fraccion(v: &Value) -> Option<f64> {
    let f = v.as_f64()?;
    (f.is_finite() && (0.0..=1.0).contains(&f)).then_some(f)
}

pub fn claude(cuota: &Value) -> Vec<Ventana> {
    let mut v = Vec::new();
    for (clave, minutos) in [("five_hour", 300), ("seven_day", 10080)] {
        let Some(w) = cuota.get(clave).filter(|w| w.is_object()) else {
            continue;
        };
        v.push(Ventana {
            grupo: None,
            minutos: Some(minutos),
            clave: clave.to_owned(),
            restante: w.get("used_percentage").and_then(porcentaje_usado),
            renueva: w.get("resets_at").and_then(Value::as_i64),
        });
    }
    v
}

/// Claves `<grupo>-<ventana>` (`gemini-5h`, `3p-weekly`…); `reset_in_seconds` cuenta desde
/// `observada`.
pub fn gemini(cuota: &Value, observada: i64) -> Vec<Ventana> {
    let Some(mapa) = cuota.as_object() else {
        return Vec::new();
    };
    let mut v: Vec<Ventana> = mapa
        .iter()
        .map(|(clave, w)| {
            let (grupo, ventana) = clave.rsplit_once('-').unwrap_or(("", clave));
            let grupo = match grupo {
                "gemini" => None,
                "3p" => Some("otros modelos".to_owned()),
                otro => Some(otro.to_owned()),
            };
            let minutos = match ventana {
                "5h" => Some(300),
                "weekly" => Some(10080),
                _ => None,
            };
            Ventana {
                grupo,
                minutos,
                clave: clave.clone(),
                restante: w.get("remaining_fraction").and_then(fraccion),
                renueva: w
                    .get("reset_in_seconds")
                    .and_then(Value::as_i64)
                    .map(|s| observada + s),
            }
        })
        .collect();
    ordenar(&mut v);
    v
}

/// `secondary` es `null` en el plan gratis, que tiene una sola ventana de 30 días.
pub fn codex(cuota: &Value) -> Vec<Ventana> {
    let mut v: Vec<Ventana> = ["primary", "secondary"]
        .into_iter()
        .filter_map(|clave| {
            let w = cuota.get(clave).filter(|w| w.is_object())?;
            Some(Ventana {
                grupo: None,
                minutos: w
                    .get("window_minutes")
                    .and_then(Value::as_u64)
                    .and_then(|m| u32::try_from(m).ok()),
                clave: clave.to_owned(),
                restante: w.get("used_percent").and_then(porcentaje_usado),
                renueva: w.get("resets_at").and_then(Value::as_i64),
            })
        })
        .collect();
    ordenar(&mut v);
    v
}

fn ordenar(v: &mut [Ventana]) {
    v.sort_by(|a, b| {
        (
            a.grupo.is_some(),
            &a.grupo,
            a.minutos.unwrap_or(u32::MAX),
            &a.clave,
        )
            .cmp(&(
                b.grupo.is_some(),
                &b.grupo,
                b.minutos.unwrap_or(u32::MAX),
                &b.clave,
            ))
    });
}

fn guardada(ruta: &Path) -> Option<(i64, Value)> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(ruta).ok()?).ok()?;
    Some((v.get("observada")?.as_i64()?, v.get("cuota")?.clone()))
}

fn carpeta_guardadas() -> PathBuf {
    glib::user_cache_dir().join("morgan/cuotas")
}

/// Lee solo la cola de cada archivo: el evento se reescribe en cada turno.
fn ultima_de_codex() -> Option<(i64, Value)> {
    let mut archivos = Vec::new();
    juntar_jsonl(
        &glib::home_dir().join(".codex/sessions"),
        &mut archivos,
    );
    archivos.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    let (mas_reciente_t, mas_reciente_ruta) = archivos.first()?;

    type CacheCodex = Option<(PathBuf, std::time::SystemTime, Option<(i64, Value)>)>;
    thread_local! {
        static CACHE: std::cell::RefCell<CacheCodex> =
            const { std::cell::RefCell::new(None) };
    }
    if let Some((ruta, t, res)) = CACHE.with(|c| c.borrow().clone())
        && &ruta == mas_reciente_ruta
        && &t == mas_reciente_t
    {
        return res;
    }

    let resultado = archivos
        .iter()
        .take(5)
        .find_map(|(_, ruta)| ultima_linea_con_cuota(&cola(ruta)?));
    CACHE.with(|c| {
        *c.borrow_mut() = Some((mas_reciente_ruta.clone(), *mas_reciente_t, resultado.clone()));
    });
    resultado
}

fn juntar_jsonl(dir: &Path, salida: &mut Vec<(std::time::SystemTime, PathBuf)>) {
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return;
    };
    let mut subdirs = Vec::new();
    for e in entradas.flatten() {
        let ruta = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            subdirs.push(ruta);
        } else if ruta.extension().is_some_and(|x| x == "jsonl")
            && let Ok(t) = meta.modified()
        {
            salida.push((t, ruta));
        }
    }
    subdirs.sort_by(|a, b| b.cmp(a));
    for d in subdirs {
        juntar_jsonl(&d, salida);
    }
}

fn cola(ruta: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    const TOPE: u64 = 512 * 1024;
    let mut f = std::fs::File::open(ruta).ok()?;
    let largo = f.metadata().ok()?.len();
    let desde = largo.saturating_sub(TOPE);
    f.seek(SeekFrom::Start(desde)).ok()?;
    let a_leer = usize::try_from(largo - desde).unwrap_or(0);
    let mut bytes = Vec::with_capacity(a_leer);
    f.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn ultima_linea_con_cuota(texto: &str) -> Option<(i64, Value)> {
    texto.lines().rev().find_map(|l| {
        if !l.contains("\"rate_limits\"") {
            return None;
        }
        let v: Value = serde_json::from_str(l).ok()?;
        let cuota = v.get("payload")?.get("rate_limits")?;
        if !cuota.is_object() {
            return None;
        }
        let cuando = glib::DateTime::from_iso8601(v.get("timestamp")?.as_str()?, None)
            .ok()?
            .to_unix();
        Some((cuando, cuota.clone()))
    })
}

pub fn leer(agente: Agente) -> Lectura {
    let datos = match agente {
        Agente::Claude => guardada(&carpeta_guardadas().join("claude.json")),
        Agente::Gemini => guardada(&carpeta_guardadas().join("gemini.json")),
        Agente::Codex => ultima_de_codex(),
    };
    let Some((observada, cuota)) = datos else {
        return Lectura::default();
    };
    let ventanas = match agente {
        Agente::Claude => claude(&cuota),
        Agente::Gemini => gemini(&cuota, observada),
        Agente::Codex => codex(&cuota),
    };
    Lectura {
        observada: Some(observada),
        ventanas,
    }
}

pub fn ahora() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}


impl Ventana {
    pub fn pendiente(&self, ahora: i64) -> bool {
        self.renueva.is_some_and(|r| r <= ahora)
    }

    pub fn vigente(&self, ahora: i64) -> Option<f64> {
        self.restante.filter(|_| !self.pendiente(ahora))
    }

    pub fn en_alerta(&self, ahora: i64) -> bool {
        self.vigente(ahora).is_some_and(|r| r < UMBRAL)
    }
}

/// Siempre la más corta del grupo principal: una más larga bajo el umbral no la reemplaza,
/// se ve en rojo al desplegar.
pub fn principal(l: &Lectura) -> Option<usize> {
    l.ventanas.iter().position(|v| v.grupo.is_none())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pintura {
    pub glifo: &'static str,
    pub nombre: String,
    pub renueva: String,
    pub porcentaje: String,
    pub fraccion: f64,
    pub alerta: bool,
    pub apagada: bool,
}

pub fn glifo(minutos: Option<u32>) -> &'static str {
    match minutos {
        Some(m) if m <= 24 * 60 => "󰔛",
        Some(10080) => "󰨳",
        Some(m) if m > 10080 => "󰸗",
        _ => "󰃭",
    }
}

pub fn nombre_ventana(v: &Ventana) -> String {
    let largo = match v.minutos {
        Some(10080) => "semana".to_owned(),
        Some(m) if m % 1440 == 0 => format!("{} d", m / 1440),
        Some(m) if m % 60 == 0 => format!("{} h", m / 60),
        Some(m) => format!("{m} min"),
        None => v.clave.clone(),
    };
    match &v.grupo {
        Some(g) => format!("{g} · {largo}"),
        None => largo,
    }
}

pub fn lapso(segundos: i64) -> String {
    let m = (segundos.max(0) + 59) / 60;
    match m {
        m if m < 60 => format!("{} min", m.max(1)),
        m if m < 24 * 60 => format!("{} h {:02}", m / 60, m % 60),
        m => format!("{} d {} h", m / 1440, m % 1440 / 60),
    }
}

fn antiguedad(segundos: i64) -> String {
    match segundos.max(0) / 60 {
        m if m < 60 => format!("{m} min"),
        m if m < 24 * 60 => format!("{} h", m / 60),
        m => format!("{} d", m / 1440),
    }
}

pub fn pintar(v: &Ventana, agente: Option<(Agente, Option<i64>)>, ahora: i64) -> Pintura {
    let pendiente = v.pendiente(ahora);
    let vigente = v.vigente(ahora);
    let mut nombre = nombre_ventana(v);
    let mut glifo = glifo(v.minutos);
    if let Some((a, observada)) = agente {
        glifo = a.glifo();
        nombre = format!("{} · {nombre}", a.nombre());
        if let Some(o) = observada.filter(|o| ahora - o >= VIEJO) {
            nombre = format!("{nombre} · hace {}", antiguedad(ahora - o));
        }
    }
    let renueva = match (pendiente, v.renueva, vigente) {
        (true, _, _) => "renovada".to_owned(),
        // Llena no hay nada que renovar (Antigravity tampoco lo dice).
        (_, _, Some(r)) if r >= 1.0 => String::new(),
        (_, Some(r), _) => format!("↻ {}", lapso(r - ahora)),
        _ => String::new(),
    };
    Pintura {
        glifo,
        nombre,
        renueva,
        porcentaje: vigente.map_or("—".to_owned(), |r| format!("{:.0}%", r * 100.0)),
        fraccion: vigente.or(v.restante).unwrap_or(0.0),
        alerta: v.en_alerta(ahora),
        apagada: vigente.is_none(),
    }
}

pub fn sin_datos(a: Agente) -> Pintura {
    Pintura {
        glifo: a.glifo(),
        nombre: a.nombre().to_owned(),
        renueva: "sin datos".to_owned(),
        porcentaje: "—".to_owned(),
        fraccion: 0.0,
        alerta: false,
        apagada: true,
    }
}

/// La observación más reciente es la del agente que se está usando: cada fuente se
/// actualiza solo mientras se usa su agente.
pub fn resumen(lecturas: &[(Agente, Lectura)], ahora: i64) -> (String, bool) {
    let ultimo = lecturas
        .iter()
        .filter_map(|(a, l)| Some((*a, l.observada?, &l.ventanas[principal(l)?])))
        .max_by_key(|(_, observada, _)| *observada);
    match ultimo {
        Some((a, _, v)) => {
            let p = pintar(v, None, ahora);
            (format!("{}: {}", a.corto(), p.porcentaje), p.alerta)
        }
        None => ("Sin datos".to_owned(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const AHORA: i64 = 1_791_310_000;

    #[test]
    fn claude_lee_las_dos_ventanas_como_lo_que_queda() {
        let v = claude(&json!({
            "five_hour": {"used_percentage": 38, "resets_at": AHORA + 600},
            "seven_day": {"used_percentage": 9.4, "resets_at": AHORA + 86400},
        }));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].minutos, Some(300));
        assert!((v[0].restante.unwrap() - 0.62).abs() < 1e-9);
        assert_eq!(v[1].renueva, Some(AHORA + 86400));
    }

    #[test]
    fn un_porcentaje_que_falta_o_no_cierra_no_es_un_cero() {
        let v = claude(&json!({
            "five_hour": {"resets_at": AHORA},
            "seven_day": {"used_percentage": 140},
        }));
        assert_eq!(v[0].restante, None);
        assert_eq!(v[1].restante, None);
    }

    #[test]
    fn gemini_separa_los_grupos_y_ordena() {
        // Un payload real.
        let v = gemini(
            &json!({
                "3p-5h": {"remaining_fraction": 1, "reset_in_seconds": 17996},
                "3p-weekly": {"remaining_fraction": 1, "reset_in_seconds": 604796},
                "gemini-5h": {"remaining_fraction": 0.9781684, "reset_in_seconds": 11618},
                "gemini-weekly": {"remaining_fraction": 0.7975732, "reset_in_seconds": 66638},
            }),
            AHORA,
        );
        let claves: Vec<_> = v.iter().map(|w| w.clave.as_str()).collect();
        assert_eq!(claves, ["gemini-5h", "gemini-weekly", "3p-5h", "3p-weekly"]);
        assert_eq!(v[0].renueva, Some(AHORA + 11618));
        assert_eq!(nombre_ventana(&v[3]), "otros modelos · semana");
    }

    #[test]
    fn gemini_nombra_tal_cual_una_ventana_que_no_conoce() {
        let v = gemini(&json!({"gemini-daily": {"remaining_fraction": 0.5}}), AHORA);
        assert_eq!(v[0].minutos, None);
        assert_eq!(nombre_ventana(&v[0]), "gemini-daily");
    }

    #[test]
    fn codex_gratis_tiene_una_sola_ventana_de_30_dias() {
        // Un `rate_limits` real, del plan gratis.
        let l = r#"{"timestamp":"2026-10-06T18:12:12.373Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":41.0,"window_minutes":43200,"resets_at":1792268475},"secondary":null}}}"#;
        let otra = r#"{"timestamp":"2026-10-06T18:12:13Z","type":"event_msg","payload":{"type":"token_count","rate_limits":null}}"#;
        let (cuando, cuota) = ultima_linea_con_cuota(&format!("{l}\n{otra}\n")).unwrap();
        assert_eq!(cuando, 1_791_310_332);
        let v = codex(&cuota);
        assert_eq!(v.len(), 1);
        assert_eq!(nombre_ventana(&v[0]), "30 d");
        assert_eq!(glifo(v[0].minutos), "󰸗");
        assert!((v[0].restante.unwrap() - 0.59).abs() < 1e-9);
    }

    fn ventana(minutos: u32, restante: f64, renueva: i64) -> Ventana {
        Ventana {
            grupo: None,
            minutos: Some(minutos),
            clave: String::new(),
            restante: Some(restante),
            renueva: Some(renueva),
        }
    }

    #[test]
    fn la_fila_es_la_de_5_h_aunque_la_semanal_este_bajo_el_umbral() {
        let l = Lectura {
            observada: Some(AHORA),
            ventanas: vec![
                ventana(300, 0.9, AHORA + 60),
                ventana(10080, 0.1, AHORA + 60),
            ],
        };
        assert_eq!(principal(&l), Some(0));
        // Desplegada, la semanal se ve en alerta.
        assert!(pintar(&l.ventanas[1], None, AHORA).alerta);
    }

    #[test]
    fn otro_grupo_nunca_es_la_fila() {
        let mut otro = ventana(300, 0.5, AHORA + 60);
        otro.grupo = Some("otros modelos".into());
        let l = Lectura {
            observada: Some(AHORA),
            ventanas: vec![otro],
        };
        assert_eq!(principal(&l), None);
    }

    #[test]
    fn la_fila_del_agente_lleva_su_logo_y_las_desplegadas_el_largo() {
        let v = ventana(10080, 0.5, AHORA + 60);
        let fila = pintar(&v, Some((Agente::Claude, Some(AHORA))), AHORA);
        assert_eq!(fila.glifo, "\u{ec82}");
        assert_eq!(pintar(&v, None, AHORA).glifo, "󰨳");
    }

    #[test]
    fn pasada_la_renovacion_queda_pendiente_y_no_en_alerta() {
        let v = ventana(300, 0.05, AHORA - 1);
        let p = pintar(&v, None, AHORA);
        assert_eq!(p.porcentaje, "—");
        assert_eq!(p.renueva, "renovada");
        assert!(p.apagada && !p.alerta);
    }

    #[test]
    fn la_fila_del_agente_dice_de_cuando_es_un_dato_viejo() {
        let v = ventana(300, 0.62, AHORA + 3 * 3600 + 18 * 60);
        let fresca = pintar(&v, Some((Agente::Claude, Some(AHORA - 120))), AHORA);
        assert_eq!(fresca.nombre, "Claude Code · 5 h");
        assert_eq!(fresca.renueva, "↻ 3 h 18");
        assert_eq!(fresca.porcentaje, "62%");
        let vieja = pintar(&v, Some((Agente::Gemini, Some(AHORA - 2 * 86400))), AHORA);
        assert_eq!(vieja.nombre, "Gemini · 5 h · hace 2 d");
    }

    #[test]
    fn llena_no_dice_cuando_se_renueva() {
        assert_eq!(
            pintar(&ventana(300, 1.0, AHORA + 60), None, AHORA).renueva,
            ""
        );
    }

    #[test]
    fn lapsos() {
        assert_eq!(lapso(30), "1 min");
        assert_eq!(lapso(12 * 60), "12 min");
        assert_eq!(lapso(2 * 3600 + 51 * 60), "2 h 51");
        assert_eq!(lapso(6 * 86400 + 2 * 3600), "6 d 2 h");
    }

    #[test]
    fn la_tarjeta_nombra_el_que_se_uso_ultimo() {
        let l = |r, observada| Lectura {
            observada: Some(observada),
            ventanas: vec![ventana(300, r, AHORA + 60)],
        };
        let lecturas = [
            (Agente::Claude, l(0.65, AHORA - 60)),
            (Agente::Codex, l(0.59, AHORA - 86400)),
            (Agente::Gemini, l(0.98, AHORA - 3600)),
        ];
        assert_eq!(resumen(&lecturas, AHORA), ("Claude: 65%".to_owned(), false));
        let lecturas = [
            (Agente::Claude, l(0.65, AHORA - 60)),
            (Agente::Gemini, l(0.15, AHORA - 10)),
        ];
        assert_eq!(resumen(&lecturas, AHORA), ("Gemini: 15%".to_owned(), true));
        assert_eq!(
            resumen(&[(Agente::Codex, Lectura::default())], AHORA),
            ("Sin datos".to_owned(), false)
        );
    }
}
