//! Una señal solo avisa que algo cambió; el estado se relee entero del servicio.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use futures_lite::StreamExt;
use zbus::{Connection, MatchRule, MessageStream};

thread_local! {
    static BUS: RefCell<Option<Connection>> = const { RefCell::new(None) };
}

/// Lo que se espera después de una señal para juntar las que vienen detrás.
const RAFAGA: Duration = Duration::from_millis(30);

pub async fn bus() -> zbus::Result<Connection> {
    if let Some(c) = BUS.with_borrow(|b| b.clone()) {
        return Ok(c);
    }
    let c = Connection::system().await?;
    // Varios pedidos a la vez al arrancar abren una conexión cada uno: gana la primera
    // guardada y las demás se sueltan. Antes ganaba la última, y lo publicado por una que
    // perdía (el agente de polkit) se iba con ella al soltarse.
    Ok(BUS.with_borrow_mut(|b| b.get_or_insert(c).clone()))
}

pub type Releer = Box<dyn FnMut() -> Pin<Box<dyn Future<Output = ()>>>>;

pub fn vigilar(regla: MatchRule<'static>, mut releer: Releer) -> glib::JoinHandle<()> {
    glib::MainContext::default().spawn_local(async move {
        let conn = match bus().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("morgan: bus del sistema: {e}");
                return;
            }
        };
        // Suscripción antes de leer: un cambio entre las dos queda en la fila.
        let mut senales = match MessageStream::for_match_rule(regla, &conn, Some(64)).await {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("morgan: bus del sistema: suscripción: {e}");
                None
            }
        };
        releer().await;
        let Some(senales) = senales.as_mut() else {
            return;
        };
        while senales.next().await.is_some() {
            glib::timeout_future(RAFAGA).await;
            while let Some(Some(_)) = futures_lite::future::poll_once(senales.next()).await {}
            releer().await;
        }
    })
}

pub fn propiedades(destino: &'static str, ruta: &'static str) -> MatchRule<'static> {
    MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(destino)
        .and_then(|b| b.path(ruta))
        .and_then(|b| b.interface("org.freedesktop.DBus.Properties"))
        .and_then(|b| b.member("PropertiesChanged"))
        .map(|b| b.build())
        .expect("regla fija")
}

/// Las de todos los objetos debajo de `raiz`, que pueden aparecer y desaparecer.
pub fn propiedades_bajo(destino: &'static str, raiz: &'static str) -> MatchRule<'static> {
    MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(destino)
        .and_then(|b| b.path_namespace(raiz))
        .and_then(|b| b.interface("org.freedesktop.DBus.Properties"))
        .and_then(|b| b.member("PropertiesChanged"))
        .map(|b| b.build())
        .expect("regla fija")
}

pub fn de(destino: &'static str) -> MatchRule<'static> {
    MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(destino)
        .map(|b| b.build())
        .expect("regla fija")
}

pub async fn proxy(
    destino: &'static str,
    ruta: impl TryInto<zbus::zvariant::ObjectPath<'static>, Error = impl Into<zbus::Error>>,
    interfaz: &'static str,
) -> zbus::Result<zbus::Proxy<'static>> {
    let ruta = ruta.try_into().map_err(Into::into)?;
    zbus::proxy::Builder::<zbus::Proxy>::new(&bus().await?)
        .destination(destino)?
        .path(ruta)?
        .interface(interfaz)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}
