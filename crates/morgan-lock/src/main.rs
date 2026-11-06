//! Binario aparte de morgan: si morgan se cae, el bloqueo sigue.
//!
//!   morgan-lock [--pam SERVICIO] [--sin-logind]
//!
//! `--pam vlock` y `--sin-logind` son para probarlo en un Hyprland anidado: un fallo no suma
//! a faillock y no se toca el `LockedHint` de la sesión real.
//!
//! Todo lo que se ve y se tipea está en la biblioteca (`pantalla.rs`); acá, PAM.

mod pam;

use std::process::ExitCode;

use morgan_lock::pantalla::{self, Ajustes};
use morgan_lock::perfil::Rutas;

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

fn main() -> ExitCode {
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
    pantalla::correr(Ajustes {
        verificador: Box::new(pam::Pam {
            servicio: o.servicio,
            usuario,
        }),
        logind: o.logind,
        xray: true,
        rutas: Rutas::del_home(),
        eleccion: None,
    })
}
