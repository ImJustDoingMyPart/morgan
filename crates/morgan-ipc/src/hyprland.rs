//! El socket de pedidos de Hyprland (`.socket.sock`), sin GTK: lo usan morgan y morgan-lock.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn dir_from(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let runtime = env("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())?;
    let firma = env("HYPRLAND_INSTANCE_SIGNATURE").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(runtime).join("hypr").join(firma))
}

/// Conecta, escribe y lee de un tirón. Hyprland, después de aceptar, traba su loop hasta 5 s
/// esperando el pedido: entre el connect y el write no puede haber nada que espere a
/// Hyprland (en morgan va en un hilo; ver `pedir` ahí).
pub fn pedir_ya(ruta: &Path, comando: &str) -> std::io::Result<String> {
    let mut s = UnixStream::connect(ruta)?;
    s.write_all(comando.as_bytes())?;
    // Tope por si no contesta: el hilo no queda colgado para siempre.
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut respuesta = Vec::new();
    s.read_to_end(&mut respuesta)?;
    Ok(String::from_utf8_lossy(&respuesta).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_pedido_llega_entero_sin_el_main_loop() {
        let dir = std::env::temp_dir().join(format!("morgan-pedir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join(".socket.sock");
        let escucha = std::os::unix::net::UnixListener::bind(&ruta).unwrap();
        // Como Hyprland: lee el pedido apenas acepta, contesta y cierra.
        let hypr = std::thread::spawn(move || {
            let (mut c, _) = escucha.accept().unwrap();
            let mut b = [0u8; 64];
            let n = c.read(&mut b).unwrap();
            c.write_all(b"ok").unwrap();
            String::from_utf8_lossy(&b[..n]).into_owned()
        });
        assert_eq!(pedir_ya(&ruta, "j/monitors").unwrap(), "ok");
        assert_eq!(hypr.join().unwrap(), "j/monitors");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dir_de_la_instancia() {
        let env = |k: &str| match k {
            "XDG_RUNTIME_DIR" => Some("/run/user/1000".into()),
            "HYPRLAND_INSTANCE_SIGNATURE" => Some("abc_1_2".into()),
            _ => None,
        };
        assert_eq!(
            dir_from(env),
            Some(PathBuf::from("/run/user/1000/hypr/abc_1_2"))
        );
        assert_eq!(dir_from(|_| None), None);
        assert_eq!(
            dir_from(|k| (k == "XDG_RUNTIME_DIR").then(|| "/run".into())),
            None
        );
    }
}
