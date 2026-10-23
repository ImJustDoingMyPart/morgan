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

fn correccion(logica: (i32, i32, i32, i32), tinta: (i32, i32, i32, i32)) -> (i32, i32) {
    let centro = |x: i32, w: i32| 2 * x + w;
    let dx = (centro(logica.0, logica.2) - centro(tinta.0, tinta.2)) / 2;
    let dy = (centro(logica.1, logica.3) - centro(tinta.1, tinta.3)) / 2;
    (dx, dy)
}

/// GTK centra un glifo por el ancho que reserva la fuente, no por su dibujo: los de la Nerd
/// Font tienen la tinta corrida dentro de ese ancho, y cuánto depende de la fuente que
/// resuelva fontconfig. Se mide la tinta y se compensa con márgenes.
pub fn centrar_tinta(l: &gtk::Label) {
    let (tinta, logica) = l.layout().pixel_extents();
    let r = |r: gtk::pango::Rectangle| (r.x(), r.y(), r.width(), r.height());
    let (dx, dy) = correccion(r(logica), r(tinta));
    l.set_margin_start((2 * dx).max(0));
    l.set_margin_end((-2 * dx).max(0));
    l.set_margin_top((2 * dy).max(0));
    l.set_margin_bottom((-2 * dy).max(0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_tinta_va_al_centro() {
        // Caja de 20×24 en el origen; tinta de 12×12 pegada arriba a la izquierda: hay que
        // correrla 4 a la derecha y 6 abajo.
        assert_eq!(correccion((0, 0, 20, 24), (0, 0, 12, 12)), (4, 6));
        // Ya centrada: nada.
        assert_eq!(correccion((0, 0, 20, 20), (4, 4, 12, 12)), (0, 0));
        // Corrida a la derecha: hacia la izquierda.
        assert_eq!(correccion((0, 0, 20, 20), (8, 4, 12, 12)), (-4, 0));
    }
}
