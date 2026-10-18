//! La capa cubre la salida para cerrar con un click afuera. Lo marcado es lo que mandó la app
//! al abrir; elegir una fila solo pide y cierra.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::bajada::Bajada;
use crate::bandeja::menu::{Marca, Nodo, secciones};
use crate::centro::desplegable::Grupo;

const ETIQUETA_MAX: i32 = 40;
/// Un menú de dos palabras no queda en una tira.
const ANCHO_MIN: i32 = 200;

#[derive(Clone)]
pub struct MenuBandeja {
    inner: Rc<Inner>,
}

struct Inner {
    window: gtk::ApplicationWindow,
    panel: gtk::Box,
    al_cerrar: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl MenuBandeja {
    pub fn abrir(
        barra: &gtk::ApplicationWindow,
        icono: &impl IsA<gtk::Widget>,
        raiz: &Nodo,
        al_elegir: impl Fn(i32) + 'static,
        al_cerrar: impl FnOnce() + 'static,
    ) -> Option<Self> {
        let app = barra.application()?;
        let window = gtk::ApplicationWindow::new(&app);
        window.add_css_class("morgan");
        window.add_css_class("blank-window");
        window.init_layer_shell();
        window.set_namespace(Some("morgan-bandeja"));
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        window.set_monitor(barra.monitor().as_ref());
        for borde in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(borde, true);
        }

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        panel.add_css_class("control-center");
        panel.add_css_class("menu-bandeja");
        panel.set_halign(gtk::Align::End);
        panel.set_valign(gtk::Align::Start);
        panel.set_size_request(ANCHO_MIN, -1);
        if let Some(r) = icono.compute_bounds(barra) {
            let derecha = barra.width() as f32 - (r.x() + r.width());
            panel.set_margin_end((derecha.round() as i32 - 8).max(0));
        }
        let bajada = Bajada::new(&panel);
        window.set_child(Some(&bajada));

        let m = MenuBandeja {
            inner: Rc::new(Inner {
                window,
                panel,
                al_cerrar: RefCell::new(Some(Box::new(al_cerrar))),
            }),
        };
        // La fila guarda el menú sin retenerlo: el menú retiene sus filas.
        let debil = Rc::downgrade(&m.inner);
        let elegir: Rc<dyn Fn(i32)> = Rc::new(move |id| {
            al_elegir(id);
            if let Some(inner) = debil.upgrade() {
                MenuBandeja { inner }.cerrar();
            }
        });
        for lista in listas(&raiz.hijos, &elegir) {
            m.inner.panel.append(&lista);
        }
        m.conectar();
        bajada.arrancar();
        m.inner.window.present();
        Some(m)
    }

    pub fn cerrar(&self) {
        let Some(f) = self.inner.al_cerrar.take() else {
            return;
        };
        self.inner.window.destroy();
        f();
    }

    fn conectar(&self) {
        let w = &self.inner.window;
        let teclas = gtk::EventControllerKey::new();
        let debil = Rc::downgrade(&self.inner);
        teclas.connect_key_pressed(move |_, tecla, _, _| {
            if tecla != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            if let Some(inner) = debil.upgrade() {
                MenuBandeja { inner }.cerrar();
            }
            glib::Propagation::Stop
        });
        w.add_controller(teclas);

        // Cualquier botón: con el teclado tomado, Hyprland le manda a esta capa también los clicks
        // sobre la barra.
        let click = gtk::GestureClick::new();
        click.set_button(0);
        let debil = Rc::downgrade(&self.inner);
        click.connect_released(move |g, _, x, y| {
            let Some(inner) = debil.upgrade() else { return };
            let panel = inner.panel.upcast_ref::<gtk::Widget>();
            let dentro = inner
                .window
                .pick(x, y, gtk::PickFlags::DEFAULT)
                .is_some_and(|p| p == *panel || p.is_ancestor(panel));
            if !dentro {
                g.set_state(gtk::EventSequenceState::Claimed);
                MenuBandeja { inner }.cerrar();
            }
        });
        w.add_controller(click);
    }
}

fn listas(hijos: &[Nodo], elegir: &Rc<dyn Fn(i32)>) -> Vec<gtk::Box> {
    let grupo = Grupo::default();
    secciones(hijos)
        .into_iter()
        .map(|seccion| {
            let lista = gtk::Box::new(gtk::Orientation::Vertical, 0);
            lista.add_css_class("cc-lista");
            for n in seccion {
                lista.append(&fila(n, &grupo, elegir));
            }
            lista
        })
        .collect()
}

fn fila(n: &Nodo, grupo: &Grupo, elegir: &Rc<dyn Fn(i32)>) -> gtk::Widget {
    let caja = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let texto = gtk::Label::new(Some(&n.etiqueta));
    // dbusmenu marca la tecla con `_` (y escribe `__` para uno de verdad), como GTK.
    texto.set_use_underline(true);
    texto.set_xalign(0.0);
    texto.set_hexpand(true);
    texto.set_max_width_chars(ETIQUETA_MAX);
    texto.set_ellipsize(gtk::pango::EllipsizeMode::End);
    caja.append(&texto);
    let boton = gtk::Button::new();
    boton.add_css_class("cc-opcion");
    boton.set_sensitive(n.habilitado);

    if !n.hijos.is_empty() {
        let flecha = gtk::Label::new(Some("\u{F0140}"));
        flecha.add_css_class("flecha");
        caja.append(&flecha);
        boton.set_child(Some(&caja));
        let d = grupo.nueva(&boton);
        // Un nivel adentro: abrir un submenú cierra los de su nivel, no al de arriba.
        let interno = Grupo::default();
        for (i, seccion) in secciones(&n.hijos).into_iter().enumerate() {
            for (j, h) in seccion.into_iter().enumerate() {
                let w = fila(h, &interno, elegir);
                if i > 0 && j == 0 {
                    w.add_css_class("otra-seccion");
                }
                d.opciones().append(&w);
            }
        }
        let d2 = d.clone();
        boton.connect_clicked(move |_| d2.alternar());
        let submenu = gtk::Box::new(gtk::Orientation::Vertical, 0);
        submenu.append(&boton);
        submenu.append(&d.revealer);
        return submenu.upcast();
    }

    match n.marca {
        Some(Marca::Casilla(puesta)) => {
            let casilla = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            casilla.add_css_class("casilla");
            casilla.set_valign(gtk::Align::Center);
            caja.append(&casilla);
            if puesta {
                boton.add_css_class("encendida");
            }
        }
        Some(Marca::Radio(true)) => {
            let tilde = gtk::Label::new(Some("󰄬"));
            tilde.add_css_class("tilde");
            caja.append(&tilde);
            boton.add_css_class("marcada");
        }
        Some(Marca::Radio(false)) | None => {}
    }
    boton.set_child(Some(&caja));
    let (id, f) = (n.id, elegir.clone());
    boton.connect_clicked(move |_| f(id));
    boton.upcast()
}
