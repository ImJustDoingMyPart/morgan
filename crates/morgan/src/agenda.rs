//! La fuente es la URL secreta en formato iCal de cada calendario (sin OAuth). Es una
//! credencial (quien la tenga lee el calendario entero), así que vive aparte de la config,
//! con permisos 600. El feed no avisa cuando cambia: se relee cada 15 minutos.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use gtk::{gio, glib};

use crate::comando;

const RELEER: Duration = Duration::from_secs(15 * 60);
/// Desde la medianoche de hoy: contando desde ahora, un evento de día entero desaparecería el
/// día en que ocurre.
const DIAS: i64 = 30;
const MAX_EVENTOS: usize = 25;

pub fn archivo_urls() -> PathBuf {
    glib::user_config_dir().join("morgan/calendarios")
}

fn urls() -> Vec<String> {
    std::fs::read_to_string(archivo_urls())
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Sin mostrar las URLs.
pub fn resumen_calendarios() -> Option<String> {
    match urls().as_slice() {
        [] => None,
        [u] => Some(
            u.split("://")
                .nth(1)
                .and_then(|r| r.split('/').next())
                .unwrap_or("un calendario")
                .to_owned(),
        ),
        v => Some(format!("{} calendarios", v.len())),
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Evento {
    pub inicio: String,
    pub dia_entero: bool,
    pub resumen: String,
    pub lugar: String,
}

fn fecha(iso: &str, zona: &glib::TimeZone) -> Option<glib::DateTime> {
    glib::DateTime::from_iso8601(iso, None)
        .ok()
        .and_then(|d| d.to_timezone(zona).ok())
}

impl Evento {
    pub fn comienzo(&self, zona: &glib::TimeZone) -> Option<glib::DateTime> {
        fecha(&self.inicio, zona)
    }
}

#[derive(Clone)]
pub struct Agenda {
    inner: Rc<Inner>,
}

struct Inner {
    eventos: RefCell<Vec<Evento>>,
    oyentes: RefCell<Vec<Rc<dyn Fn()>>>,
}

impl Agenda {
    pub fn new() -> Self {
        let a = Agenda {
            inner: Rc::new(Inner {
                eventos: RefCell::default(),
                oyentes: RefCell::default(),
            }),
        };
        a.refrescar();
        let yo = a.clone();
        glib::timeout_add_local(RELEER, move || {
            yo.refrescar();
            glib::ControlFlow::Continue
        });
        a
    }

    pub fn al_cambiar(&self, f: impl Fn() + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    pub fn eventos(&self) -> Vec<Evento> {
        self.inner.eventos.borrow().clone()
    }

    pub fn refrescar(&self) {
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            // Sin nada legible (sin red y sin caché), queda lo que había.
            let Some(evs) = leer().await else { return };
            if *yo.inner.eventos.borrow() != evs {
                yo.inner.eventos.replace(evs);
                yo.avisar();
            }
        });
    }

    /// `webcal://` es `https://` para bajarla.
    pub fn poner_url(&self, url: &str) -> std::io::Result<()> {
        let url = url
            .trim()
            .strip_prefix("webcal://")
            .map_or_else(|| url.trim().to_owned(), |r| format!("https://{r}"));
        guardar(&archivo_urls(), &format!("{url}\n"))?;
        self.refrescar();
        Ok(())
    }

    fn avisar(&self) {
        let oyentes = self.inner.oyentes.borrow().clone();
        for f in oyentes {
            f();
        }
    }
}

/// `None` si no se pudo leer ninguno, para no vaciar la agenda por un corte de red.
async fn leer() -> Option<Vec<Evento>> {
    let urls = urls();
    if urls.is_empty() {
        return Some(Vec::new());
    }
    let mut feeds = Vec::new();
    for url in &urls {
        if let Some(ics) = feed(url).await {
            feeds.push(ics);
        }
    }
    if feeds.is_empty() {
        return None;
    }
    gio::spawn_blocking(move || {
        let ahora = chrono::Local::now();
        let desde = ahora
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|n| chrono::TimeZone::from_local_datetime(&chrono::Local, &n).earliest())
            .unwrap_or(ahora);
        let hasta = ahora + chrono::Duration::days(DIAS);
        let mut todos: Vec<Evento> = feeds
            .iter()
            .flat_map(|ics| crate::ical::eventos(ics, desde, hasta))
            .collect();
        todos.sort_by_key(|e| chrono::DateTime::parse_from_rfc3339(&e.inicio).ok());
        todos.truncate(MAX_EVENTOS);
        todos
    })
    .await
    .ok()
}

/// Si la red falla, la caché vieja: una agenda de hace una hora sirve más que ninguna.
async fn feed(url: &str) -> Option<String> {
    let id = glib::compute_checksum_for_string(glib::ChecksumType::Sha256, url)?;
    let cache = glib::user_cache_dir()
        .join("morgan/calendarios")
        .join(format!("{}.ics", &id[..16]));
    let fresca = std::fs::metadata(&cache)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|edad| edad < RELEER);
    if fresca && let Ok(t) = std::fs::read_to_string(&cache) {
        return Some(t);
    }
    // --max-time: mejor la caché vieja que la agenda esperando a la red.
    match comando::salida(&["curl", "-fsSL", "--max-time", "8", url]).await {
        Ok(t) if t.contains("BEGIN:VCALENDAR") => {
            if let Err(e) = guardar(&cache, &t) {
                eprintln!("morgan: agenda: {e}");
            }
            Some(t)
        }
        r => {
            if let Err(e) = r {
                eprintln!("morgan: agenda: {e}");
            }
            std::fs::read_to_string(&cache).ok()
        }
    }
}

/// Solo legible por el usuario: el feed es el calendario entero, y la lista de URLs,
/// credenciales.
pub fn guardar(ruta: &std::path::Path, texto: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(d) = ruta.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = ruta.with_extension("tmp");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(texto.as_bytes())?;
    std::fs::rename(&tmp, ruta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_inicio_se_lee_en_la_zona_pedida() {
        let e = Evento {
            inicio: "2026-10-17T00:00:00-03:00".into(),
            dia_entero: true,
            resumen: "Cumplemes".into(),
            lugar: String::new(),
        };
        let zona = glib::TimeZone::from_offset(-3 * 3600);
        assert_eq!(e.comienzo(&zona).map(|d| d.day_of_month()), Some(17));
        let utc = glib::TimeZone::utc();
        assert_eq!(e.comienzo(&utc).map(|d| d.hour()), Some(3));
    }
}
