//! El panel crece con el contenido hasta el alto de la salida y lo que no entra se scrollea
//! adentro, sin medir nada a mano. Los servicios se escuchan solo mientras está abierto.

mod cuotas;
mod deslizador;
pub mod desplegable;
mod ecualizador;
mod lista;
pub mod marquesina;
mod nivel;
mod reproductor;
mod resumen;
mod tarjetas;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::bajada::Bajada;
use crate::bluetooth::Bluetooth;
use crate::comando;
use crate::energia::Energia;
use crate::idle::Idle;
use crate::mezclador::{Estado as Audio, Mezclador};
use crate::mpris::Mpris;
use crate::notificaciones::{Cambio, Notificaciones};
use crate::luz::Luz;
use crate::osd::Osd;
use crate::red::{self, Red, Visible};
use crate::toasts::Toasts;
use crate::deslizable::Accion;
use desplegable::{Desplegable, Grupo, Item};
use nivel::Nivel;
use tarjetas::{Tarjeta, Tarjetas};

/// Con las tarjetas en dos columnas de monoespaciada y los títulos a 14 px, menos no entra.
const ANCHO: i32 = 380;

const CONFIRMAR_TRAS: Duration = Duration::from_millis(320);

pub struct Servicios {
    pub notificaciones: Notificaciones,
    pub osd: Osd,
    pub toasts: Toasts,
    pub idle: Idle,
    pub luz: Luz,
    pub brillo: crate::brillo::Brillo,
}

type Oyentes = RefCell<Vec<Rc<dyn Fn(bool)>>>;

#[derive(Clone)]
pub struct Centro {
    inner: Rc<Inner>,
}

struct Inner {
    window: gtk::ApplicationWindow,
    panel: gtk::ScrolledWindow,
    bajada: Bajada,
    grupo: Grupo,
    notificaciones: Notificaciones,
    osd: Osd,
    toasts: Toasts,
    idle: Idle,
    luz_: Luz,
    brillo_: crate::brillo::Brillo,
    mezclador: Mezclador,
    red: Red,
    bluetooth: Bluetooth,
    energia: Energia,
    mpris: Mpris,

    caffeine: RefCell<Option<Tarjeta>>,
    no_molestar: RefCell<Option<Tarjeta>>,
    luz: RefCell<Option<Tarjeta>>,
    red_: RefCell<Option<(Tarjeta, Desplegable)>>,
    /// La Wi-Fi nueva que pide su clave. Mientras el campo está abierto la lista de red no
    /// se rehace: cada escaneo se llevaría lo escrito y el foco.
    clave: RefCell<Option<Pedido>>,
    bt: RefCell<Option<(Tarjeta, Desplegable)>>,
    energia_: RefCell<Option<(Tarjeta, Desplegable)>>,
    sesion: RefCell<Option<Tarjeta>>,
    cuotas: RefCell<Option<(Tarjeta, cuotas::Vista)>>,
    menu_sesion: RefCell<Option<Desplegable>>,
    salida: RefCell<Option<(Nivel, Desplegable)>>,
    microfono: RefCell<Option<(Nivel, Desplegable)>>,
    brillo: RefCell<Option<Nivel>>,
    apps: RefCell<Option<Apps>>,
    reproductor: RefCell<Option<reproductor::Vista>>,
    lista: RefCell<Option<lista::Lista>>,

    abierto: Cell<bool>,
    al_abrir: Oyentes,
    lista_sucia: Cell<bool>,
    /// Para no rehacer las listas de audio en cada evento.
    salidas_vistas: RefCell<String>,
    entradas_vistas: RefCell<String>,
    ocultas: RefCell<Vec<String>>,
    reloj: RefCell<Option<glib::SourceId>>,
}

impl Centro {
    pub fn new(app: &gtk::Application, s: Servicios) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.add_css_class("morgan");
        window.add_css_class("blank-window");
        window.init_layer_shell();
        window.set_namespace(Some("morgan-centro"));
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        for borde in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(borde, true);
        }

        let panel = gtk::ScrolledWindow::new();
        panel.add_css_class("control-center");
        panel.set_hscrollbar_policy(gtk::PolicyType::Never);
        panel.set_overflow(gtk::Overflow::Hidden);
        panel.set_halign(gtk::Align::End);
        panel.set_valign(gtk::Align::Start);
        panel.set_propagate_natural_height(true);
        panel.set_size_request(ANCHO, -1);
        let bajada = Bajada::new(&panel);
        window.set_child(Some(&bajada));

        let inner = Rc::new(Inner {
            window,
            panel,
            bajada,
            grupo: Grupo::default(),
            notificaciones: s.notificaciones,
            osd: s.osd,
            toasts: s.toasts,
            idle: s.idle,
            luz_: s.luz,
            brillo_: s.brillo,
            mezclador: Mezclador::new(),
            red: Red::default(),
            bluetooth: Bluetooth::default(),
            energia: Energia::default(),
            mpris: Mpris::default(),
            caffeine: RefCell::default(),
            no_molestar: RefCell::default(),
            luz: RefCell::default(),
            red_: RefCell::default(),
            clave: RefCell::default(),
            bt: RefCell::default(),
            energia_: RefCell::default(),
            sesion: RefCell::default(),
            cuotas: RefCell::default(),
            menu_sesion: RefCell::default(),
            salida: RefCell::default(),
            microfono: RefCell::default(),
            brillo: RefCell::default(),
            apps: RefCell::default(),
            reproductor: RefCell::default(),
            lista: RefCell::default(),
            abierto: Cell::new(false),
            al_abrir: RefCell::default(),
            lista_sucia: Cell::new(true),
            salidas_vistas: RefCell::default(),
            entradas_vistas: RefCell::default(),
            ocultas: RefCell::default(),
            reloj: RefCell::default(),
        });
        let c = Centro { inner };
        c.armar();
        c
    }

    pub fn abierto(&self) -> bool {
        self.inner.abierto.get()
    }

    pub fn limpiar_notificaciones(&self) {
        if self.abierto()
            && let Some(l) = self.inner.lista.borrow().as_ref()
        {
            return l.limpiar();
        }
        self.inner.notificaciones.limpiar();
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
        i.toasts.callar(true);
        i.mezclador.abrir();
        i.red.abrir();
        i.bluetooth.abrir();
        i.energia.abrir();
        i.mpris.abrir();
        // Si sonaba al cerrar y sigue igual, el reproductor no avisa nada: se retoma acá.
        if let Some(v) = i.reproductor.borrow().as_ref() {
            v.reanudar();
        }
        self.leer_comandos();
        self.marcar_no_molestar();
        self.pintar_sesion();
        self.pintar_cuotas();
        if i.lista_sucia.get() {
            self.armar_lista();
        }
        if let Some(l) = i.lista.borrow().as_ref() {
            l.refrescar_horas();
        }
        // Las horas relativas siguen andando mientras se mira.
        let yo = self.clone();
        let id = glib::timeout_add_seconds_local(60, move || {
            if let Some(l) = yo.inner.lista.borrow().as_ref() {
                l.refrescar_horas();
            }
            yo.pintar_cuotas();
            glib::ControlFlow::Continue
        });
        if let Some(viejo) = i.reloj.replace(Some(id)) {
            viejo.remove();
        }
        i.panel.vadjustment().set_value(0.0);
        i.bajada.arrancar();
        i.window.set_visible(true);
        self.avisar(true);
        let con_foco = i.lista.borrow().as_ref().is_some_and(|l| l.enfocar());
        if !con_foco {
            GtkWindowExt::set_focus(&i.window, None::<&gtk::Widget>);
        }
    }

    pub fn cerrar(&self) {
        let i = &self.inner;
        if !i.abierto.replace(false) {
            return;
        }
        i.window.set_visible(false);
        self.avisar(false);
        i.toasts.callar(false);
        i.grupo.cerrar();
        if let Some((_, v)) = i.cuotas.borrow().as_ref() {
            v.cerrar();
        }
        // Una confirmación a medias no queda para la próxima apertura, ni una clave.
        self.llenar_sesion();
        if i.clave.take().is_some() {
            self.pintar_red(true);
        }
        i.mezclador.cerrar();
        i.red.cerrar();
        i.bluetooth.cerrar();
        i.energia.cerrar();
        i.mpris.cerrar();
        if let Some(v) = i.reproductor.borrow().as_ref() {
            v.detener();
        }
        if let Some(id) = i.reloj.take() {
            id.remove();
        }
    }

    fn armar(&self) {
        let i = &self.inner;
        // 8 px entre bloques: el ritmo del panel.
        let caja = gtk::Box::new(gtk::Orientation::Vertical, 8);
        caja.add_css_class("widgets");
        caja.set_vexpand(true);
        caja.set_size_request(ANCHO, -1);
        i.panel.set_child(Some(&caja));

        caja.append(&self.armar_tarjetas());
        caja.append(&self.armar_niveles());

        let vista = reproductor::Vista::new(&i.mpris);
        caja.append(&vista.raiz);
        i.reproductor.replace(Some(vista));

        let lista = lista::Lista::new(&i.notificaciones);
        caja.append(&lista.raiz);
        i.lista.replace(Some(lista));

        self.conectar();
    }

    fn armar_tarjetas(&self) -> gtk::Box {
        let i = &self.inner;
        let t = Tarjetas::new(&i.grupo);
        i.red_.replace(Some(t.con_lista("󰈀", tr!("Red"))));
        i.bt.replace(Some(t.con_lista("󰂯", tr!("Bluetooth"))));

        let yo = self.clone();
        let dnd = t.interruptor("󰂛", tr!("No molestar"), move || {
            let n = &yo.inner.notificaciones;
            n.set_no_molestar(!n.no_molestar());
            yo.inner
                .osd
                .mostrar(&crate::aviso_no_molestar(n.no_molestar()));
        });
        i.no_molestar.replace(Some(dnd));
        let yo = self.clone();
        let caffeine = t.interruptor("󰅶", tr!("Caffeine"), move || yo.alternar_caffeine());
        i.caffeine.replace(Some(caffeine));
        let yo = self.clone();
        i.idle.al_cambiar(move || yo.pintar_caffeine());
        let yo = self.clone();
        let luz = t.interruptor("󰖔", tr!("Luz nocturna"), move || yo.inner.luz_.alternar());
        i.luz.replace(Some(luz));
        let yo = self.clone();
        i.luz_.al_cambiar(move || yo.pintar_luz());
        i.energia_.replace(Some(t.con_lista("󰾅", tr!("Energía"))));
        let (tarjeta, lista) = t.con_lista("󰊚", tr!("Cuotas de IA"));
        let vista = cuotas::Vista::new();
        lista.revealer.set_child(Some(&vista.raiz));
        i.cuotas.replace(Some((tarjeta, vista)));
        let (tarjeta, menu) = t.con_lista("󰐥", tr!("Sesión"));
        i.sesion.replace(Some(tarjeta));
        i.menu_sesion.replace(Some(menu));
        self.llenar_sesion();
        t.raiz
    }

    fn armar_niveles(&self) -> gtk::Box {
        let i = &self.inner;
        let raiz = gtk::Box::new(gtk::Orientation::Vertical, 0);
        raiz.add_css_class("cc-niveles");

        let m = i.mezclador.clone();
        let m2 = i.mezclador.clone();
        let salida = Nivel::new(
            "󰕾",
            move |v| {
                if let Some(s) = m.estado().predeterminada {
                    m.volumen_salida(&s, v);
                }
            },
            Some(Box::new(move || {
                if let Some(s) = m2.estado().salida_predeterminada() {
                    m2.silenciar_salida(&s.nombre, !s.mudo);
                }
            })),
        );
        let lista = salida.con_lista(&i.grupo);
        raiz.append(&salida.raiz);
        i.salida.replace(Some((salida, lista)));
        let apps = Apps::new(&i.mezclador, &i.grupo);
        raiz.append(&apps.raiz);
        i.apps.replace(Some(apps));

        let m = i.mezclador.clone();
        let m2 = i.mezclador.clone();
        let microfono = Nivel::new(
            "󰍬",
            move |v| {
                if let Some(e) = m.estado().entrada_predeterminada {
                    m.volumen_entrada(&e, v);
                }
            },
            Some(Box::new(move || {
                if let Some(e) = m2.estado().entrada_predeterminada() {
                    m2.silenciar_entrada(&e.nombre, !e.mudo);
                }
            })),
        );
        let lista = microfono.con_lista(&i.grupo);
        raiz.append(&microfono.raiz);
        i.microfono.replace(Some((microfono, lista)));

        let yo = self.clone();
        let brillo = Nivel::new("󰖙", move |v| yo.mover_brillo(v), None);
        brillo.set_nombre(tr!("Brillo"));
        raiz.append(&brillo.raiz);
        i.brillo.replace(Some(brillo));
        let yo = self.clone();
        i.brillo_.al_cambiar(move || yo.pintar_brillo());
        self.pintar_brillo();
        raiz
    }

    pub fn abrir_sesion(&self) {
        self.abrir();
        if let Some(m) = self.inner.menu_sesion.borrow().as_ref() {
            m.desplegar();
        }
        if let Some(b) = self.llenar_sesion().first() {
            b.grab_focus();
        }
        // Se abre por un bind, no por una tecla de GTK: sin esto el foco no se marca.
        self.inner.window.set_focus_visible(true);
    }

    /// Bloquear y suspender actúan directo; cerrar sesión, reiniciar y apagar piden
    /// confirmación.
    fn llenar_sesion(&self) -> Vec<gtk::Button> {
        let Some(menu) = self.inner.menu_sesion.borrow().clone() else {
            return Vec::new();
        };
        let directo = |glifo: &str, texto: &str, args: &'static [&'static str]| {
            let yo = self.clone();
            Item::new(glifo, texto, None, move || {
                yo.cerrar();
                comando::lanzar(args);
            })
        };
        let confirmar = |glifo: &'static str,
                         texto: &str,
                         pregunta: &'static str,
                         args: &'static [&'static str]| {
            let yo = self.clone();
            Item::new(glifo, texto, None, move || {
                yo.confirmar(glifo, pregunta, args)
            })
        };
        menu.llenar(vec![
            directo("󰌾", tr!("Bloquear pantalla"), &["loginctl", "lock-session"]),
            directo("󰤄", tr!("Suspender"), &["systemctl", "suspend"]),
            confirmar("󰍃", tr!("Cerrar sesión"), tr!("Sí, cerrar sesión"), comando::salir_de_la_sesion()),
            confirmar("󰜉", tr!("Reiniciar"), tr!("Sí, reiniciar"), &["systemctl", "reboot"]),
            confirmar("󰐥", tr!("Apagar"), tr!("Sí, apagar"), &["systemctl", "poweroff"]),
        ])
    }

    /// El "Sí" se habilita un rato después: un doble click no confirma solo.
    fn confirmar(
        &self,
        glifo: &'static str,
        pregunta: &'static str,
        args: &'static [&'static str],
    ) {
        let Some(menu) = self.inner.menu_sesion.borrow().clone() else {
            return;
        };
        let yo = self.clone();
        let si = Item::new(glifo, pregunta, None, move || {
            yo.cerrar();
            comando::lanzar(args);
        });
        let yo = self.clone();
        let volver = Item::new("󰁍", tr!("Volver"), None, move || {
            if let Some(b) = yo.llenar_sesion().first() {
                b.grab_focus();
            }
        });
        let botones = menu.llenar(vec![si, volver]);
        if let [si, volver] = botones.as_slice() {
            volver.grab_focus();
            self.inner.window.set_focus_visible(true);
            si.set_sensitive(false);
            let si = si.clone();
            glib::timeout_add_local_once(CONFIRMAR_TRAS, move || si.set_sensitive(true));
        }
    }

    fn conectar(&self) {
        let i = &self.inner;

        let teclas = gtk::EventControllerKey::new();
        let yo = self.clone();
        teclas.connect_key_pressed(move |_, tecla, _, _| {
            if tecla == gdk::Key::Escape {
                yo.cerrar();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
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
        i.notificaciones.al_cambiar(move |c| match c {
            Cambio::NoMolestar(_) => yo.marcar_no_molestar(),
            Cambio::Llego { id, .. } => yo.actualizar_lista(&[id]),
            Cambio::Cerrada(_) => yo.actualizar_lista(&[]),
        });
        let yo = self.clone();
        i.mezclador.al_cambiar(move || yo.mostrar_audio());
        let yo = self.clone();
        i.red.al_cambiar(move || yo.mostrar_red());
        let yo = self.clone();
        i.bluetooth.al_cambiar(move || yo.mostrar_bluetooth());
        let yo = self.clone();
        i.energia.al_cambiar(move || yo.mostrar_energia());
        let yo = self.clone();
        i.mpris.al_cambiar(move || {
            if let Some(v) = yo.inner.reproductor.borrow().as_ref() {
                v.mostrar(yo.inner.mpris.actual());
            }
        });
        self.mostrar_red();
        self.mostrar_bluetooth();
        self.mostrar_energia();
        // Antes de la primera apertura y sin demorar el arranque.
        let yo = self.clone();
        glib::idle_add_local_once(move || {
            if yo.inner.lista_sucia.get() {
                yo.armar_lista();
            }
        });
    }

    fn armar_lista(&self) {
        self.inner.lista_sucia.set(false);
        if let Some(l) = self.inner.lista.borrow().as_ref() {
            l.actualizar(&[]);
        }
    }

    fn actualizar_lista(&self, cambiados: &[u32]) {
        if self.inner.lista_sucia.get() {
            return self.armar_lista();
        }
        if let Some(l) = self.inner.lista.borrow().as_ref() {
            l.actualizar(cambiados);
        }
    }

    fn pintar_sesion(&self) {
        let segundos = std::fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
            .unwrap_or_default();
        if let Some(t) = self.inner.sesion.borrow().as_ref() {
            t.poner(&resumen::sesion(segundos as u64));
        }
    }

    fn pintar_cuotas(&self) {
        if let Some((t, v)) = self.inner.cuotas.borrow().as_ref() {
            let (estado, alerta) = v.refrescar();
            t.poner(&resumen::Resumen {
                glifo: "󰊚",
                estado,
                encendida: false,
            });
            t.alertar(alerta);
        }
    }

    fn marcar_no_molestar(&self) {
        if let Some(t) = self.inner.no_molestar.borrow().as_ref() {
            t.poner(&resumen::no_molestar(
                self.inner.notificaciones.no_molestar(),
            ));
        }
    }

    fn leer_comandos(&self) {
        self.pintar_caffeine();
        self.pintar_luz();
        self.pintar_brillo();
    }

    fn pintar_caffeine(&self) {
        if let Some(t) = self.inner.caffeine.borrow().as_ref() {
            t.poner(&resumen::caffeine(self.inner.idle.caffeine()));
        }
    }

    fn pintar_luz(&self) {
        if let Some(t) = self.inner.luz.borrow().as_ref() {
            t.poner(&resumen::luz(self.inner.luz_.prendida()));
        }
    }

    fn alternar_caffeine(&self) {
        let idle = &self.inner.idle;
        idle.set_caffeine(!idle.caffeine());
        self.inner
            .osd
            .mostrar(&crate::idle::aviso_caffeine(idle.caffeine()));
    }

    fn pintar_brillo(&self) {
        if let Some(n) = self.inner.brillo.borrow().as_ref() {
            n.raiz.set_visible(self.inner.brillo_.disponible());
            n.poner_valor(self.inner.brillo_.valor());
        }
    }

    fn mover_brillo(&self, v: f64) {
        if let Err(e) = self.inner.brillo_.poner(v) {
            eprintln!("morgan: brillo: {e}");
        }
    }

    pub fn aplicar_config(&self, c: &crate::config::Sonido) {
        self.inner.ocultas.replace(c.ocultas.clone());
        if self.inner.abierto.get() {
            self.mostrar_audio();
        }
    }

    fn mostrar_audio(&self) {
        let i = &self.inner;
        let mut e = i.mezclador.estado();
        // Una salida oculta no se ofrece, salvo que esté en uso (la puesta o la de una app):
        // si no, no habría cómo verla ni sacarle una app.
        {
            let ocultas = i.ocultas.borrow();
            let en_uso = |s: &crate::mezclador::Salida| {
                e.predeterminada.as_deref() == Some(s.nombre.as_str())
                    || e.apps.iter().any(|a| a.salida == s.indice)
            };
            let fuera: Vec<u32> = e
                .salidas
                .iter()
                .filter(|s| ocultas.contains(&s.nombre) && !en_uso(s))
                .map(|s| s.indice)
                .collect();
            e.salidas.retain(|s| !fuera.contains(&s.indice));
        }
        if let (Some((n, _)), Some(s)) = (i.salida.borrow().as_ref(), e.salida_predeterminada()) {
            let glifo = if s.mudo { "󰖁" } else { s.glifo() };
            n.poner(glifo, &s.descripcion, s.volumen, s.mudo);
        }
        if let Some((n, _)) = i.microfono.borrow().as_ref() {
            match e.entrada_predeterminada() {
                Some(m) => {
                    let glifo = if m.mudo { "󰍭" } else { "󰍬" };
                    n.poner(glifo, &m.descripcion, m.volumen, m.mudo);
                }
                None => n.poner("󰍭", tr!("Sin micrófono"), 0.0, true),
            }
        }
        if let Some(a) = i.apps.borrow().as_ref() {
            a.mostrar(&e);
        }
        // Las opciones se rehacen solo si cambiaron (no en cada paso de volumen).
        let clave = format!(
            "{:?}|{:?}",
            e.predeterminada,
            e.salidas
                .iter()
                .map(|s| (&s.nombre, &s.descripcion, &s.puerto))
                .collect::<Vec<_>>()
        );
        if *i.salidas_vistas.borrow() != clave {
            i.salidas_vistas.replace(clave);
            if let Some((_, lista)) = i.salida.borrow().as_ref() {
                // La puesta no va: ya está en el nombre del nivel.
                let items = e
                    .salidas
                    .iter()
                    .filter(|s| e.predeterminada.as_deref() != Some(s.nombre.as_str()))
                    .map(|s| {
                        let m = i.mezclador.clone();
                        let nombre = s.nombre.clone();
                        Item::new(s.glifo(), &s.descripcion, None, move || {
                            m.predeterminar(&nombre)
                        })
                    })
                    .collect();
                lista.llenar(items);
            }
        }
        let clave = format!(
            "{:?}|{:?}",
            e.entrada_predeterminada,
            e.entradas
                .iter()
                .map(|s| (&s.nombre, &s.descripcion))
                .collect::<Vec<_>>()
        );
        if *i.entradas_vistas.borrow() != clave {
            i.entradas_vistas.replace(clave);
            if let Some((_, lista)) = i.microfono.borrow().as_ref() {
                let items = e
                    .entradas
                    .iter()
                    .filter(|s| e.entrada_predeterminada.as_deref() != Some(s.nombre.as_str()))
                    .map(|s| {
                        let m = i.mezclador.clone();
                        let nombre = s.nombre.clone();
                        Item::new("󰍬", &s.descripcion, None, move || {
                            m.predeterminar_entrada(&nombre)
                        })
                    })
                    .collect();
                lista.llenar(items);
            }
        }
    }

    fn mostrar_red(&self) {
        self.pintar_red(false);
    }

    fn pintar_red(&self, forzar: bool) {
        let i = &self.inner;
        let Some((tarjeta, lista)) = i.red_.borrow().clone() else {
            return;
        };
        let e = i.red.estado();
        tarjeta.poner(&resumen::red(&e));
        if !forzar && i.clave.borrow().is_some() {
            return;
        }
        // Cable y VPN arriba; las Wi-Fi, guardadas y a la vista, debajo de su radio y solo
        // con la radio prendida.
        let conexion = |c: &red::Conexion| {
            let r = i.red.clone();
            let ruta = c.ruta.clone();
            Item::new(
                c.tipo.glifo(),
                &c.nombre,
                Some(c.activa.is_some()),
                move || r.alternar(&ruta),
            )
        };
        let (wifis, otras): (Vec<_>, Vec<_>) =
            e.conexiones.iter().partition(|c| c.tipo == red::Tipo::Wifi);
        let mut items: Vec<Item> = otras.into_iter().map(conexion).collect();
        if let Some(w) = e.wifi {
            let r = i.red.clone();
            items.push(Item::new("󰤨", "Wi-Fi", Some(w), move || {
                r.alternar_wifi()
            }));
        }
        if e.wifi == Some(true) {
            for c in wifis {
                let yo = self.clone();
                let ruta = c.ruta.clone();
                items.push(conexion(c).deslizable(Accion {
                    glifo: "󰆴",
                    texto: tr!("olvidar"),
                    se_va: true,
                    f: Rc::new(move || {
                        let yo2 = yo.clone();
                        yo.inner.red.olvidar(&ruta, move || yo2.pintar_red(true));
                    }),
                }));
            }
        }
        let antes = items.len();
        for v in &e.visibles {
            let yo = self.clone();
            let v2 = v.clone();
            items.push(Item::new(v.glifo(), &v.nombre, None, move || {
                yo.elegir_wifi(&v2)
            }));
        }
        let botones = lista.llenar(items);
        let pedida = i.clave.borrow().as_ref().map(|p| p.visible.ssid.clone());
        let Some(ssid) = pedida else { return };
        // La red se fue de la vista: el campo no tiene adónde ir.
        let Some(n) = e.visibles.iter().position(|v| v.ssid == ssid) else {
            i.clave.replace(None);
            return;
        };
        let Some(boton) = botones.get(antes + n) else {
            return;
        };
        boton.add_css_class("abierto");
        let campo = self.campo_clave();
        lista.opciones().insert_child_after(&campo, Some(&lista.fila(boton)));
        if let Some((texto, _)) = i.clave.borrow().as_ref().and_then(|p| p.campo.as_ref()) {
            texto.grab_focus();
        }
    }

    /// Una abierta conecta al click; una con clave abre el campo debajo (o lo cierra).
    fn elegir_wifi(&self, v: &Visible) {
        let i = &self.inner;
        if !v.seguridad.pide_clave() {
            i.clave.replace(None);
            i.red.conectar_nueva(v, None, |_| {});
            self.pintar_red(true);
            return;
        }
        let misma = i
            .clave
            .borrow()
            .as_ref()
            .is_some_and(|p| p.visible.ssid == v.ssid);
        i.clave.replace((!misma).then(|| Pedido {
            visible: v.clone(),
            campo: None,
        }));
        self.pintar_red(true);
    }

    /// El campo del launcher como un renglón más, colgado del de la red: empieza en la
    /// columna del nombre y el ojo va en la del tilde. Sin candado: ya está en el glifo.
    fn campo_clave(&self) -> gtk::Box {
        let caja = gtk::Box::new(gtk::Orientation::Vertical, 0);
        caja.add_css_class("cc-clave");
        let entrada = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        entrada.add_css_class("launcher-entrada");
        let texto = gtk::Text::new();
        texto.set_placeholder_text(Some(tr!("Contraseña")));
        texto.set_visibility(false);
        // El ● de GTK ocupa la celda entera de la monoespaciada y se pegan.
        texto.set_invisible_char('•');
        texto.set_hexpand(true);
        entrada.append(&texto);
        let ojo = gtk::Button::with_label("\u{F0208}");
        ojo.add_css_class("glifo");
        ojo.set_focus_on_click(false);
        let t = texto.clone();
        ojo.connect_clicked(move |b| {
            let ver = !t.is_visible();
            t.set_visibility(ver);
            b.set_label(if ver { "\u{F0209}" } else { "\u{F0208}" });
        });
        entrada.append(&ojo);
        caja.append(&entrada);
        let aviso = gtk::Label::new(None);
        aviso.add_css_class("aviso");
        aviso.set_xalign(0.0);
        aviso.set_wrap(true);
        aviso.set_visible(false);
        caja.append(&aviso);

        let yo = self.clone();
        texto.connect_activate(move |_| yo.enviar_clave());
        let teclas = gtk::EventControllerKey::new();
        let yo = self.clone();
        teclas.connect_key_pressed(move |_, tecla, _, _| {
            if tecla != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            yo.inner.clave.replace(None);
            yo.pintar_red(true);
            glib::Propagation::Stop
        });
        texto.add_controller(teclas);

        if let Some(p) = self.inner.clave.borrow_mut().as_mut() {
            p.campo = Some((texto, aviso));
        }
        caja
    }

    fn enviar_clave(&self) {
        let i = &self.inner;
        let Some((v, texto, aviso)) = i.clave.borrow().as_ref().and_then(|p| {
            let (t, a) = p.campo.clone()?;
            Some((p.visible.clone(), t, a))
        }) else {
            return;
        };
        let clave = texto.text().to_string();
        if !red::clave_valida(v.seguridad, &clave) {
            avisar(&aviso, tr!("Tiene de 8 a 63 caracteres"), true);
            return;
        }
        texto.set_sensitive(false);
        avisar(&aviso, tr!("Conectando…"), false);
        let yo = self.clone();
        let ssid = v.ssid.clone();
        i.red.conectar_nueva(&v, Some(clave), move |r| {
            let sigue = yo
                .inner
                .clave
                .borrow()
                .as_ref()
                .is_some_and(|p| p.visible.ssid == ssid);
            match r {
                Ok(()) => {
                    if sigue {
                        yo.inner.clave.replace(None);
                    }
                    yo.pintar_red(true);
                }
                Err(f) if sigue => {
                    texto.set_sensitive(true);
                    texto.set_text("");
                    texto.grab_focus();
                    let m = match f {
                        red::Falla::Clave => tr!("Contraseña incorrecta"),
                        red::Falla::Otra => tr!("No se pudo conectar"),
                    };
                    avisar(&aviso, m, true);
                }
                Err(_) => {}
            }
        });
    }

    fn mostrar_bluetooth(&self) {
        let i = &self.inner;
        let Some((tarjeta, lista)) = i.bt.borrow().clone() else {
            return;
        };
        let e = i.bluetooth.estado();
        tarjeta.poner(&resumen::bluetooth(&e));
        let b = i.bluetooth.clone();
        let mut items = vec![Item::new(
            "󰂯",
            tr!("Bluetooth"),
            Some(e.encendido),
            move || b.alternar(),
        )];
        if e.encendido {
            for d in e.dispositivos {
                let b = i.bluetooth.clone();
                let ruta = d.ruta.clone();
                items.push(Item::new(
                    d.glifo(),
                    &d.nombre,
                    Some(d.conectado),
                    move || b.alternar_dispositivo(&ruta),
                ));
            }
        }
        lista.llenar(items);
    }

    fn mostrar_energia(&self) {
        let i = &self.inner;
        let Some((tarjeta, lista)) = i.energia_.borrow().clone() else {
            return;
        };
        let e = i.energia.estado();
        tarjeta.poner(&resumen::energia(e.activo.as_deref()));
        // El puesto no va: ya está en el estado de la tarjeta.
        let items = e
            .perfiles
            .iter()
            .filter(|p| e.activo.as_deref() != Some(p.as_str()))
            .map(|p| {
                let (glifo, nombre) = crate::energia::etiqueta(p);
                let en = i.energia.clone();
                let perfil = p.clone();
                Item::new(glifo, nombre, None, move || en.elegir(&perfil))
            })
            .collect();
        lista.llenar(items);
    }
}

struct Pedido {
    visible: Visible,
    campo: Option<(gtk::Text, gtk::Label)>,
}

fn avisar(aviso: &gtk::Label, texto: &str, error: bool) {
    aviso.set_label(texto);
    aviso.set_visible(true);
    if error {
        aviso.add_css_class("error");
    } else {
        aviso.remove_css_class("error");
    }
}

/// Se actualiza en el lugar: rehacerla en cada evento cortaría el arrastre de su slider.
struct Apps {
    raiz: gtk::Box,
    mezclador: Mezclador,
    grupo: Grupo,
    filas: RefCell<HashMap<u32, FilaApp>>,
}

struct FilaApp {
    nivel: Nivel,
    lista: Desplegable,
    vistas: RefCell<String>,
}

impl Apps {
    fn new(mezclador: &Mezclador, grupo: &Grupo) -> Self {
        let raiz = gtk::Box::new(gtk::Orientation::Vertical, 0);
        raiz.add_css_class("per-app-volume");
        raiz.set_visible(false);
        Apps {
            raiz,
            mezclador: mezclador.clone(),
            grupo: grupo.clone(),
            filas: RefCell::default(),
        }
    }

    fn mostrar(&self, e: &Audio) {
        let mut filas = self.filas.borrow_mut();
        filas.retain(|indice, f| {
            let sigue = e.apps.iter().any(|a| a.indice == *indice);
            if !sigue {
                self.raiz.remove(&f.nivel.raiz);
                self.grupo.quitar(&f.lista);
            }
            sigue
        });
        for a in &e.apps {
            let f = filas.entry(a.indice).or_insert_with(|| self.fila(a.indice));
            let actual = e.salidas.iter().find(|s| s.indice == a.salida);
            let glifo = if a.mudo {
                "󰖁"
            } else {
                actual.map_or("󰓃", |s| s.glifo())
            };
            f.nivel.poner(glifo, &a.nombre, a.volumen, a.mudo);
            // Las otras salidas: la puesta ya está en el glifo.
            let clave = format!(
                "{}|{:?}",
                a.salida,
                e.salidas
                    .iter()
                    .map(|s| (s.indice, &s.descripcion))
                    .collect::<Vec<_>>()
            );
            if *f.vistas.borrow() != clave {
                f.vistas.replace(clave);
                let items = e
                    .salidas
                    .iter()
                    .filter(|s| s.indice != a.salida)
                    .map(|s| {
                        let (m, indice, nombre) =
                            (self.mezclador.clone(), a.indice, s.nombre.clone());
                        Item::new(s.glifo(), &s.descripcion, None, move || {
                            m.mover_app(indice, &nombre)
                        })
                    })
                    .collect();
                f.lista.llenar(items);
            }
        }
        self.raiz.set_visible(!filas.is_empty());
    }

    fn fila(&self, indice: u32) -> FilaApp {
        let m = self.mezclador.clone();
        let m2 = self.mezclador.clone();
        let nivel = Nivel::new(
            "󰓃",
            move |v| m.volumen_app(indice, v),
            Some(Box::new(move || {
                if let Some(a) = m2.estado().apps.iter().find(|a| a.indice == indice) {
                    m2.silenciar_app(indice, !a.mudo);
                }
            })),
        );
        let lista = nivel.con_lista(&self.grupo);
        self.raiz.append(&nivel.raiz);
        FilaApp {
            nivel,
            lista,
            vistas: RefCell::default(),
        }
    }
}
