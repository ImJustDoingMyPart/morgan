//! El greeter de morgan: la pantalla del bloqueo, para entrar.
//!
//!   morgan-greet                     lo que corre greetd: arma la config de Hyprland y lo
//!                                    lanza; Hyprland lanza la pantalla y sale cuando termina
//!   morgan-greet pantalla            la pantalla, contra greetd (`GREETD_SOCK`)
//!   morgan-greet pantalla --demo     la pantalla sin greetd, para probarla en un Hyprland
//!                                    anidado: cualquier clave entra y «mal» falla
//!
//! La pantalla es la de `morgan-lock` (`pantalla.rs`), con greetd en vez de PAM, sin logind
//! y con los archivos de la cuenta copiados en `/var/lib/morgan-greet` (`greeter.rs`).

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use morgan_lock::clave::Secreto;
use morgan_lock::greetd::Greetd;
use morgan_lock::greeter::{self, BASE, Recuerdo, Sesion, Usuario};
use morgan_lock::pantalla::{self, Ajustes};
use morgan_lock::perfil::Rutas;
use morgan_lock::verificar::{Resultado, Verificador};

const USO: &str = "uso: morgan-greet [pantalla [--demo]]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] => lanzar(),
        ["pantalla"] => pantalla(false),
        ["pantalla", "--demo"] => pantalla(true),
        _ => {
            eprintln!("morgan-greet: {USO}");
            ExitCode::FAILURE
        }
    }
}

/// Escribe la config y reemplaza este proceso por Hyprland (con su watchdog,
/// `start-hyprland`). Lo que Hyprland lanza hereda `GREETD_SOCK`.
fn lanzar() -> ExitCode {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("morgan-greet: no se sabe dónde está este binario: {e}");
            return ExitCode::FAILURE;
        }
    };
    let teclado = greeter::teclado(
        &std::fs::read_to_string("/etc/X11/xorg.conf.d/00-keyboard.conf").unwrap_or_default(),
    );
    let config = greeter::config_hyprland(
        &[exe.to_string_lossy().into_owned(), "pantalla".to_owned()],
        &teclado,
    );
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|d| d.is_dir())
        .unwrap_or_else(std::env::temp_dir)
        .join("morgan-greet");
    let ruta = dir.join("hyprland.lua");
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&ruta, config)) {
        eprintln!("morgan-greet: {}: {e}", ruta.display());
        return ExitCode::FAILURE;
    }
    let e = Command::new("start-hyprland")
        .arg("--")
        .arg("-c")
        .arg(&ruta)
        .exec();
    eprintln!("morgan-greet: start-hyprland: {e}");
    ExitCode::FAILURE
}

fn pantalla(demo: bool) -> ExitCode {
    if demo {
        return pantalla::correr(Ajustes {
            verificador: Box::new(Demo),
            logind: false,
            xray: false,
            rutas: Rutas::del_home(),
        });
    }
    let usuarios = greeter::usuarios(&std::fs::read_to_string("/etc/passwd").unwrap_or_default());
    let sesiones = greeter::sesiones(&[PathBuf::from("/usr/share/wayland-sessions")]);
    let recuerdo = Recuerdo::leer();
    let Some(usuario) = recuerdo
        .usuario
        .as_ref()
        .and_then(|n| usuarios.iter().find(|u| &u.nombre == n))
        .or(usuarios.first())
        .cloned()
    else {
        eprintln!("morgan-greet: no hay cuentas de personas en /etc/passwd");
        return ExitCode::FAILURE;
    };
    let Some(sesion) = recuerdo
        .sesion
        .as_ref()
        .and_then(|id| sesiones.iter().find(|s| &s.id == id))
        .or_else(|| sesiones.get(greeter::por_defecto(&sesiones)))
        .cloned()
    else {
        eprintln!("morgan-greet: no hay sesiones en /usr/share/wayland-sessions");
        return ExitCode::FAILURE;
    };
    let greetd = match Greetd::conectar(&usuario.nombre) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("morgan-greet: {e}");
            return ExitCode::FAILURE;
        }
    };
    let rutas = greeter::rutas_de(&usuario, Path::new(BASE));
    pantalla::correr(Ajustes {
        verificador: Box::new(Entrar {
            greetd,
            usuario,
            sesion,
        }),
        logind: false,
        xray: false,
        rutas: Some(rutas),
    })
}

/// greetd, y al acertar, la sesión elegida.
struct Entrar {
    greetd: Greetd,
    usuario: Usuario,
    sesion: Sesion,
}

impl Verificador for Entrar {
    fn verificar(&mut self, clave: Secreto) -> Resultado {
        self.greetd.verificar(clave)
    }

    fn al_entrar(&mut self) -> Result<(), String> {
        self.greetd
            .iniciar(&self.sesion.cmd, &self.sesion.entorno())
            .map_err(|e| e.to_string())?;
        let r = Recuerdo {
            usuario: Some(self.usuario.nombre.clone()),
            sesion: Some(self.sesion.id.clone()),
        };
        if let Err(e) = r.guardar() {
            eprintln!("morgan-greet: {BASE}/estado.toml: {e}");
        }
        Ok(())
    }
}

/// Para probar sin greetd: tarda como PAM y «mal» falla.
struct Demo;

impl Verificador for Demo {
    fn verificar(&mut self, clave: Secreto) -> Resultado {
        let mal = clave.bytes() == b"mal";
        std::thread::sleep(Duration::from_millis(if mal { 1500 } else { 600 }));
        if mal {
            Resultado::Fallo(Vec::new())
        } else {
            Resultado::Bien
        }
    }
}
