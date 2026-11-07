//! La pantalla: superficies de `ext-session-lock` (una por salida), el teclado, el puntero, el
//! verificador (por su hilo), logind (por el suyo) y el estado; el de render (`pintor.rs`)
//! dibuja con vello y anima solo, así que una traba del principal no congela la animación.
//!
//! La usan los dos binarios: `morgan-lock` con PAM, logind y el xray de Hyprland, y
//! `morgan-greet` con greetd, dentro de su propio Hyprland, donde bloquear es la forma de
//! tener una superficie por salida con el teclado.

use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{
    EventLoop, LoopHandle, LoopSignal, RegistrationToken,
};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
};
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::session_lock::{
    SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
    SessionLockSurfaceConfigure,
};
use smithay_client_toolkit::{delegate_dispatch2, delegate_registry, registry_handlers};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface};
use wayland_client::{Connection, QueueHandle};

use morgan_ipc::bloqueo::Bloqueo;

use crate::clave::Clave;
use crate::dibujo::{ANCHO_FLECHA, Y_CAMPO, Y_RENGLON};
use crate::eleccion::{AVANCE_MS, Eleccion, Paso, Que, Zona};
use crate::escena::{
    ALTO, ANCHO, Borde, Cambio, DESTELLO_MS, ENTRADA_MS, Estado, FALLO_MS, Paleta, SALIDA_MS, ahora,
};
use crate::logind::Logind;
use crate::perfil::{self, Rutas};
use crate::pintor::{self, Hilo, Orden};
use crate::verificar::{Resultado, Verificador};
use crate::xray;

/// Lo que distingue al bloqueo del greeter.
pub struct Ajustes {
    pub verificador: Box<dyn Verificador>,
    /// `LockedHint` y `Unlock` de la sesión de logind.
    pub logind: bool,
    /// `misc:session_lock_xray` de Hyprland en los fundidos (`xray.rs`).
    pub xray: bool,
    /// Paleta, fondo y config; sin rutas, los valores de fábrica.
    pub rutas: Option<Rutas>,
    /// El greeter: cuentas y sesiones, y por cuáles empezar (`eleccion.rs`). Con elección,
    /// el puntero se ve y sirve.
    pub eleccion: Option<(Vec<String>, Vec<String>, usize, usize)>,
}

/// Lo que llega al hilo principal desde los otros.
enum Aviso {
    /// El verificador vuelve con el resultado, listo para el próximo intento.
    Verificado(Resultado, Box<dyn Verificador>),
    /// `Unlock` de logind o SIGUSR1: quien puede mandarlos ya puede `loginctl unlock-session`.
    Desbloquear,
    Desperto,
}

enum ALogind {
    Marcar(bool, Option<mpsc::Sender<()>>),
}

/// Corre hasta que se acierta la clave (o algo falla). Se llama antes de crear hilos: bloquea
/// SIGUSR1 para todos y fija el locale.
pub fn correr(a: Ajustes) -> ExitCode {
    // SIGUSR1 se bloquea antes de crear hilos (la máscara se hereda) y la espera uno propio.
    bloquear_sigusr1();
    // SAFETY: al principio, antes de cualquier otro hilo; "" toma el locale del entorno
    // (la fecha en el idioma de la sesión, como GTK).
    unsafe { libc::setlocale(libc::LC_ALL, c"".as_ptr()) };
    if !a.xray {
        xray::desactivar();
    }

    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("morgan-lock: wayland: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (globales, cola) = match registry_queue_init::<App>(&conn) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("morgan-lock: wayland: {e}");
            return ExitCode::FAILURE;
        }
    };
    let qh = cola.handle();
    let mut bucle: EventLoop<App> = EventLoop::try_new().expect("bucle principal");
    let (avisos, rx_avisos) = channel::channel::<Aviso>();
    bucle
        .handle()
        .insert_source(rx_avisos, |ev, _, app| {
            if let channel::Event::Msg(a) = ev {
                app.aviso(a);
            }
        })
        .expect("canal de avisos");
    esperar_sigusr1(avisos.clone());

    // El fondo se decodifica y desenfoca en paralelo con la GPU; el render lo espera antes
    // del primer cuadro.
    let rutas = a.rutas;
    let fondo = {
        let ruta = rutas.as_ref().map(|r| r.fondo.clone());
        std::thread::spawn(move || ruta.and_then(|r| perfil::cargar_fondo(&r)))
    };
    let paleta = rutas
        .as_ref()
        .map(|r| perfil::paleta(&r.tema))
        .unwrap_or_else(|| Paleta::desde_css(""));
    let (estilo, borde) = rutas
        .as_ref()
        .map(|r| perfil::estilo(&r.config))
        .unwrap_or((Bloqueo::default(), Borde::Sombra));

    let eleccion = a
        .eleccion
        .map(|(usuarios, sesiones, u, s)| Eleccion::new(usuarios, sesiones, u, s));
    let logind = a.logind.then(|| lanzar_logind(avisos.clone()));
    let session_lock_state = SessionLockState::new(&globales, &qh);
    let compositor = match CompositorState::bind(&globales, &qh) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("morgan-lock: wl_compositor: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Antes del bloqueo: el primer cuadro ya tiene que verse sobre el escritorio (xray.rs).
    let cruzado = !logind.as_ref().is_some_and(|(_, bloqueada)| {
        bloqueada
            .recv_timeout(Duration::from_millis(200))
            .unwrap_or(true)
    });
    xray::poner(cruzado);

    let mut estado = Estado::new(paleta, estilo);
    estado.borde = borde;
    if let Some(el) = &eleccion {
        estado.aplicar(Cambio::Elegir {
            usuarios: el.usuarios.clone(),
            sesiones: el.sesiones.clone(),
            u: el.u,
            s: el.s,
        });
    }
    let pintor = pintor::lanzar(conn.clone(), estado, fondo);

    let mut app = App {
        conn: conn.clone(),
        qh: qh.clone(),
        bucle: bucle.handle(),
        fin: bucle.get_signal(),
        registro: RegistryState::new(&globales),
        compositor,
        salidas: OutputState::new(&globales, &qh),
        asientos: SeatState::new(&globales, &qh),
        bloqueos: session_lock_state,
        bloqueo: None,
        superficies: Vec::new(),
        proximo_id: 0,
        teclado: None,
        puntero: None,
        clave: Clave::new(),
        ctrl: false,
        mayus: None,
        ocupado: false,
        fallos: 0,
        generacion: 0,
        saliendo: false,
        estilo,
        verificador: Some(a.verificador),
        cursor: if eleccion.is_some() {
            CursorShapeManager::bind(&globales, &qh).ok()
        } else {
            None
        },
        forma: None,
        eleccion,
        avanzando: false,
        encima: None,
        puntero_en: None,
        serial_puntero: 0,
        avisos,
        logind: logind.map(|(tx, _)| tx),
        pintor,
        reloj: None,
        cruzado,
        salida: ExitCode::SUCCESS,
        duena: None,
        puntero_visto: false,
        con_teclado: None,
        greeter: !a.xray,
    };

    match app.bloqueos.lock(&qh) {
        Ok(b) => app.bloqueo = Some(b),
        Err(e) => {
            xray::poner(false);
            eprintln!("morgan-lock: el compositor no tiene ext-session-lock: {e}");
            app.pintor.terminar();
            return ExitCode::FAILURE;
        }
    }
    for s in app.salidas.outputs().collect::<Vec<_>>() {
        app.cubrir(s);
    }
    // Sin nada que elegir, la cuenta y la sesión ya están.
    if app
        .eleccion
        .as_ref()
        .is_some_and(|e| e.actual() == Paso::Clave)
    {
        app.cuenta_elegida();
    }
    app.reloj();

    WaylandSource::new(conn, cola)
        .insert(bucle.handle())
        .expect("cola de wayland");
    if let Err(e) = bucle.run(None, &mut app, |_| {}) {
        eprintln!("morgan-lock: {e}");
        return ExitCode::FAILURE;
    }
    app.salida
}

struct Superficie {
    id: u32,
    salida: wl_output::WlOutput,
    lock: SessionLockSurface,
    tam: (u32, u32),
    escala: i32,
}

struct App {
    conn: Connection,
    qh: QueueHandle<App>,
    bucle: LoopHandle<'static, App>,
    fin: LoopSignal,
    registro: RegistryState,
    compositor: CompositorState,
    salidas: OutputState,
    asientos: SeatState,
    bloqueos: SessionLockState,
    bloqueo: Option<SessionLock>,
    superficies: Vec<Superficie>,
    proximo_id: u32,
    teclado: Option<wl_keyboard::WlKeyboard>,
    puntero: Option<wl_pointer::WlPointer>,
    clave: Clave,
    ctrl: bool,
    mayus: Option<bool>,
    /// PAM está verificando: no se tipea ni se envía otra vez.
    ocupado: bool,
    fallos: u32,
    /// Sube con cada cambio del campo: un temporizador viejo no pisa uno nuevo.
    generacion: u64,
    /// Ya empezó la salida: un segundo pedido no desbloquea dos veces.
    saliendo: bool,
    estilo: Bloqueo,
    /// En su hilo mientras verifica (`ocupado`).
    verificador: Option<Box<dyn Verificador>>,
    avisos: Sender<Aviso>,
    logind: Option<async_channel::Sender<ALogind>>,
    pintor: Hilo,
    reloj: Option<RegistrationToken>,
    cruzado: bool,
    salida: ExitCode,
    /// La superficie que muestra el contenido (`escena::Presencia`): la del puntero. Mientras
    /// el puntero no entró a ninguna, la del teclado.
    duena: Option<u32>,
    puntero_visto: bool,
    con_teclado: Option<u32>,
    /// En el greeter, si el teclado no entra solo se le pide a Hyprland (`foco.rs`).
    greeter: bool,
    /// Solo con elección: la forma del puntero (`wp_cursor_shape_v1`).
    cursor: Option<CursorShapeManager>,
    forma: Option<WpCursorShapeDeviceV1>,
    /// El mismo modelo que el render, sin animar: qué paso es y qué está elegido.
    eleccion: Option<Eleccion>,
    /// Entre el destello y el cambio de paso: no se acepta otro.
    avanzando: bool,
    encima: Option<Zona>,
    /// La superficie y el punto (lógico) del puntero.
    puntero_en: Option<(u32, f64, f64)>,
    serial_puntero: u32,
}

/// `BTN_LEFT` de `linux/input-event-codes.h`.
const BTN_IZQUIERDO: u32 = 0x110;

impl App {
    /// Una superficie de bloqueo por salida.
    fn cubrir(&mut self, salida: wl_output::WlOutput) {
        let Some(b) = &self.bloqueo else { return };
        if self.superficies.iter().any(|s| s.salida == salida) {
            return;
        }
        let wl = self.compositor.create_surface(&self.qh);
        let lock = b.create_lock_surface(wl, &salida, &self.qh);
        self.proximo_id += 1;
        self.superficies.push(Superficie {
            id: self.proximo_id,
            salida,
            lock,
            tam: (0, 0),
            escala: 1,
        });
    }

    fn id_de(&self, wl: &wl_surface::WlSurface) -> Option<u32> {
        self.superficies
            .iter()
            .find(|s| s.lock.wl_surface() == wl)
            .map(|s| s.id)
    }

    /// Sin teclado todavía: la salida del contenido (o la primera) pasa a ser la enfocada.
    fn pedir_teclado(&mut self) {
        if self.con_teclado.is_some() {
            return;
        }
        let s = self
            .duena
            .and_then(|d| self.superficies.iter().find(|s| s.id == d))
            .or_else(|| self.superficies.first());
        let Some(info) = s.and_then(|s| self.salidas.info(&s.salida)) else {
            return;
        };
        let (Some(nombre), Some((x, y)), Some((w, h))) =
            (info.name, info.logical_position, info.logical_size)
        else {
            return;
        };
        eprintln!("morgan-greet: el teclado no entró; se enfoca {nombre}");
        crate::foco::pedir(&nombre, (x + w / 2, y + h / 2));
    }

    fn nombrar(&mut self, id: u32) {
        if self.duena != Some(id) {
            self.duena = Some(id);
            self.pintor.cambio(Cambio::Duena { id, t: ahora() });
        }
    }

    fn mandar_superficie(&self, i: usize) {
        let s = &self.superficies[i];
        self.pintor.mandar(Orden::Superficie {
            id: s.id,
            wl: s.lock.wl_surface().clone(),
            ancho: s.tam.0,
            alto: s.tam.1,
            escala: s.escala,
        });
    }

    fn aviso(&mut self, a: Aviso) {
        match a {
            Aviso::Verificado(r, v) => {
                self.verificador = Some(v);
                self.ocupado = false;
                match r {
                    Resultado::Bien => self.desbloquear(),
                    Resultado::Fallo(mensajes) => {
                        self.pintor
                            .cambio(Cambio::Aviso(mensajes.into_iter().next()));
                        self.fallo();
                    }
                    Resultado::Error(e) => {
                        eprintln!("morgan-lock: {e}");
                        self.fallo();
                    }
                }
            }
            Aviso::Desbloquear => self.desbloquear(),
            Aviso::Desperto => self.reloj(),
        }
    }

    fn fallo(&mut self) {
        self.fallos += 1;
        self.generacion += 1;
        let g = self.generacion;
        self.pintor.cambio(Cambio::Fallo {
            n: self.fallos,
            t: ahora(),
        });
        let _ = self.bucle.insert_source(
            Timer::from_duration(Duration::from_millis(FALLO_MS as u64)),
            move |_, _, app| {
                if app.generacion == g {
                    app.normal();
                }
                TimeoutAction::Drop
            },
        );
    }

    fn normal(&mut self) {
        self.generacion += 1;
        self.pintor.cambio(Cambio::Normal { t: ahora() });
    }

    fn tecla(&mut self, ev: KeyEvent) {
        if self.saliendo || self.ocupado || self.avanzando {
            return;
        }
        if let Some(paso) = self.eleccion.as_ref().map(Eleccion::actual) {
            match paso {
                Paso::Elegir(q) => return self.tecla_eleccion(q, ev.keysym),
                // Con el campo vacío, Escape vuelve un paso.
                Paso::Clave if ev.keysym == Keysym::Escape && self.clave.is_empty() => {
                    return self.volver(None);
                }
                Paso::Clave => {}
            }
        }
        let antes = self.clave.len();
        match ev.keysym {
            Keysym::Escape => self.clave.vaciar(),
            Keysym::Return | Keysym::KP_Enter => return self.enviar(),
            Keysym::BackSpace => {
                self.clave.borrar_uno();
            }
            _ => {
                if self.ctrl {
                    return;
                }
                if let Some(s) = ev.utf8.as_deref() {
                    self.clave.agregar(s);
                }
            }
        }
        let n = self.clave.len();
        let t = ahora();
        if n > antes {
            // Tipear saca el fallo (el anillo vuelve; el temporizador viejo queda sin efecto).
            self.generacion += 1;
            self.pintor.cambio(Cambio::Agregar { n, t });
        } else if n < antes {
            let todo = n == 0 && antes > 1;
            self.pintor.cambio(Cambio::Quitar { n, todo, t });
        }
    }

    fn tecla_eleccion(&mut self, q: Que, k: Keysym) {
        match k {
            Keysym::Left | Keysym::KP_Left => self.girar(q, -1),
            Keysym::Right | Keysym::KP_Right => self.girar(q, 1),
            Keysym::Return | Keysym::KP_Enter => self.avanzar(),
            Keysym::Escape | Keysym::BackSpace => self.volver(None),
            _ => {}
        }
    }

    fn girar(&mut self, q: Que, d: i32) {
        let Some(el) = &mut self.eleccion else { return };
        let i = el.lista(q).1 as i32 + d;
        let t = ahora();
        if i >= 0 && el.girar(q, i as usize, t) {
            self.pintor.cambio(Cambio::Girar {
                que: q,
                i: i as usize,
                t,
            });
        }
    }

    /// El destello y, cuando el anillo se completó, el paso siguiente.
    fn avanzar(&mut self) {
        if self.avanzando
            || self
                .eleccion
                .as_ref()
                .is_none_or(|e| e.actual() == Paso::Clave)
        {
            return;
        }
        self.avanzando = true;
        self.pintor.cambio(Cambio::Avance { t: ahora() });
        let _ = self.bucle.insert_source(
            Timer::from_duration(Duration::from_millis(AVANCE_MS as u64)),
            |_, _, app| {
                app.avanzando = false;
                if let Some(el) = &mut app.eleccion {
                    let (k, t) = (el.paso + 1, ahora());
                    el.ir(k, t);
                    app.pintor.cambio(Cambio::Ir { paso: el.paso, t });
                    if el.actual() == Paso::Clave {
                        app.cuenta_elegida();
                    }
                }
                app.refrescar_encima();
                TimeoutAction::Drop
            },
        );
    }

    /// Al paso `k`, o al anterior. Lo tipeado se pierde.
    fn volver(&mut self, k: Option<usize>) {
        let Some(el) = &mut self.eleccion else { return };
        let Some(k) = k.or_else(|| el.paso.checked_sub(1)) else {
            return;
        };
        if k >= el.paso {
            return;
        }
        let t = ahora();
        el.ir(k, t);
        self.pintor.cambio(Cambio::Ir { paso: k, t });
        if !self.clave.is_empty() {
            self.clave.vaciar();
            self.pintor.cambio(Cambio::Vaciar { t });
        }
        if matches!(self.fallos, 1..) {
            self.normal();
        }
        self.refrescar_encima();
    }

    /// La cuenta y la sesión quedaron: el verificador se entera antes de la primera clave.
    fn cuenta_elegida(&mut self) {
        if let (Some(el), Some(v)) = (&self.eleccion, &mut self.verificador) {
            v.elegir(el.u, el.s);
        }
    }

    /// Qué hay bajo el puntero, en la superficie que muestra el contenido.
    fn zona(&self, id: u32, x: f64, y: f64) -> Option<Zona> {
        let el = self.eleccion.as_ref()?;
        if Some(id) != self.duena {
            return None;
        }
        let (w, h) = self.superficies.iter().find(|s| s.id == id)?.tam;
        let (w, h) = (w as f64, h as f64);
        let (ancho, alto) = (ANCHO as f64, ALTO as f64);
        let x0 = (w / 2.0 - ancho / 2.0).round();
        let y0 = (h / 2.0 - Y_CAMPO - alto / 2.0).round();
        if (x0..x0 + ancho).contains(&x) && (y0..y0 + alto).contains(&y) {
            let Paso::Elegir(q) = el.actual() else {
                return None;
            };
            let (lista, i) = el.lista(q);
            return if x < x0 + ANCHO_FLECHA {
                (i > 0).then_some(Zona::Antes)
            } else if x >= x0 + ancho - ANCHO_FLECHA {
                (i + 1 < lista.len()).then_some(Zona::Despues)
            } else {
                Some(Zona::Centro)
            };
        }
        let yr = y0 - Y_RENGLON;
        if (y - yr).abs() <= 14.0 {
            let cx = w / 2.0;
            let q = if (cx - 240.0..cx - 6.0).contains(&x) {
                Que::Usuario
            } else if (cx + 6.0..cx + 240.0).contains(&x) {
                Que::Sesion
            } else {
                return None;
            };
            return el.miga_vuelve(q).map(|_| Zona::Miga(q));
        }
        None
    }

    fn refrescar_encima(&mut self) {
        let z = self.puntero_en.and_then(|(id, x, y)| self.zona(id, x, y));
        let z = if self.avanzando || self.ocupado || self.saliendo {
            None
        } else {
            z
        };
        if z != self.encima {
            if z.is_some() != self.encima.is_some()
                && let Some(f) = &self.forma
            {
                let forma = if z.is_some() {
                    Shape::Pointer
                } else {
                    Shape::Default
                };
                f.set_shape(self.serial_puntero, forma);
            }
            self.encima = z;
            self.pintor.cambio(Cambio::Encima(z));
        }
    }

    fn click(&mut self) {
        if self.saliendo || self.ocupado || self.avanzando {
            return;
        }
        let Some(el) = &self.eleccion else { return };
        let actual = el.actual();
        match (self.encima, actual) {
            (Some(Zona::Antes), Paso::Elegir(q)) => self.girar(q, -1),
            (Some(Zona::Despues), Paso::Elegir(q)) => self.girar(q, 1),
            (Some(Zona::Centro), Paso::Elegir(_)) => self.avanzar(),
            (Some(Zona::Miga(q)), _) => {
                let k = el.miga_vuelve(q);
                self.volver(k);
            }
            _ => {}
        }
        self.refrescar_encima();
    }

    fn enviar(&mut self) {
        if self.clave.is_empty() {
            return;
        }
        let Some(mut v) = self.verificador.take() else {
            return;
        };
        self.ocupado = true;
        let clave = self.clave.tomar();
        let t = ahora();
        self.pintor.cambio(Cambio::Vaciar { t });
        self.generacion += 1;
        self.pintor.cambio(Cambio::Verificando { t });
        let tx = self.avisos.clone();
        std::thread::spawn(move || {
            let r = match v.verificar(clave) {
                Resultado::Bien => v
                    .al_entrar()
                    .map_or_else(Resultado::Error, |()| Resultado::Bien),
                otro => otro,
            };
            let _ = tx.send(Aviso::Verificado(r, v));
        });
    }

    /// El destello y el fundido quedan declarados en el render con el acierto; acá solo se
    /// espera lo que duran y se suelta el bloqueo.
    fn desbloquear(&mut self) {
        if self.saliendo {
            return;
        }
        self.saliendo = true;
        xray::poner_luego(true);
        self.pintor.cambio(Cambio::Acierto { t: ahora() });
        let destello = if self.estilo.destello { DESTELLO_MS } else { 0 };
        let _ = self.bucle.insert_source(
            Timer::from_duration(Duration::from_millis((destello + SALIDA_MS) as u64)),
            |_, _, app| {
                app.soltar();
                TimeoutAction::Drop
            },
        );
    }

    fn soltar(&mut self) {
        // Primero el render: no puede quedar presentando sobre superficies destruidas.
        self.pintor.terminar();
        if let Some(b) = self.bloqueo.take() {
            b.unlock();
        }
        self.superficies.clear();
        let _ = self.conn.roundtrip();
        xray::poner(false);
        if let Some(l) = &self.logind {
            let (tx, rx) = mpsc::channel();
            if l.send_blocking(ALogind::Marcar(false, Some(tx))).is_ok() {
                let _ = rx.recv_timeout(Duration::from_secs(2));
            }
        }
        self.fin.stop();
    }

    /// El reloj cambia al cambiar el minuto: un temporizador al próximo, no un sondeo.
    /// Llamarlo de nuevo (al despertar: el monotónico no corre suspendido) reemplaza al
    /// pendiente.
    fn reloj(&mut self) {
        let (hora, minutos, fecha, falta) = perfil::ahora_local();
        self.pintor.cambio(Cambio::Reloj {
            hora,
            minutos,
            fecha,
        });
        if let Some(viejo) = self.reloj.take() {
            self.bucle.remove(viejo);
        }
        self.reloj = self
            .bucle
            .insert_source(Timer::from_duration(falta), |_, _, app| {
                app.reloj = None;
                app.reloj();
                TimeoutAction::Drop
            })
            .ok();
    }
}

/// logind en su hilo: contesta `LockedHint` (para el fundido cruzado), escucha `Unlock` y el
/// despertar, y marca la sesión cuando se le pide.
fn lanzar_logind(avisos: Sender<Aviso>) -> (async_channel::Sender<ALogind>, mpsc::Receiver<bool>) {
    let (tx, rx) = async_channel::unbounded::<ALogind>();
    let (tx_hint, rx_hint) = mpsc::channel();
    std::thread::spawn(move || {
        futures_lite::future::block_on(async move {
            let l = match Logind::conectar().await {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("morgan-lock: logind: {e}");
                    return;
                }
            };
            let _ = tx_hint.send(l.bloqueada().await.unwrap_or(true));
            let a = avisos.clone();
            let unlock = async {
                if l.esperar_unlock().await.is_ok() {
                    let _ = a.send(Aviso::Desbloquear);
                }
            };
            let despertar = async {
                if let Err(e) = l
                    .al_despertar(|| {
                        let _ = avisos.send(Aviso::Desperto);
                    })
                    .await
                {
                    eprintln!("morgan-lock: logind: {e}");
                }
            };
            let ordenes = async {
                while let Ok(ALogind::Marcar(si, listo)) = rx.recv().await {
                    if let Err(e) = l.marcar_bloqueada(si).await {
                        eprintln!("morgan-lock: logind: {e}");
                    }
                    if let Some(listo) = listo {
                        let _ = listo.send(());
                    }
                }
            };
            futures_lite::future::zip(unlock, futures_lite::future::zip(despertar, ordenes)).await;
        });
    });
    (tx, rx_hint)
}

fn bloquear_sigusr1() {
    // SAFETY: máscara local inicializada con sigemptyset antes de usarse.
    unsafe {
        let mut m: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut m);
        libc::sigaddset(&mut m, libc::SIGUSR1);
        libc::pthread_sigmask(libc::SIG_BLOCK, &m, std::ptr::null_mut());
    }
}

/// SIGUSR1 desbloquea, como en `morgan-lock`.
fn esperar_sigusr1(avisos: Sender<Aviso>) {
    std::thread::spawn(move || {
        // SAFETY: igual que arriba; sigwait escribe la señal en `sig`.
        unsafe {
            let mut m: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut m);
            libc::sigaddset(&mut m, libc::SIGUSR1);
            let mut sig = 0;
            while libc::sigwait(&m, &mut sig) == 0 {
                if avisos.send(Aviso::Desbloquear).is_err() {
                    return;
                }
            }
        }
    });
}

impl SessionLockHandler for App {
    fn locked(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        if let Some(l) = &self.logind {
            let _ = l.send_blocking(ALogind::Marcar(true, None));
        }
        if self.greeter {
            // El tiempo de que las superficies se mapeen y Hyprland reparta el foco.
            let _ = self.bucle.insert_source(
                Timer::from_duration(Duration::from_millis(300)),
                |_, _, app| {
                    app.pedir_teclado();
                    TimeoutAction::Drop
                },
            );
        }
        if self.cruzado {
            let _ = self.bucle.insert_source(
                Timer::from_duration(Duration::from_millis((ENTRADA_MS + 50) as u64)),
                |_, _, _| {
                    xray::poner_luego(false);
                    TimeoutAction::Drop
                },
            );
        }
    }

    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        xray::poner(false);
        eprintln!("morgan-lock: no se pudo bloquear (¿otro bloqueo activo?)");
        self.pintor.terminar();
        self.bloqueo = None;
        self.superficies.clear();
        self.salida = ExitCode::FAILURE;
        self.fin.stop();
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        superficie: SessionLockSurface,
        c: SessionLockSurfaceConfigure,
        _: u32,
    ) {
        if let Some(i) = self
            .superficies
            .iter()
            .position(|s| s.lock.wl_surface() == superficie.wl_surface())
        {
            self.superficies[i].tam = c.new_size;
            self.mandar_superficie(i);
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if let Some(i) = self
            .superficies
            .iter()
            .position(|s| s.lock.wl_surface() == surface)
        {
            self.superficies[i].escala = factor.max(1);
            if self.superficies[i].tam.0 > 0 {
                self.mandar_superficie(i);
            }
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.salidas
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, salida: wl_output::WlOutput) {
        self.cubrir(salida);
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        salida: wl_output::WlOutput,
    ) {
        let Some(i) = self.superficies.iter().position(|s| s.salida == salida) else {
            return;
        };
        let s = self.superficies.remove(i);
        // El render suelta su superficie de wgpu antes de que se destruya la de Wayland.
        let (tx, rx) = mpsc::channel();
        self.pintor.mandar(Orden::Quitar {
            id: s.id,
            listo: tx,
        });
        let _ = rx.recv_timeout(Duration::from_secs(1));
        if self.con_teclado == Some(s.id) {
            self.con_teclado = None;
        }
        // Si se fue la del contenido, pasa a la del teclado, o a cualquiera.
        if self.duena == Some(s.id) {
            self.duena = None;
            if let Some(id) = self
                .con_teclado
                .or_else(|| self.superficies.first().map(|s| s.id))
            {
                self.nombrar(id);
            }
        }
        drop(s);
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.asientos
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        c: Capability,
    ) {
        // En el bloqueo, el puntero solo se pide para esconderlo; con elección, se ve y sirve.
        if c == Capability::Pointer && self.puntero.is_none() {
            self.puntero = self.asientos.get_pointer(qh, &seat).ok();
            if let (Some(m), Some(p)) = (&self.cursor, &self.puntero) {
                self.forma = Some(m.get_shape_device(p, qh));
            }
        }
        if c == Capability::Keyboard && self.teclado.is_none() {
            match self.asientos.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                self.bucle.clone(),
                Box::new(|app, _, ev| app.tecla(ev)),
            ) {
                Ok(k) => self.teclado = Some(k),
                Err(e) => eprintln!("morgan-lock: teclado: {e}"),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        c: Capability,
    ) {
        if c == Capability::Keyboard
            && let Some(k) = self.teclado.take()
        {
            k.release();
        }
        if c == Capability::Pointer
            && let Some(p) = self.puntero.take()
        {
            p.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    /// El compositor le da el teclado a la superficie de la salida enfocada: hasta que el
    /// puntero entre a alguna, el contenido va ahí.
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        wl: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.con_teclado = self.id_de(wl);
        if !self.puntero_visto
            && let Some(id) = self.con_teclado
        {
            self.nombrar(id);
        }
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        ev: KeyEvent,
    ) {
        self.tecla(ev);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        ev: KeyEvent,
    ) {
        self.tecla(ev);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    /// Bloq Mayús sale de los modificadores de xkb, no de la tecla.
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        m: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.ctrl = m.ctrl;
        if self.mayus != Some(m.caps_lock) {
            self.mayus = Some(m.caps_lock);
            self.pintor.cambio(Cambio::Mayus {
                si: m.caps_lock,
                t: ahora(),
            });
        }
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        puntero: &wl_pointer::WlPointer,
        eventos: &[PointerEvent],
    ) {
        for e in eventos {
            let id = self.id_de(&e.surface);
            match e.kind {
                PointerEventKind::Enter { serial } => {
                    match &self.forma {
                        Some(f) => f.set_shape(serial, Shape::Default),
                        None => puntero.set_cursor(serial, None, 0, 0),
                    }
                    // La forma recién puesta es la de siempre: lo de encima se recalcula.
                    if self.encima.take().is_some() {
                        self.pintor.cambio(Cambio::Encima(None));
                    }
                    self.serial_puntero = serial;
                    // El contenido sigue al puntero (`escena::Presencia`).
                    if let Some(id) = id {
                        self.puntero_visto = true;
                        self.nombrar(id);
                    }
                }
                PointerEventKind::Leave { .. } => self.puntero_en = None,
                PointerEventKind::Press { button, .. } if button == BTN_IZQUIERDO => {
                    self.click();
                }
                _ => {}
            }
            if !matches!(e.kind, PointerEventKind::Leave { .. })
                && let Some(id) = id
            {
                self.puntero_en = Some((id, e.position.0, e.position.1));
            }
        }
        if self.eleccion.is_some() {
            self.refrescar_encima();
        }
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registro
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(App);
delegate_dispatch2!(App);
