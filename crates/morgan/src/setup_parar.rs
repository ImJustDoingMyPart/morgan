//! `morgan setup`, lo que compite con morgan: otra barra, otro servidor de notificaciones, otro
//! idle… Se busca corriendo y en los tres lugares desde donde suele arrancar: una unidad de
//! systemd --user, una línea de autoarranque de la config de Hyprland (`hl.exec_cmd`, no la de
//! un atajo) y un `.desktop` de autostart. Pararlo deja todo anotado para que
//! `morgan setup --deshacer` lo devuelva: las unidades en `setup-parados` (estado de morgan), las
//! líneas comentadas con `MARCA` y los `.desktop` renombrados o tapados con `TAPA`.

use std::path::{Path, PathBuf};
use std::process::Command;

use morgan_idioma::{t, tr, tr_const, trf};

pub struct Competidor {
    pub programa: &'static str,
    /// Lo que hace, para la lista (se traduce al mostrarlo).
    pub rol: &'static str,
}

const fn c(programa: &'static str, rol: &'static str) -> Competidor {
    Competidor { programa, rol }
}

/// Los shells de Quickshell (Noctalia hasta la v4, DankMaterialShell, Caelestia) corren como
/// `quickshell`, o `qs` si se lanzaron por ese nombre; Noctalia desde la v5 es `noctalia`.
/// hyprsunset no está: la luz nocturna de morgan lo usa.
pub const COMPETIDORES: &[Competidor] = &[
    c("waybar", tr_const!("barra")), // no-tr
    c("swaync", tr_const!("notificaciones")), // no-tr
    c("mako", tr_const!("notificaciones")), // no-tr
    c("dunst", tr_const!("notificaciones")), // no-tr
    c("swayosd-server", tr_const!("OSD de volumen y brillo")), // no-tr
    c("hypridle", tr_const!("idle")), // no-tr
    c("swayidle", tr_const!("idle")), // no-tr
    c("hyprlock", tr_const!("pantalla de bloqueo")), // no-tr
    c("swaylock", tr_const!("pantalla de bloqueo")), // no-tr
    c("quickshell", tr_const!("shell completo")), // no-tr
    c("qs", tr_const!("shell completo")), // no-tr
    c("noctalia", tr_const!("shell completo")), // no-tr
    c("ags", tr_const!("shell completo")), // no-tr
    c("eww", tr_const!("barra o widgets")), // no-tr
    c("hyprpolkitagent", tr_const!("pedidos de clave (polkit)")), // no-tr
    c("polkit-gnome-authentication-agent-1", tr_const!("pedidos de clave (polkit)")), // no-tr
    c("polkit-kde-authentication-agent-1", tr_const!("pedidos de clave (polkit)")), // no-tr
    c("hyprpaper", tr_const!("fondos de pantalla")), // no-tr
    c("swaybg", tr_const!("fondos de pantalla")), // no-tr
    c("wlsunset", tr_const!("luz nocturna")), // no-tr
    c("gammastep", tr_const!("luz nocturna")), // no-tr
];

/// La marca de las líneas que comentó `morgan setup` en la config de Hyprland.
pub const MARCA: &str = "-- [morgan setup] "; // no-tr
/// La línea que distingue un `.desktop` que tapa al del sistema.
const TAPA: &str = "X-Morgan-Setup=true"; // no-tr
/// La extensión de un `.desktop` propio apartado.
const APARTADO: &str = ".morgan-parado"; // no-tr

/// Lo encontrado de un competidor.
#[derive(Debug, Default)]
pub struct Hallazgo {
    pub programa: &'static str,
    pub rol: &'static str,
    /// Si hay un proceso corriendo, dentro o fuera de una unidad.
    pub corre: bool,
    /// Los procesos que corren fuera de una unidad que se deshabilita.
    pub pids: Vec<i32>,
    pub unidades: Vec<String>,
    /// Líneas de autoarranque de la config de Hyprland que solo lanzan esto.
    pub lineas: Vec<(PathBuf, usize)>,
    /// Líneas que lo lanzan junto con otra cosa: se cambian a mano.
    pub mezcladas: Vec<(PathBuf, usize)>,
    /// `.desktop` de autostart: el camino y si es del usuario.
    pub autostart: Vec<(PathBuf, bool)>,
}

impl Hallazgo {
    fn vacio(&self) -> bool {
        self.pids.is_empty()
            && self.unidades.is_empty()
            && self.lineas.is_empty()
            && self.mezcladas.is_empty()
            && self.autostart.is_empty()
    }

    /// Una línea para la lista: qué es y dónde está.
    pub fn resumen(&self, hypr: &Path) -> String {
        let mut donde = Vec::new();
        if self.corre {
            donde.push(tr!("corre ahora").to_owned());
        }
        donde.extend(self.unidades.iter().cloned());
        for (archivo, n) in self.lineas.iter().chain(&self.mezcladas) {
            donde.push(format!("{}:{n}", corto(archivo, hypr)));
        }
        if !self.autostart.is_empty() {
            donde.push(tr!("apps de inicio").to_owned());
        }
        format!("{} ({}): {}", self.programa, t(self.rol), donde.join(" · ")) // no-tr
    }
}

/// El camino relativo a la carpeta de Hyprland, si está adentro.
fn corto(p: &Path, hypr: &Path) -> String {
    p.strip_prefix(hypr).unwrap_or(p).display().to_string()
}

/// Busca cada competidor corriendo y en los lugares desde donde arranca.
pub fn buscar(hypr: &Path) -> Vec<Hallazgo> {
    let procesos = procesos();
    let unidades = unidades_habilitadas();
    let lineas = lineas_lua(hypr);
    let autostart = autostarts();
    let mut v = Vec::new();
    for comp in COMPETIDORES {
        let mut h = Hallazgo {
            programa: comp.programa,
            rol: comp.rol,
            ..Default::default()
        };
        for (u, prog) in &unidades {
            if prog == comp.programa && !h.unidades.contains(u) {
                h.unidades.push(u.clone());
            }
        }
        for (pid, prog, unidad) in &procesos {
            if prog != comp.programa {
                continue;
            }
            h.corre = true;
            match unidad {
                // Una unidad propia (no la de una app lanzada por uwsm): se deshabilita y para.
                Some(u) if !u.starts_with("app-") => {
                    if !h.unidades.contains(u) {
                        h.unidades.push(u.clone());
                    }
                }
                _ => h.pids.push(*pid),
            }
        }
        for (archivo, n, linea) in &lineas {
            match clasificar(linea) {
                Some(Lanzado::Solo(p)) if p == comp.programa => h.lineas.push((archivo.clone(), *n)),
                Some(Lanzado::Mezcla(palabras)) if palabras.iter().any(|w| w == comp.programa) => {
                    h.mezcladas.push((archivo.clone(), *n))
                }
                _ => {}
            }
        }
        for (camino, propio, prog) in &autostart {
            if prog == comp.programa {
                h.autostart.push((camino.clone(), *propio));
            }
        }
        if !h.vacio() {
            v.push(h);
        }
    }
    v
}

/// Para lo elegido y lo saca de donde arranca. Con `seco`, solo dice qué haría.
pub fn parar(hallazgos: &[&Hallazgo], hypr: &Path, seco: bool) {
    let verbo = if seco { tr!("haría") } else { tr!("hecho") };
    let mut a_mano = Vec::new();
    for h in hallazgos {
        for u in &h.unidades {
            println!("  {verbo}: systemctl --user disable --now {u}"); // no-tr
            if !seco {
                let ok = Command::new("systemctl")
                    .args(["--user", "disable", "--now", u])
                    .status()
                    .is_ok_and(|s| s.success());
                if ok {
                    anotar_unidad(u);
                } else {
                    eprintln!("morgan setup: systemctl --user disable --now {u} falló");
                }
            }
        }
        for (archivo, n) in &h.lineas {
            println!("  {verbo}: {}", trf!("comentar la línea {} de {}", n, corto(archivo, hypr))); // no-tr
            if !seco && let Err(e) = comentar_en(archivo, *n) {
                eprintln!("morgan setup: {}: {e}", archivo.display());
            }
        }
        for (camino, propio) in &h.autostart {
            let nombre = camino.file_name().unwrap_or_default().to_string_lossy();
            println!("  {verbo}: {}", trf!("sacar {} de las apps de inicio", nombre)); // no-tr
            if !seco && let Err(e) = sacar_autostart(camino, *propio) {
                eprintln!("morgan setup: {}: {e}", camino.display());
            }
        }
        for pid in &h.pids {
            println!("  {verbo}: {}", trf!("cerrar {} (proceso {})", h.programa, pid)); // no-tr
            if !seco {
                // SAFETY: kill solo manda una señal a un proceso propio.
                unsafe {
                    libc::kill(*pid, libc::SIGTERM);
                }
            }
        }
        for (archivo, n) in &h.mezcladas {
            a_mano.push(trf!(
                "En {}:{} la línea lanza {} junto con otras cosas: sacá {} de esa línea.",
                archivo.display(),
                n,
                h.programa,
                h.programa
            ));
        }
        let sin_origen = h.unidades.is_empty() && h.lineas.is_empty() && h.mezcladas.is_empty() && h.autostart.is_empty();
        if sin_origen {
            a_mano.push(trf!(
                "No encontré desde dónde arranca {}. Si vuelve la próxima vez que entres, buscalo en la config de Hyprland ({}) o en las apps de inicio.",
                h.programa,
                hypr.display()
            ));
        }
    }
    if !a_mano.is_empty() {
        println!("{}", tr!("A mano:"));
        for paso in a_mano {
            println!("  - {paso}"); // no-tr
        }
    }
}

/// Devuelve lo que paró `parar`: habilita las unidades (arrancan en el próximo inicio de sesión),
/// descomenta las líneas y repone los `.desktop`.
pub fn deshacer(hypr: &Path, seco: bool) {
    let verbo = if seco { tr!("haría") } else { tr!("hecho") };
    let archivo = estado();
    let unidades = std::fs::read_to_string(&archivo).unwrap_or_default();
    for u in unidades.lines().filter(|l| !l.trim().is_empty()) {
        println!("  {verbo}: systemctl --user enable {u}"); // no-tr
        if !seco {
            let _ = Command::new("systemctl").args(["--user", "enable", u]).status();
        }
    }
    if !seco && !unidades.is_empty() {
        let _ = std::fs::remove_file(&archivo);
    }
    for archivo in archivos_lua(hypr) {
        let Ok(texto) = std::fs::read_to_string(&archivo) else { continue };
        if let Some(nuevo) = descomentar(&texto) {
            println!("  {verbo}: {}", trf!("volver a activar las líneas de {}", corto(&archivo, hypr))); // no-tr
            if !seco && let Err(e) = std::fs::write(&archivo, nuevo) {
                eprintln!("morgan setup: {}: {e}", archivo.display());
            }
        }
    }
    let propios = glib::user_config_dir().join("autostart");
    for e in std::fs::read_dir(&propios).into_iter().flatten().flatten() {
        let p = e.path();
        let nombre = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if let Some(original) = nombre.strip_suffix(APARTADO) {
            println!("  {verbo}: {}", trf!("devolver {} a las apps de inicio", original)); // no-tr
            if !seco && !propios.join(original).exists() {
                let _ = std::fs::rename(&p, propios.join(original));
            }
        } else if std::fs::read_to_string(&p).is_ok_and(|t| t.lines().any(|l| l.trim() == TAPA)) {
            println!("  {verbo}: {}", trf!("devolver {} a las apps de inicio", nombre)); // no-tr
            if !seco {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}

fn estado() -> PathBuf {
    glib::user_state_dir().join("morgan/setup-parados")
}

fn anotar_unidad(u: &str) {
    let p = estado();
    let mut t = std::fs::read_to_string(&p).unwrap_or_default();
    if t.lines().any(|l| l == u) {
        return;
    }
    t.push_str(u);
    t.push('\n');
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&p, t);
}

// --- Procesos y unidades ---------------------------------------------------------------------

/// Los procesos del usuario: pid, programa (el nombre de `argv[0]`) y la unidad en la que corren.
fn procesos() -> Vec<(i32, String, Option<String>)> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: getuid no falla ni toca memoria.
    let uid = unsafe { libc::getuid() };
    let mut v = Vec::new();
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else { continue };
        if e.metadata().map(|m| m.uid()).ok() != Some(uid) {
            continue;
        }
        let dir = e.path();
        let Ok(cmd) = std::fs::read(dir.join("cmdline")) else { continue };
        let argv0 = cmd.split(|b| *b == 0).next().unwrap_or_default();
        let argv0 = String::from_utf8_lossy(argv0);
        let Some(prog) = nombre(&argv0) else { continue };
        let unidad = std::fs::read_to_string(dir.join("cgroup")).ok().and_then(|c| unidad_de_cgroup(&c));
        v.push((pid, prog.to_owned(), unidad));
    }
    v
}

/// El nombre de un programa a partir de su camino o de `argv[0]`.
fn nombre(s: &str) -> Option<&str> {
    let n = s.rsplit('/').next()?.trim();
    (!n.is_empty()).then_some(n)
}

/// La unidad de servicio de un proceso, de su `/proc/<pid>/cgroup` (cgroup v2).
fn unidad_de_cgroup(c: &str) -> Option<String> {
    // La última parte: la de más arriba (user@1000.service) es la del gestor de systemd.
    let ultima = c.lines().find_map(|l| l.strip_prefix("0::"))?.trim().rsplit('/').next()?;
    ultima.ends_with(".service").then(|| ultima.to_owned())
}

/// Las unidades de servicio habilitadas del usuario y el programa que lanza cada una.
fn unidades_habilitadas() -> Vec<(String, String)> {
    let Ok(o) = Command::new("systemctl")
        .args(["--user", "list-unit-files", "--type=service", "--state=enabled", "--no-legend", "--plain"])
        .output()
    else {
        return Vec::new();
    };
    let lista = String::from_utf8_lossy(&o.stdout);
    let nombres: Vec<&str> = lista
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|u| !u.contains("@.") && *u != "morgan.service" && *u != "morgan-lock.service")
        .collect();
    if nombres.is_empty() {
        return Vec::new();
    }
    let Ok(o) = Command::new("systemctl")
        .args(["--user", "show", "-p", "Id", "-p", "ExecStart"]) // no-tr
        .args(&nombres)
        .output()
    else {
        return Vec::new();
    };
    programas_de_show(&String::from_utf8_lossy(&o.stdout))
}

/// De la salida de `systemctl show -p Id -p ExecStart`: la unidad y el programa que lanza.
fn programas_de_show(s: &str) -> Vec<(String, String)> {
    let mut v = Vec::new();
    for bloque in s.split("\n\n") {
        let mut id = None;
        let mut prog = None;
        for l in bloque.lines() {
            if let Some(i) = l.strip_prefix("Id=") {
                id = Some(i.to_owned());
            } else if let Some(e) = l.strip_prefix("ExecStart=") {
                prog = e
                    .split(" ; ")
                    .find_map(|p| p.trim_start_matches("{ ").strip_prefix("path="))
                    .and_then(nombre)
                    .map(str::to_owned);
            }
        }
        if let (Some(i), Some(p)) = (id, prog) {
            v.push((i, p));
        }
    }
    v
}

// --- La config de Hyprland -------------------------------------------------------------------

/// Los `.lua` de la config de Hyprland, sin el que genera morgan.
fn archivos_lua(hypr: &Path) -> Vec<PathBuf> {
    fn recorrer(dir: &Path, v: &mut Vec<PathBuf>, prof: u32) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() && prof < 4 {
                recorrer(&p, v, prof + 1);
            } else if p.extension().is_some_and(|x| x == "lua") && p.file_name().is_some_and(|n| n != "morgan.lua") {
                v.push(p);
            }
        }
    }
    let mut v = Vec::new();
    recorrer(hypr, &mut v, 0);
    v.sort();
    v
}

fn lineas_lua(hypr: &Path) -> Vec<(PathBuf, usize, String)> {
    let mut v = Vec::new();
    for archivo in archivos_lua(hypr) {
        let Ok(texto) = std::fs::read_to_string(&archivo) else { continue };
        for (i, l) in texto.lines().enumerate() {
            if l.contains("exec_cmd") {
                v.push((archivo.clone(), i + 1, l.to_owned()));
            }
        }
    }
    v
}

#[derive(Debug, PartialEq)]
enum Lanzado {
    /// La línea entera es `hl.exec_cmd("programa …")`: se puede comentar.
    Solo(String),
    /// Lanza varias cosas o está dentro de otra expresión: las palabras que aparecen.
    Mezcla(Vec<String>),
}

/// Qué lanza una línea de autoarranque (`hl.exec_cmd`, no `hl.dsp.exec_cmd`, que es de un atajo).
fn clasificar(linea: &str) -> Option<Lanzado> {
    let l = linea.trim();
    if l.starts_with("--") {
        return None;
    }
    let sin_dsp = l.replace("hl.dsp.exec_cmd", "");
    let inicio = sin_dsp.find("hl.exec_cmd(")?;
    let palabras = || {
        sin_dsp[inicio..]
            .split(|c: char| !(c.is_alphanumeric() || "-_./".contains(c)))
            .filter_map(nombre)
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    // Entera: hl.exec_cmd("…") o con '…', y un ; opcional.
    let entera = l
        .strip_prefix("hl.exec_cmd(")
        .map(|r| r.trim_end().trim_end_matches(';').trim_end())
        .and_then(|r| r.strip_suffix(')'))
        .map(str::trim)
        .and_then(|r| {
            let q = r.chars().next().filter(|q| *q == '"' || *q == '\'')?;
            r.strip_prefix(q)?.strip_suffix(q).filter(|s| !s.contains(q) && !s.contains('\\'))
        });
    let Some(cmd) = entera else {
        return Some(Lanzado::Mezcla(palabras()));
    };
    if cmd.contains(['&', ';', '|', '`', '$', '(']) {
        return Some(Lanzado::Mezcla(palabras()));
    }
    let mut t: Vec<&str> = cmd.split_whitespace().collect();
    // uwsm app -- programa, uwsm-app -- programa, exec programa.
    if t.first() == Some(&"uwsm") && t.get(1) == Some(&"app") {
        t.drain(..2);
    } else if t.first() == Some(&"uwsm-app") || t.first() == Some(&"exec") {
        t.remove(0);
    }
    if t.first() == Some(&"--") {
        t.remove(0);
    }
    let prog = t.first().and_then(|p| nombre(p))?;
    Some(Lanzado::Solo(prog.to_owned()))
}

/// El texto con la línea `n` (desde 1) comentada con la marca, con su sangría.
fn comentar(texto: &str, n: usize) -> Option<String> {
    let mut lineas: Vec<String> = texto.lines().map(str::to_owned).collect();
    let l = lineas.get_mut(n.checked_sub(1)?)?;
    if l.trim_start().starts_with(MARCA) {
        return None;
    }
    let sangria = l.len() - l.trim_start().len();
    *l = format!("{}{MARCA}{}", &l[..sangria], &l[sangria..]);
    let mut t = lineas.join("\n");
    if texto.ends_with('\n') {
        t.push('\n');
    }
    Some(t)
}

/// El texto sin las marcas de `morgan setup`; `None` si no tiene ninguna.
fn descomentar(texto: &str) -> Option<String> {
    if !texto.contains(MARCA) {
        return None;
    }
    let mut t = texto
        .lines()
        .map(|l| {
            let sangria = l.len() - l.trim_start().len();
            match l[sangria..].strip_prefix(MARCA) {
                Some(resto) => format!("{}{resto}", &l[..sangria]),
                None => l.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if texto.ends_with('\n') {
        t.push('\n');
    }
    Some(t)
}

/// Comenta la línea y, la primera vez, deja una copia del archivo al lado.
fn comentar_en(archivo: &Path, n: usize) -> std::io::Result<()> {
    let texto = std::fs::read_to_string(archivo)?;
    let Some(nuevo) = comentar(&texto, n) else { return Ok(()) };
    let mut copia = archivo.as_os_str().to_owned();
    copia.push(".antes-de-morgan");
    let copia = PathBuf::from(copia);
    if !copia.exists() {
        std::fs::copy(archivo, &copia)?;
    }
    std::fs::write(archivo, nuevo)
}

// --- Autostart -------------------------------------------------------------------------------

/// Los `.desktop` de autostart vigentes: camino, si es del usuario y el programa que lanzan.
fn autostarts() -> Vec<(PathBuf, bool, String)> {
    let propios = glib::user_config_dir().join("autostart");
    let mut dirs = vec![(propios.clone(), true)];
    for d in glib::system_config_dirs() {
        dirs.push((d.join("autostart"), false));
    }
    let mut v = Vec::new();
    let mut vistos = std::collections::HashSet::new();
    for (dir, propio) in dirs {
        let mut entradas: Vec<PathBuf> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "desktop"))
            .collect();
        entradas.sort();
        for p in entradas {
            let nombre = p.file_name().unwrap_or_default().to_owned();
            // El del usuario tapa al del sistema con el mismo nombre.
            if !vistos.insert(nombre) {
                continue;
            }
            if let Some(prog) = std::fs::read_to_string(&p).ok().and_then(|t| programa_desktop(&t)) {
                v.push((p, propio, prog));
            }
        }
    }
    v
}

/// El programa que lanza un `.desktop` de autostart, salvo que esté oculto.
fn programa_desktop(texto: &str) -> Option<String> {
    let mut en_entrada = false;
    let mut exec = None;
    for l in texto.lines().map(str::trim) {
        if l.starts_with('[') {
            en_entrada = l == "[Desktop Entry]"; // no-tr
            continue;
        }
        if !en_entrada {
            continue;
        }
        if l.replace(' ', "") == "Hidden=true" {
            return None;
        }
        if let Some(e) = l.strip_prefix("Exec=") {
            let mut t = e.split_whitespace();
            let mut p = t.next()?;
            if p == "env" {
                p = t.find(|w| !w.contains('='))?;
            }
            exec = nombre(p).map(str::to_owned);
        }
    }
    exec
}

/// Uno propio se aparta con otro nombre; uno del sistema se tapa con uno oculto del usuario.
fn sacar_autostart(camino: &Path, propio: bool) -> std::io::Result<()> {
    if propio {
        let mut nuevo = camino.as_os_str().to_owned();
        nuevo.push(APARTADO);
        return std::fs::rename(camino, nuevo);
    }
    let dir = glib::user_config_dir().join("autostart");
    std::fs::create_dir_all(&dir)?;
    let nombre = camino.file_name().unwrap_or_default();
    std::fs::write(dir.join(nombre), format!("[Desktop Entry]\nType=Application\nHidden=true\n{TAPA}\n")) // no-tr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_linea_que_solo_lanza_eso_se_puede_comentar() {
        let s = |p: &str| Some(Lanzado::Solo(p.into()));
        assert_eq!(clasificar(r#"    hl.exec_cmd("waybar")"#), s("waybar"));
        assert_eq!(clasificar(r#"hl.exec_cmd('uwsm app -- swaync');"#), s("swaync"));
        assert_eq!(clasificar(r#"hl.exec_cmd("uwsm-app -- /usr/bin/hypridle -c x")"#), s("hypridle"));
        assert_eq!(clasificar(r#"hl.exec_cmd("/usr/lib/hyprpolkitagent/hyprpolkitagent")"#), s("hyprpolkitagent"));
    }

    #[test]
    fn varias_cosas_en_una_linea_van_a_mano() {
        let Some(Lanzado::Mezcla(p)) = clasificar(r#"  hl.exec_cmd("waybar & hyprpaper & firefox")"#) else {
            panic!()
        };
        assert!(p.contains(&"waybar".into()) && p.contains(&"hyprpaper".into()));
        assert!(matches!(
            clasificar(r#"hl.on("hyprland.start", function() hl.exec_cmd("mako") end)"#),
            Some(Lanzado::Mezcla(_))
        ));
    }

    #[test]
    fn un_atajo_o_un_comentario_no_cuentan() {
        assert_eq!(clasificar(r#"hl.bind("SUPER + L", hl.dsp.exec_cmd("hyprlock"))"#), None);
        assert_eq!(clasificar(r#"-- hl.exec_cmd("waybar")"#), None);
        assert_eq!(clasificar("hl.config({})"), None);
    }

    #[test]
    fn comentar_y_descomentar_dejan_el_archivo_igual() {
        let t = "hl.on(\"hyprland.start\", function()\n    hl.exec_cmd(\"waybar\")\nend)\n";
        let c = comentar(t, 2).unwrap();
        assert_eq!(c.lines().nth(1), Some("    -- [morgan setup] hl.exec_cmd(\"waybar\")"));
        assert_eq!(comentar(&c, 2), None);
        assert_eq!(clasificar(c.lines().nth(1).unwrap()), None);
        assert_eq!(descomentar(&c).as_deref(), Some(t));
        assert_eq!(descomentar(t), None);
        assert_eq!(comentar(t, 9), None);
    }

    #[test]
    fn la_unidad_sale_del_cgroup() {
        let u = |c: &str| unidad_de_cgroup(c);
        assert_eq!(
            u("0::/user.slice/user-1000.slice/user@1000.service/app.slice/waybar.service\n").as_deref(),
            Some("waybar.service")
        );
        assert_eq!(
            u("0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-Hyprland-mako@1a2b.service\n").as_deref(),
            Some("app-Hyprland-mako@1a2b.service")
        );
        assert_eq!(u("0::/user.slice/user-1000.slice/session-2.scope\n"), None);
        assert_eq!(u("0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-kitty-1.scope\n"), None);
    }

    #[test]
    fn el_programa_de_cada_unidad() {
        let s = "Id=waybar.service\nExecStart={ path=/usr/bin/waybar ; argv[]=/usr/bin/waybar ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n\nId=algo.service\nExecStart=\n";
        assert_eq!(programas_de_show(s), vec![("waybar.service".to_owned(), "waybar".to_owned())]);
    }

    #[test]
    fn un_desktop_oculto_no_arranca() {
        assert_eq!(programa_desktop("[Desktop Entry]\nExec=/usr/bin/mako --x\n").as_deref(), Some("mako"));
        assert_eq!(programa_desktop("[Desktop Entry]\nExec=env A=1 waybar\n").as_deref(), Some("waybar"));
        assert_eq!(programa_desktop("[Desktop Entry]\nExec=waybar\nHidden=true\n"), None);
        assert_eq!(programa_desktop("[Desktop Action x]\nExec=waybar\n"), None);
    }
}
