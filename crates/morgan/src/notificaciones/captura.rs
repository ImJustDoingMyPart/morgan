//! Es una notificación de morgan mismo, así que sus acciones no viajan por D-Bus a nadie:
//! las hace morgan, y siguen andando desde el historial después de un reinicio. Se reconoce
//! por la categoría (la spec deja las de la forma `x-vendor.clase`).

use std::path::Path;

use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;

use super::modelo::{Accion, Imagen, Notificacion, PedidoSonido, Urgencia};

pub const CATEGORIA: &str = "x-morgan.captura";

const ICONO: &str = "accessories-screenshot";

pub fn es_captura(n: &Notificacion) -> bool {
    n.categoria.as_deref() == Some(CATEGORIA)
}

/// hyprshot ya la copió al portapapeles.
pub fn notificacion(ruta: &Path, llegada: i64) -> Notificacion {
    let nombre = ruta
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let accion = |clave: &str, etiqueta: &str| Accion {
        clave: clave.into(),
        etiqueta: etiqueta.into(),
    };
    Notificacion {
        id: 0,
        app: "Captura".into(),
        entrada: None,
        nombre_app: None,
        icono_app: Some(Imagen::Nombre(ICONO.into())),
        resumen: "Captura guardada".into(),
        cuerpo: glib::markup_escape_text(&format!("{nombre}\nCopiada al portapapeles")).to_string(),
        acciones: vec![
            // `default` es el clic en la tarjeta, que no se dibuja como botón.
            accion("default", "Abrir"),
            accion("abrir", "Abrir"),
            accion("carpeta", "Carpeta"),
            accion("borrar", "Borrar"),
        ],
        urgencia: Urgencia::Normal,
        categoria: Some(CATEGORIA.into()),
        imagen: Some(Imagen::Archivo(ruta.into())),
        residente: false,
        transitoria: false,
        progreso: None,
        vence_ms: -1,
        llegada,
        // El obturador del tema de sonidos, no el de un mensaje.
        sonido: PedidoSonido::Nombre("screen-capture".into()),
    }
}

pub fn glifo(clave: &str) -> Option<&'static str> {
    match clave {
        "abrir" => Some("󰈈"),
        "carpeta" => Some("󰉋"),
        "borrar" => Some("󰆴"),
        _ => None,
    }
}

pub fn accion(n: &Notificacion, clave: &str) {
    let Some(Imagen::Archivo(ruta)) = &n.imagen else {
        return;
    };
    let archivo = gio::File::for_path(ruta);
    match clave {
        "default" | "abrir" => abrir(&archivo),
        "carpeta" => mostrar_en_carpeta(archivo),
        "borrar" => {
            // A la papelera, no borrada: se puede deshacer.
            archivo.trash_async(glib::Priority::DEFAULT, gio::Cancellable::NONE, |r| {
                if let Err(e) = r {
                    eprintln!("morgan: captura: no se pudo mandar a la papelera: {e}");
                }
            });
        }
        _ => {}
    }
}

fn abrir(archivo: &gio::File) {
    let Some(ruta) = archivo.path() else { return };
    if let Err(e) = crate::comando::abrir(&ruta, "application/octet-stream") {
        eprintln!("morgan: captura: no se pudo abrir {}: {e}", ruta.display());
    }
}

/// Sin quien atienda `FileManager1.ShowItems`, se abre la carpeta sola.
fn mostrar_en_carpeta(archivo: gio::File) {
    glib::MainContext::default().spawn_local(async move {
        let uri = archivo.uri().to_string();
        let r = match gio::bus_get_future(gio::BusType::Session).await {
            Ok(bus) => bus
                .call_future(
                    Some("org.freedesktop.FileManager1"),
                    "/org/freedesktop/FileManager1",
                    "org.freedesktop.FileManager1",
                    "ShowItems",
                    Some(&(vec![uri], "").to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    5000,
                )
                .await
                .map(|_| ()),
            Err(e) => Err(e),
        };
        if let Err(e) = r {
            eprintln!("morgan: captura: FileManager1: {e}");
            if let Some(dir) = archivo.parent() {
                abrir(&dir);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_captura_se_reconoce_y_trae_sus_acciones() {
        let n = notificacion(Path::new("/x/Imágenes/a<b>.png"), 7);
        assert!(es_captura(&n));
        assert_eq!(
            n.imagen,
            Some(Imagen::Archivo("/x/Imágenes/a<b>.png".into()))
        );
        assert_eq!(n.cuerpo, "a&lt;b&gt;.png\nCopiada al portapapeles");
        let botones: Vec<&str> = n.botones().map(|a| a.clave.as_str()).collect();
        assert_eq!(botones, ["abrir", "carpeta", "borrar"]);
        assert!(n.acciones.iter().any(|a| a.clave == "default"));
        assert!(!es_captura(&crate::notificaciones::modelo::prueba(
            1, "Captura"
        )));
    }
}
