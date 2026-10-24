//! `theme.css` es un symlink al tema activo y cada cambio de tema o de wallpaper reescribe el
//! destino. Se vigilan los dos (inotify vía `GFileMonitor`, sin polling).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use gio::prelude::*;
use gio::{FileMonitorEvent, FileMonitorFlags};

use morgan_motor::forma::Borde;

use crate::config::Shell;

#[derive(Clone)]
pub struct Tema {
    inner: Rc<Inner>,
}

struct Inner {
    link: PathBuf,
    pendiente: Cell<bool>,
    estado: RefCell<Estado>,
    /// Las superficies reciben los colores cargados, la opacidad de las tarjetas y lo
    /// de afuera del anillo.
    cargado: RefCell<(String, f64, Borde)>,
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
    pub fn new(link: PathBuf, shell: &Shell, borde: Borde) -> Self {
        let inner = Rc::new(Inner {
            link,
            pendiente: Cell::new(false),
            estado: RefCell::default(),
            cargado: RefCell::new((String::new(), shell.opacidad, borde)),
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
        self.inner.cargado.borrow_mut().1 = shell.opacidad;
        self.rehacer();
    }

    pub fn set_borde(&self, borde: Borde) {
        self.inner.cargado.borrow_mut().2 = borde;
        self.rehacer();
    }

    fn rehacer(&self) {
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
