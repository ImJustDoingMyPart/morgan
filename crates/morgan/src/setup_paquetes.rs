//! `morgan setup`, lo recomendado: los programas con los que se armó y se probó morgan (los
//! `optdepends` del paquete) y los servicios del sistema que usa. Lo que se instala sin riesgo va
//! a una lista para elegir; lo que puede dejar el equipo sin red, sin audio o con dos programas
//! peleándose por la CPU se explica con los pasos para hacerlo a mano.

use std::process::Command;

use morgan_idioma::{t, tr, tr_const, trf};

/// Un paquete recomendado y para qué lo usa morgan.
pub struct Recomendado {
    pub paquete: &'static str,
    pub para: &'static str,
}

const fn r(paquete: &'static str, para: &'static str) -> Recomendado {
    Recomendado { paquete, para }
}

/// Los `optdepends` de `dist/arch/*/PKGBUILD`, más los íconos y el tema de sonidos que esperan.
pub const RECOMENDADOS: &[Recomendado] = &[
    r("ttf-jetbrains-mono-nerd", tr_const!("la fuente de la interfaz y sus íconos")), // no-tr
    r("matugen", tr_const!("temas")), // no-tr
    r("awww", tr_const!("fondos de pantalla")), // no-tr
    r("libvips", tr_const!("miniaturas del selector de fondos")), // no-tr
    r("hyprsunset", tr_const!("luz nocturna")), // no-tr
    r("ddcutil", tr_const!("brillo de monitores externos")), // no-tr
    r("hyprshot", tr_const!("capturas de pantalla")), // no-tr
    r("cliphist", tr_const!("historial del portapapeles")), // no-tr
    r("wtype", tr_const!("escribir símbolos y emoji desde el launcher")), // no-tr
    r("libqalculate", tr_const!("calculadora")), // no-tr
    r("cava", tr_const!("ecualizador del reproductor")), // no-tr
    r("libcanberra", tr_const!("sonidos del sistema")), // no-tr
    r("sound-theme-freedesktop", tr_const!("el tema de sonidos de base")), // no-tr
    r("uwsm", tr_const!("lanzar las apps como unidades de systemd")), // no-tr
    r("gpu-screen-recorder-ui", tr_const!("grabar la pantalla, con el contador en la barra")), // no-tr
    r("papirus-icon-theme", tr_const!("íconos")), // no-tr
    r("papirus-folders", tr_const!("color de las carpetas según el tema")), // no-tr
    r("xdg-desktop-portal-gtk", tr_const!("que navegadores y apps de Electron sigan el modo claro u oscuro")), // no-tr
];

/// Los que no están en los repos oficiales de Arch: van por el ayudante de AUR.
const DEL_AUR: &[&str] = &["papirus-folders"]; // no-tr

/// Algo para instalar: el texto de la lista, los paquetes y el servicio del sistema a prender.
pub struct Item {
    pub texto: String,
    pub paquetes: Vec<&'static str>,
    pub servicio: Option<&'static str>,
}

/// Lo que se puede instalar y los pasos que quedan a mano.
pub struct Plan {
    pub items: Vec<Item>,
    pub a_mano: Vec<String>,
}

pub fn armar() -> Plan {
    let mut plan = Plan { items: Vec::new(), a_mano: Vec::new() };
    let hay_pacman = glib::find_program_in_path("pacman").is_some();
    let faltan = faltantes(RECOMENDADOS.iter().map(|r| r.paquete));
    for rec in RECOMENDADOS.iter().filter(|r| faltan.contains(&r.paquete)) {
        plan.items.push(Item {
            texto: format!("{} ({})", rec.paquete, t(rec.para)), // no-tr
            paquetes: vec![rec.paquete],
            servicio: None,
        });
    }
    bluetooth(&mut plan);
    energia(&mut plan);
    red(&mut plan);
    audio(&mut plan);
    if !hay_pacman && !plan.items.is_empty() {
        let lista: Vec<&str> = plan.items.iter().flat_map(|i| i.paquetes.iter().copied()).collect();
        plan.a_mano.insert(0, trf!("Instalá con el gestor de paquetes de tu distribución: {}", lista.join(" ")));
        plan.items.clear();
    }
    plan
}

/// Instala lo elegido: los paquetes oficiales en una sola llamada a pacman, los del AUR con el
/// ayudante que haya, y prende los servicios.
pub fn instalar(items: &[&Item], seco: bool) -> Vec<String> {
    let mut a_mano = Vec::new();
    let paquetes: Vec<&str> = items.iter().flat_map(|i| i.paquetes.iter().copied()).collect();
    let (aur, oficiales): (Vec<&str>, Vec<&str>) = paquetes.iter().partition(|p| DEL_AUR.contains(p));
    let correr = |cmd: &[&str]| {
        println!("  {}: {}", if seco { tr!("haría") } else { tr!("corriendo") }, cmd.join(" "));
        seco || Command::new(cmd[0]).args(&cmd[1..]).status().is_ok_and(|s| s.success())
    };
    let mut ok_oficiales = true;
    if !oficiales.is_empty() {
        let mut cmd = vec!["sudo", "pacman", "-S", "--needed"]; // no-tr
        cmd.extend(&oficiales);
        ok_oficiales = correr(&cmd);
        if !ok_oficiales {
            a_mano.push(trf!("pacman no terminó. Probá de nuevo con: sudo pacman -S --needed {}", oficiales.join(" ")));
        }
    }
    if !aur.is_empty() {
        match ["paru", "yay"].into_iter().find(|h| glib::find_program_in_path(h).is_some()) {
            Some(ayudante) => {
                let mut cmd = vec![ayudante, "-S", "--needed"];
                cmd.extend(&aur);
                if !correr(&cmd) {
                    a_mano.push(trf!("{} no terminó. Probá de nuevo con: {} -S --needed {}", ayudante, ayudante, aur.join(" ")));
                }
            }
            None => {
                for p in &aur {
                    a_mano.push(trf!(
                        "{} está en el AUR. Con un ayudante (yay o paru): yay -S {}. Sin ayudante: git clone https://aur.archlinux.org/{}.git && cd {} && makepkg -si",
                        p,
                        p,
                        p,
                        p
                    ));
                }
            }
        }
    }
    for s in items.iter().filter_map(|i| i.servicio) {
        if ok_oficiales && !correr(&["sudo", "systemctl", "enable", "--now", s]) {
            a_mano.push(trf!("No se pudo prender {}: sudo systemctl enable --now {}", s, s));
        }
    }
    a_mano
}

/// De estos paquetes, los que no están instalados (ni algo que los provea): `pacman -T`.
fn faltantes<'a>(paquetes: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let paquetes: Vec<&str> = paquetes.collect();
    let Ok(o) = Command::new("pacman").arg("-T").args(&paquetes).output() else {
        return Vec::new();
    };
    let salida = String::from_utf8_lossy(&o.stdout);
    paquetes.into_iter().filter(|p| salida.lines().any(|l| l.trim() == *p)).collect()
}

fn activo(servicio: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", servicio])
        .status()
        .is_ok_and(|s| s.success())
}

fn habilitado(servicio: &str) -> bool {
    Command::new("systemctl")
        .args(["is-enabled", "--quiet", servicio])
        .status()
        .is_ok_and(|s| s.success())
}

/// Con un adaptador de Bluetooth: bluez y su servicio. Prenderlo no cambia nada más.
fn bluetooth(plan: &mut Plan) {
    let hay_adaptador = std::fs::read_dir("/sys/class/bluetooth").is_ok_and(|mut d| d.next().is_some());
    if !hay_adaptador || activo("bluetooth.service") {
        return;
    }
    plan.items.push(Item {
        texto: tr!("Bluetooth: bluez y su servicio").to_owned(),
        paquetes: faltantes(["bluez"].into_iter()), // no-tr
        servicio: Some("bluetooth.service"), // no-tr
    });
}

/// Lo que maneja la energía además de power-profiles-daemon: el servicio y los pasos para
/// dejarlo, sin desinstalarlo, por si se quiere volver.
const OTROS_ENERGIA: &[(&str, &str)] = &[
    ("tlp.service", "sudo systemctl disable --now tlp.service"), // no-tr
    ("auto-cpufreq.service", "sudo systemctl disable --now auto-cpufreq.service"), // no-tr
    ("tuned.service", "sudo systemctl disable --now tuned.service"), // no-tr
    ("system76-power.service", "sudo systemctl disable --now system76-power.service"), // no-tr
];

/// power-profiles-daemon o algo que da lo mismo (tuned-ppd). Si otro maneja la energía, a mano:
/// dos a la vez se pisan los ajustes del procesador.
fn energia(plan: &mut Plan) {
    if activo("power-profiles-daemon.service") || activo("tuned-ppd.service") {
        return;
    }
    let otros: Vec<&(&str, &str)> = OTROS_ENERGIA.iter().filter(|(s, _)| activo(s) || habilitado(s)).collect();
    if otros.is_empty() {
        plan.items.push(Item {
            texto: tr!("Perfiles de energía: power-profiles-daemon").to_owned(),
            paquetes: faltantes(["power-profiles-daemon"].into_iter()), // no-tr
            servicio: Some("power-profiles-daemon.service"), // no-tr
        });
        return;
    }
    let nombres: Vec<&str> = otros.iter().map(|(s, _)| s.trim_end_matches(".service")).collect();
    let mut pasos: Vec<&str> = otros.iter().map(|(_, p)| *p).collect();
    pasos.push("sudo pacman -S --needed power-profiles-daemon"); // no-tr
    pasos.push("sudo systemctl enable --now power-profiles-daemon.service"); // no-tr
    plan.a_mano.push(trf!(
        "La energía la maneja {}. La tarjeta de energía de morgan usa power-profiles-daemon, y los dos juntos se pisan los ajustes del procesador. Para cambiar (lo de antes queda instalado, por si querés volver):\n{}",
        nombres.join(", "),
        sangrar(&pasos)
    ));
}

/// Lo que maneja la red además de NetworkManager. wpa_supplicant no: NetworkManager lo usa, y
/// deshabilitarlo le saca el alias de D-Bus por el que lo arranca.
const OTROS_RED: &[&str] = &["iwd.service", "systemd-networkd.service", "connman.service", "dhcpcd.service", "netctl.service"]; // no-tr

/// NetworkManager nunca se instala solo: cambiar quién maneja la red puede cortarla en el medio.
fn red(plan: &mut Plan) {
    if activo("NetworkManager.service") {
        return;
    }
    let otros: Vec<&str> = OTROS_RED.iter().copied().filter(|s| activo(s)).collect();
    let mut pasos = vec!["sudo pacman -S --needed networkmanager"]; // no-tr
    let parar = format!("sudo systemctl disable --now {}", otros.join(" ")); // no-tr
    if !otros.is_empty() {
        pasos.push(&parar);
    }
    pasos.push("sudo systemctl enable --now NetworkManager.service"); // no-tr
    pasos.push("nmtui"); // no-tr
    let quien = if otros.is_empty() {
        tr!("otro programa").to_owned()
    } else {
        otros.iter().map(|s| s.trim_end_matches(".service")).collect::<Vec<_>>().join(", ")
    };
    plan.a_mano.push(trf!(
        "La red la maneja {}. La tarjeta de red de morgan usa NetworkManager. Para cambiar, mejor con el cable enchufado, por si la Wi-Fi se corta en el medio (nmtui, al final, sirve para elegir la Wi-Fi y poner la clave):\n{}",
        quien,
        sangrar(&pasos)
    ));
}

/// El audio es de PipeWire (con pipewire-pulse) o se explica cómo pasarlo: cambiar el servidor
/// de sonido con la sesión abierta deja las apps sin audio hasta volver a entrar.
fn audio(plan: &mut Plan) {
    let info = Command::new("pactl").arg("info").output().ok();
    let servidor = info
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("Server Name:").map(|v| v.trim().to_owned()))); // no-tr
    if servidor.as_deref().is_some_and(|s| s.contains("PipeWire")) {
        return;
    }
    let pasos = [
        "sudo pacman -S --needed pipewire-pulse wireplumber", // no-tr
        "systemctl --user enable --now pipewire-pulse.socket wireplumber.service", // no-tr
    ];
    let quien = servidor.unwrap_or_else(|| tr!("ningún servidor de sonido").to_owned());
    plan.a_mano.push(trf!(
        "El audio lo maneja {}. El volumen, el audio por app y el micrófono de morgan usan PipeWire. Para cambiar (si pacman ofrece quitar pulseaudio, aceptá), y después cerrá la sesión y volvé a entrar:\n{}",
        quien,
        sangrar(&pasos)
    ));
}

fn sangrar(pasos: &[&str]) -> String {
    pasos.iter().map(|p| format!("      {p}")).collect::<Vec<_>>().join("\n")
}
