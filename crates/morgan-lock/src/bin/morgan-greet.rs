//! El greeter de morgan: la pantalla del bloqueo, para entrar.
//!
//!   morgan-greet [--compositor C]    lo que corre greetd: arma la config del compositor
//!                                    (hyprland o umbriel; sin nada, el primero instalado) y lo
//!                                    lanza; el compositor lanza la pantalla y sale cuando termina
//!   morgan-greet pantalla            la pantalla, contra greetd (`GREETD_SOCK`)
//!   morgan-greet pantalla --demo     la pantalla sin greetd, para probarla en un Hyprland
//!                                    anidado: cualquier clave entra y «mal» falla
//!   morgan-greet install [--apply]   lo pone en greetd (como root; ver `instalar.rs`)
//!   morgan-greet restore [--apply]   vuelve a la config de greetd de antes
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

const USO: &str = "uso: morgan-greet [--compositor hyprland|umbriel] [pantalla [--demo] | install [--apply] | restore [--apply]] [--raiz DIR]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut args: Vec<&str> = args.iter().map(String::as_str).collect();
    // `--raiz` es para probar el instalador sin root, sobre una copia de /etc y /var.
    let raiz = match args.iter().position(|a| *a == "--raiz") {
        Some(i) if i + 1 < args.len() => {
            let r = PathBuf::from(args[i + 1]);
            args.drain(i..=i + 1);
            Some(r)
        }
        Some(_) => {
            eprintln!("morgan-greet: {USO}");
            return ExitCode::FAILURE;
        }
        None => None,
    };
    // `--compositor`: en qué compositor corre el greeter; `install` lo deja en el command.
    let compositor = match args.iter().position(|a| *a == "--compositor") {
        Some(i) if i + 1 < args.len() => {
            let c = args[i + 1].to_owned();
            args.drain(i..=i + 1);
            if !morgan_lock::instalar::COMPOSITORES
                .iter()
                .any(|(n, _)| *n == c)
            {
                eprintln!("morgan-greet: compositor desconocido: {c}");
                return ExitCode::FAILURE;
            }
            Some(c)
        }
        Some(_) => {
            eprintln!("morgan-greet: {USO}");
            return ExitCode::FAILURE;
        }
        None => None,
    };
    match args.as_slice() {
        [] => lanzar(compositor),
        ["pantalla"] => pantalla(false),
        ["pantalla", "--demo"] => pantalla(true),
        ["install"] => instalar(raiz, compositor, false),
        ["install", "--apply"] => instalar(raiz, compositor, true),
        ["restore"] => restaurar(raiz, false),
        ["restore", "--apply"] => restaurar(raiz, true),
        _ => {
            eprintln!("morgan-greet: {USO}");
            ExitCode::FAILURE
        }
    }
}

/// Escribe la config y reemplaza este proceso por el compositor. Lo que el compositor
/// lanza hereda `GREETD_SOCK`.
fn lanzar(compositor: Option<String>) -> ExitCode {
    idioma_del_sistema();
    let Some(compositor) = compositor
        .or_else(|| morgan_lock::instalar::instalado(greeter::en_el_path).map(str::to_owned))
    else {
        eprintln!("morgan-greet: no hay ningún compositor que el greeter sepa lanzar");
        return ExitCode::FAILURE;
    };
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
    let pantalla = [exe.to_string_lossy().into_owned(), "pantalla".to_owned()];
    let (archivo, config) = match compositor.as_str() {
        "umbriel" => ("umbriel.toml", greeter::config_umbriel(&teclado)),
        _ => (
            "hyprland.lua",
            greeter::config_hyprland(&pantalla, &teclado),
        ),
    };
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|d| d.is_dir())
        .unwrap_or_else(std::env::temp_dir)
        .join("morgan-greet");
    let ruta = dir.join(archivo);
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&ruta, config)) {
        eprintln!("morgan-greet: {}: {e}", ruta.display());
        return ExitCode::FAILURE;
    }
    let e = match compositor.as_str() {
        // `umbriel` y no `start-umbriel`: ese arma una sesión de systemd del usuario, y acá
        // no hay usuario todavía.
        "umbriel" => Command::new("umbriel")
            .arg("-c")
            .arg(&ruta)
            .arg("-s")
            .arg(greeter::arranque_umbriel(&pantalla))
            .exec(),
        _ => Command::new("start-hyprland")
            .arg("--")
            .arg("-c")
            .arg(&ruta)
            .exec(),
    };
    eprintln!("morgan-greet: {compositor}: {e}");
    ExitCode::FAILURE
}

/// greetd no siempre le pasa el idioma al greeter: sin `LANG`, el de `/etc/locale.conf`, para
/// que la fecha salga en el idioma de la máquina. Antes de crear hilos y de lanzar Hyprland,
/// que lo hereda.
fn idioma_del_sistema() {
    if std::env::var_os("LANG").is_some() {
        return;
    }
    let Ok(t) = std::fs::read_to_string("/etc/locale.conf") else {
        return;
    };
    for l in t.lines() {
        if let Some((k, v)) = l.split_once('=')
            && (k == "LANG" || k.starts_with("LC_"))
            && std::env::var_os(k).is_none()
        {
            // SAFETY: todavía hay un solo hilo.
            unsafe { std::env::set_var(k, v.trim_matches('"')) };
        }
    }
}

/// Lo que va a hacer `install`, paso por paso, y si `aplicar`, lo hace.
fn instalar(raiz: Option<PathBuf>, compositor: Option<String>, aplicar: bool) -> ExitCode {
    use morgan_lock::instalar::*;
    let r = raiz.clone().unwrap_or_else(|| PathBuf::from("/"));
    if aplicar && raiz.is_none() && unsafe { libc::geteuid() } != 0 {
        eprintln!("morgan-greet: install --apply necesita root (sudo)");
        return ExitCode::FAILURE;
    }
    if let Some(c) = &compositor {
        let prog = COMPOSITORES.iter().find(|(n, _)| n == c).map(|(_, p)| *p);
        if !prog.is_some_and(greeter::en_el_path) {
            eprintln!("morgan-greet: {} no está instalado", nombre_visible(c));
            return ExitCode::FAILURE;
        }
    }
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("morgan-greet: {e}");
            return ExitCode::FAILURE;
        }
    };
    let del_paquete = exe.starts_with("/usr/");
    let binario = if del_paquete {
        exe.clone()
    } else {
        Path::new("/").join(BINARIO)
    };
    if let Some(m) = falta_greetd(&r) {
        eprintln!("morgan-greet: {m}");
        return ExitCode::FAILURE;
    }
    let greetd = r.join(GREETD);
    let actual = match std::fs::read_to_string(&greetd) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("morgan-greet: {}: {e}", greetd.display());
            return ExitCode::FAILURE;
        }
    };
    let compositor = match compositor {
        Some(c) => c,
        None => {
            let puesto = compositor_puesto(&actual, &binario.to_string_lossy());
            match elegir_compositor(puesto.as_deref()) {
                Some(c) => c,
                None => return ExitCode::FAILURE,
            }
        }
    };
    let command = comando(&binario.to_string_lossy(), &compositor);
    let nueva = match config_nueva(&actual, &command) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("morgan-greet: {e}");
            return ExitCode::FAILURE;
        }
    };
    let f = fecha();
    let respaldo = greetd.with_file_name(nombre_respaldo(&f));
    let estado = r.join(ESTADO);
    // El llavero, si está su módulo y no lo tiene ya.
    let pam = r.join(PAM);
    let pam_nueva = r
        .join(PAM_LLAVERO)
        .exists()
        .then(|| std::fs::read_to_string(&pam).ok())
        .flatten()
        .and_then(|t| pam_nuevo(&t));
    let respaldo_pam = greetd.with_file_name(nombre_respaldo_pam(&f));

    println!("Plan (compositor: {}):", nombre_visible(&compositor));
    if del_paquete {
        println!("  1. el greeter es el del paquete: {}", binario.display());
    } else {
        println!(
            "  1. copiar {} a {}",
            exe.display(),
            r.join(BINARIO).display()
        );
    }
    println!(
        "  2. crear {} (de greeter) y {}/usuarios (con sticky bit)",
        estado.display(),
        estado.display()
    );
    // Ya instalado: sin respaldo nuevo, así `restore` sigue volviendo a la de antes del greeter.
    let config_lista = nueva == actual;
    let con_greeter = tiene_greeter(&actual, &binario.to_string_lossy());
    if config_lista {
        println!(
            "  3-4. {} ya tiene el greeter: no se toca",
            greetd.display()
        );
    } else if con_greeter {
        println!(
            "  3-4. {} ya tiene el greeter: command = \"{command}\" (sin respaldo nuevo: `restore` vuelve a la de antes del greeter)",
            greetd.display()
        );
    } else {
        println!(
            "  3. respaldar {} en {}",
            greetd.display(),
            respaldo.display()
        );
        println!(
            "  4. en {}: [default_session] command = \"{command}\", y sin [initial_session] (sin inicio automático)",
            greetd.display(),
        );
    }
    if pam_nueva.is_some() {
        println!(
            "  5. respaldar {} en {} y sumarle pam_gnome_keyring: la clave del greeter abre el llavero",
            pam.display(),
            respaldo_pam.display()
        );
    }
    let logind = std::fs::read_to_string(r.join("etc/systemd/logind.conf")).unwrap_or_default();
    if hay_tty_de_emergencia(&logind) {
        println!(
            "Salida de emergencia: si el greeter no arranca, Ctrl+Alt+F2 da un login de texto;"
        );
        println!(
            "  ahí, `sudo morgan-greet restore --apply` (o el binario de {}).",
            binario.display()
        );
    } else {
        println!("Ojo: logind tiene NAutoVTs=0: sin login de texto de emergencia. Se sigue igual.");
    }
    println!("Vale desde el próximo arranque: greetd no se reinicia.");
    if let Some(m) = aviso_gestor(&r) {
        println!("{m}");
    }
    if !aplicar {
        println!(
            "Nada cambió. Para hacerlo: sudo {} --compositor {compositor} install --apply",
            exe.display()
        );
        return ExitCode::SUCCESS;
    }

    let hecho = (|| -> Result<(), String> {
        let e = |p: &Path, err: std::io::Error| format!("{}: {err}", p.display());
        if !del_paquete {
            let destino = r.join(BINARIO);
            let dir = destino.parent().unwrap_or(&r);
            std::fs::create_dir_all(dir).map_err(|x| e(dir, x))?;
            let tmp = destino.with_extension("nuevo");
            std::fs::copy(&exe, &tmp).map_err(|x| e(&tmp, x))?;
            std::fs::rename(&tmp, &destino).map_err(|x| e(&destino, x))?;
        }
        let usuarios = estado.join("usuarios");
        std::fs::create_dir_all(&usuarios).map_err(|x| e(&usuarios, x))?;
        permisos(&estado, 0o755)?;
        permisos(&usuarios, 0o1777)?;
        if raiz.is_none() {
            de_greeter(&estado)?;
        }
        if !config_lista {
            if !con_greeter {
                std::fs::copy(&greetd, &respaldo).map_err(|x| e(&respaldo, x))?;
            }
            // Con los permisos del original (en Arch, solo root la lee).
            reemplazar(&greetd, &nueva)?;
        }
        if let Some(n) = &pam_nueva {
            std::fs::copy(&pam, &respaldo_pam).map_err(|x| e(&respaldo_pam, x))?;
            reemplazar(&pam, n)?;
        }
        Ok(())
    })();
    match hecho {
        Ok(()) => {
            println!("Hecho.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("morgan-greet: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Sin `--compositor`: con uno solo instalado, ese; con varios, se pregunta en la terminal,
/// proponiendo el que ya tiene el greeter. Sin terminal, hay que elegirlo con `--compositor`.
fn elegir_compositor(puesto: Option<&str>) -> Option<String> {
    use morgan_lock::instalar::*;
    use std::io::{BufRead, IsTerminal, Write};
    let hay = instalados(greeter::en_el_path);
    let Some(prop) = propuesto(&hay, puesto) else {
        eprintln!(
            "morgan-greet: no hay ningún compositor que el greeter sepa lanzar (Hyprland o Umbriel)"
        );
        return None;
    };
    if hay.len() == 1 {
        return Some(prop.to_owned());
    }
    let nombres: Vec<&str> = hay.iter().map(|c| nombre_visible(c)).collect();
    if !std::io::stdin().is_terminal() {
        eprintln!(
            "morgan-greet: hay varios compositores ({}); elegí uno con --compositor {}",
            nombres.join(", "),
            hay.join("|")
        );
        return None;
    }
    println!(
        "La pantalla de inicio corre dentro de un compositor, aparte de la sesión a la que se entra."
    );
    println!("Hay varios instalados:");
    for (i, (c, n)) in hay.iter().zip(&nombres).enumerate() {
        let marca = if *c == prop { "  (Enter)" } else { "" };
        println!("  {}. {n}{marca}", i + 1);
    }
    let mut entrada = std::io::stdin().lock();
    loop {
        print!("¿Cuál? ");
        let _ = std::io::stdout().flush();
        let mut linea = String::new();
        match entrada.read_line(&mut linea) {
            Ok(0) | Err(_) => return None,
            Ok(_) => {}
        }
        if let Some(c) = respuesta(&linea, &hay, prop) {
            return Some(c.to_owned());
        }
        println!(
            "Un número de la lista, o Enter para {}.",
            nombre_visible(prop)
        );
    }
}

/// Escribe `p` por un temporal que se renombra, con los permisos que tenía.
fn reemplazar(p: &Path, texto: &str) -> Result<(), String> {
    let e = |q: &Path, err: std::io::Error| format!("{}: {err}", q.display());
    let tmp = p.with_extension("morgan-nuevo");
    std::fs::write(&tmp, texto).map_err(|x| e(&tmp, x))?;
    let modo = std::fs::metadata(p).map_err(|x| e(p, x))?.permissions();
    std::fs::set_permissions(&tmp, modo).map_err(|x| e(&tmp, x))?;
    std::fs::rename(&tmp, p).map_err(|x| e(p, x))
}

fn permisos(p: &Path, modo: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(modo))
        .map_err(|e| format!("{}: {e}", p.display()))
}

/// Dueño: el usuario `greeter` (escribe ahí el recuerdo de la última cuenta y sesión).
fn de_greeter(p: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: getpwnam con una cadena terminada en nul; se copian uid y gid enseguida.
    let pw = unsafe { libc::getpwnam(c"greeter".as_ptr()) };
    if pw.is_null() {
        return Err("no existe el usuario greeter (lo crea el paquete de greetd)".into());
    }
    let (uid, gid) = unsafe { ((*pw).pw_uid, (*pw).pw_gid) };
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    // SAFETY: ruta terminada en nul.
    if unsafe { libc::chown(c.as_ptr(), uid, gid) } != 0 {
        return Err(format!(
            "{}: {}",
            p.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Vuelve al último respaldo de la config de greetd. El binario y las carpetas quedan:
/// sin la config, nada los usa.
fn restaurar(raiz: Option<PathBuf>, aplicar: bool) -> ExitCode {
    use morgan_lock::instalar::*;
    let r = raiz.clone().unwrap_or_else(|| PathBuf::from("/"));
    if aplicar && raiz.is_none() && unsafe { libc::geteuid() } != 0 {
        eprintln!("morgan-greet: restore --apply necesita root (sudo)");
        return ExitCode::FAILURE;
    }
    let greetd = r.join(GREETD);
    let Some(respaldo) = greetd.parent().and_then(ultimo_respaldo) else {
        eprintln!(
            "morgan-greet: no hay respaldos de morgan-greet junto a {}",
            greetd.display()
        );
        return ExitCode::FAILURE;
    };
    let pam = r.join(PAM);
    let respaldo_pam = greetd.parent().and_then(ultimo_respaldo_pam);
    println!(
        "Plan: copiar {} sobre {}",
        respaldo.display(),
        greetd.display()
    );
    if let Some(rp) = &respaldo_pam {
        println!(
            "  y {} sobre {} (sin el llavero)",
            rp.display(),
            pam.display()
        );
    }
    println!("Vale desde el próximo arranque: greetd no se reinicia.");
    if !aplicar {
        println!("Nada cambió. Para hacerlo: sudo morgan-greet restore --apply");
        return ExitCode::SUCCESS;
    }
    let volver = |de: &Path, a: &Path| {
        std::fs::read_to_string(de)
            .map_err(|e| format!("{}: {e}", de.display()))
            .and_then(|t| reemplazar(a, &t))
    };
    let hecho = volver(&respaldo, &greetd).and_then(|()| match &respaldo_pam {
        Some(rp) => volver(rp, &pam),
        None => Ok(()),
    });
    match hecho {
        Ok(()) => {
            println!("Hecho.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("morgan-greet: {e}");
            ExitCode::FAILURE
        }
    }
}

fn pantalla(demo: bool) -> ExitCode {
    if !demo {
        registro();
    }
    let mut usuarios =
        greeter::usuarios(&std::fs::read_to_string("/etc/passwd").unwrap_or_default());
    let sesiones = greeter::sesiones(&[PathBuf::from("/usr/share/wayland-sessions")]);
    if demo {
        // Con dos cuentas, para ver los dos pasos aunque la máquina tenga una.
        usuarios.push(Usuario {
            nombre: "invitado".into(),
            uid: u32::MAX,
        });
    }
    if usuarios.is_empty() {
        eprintln!("morgan-greet: no hay cuentas de personas en /etc/passwd");
        return ExitCode::FAILURE;
    }
    if sesiones.is_empty() {
        eprintln!("morgan-greet: no hay sesiones en /usr/share/wayland-sessions");
        return ExitCode::FAILURE;
    }
    let recuerdo = if demo {
        Recuerdo::default()
    } else {
        Recuerdo::leer()
    };
    let u = recuerdo
        .usuario
        .as_ref()
        .and_then(|n| usuarios.iter().position(|u| &u.nombre == n))
        .unwrap_or(0);
    let s = recuerdo
        .sesion
        .as_ref()
        .and_then(|id| sesiones.iter().position(|s| &s.id == id))
        .unwrap_or_else(|| greeter::por_defecto(&sesiones));
    let eleccion = Some((
        usuarios.iter().map(|u| u.nombre.clone()).collect(),
        sesiones.iter().map(|s| s.nombre.clone()).collect(),
        u,
        s,
    ));
    if demo {
        return pantalla::correr(Ajustes {
            verificador: Box::new(Demo),
            logind: false,
            xray: false,
            rutas: Rutas::del_home(),
            eleccion,
        });
    }
    let greetd = match Greetd::conectar(&usuarios[u].nombre) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("morgan-greet: {e}");
            return ExitCode::FAILURE;
        }
    };
    if morgan_ipc::compositor::actual() == morgan_ipc::compositor::Tipo::Umbriel {
        greeter::poner_modos_umbriel();
    }
    // Los colores y el fondo, los de la cuenta con la que se empieza.
    let rutas = greeter::rutas_de(&usuarios[u], Path::new(BASE));
    pantalla::correr(Ajustes {
        verificador: Box::new(Entrar {
            greetd,
            usuarios,
            sesiones,
            u,
            s,
        }),
        logind: false,
        xray: false,
        rutas: Some(rutas),
        eleccion,
    })
}

/// Lo que la pantalla escribe en stderr va a `/var/lib/morgan-greet/pantalla.log`, que se
/// vacía en cada arranque: greetd no guarda la salida del greeter, y sin esto no queda rastro
/// de qué pasó al prender la máquina (p. ej., si el teclado entró). Si no se puede abrir,
/// sigue a stderr.
fn registro() {
    use std::os::fd::AsRawFd;
    let ruta = Path::new(BASE).join("pantalla.log");
    match std::fs::File::create(&ruta) {
        // SAFETY: dup2 sobre descriptores abiertos; el archivo queda abierto como fd 2.
        Ok(f) => unsafe {
            libc::dup2(f.as_raw_fd(), 2);
        },
        Err(e) => eprintln!("morgan-greet: {}: {e}", ruta.display()),
    }
}

/// greetd, y al acertar, la sesión elegida.
struct Entrar {
    greetd: Greetd,
    usuarios: Vec<Usuario>,
    sesiones: Vec<Sesion>,
    u: usize,
    s: usize,
}

impl Verificador for Entrar {
    fn verificar(&mut self, clave: Secreto) -> Resultado {
        self.greetd.verificar(clave)
    }

    fn elegir(&mut self, u: usize, s: usize) {
        if u < self.usuarios.len() && s < self.sesiones.len() {
            (self.u, self.s) = (u, s);
            if let Err(e) = self.greetd.usuario(&self.usuarios[u].nombre) {
                eprintln!("morgan-greet: {e}");
            }
        }
    }

    fn al_entrar(&mut self) -> Result<(), String> {
        let sesion = &self.sesiones[self.s];
        self.greetd
            .iniciar(&sesion.cmd, &sesion.entorno())
            .map_err(|e| e.to_string())?;
        let r = Recuerdo {
            usuario: Some(self.usuarios[self.u].nombre.clone()),
            sesion: Some(sesion.id.clone()),
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
