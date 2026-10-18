//! Sin repetir lo que ya está a la vista: la fecha y la hora están en la barra, así que el
//! panel no las dice; el día elegido se ve en la grilla, así que el detalle no lo nombra.

pub mod modelo;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::agenda::{Agenda, Evento};
use crate::bajada::Bajada;
use crate::centro::marquesina::Marquesina;
use modelo::Clima;

const ANCHO: i32 = 380;

type Dia = (i32, i32, i32);

#[derive(Clone)]
pub struct Calendario {
    inner: Rc<Inner>,
}

struct Inner {
    window: gtk::ApplicationWindow,
    panel: gtk::Box,
    bajada: Bajada,
    agenda: Agenda,
    titulo: gtk::Label,
    volver: gtk::Button,
    grilla: gtk::Grid,
    detalle: gtk::Box,
    mes: Cell<(i32, i32)>,
    elegido: Cell<Dia>,
    clima: RefCell<Vec<Clima>>,
    ahora: RefCell<Option<String>>,
    ubicacion: RefCell<crate::config::Clima>,
    abierto: Cell<bool>,
    al_abrir: Oyentes,
}

type Oyentes = RefCell<Vec<Rc<dyn Fn(bool)>>>;

fn hoy() -> Dia {
    let h = glib::DateTime::now_local().expect("reloj local");
    (h.year(), h.month(), h.day_of_month())
}

impl Calendario {
    pub fn new(app: &gtk::Application, agenda: &Agenda, ubicacion: &crate::config::Clima) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.add_css_class("morgan");
        window.add_css_class("blank-window");
        window.init_layer_shell();
        window.set_namespace(Some("morgan-calendar"));
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        for borde in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(borde, true);
        }

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
        panel.add_css_class("control-center");
        panel.add_css_class("calendario");
        panel.set_halign(gtk::Align::Center);
        panel.set_valign(gtk::Align::Start);
        panel.set_size_request(ANCHO, -1);
        let bajada = Bajada::new(&panel);
        window.set_child(Some(&bajada));

        let cabecera = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        cabecera.add_css_class("cc-seccion");
        let titulo = gtk::Label::new(None);
        titulo.set_xalign(0.0);
        titulo.set_hexpand(true);
        cabecera.append(&titulo);
        let volver = gtk::Button::with_label("hoy");
        cabecera.append(&volver);
        let antes = gtk::Button::with_label("󰅁");
        let despues = gtk::Button::with_label("󰅂");
        for b in [&volver, &antes, &despues] {
            b.set_focusable(false);
        }
        cabecera.append(&antes);
        cabecera.append(&despues);
        panel.append(&cabecera);

        let mes = gtk::Box::new(gtk::Orientation::Vertical, 4);
        mes.add_css_class("cal-mes");
        let semana = gtk::Grid::new();
        semana.add_css_class("cal-semana");
        semana.set_column_homogeneous(true);
        for (i, d) in ["lu", "ma", "mi", "ju", "vi", "sá", "do"]
            .iter()
            .enumerate()
        {
            semana.attach(&gtk::Label::new(Some(d)), i as i32, 0, 1, 1);
        }
        mes.append(&semana);
        let grilla = gtk::Grid::new();
        grilla.set_column_homogeneous(true);
        grilla.set_row_spacing(2);
        grilla.set_column_spacing(2);
        mes.append(&grilla);
        panel.append(&mes);

        let detalle = gtk::Box::new(gtk::Orientation::Vertical, 8);
        panel.append(&detalle);

        let c = Calendario {
            inner: Rc::new(Inner {
                window,
                panel,
                bajada,
                agenda: agenda.clone(),
                titulo,
                volver: volver.clone(),
                grilla,
                detalle,
                mes: Cell::new((0, 0)),
                elegido: Cell::new((0, 0, 0)),
                clima: RefCell::default(),
                ahora: RefCell::default(),
                ubicacion: RefCell::new(ubicacion.clone()),
                abierto: Cell::new(false),
                al_abrir: RefCell::default(),
            }),
        };
        let yo = c.clone();
        volver.connect_clicked(move |_| yo.elegir(hoy()));
        let yo = c.clone();
        antes.connect_clicked(move |_| yo.correr_mes(-1));
        let yo = c.clone();
        despues.connect_clicked(move |_| yo.correr_mes(1));
        c.conectar();
        c
    }

    pub fn abierto(&self) -> bool {
        self.inner.abierto.get()
    }

    pub fn al_abrir(&self, f: impl Fn(bool) + 'static) {
        self.inner.al_abrir.borrow_mut().push(Rc::new(f));
    }

    fn avisar(&self, abierto: bool) {
        let oyentes = self.inner.al_abrir.borrow().clone();
        for f in oyentes {
            f(abierto);
        }
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
        self.elegir(hoy());
        i.bajada.arrancar();
        i.window.set_visible(true);
        self.avisar(true);
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move { yo.leer_clima().await });
    }

    pub fn cerrar(&self) {
        let i = &self.inner;
        if i.abierto.replace(false) {
            i.window.set_visible(false);
            self.avisar(false);
        }
    }

    fn conectar(&self) {
        let i = &self.inner;
        let teclas = gtk::EventControllerKey::new();
        let yo = self.clone();
        teclas.connect_key_pressed(move |_, tecla, _, _| {
            let d = yo.inner.elegido.get();
            match tecla {
                gdk::Key::Escape => yo.cerrar(),
                gdk::Key::Left => yo.elegir(modelo::correr(d, -1)),
                gdk::Key::Right => yo.elegir(modelo::correr(d, 1)),
                gdk::Key::Up => yo.elegir(modelo::correr(d, -7)),
                gdk::Key::Down => yo.elegir(modelo::correr(d, 7)),
                gdk::Key::Page_Up => yo.correr_mes(-1),
                gdk::Key::Page_Down => yo.correr_mes(1),
                gdk::Key::Home => yo.elegir(hoy()),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        i.window.add_controller(teclas);
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

        let yo = self.clone();
        i.agenda.al_cambiar(move || {
            if yo.abierto() {
                yo.pintar();
            }
        });
    }

    fn elegir(&self, d: Dia) {
        self.inner.elegido.set(d);
        self.inner.mes.set((d.0, d.1));
        self.pintar();
    }

    fn correr_mes(&self, n: i32) {
        let i = &self.inner;
        i.mes.set(modelo::correr_mes(i.mes.get(), n));
        self.pintar();
    }

    async fn leer_clima(&self) {
        let i = &self.inner;
        let ubicacion = i.ubicacion.borrow().clone();
        let p = crate::clima::pronostico(&ubicacion).await;
        i.clima.replace(p.dias);
        i.ahora.replace(p.ahora);
        if self.abierto() {
            self.pintar();
        }
    }

    pub fn aplicar_config(&self, c: &crate::config::Clima) {
        self.inner.ubicacion.replace(c.clone());
    }

    fn pintar(&self) {
        self.pintar_mes();
        self.pintar_detalle();
    }

    fn pintar_mes(&self) {
        let i = &self.inner;
        let (año, mes) = i.mes.get();
        let hoy = hoy();
        i.titulo
            .set_label(&format!("{} {año}", modelo::MESES[(mes - 1) as usize]));
        i.volver
            .set_visible(i.elegido.get() != hoy || (año, mes) != (hoy.0, hoy.1));
        while let Some(h) = i.grilla.first_child() {
            i.grilla.remove(&h);
        }
        let zona = glib::TimeZone::local();
        let con_eventos = modelo::dias_con_eventos(&i.agenda.eventos(), (año, mes), &zona);
        let clima = i.clima.borrow();
        let (vacios, dias) = modelo::mes(año, mes);
        for n in 1..=dias {
            let d = (año, mes, n as i32);
            let lugar = vacios + n - 1;
            let desde_hoy = modelo::distancia(hoy, d);
            let pronostico = usize::try_from(desde_hoy).ok().and_then(|k| clima.get(k));
            let celda = self.celda(d, pronostico, con_eventos.contains(&n));
            if d == hoy {
                celda.add_css_class("hoy");
            }
            if d == i.elegido.get() {
                celda.add_css_class("elegido");
            }
            if desde_hoy < 0 {
                celda.add_css_class("pasado");
            }
            i.grilla
                .attach(&celda, (lugar % 7) as i32, (lugar / 7) as i32, 1, 1);
        }
    }

    fn celda(&self, d: Dia, clima: Option<&Clima>, con_eventos: bool) -> gtk::Button {
        let caja = gtk::Box::new(gtk::Orientation::Vertical, 0);
        caja.set_valign(gtk::Align::Center);
        let numero = gtk::Label::new(Some(&d.2.to_string()));
        numero.add_css_class("numero");
        caja.append(&numero);
        let glifo = gtk::Label::new(clima.map(|c| c.glifo.as_str()));
        glifo.add_css_class("glifo");
        caja.append(&glifo);
        let capas = gtk::Overlay::new();
        capas.set_child(Some(&caja));
        if con_eventos {
            let punto = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            punto.add_css_class("marca");
            punto.set_halign(gtk::Align::End);
            punto.set_valign(gtk::Align::Start);
            capas.add_overlay(&punto);
        }
        let b = gtk::Button::new();
        b.add_css_class("cal-dia");
        b.set_focusable(false);
        b.set_child(Some(&capas));
        let yo = self.clone();
        b.connect_clicked(move |_| yo.elegir(d));
        b
    }

    fn pintar_detalle(&self) {
        let i = &self.inner;
        while let Some(h) = i.detalle.first_child() {
            i.detalle.remove(&h);
        }
        let d = i.elegido.get();
        let hoy = hoy();
        let desde_hoy = modelo::distancia(hoy, d);
        if let Some(c) = usize::try_from(desde_hoy)
            .ok()
            .and_then(|k| i.clima.borrow().get(k).cloned())
        {
            let estado = match i.ahora.borrow().as_deref() {
                Some(t) if d == hoy => format!("ahora {t} · {}", c.temperaturas),
                _ => c.temperaturas.clone(),
            };
            i.detalle
                .append(&fila_clima(&c.glifo, &c.descripcion, &estado));
        }
        let zona = glib::TimeZone::local();
        let eventos = i.agenda.eventos();
        for e in modelo::del_mes(&eventos, i.mes.get(), &zona) {
            let fila = fila_evento(e, &modelo::cuando(e, &zona));
            match modelo::dia(e, &zona) {
                Some(x) if x == d => fila.add_css_class("elegido"),
                Some(x) if modelo::distancia(hoy, x) < 0 => fila.add_css_class("pasado"),
                _ => {}
            }
            i.detalle.append(&fila);
        }
        i.detalle.set_visible(i.detalle.first_child().is_some());
    }
}

fn fila_clima(glifo: &str, titulo: &str, estado: &str) -> gtk::Box {
    let fila = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    fila.add_css_class("cal-clima");
    let icono = gtk::Label::new(Some(glifo));
    icono.add_css_class("icono");
    icono.set_valign(gtk::Align::Center);
    fila.append(&icono);
    let textos = gtk::Box::new(gtk::Orientation::Vertical, 0);
    textos.set_valign(gtk::Align::Center);
    let t = gtk::Label::new(Some(titulo));
    t.add_css_class("titulo");
    t.set_xalign(0.0);
    textos.append(&t);
    let e = gtk::Label::new(Some(estado));
    e.add_css_class("estado");
    e.set_xalign(0.0);
    textos.append(&e);
    fila.append(&textos);
    fila
}

fn fila_evento(e: &Evento, hora: &str) -> gtk::Box {
    let fila = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    fila.add_css_class("cal-evento");
    let glifo = gtk::Label::new(Some("󰃭"));
    glifo.add_css_class("icono");
    glifo.set_valign(gtk::Align::Center);
    fila.append(&glifo);
    let textos = gtk::Box::new(gtk::Orientation::Vertical, 0);
    textos.set_valign(gtk::Align::Center);
    textos.set_hexpand(true);
    let arriba = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    // En marquesina: el panel no se estira por un nombre largo, y el nombre se lee entero.
    let resumen = Marquesina::new();
    resumen.etiqueta.add_css_class("titulo");
    resumen.set_text(&e.resumen);
    arriba.append(&resumen.raiz);
    let cuando = gtk::Label::new(Some(hora));
    cuando.add_css_class("estado");
    arriba.append(&cuando);
    textos.append(&arriba);
    if !e.lugar.is_empty() {
        let lugar = Marquesina::new();
        lugar.etiqueta.add_css_class("estado");
        lugar.set_text(&e.lugar);
        textos.append(&lugar.raiz);
    }
    fila.append(&textos);
    fila
}
