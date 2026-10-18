//! Lo marcado como actual sale del fondo anotado al abrir; al aplicar se marca el pedido (el
//! tema sigue calculándose detrás).

mod modelo;
mod motor;
mod transicion;

pub use motor::actual;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::config;
use crate::hyprland::Hyprland;
use modelo::Geometria;

const DURACION: Duration = Duration::from_millis(220);
const CARGA_INICIAL: usize = 2 * modelo::VECINAS + 3;
const TANDA: usize = 6;

#[derive(Clone)]
pub struct Fondos {
    inner: Rc<Inner>,
}

struct Inner {
    app: gtk::Application,
    hyprland: Hyprland,
    motor: motor::Motor,
    abierta: RefCell<Option<Rc<Banda>>>,
    /// Sube con cada pedido: una apertura que termina de leer tarde no abre si mientras tanto se
    /// pidió cerrar.
    pedido: Cell<u64>,
    al_volver: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Fondos {
    pub fn new(app: &gtk::Application, hyprland: Hyprland, c: &config::Fondos) -> Self {
        hyprland.al_evento(|linea| {
            if let Some(salida) = motor::salida_nueva(linea) {
                motor::pintar_salida(salida.to_owned());
            }
        });
        Fondos {
            inner: Rc::new(Inner {
                app: app.clone(),
                hyprland,
                motor: motor::Motor::new(c),
                abierta: RefCell::default(),
                pedido: Cell::new(0),
                al_volver: RefCell::default(),
            }),
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
        self.inner.abierta.borrow().is_some()
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
            let banda = Banda::new(&yo, datos, volver);
            yo.inner.abierta.replace(Some(banda));
        });
    }

    pub fn cerrar(&self) {
        self.inner.pedido.set(self.inner.pedido.get() + 1);
        if let Some(b) = self.inner.abierta.take() {
            b.window.destroy();
        }
    }

    fn volver(&self) {
        let f = self.inner.al_volver.borrow().clone();
        self.cerrar();
        if let Some(f) = f {
            f();
        }
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

struct Banda {
    fondos: Weak<Inner>,
    window: gtk::ApplicationWindow,
    scroller: gtk::ScrolledWindow,
    tira: gtk::Box,
    filtro_label: gtk::Label,
    g: Geometria,
    cache: PathBuf,
    faltan: RefCell<std::collections::HashSet<PathBuf>>,
    todos: Vec<PathBuf>,
    visibles: RefCell<Vec<usize>>,
    filtro: RefCell<String>,
    sel: Cell<usize>,
    celdas: RefCell<Vec<(gtk::Box, gtk::Picture)>>,
    actual: RefCell<Option<PathBuf>>,
    generacion: Cell<u64>,
    anim: Cell<(f64, f64, Option<i64>)>,
    tick: RefCell<Option<gtk::TickCallbackId>>,
    volver: bool,
}

impl Banda {
    fn new(f: &Fondos, d: Datos, volver: bool) -> Rc<Self> {
        let window = gtk::ApplicationWindow::new(&f.inner.app);
        window.add_css_class("morgan");
        window.add_css_class("fondos");
        window.init_layer_shell();
        window.set_namespace(Some("morgan-wallpapers"));
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        for borde in [Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(borde, true);
        }
        window.set_exclusive_zone(-1);
        let monitor = f.monitor();
        if let Some(m) = &monitor {
            window.set_monitor(Some(m));
        }
        let (ancho, alto) = monitor
            .map(|m| (m.geometry().width(), m.geometry().height()))
            .unwrap_or((1920, 1080));
        let g = modelo::geometria(ancho, alto);

        let banda = gtk::Box::new(gtk::Orientation::Vertical, 8);
        banda.add_css_class("banda");
        banda.set_size_request(-1, g.alto);
        window.set_child(Some(&banda));

        let cabecera = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        cabecera.add_css_class("cabecera");
        let carpeta = d
            .raiz
            .file_name()
            .map(|n| capitalizar(&n.to_string_lossy()))
            .unwrap_or_default();
        let prompt = gtk::Label::new(Some(&format!("󰸉  {carpeta}")));
        prompt.add_css_class("prompt");
        let filtro_label = gtk::Label::new(Some("Buscar…"));
        filtro_label.set_xalign(0.0);
        filtro_label.add_css_class("filtro");
        filtro_label.add_css_class("vacio");
        cabecera.append(&prompt);
        cabecera.append(&filtro_label);
        banda.append(&cabecera);

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_vexpand(true);
        // EXTERNAL: se desplaza por código, sin barra.
        scroller.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
        banda.append(&scroller);
        let tira = gtk::Box::new(gtk::Orientation::Horizontal, g.separacion);
        tira.set_valign(gtk::Align::Center);
        tira.set_margin_start(g.costado);
        tira.set_margin_end(g.costado);
        tira.set_margin_top(modelo::MARGEN_GLOW);
        tira.set_margin_bottom(modelo::MARGEN_GLOW);
        scroller.set_child(Some(&tira));

        let visibles: Vec<usize> = (0..d.todos.len()).collect();
        let sel = d
            .actual
            .as_ref()
            .and_then(|a| d.todos.iter().position(|t| t == a))
            .unwrap_or(0);
        let b = Rc::new(Banda {
            fondos: Rc::downgrade(&f.inner),
            window: window.clone(),
            scroller: scroller.clone(),
            tira,
            filtro_label,
            g,
            cache: motor::cache_miniaturas(),
            faltan: RefCell::new(d.faltan.into_iter().collect()),
            todos: d.todos,
            visibles: RefCell::new(visibles),
            filtro: RefCell::default(),
            sel: Cell::new(0),
            celdas: RefCell::default(),
            actual: RefCell::new(d.actual),
            generacion: Cell::new(0),
            anim: Cell::new((0.0, 0.0, None)),
            tick: RefCell::default(),
            volver,
        });

        let teclas = gtk::EventControllerKey::new();
        teclas.set_propagation_phase(gtk::PropagationPhase::Capture);
        let debil = Rc::downgrade(&b);
        teclas.connect_key_pressed(move |_, k, _, m| match debil.upgrade() {
            Some(b) => b.tecla(k, m),
            None => glib::Propagation::Proceed,
        });
        window.add_controller(teclas);

        let rueda = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::BOTH_AXES | gtk::EventControllerScrollFlags::DISCRETE,
        );
        rueda.set_propagation_phase(gtk::PropagationPhase::Capture);
        let debil = Rc::downgrade(&b);
        rueda.connect_scroll(move |_, dx, dy| {
            if let Some(b) = debil.upgrade() {
                let paso = if dy.abs() >= dx.abs() { dy } else { dx };
                if paso != 0.0 {
                    let n = b.celdas.borrow().len();
                    if n > 0 {
                        let i = if paso > 0.0 {
                            b.sel.get() + 1
                        } else {
                            b.sel.get() + n - 1
                        };
                        b.seleccionar(i, true);
                    }
                }
            }
            glib::Propagation::Stop
        });
        scroller.add_controller(rueda);

        b.poblar(sel);
        b.centrar_cuando_se_pueda();
        window.present();
        b.miniaturas();
        b
    }

    /// Empezando por las más cercanas a la elegida, recalculadas en cada tanda: si uno se mueve,
    /// lo que sigue es lo que tiene alrededor.
    fn miniaturas(self: &Rc<Self>) {
        if self.faltan.borrow().is_empty() {
            return;
        }
        let debil = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let tanda_max = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
            loop {
                let Some(b) = debil.upgrade() else { return };
                let n = b.celdas.borrow().len();
                let mut tanda: Vec<PathBuf> = modelo::orden_de_carga(n, b.sel.get())
                    .into_iter()
                    .filter_map(|i| b.ruta(i))
                    .filter(|r| b.faltan.borrow().contains(r))
                    .take(tanda_max)
                    .collect();
                // Las que el filtro dejó afuera también se hacen, después.
                if tanda.is_empty() {
                    tanda = b.faltan.borrow().iter().take(tanda_max).cloned().collect();
                }
                if tanda.is_empty() {
                    return;
                }
                let cache = b.cache.clone();
                drop(b);
                let hechas = tanda.clone();
                if gio::spawn_blocking(move || motor::crear_miniaturas(&cache, &hechas))
                    .await
                    .is_err()
                {
                    return;
                }
                let Some(b) = debil.upgrade() else { return };
                for r in &tanda {
                    b.faltan.borrow_mut().remove(r);
                }
                for i in 0..b.celdas.borrow().len() {
                    if b.ruta(i).is_some_and(|r| tanda.contains(&r)) {
                        b.cargar(i);
                    }
                }
            }
        });
    }

    fn ruta(&self, i: usize) -> Option<PathBuf> {
        let todo = *self.visibles.borrow().get(i)?;
        self.todos.get(todo).cloned()
    }

    fn poblar(self: &Rc<Self>, seleccion: usize) {
        while let Some(h) = self.tira.first_child() {
            self.tira.remove(&h);
        }
        // Invalida las tandas de una población anterior (se filtró mientras cargaba).
        self.generacion.set(self.generacion.get() + 1);
        let actual = self.actual.borrow().clone();
        let mut celdas = Vec::new();
        let n = self.visibles.borrow().len();
        for i in 0..n {
            // La foto nace vacía: decodificar es lo caro, y se hace por cercanía.
            let foto = gtk::Picture::new();
            foto.set_content_fit(gtk::ContentFit::Cover);
            foto.set_can_shrink(true);
            foto.set_hexpand(true);
            foto.set_vexpand(true);
            foto.set_overflow(gtk::Overflow::Hidden);
            let celda = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            celda.add_css_class("celda");
            celda.set_size_request(self.g.celda, self.g.celda);
            celda.append(&foto);
            if actual.is_some() && self.ruta(i) == actual {
                celda.add_css_class("actual");
            }
            let click = gtk::GestureClick::new();
            let debil = Rc::downgrade(self);
            click.connect_released(move |_, _, _, _| {
                if let Some(b) = debil.upgrade() {
                    b.seleccionar(i, true);
                    b.aplicar();
                }
            });
            celda.add_controller(click);
            self.tira.append(&celda);
            celdas.push((celda, foto));
        }
        let sel = seleccion.min(n.saturating_sub(1));
        self.sel.set(sel);
        if let Some((c, _)) = celdas.get(sel) {
            c.add_css_class("sel");
        }
        self.celdas.replace(celdas);
        self.cargar_cercanas();
    }

    /// Si todavía falta, la celda queda vacía hasta que llegue: decodificar el original (un 4K)
    /// es justo lo que las miniaturas evitan.
    fn cargar(&self, i: usize) {
        let Some(ruta) = self.ruta(i) else { return };
        if self.faltan.borrow().contains(&ruta) {
            return;
        }
        let mini = modelo::miniatura(&self.cache, &ruta);
        let archivo = if mini.exists() { mini } else { ruta };
        if let Some((_, foto)) = self.celdas.borrow().get(i) {
            foto.set_filename(Some(&archivo));
        }
    }

    fn cargar_cercanas(self: &Rc<Self>) {
        let orden = modelo::orden_de_carga(self.celdas.borrow().len(), self.sel.get());
        for i in orden.iter().take(CARGA_INICIAL) {
            self.cargar(*i);
        }
        let mut pendientes: Vec<usize> = orden.into_iter().skip(CARGA_INICIAL).collect();
        pendientes.reverse();
        if pendientes.is_empty() {
            return;
        }
        let generacion = self.generacion.get();
        let debil = Rc::downgrade(self);
        glib::idle_add_local(move || {
            let Some(b) = debil.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if b.generacion.get() != generacion {
                return glib::ControlFlow::Break;
            }
            for _ in 0..TANDA {
                match pendientes.pop() {
                    Some(i) => b.cargar(i),
                    None => break,
                }
            }
            if pendientes.is_empty() {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    /// El primer aviso llega con la tira a medio medir y `set_value` lo recortaría a ese límite
    /// provisorio.
    fn centrar_cuando_se_pueda(self: &Rc<Self>) {
        let debil = Rc::downgrade(self);
        self.scroller.add_tick_callback(move |s, _| {
            let Some(b) = debil.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let adj = s.hadjustment();
            let destino = f64::from(b.g.paso) * b.sel.get() as f64;
            if adj.page_size() <= 0.0 || adj.upper() - adj.page_size() < destino {
                return glib::ControlFlow::Continue;
            }
            adj.set_value(destino);
            glib::ControlFlow::Break
        });
    }

    fn seleccionar(self: &Rc<Self>, i: usize, animar: bool) {
        let celdas = self.celdas.borrow();
        if celdas.is_empty() {
            return;
        }
        let i = i % celdas.len();
        celdas[self.sel.get()].0.remove_css_class("sel");
        self.sel.set(i);
        celdas[i].0.add_css_class("sel");
        drop(celdas);
        let destino = f64::from(self.g.paso) * i as f64;
        if animar {
            self.animar_a(destino);
        } else {
            self.scroller.hadjustment().set_value(destino);
        }
    }

    fn animar_a(self: &Rc<Self>, destino: f64) {
        self.anim
            .set((self.scroller.hadjustment().value(), destino, None));
        if self.tick.borrow().is_some() {
            return;
        }
        let debil = Rc::downgrade(self);
        let id = self.scroller.add_tick_callback(move |s, reloj| {
            let Some(b) = debil.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let (desde, hasta, t0) = b.anim.get();
            let ahora = reloj.frame_time();
            let t0 = t0.unwrap_or(ahora);
            b.anim.set((desde, hasta, Some(t0)));
            let p = ((ahora - t0) as f64 / crate::animaciones::dur(DURACION).as_micros() as f64)
                .min(1.0);
            let curva = 1.0 - (1.0 - p).powi(3);
            s.hadjustment().set_value(desde + (hasta - desde) * curva);
            if p >= 1.0 {
                b.tick.take();
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
        self.tick.replace(Some(id));
    }

    fn aplicar(&self) {
        let Some(ruta) = self.ruta(self.sel.get()) else {
            return;
        };
        // El motor resuelve las corridas encimadas (gana el último).
        if let Some(inner) = self.fondos.upgrade() {
            inner.motor.aplicar(ruta.clone());
        }
        for (i, (c, _)) in self.celdas.borrow().iter().enumerate() {
            c.set_css_classes(&if i == self.sel.get() {
                vec!["celda", "actual", "sel"]
            } else {
                vec!["celda"]
            });
        }
        self.actual.replace(Some(ruta));
    }

    fn volver_o_cerrar(&self) {
        let Some(inner) = self.fondos.upgrade() else {
            return;
        };
        let f = Fondos { inner };
        if self.volver {
            f.volver();
        } else {
            f.cerrar();
        }
    }

    fn filtrar(self: &Rc<Self>) {
        let filtro = self.filtro.borrow().clone();
        self.filtro_label.set_label(if filtro.is_empty() {
            "Buscar…"
        } else {
            &filtro
        });
        if filtro.is_empty() {
            self.filtro_label.add_css_class("vacio");
        } else {
            self.filtro_label.remove_css_class("vacio");
        }
        let anterior = self.ruta(self.sel.get());
        let nuevos = modelo::filtrar(&self.todos, &filtro);
        let sel = anterior
            .and_then(|a| nuevos.iter().position(|i| self.todos[*i] == a))
            .unwrap_or(0);
        self.visibles.replace(nuevos);
        self.poblar(sel);
        self.centrar_cuando_se_pueda();
    }

    fn tecla(self: &Rc<Self>, k: gdk::Key, m: gdk::ModifierType) -> glib::Propagation {
        let control = m.contains(gdk::ModifierType::CONTROL_MASK);
        let n = self.celdas.borrow().len();
        let sel = self.sel.get();
        match k {
            gdk::Key::Escape => self.volver_o_cerrar(),
            gdk::Key::Left | gdk::Key::KP_Left => self.seleccionar(sel + n.max(1) - 1, true),
            gdk::Key::b if control => self.seleccionar(sel + n.max(1) - 1, true),
            gdk::Key::Right | gdk::Key::KP_Right => self.seleccionar(sel + 1, true),
            gdk::Key::f if control => self.seleccionar(sel + 1, true),
            gdk::Key::Home => self.seleccionar(0, true),
            gdk::Key::End => self.seleccionar(n.saturating_sub(1), true),
            gdk::Key::Return | gdk::Key::KP_Enter => self.aplicar(),
            gdk::Key::BackSpace => {
                if self.filtro.borrow_mut().pop().is_some() {
                    self.filtrar();
                }
            }
            _ => match k.to_unicode().filter(|c| !control && !c.is_control()) {
                Some(c) => {
                    self.filtro.borrow_mut().push(c);
                    self.filtrar();
                }
                None => return glib::Propagation::Proceed,
            },
        }
        glib::Propagation::Stop
    }
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
