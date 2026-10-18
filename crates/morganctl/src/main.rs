//! Cliente de línea de comandos de morgan: lo usan los binds de Hyprland y los scripts.
//!
//! Salida: 0 si morgan cumplió, 1 si morgan contestó con error, 2 si los argumentos están mal,
//! 3 si no se pudo hablar con morgan.

use std::process::ExitCode;

use morgan_ipc::{Request, Response};

const USO: &str = "\
usage: morganctl ping
       morganctl osd message <title> [--state <text>] [--on] [--icon <name>] [--ms <200-60000>]
       morganctl osd progress <0-1> [--title <text>] [--icon <name>] [--ms <200-60000>]
       morganctl volume up|down|mute [--mic] [--device <name>]
       morganctl player play-pause|next|previous
       morganctl dnd toggle|on|off
       morganctl brightness up|down|<0-100>
       morganctl caffeine toggle|on|off|status
       morganctl nightlight toggle|on|off|status
       morganctl wallpaper <absolute path>|random|current
       morganctl capslock
       morganctl notifications clear
       morganctl screenshot <absolute path>
       morganctl center toggle|open|close
       morganctl clock toggle|open|close
       morganctl calculator toggle|open|close
       morganctl wallpapers toggle|open|close
       morganctl session
       morganctl windows [release]
       morganctl menu toggle|open|close [root|wallpapers|weather]
       morganctl launcher toggle|open|close [apps|symbols|clipboard|keybinds|city|calendar]
       morganctl theme list|current|reload
       morganctl theme apply <theme>
       morganctl theme wallpaper [image]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "theme") {
        return tema(&args[1..]);
    }
    let request = match Request::from_args(&args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("morganctl: {e}\n{USO}");
            return ExitCode::from(2);
        }
    };
    let path = match morgan_ipc::socket_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("morganctl: {e}");
            return ExitCode::from(3);
        }
    };
    match morgan_ipc::send(&path, &request) {
        Ok(Response::Pong { version }) => {
            println!("morgan {version}");
            ExitCode::SUCCESS
        }
        Ok(Response::Done) => ExitCode::SUCCESS,
        Ok(Response::Texto { texto }) => {
            print!("{texto}");
            ExitCode::SUCCESS
        }
        Ok(Response::Error { message }) => {
            eprintln!("morgan: {message}");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!(
                "morganctl: could not reach morgan at {}: {e}",
                path.display()
            );
            ExitCode::from(3)
        }
    }
}

/// El tema corre acá y no en morgan: los scripts (el del wallpaper) lo usan aunque morgan no
/// esté levantado, y lo lento (matugen) no traba su main loop.
fn tema(args: &[String]) -> ExitCode {
    let r = morgan_temas::Rutas::del_entorno();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let res = match args.as_slice() {
        ["list"] => {
            let activo = morgan_temas::activo(&r);
            for t in morgan_temas::catalogo(&r) {
                println!("{} {t}", if t == activo { '*' } else { ' ' });
            }
            return ExitCode::SUCCESS;
        }
        ["current"] => {
            println!("{}", morgan_temas::activo(&r));
            return ExitCode::SUCCESS;
        }
        ["apply", t] => morgan_temas::aplicar(&r, t, None).map(Some),
        ["reload"] => morgan_temas::recargar(&r).map(Some),
        ["wallpaper"] => morgan_temas::wallpaper(&r, None),
        ["wallpaper", img] => morgan_temas::wallpaper(&r, Some(std::path::Path::new(img))),
        _ => {
            eprintln!("morganctl: theme: invalid arguments\n{USO}");
            return ExitCode::from(2);
        }
    };
    match res {
        Ok(inf) => {
            for a in inf.map(|i| i.avisos).unwrap_or_default() {
                eprintln!("morganctl: theme: {a}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("morganctl: theme: {e}");
            ExitCode::from(1)
        }
    }
}
