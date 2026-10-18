//! Autenticación por PAM con FFI propio: `pam_start`, `pam_authenticate`, `pam_end` y la
//! conversación. Solo la pila `auth` del servicio; nada de sesiones ni credenciales.
//!
//! `autenticar` bloquea (pam_unix demora ~2 s al fallar): se llama desde otro hilo.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{Ordering, compiler_fence};

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
/// Una cuenta sin contraseña no desbloquea con el campo vacío.
const PAM_DISALLOW_NULL_AUTHTOK: c_int = 0x0001;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

type Conversacion = unsafe extern "C" fn(
    c_int,
    *mut *const PamMessage,
    *mut *mut PamResponse,
    *mut c_void,
) -> c_int;

#[repr(C)]
struct PamConv {
    conv: Conversacion,
    appdata_ptr: *mut c_void,
}

#[repr(C)]
struct PamHandle {
    _opaco: [u8; 0],
}

#[repr(C)]
struct Passwd {
    pw_name: *mut c_char,
    pw_passwd: *mut c_char,
    pw_uid: u32,
    pw_gid: u32,
    pw_gecos: *mut c_char,
    pw_dir: *mut c_char,
    pw_shell: *mut c_char,
}

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conv: *const PamConv,
        pamh: *mut *mut PamHandle,
    ) -> c_int;
    fn pam_authenticate(pamh: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_end(pamh: *mut PamHandle, status: c_int) -> c_int;
    fn pam_strerror(pamh: *mut PamHandle, errnum: c_int) -> *const c_char;
}

// libc: PAM libera las respuestas con `free`, así que se piden con su `calloc`.
unsafe extern "C" {
    fn calloc(n: usize, tam: usize) -> *mut c_void;
    fn free(p: *mut c_void);
    fn strdup(s: *const c_char) -> *mut c_char;
    fn getuid() -> u32;
    fn getpwuid_r(
        uid: u32,
        pwd: *mut Passwd,
        buf: *mut c_char,
        buflen: usize,
        result: *mut *mut Passwd,
    ) -> c_int;
}

/// Bytes que se pisan con ceros al soltarse (la contraseña).
pub struct Secreto(Vec<u8>);

impl Secreto {
    pub fn new(texto: String) -> Self {
        Secreto(texto.into_bytes())
    }
}

impl Drop for Secreto {
    fn drop(&mut self) {
        borrar(&mut self.0);
    }
}

/// Pisa con ceros todo lo reservado, no solo lo usado: una escritura volátil por byte, que
/// el compilador no puede sacar aunque nadie lea después.
fn borrar(v: &mut Vec<u8>) {
    let cap = v.capacity();
    let p = v.as_mut_ptr();
    for i in 0..cap {
        // SAFETY: `i < capacity`, dentro de la reserva del Vec.
        unsafe { ptr::write_volatile(p.add(i), 0) };
    }
    compiler_fence(Ordering::SeqCst);
    v.clear();
}

#[derive(Debug, PartialEq)]
pub enum Resultado {
    Bien,
    Fallo(Vec<String>),
    Error(String),
}

/// El servicio existe en alguna de las dos carpetas que lee Linux-PAM. Sin archivo, PAM usa
/// `other`, que niega todo: bloquear así deja la sesión sin salida.
pub fn servicio_existe(servicio: &str) -> bool {
    !servicio.is_empty()
        && !servicio.contains('/')
        && ["/etc/pam.d", "/usr/lib/pam.d"]
            .iter()
            .any(|d| Path::new(d).join(servicio).is_file())
}

/// El usuario de este proceso, por su uid (no por `$USER`).
pub fn usuario() -> Option<String> {
    let mut pwd = std::mem::MaybeUninit::<Passwd>::zeroed();
    let mut buf = vec![0 as c_char; 4096];
    let mut res: *mut Passwd = ptr::null_mut();
    // SAFETY: buffers propios y vivos durante la llamada; `res` apunta a `pwd` o es nulo.
    let r = unsafe {
        getpwuid_r(
            getuid(),
            pwd.as_mut_ptr(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut res,
        )
    };
    if r != 0 || res.is_null() {
        return None;
    }
    // SAFETY: `res` no es nulo, así que `pwd` quedó escrita y `pw_name` apunta dentro de `buf`.
    let nombre = unsafe { CStr::from_ptr((*res).pw_name) };
    Some(nombre.to_string_lossy().into_owned())
}

struct Datos {
    /// Terminada en nul, para `strdup`.
    clave: Vec<u8>,
    mensajes: Vec<String>,
}

pub fn autenticar(servicio: &str, usuario: &str, clave: Secreto) -> Resultado {
    let (Ok(c_servicio), Ok(c_usuario)) = (CString::new(servicio), CString::new(usuario)) else {
        return Resultado::Error("servicio o usuario con nul".into());
    };
    if clave.0.contains(&0) {
        return Resultado::Fallo(Vec::new());
    }
    let mut datos = Datos {
        clave: {
            let mut v = Vec::with_capacity(clave.0.len() + 1);
            v.extend_from_slice(&clave.0);
            v.push(0);
            v
        },
        mensajes: Vec::new(),
    };
    drop(clave);
    let conv = PamConv {
        conv: conversar,
        appdata_ptr: (&raw mut datos).cast(),
    };
    let mut h: *mut PamHandle = ptr::null_mut();
    // SAFETY: cadenas terminadas en nul; `conv` y `datos` viven hasta después de `pam_end`.
    let r = unsafe { pam_start(c_servicio.as_ptr(), c_usuario.as_ptr(), &conv, &mut h) };
    if r != PAM_SUCCESS {
        borrar(&mut datos.clave);
        return Resultado::Error(format!("pam_start: {r}"));
    }
    // SAFETY: `h` salió de un `pam_start` exitoso.
    let r = unsafe { pam_authenticate(h, PAM_DISALLOW_NULL_AUTHTOK) };
    let texto = if r == PAM_SUCCESS {
        String::new()
    } else {
        // SAFETY: `h` sigue vivo; pam_strerror devuelve una cadena estática.
        unsafe { CStr::from_ptr(pam_strerror(h, r)) }
            .to_string_lossy()
            .into_owned()
    };
    // SAFETY: se cierra una sola vez, con el resultado de la última llamada.
    unsafe { pam_end(h, r) };
    borrar(&mut datos.clave);
    match r {
        PAM_SUCCESS => Resultado::Bien,
        // Módulo que no carga, error del sistema o de memoria, conversación, aborto: la pila
        // está rota, no la contraseña.
        1..=5 | PAM_CONV_ERR | 26 | 28 => Resultado::Error(format!("pam_authenticate: {texto}")),
        _ => Resultado::Fallo(datos.mensajes),
    }
}

/// La conversación de PAM: cada pedido de contraseña recibe una copia propia (PAM la pisa y
/// la libera); los avisos se juntan para mostrarlos.
unsafe extern "C" fn conversar(
    n: c_int,
    msg: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    appdata: *mut c_void,
) -> c_int {
    if n <= 0 || msg.is_null() || resp.is_null() || appdata.is_null() {
        return PAM_CONV_ERR;
    }
    let n = n as usize;
    // SAFETY: `appdata` es el `Datos` de `autenticar`, vivo mientras PAM corre.
    let datos = unsafe { &mut *appdata.cast::<Datos>() };
    // SAFETY: calloc de n respuestas; PAM las libera con free.
    let respuestas = unsafe { calloc(n, size_of::<PamResponse>()) }.cast::<PamResponse>();
    if respuestas.is_null() {
        return PAM_BUF_ERR;
    }
    for i in 0..n {
        // SAFETY: Linux-PAM pasa un arreglo de n punteros a mensaje.
        let m = unsafe { &**msg.add(i) };
        match m.msg_style {
            PAM_PROMPT_ECHO_OFF | PAM_PROMPT_ECHO_ON => {
                // SAFETY: `clave` termina en nul.
                let copia = unsafe { strdup(datos.clave.as_ptr().cast()) };
                if copia.is_null() {
                    // SAFETY: lo reservado hasta acá se suelta; PAM no ve `resp`.
                    unsafe { liberar(respuestas, i) };
                    return PAM_BUF_ERR;
                }
                // SAFETY: `i < n`, dentro de lo reservado.
                unsafe { (*respuestas.add(i)).resp = copia };
            }
            PAM_ERROR_MSG | PAM_TEXT_INFO => {
                if !m.msg.is_null() {
                    // SAFETY: mensaje de PAM, terminado en nul.
                    let t = unsafe { CStr::from_ptr(m.msg) }.to_string_lossy();
                    let t = t.trim();
                    if !t.is_empty() {
                        datos.mensajes.push(t.to_owned());
                    }
                }
            }
            _ => {
                // SAFETY: ídem.
                unsafe { liberar(respuestas, i) };
                return PAM_CONV_ERR;
            }
        }
    }
    // SAFETY: `resp` no es nulo.
    unsafe { *resp = respuestas };
    PAM_SUCCESS
}

/// Suelta las primeras `hasta` respuestas (pisando las copias de la clave) y el arreglo.
unsafe fn liberar(respuestas: *mut PamResponse, hasta: usize) {
    for i in 0..hasta {
        // SAFETY: lo llama `conversar` con respuestas propias, `i` dentro de lo reservado.
        let r = unsafe { (*respuestas.add(i)).resp };
        if !r.is_null() {
            // SAFETY: cadena de `strdup`, terminada en nul.
            let largo = unsafe { CStr::from_ptr(r) }.to_bytes().len();
            for j in 0..largo {
                // SAFETY: dentro de la cadena.
                unsafe { ptr::write_volatile(r.add(j), 0) };
            }
            // SAFETY: de `strdup`.
            unsafe { free(r.cast()) };
        }
    }
    // SAFETY: de `calloc`.
    unsafe { free(respuestas.cast()) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servicio_inexistente_no_sirve() {
        assert!(!servicio_existe("morgan-lock-que-no-existe"));
        assert!(!servicio_existe(""));
        assert!(!servicio_existe("../pam.d/login"));
    }

    #[test]
    fn servicio_del_sistema_sirve() {
        // `login` lo trae util-linux.
        assert!(servicio_existe("login"));
    }

    #[test]
    fn el_usuario_sale_del_uid() {
        let u = usuario().expect("usuario");
        assert!(!u.is_empty());
    }

    #[test]
    fn borrar_pisa_toda_la_reserva() {
        let mut v = Vec::with_capacity(32);
        v.extend_from_slice(b"hunter2");
        let p = v.as_ptr();
        let cap = v.capacity();
        borrar(&mut v);
        assert!(v.is_empty());
        // SAFETY: la reserva sigue siendo de `v`, que no se soltó.
        let crudo = unsafe { std::slice::from_raw_parts(p, cap) };
        assert!(crudo.iter().all(|&b| b == 0));
    }

    #[test]
    fn servicio_roto_es_error_y_no_bien() {
        // Un servicio sin archivo cae en `other` (niega todo): nunca puede dar Bien.
        let r = autenticar(
            "morgan-lock-que-no-existe",
            &usuario().unwrap(),
            Secreto::new("x".into()),
        );
        assert_ne!(r, Resultado::Bien);
    }
}
