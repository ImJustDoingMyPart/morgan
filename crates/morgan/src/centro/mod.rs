//! El centro de control, sin GTK: lo dibuja su propio hilo con el motor de `morgan-motor`
//! (`hilo.rs`, `dibujo/`). Acá, en el hilo de GTK, quedan los servicios (audio, red, bluetooth,
//! energía, el reproductor, las notificaciones) y lo que hace cada click: se le manda al hilo lo
//! que se ve, en palabras y glifos (`vista.rs`), y vuelve lo que se tocó.
//!
//! Los servicios se escuchan solo mientras está abierto.

mod acciones;
mod bt;
mod dibujo;
mod ecualizador;
pub mod hilo;
pub mod lista;
pub mod marquesina;
mod resumen;
pub mod vista;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use morgan_motor::color::Paleta;
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::bluetooth::Bluetooth;
use crate::comando;
use crate::energia::Energia;
use crate::idle::Idle;
use crate::luz::Luz;
use crate::mezclador::Mezclador;
use crate::mpris::Mpris;
use crate::notificaciones::servidor::Razon;
use crate::notificaciones::{Cambio, Notificaciones};
use crate::osd::Osd;
use crate::red::{self, Red, Visible};
use crate::tema::Tema;
use crate::toasts::Toasts;
use acciones::{Acciones, Parte};
use hilo::Orden;
use vista::{
    BotonCampo, BotonRepro, Caja, Campo, Contenido, Cuota, Deslizar, Evento, EventoToast, ListaId,
    Nivel, NivelId, Opcion, Tarjeta, Toque,
};

const CONFIRMAR_TRAS: Duration = Duration::from_millis(320);

pub struct Servicios {
    pub notificaciones: Notificaciones,
    pub hyprland: crate::hyprland::Hyprland,
    pub osd: Osd,
    pub idle: Idle,
    pub luz: Luz,
    pub brillo: crate::brillo::Brillo,
    pub tema: Tema,
}

type Oyentes = RefCell<Vec<Rc<dyn Fn(bool)>>>;

/// El lugar de cada tarjeta en la grilla.
const RED: usize = 0;
const BT: usize = 1;
const NO_MOLESTAR: usize = 2;
const CAFFEINE: usize = 3;
const LUZ: usize = 4;
const ENERGIA: usize = 5;
const CUOTAS: usize = 6;
const SESION: usize = 7;

#[derive(Clone)]
pub struct Centro {
    inner: Rc<Inner>,
}

struct Inner {
    hilo: Sender<Orden>,
    acciones: Acciones,
    notificaciones: Notificaciones,
    osd: Osd,
    toasts: Toasts,
    idle: Idle,
    luz: Luz,
    brillo: crate::brillo::Brillo,
    mezclador: Mezclador,
    red: Red,
    bluetooth: Bluetooth,
    energia: Energia,
    mpris: Mpris,
    lista: Rc<lista::Lista>,

    tarjetas: RefCell<Vec<Tarjeta>>,
    /// La Wi-Fi nueva que pide su clave.
    clave: RefCell<Option<Pedido>>,
    /// El dispositivo que se está emparejando, con lo que muestra su campo.
    emparejo: RefCell<Option<bt::Emparejo>>,
    confirmando: RefCell<Option<Confirmacion>>,
    /// Cada campo nuevo lleva otro número: lo escrito se borra y toma el foco.
    generaciones: Cell<u64>,
    /// Las acciones de cada nivel, que no cambian mientras exista (un arrastre no se corta).
    niveles: RefCell<HashMap<NivelId, (vista::Id, Option<vista::Id>)>>,
    caratulas: RefCell<HashMap<std::path::PathBuf, Option<morgan_motor::vello::peniko::ImageData>>>,

    abierto: Cell<bool>,
    al_abrir: Oyentes,
    /// Para no rehacer las listas de audio en cada evento.
    salidas_vistas: RefCell<String>,
    entradas_vistas: RefCell<String>,
    apps_vistas: RefCell<HashMap<u32, String>>,
    ocultas: RefCell<Vec<String>>,
    reloj: RefCell<Option<glib::SourceId>>,
    escuchar: RefCell<Option<glib::JoinHandle<()>>>,
    /// El menú de la bandeja abierto: qué hacer al elegir y al cerrarse.
    menu: RefCell<Option<MenuAbierto>>,
    /// El launcher dibuja en el hilo del centro: lo que hace allá le llega acá.
    al_launcher: RefCell<Option<AlLauncher>>,
    /// Lo mismo, del menú de sistema.
    al_ajustes: RefCell<Option<AlAjustes>>,
    al_calculadora: RefCell<Option<AlCalculadora>>,
    al_permiso: RefCell<Option<AlPermiso>>,
    al_fondos: RefCell<Option<AlFondos>>,
    al_ventanas: RefCell<Option<AlVentanas>>,
}

type AlLauncher = Rc<dyn Fn(vista::EventoLauncher)>;
type AlAjustes = Rc<dyn Fn(vista::EventoAjustes)>;
type AlCalculadora = Rc<dyn Fn(vista::EventoCalculadora)>;
type AlPermiso = Rc<dyn Fn(vista::EventoPermiso)>;
type AlFondos = Rc<dyn Fn(vista::EventoFondos)>;
type AlVentanas = Rc<dyn Fn(vista::EventoVentanas)>;

struct MenuAbierto {
    elegir: Box<dyn Fn(i32)>,
    al_cerrar: Box<dyn FnOnce()>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(h) = self.escuchar.take() {
            h.abort();
        }
    }
}

/// La confirmación de sesión a medias: glifo, pregunta, comando y si ya se habilitó el «Sí».
type Confirmacion = (&'static str, &'static str, &'static [&'static str], bool);

struct Pedido {
    visible: Visible,
    generacion: u64,
    aviso: Option<(String, bool)>,
    ocupado: bool,
    /// El número del último fallo (de `generacion`, que siempre sube).
    fallos: u64,
}

/// El color del texto del tema, para los íconos simbólicos.
fn color_texto(tema: &Tema) -> [f32; 4] {
    Paleta::desde_css(&tema.actual().0).on_surface
}

impl Centro {
    pub fn new(s: Servicios) -> Self {
        let (css, opacidad, borde) = s.tema.actual();
        let (tx, rx) = hilo::lanzar(Paleta::desde_css(&css), opacidad as f32, borde);
        let t = tx.clone();
        let lista = lista::Lista::new(&s.notificaciones, color_texto(&s.tema), lista::CENTRO);
        let toasts = Toasts::new(
            tx.clone(),
            &s.notificaciones,
            &s.hyprland,
            color_texto(&s.tema),
        );
        let (l, ts) = (lista.clone(), toasts.clone());
        s.tema.al_cambiar(move |css, opacidad, borde| {
            let p = Paleta::desde_css(css);
            l.poner_color(p.on_surface);
            let _ = t.send(Orden::Tema(p, opacidad as f32, borde));
            ts.poner_color(p.on_surface);
        });
        let inner = Rc::new(Inner {
            hilo: tx,
            acciones: Acciones::default(),
            notificaciones: s.notificaciones,
            osd: s.osd,
            toasts,
            idle: s.idle,
            luz: s.luz,
            brillo: s.brillo,
            mezclador: Mezclador::new(),
            red: Red::default(),
            bluetooth: Bluetooth::default(),
            energia: Energia::default(),
            mpris: Mpris::default(),
            lista,
            tarjetas: RefCell::default(),
            clave: RefCell::default(),
            emparejo: RefCell::default(),
            confirmando: RefCell::default(),
            generaciones: Cell::new(0),
            niveles: RefCell::default(),
            caratulas: RefCell::default(),
            abierto: Cell::new(false),
            al_abrir: RefCell::default(),
            salidas_vistas: RefCell::default(),
            entradas_vistas: RefCell::default(),
            apps_vistas: RefCell::default(),
            ocultas: RefCell::default(),
            reloj: RefCell::default(),
            escuchar: RefCell::default(),
            menu: RefCell::default(),
            al_launcher: RefCell::default(),
            al_ajustes: RefCell::default(),
            al_calculadora: RefCell::default(),
            al_permiso: RefCell::default(),
            al_fondos: RefCell::default(),
            al_ventanas: RefCell::default(),
        });
        let c = Centro { inner };
        let weak = Rc::downgrade(&c.inner);
        let escuchar = glib::MainContext::default().spawn_local(async move {
            while let Ok(e) = rx.recv().await {
                let Some(inner) = weak.upgrade() else { return };
                Centro { inner }.evento(e);
            }
        });
        c.inner.escuchar.replace(Some(escuchar));
        c.armar();
        c
    }

    fn mandar(&self, o: Orden) {
        // Si el hilo terminó (sin Wayland o sin GPU) ya lo dijo al terminar.
        let _ = self.inner.hilo.send(o);
    }

    pub fn abierto(&self) -> bool {
        self.inner.abierto.get()
    }

    /// Con el centro abierto, salen una tras otra, como con el botón.
    pub fn limpiar_notificaciones(&self) {
        if self.abierto() {
            self.mandar(Orden::Limpiar);
        } else {
            self.inner.notificaciones.limpiar();
        }
    }

    /// El menú de una app de la bandeja, dibujado por el hilo del centro. Uno abierto antes
    /// se cierra.
    pub fn abrir_menu(
        &self,
        m: vista::Menu,
        elegir: impl Fn(i32) + 'static,
        al_cerrar: impl FnOnce() + 'static,
    ) {
        self.cerrar_menu();
        self.inner.menu.replace(Some(MenuAbierto {
            elegir: Box::new(elegir),
            al_cerrar: Box::new(al_cerrar),
        }));
        self.mandar(Orden::Menu(Some(Box::new(m))));
    }

    pub fn cerrar_menu(&self) {
        let Some(m) = self.inner.menu.take() else {
            return;
        };
        self.mandar(Orden::Menu(None));
        (m.al_cerrar)();
    }

    /// El canal del hilo del centro, para las superficies que dibujan en él (el launcher).
    pub fn hilo(&self) -> Sender<Orden> {
        self.inner.hilo.clone()
    }

    pub fn al_launcher(&self, f: impl Fn(vista::EventoLauncher) + 'static) {
        self.inner.al_launcher.replace(Some(Rc::new(f)));
    }

    pub fn al_calculadora(&self, f: impl Fn(vista::EventoCalculadora) + 'static) {
        self.inner.al_calculadora.replace(Some(Rc::new(f)));
    }

    pub fn al_permiso(&self, f: impl Fn(vista::EventoPermiso) + 'static) {
        self.inner.al_permiso.replace(Some(Rc::new(f)));
    }

    pub fn al_fondos(&self, f: impl Fn(vista::EventoFondos) + 'static) {
        self.inner.al_fondos.replace(Some(Rc::new(f)));
    }

    pub fn al_ventanas(&self, f: impl Fn(vista::EventoVentanas) + 'static) {
        self.inner.al_ventanas.replace(Some(Rc::new(f)));
    }

    pub fn al_ajustes(&self, f: impl Fn(vista::EventoAjustes) + 'static) {
        self.inner.al_ajustes.replace(Some(Rc::new(f)));
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
        self.abrir_con(false);
    }

    fn abrir_con(&self, sesion: bool) {
        let i = &self.inner;
        if i.abierto.replace(true) {
            if sesion {
                self.llenar_sesion();
                self.mandar(Orden::Abrir { sesion });
            }
            return;
        }
        i.toasts.callar(true);
        i.mezclador.abrir();
        i.red.abrir();
        i.bluetooth.abrir();
        i.energia.abrir();
        i.mpris.abrir();
        self.leer_comandos();
        self.marcar_no_molestar();
        self.pintar_sesion();
        self.pintar_cuotas();
        self.mandar_avisos();
        self.mandar(Orden::Abrir { sesion });
        // Las cuotas siguen al reloj mientras se mira.
        let yo = self.clone();
        let id = glib::timeout_add_seconds_local(60, move || {
            yo.pintar_cuotas();
            glib::ControlFlow::Continue
        });
        if let Some(viejo) = i.reloj.replace(Some(id)) {
            viejo.remove();
        }
        self.avisar(true);
    }

    pub fn cerrar(&self) {
        if !self.inner.abierto.get() {
            return;
        }
        self.mandar(Orden::Cerrar);
        self.cerrado();
    }

    /// Lo que pasa al cerrarse, lo haya cerrado quien sea.
    fn cerrado(&self) {
        let i = &self.inner;
        if !i.abierto.replace(false) {
            return;
        }
        self.avisar(false);
        i.toasts.callar(false);
        // Una confirmación a medias no queda para la próxima apertura, ni una clave.
        self.llenar_sesion();
        if i.clave.take().is_some() {
            self.pintar_red();
        }
        self.cancelar_emparejo();
        i.mezclador.cerrar();
        i.red.cerrar();
        i.bluetooth.cerrar();
        i.energia.cerrar();
        i.mpris.cerrar();
        if let Some(id) = i.reloj.take() {
            id.remove();
        }
        i.lista.barrer_previas();
    }

    pub fn abrir_sesion(&self) {
        self.llenar_sesion();
        self.abrir_con(true);
    }

    fn generacion(&self) -> u64 {
        let g = self.inner.generaciones.get() + 1;
        self.inner.generaciones.set(g);
        g
    }

    fn evento(&self, e: Evento) {
        let i = &self.inner;
        match e {
            Evento::Tocar(id) => i.acciones.hacer(id),
            Evento::Nivel(id, v) => i.acciones.hacer_mover(id, v),
            Evento::Enviar(id, s) => i.acciones.hacer_enviar(id, s),
            Evento::Repro(b) => match b {
                BotonRepro::Mezcla => i.mpris.alternar_mezcla(),
                BotonRepro::Anterior => i.mpris.pedir("Previous"), // no-tr
                BotonRepro::Play => i.mpris.pedir("PlayPause"),
                BotonRepro::Siguiente => i.mpris.pedir("Next"), // no-tr
                BotonRepro::Repetir => i.mpris.ciclar_repeticion(),
            },
            Evento::Accion(id) => i.notificaciones.accion(id, "default"),
            Evento::Boton(id, clave) => i.notificaciones.accion(id, &clave),
            Evento::Cerrar(id) => i.notificaciones.cerrar(id, Razon::Usuario),
            Evento::Copiar(id) => {
                if let Some(n) = i.notificaciones.get(id) {
                    let texto = crate::notificaciones::texto_plano(&n);
                    comando::lanzar(&["wl-copy", "--", &texto]);
                }
            }
            Evento::CerrarGrupo(grupo) => {
                let ids: Vec<u32> = i
                    .notificaciones
                    .lista()
                    .iter()
                    .filter(|n| n.grupo() == grupo)
                    .map(|n| n.id)
                    .collect();
                for id in ids {
                    i.notificaciones.cerrar(id, Razon::Usuario);
                }
            }
            Evento::Limpiar(ids) => {
                for id in ids {
                    i.notificaciones.cerrar(id, Razon::Usuario);
                }
            }
            Evento::Cerrado => self.cerrado(),
            Evento::Elegido(id) => {
                if let Some(m) = i.menu.borrow().as_ref() {
                    (m.elegir)(id);
                }
            }
            Evento::MenuCerrado => {
                if let Some(m) = i.menu.take() {
                    (m.al_cerrar)();
                }
            }
            // Lo mismo que en la lista, y el toast se va (copiar lo deja).
            Evento::Calculadora(e) => {
                let f = i.al_calculadora.borrow().clone();
                if let Some(f) = f {
                    f(e);
                }
            }
            Evento::Permiso(e) => {
                let f = i.al_permiso.borrow().clone();
                if let Some(f) = f {
                    f(e);
                }
            }
            Evento::Ventanas(e) => {
                let f = i.al_ventanas.borrow().clone();
                if let Some(f) = f {
                    f(e);
                }
            }
            Evento::Fondos(e) => {
                let f = i.al_fondos.borrow().clone();
                if let Some(f) = f {
                    f(e);
                }
            }
            Evento::Ajustes(e) => {
                let f = i.al_ajustes.borrow().clone();
                if let Some(f) = f {
                    f(e);
                }
            }
            Evento::Launcher(e) => {
                let f = i.al_launcher.borrow().clone();
                if let Some(f) = f {
                    f(e);
                }
            }
            Evento::Toast(t) => match t {
                EventoToast::Accion(id) => {
                    self.evento(Evento::Accion(id));
                    i.toasts.quitar(id);
                }
                EventoToast::Boton(id, clave) => {
                    self.evento(Evento::Boton(id, clave));
                    i.toasts.quitar(id);
                }
                EventoToast::Cerrar(id) => {
                    self.evento(Evento::Cerrar(id));
                    i.toasts.quitar(id);
                }
                EventoToast::Copiar(id) => self.evento(Evento::Copiar(id)),
                EventoToast::Encima(id) => i.toasts.encima(id),
            },
        }
    }

    // ── Armar ──

    fn armar(&self) {
        let i = &self.inner;
        let a = &i.acciones;
        let lista = |glifo: &str, titulo: &str, l: ListaId| Tarjeta {
            glifo: glifo.into(),
            titulo: titulo.into(),
            estado: String::new(),
            encendida: false,
            alerta: false,
            que: Toque::Lista(l),
        };
        let interruptor = |glifo: &str, titulo: &str, id: vista::Id| Tarjeta {
            glifo: glifo.into(),
            titulo: titulo.into(),
            estado: String::new(),
            encendida: false,
            alerta: false,
            que: Toque::Accion(id),
        };
        let yo = self.clone();
        let dnd = a.tocar(Parte::Fija, move || {
            let n = &yo.inner.notificaciones;
            n.set_no_molestar(!n.no_molestar());
            yo.inner
                .osd
                .mostrar(&crate::aviso_no_molestar(n.no_molestar()));
        });
        let yo = self.clone();
        let caffeine = a.tocar(Parte::Fija, move || yo.alternar_caffeine());
        let yo = self.clone();
        let luz = a.tocar(Parte::Fija, move || yo.inner.luz.alternar());
        i.tarjetas.replace(vec![
            lista("󰈀", tr!("Red"), ListaId::Red),
            lista("󰂯", tr!("Bluetooth"), ListaId::Bluetooth),
            interruptor("󰂛", tr!("No molestar"), dnd),
            interruptor("󰅶", tr!("Caffeine"), caffeine),
            interruptor("󰖔", tr!("Luz nocturna"), luz),
            lista("󰾅", tr!("Energía"), ListaId::Energia),
            lista("󰊚", tr!("Cuotas de IA"), ListaId::Cuotas),
            lista("󰐥", tr!("Sesión"), ListaId::Sesion),
        ]);
        self.marcar_no_molestar();
        self.pintar_caffeine();
        self.pintar_luz();
        self.pintar_sesion();
        self.pintar_cuotas();
        self.llenar_sesion();
        self.mandar_iconos();
        self.conectar();
    }

    fn poner_tarjeta(&self, i: usize, r: &resumen::Resumen, alerta: bool) {
        {
            let mut t = self.inner.tarjetas.borrow_mut();
            let Some(t) = t.get_mut(i) else { return };
            let nueva = Tarjeta {
                glifo: r.glifo.into(),
                estado: r.estado.clone(),
                encendida: r.encendida,
                alerta,
                ..t.clone()
            };
            if *t == nueva {
                return;
            }
            *t = nueva;
        }
        self.mandar(Orden::Tarjetas(self.inner.tarjetas.borrow().clone()));
    }

    fn mandar_iconos(&self) {
        let escala = crate::pixeles::escala() as u32;
        let icono = |n: &str| crate::icono::de_nombre(n, 16, escala, [1.0; 4]);
        let iconos = vista::Iconos {
            play: icono("media-playback-start-symbolic"),
            pausa: icono("media-playback-pause-symbolic"),
            anterior: icono("media-seek-backward-symbolic"),
            siguiente: icono("media-seek-forward-symbolic"),
            mezcla: icono("media-playlist-shuffle-symbolic"),
            repetir: icono("media-playlist-repeat-symbolic"),
            repetir_uno: icono("media-playlist-repeat-song-symbolic"),
            contraer: icono("pan-up-symbolic"),
            cerrar: icono("window-close-symbolic"),
        };
        self.mandar(Orden::Iconos(iconos));
    }

    fn conectar(&self) {
        let i = &self.inner;
        let yo = self.clone();
        i.notificaciones.al_cambiar(move |c| match c {
            Cambio::NoMolestar(_) => yo.marcar_no_molestar(),
            Cambio::Llego { .. } | Cambio::Cerrada(_) => yo.mandar_avisos(),
        });
        let yo = self.clone();
        i.lista.al_llegar(move || yo.mandar_avisos());
        let yo = self.clone();
        i.idle.al_cambiar(move || yo.pintar_caffeine());
        let yo = self.clone();
        i.luz.al_cambiar(move || yo.pintar_luz());
        let yo = self.clone();
        i.brillo.al_cambiar(move || yo.mostrar_niveles());
        let yo = self.clone();
        i.mezclador.al_cambiar(move || yo.mostrar_audio());
        let yo = self.clone();
        i.red.al_cambiar(move || yo.pintar_red());
        let yo = self.clone();
        i.bluetooth.al_cambiar(move || yo.pintar_bt());
        let yo = self.clone();
        i.bluetooth.al_pedido(move |p| yo.atender_pedido(p));
        let yo = self.clone();
        i.energia.al_cambiar(move || yo.mostrar_energia());
        let yo = self.clone();
        i.mpris.al_cambiar(move || yo.mostrar_reproductor());
        self.pintar_red();
        self.pintar_bt();
        self.mostrar_energia();
        self.mostrar_niveles();
        // Antes de la primera apertura y sin demorar el arranque.
        let yo = self.clone();
        glib::idle_add_local_once(move || yo.mandar_avisos());
    }

    fn mandar_avisos(&self) {
        let g = self.inner.lista.grupos();
        self.mandar(Orden::Avisos(g));
    }

    // ── Sesión ──

    /// Bloquear y suspender actúan directo; cerrar sesión, reiniciar y apagar piden
    /// confirmación.
    fn llenar_sesion(&self) {
        let a = &self.inner.acciones;
        let p = Parte::Lista(ListaId::Sesion);
        a.soltar(p);
        self.inner.confirmando.replace(None);
        let directo = |glifo: &str, texto: &str, args: &'static [&'static str]| {
            let yo = self.clone();
            Opcion::new(
                glifo,
                texto,
                None,
                a.tocar(p, move || {
                    yo.cerrar();
                    comando::lanzar(args);
                }),
            )
        };
        let confirmar = |glifo: &'static str,
                         texto: &str,
                         pregunta: &'static str,
                         args: &'static [&'static str]| {
            let yo = self.clone();
            Opcion::new(
                glifo,
                texto,
                None,
                a.tocar(p, move || yo.confirmar(glifo, pregunta, args)),
            )
        };
        let ops = vec![
            directo("󰌾", tr!("Bloquear pantalla"), &["loginctl", "lock-session"]),
            directo("󰤄", tr!("Suspender"), &["systemctl", "suspend"]),
            confirmar(
                "󰍃",
                tr!("Cerrar sesión"),
                tr!("Sí, cerrar sesión"),
                comando::salir_de_la_sesion(),
            ),
            confirmar(
                "󰜉",
                tr!("Reiniciar"),
                tr!("Sí, reiniciar"),
                &["systemctl", "reboot"],
            ),
            confirmar(
                "󰐥",
                tr!("Apagar"),
                tr!("Sí, apagar"),
                &["systemctl", "poweroff"],
            ),
        ];
        self.mandar(Orden::Lista(ListaId::Sesion, Contenido::Opciones(ops)));
    }

    /// El "Sí" se habilita un rato después: un doble click no confirma solo.
    fn confirmar(
        &self,
        glifo: &'static str,
        pregunta: &'static str,
        args: &'static [&'static str],
    ) {
        self.inner
            .confirmando
            .replace(Some((glifo, pregunta, args, false)));
        self.pintar_confirmacion();
        self.mandar(Orden::Enfocar(ListaId::Sesion, 1));
        let yo = self.clone();
        glib::timeout_add_local_once(CONFIRMAR_TRAS, move || {
            let mut c = yo.inner.confirmando.borrow_mut();
            let Some(x) = c.as_mut() else { return };
            if x.0 != glifo || x.3 {
                return;
            }
            x.3 = true;
            drop(c);
            yo.pintar_confirmacion();
        });
    }

    fn pintar_confirmacion(&self) {
        let Some((glifo, pregunta, args, listo)) = *self.inner.confirmando.borrow() else {
            return;
        };
        let a = &self.inner.acciones;
        let p = Parte::Lista(ListaId::Sesion);
        a.soltar(p);
        let yo = self.clone();
        let mut si = Opcion::new(
            glifo,
            pregunta,
            None,
            a.tocar(p, move || {
                yo.cerrar();
                comando::lanzar(args);
            }),
        );
        si.habilitada = listo;
        let yo = self.clone();
        let volver = Opcion::new(
            "󰁍",
            tr!("Volver"),
            None,
            a.tocar(p, move || {
                yo.llenar_sesion();
                yo.mandar(Orden::Enfocar(ListaId::Sesion, 0));
            }),
        );
        self.mandar(Orden::Lista(
            ListaId::Sesion,
            Contenido::Opciones(vec![si, volver]),
        ));
    }

    // ── Tarjetas ──

    fn pintar_sesion(&self) {
        let segundos = std::fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
            .unwrap_or_default();
        self.poner_tarjeta(SESION, &resumen::sesion(segundos as u64), false);
    }

    fn pintar_cuotas(&self) {
        use crate::cuotas;
        let ahora = cuotas::ahora();
        let lecturas: Vec<_> = cuotas::AGENTES
            .into_iter()
            .map(|a| (a, cuotas::leer(a)))
            .collect();
        let fila = |p: cuotas::Pintura, otras: Vec<Cuota>| Cuota {
            glifo: p.glifo.into(),
            nombre: p.nombre,
            renueva: p.renueva,
            porcentaje: p.porcentaje,
            fraccion: p.fraccion,
            apagada: p.apagada,
            alerta: p.alerta,
            otras,
        };
        let filas = lecturas
            .iter()
            .map(|(agente, l)| match cuotas::principal(l) {
                None => fila(cuotas::sin_datos(*agente), Vec::new()),
                Some(i) => {
                    let otras = l
                        .ventanas
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(_, v)| fila(cuotas::pintar(v, None, ahora), Vec::new()))
                        .collect();
                    fila(
                        cuotas::pintar(&l.ventanas[i], Some((*agente, l.observada)), ahora),
                        otras,
                    )
                }
            })
            .collect();
        self.mandar(Orden::Lista(ListaId::Cuotas, Contenido::Cuotas(filas)));
        let (estado, alerta) = cuotas::resumen(&lecturas, ahora);
        self.poner_tarjeta(
            CUOTAS,
            &resumen::Resumen {
                glifo: "󰊚",
                estado,
                encendida: false,
            },
            alerta,
        );
    }

    fn marcar_no_molestar(&self) {
        let r = resumen::no_molestar(self.inner.notificaciones.no_molestar());
        self.poner_tarjeta(NO_MOLESTAR, &r, false);
    }

    fn leer_comandos(&self) {
        self.pintar_caffeine();
        self.pintar_luz();
        self.mostrar_niveles();
    }

    fn pintar_caffeine(&self) {
        let r = resumen::caffeine(self.inner.idle.caffeine());
        self.poner_tarjeta(CAFFEINE, &r, false);
    }

    fn pintar_luz(&self) {
        let r = resumen::luz(self.inner.luz.prendida());
        self.poner_tarjeta(LUZ, &r, false);
    }

    fn alternar_caffeine(&self) {
        let idle = &self.inner.idle;
        idle.set_caffeine(!idle.caffeine());
        self.inner
            .osd
            .mostrar(&crate::idle::aviso_caffeine(idle.caffeine()));
    }

    pub fn aplicar_config(&self, c: &crate::config::Sonido) {
        self.inner.ocultas.replace(c.ocultas.clone());
        self.inner.salidas_vistas.replace(String::new());
        if self.inner.abierto.get() {
            self.mostrar_audio();
        }
    }

    // ── Niveles ──

    /// Las acciones de un nivel: mover y, si tiene, el glifo. Se registran una vez.
    fn acciones_nivel(&self, id: NivelId) -> (vista::Id, Option<vista::Id>) {
        if let Some(a) = self.inner.niveles.borrow().get(&id) {
            return *a;
        }
        let i = &self.inner;
        let a = &i.acciones;
        let m = i.mezclador.clone();
        let par = match id {
            NivelId::Salida => {
                let m2 = m.clone();
                (
                    a.mover(Parte::Fija, move |v| {
                        if let Some(s) = m.estado().predeterminada {
                            m.volumen_salida(&s, v);
                        }
                    }),
                    Some(a.tocar(Parte::Fija, move || {
                        if let Some(s) = m2.estado().salida_predeterminada() {
                            m2.silenciar_salida(&s.nombre, !s.mudo);
                        }
                    })),
                )
            }
            NivelId::Microfono => {
                let m2 = m.clone();
                (
                    a.mover(Parte::Fija, move |v| {
                        if let Some(e) = m.estado().entrada_predeterminada {
                            m.volumen_entrada(&e, v);
                        }
                    }),
                    Some(a.tocar(Parte::Fija, move || {
                        if let Some(e) = m2.estado().entrada_predeterminada() {
                            m2.silenciar_entrada(&e.nombre, !e.mudo);
                        }
                    })),
                )
            }
            NivelId::App(indice) => {
                let m2 = m.clone();
                (
                    a.mover(Parte::Fija, move |v| m.volumen_app(indice, v)),
                    Some(a.tocar(Parte::Fija, move || {
                        if let Some(x) = m2.estado().apps.iter().find(|x| x.indice == indice) {
                            m2.silenciar_app(indice, !x.mudo);
                        }
                    })),
                )
            }
            NivelId::Brillo => {
                let b = i.brillo.clone();
                (
                    a.mover(Parte::Fija, move |v| {
                        if let Err(e) = b.poner(v) {
                            eprintln!("morgan: brillo: {e}");
                        }
                    }),
                    None,
                )
            }
        };
        self.inner.niveles.borrow_mut().insert(id, par);
        par
    }

    fn mostrar_niveles(&self) {
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
        let mut niveles = Vec::new();
        let nivel = |id: NivelId,
                     glifo: &str,
                     nombre: &str,
                     valor: f64,
                     mudo: bool,
                     lista: Option<ListaId>| {
            let (mover, glifo_accion) = self.acciones_nivel(id);
            Nivel {
                id,
                glifo: glifo.into(),
                nombre: nombre.into(),
                valor,
                mudo,
                glifo_accion,
                lista,
                mover,
            }
        };
        if let Some(s) = e.salida_predeterminada() {
            let glifo = if s.mudo { "󰖁" } else { s.glifo() };
            niveles.push(nivel(
                NivelId::Salida,
                glifo,
                &s.descripcion,
                s.volumen,
                s.mudo,
                Some(ListaId::Salida),
            ));
        }
        for a in &e.apps {
            let actual = e.salidas.iter().find(|s| s.indice == a.salida);
            let glifo = if a.mudo {
                "󰖁"
            } else {
                actual.map_or("󰓃", |s| s.glifo())
            };
            niveles.push(nivel(
                NivelId::App(a.indice),
                glifo,
                &a.nombre,
                a.volumen,
                a.mudo,
                Some(ListaId::App(a.indice)),
            ));
        }
        match e.entrada_predeterminada() {
            Some(m) => {
                let glifo = if m.mudo { "󰍭" } else { "󰍬" };
                niveles.push(nivel(
                    NivelId::Microfono,
                    glifo,
                    &m.descripcion,
                    m.volumen,
                    m.mudo,
                    Some(ListaId::Microfono),
                ));
            }
            None => niveles.push(nivel(
                NivelId::Microfono,
                "󰍭",
                tr!("Sin micrófono"),
                0.0,
                true,
                Some(ListaId::Microfono),
            )),
        }
        if i.brillo.disponible() {
            niveles.push(nivel(
                NivelId::Brillo,
                "󰖙",
                tr!("Brillo"),
                i.brillo.valor(),
                false,
                None,
            ));
        }
        // Las apps que se fueron sueltan sus acciones.
        {
            let vivas: Vec<u32> = e.apps.iter().map(|a| a.indice).collect();
            i.niveles
                .borrow_mut()
                .retain(|id, _| !matches!(id, NivelId::App(x) if !vivas.contains(x)));
            i.apps_vistas.borrow_mut().retain(|x, _| vivas.contains(x));
        }
        self.mandar(Orden::Niveles(niveles));
        self.listas_audio(&e);
    }

    fn mostrar_audio(&self) {
        self.mostrar_niveles();
    }

    /// Las opciones se rehacen solo si cambiaron (no en cada paso de volumen).
    fn listas_audio(&self, e: &crate::mezclador::Estado) {
        let i = &self.inner;
        let a = &i.acciones;
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
            let p = Parte::Lista(ListaId::Salida);
            a.soltar(p);
            // La puesta no va: ya está en el nombre del nivel.
            let ops = e
                .salidas
                .iter()
                .filter(|s| e.predeterminada.as_deref() != Some(s.nombre.as_str()))
                .map(|s| {
                    let m = i.mezclador.clone();
                    let nombre = s.nombre.clone();
                    Opcion::new(
                        s.glifo(),
                        &s.descripcion,
                        None,
                        a.tocar(p, move || m.predeterminar(&nombre)),
                    )
                })
                .collect();
            self.mandar(Orden::Lista(ListaId::Salida, Contenido::Opciones(ops)));
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
            let p = Parte::Lista(ListaId::Microfono);
            a.soltar(p);
            let ops = e
                .entradas
                .iter()
                .filter(|s| e.entrada_predeterminada.as_deref() != Some(s.nombre.as_str()))
                .map(|s| {
                    let m = i.mezclador.clone();
                    let nombre = s.nombre.clone();
                    Opcion::new(
                        "󰍬",
                        &s.descripcion,
                        None,
                        a.tocar(p, move || m.predeterminar_entrada(&nombre)),
                    )
                })
                .collect();
            self.mandar(Orden::Lista(ListaId::Microfono, Contenido::Opciones(ops)));
        }
        // Las otras salidas de cada app: la puesta ya está en el glifo.
        for app in &e.apps {
            let clave = format!(
                "{}|{:?}",
                app.salida,
                e.salidas
                    .iter()
                    .map(|s| (s.indice, &s.descripcion))
                    .collect::<Vec<_>>()
            );
            if i.apps_vistas.borrow().get(&app.indice) == Some(&clave) {
                continue;
            }
            i.apps_vistas.borrow_mut().insert(app.indice, clave);
            let l = ListaId::App(app.indice);
            let p = Parte::Lista(l);
            a.soltar(p);
            let ops = e
                .salidas
                .iter()
                .filter(|s| s.indice != app.salida)
                .map(|s| {
                    let (m, indice, nombre) = (i.mezclador.clone(), app.indice, s.nombre.clone());
                    Opcion::new(
                        s.glifo(),
                        &s.descripcion,
                        None,
                        a.tocar(p, move || m.mover_app(indice, &nombre)),
                    )
                })
                .collect();
            self.mandar(Orden::Lista(l, Contenido::Opciones(ops)));
        }
    }

    // ── Reproductor ──

    fn mostrar_reproductor(&self) {
        let r = self.inner.mpris.actual().map(|r| {
            let caratula = r.caratula.as_ref().and_then(|p| self.caratula(p));
            vista::Reproductor {
                caratula,
                titulo: r.titulo,
                subtitulo: r.subtitulo,
                sonando: r.estado == Some(crate::reproductor::Estado::Reproduciendo),
                mezcla: r.mezcla,
                repetir: r.repetir,
                puede_anterior: r.puede_anterior,
                puede_siguiente: r.puede_siguiente,
            }
        });
        self.mandar(Orden::Reproductor(r));
    }

    /// La carátula (solo local), a 72 px; se guarda la última.
    fn caratula(&self, p: &std::path::Path) -> Option<morgan_motor::vello::peniko::ImageData> {
        if let Some(c) = self.inner.caratulas.borrow().get(p) {
            return c.clone();
        }
        let img = p
            .exists()
            .then(|| crate::pixeles::archivo(p, 72 * crate::pixeles::escala()))
            .flatten();
        let mut c = self.inner.caratulas.borrow_mut();
        c.clear();
        c.insert(p.to_owned(), img.clone());
        img
    }

    // ── Red ──

    fn pintar_red(&self) {
        let i = &self.inner;
        let e = i.red.estado();
        self.poner_tarjeta(RED, &resumen::red(&e), false);
        let a = &i.acciones;
        let p = Parte::Lista(ListaId::Red);
        a.soltar(p);
        a.soltar(Parte::Campo(ListaId::Red));
        // Cable y VPN arriba; las Wi-Fi, guardadas y a la vista, debajo de su radio y solo
        // con la radio prendida.
        let conexion = |c: &red::Conexion| {
            let r = i.red.clone();
            let ruta = c.ruta.clone();
            Opcion::new(
                c.tipo.glifo(),
                &c.nombre,
                Some(c.activa.is_some()),
                a.tocar(p, move || r.alternar(&ruta)),
            )
        };
        let (wifis, otras): (Vec<_>, Vec<_>) =
            e.conexiones.iter().partition(|c| c.tipo == red::Tipo::Wifi);
        let mut ops: Vec<Opcion> = otras.into_iter().map(conexion).collect();
        if let Some(w) = e.wifi {
            let r = i.red.clone();
            ops.push(Opcion::new(
                "󰤨",
                "Wi-Fi",
                Some(w),
                a.tocar(p, move || r.alternar_wifi()),
            ));
        }
        // Mientras se prueba una clave, NetworkManager la tiene guardada (se borra si falla):
        // no aparece como guardada, y la fila con su campo sigue donde estaba.
        let probando = i
            .clave
            .borrow()
            .as_ref()
            .filter(|p| p.ocupado)
            .map(|p| p.visible.clone());
        if e.wifi == Some(true) {
            for c in wifis {
                if probando.as_ref().is_some_and(|v| v.nombre == c.nombre) {
                    continue;
                }
                let yo = self.clone();
                let ruta = c.ruta.clone();
                let yo2 = self.clone();
                let ruta2 = c.ruta.clone();
                let mut o = conexion(c);
                o.derecha = Some(Deslizar {
                    glifo: "󰆴",
                    texto: tr!("olvidar").into(),
                    se_va: true,
                    accion: a.tocar(p, move || {
                        let yo2 = yo.clone();
                        yo.inner.red.olvidar(&ruta, move || yo2.pintar_red());
                    }),
                });
                o.izquierda = Some(Deslizar {
                    glifo: "󰆏",
                    texto: tr!("copiar la clave").into(),
                    se_va: false,
                    accion: a.tocar(p, move || yo2.copiar_clave(&ruta2)),
                });
                ops.push(o);
            }
        }
        let pedida = i.clave.borrow().as_ref().map(|p| p.visible.ssid.clone());
        let mut visibles = e.visibles.clone();
        if let Some(ssid) = &pedida
            && !visibles.iter().any(|v| &v.ssid == ssid)
        {
            match probando {
                Some(v) => visibles.insert(0, v),
                // La red se fue de la vista: el campo no tiene adónde ir.
                None => {
                    i.clave.replace(None);
                }
            }
        }
        for v in &visibles {
            let yo = self.clone();
            let v2 = v.clone();
            let mut o = Opcion::new(
                v.glifo(),
                &v.nombre,
                None,
                a.tocar(p, move || yo.elegir_wifi(&v2)),
            );
            if pedida.as_ref() == Some(&v.ssid) {
                o.abierta = true;
                o.campo = self.campo_clave();
            }
            ops.push(o);
        }
        self.mandar(Orden::Lista(ListaId::Red, Contenido::Opciones(ops)));
    }

    /// Al portapapeles con `--sensitive`: el historial (cliphist) no la guarda.
    fn copiar_clave(&self, ruta: &zbus::zvariant::OwnedObjectPath) {
        let osd = self.inner.osd.clone();
        self.inner.red.clave(ruta, move |clave| {
            let aviso = match clave {
                Some(c) => {
                    comando::lanzar_con_entrada(&["wl-copy", "--sensitive"], c.as_bytes());
                    crate::osd::Aviso::estado("󰆏", "Wi-Fi", tr!("Clave copiada"), true, 2000)
                }
                None => crate::osd::Aviso::estado(
                    "󰆏",
                    "Wi-Fi",
                    tr!("No se pudo leer la clave"),
                    false,
                    2000,
                ),
            };
            osd.mostrar(&aviso);
        });
    }

    /// Una abierta conecta al click; una con clave abre el campo debajo (o lo cierra).
    fn elegir_wifi(&self, v: &Visible) {
        let i = &self.inner;
        if !v.seguridad.pide_clave() {
            i.clave.replace(None);
            i.red.conectar_nueva(v, None, |_| {});
            self.pintar_red();
            return;
        }
        let misma = i
            .clave
            .borrow()
            .as_ref()
            .is_some_and(|p| p.visible.ssid == v.ssid);
        let nuevo = (!misma).then(|| Pedido {
            visible: v.clone(),
            generacion: self.generacion(),
            aviso: None,
            ocupado: false,
            fallos: 0,
        });
        i.clave.replace(nuevo);
        self.pintar_red();
    }

    fn campo_clave(&self) -> Option<Campo> {
        let i = &self.inner;
        let c = i.clave.borrow();
        let pedido = c.as_ref()?;
        let a = &i.acciones;
        let p = Parte::Campo(ListaId::Red);
        let yo = self.clone();
        let enviar = a.enviar(p, move |s| yo.enviar_clave(s));
        let yo = self.clone();
        let escape = a.tocar(p, move || {
            yo.inner.clave.replace(None);
            yo.pintar_red();
        });
        Some(Campo {
            generacion: pedido.generacion,
            caja: Some(Caja {
                glifo: "\u{F033E}",
                placeholder: tr!("Contraseña").into(),
                clave: true,
                fijo: None,
                boton: Some(BotonCampo::Ojo),
                ocupado: pedido.ocupado,
                enviar,
                escape,
            }),
            aviso: pedido.aviso.clone(),
            fallo: pedido.fallos,
        })
    }

    fn enviar_clave(&self, clave: String) {
        let i = &self.inner;
        let Some(v) = i.clave.borrow().as_ref().map(|p| p.visible.clone()) else {
            return;
        };
        if !red::clave_valida(v.seguridad, &clave) {
            let n = self.generacion();
            if let Some(p) = i.clave.borrow_mut().as_mut() {
                p.aviso = Some((tr!("Tiene de 8 a 63 caracteres").into(), true));
                p.fallos = n;
            }
            self.pintar_red();
            return;
        }
        if let Some(p) = i.clave.borrow_mut().as_mut() {
            p.ocupado = true;
            p.aviso = Some((tr!("Conectando…").into(), false));
        }
        self.pintar_red();
        let yo = self.clone();
        let ssid = v.ssid.clone();
        let nombre = v.nombre.clone();
        i.red.conectar_nueva(&v, Some(clave), move |r| {
            // Con el campo a la vista, el resultado va ahí; si se cerró (el centro, o Escape)
            // mientras se probaba, lo dice el OSD: nunca en silencio.
            let sigue = yo
                .inner
                .clave
                .borrow()
                .as_ref()
                .is_some_and(|p| p.visible.ssid == ssid);
            let falla = |f: &red::Falla| match f {
                red::Falla::Clave => tr!("Contraseña incorrecta"),
                red::Falla::Otra => tr!("No se pudo conectar"),
            };
            match r {
                Ok(()) => {
                    if sigue {
                        yo.inner.clave.replace(None);
                    } else {
                        yo.inner.osd.mostrar(&crate::osd::Aviso::estado(
                            "󰤨",
                            &nombre,
                            tr!("Conectada"),
                            true,
                            2000,
                        ));
                    }
                    yo.pintar_red();
                }
                Err(f) if sigue => {
                    let g = yo.generacion();
                    if let Some(p) = yo.inner.clave.borrow_mut().as_mut() {
                        // Lo escrito se borra y el campo vuelve a tomar el foco; tiembla.
                        p.ocupado = false;
                        p.generacion = g;
                        p.fallos = g;
                        p.aviso = Some((falla(&f).into(), true));
                    }
                    yo.pintar_red();
                }
                Err(f) => {
                    yo.inner
                        .osd
                        .mostrar(&crate::osd::Aviso::fallo("󰤫", &nombre, falla(&f), 3000));
                    yo.pintar_red();
                }
            }
        });
    }

    // ── Energía ──

    fn mostrar_energia(&self) {
        let i = &self.inner;
        let e = i.energia.estado();
        self.poner_tarjeta(ENERGIA, &resumen::energia(e.activo.as_deref()), false);
        let a = &i.acciones;
        let p = Parte::Lista(ListaId::Energia);
        a.soltar(p);
        // El puesto no va: ya está en el estado de la tarjeta.
        let ops = e
            .perfiles
            .iter()
            .filter(|x| e.activo.as_deref() != Some(x.as_str()))
            .map(|x| {
                let (glifo, nombre) = crate::energia::etiqueta(x);
                let en = i.energia.clone();
                let perfil = x.clone();
                Opcion::new(glifo, nombre, None, a.tocar(p, move || en.elegir(&perfil)))
            })
            .collect();
        self.mandar(Orden::Lista(ListaId::Energia, Contenido::Opciones(ops)));
    }
}
