//! El teclado del greeter sin click. Con la sesión bloqueada, Hyprland 0.57.1 le da el
//! teclado a la superficie de bloqueo de su monitor enfocado (`InputManager.cpp`,
//! `mouseMoveUnified`), no a la del puntero. En el Hyprland del greeter, recién arrancado,
//! puede no haber monitor enfocado, o ser uno apagado (la TV): el teclado no llegaba a ninguna
//! superficie hasta el primer click (Roberto, 2026-11-07).
//!
//! Si al rato de bloquear el teclado no entró, se enfoca la salida que muestra el contenido y
//! se lleva el puntero a su centro: `cursor.move` vuelve a calcular el foco
//! (`simulateMouseMovement`), ya con ese monitor. Solo en el greeter: en la sesión, el bloqueo
//! siempre encuentra un monitor enfocado.

use morgan_ipc::compositor::{self, Tipo};
use morgan_ipc::hyprland::{dir_from, pedir_ya};

/// `salida`: el nombre de la salida (`DP-3`); `centro`: su centro en coordenadas lógicas.
pub fn pedir(salida: &str, centro: (i32, i32)) {
    if compositor::actual() != Tipo::Hyprland {
        return;
    }
    let Some(dir) = dir_from(|k| std::env::var_os(k)) else {
        return;
    };
    let socket = dir.join(".socket.sock");
    let nombre: String = salida
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c))
        .collect();
    for lua in [
        format!("dispatch hl.dsp.focus({{ monitor = \"{nombre}\" }})"),
        format!(
            "dispatch hl.dsp.cursor.move({{ x = {}, y = {} }})",
            centro.0, centro.1
        ),
    ] {
        match pedir_ya(&socket, &lua) {
            Ok(r) if r.trim() == "ok" || r.trim().is_empty() => {}
            Ok(r) => eprintln!("morgan-greet: foco: {}", r.trim()),
            Err(e) => eprintln!("morgan-greet: foco: {e}"),
        }
    }
}
