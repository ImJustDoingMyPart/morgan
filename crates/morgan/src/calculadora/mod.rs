//! El motor es `qalc`, con 60 ms de espera y uno a la vez: el de un cambio nuevo mata al
//! anterior, y ninguno vive más de `TOPE` (la actualización de las tasas de cambio puede
//! colgarse con la red).

pub(crate) mod modelo;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::gio::prelude::*;
use gtk::{gio, glib};
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::centro::Centro;
use crate::centro::hilo::Orden;
use crate::centro::vista::{EventoCalculadora, VistaCalculadora};
use crate::comando;
use modelo::{Accion, Expresion, Historial};

const ESPERA: Duration = Duration::from_millis(60);

const TOPE: Duration = Duration::from_secs(10);

const AVISO: Duration = Duration::from_millis(900);

/// Se dibuja en el hilo del centro (`centro/dibujo/calculadora.rs`), que le manda lo que se
/// tocó; acá quedan la cuenta, `qalc` y la cinta.
#[derive(Clone)]
pub struct Calculadora {
    inner: Rc<Inner>,
}

struct Inner {
    hilo: Sender<Orden>,
    historial: RefCell<Historial>,
    archivo: Option<PathBuf>,
    expresion: RefCell<Expresion>,
    previa: RefCell<Option<String>>,
    cuenta: RefCell<Option<String>>,
    turno: Cell<u64>,
    espera: RefCell<Option<glib::SourceId>>,
    consulta: RefCell<Option<gio::Cancellable>>,
    igual_pedido: Cell<bool>,
    aviso: RefCell<Option<glib::SourceId>>,
    abierto: Cell<bool>,
}

impl Calculadora {
    pub fn new(centro: &Centro) -> Self {
        let archivo = crate::notificaciones::dir_from(|k| std::env::var_os(k))
            .map(|d| d.join("calculadora.json"));
        let historial = archivo.as_deref().map(cargar).unwrap_or_default();
        let c = Calculadora {
            inner: Rc::new(Inner {
                hilo: centro.hilo(),
                historial: RefCell::new(historial),
                archivo,
                expresion: RefCell::default(),
                previa: RefCell::default(),
                cuenta: RefCell::default(),
                turno: Cell::new(0),
                espera: RefCell::default(),
                consulta: RefCell::default(),
                igual_pedido: Cell::new(false),
                aviso: RefCell::default(),
                abierto: Cell::new(false),
            }),
        };
        let weak = Rc::downgrade(&c.inner);
        centro.al_calculadora(move |e| {
            if let Some(inner) = weak.upgrade() {
                Calculadora { inner }.evento(e);
            }
        });
        c.pintar();
        c
    }

    fn evento(&self, e: EventoCalculadora) {
        match e {
            EventoCalculadora::Accion(a) => self.hacer(a),
            EventoCalculadora::Escribir(s) => self.escribir(&s),
            EventoCalculadora::Copiar => self.copiar(),
            EventoCalculadora::Cerrada => self.inner.abierto.set(false),
        }
    }

    fn anotar(&self, cuenta: &str, resultado: &str) {
        let i = &self.inner;
        if !i.historial.borrow_mut().agregar(cuenta, resultado) {
            return;
        }
        if let Some(archivo) = &i.archivo
            && let Err(e) = guardar(archivo, &i.historial.borrow())
        {
            eprintln!("morgan: calculadora: {}: {e}", archivo.display());
        }
    }

    pub fn abierto(&self) -> bool {
        self.inner.abierto.get()
    }

    pub fn alternar(&self) {
        if self.abierto() {
            self.cerrar();
        } else {
            self.abrir();
        }
    }

    pub fn abrir(&self) {
        let i = &self.inner;
        if i.abierto.replace(true) {
            return;
        }
        let _ = i.hilo.send(Orden::CalculadoraAbrir);
        self.pintar();
    }

    pub fn cerrar(&self) {
        let i = &self.inner;
        if i.abierto.replace(false) {
            let _ = i.hilo.send(Orden::CalculadoraCerrar);
        }
    }

    fn hacer(&self, accion: Accion) {
        match accion {
            Accion::Escribir(s) => self.escribir(s),
            Accion::Parentesis => {
                self.inner.expresion.borrow_mut().parentesis();
                self.cambio();
            }
            Accion::Borrar => {
                self.inner.expresion.borrow_mut().borrar();
                self.cambio();
            }
            Accion::Limpiar => {
                self.inner.expresion.borrow_mut().limpiar();
                self.cambio();
            }
            Accion::Igual => self.igual(),
        }
    }

    fn escribir(&self, s: &str) {
        self.inner.expresion.borrow_mut().escribir(s);
        self.cambio();
    }

    fn cambio(&self) {
        let i = &self.inner;
        self.quitar_aviso();
        i.cuenta.take();
        i.previa.take();
        i.igual_pedido.set(false);
        self.pintar();
        if let Some(viejo) = i.espera.take() {
            viejo.remove();
        }
        if let Some(vieja) = i.consulta.take() {
            vieja.cancel();
        }
        i.turno.set(i.turno.get() + 1);
        if i.expresion.borrow().texto().is_empty() {
            return;
        }
        let yo = self.clone();
        let id = glib::timeout_add_local_once(ESPERA, move || {
            yo.inner.espera.take();
            yo.consultar();
        });
        i.espera.replace(Some(id));
    }

    /// La respuesta vale solo si no hubo otro cambio.
    fn consultar(&self) {
        let i = &self.inner;
        let expresion = i.expresion.borrow().texto().to_owned();
        let turno = i.turno.get();
        let cancelar = gio::Cancellable::new();
        i.consulta.replace(Some(cancelar.clone()));
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let r = calcular(&expresion, &cancelar).await;
            let i = &yo.inner;
            if i.turno.get() != turno {
                return;
            }
            i.consulta.take();
            i.previa.replace(r);
            // Sin resultado útil el `=` pedido se olvida: otro `=` vuelve a preguntar.
            if i.igual_pedido.take() && i.previa.borrow().is_some() {
                yo.igual();
            } else {
                yo.pintar();
            }
        });
    }

    fn igual(&self) {
        let i = &self.inner;
        if i.expresion.borrow().es_resultado() || i.expresion.borrow().texto().is_empty() {
            return;
        }
        let lista = i.previa.borrow().clone();
        if let Some(r) = lista {
            let cuenta = i.expresion.borrow().texto().to_owned();
            i.expresion.borrow_mut().poner_resultado(&r);
            i.previa.take();
            self.anotar(&cuenta, &r);
            i.cuenta.replace(Some(cuenta));
            self.pintar();
            return;
        }
        if i.consulta.borrow().is_some() {
            i.igual_pedido.set(true);
            return;
        }
        // Todavía en la espera de 60 ms: se pregunta ya. Si `qalc` ya contestó sin resultado
        // útil, se vuelve a preguntar (pudo haberse cortado por el tope).
        if let Some(espera) = i.espera.take() {
            espera.remove();
        }
        i.igual_pedido.set(true);
        self.consultar();
    }

    fn copiar(&self) {
        let i = &self.inner;
        let r = if i.expresion.borrow().es_resultado() {
            Some(i.expresion.borrow().texto().to_owned())
        } else {
            i.previa.borrow().clone()
        };
        let Some(r) = r else { return };
        comando::lanzar_con_entrada(&["wl-copy", "--"], r.as_bytes());
        self.quitar_aviso();
        let yo = self.clone();
        let id = glib::timeout_add_local_once(AVISO, move || {
            yo.inner.aviso.take();
            yo.pintar();
        });
        i.aviso.replace(Some(id));
        self.pintar();
    }

    /// Lo que se escriba después no hereda el aviso.
    fn quitar_aviso(&self) {
        if let Some(id) = self.inner.aviso.take() {
            id.remove();
        }
    }

    fn pintar(&self) {
        let i = &self.inner;
        let e = i.expresion.borrow();
        let copiado = i.aviso.borrow().is_some();
        let chica = if copiado {
            tr!("copiado").to_owned()
        } else if e.es_resultado() {
            i.cuenta.borrow().clone().unwrap_or_default()
        } else {
            i.previa
                .borrow()
                .as_ref()
                .map(|r| format!("= {r}"))
                .unwrap_or_default()
        };
        let cinta = i
            .historial
            .borrow()
            .cuentas()
            .iter()
            .map(|c| (c.cuenta.clone(), c.resultado.clone()))
            .collect();
        let _ = i.hilo.send(Orden::Calculadora(VistaCalculadora {
            grande: e.texto().to_owned(),
            vacia: e.texto().is_empty(),
            chica,
            copiado,
            cinta,
        }));
    }
}

async fn calcular(expresion: &str, cancelar: &gio::Cancellable) -> Option<String> {
    let args = ["qalc", "-t", "-s", "update_exchange_rates 1days", expresion];
    match comando::salida_cortable(&args, TOPE, cancelar).await {
        Ok(s) => modelo::resultado(expresion, &s),
        Err(e) => {
            // Cancelado es lo normal (se siguió escribiendo); cortado por el tope, no.
            if !cancelar.is_cancelled() {
                eprintln!("morgan: calculadora: {e}");
            }
            None
        }
    }
}

/// Un historial ilegible no impide abrir: se empieza vacío.
fn cargar(archivo: &Path) -> Historial {
    match std::fs::read_to_string(archivo) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_else(|e| {
            eprintln!("morgan: calculadora: {}: {e}", archivo.display());
            Historial::default()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Historial::default(),
        Err(e) => {
            eprintln!("morgan: calculadora: {}: {e}", archivo.display());
            Historial::default()
        }
    }
}

fn guardar(archivo: &Path, h: &Historial) -> std::io::Result<()> {
    if let Some(dir) = archivo.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = archivo.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(h)?)?;
    std::fs::rename(tmp, archivo)
}
