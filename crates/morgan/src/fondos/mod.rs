//! Lo marcado como actual sale del fondo anotado al abrir; al aplicar se marca el pedido (el
//! tema sigue calculándose detrás). La banda se dibuja en el hilo del centro
//! (`centro/dibujo/fondos.rs`); acá quedan la carpeta, aplicar y la rotación.

pub(crate) mod modelo;
pub(crate) mod motor;
mod transicion;

pub use motor::actual;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk::{gio, glib};
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::centro::Centro;
use crate::centro::hilo::Orden;
use crate::centro::vista::{EventoFondos, VistaFondos};
use crate::config;
use crate::hyprland::Hyprland;

#[derive(Clone)]
pub struct Fondos {
    inner: Rc<Inner>,
}

struct Inner {
    hyprland: Hyprland,
    motor: motor::Motor,
    hilo: Sender<Orden>,
    abierta: Cell<bool>,
    /// Sube con cada pedido: una apertura que termina de leer tarde no abre si mientras tanto se
    /// pidió cerrar.
    pedido: Cell<u64>,
    al_volver: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Fondos {
    pub fn new(hyprland: Hyprland, c: &config::Fondos, centro: &Centro) -> Self {
        hyprland.al_evento(|linea| {
            if let Some(salida) = motor::salida_nueva(linea) {
                motor::pintar_salida(salida.to_owned());
            }
        });
        let f = Fondos {
            inner: Rc::new(Inner {
                hyprland,
                motor: motor::Motor::new(c),
                hilo: centro.hilo(),
                abierta: Cell::new(false),
                pedido: Cell::new(0),
                al_volver: RefCell::default(),
            }),
        };
        let weak = Rc::downgrade(&f.inner);
        centro.al_fondos(move |e| {
            if let Some(inner) = weak.upgrade() {
                Fondos { inner }.evento(e);
            }
        });
        f
    }

    fn evento(&self, e: EventoFondos) {
        match e {
            // La banda queda abierta: se puede seguir probando.
            EventoFondos::Aplicar(ruta) => self.inner.motor.aplicar(ruta),
            EventoFondos::Cerrada { volver } => {
                self.inner.abierta.set(false);
                let f = self.inner.al_volver.borrow().clone();
                if volver && let Some(f) = f {
                    f();
                }
            }
        }
    }

    pub fn aplicar(&self, img: PathBuf) -> Result<(), String> {
        if !img.is_absolute() || std::fs::File::open(&img).is_err() {
            return Err(format!("no es una imagen legible: «{}»", img.display()));
        }
        self.inner.motor.aplicar(img);
        Ok(())
    }

    pub async fn rotar(&self) -> Result<(), String> {
        self.inner.motor.rotar().await
    }

    pub fn aplicar_config(&self, c: &config::Fondos) {
        self.inner.motor.aplicar_config(c);
    }

    pub fn al_volver(&self, f: impl Fn() + 'static) {
        self.inner.al_volver.replace(Some(Rc::new(f)));
    }

    pub fn abierto(&self) -> bool {
        self.inner.abierta.get()
    }

    pub fn alternar(&self) {
        if self.abierto() {
            self.cerrar();
        } else {
            self.abrir(false);
        }
    }

    pub fn abrir(&self, volver: bool) {
        if self.abierto() {
            return;
        }
        let pedido = self.inner.pedido.get() + 1;
        self.inner.pedido.set(pedido);
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let Some(datos) = leer(yo.inner.motor.config()).await else {
                return;
            };
            if yo.inner.pedido.get() != pedido || yo.abierto() {
                return;
            }
            let carpeta = datos
                .raiz
                .file_name()
                .map(|n| capitalizar(&n.to_string_lossy()))
                .unwrap_or_default();
            let _ = yo.inner.hilo.send(Orden::FondosAbrir(Box::new(VistaFondos {
                salida: yo.inner.hyprland.espacios().enfocada,
                carpeta,
                rutas: datos.todos,
                actual: datos.actual,
                faltan: datos.faltan,
                cache: motor::cache_miniaturas(),
                volver,
            })));
            yo.inner.abierta.set(true);
        });
    }

    pub fn cerrar(&self) {
        self.inner.pedido.set(self.inner.pedido.get() + 1);
        if self.inner.abierta.replace(false) {
            let _ = self.inner.hilo.send(Orden::FondosCerrar);
        }
    }
}

struct Datos {
    raiz: PathBuf,
    actual: Option<PathBuf>,
    todos: Vec<PathBuf>,
    faltan: Vec<PathBuf>,
}

async fn leer(c: config::Fondos) -> Option<Datos> {
    let raiz = c.raiz();
    let r = raiz.clone();
    // Solo se lista y se ve qué miniaturas faltan: hacerlas antes de abrir tarda segundos con
    // miles de imágenes. Se hacen después, por cercanía.
    let (todos, faltan) = gio::spawn_blocking(move || {
        let todos = modelo::listar(&r, &c.excluir);
        let faltan = modelo::sin_miniatura(&motor::cache_miniaturas(), &todos);
        (todos, faltan)
    })
    .await
    .ok()?;
    Some(Datos {
        raiz,
        actual: actual(),
        todos,
        faltan,
    })
}

fn capitalizar(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|p| p.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carpeta_capitalizada() {
        assert_eq!(capitalizar("imágenes"), "Imágenes");
        assert_eq!(capitalizar("ñandú"), "Ñandú");
        assert_eq!(capitalizar(""), "");
    }
}
