//! `morgan setup`: lo que cada usuario hace una vez y un paquete no puede hacer por él (la
//! línea de `hyprland.lua`, el include de la config de Umbriel y la unidad de systemd --user). Se puede correr de nuevo: lo que ya
//! está no se repite.

use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::{Command, ExitCode};

use crate::{setup_paquetes, setup_parar};

use morgan_idioma::{tr, trf};

pub const LINEA: &str = "pcall(require, \"morgan\")"; // no-tr

/// La primera línea del comentario es la que busca `install.sh --desinstalar`.
const COMENTARIO: &str = "-- morgan: sus binds, los colores del tema, las reglas de sus capas y lo que se cambia\n-- desde su menú. Va al final, para pisar lo de arriba.\n"; // no-tr

/// El texto con la línea de morgan al final; `None` si ya la tiene.
pub fn con_linea(texto: &str) -> Option<String> {
    if texto.lines().any(|l| l.trim() == LINEA) {
        return None;
    }
    let mut t = texto.to_owned();
    if !t.is_empty() && !t.ends_with('\n') {
        t.push('\n');
    }
    t.push('\n');
    t.push_str(COMENTARIO);
    t.push_str(LINEA);
    t.push('\n');
    Some(t)
}

pub fn correr() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let mut seco = false;
    let mut deshacer = false;
    for a in &args {
        match a.as_str() {
            "--seco" | "--dry-run" => seco = true, // no-tr
            "--deshacer" | "--undo" => deshacer = true, // no-tr
            _ => {
                eprintln!("{}", tr!("uso: morgan setup [--seco] [--deshacer]"));
                return ExitCode::FAILURE;
            }
        }
    }
    let config = glib::user_config_dir();
    let hypr_dir = config.join("hypr");
    let hypr = hypr_dir.join("hyprland.lua");
    if deshacer {
        return correr_deshacer(&hypr_dir, &hypr, seco);
    }
    let umbriel = glib::find_program_in_path("umbriel").is_some();
    if hypr.is_file() {
        if seco {
            if con_linea(&std::fs::read_to_string(&hypr).unwrap_or_default()).is_some() {
                println!("{}", trf!("Haría: agregar la línea de morgan al final de {}.", hypr.display()));
            }
        } else if let Err(e) = agregar_linea(&hypr) {
            eprintln!("{}", trf!("No se pudo escribir {}: {}", hypr.display(), e));
            return ExitCode::FAILURE;
        }
    } else if !umbriel {
        if config.join("hypr/hyprland.conf").is_file() {
            eprintln!("{}", trf!("morgan necesita la config de Hyprland en Lua ({}); hay una hyprland.conf.", hypr.display()));
        } else {
            eprintln!("{}", trf!("No está {}: morgan necesita la config de Hyprland en Lua.", hypr.display()));
        }
        return ExitCode::FAILURE;
    }
    if umbriel && !seco {
        let c = crate::config::path()
            .ok()
            .and_then(|p| crate::config::load(&p).ok())
            .unwrap_or_default();
        if let Err(e) = crate::setup_umbriel::correr(&c) {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    }
    let interactivo = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    paso_competidores(&hypr_dir, interactivo, seco);
    paso_recomendados(interactivo, seco);
    if seco {
        println!("{}", tr!("Nada cambió (--seco)."));
        return ExitCode::SUCCESS;
    }
    if !systemctl(&["enable", "morgan.service"]) {
        return ExitCode::FAILURE;
    }
    let ajenas = notificaciones_ajenas();
    if let Some(quien) = &ajenas {
        println!("  ! {}", trf!("{} tiene las notificaciones: hay que pararlo antes de arrancar morgan.", quien));
    }
    let en_sesion = morgan_ipc::compositor::actual() != morgan_ipc::compositor::Tipo::Otro;
    if en_sesion && ajenas.is_none() {
        let _ = Command::new("systemctl")
            .args(["--user", "import-environment"])
            .args(VARIABLES)
            .status();
        if systemctl(&["start", "morgan.service"]) {
            println!("{}", tr!("morgan arrancó. El menú de sistema se abre con Super+coma."));
        }
    } else {
        println!("{}", tr!("Listo: morgan arranca la próxima vez que entres a Hyprland o a Umbriel."));
    }
    println!("{}", tr!("Para conocer a Morgan: morgan about"));
    ExitCode::SUCCESS
}

/// Lo que hace lo mismo que morgan: se lista y, con permiso, se para y se saca de donde arranca.
fn paso_competidores(hypr_dir: &Path, interactivo: bool, seco: bool) {
    let hallazgos = setup_parar::buscar(hypr_dir);
    if hallazgos.is_empty() {
        return;
    }
    println!();
    println!("{}", tr!("Esto hace lo mismo que morgan y se pisaría con él:"));
    let lineas: Vec<String> = hallazgos.iter().map(|h| h.resumen(hypr_dir)).collect();
    let elegidos = if interactivo || seco {
        elegir(&lineas, tr!("¿Lo paro y lo saco de donde arranca? (morgan setup --deshacer lo devuelve)"), seco)
    } else {
        for l in &lineas {
            println!("  ! {l}");
        }
        println!("{}", tr!("Para pararlo, corré morgan setup en una terminal."));
        Vec::new()
    };
    let elegidos: Vec<&setup_parar::Hallazgo> = elegidos.into_iter().map(|i| &hallazgos[i]).collect();
    if !elegidos.is_empty() {
        setup_parar::parar(&elegidos, hypr_dir, seco);
    }
}

/// Los programas y servicios recomendados: los que faltan, para elegir, y lo que va a mano.
fn paso_recomendados(interactivo: bool, seco: bool) {
    let plan = setup_paquetes::armar();
    let mut a_mano = plan.a_mano;
    if !plan.items.is_empty() {
        println!();
        println!("{}", tr!("Recomendado para morgan, y no está instalado:"));
        let lineas: Vec<String> = plan.items.iter().map(|i| i.texto.clone()).collect();
        if interactivo || seco {
            let elegidos = elegir(&lineas, tr!("¿Lo instalo? (pide la clave de administrador)"), seco);
            let items: Vec<&setup_paquetes::Item> = elegidos.into_iter().map(|i| &plan.items[i]).collect();
            if !items.is_empty() {
                a_mano.extend(setup_paquetes::instalar(&items, seco));
            }
        } else {
            for l in &lineas {
                println!("  - {l}");
            }
            println!("{}", tr!("Para instalarlo, corré morgan setup en una terminal."));
        }
    }
    if !a_mano.is_empty() {
        println!();
        println!("{}", tr!("Esto no lo hago solo, porque podría dejarte sin red, sin audio o con dos programas peleándose. Los pasos, por si querés hacerlo:"));
        for paso in a_mano {
            println!("  - {paso}"); // no-tr
        }
    }
}

fn correr_deshacer(hypr_dir: &Path, hypr: &Path, seco: bool) -> ExitCode {
    println!("{}", tr!("Devolviendo lo que paró morgan setup (vuelve la próxima vez que entres):"));
    setup_parar::deshacer(hypr_dir, seco);
    let verbo = if seco { tr!("haría") } else { tr!("hecho") };
    println!("  {verbo}: systemctl --user disable morgan.service"); // no-tr
    if !seco {
        let _ = Command::new("systemctl").args(["--user", "disable", "morgan.service"]).status();
    }
    if let Some(nuevo) = std::fs::read_to_string(hypr).ok().and_then(|t| sin_linea(&t)) {
        println!("  {verbo}: {}", trf!("sacar la línea de morgan de {}", hypr.display())); // no-tr
        if !seco && let Err(e) = std::fs::write(hypr, nuevo) {
            eprintln!("{}", trf!("No se pudo escribir {}: {}", hypr.display(), e));
            return ExitCode::FAILURE;
        }
    }
    println!("{}", tr!("La config de morgan (~/.config/morgan) y los programas instalados quedan."));
    ExitCode::SUCCESS
}

/// El texto sin la línea de morgan ni su comentario; `None` si no la tiene.
pub fn sin_linea(texto: &str) -> Option<String> {
    if !texto.lines().any(|l| l.trim() == LINEA) {
        return None;
    }
    let t = texto.replace(&format!("\n\n{COMENTARIO}{LINEA}\n"), "\n");
    let t = t.replace(&format!("\n{COMENTARIO}{LINEA}\n"), "\n");
    let t = t.replace(&format!("{COMENTARIO}{LINEA}\n"), "");
    let t: Vec<&str> = t.lines().filter(|l| l.trim() != LINEA).collect();
    let mut t = t.join("\n");
    if texto.ends_with('\n') && !t.is_empty() {
        t.push('\n');
    }
    Some(t)
}

/// Lista `opciones` numeradas, todas marcadas, y pregunta cuáles. Con `seco` no pregunta: todas.
fn elegir(opciones: &[String], pregunta: &str, seco: bool) -> Vec<usize> {
    for (i, o) in opciones.iter().enumerate() {
        println!("  {:>2}. {o}", i + 1);
    }
    if seco {
        return (0..opciones.len()).collect();
    }
    loop {
        print!("{pregunta}\n{} ", tr!("Enter: todo · n: nada · los números a dejar afuera (p. ej. 2 5):")); // no-tr
        let _ = std::io::stdout().flush();
        let mut r = String::new();
        if std::io::stdin().read_line(&mut r).unwrap_or(0) == 0 {
            return Vec::new();
        }
        match leer_eleccion(&r, opciones.len()) {
            Some(v) => return v,
            None => println!("{}", tr!("No entendí. Probá de nuevo.")),
        }
    }
}

/// La respuesta a `elegir`: vacía es todo, `n` es nada, números son los que quedan afuera.
fn leer_eleccion(r: &str, n: usize) -> Option<Vec<usize>> {
    let r = r.trim().to_lowercase();
    if r.is_empty() || r == "s" || r == "y" {
        return Some((0..n).collect());
    }
    if r == "n" || r == "no" {
        return Some(Vec::new());
    }
    let mut afuera = Vec::new();
    for p in r.split(|c: char| c == ',' || c.is_whitespace()).filter(|p| !p.is_empty()) {
        let i: usize = p.parse().ok()?;
        if i == 0 || i > n {
            return None;
        }
        afuera.push(i - 1);
    }
    Some((0..n).filter(|i| !afuera.contains(i)).collect())
}

/// Lo que la unidad necesita de la sesión gráfica; sin uwsm, el gestor de systemd no lo tiene.
pub const VARIABLES: &[&str] = &[
    "WAYLAND_DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE", "XDG_CURRENT_DESKTOP", "XDG_SESSION_TYPE", "DISPLAY", // no-tr
];

fn agregar_linea(hypr: &Path) -> std::io::Result<()> {
    let texto = std::fs::read_to_string(hypr)?;
    match con_linea(&texto) {
        Some(nuevo) => {
            std::fs::write(hypr, nuevo)?;
            println!("{}", trf!("Agregada la línea de morgan al final de {}.", hypr.display()));
        }
        None => println!("{}", trf!("{} ya carga morgan.", hypr.display())),
    }
    Ok(())
}

fn systemctl(args: &[&str]) -> bool {
    match Command::new("systemctl").arg("--user").args(args).status() {
        Ok(s) if s.success() => true,
        _ => {
            eprintln!("{}", trf!("Falló systemctl --user {}.", args.join(" ")));
            false
        }
    }
}

/// El proceso que tiene `org.freedesktop.Notifications`, si no es morgan.
fn notificaciones_ajenas() -> Option<String> {
    let o = Command::new("busctl")
        .args(["--user", "status", "org.freedesktop.Notifications"]) // no-tr
        .output()
        .ok()?;
    let texto = String::from_utf8_lossy(&o.stdout);
    let quien = texto.lines().find_map(|l| l.strip_prefix("Comm="))?.trim().to_owned();
    (quien != "morgan").then_some(quien)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_linea_va_una_vez_al_final() {
        let t = con_linea("hl.config({})").unwrap();
        assert!(t.starts_with("hl.config({})\n\n-- morgan:"));
        assert!(t.ends_with("pcall(require, \"morgan\")\n"));
        assert_eq!(con_linea(&t), None);
        assert_eq!(con_linea("  pcall(require, \"morgan\")  \n"), None);
    }

    #[test]
    fn sacar_la_linea_deja_lo_de_antes() {
        let antes = "hl.config({})\n";
        let con = con_linea(antes).unwrap();
        assert_eq!(sin_linea(&con).as_deref(), Some(antes));
        assert_eq!(sin_linea(antes), None);
        assert_eq!(sin_linea("a\npcall(require, \"morgan\")\nb\n").as_deref(), Some("a\nb\n"));
    }

    #[test]
    fn la_eleccion() {
        assert_eq!(leer_eleccion("\n", 3), Some(vec![0, 1, 2]));
        assert_eq!(leer_eleccion("n", 3), Some(vec![]));
        assert_eq!(leer_eleccion(" 2 ", 3), Some(vec![0, 2]));
        assert_eq!(leer_eleccion("1,3", 3), Some(vec![1]));
        assert_eq!(leer_eleccion("4", 3), None);
        assert_eq!(leer_eleccion("x", 3), None);
    }
}
