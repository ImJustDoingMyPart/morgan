//! Muestra el mismo reproductor al que van las teclas (`reproductor::elegir`). Se sigue por
//! señales solo mientras el centro está abierto.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use futures_lite::StreamExt;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream};

use crate::reproductor::{self, Estado as Reproduciendo};

const PREFIJO: &str = "org.mpris.MediaPlayer2.";
const RUTA: &str = "/org/mpris/MediaPlayer2";
const INTERFAZ: &str = "org.mpris.MediaPlayer2.Player";
const PLAYERCTLD: &str = "org.mpris.MediaPlayer2.playerctld";

#[derive(Debug, Clone, PartialEq)]
pub struct Reproduccion {
    pub bus: String,
    pub titulo: String,
    pub subtitulo: String,
    pub caratula: Option<PathBuf>,
    pub estado: Option<Reproduciendo>,
    pub puede_anterior: bool,
    pub puede_siguiente: bool,
    pub mezcla: Option<bool>,
    pub repetir: Option<String>,
}

pub fn siguiente_repeticion(actual: &str) -> &'static str {
    match actual {
        "None" => "Playlist",
        "Playlist" => "Track",
        _ => "None",
    }
}

/// Sin título ni artista, el reproductor no se muestra.
pub fn textos(metadata: &HashMap<String, OwnedValue>) -> Option<(String, String)> {
    let texto = |k: &str| {
        metadata
            .get(k)
            .and_then(|v| <&str>::try_from(&**v).ok())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    let primero = |k: &str| {
        metadata
            .get(k)
            .and_then(|v| <Vec<String>>::try_from(Value::from(v.try_clone().ok()?)).ok())
            .and_then(|v| {
                v.into_iter()
                    .map(|a| a.trim().to_owned())
                    .find(|a| !a.is_empty())
            })
    };
    let titulo = texto("xesam:title");
    let artista = primero("xesam:artist").or_else(|| primero("xesam:albumArtist"));
    let subtitulo = match (&artista, texto("xesam:album")) {
        (Some(a), Some(al)) => format!("{a} - {al}"),
        (None, Some(al)) => al,
        (Some(a), None) => a.clone(),
        (None, None) => String::new(),
    };
    match titulo {
        Some(t) => Some((t, subtitulo)),
        None => artista.map(|a| (a, String::new())),
    }
}

pub fn caratula(metadata: &HashMap<String, OwnedValue>) -> Option<PathBuf> {
    let url = metadata
        .get("mpris:artUrl")
        .and_then(|v| <&str>::try_from(&**v).ok())?;
    let ruta = url.strip_prefix("file://")?;
    let ruta = glib::uri_unescape_string(ruta, None::<&str>)?;
    Some(PathBuf::from(ruta.as_str()))
}

#[derive(Clone, Default)]
pub struct Mpris {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    actual: RefCell<Option<Reproduccion>>,
    conexion: RefCell<Option<Connection>>,
    oyentes: RefCell<Vec<Rc<dyn Fn()>>>,
    vigia: RefCell<Option<glib::JoinHandle<()>>>,
}

impl Mpris {
    pub fn actual(&self) -> Option<Reproduccion> {
        self.inner.actual.borrow().clone()
    }

    pub fn al_cambiar(&self, f: impl Fn() + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    pub fn abrir(&self) {
        let yo = self.clone();
        let h = glib::MainContext::default().spawn_local(async move {
            let conn = match yo.conexion().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("morgan: reproductor: {e}");
                    return;
                }
            };
            // Las propiedades de cualquier reproductor y los que aparecen o se van.
            let props = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .path(RUTA)
                .and_then(|b| b.interface("org.freedesktop.DBus.Properties"))
                .map(|b| b.build());
            let duenos = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.DBus")
                .and_then(|b| b.member("NameOwnerChanged"))
                .and_then(|b| b.arg0ns("org.mpris.MediaPlayer2"))
                .map(|b| b.build());
            let (Ok(props), Ok(duenos)) = (props, duenos) else {
                return;
            };
            let (Ok(a), Ok(b)) = (
                MessageStream::for_match_rule(props, &conn, Some(64)).await,
                MessageStream::for_match_rule(duenos, &conn, Some(64)).await,
            ) else {
                return;
            };
            let mut senales = a.or(b);
            yo.releer(&conn).await;
            while senales.next().await.is_some() {
                glib::timeout_future(std::time::Duration::from_millis(30)).await;
                while let Some(Some(_)) = futures_lite::future::poll_once(senales.next()).await {}
                yo.releer(&conn).await;
            }
        });
        if let Some(viejo) = self.inner.vigia.replace(Some(h)) {
            viejo.abort();
        }
    }

    pub fn cerrar(&self) {
        if let Some(h) = self.inner.vigia.take() {
            h.abort();
        }
    }

    pub fn alternar_mezcla(&self) {
        if let Some(m) = self.actual().and_then(|r| r.mezcla) {
            self.poner("Shuffle", Value::from(!m));
        }
    }

    pub fn ciclar_repeticion(&self) {
        if let Some(r) = self.actual().and_then(|r| r.repetir) {
            self.poner("LoopStatus", Value::from(siguiente_repeticion(&r)));
        }
    }

    fn poner(&self, propiedad: &'static str, valor: Value<'static>) {
        let Some(r) = self.actual() else {
            return;
        };
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let Ok(conn) = yo.conexion().await else {
                return;
            };
            let r = match zbus::Proxy::new(&conn, r.bus.as_str(), RUTA, INTERFAZ).await {
                Ok(p) => p
                    .set_property(propiedad, valor)
                    .await
                    .map_err(zbus::Error::from),
                Err(e) => Err(e),
            };
            if let Err(e) = r {
                eprintln!("morgan: reproductor: {propiedad}: {e}");
            }
        });
    }

    pub fn pedir(&self, metodo: &'static str) {
        let Some(r) = self.actual() else {
            return;
        };
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let Ok(conn) = yo.conexion().await else {
                return;
            };
            match zbus::Proxy::new(&conn, r.bus.as_str(), RUTA, INTERFAZ).await {
                Ok(p) => {
                    if let Err(e) = p.call_method(metodo, &()).await {
                        eprintln!("morgan: reproductor: {}: {metodo}: {e}", r.bus);
                    }
                }
                Err(e) => eprintln!("morgan: reproductor: {e}"),
            }
        });
    }

    async fn conexion(&self) -> zbus::Result<Connection> {
        if let Some(c) = self.inner.conexion.borrow().as_ref() {
            return Ok(c.clone());
        }
        let c = Connection::session().await?;
        self.inner.conexion.replace(Some(c.clone()));
        Ok(c)
    }

    async fn releer(&self, conn: &Connection) {
        let nuevo = leer(conn).await.unwrap_or_else(|e| {
            eprintln!("morgan: reproductor: {e}");
            None
        });
        if *self.inner.actual.borrow() != nuevo {
            self.inner.actual.replace(nuevo);
            let oyentes = self.inner.oyentes.borrow().clone();
            for f in oyentes {
                f();
            }
        }
    }
}

async fn leer(conn: &Connection) -> zbus::Result<Option<Reproduccion>> {
    let dbus = zbus::fdo::DBusProxy::new(conn).await?;
    let mut candidatos = Vec::new();
    let mut datos = HashMap::new();
    for n in dbus.list_names().await? {
        let n = n.to_string();
        if !n.starts_with(PREFIJO) || n == PLAYERCTLD {
            continue;
        }
        let Ok(p) = zbus::proxy::Builder::<zbus::Proxy>::new(conn)
            .destination(n.clone())?
            .path(RUTA)?
            .interface(INTERFAZ)?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
        else {
            continue;
        };
        let metadata: HashMap<String, OwnedValue> =
            p.get_property("Metadata").await.unwrap_or_default();
        // Sin metadatos no se muestra (ni cuenta para elegir).
        let Some((titulo, subtitulo)) = textos(&metadata) else {
            continue;
        };
        let estado = p
            .get_property::<String>("PlaybackStatus")
            .await
            .ok()
            .and_then(|s| reproductor::Estado::de(&s));
        let puede = |k: &'static str| {
            let p = p.clone();
            async move { p.get_property::<bool>(k).await.unwrap_or(false) }
        };
        let r = Reproduccion {
            bus: n.clone(),
            titulo,
            subtitulo,
            caratula: caratula(&metadata),
            estado,
            puede_anterior: puede("CanGoPrevious").await,
            puede_siguiente: puede("CanGoNext").await,
            mezcla: p.get_property::<bool>("Shuffle").await.ok(),
            repetir: p.get_property::<String>("LoopStatus").await.ok(),
        };
        candidatos.push((n.clone(), estado));
        datos.insert(n, r);
    }
    Ok(reproductor::elegir(candidatos).and_then(|n| datos.remove(&n)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(pares: Vec<(&str, Value<'static>)>) -> HashMap<String, OwnedValue> {
        pares
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn titulo_y_subtitulo_como_swaync() {
        let m = meta(vec![
            ("xesam:title", Value::from("Jóga")),
            ("xesam:artist", Value::from(vec!["Björk", "Otro"])),
            ("xesam:album", Value::from("Homogenic")),
        ]);
        assert_eq!(
            textos(&m),
            Some(("Jóga".into(), "Björk - Homogenic".into()))
        );
        // Sin álbum, el artista.
        let m = meta(vec![
            ("xesam:title", Value::from("Jóga")),
            ("xesam:artist", Value::from(vec!["Björk"])),
        ]);
        assert_eq!(textos(&m), Some(("Jóga".into(), "Björk".into())));
        let m = meta(vec![
            ("xesam:title", Value::from("Tema")),
            ("xesam:albumArtist", Value::from(vec!["Banda"])),
            ("xesam:album", Value::from("Disco")),
        ]);
        assert_eq!(textos(&m), Some(("Tema".into(), "Banda - Disco".into())));
        let m = meta(vec![("xesam:title", Value::from("Video"))]);
        assert_eq!(textos(&m), Some(("Video".into(), String::new())));
        let m = meta(vec![("xesam:title", Value::from("  "))]);
        assert_eq!(textos(&m), None);
        assert_eq!(textos(&HashMap::new()), None);
    }

    #[test]
    fn repeticion_en_ciclo() {
        assert_eq!(siguiente_repeticion("None"), "Playlist");
        assert_eq!(siguiente_repeticion("Playlist"), "Track");
        assert_eq!(siguiente_repeticion("Track"), "None");
    }

    #[test]
    fn solo_caratulas_locales() {
        let m = meta(vec![(
            "mpris:artUrl",
            Value::from("file:///tmp/con%20espacio.jpg"),
        )]);
        assert_eq!(caratula(&m), Some(PathBuf::from("/tmp/con espacio.jpg")));
        let m = meta(vec![("mpris:artUrl", Value::from("https://x/y.jpg"))]);
        assert_eq!(caratula(&m), None);
    }
}
