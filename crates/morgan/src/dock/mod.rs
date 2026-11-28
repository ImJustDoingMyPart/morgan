//! El dock: las apps más usadas y las abiertas, abajo, con un punto por ventana, y los íconos de
//! la bandeja. Lo dibuja su propio hilo (`hilo.rs`); acá, en el hilo de GTK, se lee qué hay
//! abierto y se hace lo que pide cada click. Sin sondeo: lee las ventanas cuando Hyprland avisa
//! que algo cambió, y la bandeja cuando cambia un ícono.
//!
//! Un ícono de bandeja se le junta a su app por el proceso que lo publica: el de una de sus
//! ventanas (o el que la lanzó), la unidad de systemd con que se abrió, o su ejecutable. Lo que no
//! es de ninguna app va igual, con el nombre y el ícono del ítem.
//!
//! Las más usadas se eligen al arrancar y no se mueven hasta el próximo arranque: un dock sirve
//! porque cada cosa está siempre en el mismo lugar. El uso se cuenta por ventana abierta, venga
//! de donde venga (`dock-usos`); la primera vez arranca de lo que ya contó el launcher.

mod dibujo;
mod hilo;
pub mod modelo;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gio::prelude::*;
use morgan_motor::color::Paleta;
use morgan_motor::vello::peniko::ImageData;
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::bandeja::{Bandeja, Item};
use crate::barra::hilo::Orden as OrdenBarra;
use crate::centro::Centro;
use crate::centro::vista::{LugarMenu, Menu as VistaMenu, NodoMenu};
use crate::compositor::Compositor;
use crate::config;
use crate::launcher::busqueda::Historial;
use crate::tema::Tema;
use hilo::{Arranque, Datos, Evento, Orden};
use modelo::{Abierta, Conocida, Contador, DeBandeja, Pieza};

/// Se agrupan: una ráfaga de eventos es una sola lectura.
const AGRUPAR: Duration = Duration::from_millis(80);
const LADO_ICONO: u32 = modelo::ICONO as u32;
/// En la barra: el ícono de la tira (el de junto a un workspace, 14, sale de este).
const LADO_BARRA: u32 = 16;
/// La clave de lo que no es de ninguna app: le sigue el `Id` del ítem.
const SIN_APP: &str = "bandeja:";

/// Las filas propias del menú del click derecho. Las de la bandeja tienen su id de dbusmenu, que
/// no es negativo.
const ID_NUEVA: i32 = -2;
const ID_CERRAR_TODAS: i32 = -3;
/// Menos la posición de cada una.
const ID_ACCIONES: i32 = -100;
const ID_VENTANAS: i32 = -1000;
const DESPUES_DE_VENTANAS: i32 = ID_VENTANAS + 1;

#[derive(Clone)]
pub struct Dock {
    inner: Rc<Inner>,
}

struct Inner {
    hilo: Sender<Orden>,
    /// Con el dock en la barra, las apps van a ella.
    barra: Sender<OrdenBarra>,
    en_barra: Cell<bool>,
    /// Las ventanas en el orden en que se abrieron: el de la tira de la barra.
    abiertas: RefCell<Vec<String>>,
    /// La app de la barra con su menú abierto.
    menu_barra: RefCell<Option<String>>,
    /// Lo último que se le mandó a la barra como bandeja: cada ítem, su globo y su imagen.
    bandeja_barra: RefCell<Option<Vec<(String, String, u64)>>>,
    compositor: Compositor,
    /// Las usadas: se eligen al arrancar, y un lugar libre lo ocupa la próxima app que se abra.
    frecuentes: RefCell<Vec<Conocida>>,
    usadas: Cell<usize>,
    usos: RefCell<Historial>,
    archivo: Option<PathBuf>,
    contador: RefCell<Contador>,
    /// El orden que eligió el usuario arrastrando (`dock-orden`); vacío, el de siempre.
    orden: RefCell<Vec<String>>,
    piezas: RefCell<Vec<Pieza>>,
    /// De clase a app: buscar el `.desktop` de una clase recorre todos.
    /// `None`: un programa auxiliar (`aplicacion::es_auxiliar`), que el dock no muestra.
    apps: RefCell<HashMap<String, Option<Conocida>>>,
    iconos: RefCell<HashMap<String, ImageData>>,
    programado: Cell<bool>,
    globo_abierto: Cell<bool>,
    escuchar: RefCell<Option<glib::JoinHandle<()>>>,
    bandeja: Bandeja,
    centro: Centro,
    iconos_bandeja: RefCell<BTreeMap<String, String>>,
    /// La app de cada ítem por su proceso; no cambia mientras el ítem vive.
    de_proceso: RefCell<HashMap<String, Option<Conocida>>>,
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

fn archivo_orden() -> PathBuf {
    glib::user_state_dir().join("morgan").join("dock-orden")
}

/// Una clave por línea.
fn leer_orden() -> Vec<String> {
    std::fs::read_to_string(archivo_orden())
        .map(|t| t.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default()
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
    pub fn new(
        compositor: Compositor,
        tema: &Tema,
        c: &config::Dock,
        bandeja: Bandeja,
        centro: &Centro,
        barra: Sender<OrdenBarra>,
    ) -> Self {
        bandeja.poner_en_dock(c.mostrar);
        let (css, opacidad, borde) = tema.actual();
        let (hilo, eventos) = hilo::lanzar(Arranque {
            paleta: Paleta::desde_css(&css),
            opacidad: opacidad as f32,
            borde,
            mostrar: c.abajo(),
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
                barra,
                en_barra: Cell::new(c.arriba()),
                abiertas: RefCell::default(),
                menu_barra: RefCell::default(),
                bandeja_barra: RefCell::default(),
                compositor: compositor.clone(),
                frecuentes: RefCell::new(frecuentes),
                usadas: Cell::new(c.usadas),
                usos: RefCell::new(usos),
                archivo: Some(archivo),
                contador: RefCell::default(),
                orden: RefCell::new(leer_orden()),
                piezas: RefCell::default(),
                apps: RefCell::default(),
                iconos: RefCell::default(),
                programado: Cell::new(false),
                globo_abierto: Cell::new(false),
                escuchar: RefCell::default(),
                bandeja: bandeja.clone(),
                centro: centro.clone(),
                iconos_bandeja: RefCell::new(c.iconos_bandeja.clone()),
                de_proceso: RefCell::default(),
            }),
        };
        let d = dock.clone();
        bandeja.al_cambiar(move || d.programar());

        let d = dock.clone();
        let filtro = RefCell::new(modelo::Filtro);
        compositor.al_ventanas(move |cambio| {
            if filtro
                .borrow_mut()
                .importa(&cambio, d.inner.globo_abierto.get())
            {
                d.programar();
            }
        });
        let d = dock.clone();
        compositor.al_cambiar_espacios(move |_| d.programar());
        let t = dock.inner.hilo.clone();
        let _ = t.send(Orden::Juego(compositor.juego_en_foco()));
        compositor.al_cambiar(move |juego| {
            let _ = t.send(Orden::Juego(juego));
        });

        let d = dock.clone();
        let escuchar = glib::MainContext::default().spawn_local(async move {
            while let Ok(e) = eventos.recv().await {
                d.evento(e);
            }
        });
        dock.inner.escuchar.replace(Some(escuchar));
        dock.disposicion_barra(c);
        dock.programar();
        dock
    }

    /// Cómo van las apps y la bandeja con el dock en la barra.
    fn disposicion_barra(&self, c: &config::Dock) {
        let _ = self.inner.barra.send(OrdenBarra::DockEnBarra {
            activo: c.arriba(),
            por_espacio: c.por_espacio,
            bandeja_fija: c.arriba() && !c.cajon,
        });
    }

    pub fn aplicar_config(&self, c: &config::Dock) {
        let i = &self.inner;
        i.bandeja.poner_en_dock(c.mostrar);
        self.disposicion_barra(c);
        if i.en_barra.replace(c.arriba()) != c.arriba() {
            self.a_la_barra(&i.piezas.borrow());
            // La bandeja de la barra cambia de íconos (o se vacía): se vuelve a mandar.
            i.bandeja_barra.replace(None);
            self.programar();
        }
        // Otra cantidad se elige de nuevo por uso, como al arrancar.
        if i.usadas.replace(c.usadas) != c.usadas {
            let nuevas = modelo::mas_usadas(&i.usos.borrow().ordenados(), nombre_de, c.usadas);
            i.frecuentes.replace(nuevas);
            self.programar();
        }
        if *self.inner.iconos_bandeja.borrow() != c.iconos_bandeja {
            self.inner.iconos_bandeja.replace(c.iconos_bandeja.clone());
            self.inner.iconos.borrow_mut().clear();
            self.inner.piezas.borrow_mut().clear();
            self.programar();
        }
        let _ = self.inner.hilo.send(Orden::Config {
            mostrar: c.abajo(),
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
        let Some(clientes) = i.compositor.clientes().await else {
            return;
        };
        let abiertas = modelo::de_clientes(&clientes);
        self.contar(&abiertas);
        i.abiertas
            .replace(abiertas.iter().map(|a| a.direccion.clone()).collect());
        let con_app: Vec<(Conocida, Abierta)> = abiertas
            .into_iter()
            .filter_map(|a| Some((self.app_de(&a.clase)?, a)))
            .collect();
        let activos: BTreeSet<i64> = i
            .compositor
            .espacios()
            .activos
            .iter()
            .map(|(_, id)| *id)
            .collect();
        let bandeja = self.de_bandeja(&con_app).await;
        self.bandeja_a_la_barra(&bandeja);
        let mut piezas = modelo::armar(&i.frecuentes.borrow(), &con_app, &activos, &bandeja);
        modelo::ordenar(&mut piezas, &i.orden.borrow());
        if *i.piezas.borrow() == piezas {
            return;
        }
        let iconos = piezas.iter().map(|p| self.icono(p, LADO_ICONO)).collect();
        i.piezas.replace(piezas.clone());
        self.a_la_barra(&piezas);
        let _ = i.hilo.send(Orden::Apps(Datos { piezas, iconos }));
    }

    /// Con el dock en la barra, las apps abiertas, en el orden en que se abrieron; si no,
    /// ninguna.
    fn a_la_barra(&self, piezas: &[Pieza]) {
        let i = &self.inner;
        let mut apps: Vec<(usize, crate::barra::dibujo::App)> = Vec::new();
        if i.en_barra.get() {
            let abiertas = i.abiertas.borrow();
            let lugar = |d: &str| abiertas.iter().position(|a| a == d).unwrap_or(usize::MAX);
            for p in piezas.iter().filter(|p| !p.filas.is_empty()) {
                let primera = p.filas.iter().map(|f| lugar(&f.direccion)).min();
                let ventanas = p
                    .filas
                    .iter()
                    .map(|f| crate::barra::dibujo::Ventana {
                        ws: f.ws,
                        foco: f.foco,
                    })
                    .collect();
                apps.push((
                    primera.unwrap_or(usize::MAX),
                    crate::barra::dibujo::App {
                        clave: p.clave.clone(),
                        nombre: p.nombre.clone(),
                        imagen: Some(self.icono(p, LADO_BARRA)),
                        ventanas,
                    },
                ));
            }
        }
        apps.sort_by_key(|(k, _)| *k);
        let apps = apps.into_iter().map(|(_, a)| a).collect();
        let _ = i.barra.send(OrdenBarra::Apps(apps));
    }

    /// La bandeja de la barra, mientras el dock la tiene: vacía con el dock abajo; con el dock
    /// en la barra, el ícono de color de la app de cada ítem (el mismo del dock), por
    /// consistencia con las apps. Solo si cambió: cada envío rehace los textos de la barra.
    fn bandeja_a_la_barra(&self, bandeja: &[DeBandeja]) {
        let i = &self.inner;
        if !i.bandeja.en_dock() {
            i.bandeja_barra.replace(None);
            return;
        }
        let iconos: Vec<crate::barra::dibujo::Icono> = if i.en_barra.get() {
            bandeja
                .iter()
                .map(|b| {
                    let p = Pieza {
                        clave: b.app.clave.clone(),
                        nombre: b.app.nombre.clone(),
                        filas: Vec::new(),
                        marcas: Vec::new(),
                        foco: false,
                        seccion: modelo::Seccion::Bandeja,
                        bandeja: Some(b.item.clone()),
                    };
                    crate::barra::dibujo::Icono {
                        clave: b.item.clone(),
                        tooltip: i.bandeja.item(&b.item).map(|it| it.tooltip).unwrap_or_default(),
                        imagen: Some(self.icono(&p, crate::barra::dibujo::LADO_BANDEJA)),
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let firma: Vec<(String, String, u64)> = iconos
            .iter()
            .map(|c| {
                let id = c.imagen.as_ref().map_or(0, |m| m.data.id());
                (c.clave.clone(), c.tooltip.clone(), id)
            })
            .collect();
        if i.bandeja_barra.borrow().as_ref() == Some(&firma) {
            return;
        }
        i.bandeja_barra.replace(Some(firma));
        let _ = i.barra.send(OrdenBarra::Bandeja(iconos));
    }

    /// Un click en una app de la barra. `ws`: junto a qué workspace está, si van agrupadas; el
    /// click va a sus ventanas de ahí.
    pub fn click_barra(
        &self,
        clave: &str,
        ws: Option<i64>,
        boton: u32,
        salida: String,
        izquierda: i32,
    ) {
        let i = &self.inner;
        // Con un menú abierto, cualquier click en una app lo cierra; en la suya, nada más.
        let abierto = i.menu_barra.borrow_mut().take();
        if let Some(c) = abierto {
            i.centro.cerrar_menu();
            if c == clave {
                return;
            }
        }
        let Some(p) = self.pieza(clave) else { return };
        let filas: Vec<_> = p
            .filas
            .iter()
            .filter(|f| ws.is_none_or(|w| f.ws == w))
            .cloned()
            .collect();
        let menu = || {
            let d = self.clone();
            let cerrado = move || {
                d.inner.menu_barra.borrow_mut().take();
                let _ = d.inner.barra.send(OrdenBarra::MenuApp(None));
            };
            i.menu_barra.replace(Some(clave.to_owned()));
            let _ = i.barra.send(OrdenBarra::MenuApp(Some(clave.to_owned())));
            self.menu(
                clave,
                salida.clone(),
                LugarMenu::BarraIzq { izquierda },
                cerrado,
            );
        };
        match boton {
            1 => {
                if let Some(d) = modelo::siguiente(&filas) {
                    self.enfocar(d.to_owned());
                }
            }
            // Con una ventana la cierra; con varias, el menú dice cuál.
            2 if filas.len() == 1 => {
                let (c, d) = (i.compositor.clone(), filas[0].direccion.clone());
                glib::MainContext::default().spawn_local(async move { c.cerrar(&d).await });
            }
            2 | 3 => menu(),
            _ => {}
        }
    }

    /// Vuelve al orden de siempre: las más usadas primero.
    pub fn restaurar_orden(&self) {
        self.inner.orden.borrow_mut().clear();
        let _ = std::fs::remove_file(archivo_orden());
        self.programar();
    }

    /// Guarda el orden que quedó al soltar una app (`nuevo`: las que se ven).
    fn ordenar(&self, nuevo: &[String]) {
        let i = &self.inner;
        let piezas: Vec<String> = i.piezas.borrow().iter().map(|p| p.clave.clone()).collect();
        let orden = modelo::reordenar(&i.orden.borrow(), &piezas, nuevo);
        crate::launcher::guardar(&archivo_orden(), &(orden.join("\n") + "\n"));
        i.orden.replace(orden);
        self.programar();
    }

    fn contar(&self, abiertas: &[Abierta]) {
        let i = &self.inner;
        let padre = |p: u32| crate::compositor::leer_proc(p).map(|(padre, _)| padre);
        let nuevas: Vec<String> = i
            .contador
            .borrow_mut()
            .nuevas(abiertas)
            .into_iter()
            // Cuenta lo que se abre como tal: lo que abre otra app no es elegirla.
            .filter(|a| !modelo::abierta_por_otra(a, abiertas, padre))
            .map(|a| a.clase.clone())
            .collect();
        let mut sumo = false;
        for clase in nuevas {
            let Some(clave) = self.app_de(&clase).map(|a| a.clave) else {
                continue;
            };
            // Sin `.desktop` no se la podría volver a abrir: no compite por un lugar.
            if clave.ends_with(".desktop") {
                i.usos.borrow_mut().sumar(&clave);
                sumo = true;
                if let Some(nombre) = nombre_de(&clave) {
                    let app = Conocida { clave, nombre };
                    modelo::ocupar_libre(&mut i.frecuentes.borrow_mut(), app, i.usadas.get());
                }
            }
        }
        if sumo && let Some(a) = &i.archivo {
            crate::launcher::guardar(a, &i.usos.borrow().texto());
        }
    }

    fn app_de(&self, clase: &str) -> Option<Conocida> {
        let i = &self.inner;
        if let Some(a) = i.apps.borrow().get(clase) {
            return a.clone();
        }
        let app = match crate::aplicacion::de_clase(clase) {
            Some(d) if crate::aplicacion::es_auxiliar(&d) => None,
            d => Some(d.as_ref().and_then(conocida).unwrap_or_else(|| Conocida {
                clave: clase.to_owned(),
                nombre: clase.to_owned(),
            })),
        };
        i.apps.borrow_mut().insert(clase.to_owned(), app.clone());
        app
    }

    /// Los íconos de la bandeja que van al dock, cada uno con su app.
    async fn de_bandeja(&self, abiertas: &[(Conocida, Abierta)]) -> Vec<DeBandeja> {
        let i = &self.inner;
        let padre = |p: u32| crate::compositor::leer_proc(p).map(|(padre, _)| padre);
        let mut v = Vec::new();
        for item in i.bandeja.items() {
            let pid = i.bandeja.pid(&item.clave).await;
            let app = pid
                .and_then(|p| modelo::de_ventanas(p, abiertas, padre))
                .or_else(|| self.de_proceso(&item, pid))
                .unwrap_or_else(|| sin_app(&item));
            v.push(DeBandeja {
                item: item.clave,
                app,
            });
        }
        v
    }

    fn de_proceso(&self, item: &Item, pid: Option<u32>) -> Option<Conocida> {
        let i = &self.inner;
        if let Some(c) = i.de_proceso.borrow().get(&item.clave) {
            return c.clone();
        }
        let app = pid.and_then(app_de_proceso);
        let mut cache = i.de_proceso.borrow_mut();
        // Lo de los ítems que ya no están no sirve más.
        let vivos: Vec<String> = i.bandeja.items().into_iter().map(|x| x.clave).collect();
        cache.retain(|k, _| vivos.contains(k));
        cache.insert(item.clave.clone(), app.clone());
        app
    }

    fn icono(&self, p: &Pieza, lado: u32) -> ImageData {
        let i = &self.inner;
        let item = p.bandeja.as_deref().and_then(|c| i.bandeja.item(c));
        let forzado = item
            .as_ref()
            .and_then(|it| i.iconos_bandeja.borrow().get(&it.id).cloned());
        // Lo de la bandeja puede cambiar de ícono: va en la clave.
        let clave_cache = match (&forzado, &item) {
            (Some(f), _) => format!("{lado}|{}|{f}", p.clave),
            (None, Some(it)) if p.clave.starts_with(SIN_APP) => {
                format!("{lado}|{}|{}|{}", p.clave, it.icono, it.pixmap.is_some())
            }
            _ => format!("{lado}|{}", p.clave),
        };
        if let Some(img) = i.iconos.borrow().get(&clave_cache) {
            return img.clone();
        }
        let escala = crate::pixeles::escala().max(1) as u32;
        let img = match (&forzado, &item) {
            (Some(f), _) => icono_por_nombre(f, lado, escala),
            (None, Some(it)) if p.clave.starts_with(SIN_APP) => icono_de_item(it, lado, escala),
            _ => None,
        }
        .unwrap_or_else(|| self.icono_de_app(&p.clave, lado));
        i.iconos.borrow_mut().insert(clave_cache, img.clone());
        img
    }

    fn icono_de_app(&self, clave: &str, lado: u32) -> ImageData {
        let gicon = gio_unix::DesktopAppInfo::new(clave)
            .and_then(|d| d.icon())
            .unwrap_or_else(|| gio::ThemedIcon::new(&clave.to_lowercase()).upcast());
        let gicon = crate::icono::tiene_gicon(&gicon).then_some(gicon);
        crate::icono::de_gicon(gicon.as_ref(), lado, crate::pixeles::escala() as u32)
            .unwrap_or_else(|| crate::icono::de_pixeles(vec![0; 4], 1, 1))
    }

    fn pieza(&self, clave: &str) -> Option<Pieza> {
        self.inner
            .piezas
            .borrow()
            .iter()
            .find(|p| p.clave == clave)
            .cloned()
    }

    fn evento(&self, e: Evento) {
        match e {
            Evento::Click(clave) => {
                let Some(p) = self.pieza(&clave) else {
                    return abrir(&clave);
                };
                if let Some(d) = modelo::siguiente(&p.filas) {
                    return self.enfocar(d.to_owned());
                }
                // De fondo: que la app muestre su ventana. Si no sabe (Steam), se la vuelve a
                // abrir, que es lo que hace una app que ya corre.
                match p.bandeja {
                    Some(item) => {
                        let b = self.inner.bandeja.clone();
                        glib::MainContext::default().spawn_local(async move {
                            if !b.activar(&item).await {
                                abrir(&clave);
                            }
                        });
                    }
                    None => abrir(&clave),
                }
            }
            Evento::Menu {
                clave,
                salida,
                centro,
                abajo,
                n,
            } => {
                let hilo = self.inner.hilo.clone();
                let cerrado = move || {
                    let _ = hilo.send(Orden::MenuCerrado(n));
                };
                self.menu(&clave, salida, LugarMenu::Dock { centro, abajo }, cerrado);
            }
            Evento::Enfocar(d) => self.enfocar(d),
            Evento::Cerrar(d) => {
                let c = self.inner.compositor.clone();
                glib::MainContext::default().spawn_local(async move { c.cerrar(&d).await });
            }
            Evento::Ordenar(nuevo) => self.ordenar(&nuevo),
            // Abre con los títulos de la última lectura: se ponen al día enseguida.
            Evento::Globo(abierto) => {
                self.inner.globo_abierto.set(abierto);
                if abierto {
                    self.programar();
                }
            }
        }
    }

    /// El mouse se queda en el dock: el puntero no salta a la ventana.
    fn enfocar(&self, direccion: String) {
        let c = self.inner.compositor.clone();
        glib::MainContext::default().spawn_local(async move {
            c.enfocar(&direccion, true).await;
        });
    }

    /// El menú del click derecho: las ventanas, lo que se puede hacer con la app y lo de su
    /// ícono de bandeja, en ese orden.
    fn menu(&self, clave: &str, salida: String, lugar: LugarMenu, cerrado: impl Fn() + 'static) {
        let Some(p) = self.pieza(clave) else {
            return cerrado();
        };
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let i = &yo.inner;
            let mut secciones: Vec<Vec<NodoMenu>> = Vec::new();
            if !p.filas.is_empty() {
                secciones.push(
                    p.filas
                        .iter()
                        .enumerate()
                        .map(|(k, f)| NodoMenu {
                            prefijo: Some((f.espacio.clone(), f.foco)),
                            ..fila(ID_VENTANAS - k as i32, &f.titulo, true)
                        })
                        .collect(),
                );
            }
            let app = gio_unix::DesktopAppInfo::new(&p.clave).filter(|d| d.should_show());
            let acciones: Vec<(String, String)> = app
                .as_ref()
                .map(|d| {
                    d.list_actions()
                        .iter()
                        .map(|a| (a.to_string(), d.action_name(a).to_string()))
                        .collect()
                })
                .unwrap_or_default();
            if app.is_some() {
                let mut v = vec![fila(ID_NUEVA, tr!("Ventana nueva"), true)];
                v.extend(
                    acciones
                        .iter()
                        .enumerate()
                        .map(|(k, (_, nombre))| fila(ID_ACCIONES - k as i32, nombre, true)),
                );
                v.push(fila(ID_CERRAR_TODAS, tr!("Cerrar todas"), !p.filas.is_empty()));
                secciones.push(v);
            }
            if let Some(item) = &p.bandeja
                && let Some(raiz) = i.bandeja.menu(item).await
            {
                secciones.extend(crate::barra::bandeja::nodos(&raiz.hijos));
            }
            if secciones.is_empty() {
                return cerrado();
            }
            let m = VistaMenu {
                salida,
                lugar,
                titulo: Some(p.nombre.clone()),
                secciones,
            };
            let d = yo.clone();
            let elegir = move |id: i32| match id {
                0.. => {
                    if let Some(item) = &p.bandeja {
                        d.inner.bandeja.elegir(item, id);
                    }
                }
                ID_NUEVA => abrir(&p.clave),
                ID_CERRAR_TODAS => {
                    for f in &p.filas {
                        let (c, dir) = (d.inner.compositor.clone(), f.direccion.clone());
                        glib::MainContext::default()
                            .spawn_local(async move { c.cerrar(&dir).await });
                    }
                }
                ..=ID_VENTANAS => {
                    if let Some(f) = p.filas.get((ID_VENTANAS - id) as usize) {
                        d.enfocar(f.direccion.clone());
                    }
                }
                DESPUES_DE_VENTANAS..=ID_ACCIONES => {
                    if let Some((accion, _)) = acciones.get((ID_ACCIONES - id) as usize) {
                        crate::comando::lanzar_accion(&p.clave, accion);
                    }
                }
                _ => {}
            };
            i.centro.abrir_menu(m, elegir, cerrado);
        });
    }
}

fn fila(id: i32, etiqueta: &str, habilitado: bool) -> NodoMenu {
    NodoMenu {
        id,
        etiqueta: etiqueta.to_owned(),
        habilitado,
        marca: None,
        hijos: Vec::new(),
        prefijo: None,
    }
}

/// Una app sin `.desktop` no se puede lanzar.
fn abrir(clave: &str) {
    if clave.ends_with(".desktop") {
        crate::comando::lanzar_app(clave, &[]);
    }
}

/// Lo que no es de ninguna app: su nombre es el del ítem.
fn sin_app(item: &Item) -> Conocida {
    let nombre = if item.titulo.is_empty() {
        item.id.clone()
    } else {
        item.titulo.clone()
    };
    Conocida {
        clave: format!("{SIN_APP}{}", item.id),
        nombre,
    }
}

fn conocida(d: &gio_unix::DesktopAppInfo) -> Option<Conocida> {
    Some(Conocida {
        clave: d.id()?.to_string(),
        nombre: d.display_name().to_string(),
    })
}

/// La app de un proceso por la unidad de systemd con que se lanzó o, si no, por su ejecutable.
/// Solo apps que se muestran: un `.desktop` escondido (el de autoarranque de una bandeja) no es
/// una app que se pueda abrir.
fn app_de_proceso(pid: u32) -> Option<Conocida> {
    let base = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
    let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|r| base(&r));
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|c| c.trim().to_owned());
    let es_suya = |d: &gio_unix::DesktopAppInfo| {
        let e = base(&d.executable());
        e.is_some() && (e == exe || e == comm)
    };
    let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap_or_default();
    if let Some((ids, scope)) = modelo::apps_de_unidad(&cgroup) {
        for id in ids {
            if let Some(d) = gio_unix::DesktopAppInfo::new(&id)
                && d.should_show()
                && (!scope || es_suya(&d))
            {
                return conocida(&d);
            }
        }
    }
    gio::AppInfo::all()
        .into_iter()
        .filter_map(|a| a.downcast::<gio_unix::DesktopAppInfo>().ok())
        .find(|d| d.should_show() && es_suya(d))
        .and_then(|d| conocida(&d))
}

/// Un ícono por nombre del tema (o ruta), de `lado` lógicos.
fn icono_por_nombre(nombre: &str, lado: u32, escala: u32) -> Option<ImageData> {
    let ruta = if nombre.starts_with('/') {
        Some(std::path::PathBuf::from(nombre))
    } else {
        crate::icono::buscar(nombre, lado, escala)
    }?;
    crate::icono::dibujar(&ruta, lado * escala, None)
}

/// El ícono que manda el ítem: por nombre (en el tema o en su carpeta) o en píxeles.
fn icono_de_item(item: &Item, lado: u32, escala: u32) -> Option<ImageData> {
    let nombre = if item.atencion && !item.icono_atencion.is_empty() {
        &item.icono_atencion
    } else {
        &item.icono
    };
    if !nombre.is_empty() {
        if let Some(img) = icono_por_nombre(nombre, lado, escala) {
            return Some(img);
        }
        for ext in ["svg", "png"] {
            let ruta = std::path::Path::new(&item.ruta_tema).join(format!("{nombre}.{ext}"));
            if !item.ruta_tema.is_empty()
                && ruta.is_file()
                && let Some(img) = crate::icono::dibujar(&ruta, lado * escala, None)
            {
                return Some(img);
            }
        }
    }
    let (w, h, rgba) = item.pixmap.as_ref()?;
    let i = image::RgbaImage::from_raw(
        u32::try_from(*w).ok()?,
        u32::try_from(*h).ok()?,
        rgba.clone(),
    )?;
    Some(crate::icono::de_rgba(&i, lado * escala))
}
