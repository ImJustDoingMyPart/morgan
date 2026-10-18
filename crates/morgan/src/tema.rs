//! `theme.css` es un symlink al tema activo y cada cambio de tema o de wallpaper reescribe el
//! destino. Se vigilan los dos (inotify vía `GFileMonitor`, sin polling).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use gtk::gio::{self, FileMonitorEvent, FileMonitorFlags};
use gtk::prelude::*;
use gtk::{gdk, glib};

use crate::config::Shell;

#[derive(Clone)]
pub struct Tema {
    inner: Rc<Inner>,
}

struct Inner {
    link: PathBuf,
    estructura: RefCell<String>,
    provider: gtk::CssProvider,
    pendiente: Cell<bool>,
    estado: RefCell<Estado>,
}

#[derive(Default)]
struct Estado {
    _link_monitor: Option<gio::FileMonitor>,
    destino: Option<PathBuf>,
    destino_monitor: Option<gio::FileMonitor>,
    /// Lo último cargado, para no recargar (ni repintar) si el contenido no cambió.
    colores: Option<String>,
}

impl Tema {
    pub fn new(display: &gdk::Display, link: PathBuf, shell: &Shell) -> Self {
        let provider = gtk::CssProvider::new();
        provider.connect_parsing_error(|_, section, error| {
            eprintln!("morgan: tema: {section}: {error}");
        });
        gtk::style_context_add_provider_for_display(
            display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let inner = Rc::new(Inner {
            estructura: RefCell::new(estructura(shell)),
            link,
            provider,
            pendiente: Cell::new(false),
            estado: RefCell::default(),
        });
        let link_monitor = monitor(&inner.link, Rc::downgrade(&inner));
        inner.estado.borrow_mut()._link_monitor = link_monitor;
        inner.refrescar();
        Tema { inner }
    }

    pub fn set_shell(&self, shell: &Shell) {
        self.inner.estructura.replace(estructura(shell));
        self.inner.estado.borrow_mut().colores = None;
        self.inner.refrescar();
    }
}

impl Inner {
    /// Junta los eventos de una misma escritura (un `ln -sfn` son dos) en una sola recarga.
    fn programar(self: &Rc<Self>) {
        if self.pendiente.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(inner) = weak.upgrade() {
                inner.pendiente.set(false);
                inner.refrescar();
            }
        });
    }

    fn refrescar(self: &Rc<Self>) {
        let destino = std::fs::canonicalize(&self.link).ok();
        let mut estado = self.estado.borrow_mut();
        if destino != estado.destino {
            estado.destino_monitor = destino
                .as_deref()
                .and_then(|d| monitor(d, Rc::downgrade(self)));
            estado.destino = destino;
        }
        match std::fs::read_to_string(&self.link) {
            Ok(colores) => {
                if estado.colores.as_ref() != Some(&colores) {
                    self.provider
                        .load_from_string(&format!("{colores}\n{}", self.estructura.borrow()));
                    // Otro tema (no el primero al arrancar): el color de las carpetas del tema de íconos lo
                    // sigue. Después de soltar el estado.
                    let otro_tema = estado.colores.is_some();
                    estado.colores = Some(colores);
                    if otro_tema {
                        glib::idle_add_local_once(crate::apariencia::iconos::resincronizar);
                    }
                }
            }
            // Sin archivo (o a mitad de un reemplazo) queda el último tema cargado. Al arrancar
            // sin ninguno, los colores del tema por defecto, embebidos (sin matugen no se
            // renderiza ninguno).
            Err(e) => {
                eprintln!("morgan: tema: {}: {e}", self.link.display());
                if estado.colores.is_none() {
                    self.provider.load_from_string(&format!(
                        "{}\n{}",
                        morgan_temas::colores_por_defecto(),
                        self.estructura.borrow()
                    ));
                }
            }
        }
    }
}

fn monitor(path: &Path, inner: Weak<Inner>) -> Option<gio::FileMonitor> {
    let m = gio::File::for_path(path)
        .monitor_file(FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        .inspect_err(|e| eprintln!("morgan: tema: no se puede vigilar {}: {e}", path.display()))
        .ok()?;
    m.connect_changed(move |_, _, _, event| {
        // CHANGED llega a mitad de una escritura; la recarga espera a CHANGES_DONE_HINT.
        if recarga(event)
            && let Some(inner) = inner.upgrade()
        {
            inner.programar();
        }
    });
    Some(m)
}

fn recarga(event: FileMonitorEvent) -> bool {
    !matches!(
        event,
        FileMonitorEvent::Changed
            | FileMonitorEvent::AttributeChanged
            | FileMonitorEvent::PreUnmount
            | FileMonitorEvent::Unmounted
    )
}

fn estructura(shell: &Shell) -> String {
    let o = shell.opacidad;
    format!(
        "@define-color morgan_fondo alpha(@surface_container_low, {o});\n\
         @define-color morgan_barra alpha(@surface, {o});\n{}",
        crate::animaciones::escalar_css(include_str!("estilo.css"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opacidad_en_la_hoja() {
        let hoja = estructura(&Shell { opacidad: 0.85, ..Shell::default() });
        assert!(
            hoja.starts_with("@define-color morgan_fondo alpha(@surface_container_low, 0.85);\n")
        );
        assert!(hoja.contains("@define-color morgan_barra alpha(@surface, 0.85);\n"));
        assert!(estructura(&Shell { opacidad: 1.0, ..Shell::default() }).contains(", 1);"));
    }

    #[test]
    fn la_hoja_no_trae_hex() {
        // Los colores salen de los roles de matugen, nunca de un valor fijo en la estructura.
        assert!(!include_str!("estilo.css").contains('#'));
    }
}
