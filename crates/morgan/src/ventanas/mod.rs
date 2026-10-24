//! Hyprland sabe cuándo se suelta `Mod`, morgan no: lo que GTK ve de los modificadores al
//! tomar el teclado no es confiable. El bind de `Mod+Tab` pone el submap [`SUBMAP`] y pide el
//! lienzo; adentro, `Mod+Tab` pide un paso más y soltar `Super_L`/`Super_R` sale del submap y
//! avisa. Los avisos llegan por el socket de eventos (`ipc::de_evento`), en orden.
//!
//! Un toque enfoca la anterior sin dibujar nada; la capa se pone recién a los [`DEMORA`], así
//! el teclado sigue en la ventana enfocada. El submap no tiene binds de flechas: si no, los
//! `Mod+flecha` de la cinta se los quedaría el compositor (los binds le ganan a una capa con
//! teclado exclusivo).
//!
//! Hyprland no entrega cuadros de una ventana que no toca su monitor, así que morgan saca una
//! foto cada vez que una ventana recibe el foco y, con `Mod` apretado [`RENOVAR`], renueva a
//! la vez las que tocan su monitor. Las de un workspace oculto dan siempre el mismo cuadro: con
//! una foto de después de esconderse, no se piden de nuevo.
//!
//! Se dibuja en el hilo del centro (`centro/dibujo/ventanas.rs`), que lleva la elegida, el
//! desplazamiento y el anillo; acá quedan lo que dice Hyprland, las fotos, los íconos y enfocar.

mod captura;
pub(crate) mod modelo;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gio::prelude::*;
use morgan_motor::vello::peniko::ImageData;
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::centro::Centro;
use crate::centro::hilo::Orden;
use crate::centro::vista::{EventoVentanas, VistaVentanas};
use crate::hyprland::Hyprland;
use captura::Capturador;
use modelo::Lienzo;

const SUBMAP: &str = "morgan-windows";
/// Un toque no lo dibuja.
const DEMORA: Duration = Duration::from_millis(150);
/// Antes de la [`DEMORA`], para que lleguen con el lienzo. Un toque más corto no captura nada:
/// cada captura cuenta como `screencast` para el aviso de privacidad.
const RENOVAR: Duration = Duration::from_millis(100);
/// La animación de la cinta: la foto se saca con la ventana ya en su lugar.
const ASENTARSE: Duration = Duration::from_millis(350);
/// Alcanza para la franja más alta en doble densidad.
const ANCHO_FOTO: i32 = 480;
/// El ancho de la miniatura de la pantalla (el fondo), en píxeles.
const ANCHO_FONDO: u32 = 640;
/// El lado de los íconos de las apps, en lógicos.
const LADO_ICONO: u32 = 48;

static TRAZA: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| std::env::var_os("MORGAN_TRAZA").is_some());

fn traza(t0: Instant, que: &str) {
    if *TRAZA {
        eprintln!(
            "morgan: ventanas: {:7.1} ms  {que}",
            t0.elapsed().as_secs_f64() * 1000.0
        );
    }
}

#[derive(Clone)]
pub struct Ventanas {
    inner: Rc<Inner>,
}

struct Inner {
    hyprland: Hyprland,
    hilo: Sender<Orden>,
    capturador: RefCell<Option<Rc<Capturador>>>,
    fotos: RefCell<HashMap<String, Foto>>,
    abierta: RefCell<Option<Rc<Vista>>>,
    /// Sube con cada pedido: una apertura que termina de leer tarde no abre si mientras tanto se
    /// pidió cerrar.
    pedido: Cell<u64>,
    leyendo: Cell<bool>,
    pasos: Cell<usize>,
    soltado: Cell<bool>,
    /// Sube con cada foco: solo se fotografía la que lo conserva al asentarse.
    enfoque: Cell<u64>,
    fondo: RefCell<Option<(String, ImageData)>>,
    /// Buscarlo puede recorrer todos los `.desktop` en el hilo principal: se busca una vez, al
    /// enfocar la ventana.
    iconos: RefCell<HashMap<String, ImageData>>,
}

impl Ventanas {
    pub fn new(hyprland: Hyprland, centro: &Centro) -> Self {
        let v = Ventanas {
            inner: Rc::new(Inner {
                hyprland: hyprland.clone(),
                hilo: centro.hilo(),
                capturador: RefCell::default(),
                fotos: RefCell::default(),
                abierta: RefCell::default(),
                pedido: Cell::new(0),
                leyendo: Cell::new(false),
                pasos: Cell::new(0),
                soltado: Cell::new(false),
                enfoque: Cell::new(0),
                fondo: RefCell::default(),
                iconos: RefCell::default(),
            }),
        };
        let debil = Rc::downgrade(&v.inner);
        centro.al_ventanas(move |e| {
            if let Some(inner) = debil.upgrade() {
                let v = Ventanas { inner };
                match e {
                    EventoVentanas::Enfocar { direccion, submap } => v.enfocar(direccion, submap),
                    EventoVentanas::Cerrada => v.cerrar(),
                }
            }
        });
        // Un morgan que se cayó con el lienzo abierto pudo dejar a Hyprland en el submap:
        // lo primero es salir de ahí.
        let h = hyprland.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Some(actual) = h.consultar("submap").await
                && actual.trim() == SUBMAP
            {
                h.eval("hl.dispatch(hl.dsp.submap(\"reset\"))").await;
            }
        });
        let debil = Rc::downgrade(&v.inner);
        hyprland.al_cambiar_espacios(move |e| {
            if let Some(inner) = debil.upgrade() {
                for f in inner.fotos.borrow_mut().values_mut() {
                    if f.congelada.is_some_and(|ws| e.activo(ws)) {
                        f.congelada = None;
                    }
                }
            }
        });
        let debil = Rc::downgrade(&v.inner);
        hyprland.al_enfocar(move |direccion| {
            if let Some(inner) = debil.upgrade() {
                Ventanas { inner }.al_enfocar(direccion.to_owned());
            }
        });
        v
    }

    pub fn abierto(&self) -> bool {
        self.inner.abierta.borrow().is_some()
    }

    pub fn pedir(&self) {
        // Clonada aparte: el préstamo del `if let` duraría todo el bloque.
        let abierta = self.inner.abierta.borrow().clone();
        if let Some(v) = abierta {
            v.reciente();
            return;
        }
        let i = &self.inner;
        if i.leyendo.get() {
            i.pasos.set(i.pasos.get() + 1);
            return;
        }
        // Los avisos llegan en orden: un soltar de antes es de otra vez.
        i.soltado.set(false);
        let pedido = i.pedido.get() + 1;
        i.pedido.set(pedido);
        i.leyendo.set(true);
        i.pasos.set(0);
        let yo = self.clone();
        let t0 = Instant::now();
        glib::MainContext::default().spawn_local(async move {
            let lienzo = yo.leer().await;
            traza(t0, "leído");
            let i = &yo.inner;
            i.leyendo.set(false);
            if i.pedido.get() != pedido || yo.abierto() {
                return;
            }
            let Some(lienzo) = lienzo else { return };
            let Some(mut sel) = lienzo.inicial() else {
                return;
            };
            for _ in 0..i.pasos.take() {
                sel = lienzo.reciente(sel);
            }
            // Ya se soltó: un toque. Se enfoca sin armar nada.
            if i.soltado.take() {
                yo.enfocar(lienzo.ventanas[sel].direccion.clone(), false);
                return;
            }
            let vista = Vista::abrir(&yo, lienzo, sel, t0);
            traza(t0, "armado");
            i.abierta.replace(Some(vista));
        });
    }

    pub fn soltar(&self) {
        // Clonada aparte: `confirmar` la vacía, y el préstamo del `if let` seguiría vivo.
        let abierta = self.inner.abierta.borrow().clone();
        if let Some(v) = abierta {
            v.confirmar();
            return;
        }
        self.inner.soltado.set(true);
    }

    pub fn cerrar(&self) {
        self.inner.pedido.set(self.inner.pedido.get() + 1);
        self.inner.leyendo.set(false);
        if self.inner.abierta.take().is_some() {
            let _ = self.inner.hilo.send(Orden::VentanasCerrar);
            self.salir_del_submap();
        }
    }

    fn enfocar(&self, direccion: String, submap: bool) {
        self.inner.pedido.set(self.inner.pedido.get() + 1);
        if self.inner.abierta.take().is_some() {
            let _ = self.inner.hilo.send(Orden::VentanasCerrar);
        }
        let h = self.inner.hyprland.clone();
        glib::MainContext::default().spawn_local(async move {
            h.eval(&format!(
                "hl.dispatch(hl.dsp.focus({{ window = \"address:{direccion}\" }}))"
            ))
            .await;
        });
        if submap {
            self.salir_del_submap();
        }
    }

    fn salir_del_submap(&self) {
        let h = self.inner.hyprland.clone();
        glib::MainContext::default().spawn_local(async move {
            h.eval("hl.dispatch(hl.dsp.submap(\"reset\"))").await;
        });
    }

    async fn leer(&self) -> Option<Lienzo> {
        let h = &self.inner.hyprland;
        let clientes = h.consultar("j/clients").await?;
        let monitores = h.consultar("j/monitors").await?;
        match modelo::leer(&clientes, &monitores) {
            Ok(l) => {
                // Las fotos de ventanas que ya no existen no se guardan más.
                self.inner
                    .fotos
                    .borrow_mut()
                    .retain(|d, _| l.ventanas.iter().any(|v| v.direccion == *d));
                Some(l)
            }
            Err(e) => {
                eprintln!("morgan: ventanas: j/clients ilegible: {e}");
                None
            }
        }
    }

    fn capturador(&self) -> Option<Rc<Capturador>> {
        if self.inner.capturador.borrow().is_none() {
            match Capturador::conectar() {
                Ok(c) => {
                    self.inner.capturador.replace(Some(c));
                }
                Err(e) => {
                    eprintln!("morgan: ventanas: sin fotos: {e}");
                    return None;
                }
            }
        }
        self.inner.capturador.borrow().clone()
    }

    async fn fotografiar(&self, v: &modelo::Ventana) -> Option<ImageData> {
        if !v.capturable() {
            return None;
        }
        let foto = self
            .capturador()?
            .capturar(&v.direccion, ANCHO_FOTO)
            .await?
            .imagen();
        let oculta = !self.inner.hyprland.espacios().activo(v.ws);
        self.inner.fotos.borrow_mut().insert(
            v.direccion.clone(),
            Foto {
                imagen: foto.clone(),
                congelada: oculta.then_some(v.ws),
            },
        );
        Some(foto)
    }

    /// Con un juego en foco no: nada se mete con él. Pasar por varias ventanas seguidas
    /// fotografía solo la última.
    fn al_enfocar(&self, direccion: String) {
        let enfoque = self.inner.enfoque.get() + 1;
        self.inner.enfoque.set(enfoque);
        if direccion.len() <= 2 || self.abierto() {
            return;
        }
        let yo = self.clone();
        glib::timeout_add_local_once(ASENTARSE, move || {
            if yo.inner.enfoque.get() != enfoque
                || yo.inner.hyprland.juego_en_foco()
                || yo.abierto()
            {
                return;
            }
            glib::MainContext::default().spawn_local(async move {
                let Some(l) = yo.leer().await else { return };
                if let Some(v) = l.ventanas.iter().find(|v| v.direccion == direccion) {
                    yo.icono(&v.clase);
                    yo.fotografiar(v).await;
                }
            });
        });
    }

    fn icono(&self, clase: &str) -> ImageData {
        if let Some(i) = self.inner.iconos.borrow().get(clase) {
            return i.clone();
        }
        let icono = icono_de(clase);
        let img = crate::icono::de_gicon(
            icono.as_ref(),
            LADO_ICONO,
            crate::pixeles::escala() as u32,
        )
        .unwrap_or_else(|| crate::icono::de_pixeles(vec![0; 4], 1, 1));
        self.inner
            .iconos
            .borrow_mut()
            .insert(clase.to_owned(), img.clone());
        img
    }

    fn fondo_guardado(&self) -> Option<ImageData> {
        self.inner.fondo.borrow().as_ref().map(|(_, t)| t.clone())
    }

    async fn fondo(&self) -> Option<ImageData> {
        let ruta = crate::fondos::actual()?.to_string_lossy().into_owned();
        if let Some((r, t)) = self.inner.fondo.borrow().as_ref()
            && *r == ruta
        {
            return Some(t.clone());
        }
        let r = ruta.clone();
        let img = gio::spawn_blocking(move || {
            let i = image::open(&r).ok()?.into_rgba8();
            let i = if i.width() > ANCHO_FONDO {
                let alto = (u64::from(i.height()) * u64::from(ANCHO_FONDO) / u64::from(i.width()))
                    .max(1) as u32;
                image::imageops::resize(&i, ANCHO_FONDO, alto, image::imageops::FilterType::Triangle)
            } else {
                i
            };
            Some(crate::icono::de_imagen(&i))
        })
        .await
        .ok()
        .flatten()?;
        self.inner.fondo.replace(Some((ruta, img.clone())));
        Some(img)
    }
}

struct Foto {
    imagen: ImageData,
    /// Sacada con su workspace oculto: Hyprland da ese mismo cuadro hasta que se vuelva a ver.
    congelada: Option<i64>,
}

/// El lienzo abierto: lo que se ve lo dibuja el hilo del centro; acá, las fotos que le llegan.
struct Vista {
    ventanas: std::rc::Weak<Inner>,
    lienzo: Lienzo,
    visible: Cell<bool>,
    renovadas: Cell<bool>,
    t0: Instant,
}

impl Vista {
    fn abrir(v: &Ventanas, lienzo: Lienzo, sel: usize, t0: Instant) -> Rc<Self> {
        let i = &v.inner;
        let iconos = lienzo.ventanas.iter().map(|w| v.icono(&w.clase)).collect();
        let fotos = lienzo
            .ventanas
            .iter()
            .map(|w| i.fotos.borrow().get(&w.direccion).map(|f| f.imagen.clone()))
            .collect();
        traza(t0, "celdas");
        let _ = i.hilo.send(Orden::VentanasAbrir(Box::new(VistaVentanas {
            salida: i.hyprland.espacios().enfocada,
            lienzo: lienzo.clone(),
            sel,
            iconos,
            fotos,
            fondo: v.fondo_guardado(),
        })));
        let vista = Rc::new(Vista {
            ventanas: Rc::downgrade(&v.inner),
            lienzo,
            visible: Cell::new(false),
            renovadas: Cell::new(false),
            t0,
        });
        // La capa se pone al mostrarse y Hyprland la funde entera, blur incluido: fundirla
        // desde adentro no sirve, el blur (que solo va donde el alfa pasa de `ignore_alpha`)
        // aparecería de golpe a mitad del fundido.
        let debil = Rc::downgrade(&vista);
        glib::timeout_add_local_once(RENOVAR, move || {
            if let Some(v) = debil.upgrade() {
                v.renovar_fotos();
            }
        });
        let debil = Rc::downgrade(&vista);
        glib::timeout_add_local_once(DEMORA, move || {
            if let Some(v) = debil.upgrade() {
                if let Some(d) = v.duenio() {
                    let _ = d.inner.hilo.send(Orden::VentanasMostrar);
                }
                v.activar();
            }
        });
        vista
    }

    fn duenio(&self) -> Option<Ventanas> {
        Some(Ventanas {
            inner: self.ventanas.upgrade()?,
        })
    }

    /// Un paso más de `Mod+Tab`: el hilo del centro mueve la elegida y muestra el lienzo.
    fn reciente(self: &Rc<Self>) {
        if let Some(d) = self.duenio() {
            let _ = d.inner.hilo.send(Orden::VentanasReciente);
        }
        self.activar();
    }

    /// Se soltó `Mod` antes de que el lienzo se mostrara.
    fn confirmar(&self) {
        if let Some(d) = self.duenio() {
            let _ = d.inner.hilo.send(Orden::VentanasConfirmar);
        }
    }

    /// Ya se ve: fotos nuevas y, si cambió, el fondo.
    fn activar(self: &Rc<Self>) {
        if self.visible.replace(true) {
            return;
        }
        traza(self.t0, "mostrar");
        self.renovar_fotos();
        let Some(d) = self.duenio() else { return };
        let antes = d.fondo_guardado();
        glib::MainContext::default().spawn_local(async move {
            let Some(t) = d.fondo().await else { return };
            if antes.as_ref().is_some_and(|a| a.data.id() == t.data.id()) {
                return;
            }
            let _ = d.inner.hilo.send(Orden::VentanasFondo(t));
        });
    }

    /// Todas a la vez: en serie tardan 16 ms cada una.
    fn renovar_fotos(self: &Rc<Self>) {
        if self.renovadas.replace(true) {
            return;
        }
        let Some(d) = self.duenio() else { return };
        for ventana in &self.lienzo.ventanas {
            let congelada = d
                .inner
                .fotos
                .borrow()
                .get(&ventana.direccion)
                .is_some_and(|f| f.congelada == Some(ventana.ws));
            if !ventana.capturable() || congelada {
                continue;
            }
            let (d, ventana, debil) = (d.clone(), ventana.clone(), Rc::downgrade(self));
            glib::MainContext::default().spawn_local(async move {
                if let Some(t) = d.fotografiar(&ventana).await {
                    let Some(v) = debil.upgrade() else { return };
                    traza(v.t0, "foto");
                    let _ = d
                        .inner
                        .hilo
                        .send(Orden::VentanasFoto(ventana.direccion.clone(), t));
                }
            });
        }
    }
}

fn icono_de(clase: &str) -> Option<gio::Icon> {
    if clase.is_empty() {
        return None;
    }
    let por_nombre = [clase.to_owned(), clase.to_lowercase()]
        .into_iter()
        .find_map(|n| gio_unix::DesktopAppInfo::new(&format!("{n}.desktop")));
    let app = por_nombre.or_else(|| {
        gio::AppInfo::all().into_iter().find_map(|a| {
            let d = a.downcast::<gio_unix::DesktopAppInfo>().ok()?;
            d.startup_wm_class()
                .filter(|c| c.eq_ignore_ascii_case(clase))
                .map(|_| d)
        })
    });
    let icono: gio::Icon = match app {
        Some(a) => a.icon()?,
        None => gio::ThemedIcon::new(&clase.to_lowercase()).upcast(),
    };
    // Si el tema no lo tiene, el genérico (y no el de "imagen rota").
    crate::icono::tiene_gicon(&icono).then_some(icono)
}
