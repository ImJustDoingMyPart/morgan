//! La fuente es la URL secreta en formato iCal de cada calendario (sin OAuth). Es una
//! credencial (quien la tenga lee el calendario entero), así que vive aparte de la config,
//! con permisos 600, y no sale de morgan: ni en los argumentos de un proceso (`ps` los
//! muestra a cualquiera), ni en el journal, ni en pantalla, ni en el portapapeles. El feed no
//! avisa cuando cambia: se relee cada 15 minutos.

use std::cell::RefCell;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, SystemTime};


const RELEER: Duration = Duration::from_secs(15 * 60);
/// Desde la medianoche de hoy: contando desde ahora, un evento de día entero desaparecería el
/// día en que ocurre.
const DIAS: i64 = 30;
const MAX_EVENTOS: usize = 25;

fn archivo_urls() -> PathBuf {
    glib::user_config_dir().join("morgan/calendars")
}

fn dir_cache() -> PathBuf {
    glib::user_cache_dir().join("morgan/calendarios")
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

/// Lo que se puede mostrar de una URL: el hash nombra su caché y la identifica para quitarla.
fn id(url: &str) -> String {
    glib::compute_checksum_for_string(glib::ChecksumType::Sha256, url)
        .map(|h| h[..16].to_owned())
        .unwrap_or_default()
}

/// Solo `https://` (`webcal://` es lo mismo), sin espacios, comillas ni barras invertidas: así
/// va entre comillas a la config de curl sin poder cerrarlas.
fn normalizar(texto: &str) -> Option<String> {
    let texto = texto.trim();
    let resto = texto
        .strip_prefix("https://")
        .or_else(|| texto.strip_prefix("webcal://"))?;
    if !resto
        .bytes()
        .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')
    {
        return None;
    }
    resto.split('/').next().filter(|d| d.contains('.'))?;
    Some(format!("https://{resto}"))
}

/// El dominio de una dirección de calendario pegada, o `None` si no lo es.
pub fn dominio(texto: &str) -> Option<String> {
    let url = normalizar(texto)?;
    url["https://".len()..].split('/').next().map(str::to_owned)
}

fn nombre(url: &str) -> String {
    if url.starts_with("file://") {
        return tr!("un archivo local").to_owned();
    }
    dominio(url).unwrap_or_else(|| tr!("una dirección inválida").to_owned())
}

/// Sin mostrar las URLs.
pub fn resumen_calendarios() -> Option<String> {
    match urls().as_slice() {
        [] => None,
        [u] => Some(nombre(u)),
        v => Some(trf!("{} calendarios", v.len())),
    }
}

pub struct Calendario {
    pub id: String,
    pub nombre: String,
    /// La última vez que se bajó.
    pub leido: Option<glib::DateTime>,
}

pub fn calendarios() -> Vec<Calendario> {
    urls()
        .iter()
        .map(|u| {
            let id = id(u);
            let leido = std::fs::metadata(dir_cache().join(format!("{id}.ics")))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .and_then(|d| glib::DateTime::from_unix_local(d.as_secs() as i64).ok());
            Calendario {
                id,
                nombre: nombre(u),
                leido,
            }
        })
        .collect()
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

    /// Se guarda solo si se pudo bajar y es un calendario: una dirección mal copiada no queda
    /// guardada sin que nadie se entere. Después se borra del portapapeles y de su historial.
    /// El error es para mostrar.
    pub async fn agregar(&self, texto: &str) -> Result<(), String> {
        let url = normalizar(texto)
            .ok_or_else(|| tr!("no es una dirección https:// ni webcal://").to_owned())?;
        let ics = bajar(&url).await?;
        let mut lista = std::fs::read_to_string(archivo_urls()).unwrap_or_default();
        if !urls().contains(&url) {
            if !lista.is_empty() && !lista.ends_with('\n') {
                lista.push('\n');
            }
            lista.push_str(&url);
            lista.push('\n');
            guardar(&archivo_urls(), &lista).map_err(|e| e.to_string())?;
        }
        guardar_cache(&url, &ics);
        let pegado = texto.trim().to_owned();
        let _ = gio::spawn_blocking(move || olvidar_del_portapapeles(&pegado)).await;
        self.refrescar();
        Ok(())
    }

    /// Con su caché: el feed es el calendario entero.
    pub fn quitar(&self, id_quitado: &str) -> std::io::Result<()> {
        let lista = std::fs::read_to_string(archivo_urls()).unwrap_or_default();
        let quedan: String = lista
            .lines()
            .filter(|l| id(l.trim()) != id_quitado)
            .flat_map(|l| [l, "\n"])
            .collect();
        guardar(&archivo_urls(), &quedan)?;
        limpiar_cache(&urls());
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
    limpiar_cache(&urls);
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

/// Si la red falla, la caché vieja: una agenda de hace una hora sirve más que ninguna. Un
/// `file://` (lo usa `dev/muestra`) se lee del disco, sin caché.
async fn feed(url: &str) -> Option<String> {
    if let Some(ruta) = url.strip_prefix("file://") {
        return std::fs::read_to_string(ruta).ok();
    }
    let cache = dir_cache().join(format!("{}.ics", id(url)));
    let fresca = std::fs::metadata(&cache)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|edad| edad < RELEER);
    if fresca && let Ok(t) = std::fs::read_to_string(&cache) {
        return Some(t);
    }
    // Una línea escrita a mano que no pasa el filtro no se baja.
    let bajado = match normalizar(url) {
        Some(u) if u == url => bajar(url).await,
        _ => Err(tr!("no es una dirección https://").to_owned()),
    };
    match bajado {
        Ok(t) => {
            guardar_cache(url, &t);
            Some(t)
        }
        Err(e) => {
            // El dominio y no la URL: el journal lo lee más gente que el archivo.
            eprintln!("morgan: agenda: {}: {e}", nombre(url));
            std::fs::read_to_string(&cache).ok()
        }
    }
}

/// El error es para mostrar.
async fn bajar(url: &str) -> Result<String, String> {
    let url = url.to_owned();
    let ics = gio::spawn_blocking(move || curl(&url))
        .await
        .map_err(|_| tr!("no se pudo bajar").to_owned())?
        .map_err(|codigo| motivo(codigo).to_owned())?;
    if !ics.contains("BEGIN:VCALENDAR") {
        return Err(tr!("lo que devuelve no es un calendario").to_owned());
    }
    Ok(ics)
}

/// La URL va por la entrada, en la config de curl, y no en los argumentos: los de cualquier
/// proceso se leen en `/proc` (`ps`). `-q` (primero) deja fuera `~/.curlrc`, que podría pedir,
/// por ejemplo, `--insecure`. Solo https, también en las redirecciones.
fn curl(url: &str) -> Result<String, Option<i32>> {
    let mut hijo = Command::new("curl")
        .args([
            "-q",
            "--fail",
            "--silent",
            "--location",
            "--max-time",
            "8",
            "--max-filesize",
            "20M",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--config",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| None)?;
    if let Some(mut entrada) = hijo.stdin.take() {
        entrada
            .write_all(format!("url = \"{url}\"\n").as_bytes())
            .map_err(|_| None)?;
    }
    let fin = hijo.wait_with_output().map_err(|_| None)?;
    if !fin.status.success() {
        return Err(fin.status.code());
    }
    Ok(String::from_utf8_lossy(&fin.stdout).into_owned())
}

/// Los códigos de salida de curl (`man curl`, "EXIT CODES").
fn motivo(codigo: Option<i32>) -> &'static str {
    match codigo {
        Some(6 | 7) => tr!("no hay conexión con el servidor"),
        Some(28) => tr!("el servidor tardó demasiado"),
        Some(22) => tr!("el servidor no la acepta (¿está bien copiada?)"),
        Some(35 | 60) => tr!("la conexión segura falló"),
        Some(63) => tr!("el calendario pesa más de 20 MB"),
        Some(1) => tr!("redirige a una dirección que no es https"),
        None => tr!("falta curl"),
        _ => tr!("no se pudo bajar"),
    }
}

/// La carpeta es 700: los archivos ya son 600, pero sus nombres dicen cuántos calendarios hay.
fn guardar_cache(url: &str, ics: &str) {
    let dir = dir_cache();
    let r = std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .and_then(|()| std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)))
        .and_then(|()| guardar(&dir.join(format!("{}.ics", id(url))), ics));
    if let Err(e) = r {
        eprintln!("morgan: agenda: {e}");
    }
}

/// La de un calendario que ya no está se borra: es el calendario entero.
fn limpiar_cache(urls: &[String]) {
    let vigentes: Vec<String> = urls.iter().map(|u| format!("{}.ics", id(u))).collect();
    let Ok(dir) = std::fs::read_dir(dir_cache()) else {
        return;
    };
    // Una carpeta de antes se cierra ya, sin esperar a la próxima bajada.
    let _ = std::fs::set_permissions(dir_cache(), std::fs::Permissions::from_mode(0o700));
    for f in dir.flatten() {
        let nombre = f.file_name().to_string_lossy().into_owned();
        if !vigentes.contains(&nombre) {
            let _ = std::fs::remove_file(f.path());
        }
    }
}

/// Solo legible por el usuario: el feed es el calendario entero, y la lista de URLs,
/// credenciales. El temporal se crea de cero: uno que ya existía conservaría sus permisos.
fn guardar(ruta: &Path, texto: &str) -> std::io::Result<()> {
    if let Some(d) = ruta.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = ruta.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(texto.as_bytes())?;
    std::fs::rename(&tmp, ruta)
}

/// Lo pegado sale del historial de cliphist (si lo hay) y del portapapeles y la selección
/// primaria si todavía lo tienen. Todo por la entrada de cada comando, nunca en los argumentos.
fn olvidar_del_portapapeles(pegado: &str) {
    // El ancho alcanza para que la vista previa sea el texto entero.
    if let Ok(lista) = salida(&["cliphist", "-preview-width", "100000", "list"], None) {
        for linea in lineas_con(&lista, pegado) {
            let _ = salida(&["cliphist", "delete"], Some(&format!("{linea}\n")));
        }
    }
    for primaria in [false, true] {
        let pegar: &[&str] = if primaria {
            &["wl-paste", "--primary", "--no-newline"]
        } else {
            &["wl-paste", "--no-newline"]
        };
        if salida(pegar, None).is_ok_and(|t| t.trim() == pegado) {
            let borrar: &[&str] = if primaria {
                &["wl-copy", "--primary", "--clear"]
            } else {
                &["wl-copy", "--clear"]
            };
            let _ = salida(borrar, None);
        }
    }
}

/// Las de `cliphist list` (`id<TAB>vista previa`) cuyo texto es el pegado.
fn lineas_con<'a>(lista: &'a str, pegado: &str) -> Vec<&'a str> {
    lista
        .lines()
        .filter(|l| l.split_once('\t').is_some_and(|(_, p)| p.trim() == pegado))
        .collect()
}

fn salida(args: &[&str], entrada: Option<&str>) -> Result<String, ()> {
    let mut hijo = Command::new(args[0])
        .args(&args[1..])
        .stdin(if entrada.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ())?;
    if let (Some(t), Some(mut e)) = (entrada, hijo.stdin.take()) {
        e.write_all(t.as_bytes()).map_err(|_| ())?;
    }
    let fin = hijo.wait_with_output().map_err(|_| ())?;
    if !fin.status.success() {
        return Err(());
    }
    Ok(String::from_utf8_lossy(&fin.stdout).into_owned())
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

    #[test]
    fn solo_https_y_sin_poder_cerrar_las_comillas() {
        let g = "https://calendar.google.com/calendar/ical/x%40gmail.com/private-abc/basic.ics";
        assert_eq!(normalizar(&format!("  {g}\n")).as_deref(), Some(g));
        assert_eq!(
            normalizar("webcal://p01-caldav.icloud.com/published/2/abc").as_deref(),
            Some("https://p01-caldav.icloud.com/published/2/abc")
        );
        for malo in [
            "http://calendar.google.com/a.ics",
            "file:///etc/passwd",
            "https://localhost/a.ics",
            "https://a.com/x\"\nurl = \"https://b.com/",
            "https://a.com/x y",
            "https://a.com/x\\y",
            "https://a.com/ñ",
            "calendar.google.com/a.ics",
        ] {
            assert_eq!(normalizar(malo), None, "{malo}");
        }
        assert_eq!(dominio(g).as_deref(), Some("calendar.google.com"));
    }

    #[test]
    fn del_historial_sale_solo_lo_pegado() {
        let lista = "12\thttps://a.com/secreta\n11\thttps://a.com/secreta-otra\n10\thola\n9\thttps://a.com/secreta \n";
        assert_eq!(
            lineas_con(lista, "https://a.com/secreta"),
            ["12\thttps://a.com/secreta", "9\thttps://a.com/secreta "]
        );
    }
}
