//! Elegir una carpeta con el selector del escritorio, por el portal de freedesktop
//! (`org.freedesktop.portal.FileChooser`): el mismo que abre cualquier app, con el aspecto del
//! sistema.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use futures_lite::StreamExt;
use zbus::zvariant::{OwnedValue, Value};

const DESTINO: &str = "org.freedesktop.portal.Desktop";
const RUTA: &str = "/org/freedesktop/portal/desktop";

/// La carpeta elegida; `None` si se canceló o el portal no está.
pub async fn elegir(titulo: &str, etiqueta: &str, inicial: &Path) -> Option<PathBuf> {
    // En el anidado, el portal del bus de prueba abre el selector en la sesión de afuera.
    if std::env::var_os("MORGAN_SIN_PORTAL").is_some() {
        eprintln!("morgan: selector de carpetas: sin portal (MORGAN_SIN_PORTAL)");
        return None;
    }
    match pedir(titulo, etiqueta, inicial).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("morgan: selector de carpetas: {e}");
            None
        }
    }
}

async fn pedir(titulo: &str, etiqueta: &str, inicial: &Path) -> zbus::Result<Option<PathBuf>> {
    let conn = zbus::Connection::session().await?;
    let token = format!("morgan_{}", std::process::id());
    // La respuesta llega a una ruta que se sabe de antes: se escucha antes de preguntar.
    let emisor = conn
        .unique_name()
        .map(|n| n.as_str().trim_start_matches(':').replace('.', "_"))
        .unwrap_or_default();
    let pedido = format!("{RUTA}/request/{emisor}/{token}");
    let respuestas = zbus::Proxy::new(&conn, DESTINO, pedido, "org.freedesktop.portal.Request")
        .await?
        .receive_signal("Response")
        .await?;
    let mut respuestas = respuestas;

    let mut opciones: HashMap<&str, Value<'_>> = HashMap::new();
    opciones.insert("handle_token", Value::from(token));
    opciones.insert("directory", Value::from(true));
    opciones.insert("accept_label", Value::from(etiqueta));
    if inicial.is_dir() {
        // `ay` terminado en NUL.
        let mut b = inicial.as_os_str().as_encoded_bytes().to_vec();
        b.push(0);
        opciones.insert("current_folder", Value::from(b));
    }
    let selector =
        zbus::Proxy::new(&conn, DESTINO, RUTA, "org.freedesktop.portal.FileChooser").await?;
    let _: zbus::zvariant::OwnedObjectPath = selector
        .call("OpenFile", &("", titulo, opciones))
        .await?;

    let Some(m) = respuestas.next().await else {
        return Ok(None);
    };
    let (codigo, resultados): (u32, HashMap<String, OwnedValue>) = m.body().deserialize()?;
    if codigo != 0 {
        return Ok(None);
    }
    let uris: Vec<String> = resultados
        .get("uris")
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<String>::try_from(v).ok())
        .unwrap_or_default();
    Ok(uris
        .first()
        .and_then(|u| glib::filename_from_uri(u).ok())
        .map(|(ruta, _)| ruta))
}
