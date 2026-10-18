//! GTK 4.22 dibuja los SVG con su propio intérprete (`GtkSvg`, sin librsvg), que rechaza
//! cosas que librsvg acepta (`fill="var(--…)"` sale vacío). Cuando el SVG que elige el tema
//! no carga limpio, se usa la versión rasterizada de `/usr/share/pixmaps`, si la app trae una.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

pub fn imagen(display: &gdk::Display, icono: Option<&gio::Icon>, tamano: i32) -> gtk::Image {
    let imagen = match icono {
        Some(i) => match respaldo(display, i, tamano) {
            Some(png) => gtk::Image::from_file(png),
            None => gtk::Image::from_gicon(i),
        },
        None => gtk::Image::from_icon_name("application-x-executable"),
    };
    imagen.set_pixel_size(tamano);
    imagen
}

fn respaldo(display: &gdk::Display, icono: &gio::Icon, tamano: i32) -> Option<PathBuf> {
    let nombres = icono.downcast_ref::<gio::ThemedIcon>()?.names();
    let pintable = gtk::IconTheme::for_display(display).lookup_by_gicon(
        icono,
        tamano,
        1,
        gtk::TextDirection::None,
        gtk::IconLookupFlags::empty(),
    );
    let archivo = pintable.file()?.path()?;
    if archivo.extension().is_none_or(|e| e != "svg") || svg_sano(&archivo) {
        return None;
    }
    nombres
        .iter()
        .map(|n| Path::new("/usr/share/pixmaps").join(format!("{n}.png")))
        .find(|p| p.is_file())
}

/// La señal `error` llega durante la carga.
fn svg_sano(archivo: &Path) -> bool {
    let Ok(datos) = std::fs::read(archivo) else {
        return false;
    };
    let svg = gtk::Svg::new();
    let fallo = Rc::new(Cell::new(false));
    let f = fallo.clone();
    svg.connect_error(move |_, _| f.set(true));
    svg.load_from_bytes(&glib::Bytes::from_owned(datos));
    !fallo.get()
}
