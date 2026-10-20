//! El fundido cruzado con el escritorio. La ventana es transparente y su contenido se funde,
//! pero con la sesión bloqueada Hyprland no dibuja las ventanas de abajo: el fundido era desde
//! y hacia negro. `misc:session_lock_xray` las dibuja, y eso cuesta mientras dure (en el
//! anidado, con algo moviéndose debajo: 3 puntos de CPU y 7 de GPU de Hyprland). Con el
//! bloqueo puesto el fondo es opaco y no se ve nada de abajo, así que va prendido solo durante
//! los fundidos. Fuera de Hyprland no hace nada y el fundido queda desde negro.
//!
//! Nunca con la sesión ya bloqueada: un `morgan-lock` relanzado tras una caída mostraría el
//! escritorio mientras entra. Por eso `main` lo prende solo si `LockedHint` dice que no lo está.

use morgan_ipc::hyprland::{dir_from, pedir_ya};

/// Sincrónico: el valor tiene que estar puesto antes del primer cuadro del bloqueo. Seguro en
/// el hilo principal mientras no se espere a Hyprland entre el connect y el write (ver
/// `pedir_ya`); acá no hay nada en el medio.
pub fn poner(si: bool) {
    let Some(dir) = dir_from(|k| std::env::var_os(k)) else {
        return;
    };
    let lua = format!("eval hl.config({{ misc = {{ session_lock_xray = {si} }} }})");
    match pedir_ya(&dir.join(".socket.sock"), &lua) {
        Ok(r) if r.trim() == "ok" || r.trim().is_empty() => {}
        Ok(r) => eprintln!("morgan-lock: session_lock_xray: {}", r.trim()),
        Err(e) => eprintln!("morgan-lock: session_lock_xray: {e}"),
    }
}

/// En otro hilo, para cuando el orden con el bloqueo no importa.
pub fn poner_luego(si: bool) {
    std::thread::spawn(move || poner(si));
}
