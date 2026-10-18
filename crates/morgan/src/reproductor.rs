//! El aviso muestra el estado en que quedó el reproductor, no el que se pidió. Se escucha el
//! cambio de la propiedad antes de pedirlo y se espera a que llegue (sin sondear); si el
//! reproductor no avisa a tiempo, se muestra lo último que informó.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use futures_lite::StreamExt;
use gtk::glib;
use morgan_ipc::AccionReproductor;
use zbus::Connection;
use zbus::zvariant::{OwnedValue, Value};

use crate::osd::{Aviso, Cuerpo};

const PREFIJO: &str = "org.mpris.MediaPlayer2.";
const RUTA: &str = "/org/mpris/MediaPlayer2";
const INTERFAZ: &str = "org.mpris.MediaPlayer2.Player";
/// playerctld reenvía a otro reproductor: contarlo sería contar ese dos veces.
const PLAYERCTLD: &str = "org.mpris.MediaPlayer2.playerctld";

const ESPERA: Duration = Duration::from_millis(1000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Estado {
    Reproduciendo,
    Pausado,
    Detenido,
}

impl Estado {
    pub fn de(s: &str) -> Option<Estado> {
        match s {
            "Playing" => Some(Estado::Reproduciendo),
            "Paused" => Some(Estado::Pausado),
            "Stopped" => Some(Estado::Detenido),
            _ => None,
        }
    }
}

/// Entre iguales, por nombre de bus, para que sea siempre el mismo.
pub fn elegir(mut candidatos: Vec<(String, Option<Estado>)>) -> Option<String> {
    candidatos.sort_by(|a, b| a.0.cmp(&b.0));
    let con = |e| candidatos.iter().find(|c| c.1 == Some(e));
    con(Estado::Reproduciendo)
        .or_else(|| con(Estado::Pausado))
        .or(candidatos.first())
        .map(|c| c.0.clone())
}

pub fn partes(artistas: &[String], titulo: Option<&str>) -> (String, String) {
    let artistas = artistas
        .iter()
        .map(|a| a.trim())
        .filter(|a| !a.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    match titulo.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => (t.to_string(), artistas),
        None if !artistas.is_empty() => (artistas, String::new()),
        None => ("Reproductor".into(), String::new()),
    }
}

pub fn icono(accion: AccionReproductor, estado: Option<Estado>) -> &'static str {
    match accion {
        AccionReproductor::Siguiente => "󰒭",
        AccionReproductor::Anterior => "󰒮",
        AccionReproductor::PlayPausa => match estado {
            Some(Estado::Reproduciendo) => "󰐊",
            Some(Estado::Pausado) => "󰏤",
            Some(Estado::Detenido) | None => "󰓛",
        },
    }
}

#[derive(Default)]
pub struct Reproductor {
    conexion: RefCell<Option<Connection>>,
}

impl Reproductor {
    pub async fn pedir(&self, accion: AccionReproductor, ms: u32) -> Result<Aviso, String> {
        let conn = self.conexion().await?;
        let nombre = elegido(&conn).await?;
        let p = zbus::Proxy::new(&conn, nombre.as_str(), RUTA, INTERFAZ)
            .await
            .map_err(|e| format!("reproductor: {nombre}: {e}"))?;
        let (metodo, propiedad) = match accion {
            AccionReproductor::PlayPausa => ("PlayPause", "PlaybackStatus"),
            AccionReproductor::Siguiente => ("Next", "Metadata"),
            AccionReproductor::Anterior => ("Previous", "Metadata"),
        };
        // El primer valor del stream es el actual; el que importa es el siguiente.
        let mut cambios = p.receive_property_changed::<OwnedValue>(propiedad).await;
        let _ = cambios.next().await;
        p.call_method(metodo, &())
            .await
            .map_err(|e| format!("reproductor: {nombre}: {metodo}: {e}"))?;
        let _ = glib::future_with_timeout(ESPERA, cambios.next()).await;

        let estado = p
            .get_property::<String>("PlaybackStatus")
            .await
            .ok()
            .and_then(|s| Estado::de(&s));
        let metadata = p
            .get_property::<HashMap<String, OwnedValue>>("Metadata")
            .await
            .unwrap_or_default();
        let artistas = metadata
            .get("xesam:artist")
            .and_then(|v| <Vec<String>>::try_from(Value::from(v.clone())).ok())
            .unwrap_or_default();
        let titulo = metadata
            .get("xesam:title")
            .and_then(|v| <&str>::try_from(&**v).ok());
        let (titulo, texto) = partes(&artistas, titulo);
        Ok(Aviso {
            icono: Some(icono(accion, estado).into()),
            titulo,
            cuerpo: Cuerpo::Estado {
                texto,
                encendido: estado == Some(Estado::Reproduciendo),
            },
            ms,
        })
    }

    async fn conexion(&self) -> Result<Connection, String> {
        if let Some(c) = self.conexion.borrow().as_ref() {
            return Ok(c.clone());
        }
        let c = Connection::session()
            .await
            .map_err(|e| format!("reproductor: bus de sesión: {e}"))?;
        self.conexion.replace(Some(c.clone()));
        Ok(c)
    }
}

async fn elegido(conn: &Connection) -> Result<String, String> {
    let dbus = zbus::fdo::DBusProxy::new(conn)
        .await
        .map_err(|e| format!("reproductor: {e}"))?;
    let nombres = dbus
        .list_names()
        .await
        .map_err(|e| format!("reproductor: {e}"))?;
    let mut candidatos = Vec::new();
    for n in nombres {
        let n = n.to_string();
        if !n.starts_with(PREFIJO) || n == PLAYERCTLD {
            continue;
        }
        let estado = match zbus::proxy::Builder::<zbus::Proxy>::new(conn)
            .destination(n.as_str())
            .and_then(|b| b.path(RUTA))
            .and_then(|b| b.interface(INTERFAZ))
        {
            Ok(b) => match b
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await
            {
                Ok(p) => p.get_property::<String>("PlaybackStatus").await.ok(),
                Err(_) => None,
            },
            Err(_) => None,
        };
        candidatos.push((n, estado.as_deref().and_then(Estado::de)));
    }
    elegir(candidatos).ok_or_else(|| "no hay ningún reproductor MPRIS".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(n: &str, e: Option<Estado>) -> (String, Option<Estado>) {
        (format!("{PREFIJO}{n}"), e)
    }

    #[test]
    fn gana_el_que_suena() {
        let r = elegir(vec![
            c("a", Some(Estado::Pausado)),
            c("b", Some(Estado::Reproduciendo)),
            c("c", None),
        ]);
        assert_eq!(r.as_deref(), Some("org.mpris.MediaPlayer2.b"));
    }

    #[test]
    fn sin_ninguno_sonando_gana_el_pausado() {
        let r = elegir(vec![
            c("a", Some(Estado::Detenido)),
            c("z", Some(Estado::Pausado)),
            c("m", Some(Estado::Pausado)),
        ]);
        assert_eq!(r.as_deref(), Some("org.mpris.MediaPlayer2.m"));
    }

    #[test]
    fn si_no_el_primero_por_nombre() {
        let r = elegir(vec![c("z", None), c("a", Some(Estado::Detenido))]);
        assert_eq!(r.as_deref(), Some("org.mpris.MediaPlayer2.a"));
        assert_eq!(elegir(vec![]), None);
    }

    #[test]
    fn partes_del_aviso() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let p = |t: &str, e: &str| (t.to_string(), e.to_string());
        assert_eq!(partes(&a(&["Björk"]), Some("Jóga")), p("Jóga", "Björk"));
        assert_eq!(partes(&a(&["A", "B"]), Some("T")), p("T", "A, B"));
        assert_eq!(partes(&a(&[]), Some("T")), p("T", ""));
        assert_eq!(partes(&a(&["", " "]), Some("T")), p("T", ""));
        assert_eq!(partes(&a(&["A"]), None), p("A", ""));
        assert_eq!(partes(&a(&["A"]), Some("  ")), p("A", ""));
        assert_eq!(partes(&a(&[]), None), p("Reproductor", ""));
    }

    #[test]
    fn iconos_por_estado_que_quedo() {
        use AccionReproductor::*;
        assert_eq!(icono(PlayPausa, Some(Estado::Reproduciendo)), "󰐊");
        assert_eq!(icono(PlayPausa, Some(Estado::Pausado)), "󰏤");
        assert_eq!(icono(PlayPausa, None), "󰓛");
        assert_eq!(icono(Siguiente, Some(Estado::Pausado)), "󰒭");
        assert_eq!(icono(Anterior, None), "󰒮");
    }
}
