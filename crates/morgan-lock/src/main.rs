//! Binario aparte de morgan: si morgan se cae, el bloqueo sigue.
//!
//!   morgan-lock [--pam SERVICIO] [--sin-logind]
//!
//! `--pam vlock` y `--sin-logind` son para probarlo en un Hyprland anidado: un fallo no suma
//! a faillock y no se toca el `LockedHint` de la sesión real.
//!
//! Sin toolkit: el hilo principal hace Wayland (sctk), el teclado, PAM (por su hilo), logind
//! (por el suyo) y el estado; el de render (`pintor.rs`) dibuja con vello y anima solo, así
//! que una traba del principal no congela la animación.

mod caracteres;
mod clave;
mod dibujo;
mod escena;
mod fondo;
mod logind;
mod pam;
mod pintor;
mod texto;
mod xray;

use std::path::PathBuf;
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
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::session_lock::{
    SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
    SessionLockSurfaceConfigure,
};
use smithay_client_toolkit::{delegate_dispatch2, delegate_registry, registry_handlers};
use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface};
use wayland_client::{Connection, QueueHandle};

use morgan_ipc::bloqueo::Bloqueo;

use crate::clave::Clave;
use crate::escena::{Cambio, DESTELLO_MS, ENTRADA_MS, Estado, FALLO_MS, Paleta, SALIDA_MS, ahora};
use crate::logind::Logind;
use crate::pam::Resultado;
use crate::pintor::{Hilo, Orden};

struct Opciones {
    servicio: String,
    logind: bool,
}

fn opciones() -> Result<Opciones, String> {
    let mut o = Opciones {
        servicio: "morgan-lock".into(),
        logind: true,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--pam" => o.servicio = args.next().ok_or("--pam necesita un servicio")?,
            "--sin-logind" => o.logind = false,
            otro => return Err(format!("argumento desconocido: {otro}")),
        }
    }
    Ok(o)
}

/// Lo que llega al hilo principal desde los otros.
enum Aviso {
    Pam(Resultado),
    /// `Unlock` de logind o SIGUSR1: quien puede mandarlos ya puede `loginctl unlock-session`.
    Desbloquear,
    Desperto,
}

enum ALogind {
    Marcar(bool, Option<mpsc::Sender<()>>),
}

fn main() -> ExitCode {
    // SIGUSR1 se bloquea antes de crear hilos (la máscara se hereda) y la espera uno propio.
    bloquear_sigusr1();
    // SAFETY: al principio, antes de cualquier otro hilo; "" toma el locale del entorno
    // (la fecha en el idioma de la sesión, como GTK).
    unsafe { libc::setlocale(libc::LC_ALL, c"".as_ptr()) };

    let o = match opciones() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("morgan-lock: {e}\nuso: morgan-lock [--pam SERVICIO] [--sin-logind]");
            return ExitCode::FAILURE;
        }
    };
    if !pam::servicio_existe(&o.servicio) {
        eprintln!(
            "morgan-lock: no existe el servicio PAM «{}» (/etc/pam.d): no se bloquea",
            o.servicio
        );
        return ExitCode::FAILURE;
    }
    let Some(usuario) = pam::usuario() else {
        eprintln!("morgan-lock: no se encontró el usuario de este proceso");
        return ExitCode::FAILURE;
    };

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
    let fondo = std::thread::spawn(cargar_fondo);
    let paleta = Paleta::desde_css(
        &config_dir()
            .map(|d| d.join("morgan/theme.css"))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default(),
    );
    let estilo = estilo();

    let logind = o.logind.then(|| lanzar_logind(avisos.clone()));
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

    let pintor = pintor::lanzar(conn.clone(), Estado::new(paleta, estilo), fondo);

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
        servicio: o.servicio,
        usuario,
        avisos,
        logind: logind.map(|(tx, _)| tx),
        pintor,
        reloj: None,
        cruzado,
        salida: ExitCode::SUCCESS,
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
    servicio: String,
    usuario: String,
    avisos: Sender<Aviso>,
    logind: Option<async_channel::Sender<ALogind>>,
    pintor: Hilo,
    reloj: Option<RegistrationToken>,
    cruzado: bool,
    salida: ExitCode,
}

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
            Aviso::Pam(r) => {
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
        if self.saliendo || self.ocupado {
            return;
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

    fn enviar(&mut self) {
        if self.clave.is_empty() {
            return;
        }
        self.ocupado = true;
        let clave = self.clave.tomar();
        let t = ahora();
        self.pintor.cambio(Cambio::Vaciar { t });
        self.generacion += 1;
        self.pintor.cambio(Cambio::Verificando { t });
        let (s, u, tx) = (
            self.servicio.clone(),
            self.usuario.clone(),
            self.avisos.clone(),
        );
        std::thread::spawn(move || {
            let _ = tx.send(Aviso::Pam(pam::autenticar(&s, &u, clave)));
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
        let (hora, minutos, fecha, falta) = ahora_local();
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

/// Hora, minutos, fecha («jueves 22 de octubre») y lo que falta para el próximo minuto.
fn ahora_local() -> (String, String, String, Duration) {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: punteros a structs locales, del tamaño que piden.
    let tm = unsafe {
        libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts);
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&ts.tv_sec, &mut tm);
        tm
    };
    let formato = |f: &std::ffi::CStr| {
        let mut buf = [0u8; 128];
        // SAFETY: `buf` tiene el tamaño que se pasa; `tm` salió de localtime_r.
        let n = unsafe { libc::strftime(buf.as_mut_ptr().cast(), buf.len(), f.as_ptr(), &tm) };
        String::from_utf8_lossy(&buf[..n]).into_owned()
    };
    let falta = 60_000 - (tm.tm_sec as u64 * 1000 + ts.tv_nsec as u64 / 1_000_000).min(59_999);
    (
        formato(c"%H"),
        formato(c"%M"),
        formato(c"%A %-d de %B"),
        Duration::from_millis(falta.max(1)),
    )
}

fn cargar_fondo() -> Option<dibujo::Fondo> {
    let ruta = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?
        .join("wallpaper-lock.png");
    let img = match fondo::cargar(&ruta) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("morgan-lock: {}: {e}", ruta.display());
            return None;
        }
    };
    // La imagen grande ya se soltó: que glibc devuelva esa memoria.
    // SAFETY: sin argumentos que validar; solo recorta el heap.
    unsafe { libc::malloc_trim(0) };
    Some(dibujo::Fondo {
        imagen: ImageData {
            data: Blob::new(std::sync::Arc::new(img.pixeles)),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: img.ancho,
            height: img.alto,
        },
        achicada: img.achicada as f64,
    })
}

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

/// `[lock]` de la config de morgan. Cualquier problema (sin archivo, TOML roto, un valor
/// desconocido) deja los defaults y bloquea igual: la config no puede impedir el bloqueo.
fn estilo() -> Bloqueo {
    let Some(ruta) = config_dir().map(|d| d.join("morgan/config.toml")) else {
        return Bloqueo::default();
    };
    let Ok(texto) = std::fs::read_to_string(&ruta) else {
        return Bloqueo::default();
    };
    leer_estilo(&texto).unwrap_or_else(|e| {
        eprintln!(
            "morgan-lock: {}: [lock]: {e}; con los valores de fábrica",
            ruta.display()
        );
        Bloqueo::default()
    })
}

fn leer_estilo(texto: &str) -> Result<Bloqueo, String> {
    let tabla: toml::Table = texto.parse().map_err(|e: toml::de::Error| e.to_string())?;
    match tabla.get("lock") {
        Some(v) => v
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| e.to_string()),
        None => Ok(Bloqueo::default()),
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
        // El puntero solo se pide para esconderlo, como `morgan-lock`.
        if c == Capability::Pointer && self.puntero.is_none() {
            self.puntero = self.asientos.get_pointer(qh, &seat).ok();
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
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
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
            if let PointerEventKind::Enter { serial } = e.kind {
                puntero.set_cursor(serial, None, 0, 0);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use morgan_ipc::bloqueo::Escritura;

    #[test]
    fn el_estilo_sale_de_su_seccion_y_lo_roto_no_impide_leer() {
        let b = leer_estilo("[shell]\nopacity = 0.9\n[lock]\ntyping = \"dots\"\n").unwrap();
        assert_eq!(b.escritura, Escritura::Puntos);
        assert!(b.chispa);
        assert_eq!(leer_estilo("[shell]\n").unwrap(), Bloqueo::default());
        assert!(leer_estilo("[lock]\ntyping = \"nada\"\n").is_err());
        assert!(leer_estilo("no es toml [").is_err());
    }
}
