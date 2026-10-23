//! `theme.css` es un symlink al tema activo y cada cambio de tema o de wallpaper reescribe el
//! destino. Se vigilan los dos (inotify vía `GFileMonitor`, sin polling).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use gtk::gio::{self, FileMonitorEvent, FileMonitorFlags};
use gtk::prelude::*;
use gtk::{gdk, glib};

use morgan_motor::forma::Borde;

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
    /// Lo que no es GTK (el OSD) recibe los colores cargados, la opacidad de las tarjetas y lo
    /// de afuera del anillo.
    cargado: RefCell<(String, f64, Borde)>,
    shell: RefCell<Shell>,
    oyentes: RefCell<Vec<Oyente>>,
}

type Oyente = Box<dyn Fn(&str, f64, Borde)>;

#[derive(Default)]
struct Estado {
    _link_monitor: Option<gio::FileMonitor>,
    destino: Option<PathBuf>,
    destino_monitor: Option<gio::FileMonitor>,
    /// Lo último cargado, para no recargar (ni repintar) si el contenido no cambió.
    colores: Option<String>,
}

impl Tema {
    /// `borde`: lo de afuera del anillo de los paneles, que sigue a la sombra de las ventanas.
    pub fn new(display: &gdk::Display, link: PathBuf, shell: &Shell, borde: Borde) -> Self {
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
            estructura: RefCell::new(estructura(shell, borde)),
            link,
            provider,
            pendiente: Cell::new(false),
            estado: RefCell::default(),
            cargado: RefCell::new((String::new(), shell.opacidad, borde)),
            shell: RefCell::new(shell.clone()),
            oyentes: RefCell::default(),
        });
        let link_monitor = monitor(&inner.link, Rc::downgrade(&inner));
        inner.estado.borrow_mut()._link_monitor = link_monitor;
        inner.refrescar();
        Tema { inner }
    }

    /// Los colores cargados (`@define-color`), la opacidad de las tarjetas y el borde.
    pub fn actual(&self) -> (String, f64, Borde) {
        self.inner.cargado.borrow().clone()
    }

    /// `f` corre con cada cambio de colores, de opacidad o de borde.
    pub fn al_cambiar(&self, f: impl Fn(&str, f64, Borde) + 'static) {
        self.inner.oyentes.borrow_mut().push(Box::new(f));
    }

    pub fn set_shell(&self, shell: &Shell) {
        self.inner.shell.replace(shell.clone());
        self.inner.cargado.borrow_mut().1 = shell.opacidad;
        self.rehacer();
    }

    pub fn set_borde(&self, borde: Borde) {
        self.inner.cargado.borrow_mut().2 = borde;
        self.rehacer();
    }

    fn rehacer(&self) {
        let (shell, borde) = (
            self.inner.shell.borrow().clone(),
            self.inner.cargado.borrow().2,
        );
        self.inner.estructura.replace(estructura(&shell, borde));
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
                    self.cargar(&colores);
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
                    let colores = morgan_temas::colores_por_defecto();
                    self.provider
                        .load_from_string(&format!("{colores}\n{}", self.estructura.borrow()));
                    self.cargar(&colores);
                }
            }
        }
    }
}

impl Inner {
    fn cargar(&self, colores: &str) {
        let (opacidad, borde) = {
            let mut c = self.cargado.borrow_mut();
            c.0 = colores.to_owned();
            (c.1, c.2)
        };
        for f in self.oyentes.borrow().iter() {
            f(colores, opacidad, borde);
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

/// Con la sombra negra, lo de afuera del anillo de los paneles y de los toasts es el de
/// `forma::Borde::Sombra` (una línea negra y una sombra suave) en vez del brillo de la hoja.
const BORDE_SOMBRA: &str = "
.control-center,
.launcher {
    box-shadow:
        0 0 0 1px black,
        0 0 6px 1px alpha(black, 0.5);
}

.floating-notifications .notification-row .notification-background .notification {
    box-shadow:
        0 0 0 2px mix(@primary, @tertiary, 0.5),
        1px 1px 0 1px @tertiary,
        -1px -1px 0 1px @primary,
        0 0 0 3px black,
        0 0 6px 3px alpha(black, 0.5);
}

.floating-notifications .notification-row .notification-background .notification.critical {
    box-shadow:
        0 0 0 2px @error,
        0 0 0 3px black,
        0 0 6px 3px alpha(black, 0.5);
}
";

fn estructura(shell: &Shell, borde: Borde) -> String {
    let o = shell.opacidad;
    let extra = if borde == Borde::Sombra {
        BORDE_SOMBRA
    } else {
        ""
    };
    format!(
        "@define-color morgan_fondo alpha(@surface_container_low, {o});\n\
         @define-color morgan_barra alpha(@surface, {o});\n{}{extra}",
        crate::animaciones::escalar_css(include_str!("estilo.css"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opacidad_en_la_hoja() {
        let hoja = estructura(
            &Shell {
                opacidad: 0.85,
                ..Shell::default()
            },
            Borde::Brillo,
        );
        assert!(
            hoja.starts_with("@define-color morgan_fondo alpha(@surface_container_low, 0.85);\n")
        );
        assert!(hoja.contains("@define-color morgan_barra alpha(@surface, 0.85);\n"));
        assert!(
            estructura(
                &Shell {
                    opacidad: 1.0,
                    ..Shell::default()
                },
                Borde::Brillo
            )
            .contains(", 1);")
        );
    }

    #[test]
    fn la_sombra_negra_reemplaza_el_brillo() {
        let shell = Shell::default();
        assert!(!estructura(&shell, Borde::Brillo).contains(BORDE_SOMBRA));
        assert!(estructura(&shell, Borde::Sombra).ends_with(BORDE_SOMBRA));
    }

    #[test]
    fn la_hoja_no_trae_hex() {
        // Los colores salen de los roles de matugen, nunca de un valor fijo en la estructura.
        assert!(!include_str!("estilo.css").contains('#'));
    }
}
