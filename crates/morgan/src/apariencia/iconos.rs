//! Tela-circle trae un tema completo por color; en Papirus el color lo pinta `papirus-folders`
//! reescribiendo los SVG de carpeta. No hay estado propio: el paquete puesto sale de gsettings.

use std::path::PathBuf;

use gio::prelude::*;

use super::archivos::{self, reescribir};
use super::cursores::bases;

/// Sin los neutros ni las paletas de marca de Tela (black, grey, dracula, nord…), que siguen
/// elegibles de nombre pero no deben ganar por un fondo gris. Los hex son el `fill` real de
/// cada `folder.svg` instalado.
const TELA: &[(&str, &str)] = &[
    ("red", "#ef5350"),
    ("orange", "#ff9800"),
    ("yellow", "#ffca28"),
    ("green", "#66bb6a"),
    ("blue", "#5677fc"),
    ("purple", "#7e57c2"),
    ("pink", "#f06292"),
    ("brown", "#795548"),
];
/// El segundo es el nombre que entiende `papirus-folders` (su paleta dice "violet", no
/// "purple").
const PAPIRUS: &[(&str, &str, &str)] = &[
    ("red", "#e25252", "red"),
    ("orange", "#ee923a", "orange"),
    ("yellow", "#f9bd30", "yellow"),
    ("green", "#87b158", "green"),
    ("blue", "#5294e2", "blue"),
    ("purple", "#7e57c2", "violet"),
    ("pink", "#f06292", "pink"),
    ("brown", "#ae8e6c", "brown"),
];

const TELA_FAMILIA: &str = "Tela-circle";
const PAPIRUS_FAMILIA: &str = "Papirus";
const PAPIRUS_TEMA: &str = "Papirus-Dark";

fn hsl(hex: &str) -> Option<(f64, f64, f64)> {
    let h = hex.strip_prefix('#')?;
    let c = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok().map(|v| f64::from(v) / 255.0);
    let (r, g, b) = (c(0)?, c(2)?, c(4)?);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min {
        return Some((0.0, 0.0, l));
    }
    let d = max - min;
    let s = if l <= 0.5 { d / (max + min) } else { d / (2.0 - max - min) };
    let rc = (max - r) / d;
    let gc = (max - g) / d;
    let bc = (max - b) / d;
    let h = if r == max {
        bc - gc
    } else if g == max {
        2.0 + rc - bc
    } else {
        4.0 + gc - rc
    };
    Some((((h / 6.0).rem_euclid(1.0)) * 360.0, s, l))
}

/// El matiz pesa mucho más que la saturación y la luminosidad: el primary del tema sale
/// pálido u oscuro según el modo, y si la luminosidad pesara igual, uno oscuro terminaría en
/// "brown" con cualquier fondo.
fn mas_parecido<'a>(objetivo: &str, opciones: impl Iterator<Item = (&'a str, &'a str)>) -> Option<&'a str> {
    let (th, ts, tl) = hsl(objetivo)?;
    opciones
        .filter_map(|(nombre, hex)| {
            let (h, s, l) = hsl(hex)?;
            let dh = (th - h).abs();
            let dh = dh.min(360.0 - dh) / 180.0;
            Some((nombre, dh + (ts - s).abs() * 0.35 + (tl - l).abs() * 0.15))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(n, _)| n)
}

/// Lo trae la hoja de colores de morgan, que todo tema renderiza.
fn primary() -> Option<String> {
    let t = std::fs::read_to_string(glib::user_config_dir().join("morgan/theme.css")).ok()?;
    t.lines().find_map(|l| {
        let hex = l.strip_prefix("@define-color primary ")?.strip_suffix(';')?;
        (hex.len() == 7 && hex.starts_with('#')).then(|| hex.to_owned())
    })
}

fn interfaz() -> gio::Settings {
    gio::Settings::new("org.gnome.desktop.interface")
}

fn tema_puesto() -> String {
    interfaz().string("icon-theme").into()
}

pub fn actual() -> String {
    let t = tema_puesto();
    if t.starts_with(TELA_FAMILIA) {
        TELA_FAMILIA.into()
    } else if t.starts_with(PAPIRUS_FAMILIA) {
        PAPIRUS_FAMILIA.into()
    } else {
        t
    }
}

fn instalado(tema: &str) -> bool {
    bases().iter().any(|b| b.join(tema).join("index.theme").is_file())
}

pub fn listar() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for b in bases() {
        let Ok(dir) = std::fs::read_dir(&b) else { continue };
        for e in dir.flatten() {
            let Some(nombre) = e.file_name().to_str().map(str::to_owned) else { continue };
            let Ok(index) = std::fs::read_to_string(e.path().join("index.theme")) else {
                continue;
            };
            if !index.lines().any(|l| l.starts_with("Directories="))
                || index.lines().any(|l| l.trim() == "Hidden=true")
            {
                continue;
            }
            let familia = if nombre.starts_with(TELA_FAMILIA) {
                TELA_FAMILIA.to_owned()
            } else if nombre.starts_with(PAPIRUS_FAMILIA) {
                PAPIRUS_FAMILIA.to_owned()
            } else if ["hicolor", "locolor", "default"].contains(&nombre.as_str()) {
                continue;
            } else {
                nombre
            };
            v.push(familia);
        }
    }
    v.sort();
    v.dedup();
    // Los dos con color automático, primero.
    for f in [PAPIRUS_FAMILIA, TELA_FAMILIA] {
        if let Some(i) = v.iter().position(|x| x == f) {
            let x = v.remove(i);
            v.insert(0, x);
        }
    }
    v
}

pub fn tiene_variantes(familia: &str) -> bool {
    familia == TELA_FAMILIA || familia == PAPIRUS_FAMILIA
}

/// Solo si cambia: cada cambio hace que todas las apps GTK recarguen sus íconos, y esto corre
/// en cada cambio de tema.
fn poner_tema(tema: &str) {
    if tema_puesto() == tema {
        return;
    }
    if let Err(e) = interfaz().set_string("icon-theme", tema) {
        eprintln!("morgan: apariencia: gsettings icon-theme: {e}");
    }
    for ini in ["gtk-3.0/settings.ini", "gtk-4.0/settings.ini"] {
        let ruta = glib::user_config_dir().join(ini);
        reescribir(&ruta, |t| archivos::ini(t, "gtk-icon-theme-name", tema));
    }
}

pub fn aplicar(familia: &str) -> Result<(), String> {
    if !listar().iter().any(|f| f == familia) {
        return Err(format!("no existe el paquete de íconos «{familia}»"));
    }
    let hex = primary();
    match familia {
        TELA_FAMILIA => {
            let color = hex.as_deref().and_then(|h| mas_parecido(h, TELA.iter().copied()));
            let tema = color
                .map(|c| format!("{TELA_FAMILIA}-{c}-dark"))
                .filter(|t| instalado(t))
                .unwrap_or_else(|| format!("{TELA_FAMILIA}-dark"));
            poner_tema(&tema);
        }
        PAPIRUS_FAMILIA => {
            poner_tema(PAPIRUS_TEMA);
            if let Some(h) = hex {
                pintar_papirus(&h);
            }
        }
        otro => poner_tema(otro),
    }
    Ok(())
}

pub fn resincronizar() {
    let f = actual();
    if tiene_variantes(&f)
        && let Err(e) = aplicar(&f)
    {
        eprintln!("morgan: apariencia: {e}");
    }
}

fn papirus_local() -> PathBuf {
    glib::user_data_dir().join("icons")
}

/// En otro hilo y sin esperar: `papirus-folders` tarda segundos, casi todo en
/// `gtk-update-icon-cache` (la 1.14.0 no deja pedir la versión rápida). Si las carpetas ya
/// tienen ese color, nada.
///
/// `papirus-folders` escribe sobre el directorio del tema, y el del paquete es de root: se
/// copia el tema a `~/.local/share/icons`, que la spec de íconos busca antes que `/usr/share`.
fn pintar_papirus(hex: &str) {
    let Some(color) = mas_parecido(hex, PAPIRUS.iter().map(|(n, h, _)| (*n, *h)))
        .and_then(|n| PAPIRUS.iter().find(|p| p.0 == n))
        .map(|p| p.2)
    else {
        return;
    };
    let carpeta = papirus_local().join(PAPIRUS_TEMA).join("48x48/places/folder.svg");
    if std::fs::read_link(&carpeta).is_ok_and(|d| d == std::path::Path::new(&format!("folder-{color}.svg"))) {
        return;
    }
    if super::seco() {
        eprintln!("morgan: apariencia (seco): pintaría Papirus de {color}");
        return;
    }
    std::thread::spawn(move || {
        let local = papirus_local();
        for variante in ["Papirus", "Papirus-Dark", "Papirus-Light"] {
            let (src, dst) = (PathBuf::from("/usr/share/icons").join(variante), local.join(variante));
            if src.is_dir() && !dst.exists() {
                let _ = std::fs::create_dir_all(&local);
                let _ = std::process::Command::new("cp").arg("-r").arg(&src).arg(&dst).status();
            }
        }
        let _ = std::process::Command::new("papirus-folders")
            .args(["-t", PAPIRUS_TEMA, "-C", color, "-o"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsl_como_colorsys() {
        let (h, s, l) = hsl("#ff0000").unwrap();
        assert!((h - 0.0).abs() < 1e-9 && (s - 1.0).abs() < 1e-9 && (l - 0.5).abs() < 1e-9);
        let (h, _, _) = hsl("#0000ff").unwrap();
        assert!((h - 240.0).abs() < 1e-9);
        let (h, s, l) = hsl("#808080").unwrap();
        assert_eq!((h, s), (0.0, 0.0));
        assert!((l - 128.0 / 255.0).abs() < 1e-9);
        assert_eq!(hsl("rojo"), None);
    }

    #[test]
    fn gana_el_matiz() {
        // Un primary azul pálido es azul, aunque su luminosidad esté lejos del swatch.
        assert_eq!(mas_parecido("#aac4ff", TELA.iter().copied()), Some("blue"));
        // Un rojo oscuro es rojo, no marrón.
        assert_eq!(mas_parecido("#8b1a1a", TELA.iter().copied()), Some("red"));
        assert_eq!(mas_parecido("#ffb86c", PAPIRUS.iter().map(|(n, h, _)| (*n, *h))), Some("orange"));
    }
}
