//! El socket de Umbriel (`docs/user/ipc.md`): un objeto JSON por línea, ida y vuelta, y la
//! respuesta `{"ok": …}` o `{"err": "…"}`.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `UMBRIEL_SOCKET`, o el nombre que Umbriel le da por su display. Si `UMBRIEL_SOCKET` apunta a
/// un socket que no está y el del display sí, vale el del display: un Umbriel de prueba deja su
/// ruta en el entorno de `systemctl --user` y la sesión real la hereda al reiniciar el servicio.
pub fn socket_from(
    env: impl Fn(&str) -> Option<OsString>,
    existe: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let del_display = || {
        let runtime = env("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())?;
        let display = env("WAYLAND_DISPLAY").filter(|v| !v.is_empty())?;
        let display = display.to_string_lossy();
        Some(PathBuf::from(runtime).join(format!("umbriel-{display}.sock")))
    };
    match env("UMBRIEL_SOCKET").filter(|v| !v.is_empty()) {
        Some(s) if existe(Path::new(&s)) => Some(PathBuf::from(s)),
        Some(s) => Some(
            del_display()
                .filter(|d| existe(d))
                .unwrap_or_else(|| PathBuf::from(s)),
        ),
        None => del_display(),
    }
}

pub fn socket() -> Option<PathBuf> {
    socket_from(|k| std::env::var_os(k), Path::exists)
}

/// La ruta que hay que poner en `UMBRIEL_SOCKET` porque la de ahora no existe y hay otra que sí.
pub fn correccion(
    env: impl Fn(&str) -> Option<OsString>,
    existe: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let puesta = PathBuf::from(env("UMBRIEL_SOCKET").filter(|v| !v.is_empty())?);
    let buena = socket_from(env, existe)?;
    (buena != puesta).then_some(buena)
}

/// Corrige `UMBRIEL_SOCKET` en el entorno del proceso, así lo heredan los `umbriel msg` y
/// `umbriel config` que se lancen. Se llama al arrancar, antes de abrir otros hilos.
pub fn corregir_entorno() {
    let Some(buena) = correccion(|k| std::env::var_os(k), Path::exists) else {
        return;
    };
    eprintln!(
        "morgan: UMBRIEL_SOCKET apuntaba a un socket que no existe; se usa {}",
        buena.display()
    );
    // SAFETY: lo llama `main` antes de lanzar hilos.
    unsafe { std::env::set_var("UMBRIEL_SOCKET", &buena) };
}

/// Manda un pedido y devuelve lo que vino en `ok`; `err` vuelve como error.
pub fn pedir_ya(ruta: &Path, pedido: &serde_json::Value) -> std::io::Result<serde_json::Value> {
    let mut s = UnixStream::connect(ruta)?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut linea = serde_json::to_vec(pedido)?;
    linea.push(b'\n');
    s.write_all(&linea)?;
    let mut respuesta = String::new();
    BufReader::new(s).read_line(&mut respuesta)?;
    respuesta_de(&respuesta)
}

pub fn respuesta_de(linea: &str) -> std::io::Result<serde_json::Value> {
    let mut v: serde_json::Value = serde_json::from_str(linea.trim())?;
    if let Some(err) = v.get("err") {
        return Err(std::io::Error::other(
            err.as_str().map_or_else(|| err.to_string(), str::to_owned),
        ));
    }
    match v.get_mut("ok") {
        Some(ok) => Ok(ok.take()),
        None => Err(std::io::Error::other(format!("respuesta sin ok: {linea}"))),
    }
}

/// `{"cmd":"action","action":…}` más los campos de la acción.
pub fn accion(nombre: &str, campos: serde_json::Value) -> serde_json::Value {
    let mut v = serde_json::json!({ "cmd": "action", "action": nombre });
    if let (Some(v), serde_json::Value::Object(c)) = (v.as_object_mut(), campos) {
        v.extend(c);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruta_del_socket() {
        let todo = |_: &Path| true;
        let env = |k: &str| match k {
            "XDG_RUNTIME_DIR" => Some("/run/user/1000".into()),
            "WAYLAND_DISPLAY" => Some("wayland-0".into()),
            _ => None,
        };
        assert_eq!(
            socket_from(env, todo),
            Some(PathBuf::from("/run/user/1000/umbriel-wayland-0.sock"))
        );
        let con = |k: &str| (k == "UMBRIEL_SOCKET").then(|| "/x.sock".into());
        assert_eq!(socket_from(con, todo), Some(PathBuf::from("/x.sock")));
        assert_eq!(socket_from(|_| None, todo), None);
    }

    /// El caso del 2026-11-22: la ruta de un Umbriel de prueba que ya no está.
    #[test]
    fn socket_viejo_en_el_entorno() {
        let env = |k: &str| match k {
            "UMBRIEL_SOCKET" => Some("/run/user/1000/mg-prueba/umbriel-wayland-0.sock".into()),
            "XDG_RUNTIME_DIR" => Some("/run/user/1000".into()),
            "WAYLAND_DISPLAY" => Some("wayland-0".into()),
            _ => None,
        };
        let real = PathBuf::from("/run/user/1000/umbriel-wayland-0.sock");
        let solo_real = |p: &Path| p == real;
        assert_eq!(socket_from(env, solo_real), Some(real.clone()));
        assert_eq!(correccion(env, solo_real), Some(real));
        // Si existe la puesta, manda esa aunque también esté la del display.
        assert_eq!(
            socket_from(env, |_| true),
            Some(PathBuf::from(
                "/run/user/1000/mg-prueba/umbriel-wayland-0.sock"
            ))
        );
        assert_eq!(correccion(env, |_| true), None);
        // Si no hay ninguna, queda la puesta: el error dice esa ruta.
        assert_eq!(
            socket_from(env, |_| false),
            Some(PathBuf::from(
                "/run/user/1000/mg-prueba/umbriel-wayland-0.sock"
            ))
        );
        assert_eq!(correccion(env, |_| false), None);
    }

    #[test]
    fn ok_y_err() {
        assert_eq!(
            respuesta_de(r#"{"ok":null}"#).unwrap(),
            serde_json::Value::Null
        );
        assert_eq!(
            respuesta_de(r#"{"ok":[1]}"#).unwrap(),
            serde_json::json!([1])
        );
        let e = respuesta_de(r#"{"err":"no focused window"}"#).unwrap_err();
        assert_eq!(e.to_string(), "no focused window");
        assert!(respuesta_de("basura").is_err());
    }

    #[test]
    fn la_accion_lleva_sus_campos() {
        assert_eq!(
            accion(
                "workspace-switch",
                serde_json::json!({"workspace": 2, "output": "DP-1"})
            ),
            serde_json::json!({"cmd":"action","action":"workspace-switch","workspace":2,"output":"DP-1"})
        );
        assert_eq!(
            accion("dpms-off", serde_json::json!({})),
            serde_json::json!({"cmd":"action","action":"dpms-off"})
        );
    }

    #[test]
    fn ida_y_vuelta() {
        let dir = std::env::temp_dir().join(format!("morgan-umbriel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("u.sock");
        let escucha = std::os::unix::net::UnixListener::bind(&ruta).unwrap();
        let umbriel = std::thread::spawn(move || {
            let (c, _) = escucha.accept().unwrap();
            let mut l = String::new();
            BufReader::new(&c).read_line(&mut l).unwrap();
            (&c).write_all(b"{\"ok\":[]}\n").unwrap();
            l
        });
        let r = pedir_ya(&ruta, &serde_json::json!({"cmd":"windows"})).unwrap();
        assert_eq!(r, serde_json::json!([]));
        assert_eq!(umbriel.join().unwrap(), "{\"cmd\":\"windows\"}\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
