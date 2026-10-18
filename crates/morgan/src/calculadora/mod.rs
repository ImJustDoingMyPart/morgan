//! El motor es `qalc`, con 60 ms de espera y uno a la vez: el de un cambio nuevo mata al
//! anterior, y ninguno vive más de `TOPE` (la actualización de las tasas de cambio puede
//! colgarse con la red).

mod modelo;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::bajada::Bajada;
use crate::comando;
use modelo::{Accion, BOTONERA, Clase, Expresion, Historial};

const ANCHO: i32 = 320;

/// Fijo, para que la botonera no se mueva al calcular.
const ALTO_CINTA: i32 = 104;

const ESPERA: Duration = Duration::from_millis(60);

const TOPE: Duration = Duration::from_secs(10);

const AVISO: Duration = Duration::from_millis(900);

const ILUMINADA: Duration = Duration::from_millis(120);

#[derive(Clone)]
pub struct Calculadora {
    inner: Rc<Inner>,
}

struct Inner {
    window: gtk::ApplicationWindow,
    panel: gtk::Box,
    bajada: Bajada,
    grande: gtk::Label,
    chica: gtk::Label,
    cinta: gtk::Box,
    historial: RefCell<Historial>,
    archivo: Option<PathBuf>,
    teclas: Vec<Vec<gtk::Button>>,
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
    pub fn new(app: &gtk::Application) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.add_css_class("morgan");
        window.add_css_class("blank-window");
        window.init_layer_shell();
        window.set_namespace(Some("morgan-calculadora"));
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        for borde in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(borde, true);
        }

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
        panel.add_css_class("control-center");
        panel.add_css_class("calculadora");
        panel.set_halign(gtk::Align::Center);
        panel.set_valign(gtk::Align::Center);
        panel.set_size_request(ANCHO, -1);
        let bajada = Bajada::new(&panel);
        window.set_child(Some(&bajada));

        let recuadro = gtk::Box::new(gtk::Orientation::Vertical, 0);
        recuadro.add_css_class("calc-pantalla");
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_size_request(-1, ALTO_CINTA);
        let cinta = gtk::Box::new(gtk::Orientation::Vertical, 0);
        cinta.add_css_class("calc-cinta");
        // Pocas cuentas quedan abajo, pegadas a la de ahora.
        cinta.set_valign(gtk::Align::End);
        scroll.set_child(Some(&cinta));
        // Cuando la cinta crece, se ve la última.
        scroll
            .vadjustment()
            .connect_changed(|a| a.set_value(a.upper() - a.page_size()));
        recuadro.append(&scroll);

        // Lo de ahora es un botón: un click copia el resultado.
        let pantalla = gtk::Button::new();
        pantalla.add_css_class("calc-actual");
        pantalla.set_focusable(false);
        let lineas = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let grande = gtk::Label::new(None);
        grande.add_css_class("grande");
        let chica = gtk::Label::new(None);
        chica.add_css_class("chica");
        for l in [&grande, &chica] {
            l.set_xalign(1.0);
            l.set_ellipsize(gtk::pango::EllipsizeMode::Start);
            l.set_single_line_mode(true);
            lineas.append(l);
        }
        pantalla.set_child(Some(&lineas));
        recuadro.append(&pantalla);
        panel.append(&recuadro);

        let botonera = gtk::Grid::new();
        botonera.add_css_class("calc-botonera");
        botonera.set_column_homogeneous(true);
        botonera.set_row_homogeneous(true);
        botonera.set_row_spacing(4);
        botonera.set_column_spacing(4);
        let mut teclas = Vec::new();
        for (f, fila) in BOTONERA.iter().enumerate() {
            let mut botones = Vec::new();
            for (c, t) in fila.iter().enumerate() {
                let b = gtk::Button::with_label(t.etiqueta);
                b.add_css_class("calc-tecla");
                b.add_css_class(match t.clase {
                    Clase::Numero => "numero",
                    Clase::Operador => "operador",
                    Clase::Funcion => "funcion",
                    Clase::Igual => "igual",
                });
                b.set_focusable(false);
                botonera.attach(&b, c as i32, f as i32, 1, 1);
                botones.push(b);
            }
            teclas.push(botones);
        }
        panel.append(&botonera);

        let archivo = crate::notificaciones::dir_from(|k| std::env::var_os(k))
            .map(|d| d.join("calculadora.json"));
        let historial = archivo.as_deref().map(cargar).unwrap_or_default();

        let c = Calculadora {
            inner: Rc::new(Inner {
                window,
                panel,
                bajada,
                grande,
                chica,
                cinta,
                historial: RefCell::new(historial),
                archivo,
                teclas,
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
        for (f, fila) in BOTONERA.iter().enumerate() {
            for (col, t) in fila.iter().enumerate() {
                let yo = c.clone();
                let accion = t.accion;
                c.inner.teclas[f][col].connect_clicked(move |_| yo.hacer(accion));
            }
        }
        let yo = c.clone();
        pantalla.connect_clicked(move |_| yo.copiar());
        c.conectar();
        for cuenta in c.inner.historial.borrow().cuentas() {
            c.fila(&cuenta.cuenta, &cuenta.resultado);
        }
        c.pintar();
        c
    }

    fn fila(&self, cuenta: &str, resultado: &str) {
        let fila = gtk::Button::new();
        fila.add_css_class("calc-fila");
        fila.set_focusable(false);
        let caja = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let izq = gtk::Label::new(Some(cuenta));
        izq.add_css_class("cuenta");
        izq.set_hexpand(true);
        izq.set_xalign(1.0);
        izq.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        let der = gtk::Label::new(Some(&format!("= {resultado}")));
        der.add_css_class("resultado");
        der.set_ellipsize(gtk::pango::EllipsizeMode::End);
        der.set_max_width_chars(16);
        caja.append(&izq);
        caja.append(&der);
        fila.set_child(Some(&caja));
        let yo = self.clone();
        let r = resultado.to_owned();
        fila.connect_clicked(move |_| yo.escribir(&r));
        self.inner.cinta.append(&fila);
    }

    fn anotar(&self, cuenta: &str, resultado: &str) {
        let i = &self.inner;
        if !i.historial.borrow_mut().agregar(cuenta, resultado) {
            return;
        }
        self.fila(cuenta, resultado);
        let mut filas = 0;
        let mut hijo = i.cinta.first_child();
        while let Some(h) = hijo {
            filas += 1;
            hijo = h.next_sibling();
        }
        if filas > Historial::TOPE
            && let Some(primera) = i.cinta.first_child()
        {
            i.cinta.remove(&primera);
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
        i.bajada.arrancar();
        i.window.set_visible(true);
    }

    pub fn cerrar(&self) {
        let i = &self.inner;
        if i.abierto.replace(false) {
            i.window.set_visible(false);
        }
    }

    fn conectar(&self) {
        let i = &self.inner;
        let teclas = gtk::EventControllerKey::new();
        let yo = self.clone();
        teclas.connect_key_pressed(move |_, tecla, _, mods| {
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            match tecla {
                gdk::Key::Escape => yo.cerrar(),
                gdk::Key::Return | gdk::Key::KP_Enter => yo.tecla(Accion::Igual, "="),
                gdk::Key::BackSpace => yo.tecla(Accion::Borrar, "󰭜"),
                gdk::Key::Delete => yo.tecla(Accion::Limpiar, "C"),
                gdk::Key::c | gdk::Key::C if ctrl => yo.copiar(),
                // Con Ctrl o Alt no es texto: lo demás no hace nada.
                _ if ctrl || mods.contains(gdk::ModifierType::ALT_MASK) => {
                    return glib::Propagation::Proceed;
                }
                gdk::Key::equal | gdk::Key::KP_Equal => yo.tecla(Accion::Igual, "="),
                gdk::Key::KP_Decimal | gdk::Key::KP_Separator => {
                    yo.tecla(Accion::Escribir(","), ",")
                }
                _ => {
                    let Some(s) = tecla.to_unicode().and_then(modelo::del_teclado) else {
                        return glib::Propagation::Proceed;
                    };
                    // `(` y `)` del teclado se escriben tal cual; la tecla es la de los dos.
                    if s == "(" || s == ")" {
                        yo.iluminar((0, 1));
                    } else {
                        yo.iluminar_texto(&s);
                    }
                    yo.escribir(&s);
                }
            }
            glib::Propagation::Stop
        });
        i.window.add_controller(teclas);

        // El click afuera del panel cierra, como en el calendario.
        let click = gtk::GestureClick::new();
        let yo = self.clone();
        click.connect_released(move |g, _, x, y| {
            let w = &yo.inner.window;
            let dentro = w.pick(x, y, gtk::PickFlags::DEFAULT).is_some_and(|p| {
                p == *yo.inner.panel.upcast_ref::<gtk::Widget>() || p.is_ancestor(&yo.inner.panel)
            });
            if !dentro {
                g.set_state(gtk::EventSequenceState::Claimed);
                yo.cerrar();
            }
        });
        i.window.add_controller(click);
    }

    fn tecla(&self, accion: Accion, etiqueta: &str) {
        let pos = BOTONERA.iter().enumerate().find_map(|(f, fila)| {
            fila.iter()
                .position(|t| t.etiqueta == etiqueta)
                .map(|c| (f, c))
        });
        if let Some(p) = pos {
            self.iluminar(p);
        }
        self.hacer(accion);
    }

    fn iluminar_texto(&self, s: &str) {
        if let Some(p) = modelo::tecla_de(s) {
            self.iluminar(p);
        }
    }

    fn iluminar(&self, (f, c): (usize, usize)) {
        let b = self.inner.teclas[f][c].clone();
        b.add_css_class("apretada");
        glib::timeout_add_local_once(ILUMINADA, move || b.remove_css_class("apretada"));
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
        i.chica.set_text("copiado");
        i.chica.add_css_class("copiado");
        let yo = self.clone();
        let id = glib::timeout_add_local_once(AVISO, move || {
            yo.inner.aviso.take();
            yo.inner.chica.remove_css_class("copiado");
            yo.pintar();
        });
        i.aviso.replace(Some(id));
    }

    /// Lo que se escriba después no hereda el color del aviso.
    fn quitar_aviso(&self) {
        if let Some(id) = self.inner.aviso.take() {
            id.remove();
            self.inner.chica.remove_css_class("copiado");
        }
    }

    fn pintar(&self) {
        let i = &self.inner;
        let e = i.expresion.borrow();
        i.grande
            .set_text(if e.texto().is_empty() { "0" } else { e.texto() });
        i.grande
            .set_opacity(if e.texto().is_empty() { 0.5 } else { 1.0 });
        let chica = if e.es_resultado() {
            i.cuenta.borrow().clone().unwrap_or_default()
        } else {
            i.previa
                .borrow()
                .as_ref()
                .map(|r| format!("= {r}"))
                .unwrap_or_default()
        };
        // Sin texto la línea ocupa igual su alto: la pantalla no salta.
        i.chica
            .set_text(if chica.is_empty() { " " } else { &chica });
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
