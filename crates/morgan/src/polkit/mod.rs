//! El agente de autenticación de polkit, el que pide la clave cuando un programa necesita
//! permisos (reemplaza a hyprpolkitagent). Se registra en polkitd para la sesión gráfica del
//! usuario; cada `BeginAuthentication` se muestra en el hilo del centro
//! (`centro/dibujo/permiso.rs`) y PAM lo corre el ayudante de polkit (`ayudante.rs`). Con
//! otro agente ya registrado en la sesión, polkitd rechaza el registro y morgan sigue sin el
//! suyo (lo dice el journal).
//!
//! Los pedidos van de a uno: los que llegan con uno abierto esperan en fila. Un fallo deja el
//! pedido abierto para otro intento, como hacen los agentes de GNOME y KDE.

pub mod ayudante;
pub mod secreto;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::sync::mpsc;

use smithay_client_toolkit::reexports::calloop::channel::Sender;
use zbus::Proxy;
use zbus::zvariant::{OwnedValue, Value};

use crate::centro::Centro;
use crate::centro::hilo::Orden;
use crate::centro::vista::{EventoPermiso, VistaPermiso};
use ayudante::DelAyudante;
use secreto::Secreto;

const RUTA: &str = "/org/morgan/polkit";

#[derive(Clone)]
pub struct Polkit {
    inner: Rc<Inner>,
}

struct Inner {
    hilo: Sender<Orden>,
    cola: RefCell<VecDeque<Pedido>>,
    actual: RefCell<Option<Actual>>,
    generacion: Cell<u64>,
    /// Cada intento lleva su número: lo que llega de uno que ya no corre no cuenta.
    intentos: Cell<u64>,
    del_ayudante: async_channel::Sender<(u64, DelAyudante)>,
}

struct Pedido {
    cookie: String,
    usuario: String,
    mensaje: String,
    detalle: Option<String>,
    /// `true` si autenticó; cualquier otra cosa es cancelar.
    respuesta: async_channel::Sender<bool>,
}

struct Actual {
    pedido: Pedido,
    vista: VistaPermiso,
    intento: Option<Intento>,
}

struct Intento {
    numero: u64,
    socket: UnixStream,
    respuestas: mpsc::Sender<Option<Secreto>>,
    /// El ayudante pidió algo más y espera la respuesta.
    esperando: bool,
}

impl Drop for Intento {
    fn drop(&mut self) {
        let _ = self.respuestas.send(None);
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

enum DelAgente {
    Empezar(Pedido),
    Cancelar(String),
}

impl Polkit {
    pub fn new(centro: &Centro) -> Self {
        let (atx, arx) = async_channel::unbounded::<(u64, DelAyudante)>();
        let p = Polkit {
            inner: Rc::new(Inner {
                hilo: centro.hilo(),
                cola: RefCell::default(),
                actual: RefCell::default(),
                generacion: Cell::new(0),
                intentos: Cell::new(0),
                del_ayudante: atx,
            }),
        };
        let weak = Rc::downgrade(&p.inner);
        centro.al_permiso(move |e| {
            if let Some(inner) = weak.upgrade() {
                Polkit { inner }.evento(e);
            }
        });
        let weak = Rc::downgrade(&p.inner);
        glib::MainContext::default().spawn_local(async move {
            while let Ok((n, d)) = arx.recv().await {
                let Some(inner) = weak.upgrade() else { return };
                Polkit { inner }.del_ayudante(n, d);
            }
        });
        let (gtx, grx) = async_channel::unbounded();
        let weak = Rc::downgrade(&p.inner);
        glib::MainContext::default().spawn_local(async move {
            match registrar(gtx).await {
                Ok(id) => eprintln!("morgan: polkit: agente registrado en la sesión {id}"),
                Err(e) => {
                    eprintln!("morgan: polkit: no se registró el agente: {e}");
                    return;
                }
            }
            while let Ok(d) = grx.recv().await {
                let Some(inner) = weak.upgrade() else { return };
                let p = Polkit { inner };
                match d {
                    DelAgente::Empezar(pedido) => p.empezar(pedido),
                    DelAgente::Cancelar(cookie) => p.cancelar(&cookie),
                }
            }
        });
        p
    }

    fn empezar(&self, pedido: Pedido) {
        self.inner.cola.borrow_mut().push_back(pedido);
        if self.inner.actual.borrow().is_none() {
            self.siguiente();
        }
    }

    fn siguiente(&self) {
        let i = &self.inner;
        let Some(pedido) = i.cola.borrow_mut().pop_front() else {
            return;
        };
        let vista = VistaPermiso {
            usuario: pedido.usuario.clone(),
            mensaje: pedido.mensaje.clone(),
            detalle: pedido.detalle.clone(),
            placeholder: tr!("Contraseña").to_owned(),
            generacion: self.generacion(),
            ..VistaPermiso::default()
        };
        let _ = i.hilo.send(Orden::PermisoAbrir(vista.clone()));
        i.actual.replace(Some(Actual {
            pedido,
            vista,
            intento: None,
        }));
    }

    fn generacion(&self) -> u64 {
        let g = self.inner.generacion.get() + 1;
        self.inner.generacion.set(g);
        g
    }

    /// Lo que cambió del pedido, a la vista.
    fn mostrar(&self, a: &Actual) {
        let _ = self.inner.hilo.send(Orden::Permiso(a.vista.clone()));
    }

    fn evento(&self, e: EventoPermiso) {
        match e {
            EventoPermiso::Enviar(clave) => self.enviar(clave),
            EventoPermiso::Cancelado => self.terminar(false),
        }
    }

    fn enviar(&self, clave: Secreto) {
        let mut actual = self.inner.actual.borrow_mut();
        let Some(a) = actual.as_mut() else { return };
        if a.vista.ocupado {
            return;
        }
        match &mut a.intento {
            Some(t) if t.esperando => {
                t.esperando = false;
                let _ = t.respuestas.send(Some(clave));
            }
            Some(_) => return,
            None => {
                let numero = self.inner.intentos.get() + 1;
                self.inner.intentos.set(numero);
                let (rtx, rrx) = mpsc::channel();
                let tx = self.inner.del_ayudante.clone();
                match ayudante::intentar(
                    a.pedido.usuario.clone(),
                    a.pedido.cookie.clone(),
                    clave,
                    rrx,
                    move |d| {
                        let _ = tx.send_blocking((numero, d));
                    },
                ) {
                    Ok(socket) => {
                        a.intento = Some(Intento {
                            numero,
                            socket,
                            respuestas: rtx,
                            esperando: false,
                        });
                        a.vista.aviso = None;
                    }
                    Err(e) => {
                        eprintln!("morgan: polkit: {}: {e}", ayudante::SOCKET);
                        a.vista.aviso =
                            Some((tr!("No se pudo hablar con polkit").to_owned(), true));
                        a.vista.generacion = self.generacion();
                        self.mostrar(a);
                        return;
                    }
                }
            }
        }
        a.vista.ocupado = true;
        self.mostrar(a);
    }

    fn del_ayudante(&self, numero: u64, d: DelAyudante) {
        let vigente = self
            .inner
            .actual
            .borrow()
            .as_ref()
            .is_some_and(|a| a.intento.as_ref().is_some_and(|t| t.numero == numero));
        if !vigente {
            return;
        }
        if matches!(d, DelAyudante::Fin(true)) {
            return self.terminar(true);
        }
        let mut actual = self.inner.actual.borrow_mut();
        let Some(a) = actual.as_mut() else { return };
        match d {
            DelAyudante::Pedir { texto, eco } => {
                if let Some(t) = &mut a.intento {
                    t.esperando = true;
                }
                a.vista.ocupado = false;
                a.vista.eco = eco;
                a.vista.placeholder = sin_dos_puntos(&texto);
                a.vista.generacion = self.generacion();
            }
            DelAyudante::Aviso { texto, error } => {
                a.vista.aviso = Some((texto.trim().to_owned(), error));
            }
            DelAyudante::Fin(true) => return,
            DelAyudante::Fin(false) => {
                a.intento = None;
                a.vista.ocupado = false;
                a.vista.eco = false;
                a.vista.fallos += 1;
                a.vista.placeholder = tr!("Contraseña").to_owned();
                a.vista.generacion = self.generacion();
                // Un error de PAM (la cuenta bloqueada por faillock) dice más que el genérico.
                if !a.vista.aviso.as_ref().is_some_and(|(_, error)| *error) {
                    a.vista.aviso = Some((tr!("Contraseña incorrecta").to_owned(), true));
                }
            }
        }
        self.mostrar(a);
    }

    /// polkitd dejó de esperar (el programa que pedía se cerró).
    fn cancelar(&self, cookie: &str) {
        let es_el_actual = self
            .inner
            .actual
            .borrow()
            .as_ref()
            .is_some_and(|a| a.pedido.cookie == cookie);
        if es_el_actual {
            return self.terminar(false);
        }
        self.inner.cola.borrow_mut().retain(|p| {
            let sigue = p.cookie != cookie;
            if !sigue {
                let _ = p.respuesta.try_send(false);
            }
            sigue
        });
    }

    fn terminar(&self, ok: bool) {
        let Some(a) = self.inner.actual.take() else {
            return;
        };
        let _ = a.pedido.respuesta.try_send(ok);
        drop(a);
        let _ = self.inner.hilo.send(Orden::PermisoCerrar);
        self.siguiente();
    }
}

/// «Contraseña: » → «Contraseña», para el campo vacío.
fn sin_dos_puntos(s: &str) -> String {
    s.trim().trim_end_matches(':').trim_end().to_owned()
}

async fn registrar(pedidos: async_channel::Sender<DelAgente>) -> zbus::Result<String> {
    let conn = crate::sistema::bus().await?;
    conn.object_server().at(RUTA, Agente { pedidos }).await?;
    // morgan corre en una unidad de usuario: `session/auto` es la sesión gráfica.
    let id: String = Proxy::new(
        &conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1/session/auto",
        "org.freedesktop.login1.Session",
    )
    .await?
    .get_property("Id") // no-tr
    .await?;
    let sujeto = (
        "unix-session",
        HashMap::from([("session-id", Value::from(id.clone()))]),
    );
    Proxy::new(
        &conn,
        "org.freedesktop.PolicyKit1",
        "/org/freedesktop/PolicyKit1/Authority",
        "org.freedesktop.PolicyKit1.Authority",
    )
    .await?
    .call::<_, _, ()>("RegisterAuthenticationAgent", &(sujeto, locale(), RUTA))
    .await?;
    Ok(id)
}

/// El idioma de los mensajes que traduce polkitd: el de morgan.
fn locale() -> String {
    let env = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
    match morgan_idioma::actual() {
        morgan_idioma::Idioma::Es => env
            .filter(|v| v.starts_with("es"))
            .unwrap_or_else(|| "es_ES.UTF-8".to_owned()), // no-tr
        morgan_idioma::Idioma::En => "en_US.UTF-8".to_owned(), // no-tr
    }
}

/// Entre los que pueden autenticar (polkitd ya pasó los grupos de admin a usuarios): el
/// usuario de la sesión si está, si no root, si no el primero.
fn elegir(identidades: &[(String, HashMap<String, OwnedValue>)], propio: u32) -> Option<u32> {
    let uids: Vec<u32> = identidades
        .iter()
        .filter(|(tipo, _)| tipo == "unix-user")
        .filter_map(|(_, d)| d.get("uid").and_then(|v| u32::try_from(v).ok()))
        .collect();
    [propio, 0]
        .into_iter()
        .find(|u| uids.contains(u))
        .or_else(|| uids.first().copied())
}

fn nombre_de(uid: u32) -> Option<String> {
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0 as libc::c_char; 4096];
    let mut res: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: `pw` y `buf` viven durante la llamada; el nombre se copia antes de soltarlos.
    let rc = unsafe { libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut res) };
    if rc != 0 || res.is_null() {
        return None;
    }
    // SAFETY: con éxito, `pw_name` apunta a un texto terminado en cero dentro de `buf`.
    let nombre = unsafe { std::ffi::CStr::from_ptr(pw.pw_name) };
    Some(nombre.to_string_lossy().into_owned())
}

/// Qué se va a hacer: el comando de pkexec, o el programa que lo pide.
fn detalle(d: &HashMap<String, String>) -> Option<String> {
    if let Some(c) = d.get("command_line").or(d.get("program")) {
        return Some(c.clone());
    }
    let pid = d.get("polkit.subject-pid")?;
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?; // no-tr
    Some(comm.trim().to_owned())
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.PolicyKit1.Error")]
enum ErrorPolkit {
    #[zbus(error)]
    ZBus(zbus::Error),
    Failed(String),
    Cancelled(String),
}

struct Agente {
    pedidos: async_channel::Sender<DelAgente>,
}

#[zbus::interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl Agente {
    #[allow(clippy::too_many_arguments)]
    async fn begin_authentication(
        &self,
        _action_id: String,
        message: String,
        _icon_name: String,
        details: HashMap<String, String>,
        cookie: String,
        identities: Vec<(String, HashMap<String, OwnedValue>)>,
    ) -> Result<(), ErrorPolkit> {
        let propio = unsafe { libc::getuid() };
        let usuario = elegir(&identities, propio)
            .and_then(nombre_de)
            .ok_or_else(|| ErrorPolkit::Failed("sin un usuario que autentique".into()))?; // no-tr
        let (tx, rx) = async_channel::bounded(1);
        let pedido = Pedido {
            cookie,
            usuario,
            mensaje: message,
            detalle: detalle(&details),
            respuesta: tx,
        };
        self.pedidos
            .send(DelAgente::Empezar(pedido))
            .await
            .map_err(|_| ErrorPolkit::Failed("morgan no atiende".into()))?; // no-tr
        match rx.recv().await {
            Ok(true) => Ok(()),
            _ => Err(ErrorPolkit::Cancelled("cancelado".into())), // no-tr
        }
    }

    async fn cancel_authentication(&self, cookie: String) {
        let _ = self.pedidos.send(DelAgente::Cancelar(cookie)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usuario(uid: u32) -> (String, HashMap<String, OwnedValue>) {
        let v = OwnedValue::from(uid);
        ("unix-user".into(), HashMap::from([("uid".into(), v)]))
    }

    #[test]
    fn elige_el_usuario_propio_y_si_no_root() {
        let ids = [usuario(1001), usuario(0), usuario(1000)];
        assert_eq!(elegir(&ids, 1000), Some(1000));
        assert_eq!(elegir(&ids, 2000), Some(0));
        assert_eq!(elegir(&[usuario(1001)], 1000), Some(1001));
        let grupo = ("unix-group".to_owned(), HashMap::new());
        assert_eq!(elegir(&[grupo], 1000), None);
    }

    #[test]
    fn detalle_del_comando_o_del_programa() {
        let d = HashMap::from([
            ("program".to_owned(), "/usr/bin/btrfs-assistant".to_owned()),
            (
                "command_line".to_owned(),
                "/usr/bin/btrfs-assistant -x".to_owned(),
            ),
        ]);
        assert_eq!(detalle(&d).as_deref(), Some("/usr/bin/btrfs-assistant -x"));
        assert_eq!(detalle(&HashMap::new()), None);
    }

    #[test]
    fn el_campo_vacio_sin_los_dos_puntos() {
        assert_eq!(sin_dos_puntos("Contraseña: "), "Contraseña");
        assert_eq!(
            sin_dos_puntos("Código de verificación:"),
            "Código de verificación"
        );
    }
}
