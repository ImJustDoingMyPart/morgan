//! Hyprland sabe cuándo se suelta `Mod`, morgan no: lo que GTK ve de los modificadores al
//! tomar el teclado no es confiable. El bind de `Mod+Tab` pone el submap [`SUBMAP`] y pide el
//! lienzo; adentro, `Mod+Tab` pide un paso más y soltar `Super_L`/`Super_R` sale del submap y
//! avisa. Los avisos llegan por el socket de eventos ([`de_evento`]), en orden.
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

mod anillo;
mod captura;
mod modelo;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::hyprland::Hyprland;
use anillo::Anillo;
use captura::Capturador;
use modelo::{Dir, Lienzo, Medidas};

const SUBMAP: &str = "morgan-ventanas";
/// Un toque no lo dibuja.
const DEMORA: Duration = Duration::from_millis(150);
/// Antes de la [`DEMORA`], para que lleguen con el lienzo. Un toque más corto no captura nada:
/// cada captura cuenta como `screencast` para el aviso de privacidad.
const RENOVAR: Duration = Duration::from_millis(100);
/// La animación de la cinta: la foto se saca con la ventana ya en su lugar.
const ASENTARSE: Duration = Duration::from_millis(350);
/// Alcanza para la franja más alta en doble densidad.
const ANCHO_FOTO: i32 = 480;
const SEPARACION: i32 = 8;
const MARGEN_GLOW: i32 = 14;
const COLUMNA: i32 = 36;
const COSTADO: i32 = 24;
const DURACION: Duration = Duration::from_millis(220);

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
    app: gtk::Application,
    hyprland: Hyprland,
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
    fondo: RefCell<Option<(String, gdk::Texture)>>,
    /// Buscarlo puede recorrer todos los `.desktop` en el hilo principal: se busca una vez, al
    /// enfocar la ventana.
    iconos: RefCell<HashMap<String, Option<gio::Icon>>>,
}

impl Ventanas {
    pub fn new(app: &gtk::Application, hyprland: Hyprland) -> Self {
        let v = Ventanas {
            inner: Rc::new(Inner {
                app: app.clone(),
                hyprland: hyprland.clone(),
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
            let vista = Vista::new(&yo, lienzo, sel, t0);
            traza(t0, "armado");
            i.abierta.replace(Some(vista));
        });
    }

    pub fn soltar(&self) {
        // Clonada aparte: `confirmar` la vacía, y el préstamo del `if let` seguiría vivo.
        let abierta = self.inner.abierta.borrow().clone();
        if let Some(v) = abierta {
            v.confirmar(false);
            return;
        }
        self.inner.soltado.set(true);
    }

    pub fn cerrar(&self) {
        self.inner.pedido.set(self.inner.pedido.get() + 1);
        self.inner.leyendo.set(false);
        if let Some(v) = self.inner.abierta.take() {
            v.window.destroy();
            self.salir_del_submap();
        }
    }

    fn enfocar(&self, direccion: String, submap: bool) {
        self.inner.pedido.set(self.inner.pedido.get() + 1);
        if let Some(v) = self.inner.abierta.take() {
            v.window.destroy();
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

    async fn fotografiar(&self, v: &modelo::Ventana) -> Option<gdk::Texture> {
        if !v.capturable() {
            return None;
        }
        let foto = self
            .capturador()?
            .capturar(&v.direccion, ANCHO_FOTO)
            .await?;
        let textura = foto.textura();
        let oculta = !self.inner.hyprland.espacios().activo(v.ws);
        self.inner.fotos.borrow_mut().insert(
            v.direccion.clone(),
            Foto {
                textura: textura.clone(),
                congelada: oculta.then_some(v.ws),
            },
        );
        Some(textura)
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

    fn icono(&self, clase: &str) -> Option<gio::Icon> {
        if let Some(i) = self.inner.iconos.borrow().get(clase) {
            return i.clone();
        }
        let icono = gdk::Display::default().and_then(|d| icono_de(&d, clase));
        self.inner
            .iconos
            .borrow_mut()
            .insert(clase.to_owned(), icono.clone());
        icono
    }

    fn fondo_guardado(&self) -> Option<gdk::Texture> {
        self.inner.fondo.borrow().as_ref().map(|(_, t)| t.clone())
    }

    async fn fondo(&self) -> Option<gdk::Texture> {
        let ruta = crate::fondos::actual()?.to_string_lossy().into_owned();
        if let Some((r, t)) = self.inner.fondo.borrow().as_ref()
            && *r == ruta
        {
            return Some(t.clone());
        }
        let r = ruta.clone();
        let (w, h, stride, alfa, bytes) = gio::spawn_blocking(move || {
            let p = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(&r, 640, -1, true).ok()?;
            Some((
                p.width(),
                p.height(),
                p.rowstride() as usize,
                p.has_alpha(),
                p.read_pixel_bytes(),
            ))
        })
        .await
        .ok()
        .flatten()?;
        let formato = if alfa {
            gdk::MemoryFormat::R8g8b8a8
        } else {
            gdk::MemoryFormat::R8g8b8
        };
        let textura: gdk::Texture = gdk::MemoryTexture::new(w, h, formato, &bytes, stride).into();
        self.inner.fondo.replace(Some((ruta, textura.clone())));
        Some(textura)
    }

    fn monitor(&self) -> Option<gdk::Monitor> {
        let enfocada = self.inner.hyprland.espacios().enfocada;
        let lista = gdk::Display::default()?.monitors();
        let monitores: Vec<gdk::Monitor> = (0..lista.n_items())
            .filter_map(|i| lista.item(i)?.downcast().ok())
            .collect();
        monitores
            .iter()
            .find(|m| m.connector().map(String::from) == enfocada)
            .or(monitores.first())
            .cloned()
    }
}

struct Foto {
    textura: gdk::Texture,
    /// Sacada con su workspace oculto: Hyprland da ese mismo cuadro hasta que se vuelva a ver.
    congelada: Option<i64>,
}

/// Las fotos van en dos páginas que se alternan, para que la nueva entre fundiéndose.
struct Celda {
    caja: gtk::Overlay,
    paginas: gtk::Stack,
    fotos: [gtk::Picture; 2],
    rect: [f32; 4],
}

struct Vista {
    ventanas: Weak<Inner>,
    window: gtk::ApplicationWindow,
    tarjeta: gtk::Box,
    scroller: gtk::ScrolledWindow,
    lienzo: Lienzo,
    m: Medidas,
    origen: i32,
    celdas: Vec<Celda>,
    pantallas: Vec<gtk::Picture>,
    sel: Cell<usize>,
    pie_icono: gtk::Image,
    pie_titulo: gtk::Label,
    pie_detalle: gtk::Label,
    anillo: Anillo,
    visible: Cell<bool>,
    renovadas: Cell<bool>,
    anim: Cell<(f64, f64, Option<i64>)>,
    tick: RefCell<Option<gtk::TickCallbackId>>,
    t0: Cell<Instant>,
}

impl Vista {
    fn new(v: &Ventanas, lienzo: Lienzo, sel: usize, t0: Instant) -> Rc<Self> {
        let window = gtk::ApplicationWindow::new(&v.inner.app);
        window.add_css_class("morgan");
        window.init_layer_shell();
        window.set_namespace(Some("morgan-ventanas"));
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        for borde in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(borde, true);
        }
        window.set_exclusive_zone(-1);
        let monitor = v.monitor();
        if let Some(m) = &monitor {
            window.set_monitor(Some(m));
        }
        let (ancho_pantalla, alto_pantalla) = monitor
            .map(|m| (m.geometry().width(), m.geometry().height()))
            .unwrap_or((1920, 1080));

        // Tope de pantalla: el 85 % del alto, descontando la cabecera, los rellenos de la banda, la
        // separación y el aire del glow.
        let alto_cuerpo = alto_pantalla * 85 / 100 - 30 - 2 * 14 - SEPARACION - 2 * MARGEN_GLOW;
        let m = modelo::medidas(
            lienzo.franjas.len(),
            lienzo.alto_monitor(),
            alto_cuerpo,
            SEPARACION,
            alto_pantalla / 5,
        );
        let (min, max) = lienzo.extension();
        let ancho_lienzo = (f64::from(max - min) * m.escala).ceil() as i32 + 2 * MARGEN_GLOW;
        let alto_lienzo = lienzo.franjas.len() as i32 * (m.alto_franja + SEPARACION) - SEPARACION
            + 2 * MARGEN_GLOW;
        let ancho_visible = ancho_lienzo.min(ancho_pantalla - 2 * COSTADO - COLUMNA - SEPARACION);

        let tarjeta = gtk::Box::new(gtk::Orientation::Vertical, SEPARACION);
        tarjeta.add_css_class("ventanas");
        tarjeta.add_css_class("entrando");
        tarjeta.set_valign(gtk::Align::Center);
        window.set_child(Some(&tarjeta));

        let pie = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        pie.add_css_class("cabecera");
        let prompt = gtk::Label::new(Some("󰖯  Ventanas"));
        prompt.add_css_class("prompt");
        let pie_icono = gtk::Image::new();
        pie_icono.set_pixel_size(20);
        let pie_titulo = gtk::Label::new(None);
        pie_titulo.add_css_class("titulo");
        pie_titulo.set_ellipsize(gtk::pango::EllipsizeMode::End);
        pie_titulo.set_xalign(0.0);
        pie_titulo.set_hexpand(true);
        let pie_detalle = gtk::Label::new(None);
        pie_detalle.add_css_class("detalle");
        pie.append(&prompt);
        pie.append(&pie_icono);
        pie.append(&pie_titulo);
        pie.append(&pie_detalle);
        tarjeta.append(&pie);

        let cuerpo = gtk::Box::new(gtk::Orientation::Horizontal, SEPARACION);
        cuerpo.set_halign(gtk::Align::Center);
        tarjeta.append(&cuerpo);

        // Los números de workspace, fuera del desplazamiento: quedan a la vista.
        let rotulos = gtk::Box::new(gtk::Orientation::Vertical, SEPARACION);
        rotulos.set_margin_top(MARGEN_GLOW);
        rotulos.set_size_request(COLUMNA, -1);
        for f in &lienzo.franjas {
            let r = gtk::Label::new(Some(&f.ws.to_string()));
            r.add_css_class("rotulo");
            if Some(f.ws) == lienzo.ws_actual {
                r.add_css_class("actual");
            }
            r.set_size_request(COLUMNA, m.alto_franja);
            r.set_valign(gtk::Align::Start);
            rotulos.append(&r);
        }
        cuerpo.append(&rotulos);

        let scroller = gtk::ScrolledWindow::new();
        // EXTERNAL: se desplaza por código, sin barra.
        scroller.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
        scroller.set_size_request(ancho_visible, alto_lienzo);
        let fijo = gtk::Fixed::new();
        fijo.set_size_request(ancho_lienzo, alto_lienzo);
        scroller.set_child(Some(&fijo));
        cuerpo.append(&scroller);

        // Un solo gris: el viewport de cada franja. Las ventanas van encima; lo que está
        // fuera de pantalla queda sobre el fondo de la banda.
        let a_px = |x: i32| f64::from(MARGEN_GLOW) + f64::from(x - min) * m.escala;
        let display = gtk::prelude::WidgetExt::display(&window);
        let fondo_guardado = v.fondo_guardado();
        let mut celdas: Vec<Option<Celda>> = (0..lienzo.ventanas.len()).map(|_| None).collect();
        let mut pantallas = Vec::new();
        for (n, f) in lienzo.franjas.iter().enumerate() {
            let y = f64::from(MARGEN_GLOW + n as i32 * (m.alto_franja + SEPARACION));
            // El viewport: el monitor de la primera ventana (todas comparten el de su ws).
            let pantalla = lienzo.ventanas[f.ventanas[0]].monitor;
            let marco = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            marco.add_css_class("pantalla");
            marco.set_overflow(gtk::Overflow::Hidden);
            marco.set_size_request(
                (f64::from(pantalla.w) * m.escala).round() as i32,
                m.alto_franja,
            );
            // La pantalla en miniatura: el fondo guardado; si cambió, el nuevo llega al
            // mostrarse (un toque no lo lee).
            let fondo = gtk::Picture::new();
            if let Some(t) = &fondo_guardado {
                fondo.set_paintable(Some(t));
            }
            fondo.set_content_fit(gtk::ContentFit::Cover);
            fondo.set_can_shrink(true);
            fondo.set_hexpand(true);
            fondo.set_vexpand(true);
            marco.append(&fondo);
            pantallas.push(fondo);
            fijo.put(&marco, a_px(pantalla.x), y);
            for &i in &f.ventanas {
                let vent = &lienzo.ventanas[i];
                let icono = v.icono(&vent.clase);
                let w = (f64::from(vent.rect.w) * m.escala).round().max(8.0) as i32;
                let h = (f64::from(vent.rect.h) * m.escala).round().max(8.0) as i32;
                let yv = y + f64::from(vent.rect.y) * m.escala;
                let xv = a_px(vent.rect.x);
                let rect = [xv as f32, yv as f32, w as f32, h as f32];
                let celda = Celda::new(&display, icono.as_ref(), rect);
                celda.caja.set_size_request(w, h);
                if !vent.capturable() {
                    celda.caja.add_css_class("fuera");
                }
                if let Some(f) = v.inner.fotos.borrow().get(&vent.direccion) {
                    celda.poner(&f.textura);
                }
                fijo.put(&celda.caja, xv, yv);
                celdas[i] = Some(celda);
            }
        }
        traza(t0, "celdas");
        let celdas: Vec<Celda> = celdas
            .into_iter()
            .map(|c| c.expect("una por ventana"))
            .collect();
        // Encima de todo: el anillo de la elegida.
        let anillo = Anillo::new();
        anillo.set_size_request(ancho_lienzo, alto_lienzo);
        fijo.put(&anillo, 0.0, 0.0);

        let vista = Rc::new(Vista {
            ventanas: Rc::downgrade(&v.inner),
            window: window.clone(),
            tarjeta,
            scroller,
            origen: min,
            lienzo,
            m,
            celdas,
            pantallas,
            sel: Cell::new(sel),
            pie_icono,
            pie_titulo,
            pie_detalle,
            anillo,
            visible: Cell::new(false),
            renovadas: Cell::new(false),
            anim: Cell::new((0.0, 0.0, None)),
            tick: RefCell::default(),
            t0: Cell::new(Instant::now()),
        });
        vista.conectar();
        vista.seleccionar(sel, false);
        vista.t0.set(t0);

        // La capa se pone al mostrarse y Hyprland la funde entera, blur incluido. Fundirla con GTK
        // no sirve: el blur, que solo va donde el alfa pasa de `ignore_alpha`, aparece de golpe a
        // mitad del fundido.
        let debil = Rc::downgrade(&vista);
        glib::timeout_add_local_once(RENOVAR, move || {
            if let Some(v) = debil.upgrade() {
                v.renovar_fotos();
            }
        });
        let debil = Rc::downgrade(&vista);
        glib::timeout_add_local_once(DEMORA, move || {
            if let Some(v) = debil.upgrade() {
                v.mostrar();
            }
        });
        vista
    }

    fn duenio(&self) -> Option<Ventanas> {
        Some(Ventanas {
            inner: self.ventanas.upgrade()?,
        })
    }

    fn conectar(self: &Rc<Self>) {
        let teclas = gtk::EventControllerKey::new();
        teclas.set_propagation_phase(gtk::PropagationPhase::Capture);
        let debil = Rc::downgrade(self);
        teclas.connect_key_pressed(move |_, k, _, _| match debil.upgrade() {
            Some(v) => v.tecla(k),
            None => glib::Propagation::Proceed,
        });
        let debil = Rc::downgrade(self);
        teclas.connect_key_released(move |_, k, _, _| {
            if let Some(v) = debil.upgrade()
                && (k == gdk::Key::Super_L || k == gdk::Key::Super_R)
            {
                v.confirmar(true);
            }
        });
        self.window.add_controller(teclas);

        // Un click afuera de la tarjeta cierra; uno en una ventana la enfoca.
        let click = gtk::GestureClick::new();
        let debil = Rc::downgrade(self);
        click.connect_released(move |g, _, x, y| {
            let Some(v) = debil.upgrade() else { return };
            let tocado = v.window.pick(x, y, gtk::PickFlags::DEFAULT);
            let celda = tocado.as_ref().and_then(|t| {
                v.celdas.iter().position(|c| {
                    *t == *c.caja.upcast_ref::<gtk::Widget>() || t.is_ancestor(&c.caja)
                })
            });
            g.set_state(gtk::EventSequenceState::Claimed);
            if let Some(i) = celda {
                v.seleccionar(i, false);
                v.confirmar(true);
                return;
            }
            let t = &v.tarjeta;
            let dentro =
                tocado.is_some_and(|p| p == *t.upcast_ref::<gtk::Widget>() || p.is_ancestor(t));
            if !dentro && let Some(d) = v.duenio() {
                d.cerrar();
            }
        });
        self.window.add_controller(click);
    }

    fn mostrar(self: &Rc<Self>) {
        if self.visible.replace(true) {
            return;
        }
        let t0 = self.t0.get();
        traza(t0, "mostrar");
        self.window.present();
        // La banda sube unos px mientras la capa se funde: se saca la clase al primer cuadro
        // y la hoja hace la transición.
        let primero = Cell::new(true);
        self.window.add_tick_callback(move |w, _| {
            if primero.replace(false) {
                traza(t0, "primer cuadro");
                if let Some(t) = w.child() {
                    t.remove_css_class("entrando");
                }
            }
            if *TRAZA && t0.elapsed() < Duration::from_millis(600) {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        self.renovar_fotos();
        let Some(d) = self.duenio() else { return };
        let antes = d.fondo_guardado();
        let debil = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let Some(t) = d.fondo().await else { return };
            if antes.as_ref() == Some(&t) {
                return;
            }
            if let Some(v) = debil.upgrade() {
                traza(v.t0.get(), "fondo");
                for p in &v.pantallas {
                    p.set_paintable(Some(&t));
                }
            }
        });
    }

    fn tecla(self: &Rc<Self>, k: gdk::Key) -> glib::Propagation {
        let sel = self.sel.get();
        let ir = |dir| self.seleccionar(self.lienzo.vecina(sel, dir), true);
        match k {
            gdk::Key::Escape => {
                if let Some(d) = self.duenio() {
                    d.cerrar();
                }
            }
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space => self.confirmar(true),
            gdk::Key::Tab => self.reciente(),
            gdk::Key::Left | gdk::Key::KP_Left | gdk::Key::h => ir(Dir::Izquierda),
            gdk::Key::Right | gdk::Key::KP_Right | gdk::Key::l => ir(Dir::Derecha),
            gdk::Key::Up | gdk::Key::KP_Up | gdk::Key::k => ir(Dir::Arriba),
            gdk::Key::Down | gdk::Key::KP_Down | gdk::Key::j => ir(Dir::Abajo),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    fn reciente(self: &Rc<Self>) {
        let i = self.lienzo.reciente(self.sel.get());
        self.seleccionar(i, true);
        self.mostrar();
    }

    fn confirmar(&self, submap: bool) {
        let Some(d) = self.duenio() else { return };
        let direccion = self.lienzo.ventanas[self.sel.get()].direccion.clone();
        d.enfocar(direccion, submap);
    }

    fn seleccionar(self: &Rc<Self>, i: usize, animar: bool) {
        self.celdas[self.sel.get()].caja.remove_css_class("sel");
        self.sel.set(i);
        self.celdas[i].caja.add_css_class("sel");
        // Al abrir va directo; después se desliza (también antes de mostrarse: para cuando
        // se vea ya llegó).
        self.anillo.ir(self.celdas[i].rect, animar);
        let v = &self.lienzo.ventanas[i];
        let icono = self
            .duenio()
            .and_then(|d| d.inner.iconos.borrow().get(&v.clase).cloned().flatten());
        match icono {
            Some(i) => self.pie_icono.set_from_gicon(&i),
            None => self
                .pie_icono
                .set_icon_name(Some("application-x-executable")),
        }
        self.pie_titulo.set_label(&v.titulo);
        self.pie_detalle
            .set_label(&format!("{} · {}", v.clase, v.ws));
        // La elegida, centrada en lo que se ve (si el lienzo no entra entero).
        if animar {
            let ancho = self.scroller.hadjustment().page_size();
            self.animar_a(self.centro(i) - ancho / 2.0);
        } else {
            self.centrar_cuando_se_pueda();
        }
    }

    fn centro(&self, i: usize) -> f64 {
        let r = self.lienzo.ventanas[i].rect;
        f64::from(MARGEN_GLOW) + f64::from(r.x - self.origen + r.w / 2) * self.m.escala
    }

    /// Cuando el ajuste ya está medido.
    fn centrar_cuando_se_pueda(self: &Rc<Self>) {
        let debil = Rc::downgrade(self);
        self.scroller.add_tick_callback(move |s, _| {
            let Some(v) = debil.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let adj = s.hadjustment();
            if adj.page_size() <= 0.0 {
                return glib::ControlFlow::Continue;
            }
            adj.set_value(v.centro(v.sel.get()) - adj.page_size() / 2.0);
            glib::ControlFlow::Break
        });
    }

    fn animar_a(self: &Rc<Self>, destino: f64) {
        let adj = self.scroller.hadjustment();
        let destino = destino.clamp(
            adj.lower(),
            (adj.upper() - adj.page_size()).max(adj.lower()),
        );
        self.anim.set((adj.value(), destino, None));
        if self.tick.borrow().is_some() {
            return;
        }
        let debil = Rc::downgrade(self);
        let id = self.scroller.add_tick_callback(move |s, reloj| {
            let Some(v) = debil.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let (desde, hasta, t0) = v.anim.get();
            let ahora = reloj.frame_time();
            let t0 = t0.unwrap_or(ahora);
            v.anim.set((desde, hasta, Some(t0)));
            let p = ((ahora - t0) as f64 / crate::animaciones::dur(DURACION).as_micros() as f64)
                .min(1.0);
            let curva = 1.0 - (1.0 - p).powi(3);
            s.hadjustment().set_value(desde + (hasta - desde) * curva);
            if p >= 1.0 {
                v.tick.take();
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
        self.tick.replace(Some(id));
    }

    /// Todas a la vez: en serie tardan 16 ms cada una.
    fn renovar_fotos(self: &Rc<Self>) {
        if self.renovadas.replace(true) {
            return;
        }
        let Some(d) = self.duenio() else { return };
        for (i, ventana) in self.lienzo.ventanas.iter().enumerate() {
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
                    traza(v.t0.get(), "foto");
                    v.celdas[i].poner(&t);
                }
            });
        }
    }
}

impl Celda {
    fn new(display: &gdk::Display, icono: Option<&gio::Icon>, rect: [f32; 4]) -> Self {
        let caja = gtk::Overlay::new();
        caja.add_css_class("ventana");
        let interior = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        interior.add_css_class("interior");
        interior.set_overflow(gtk::Overflow::Hidden);
        let paginas = gtk::Stack::new();
        paginas.set_transition_type(gtk::StackTransitionType::Crossfade);
        paginas.set_transition_duration(crate::animaciones::ms(180));
        paginas.set_hexpand(true);
        paginas.set_vexpand(true);
        let sin_foto = crate::icono::imagen(display, icono, 48);
        sin_foto.set_halign(gtk::Align::Center);
        sin_foto.set_valign(gtk::Align::Center);
        sin_foto.add_css_class("sin-foto");
        paginas.add_child(&sin_foto);
        let fotos = [(); 2].map(|_| {
            let foto = gtk::Picture::new();
            foto.set_content_fit(gtk::ContentFit::Cover);
            foto.set_can_shrink(true);
            paginas.add_child(&foto);
            foto
        });
        paginas.set_visible_child(&sin_foto);
        interior.append(&paginas);
        caja.set_child(Some(&interior));
        let insignia = crate::icono::imagen(display, icono, 20);
        insignia.set_halign(gtk::Align::Start);
        insignia.set_valign(gtk::Align::End);
        insignia.add_css_class("insignia");
        caja.add_overlay(&insignia);
        Celda {
            caja,
            paginas,
            fotos,
            rect,
        }
    }

    /// El `Stack` no anima sin mapear.
    fn poner(&self, t: &gdk::Texture) {
        let libre = if self.paginas.visible_child().as_ref() == Some(self.fotos[0].upcast_ref()) {
            &self.fotos[1]
        } else {
            &self.fotos[0]
        };
        libre.set_paintable(Some(t));
        self.paginas.set_visible_child(libre);
    }
}

/// `Some(true)` es soltar.
pub fn de_evento(linea: &str) -> Option<bool> {
    match linea.strip_prefix("custom>>")?.trim_end() {
        "morgan windows" => Some(false),
        "morgan windows release" => Some(true),
        _ => None,
    }
}

fn icono_de(display: &gdk::Display, clase: &str) -> Option<gio::Icon> {
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
    let es_tema = icono.downcast_ref::<gio::ThemedIcon>().is_some();
    (!es_tema || gtk::IconTheme::for_display(display).has_gicon(&icono)).then_some(icono)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avisos_del_bind() {
        assert_eq!(de_evento("custom>>morgan windows"), Some(false));
        assert_eq!(de_evento("custom>>morgan windows release"), Some(true));
        assert_eq!(de_evento("custom>>morgan windowsx"), None);
        assert_eq!(de_evento("activewindowv2>>morgan windows"), None);
    }
}
