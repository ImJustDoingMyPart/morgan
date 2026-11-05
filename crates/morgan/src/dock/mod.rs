//! El dock: las apps más usadas y las abiertas, abajo, con un punto por ventana. Lo dibuja su
//! propio hilo (`hilo.rs`); acá, en el hilo de GTK, se lee qué hay abierto y se hace lo que pide
//! cada click. Sin sondeo: lee las ventanas cuando Hyprland avisa que algo cambió.
//!
//! Las más usadas se eligen al arrancar y no se mueven hasta el próximo arranque: un dock sirve
//! porque cada cosa está siempre en el mismo lugar. El uso se cuenta por ventana abierta, venga
//! de donde venga (`dock-usos`); la primera vez arranca de lo que ya contó el launcher.

mod dibujo;
mod hilo;
pub mod modelo;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gio::prelude::*;
use morgan_motor::color::Paleta;
use morgan_motor::vello::peniko::ImageData;
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::config;
use crate::hyprland::Hyprland;
use crate::launcher::busqueda::Historial;
use crate::tema::Tema;
use hilo::{Arranque, Datos, Evento, Orden};
use modelo::{Abierta, Conocida, Contador, Pieza};

/// Se agrupan: una ráfaga de eventos es una sola lectura.
const AGRUPAR: Duration = Duration::from_millis(80);
const LADO_ICONO: u32 = modelo::ICONO as u32;

#[derive(Clone)]
pub struct Dock {
    inner: Rc<Inner>,
}

struct Inner {
    hilo: Sender<Orden>,
    hyprland: Hyprland,
    frecuentes: Vec<Conocida>,
    usos: RefCell<Historial>,
    archivo: Option<PathBuf>,
    contador: RefCell<Contador>,
    piezas: RefCell<Vec<Pieza>>,
    /// De clase a app: buscar el `.desktop` de una clase recorre todos.
    apps: RefCell<HashMap<String, Conocida>>,
    iconos: RefCell<HashMap<String, ImageData>>,
    programado: Cell<bool>,
    globo_abierto: Cell<bool>,
    escuchar: RefCell<Option<glib::JoinHandle<()>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(h) = self.escuchar.take() {
            h.abort();
        }
    }
}

fn archivo_usos() -> PathBuf {
    glib::user_state_dir().join("morgan").join("dock-usos")
}

/// El uso guardado o, la primera vez, lo que contó el launcher.
fn leer_usos(archivo: &PathBuf) -> Historial {
    let leer = |a: &PathBuf| std::fs::read_to_string(a).ok();
    let lanzador = glib::user_state_dir().join("morgan").join("launcher-apps");
    leer(archivo)
        .or_else(|| leer(&lanzador))
        .map(|t| Historial::parse(&t))
        .unwrap_or_default()
}

/// El nombre de una app que sigue instalada y se muestra en los menús.
fn nombre_de(id: &str) -> Option<String> {
    let d = gio_unix::DesktopAppInfo::new(id)?;
    d.should_show().then(|| d.display_name().to_string())
}

impl Dock {
    pub fn new(hyprland: Hyprland, tema: &Tema, c: &config::Dock) -> Self {
        let (css, opacidad, borde) = tema.actual();
        let (hilo, eventos) = hilo::lanzar(Arranque {
            paleta: Paleta::desde_css(&css),
            opacidad: opacidad as f32,
            borde,
            mostrar: c.mostrar,
            ocultar: c.ocultar,
        });
        {
            let t = hilo.clone();
            tema.al_cambiar(move |css, opacidad, borde| {
                let _ = t.send(Orden::Tema(Paleta::desde_css(css), opacidad as f32, borde));
            });
        }
        let archivo = archivo_usos();
        let usos = leer_usos(&archivo);
        let frecuentes = modelo::mas_usadas(&usos.ordenados(), nombre_de, c.usadas);
        let dock = Dock {
            inner: Rc::new(Inner {
                hilo,
                hyprland: hyprland.clone(),
                frecuentes,
                usos: RefCell::new(usos),
                archivo: Some(archivo),
                contador: RefCell::default(),
                piezas: RefCell::default(),
                apps: RefCell::default(),
                iconos: RefCell::default(),
                programado: Cell::new(false),
                globo_abierto: Cell::new(false),
                escuchar: RefCell::default(),
            }),
        };

        let d = dock.clone();
        let filtro = RefCell::new(modelo::Filtro::default());
        hyprland.al_evento(move |linea| {
            if filtro.borrow_mut().importa(linea, d.inner.globo_abierto.get()) {
                d.programar();
            }
        });
        let d = dock.clone();
        hyprland.al_cambiar_espacios(move |_| d.programar());
        let t = dock.inner.hilo.clone();
        let _ = t.send(Orden::Juego(hyprland.juego_en_foco()));
        hyprland.al_cambiar(move |juego| {
            let _ = t.send(Orden::Juego(juego));
        });

        let d = dock.clone();
        let escuchar = glib::MainContext::default().spawn_local(async move {
            while let Ok(e) = eventos.recv().await {
                d.evento(e);
            }
        });
        dock.inner.escuchar.replace(Some(escuchar));
        dock.programar();
        dock
    }

    pub fn aplicar_config(&self, c: &config::Dock) {
        let _ = self.inner.hilo.send(Orden::Config {
            mostrar: c.mostrar,
            ocultar: c.ocultar,
        });
    }

    fn programar(&self) {
        if self.inner.programado.replace(true) {
            return;
        }
        let d = self.clone();
        glib::MainContext::default().spawn_local(async move {
            glib::timeout_future(AGRUPAR).await;
            d.inner.programado.set(false);
            d.refrescar().await;
        });
    }

    async fn refrescar(&self) {
        let i = &self.inner;
        let Some(clientes) = i.hyprland.consultar("j/clients").await else {
            return;
        };
        let abiertas = match modelo::leer(&clientes) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("morgan: dock: j/clients ilegible: {e}");
                return;
            }
        };
        self.contar(&abiertas);
        let con_app: Vec<(Conocida, Abierta)> = abiertas
            .into_iter()
            .map(|a| (self.app_de(&a.clase), a))
            .collect();
        let activos: BTreeSet<i64> = i
            .hyprland
            .espacios()
            .activos
            .iter()
            .map(|(_, id)| *id)
            .collect();
        let piezas = modelo::armar(&i.frecuentes, &con_app, &activos);
        if *i.piezas.borrow() == piezas {
            return;
        }
        let iconos = piezas.iter().map(|p| self.icono(&p.clave)).collect();
        i.piezas.replace(piezas.clone());
        let _ = i.hilo.send(Orden::Apps(Datos { piezas, iconos }));
    }

    fn contar(&self, abiertas: &[Abierta]) {
        let i = &self.inner;
        let nuevas: Vec<String> = i
            .contador
            .borrow_mut()
            .nuevas(abiertas)
            .into_iter()
            .map(|a| a.clase.clone())
            .collect();
        let mut sumo = false;
        for clase in nuevas {
            let clave = self.app_de(&clase).clave;
            // Sin `.desktop` no se la podría volver a abrir: no compite por un lugar.
            if clave.ends_with(".desktop") {
                i.usos.borrow_mut().sumar(&clave);
                sumo = true;
            }
        }
        if sumo && let Some(a) = &i.archivo {
            crate::launcher::guardar(a, &i.usos.borrow().texto());
        }
    }

    fn app_de(&self, clase: &str) -> Conocida {
        let i = &self.inner;
        if let Some(a) = i.apps.borrow().get(clase) {
            return a.clone();
        }
        let app = crate::aplicacion::de_clase(clase)
            .and_then(|d| {
                Some(Conocida {
                    clave: d.id()?.to_string(),
                    nombre: d.display_name().to_string(),
                })
            })
            .unwrap_or_else(|| Conocida {
                clave: clase.to_owned(),
                nombre: clase.to_owned(),
            });
        i.apps.borrow_mut().insert(clase.to_owned(), app.clone());
        app
    }

    fn icono(&self, clave: &str) -> ImageData {
        let i = &self.inner;
        if let Some(img) = i.iconos.borrow().get(clave) {
            return img.clone();
        }
        let gicon = gio_unix::DesktopAppInfo::new(clave)
            .and_then(|d| d.icon())
            .unwrap_or_else(|| gio::ThemedIcon::new(&clave.to_lowercase()).upcast());
        let gicon = crate::icono::tiene_gicon(&gicon).then_some(gicon);
        let img = crate::icono::de_gicon(
            gicon.as_ref(),
            LADO_ICONO,
            crate::pixeles::escala() as u32,
        )
        .unwrap_or_else(|| crate::icono::de_pixeles(vec![0; 4], 1, 1));
        i.iconos.borrow_mut().insert(clave.to_owned(), img.clone());
        img
    }

    fn evento(&self, e: Evento) {
        match e {
            Evento::Click(clave) => {
                let direccion = self
                    .inner
                    .piezas
                    .borrow()
                    .iter()
                    .find(|p| p.clave == clave)
                    .and_then(|p| modelo::siguiente(&p.filas).map(str::to_owned));
                match direccion {
                    Some(d) => self.enfocar(d),
                    None => abrir(&clave),
                }
            }
            Evento::Nueva(clave) => abrir(&clave),
            Evento::Enfocar(d) => self.enfocar(d),
            // Abre con los títulos de la última lectura: se ponen al día enseguida.
            Evento::Globo(abierto) => {
                self.inner.globo_abierto.set(abierto);
                if abierto {
                    self.programar();
                }
            }
        }
    }

    /// Enfocar una ventana lleva el puntero a su centro: acá el mouse se queda en el dock, así
    /// que `no_warps` vale solo durante el pedido y vuelve a lo que el usuario tenga puesto.
    fn enfocar(&self, direccion: String) {
        let h = self.inner.hyprland.clone();
        glib::MainContext::default().spawn_local(async move {
            let antes = h
                .consultar("j/getoption cursor:no_warps")
                .await
                .is_some_and(|o| modelo::opcion_activa(&o));
            h.eval(&format!(
                "hl.config({{ cursor = {{ no_warps = true }} }}) \
                 hl.dispatch(hl.dsp.focus({{ window = \"address:{direccion}\" }})) \
                 hl.config({{ cursor = {{ no_warps = {antes} }} }})"
            ))
            .await;
        });
    }
}

/// Una app sin `.desktop` no se puede lanzar.
fn abrir(clave: &str) {
    if clave.ends_with(".desktop") {
        crate::comando::lanzar_app(clave, &[]);
    }
}
