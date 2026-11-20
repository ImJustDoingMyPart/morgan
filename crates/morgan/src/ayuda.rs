//! `morgan --help` y `morgan --version`, y el aviso ante un comando que no existe.

use std::process::ExitCode;

use morgan_idioma::{tr, trf};

fn texto() -> &'static str {
    tr!(
        "uso: morgan [comando]\n\nSin comando arranca el shell; de eso se encarga el servicio morgan.service.\n\n  setup             lo prepara para este usuario: lo suma a Hyprland, habilita el servicio y lo arranca\n  setup --seco      muestra lo que haría, sin hacer nada\n  setup --deshacer  devuelve todo como estaba (antes de quitar el paquete)\n  about             la versión, y quién fue Morgan\n  --version         la versión\n  --help            esta ayuda\n\nPara manejarlo mientras corre (volumen, fondos, el centro…): morganctl."
    )
}

pub fn ayuda() -> ExitCode {
    println!("{}", texto());
    ExitCode::SUCCESS
}

pub fn version() -> ExitCode {
    match env!("MORGAN_COMMIT") {
        "" => println!("morgan {}", env!("CARGO_PKG_VERSION")),
        c => println!("morgan {} ({c})", env!("CARGO_PKG_VERSION")),
    }
    ExitCode::SUCCESS
}

pub fn desconocido(arg: &str) -> ExitCode {
    eprintln!("{}\n", trf!("morgan: no existe el comando «{}».", arg));
    eprintln!("{}", texto());
    ExitCode::FAILURE
}
