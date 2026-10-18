//! Ninguna crea la clave si falta: escribir una clave nueva en el lugar equivocado de un
//! `.ini` (fuera de su sección) es peor que no tocarla.

use std::path::Path;

pub fn ini(texto: &str, clave: &str, valor: &str) -> Option<String> {
    reemplazar_linea(texto, |l| {
        l.strip_prefix(clave)
            .and_then(|r| r.strip_prefix('='))
            .map(|_| format!("{clave}={valor}"))
    })
}

pub fn ini_fuente(texto: &str, familia: &str) -> Option<String> {
    reemplazar_linea(texto, |l| {
        let viejo = l.strip_prefix("gtk-font-name=")?;
        Some(format!("gtk-font-name={}", con_tamano(familia, viejo)))
    })
}

/// Que Qt5 y Qt6 serialicen distinta cantidad de campos da igual: cambia solo el primero.
pub fn qt(texto: &str, clave: &str, familia: &str) -> Option<String> {
    reemplazar_linea(texto, |l| {
        let resto = l.strip_prefix(clave)?.strip_prefix("=\"")?;
        let coma = resto.find(',')?;
        Some(format!("{clave}=\"{familia}{}", &resto[coma..]))
    })
}

pub fn kitty_familia(texto: &str) -> Option<String> {
    let l = texto.lines().find_map(|l| linea_kitty(l))?;
    match l.split_once("family=\"") {
        Some((_, r)) => r.split('"').next().map(str::to_owned),
        None => Some(l.trim().to_owned()),
    }
}

/// `bold_font` y compañía quedan en `auto`, que sigue a esta.
pub fn kitty(texto: &str, familia: &str) -> Option<String> {
    reemplazar_linea(texto, |l| {
        let valor = linea_kitty(l)?;
        Some(match valor.split_once("family=\"") {
            Some((antes, r)) => {
                let despues = r.split_once('"').map_or("", |(_, d)| d);
                format!("font_family      {antes}family=\"{familia}\"{despues}")
            }
            None => format!("font_family      family=\"{familia}\""),
        })
    })
}

fn linea_kitty(l: &str) -> Option<&str> {
    let r = l.trim_start().strip_prefix("font_family")?;
    r.starts_with(char::is_whitespace).then(|| r.trim_start())
}

pub fn env_leer(texto: &str, clave: &str) -> Option<String> {
    texto.lines().find_map(|l| {
        let r = l.strip_prefix("export ")?.strip_prefix(clave)?.strip_prefix('=')?;
        Some(r.trim_matches('"').to_owned())
    })
}

/// Las que no están no se agregan: el archivo es config escrita a mano.
pub fn env(texto: &str, pares: &[(&str, &str)]) -> Option<String> {
    let mut alguna = false;
    let mut salida = String::with_capacity(texto.len());
    for l in texto.split_inclusive('\n') {
        let sin_fin = l.trim_end_matches('\n');
        let nueva = pares.iter().find_map(|(k, v)| {
            let r = sin_fin.strip_prefix("export ")?.strip_prefix(k)?;
            r.starts_with('=').then(|| format!("export {k}=\"{v}\""))
        });
        match nueva {
            Some(n) => {
                alguna = true;
                salida.push_str(&n);
                if l.ends_with('\n') {
                    salida.push('\n');
                }
            }
            None => salida.push_str(l),
        }
    }
    alguna.then_some(salida)
}

pub fn sin_tamano(valor: &str) -> &str {
    match valor.rsplit_once(' ') {
        Some((fam, t)) if es_numero(t) => fam,
        _ => valor,
    }
}

pub fn con_tamano(familia: &str, viejo: &str) -> String {
    match viejo.rsplit_once(' ') {
        Some((_, t)) if es_numero(t) => format!("{familia} {t}"),
        _ => familia.to_owned(),
    }
}

fn es_numero(t: &str) -> bool {
    !t.is_empty()
        && t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',')
        && t.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn reemplazar_linea(texto: &str, f: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut hecho = false;
    let mut salida = String::with_capacity(texto.len());
    for l in texto.split_inclusive('\n') {
        let sin_fin = l.trim_end_matches('\n');
        match (!hecho).then(|| f(sin_fin)).flatten() {
            Some(n) => {
                hecho = true;
                salida.push_str(&n);
                if l.ends_with('\n') {
                    salida.push('\n');
                }
            }
            None => salida.push_str(l),
        }
    }
    hecho.then_some(salida)
}

/// Escribe sobre el mismo archivo: conserva permisos, dueño y, si lo es, el symlink.
pub fn reescribir(ruta: &Path, f: impl Fn(&str) -> Option<String>) {
    let Ok(texto) = std::fs::read_to_string(ruta) else {
        return;
    };
    match f(&texto) {
        Some(nuevo) if nuevo != texto => {
            if super::seco() {
                eprintln!("morgan: apariencia (seco): reescribiría {}", ruta.display());
            } else if let Err(e) = std::fs::write(ruta, nuevo) {
                eprintln!("morgan: apariencia: {}: {e}", ruta.display());
            }
        }
        Some(_) => {}
        None => eprintln!(
            "morgan: apariencia: no encontré la clave en {}, lo dejo como estaba",
            ruta.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_de_gtk() {
        let t = "[Settings]\ngtk-font-name=Adwaita Sans 11\ngtk-icon-theme-name=breeze\n";
        assert_eq!(
            ini_fuente(t, "Inter").unwrap(),
            "[Settings]\ngtk-font-name=Inter 11\ngtk-icon-theme-name=breeze\n"
        );
        assert!(ini(t, "gtk-icon-theme-name", "Papirus-Dark").unwrap().contains("=Papirus-Dark\n"));
        assert_eq!(ini(t, "gtk-cursor-theme-name", "X"), None);
        // Una clave que empieza igual no es la clave.
        assert_eq!(ini("gtk-font-name-extra=1\n", "gtk-font-name", "x"), None);
    }

    #[test]
    fn qt_cambia_solo_la_familia() {
        let t = "[Fonts]\nfixed=\"Mono,12,-1,5,400,0,0,0,0,0\"\ngeneral=\"Sans,11,-1,5,50\"\n";
        assert_eq!(
            qt(t, "general", "Inter").unwrap(),
            "[Fonts]\nfixed=\"Mono,12,-1,5,400,0,0,0,0,0\"\ngeneral=\"Inter,11,-1,5,50\"\n"
        );
        assert!(qt(t, "fixed", "JetBrains Mono").unwrap().contains("fixed=\"JetBrains Mono,12,"));
    }

    #[test]
    fn kitty_conserva_las_otras_claves() {
        let t = "# x\nfont_family      family=\"JetBrains Mono\" features=\"+zero\"\nfont_size 12\n";
        assert_eq!(kitty_familia(t).as_deref(), Some("JetBrains Mono"));
        let n = kitty(t, "Fira Code").unwrap();
        assert!(n.contains("font_family      family=\"Fira Code\" features=\"+zero\"\n"));
        assert!(n.ends_with("font_size 12\n"));
        assert_eq!(kitty_familia("font_family Iosevka\n").as_deref(), Some("Iosevka"));
        assert_eq!(kitty("font_family Iosevka\n", "X").unwrap(), "font_family      family=\"X\"\n");
        assert_eq!(kitty("font_familyx 1\n", "X"), None);
    }

    #[test]
    fn env_de_uwsm() {
        let t = "export XCURSOR_THEME=\"Adwaita\"\nexport XCURSOR_SIZE=\"24\"\nexport OTRA=1\n";
        assert_eq!(env_leer(t, "XCURSOR_SIZE").as_deref(), Some("24"));
        let n = env(t, &[("XCURSOR_THEME", "Bibata"), ("HYPRCURSOR_SIZE", "32")]).unwrap();
        assert_eq!(n, "export XCURSOR_THEME=\"Bibata\"\nexport XCURSOR_SIZE=\"24\"\nexport OTRA=1\n");
        assert_eq!(env("export OTRA=1\n", &[("XCURSOR_THEME", "x")]), None);
    }

    #[test]
    fn tamanos() {
        assert_eq!(sin_tamano("Adwaita Sans 11"), "Adwaita Sans");
        assert_eq!(sin_tamano("Cantarell Bold 10.5"), "Cantarell Bold");
        assert_eq!(sin_tamano("Inter"), "Inter");
        assert_eq!(con_tamano("Inter", "Adwaita Sans 11"), "Inter 11");
        assert_eq!(con_tamano("Inter", "Adwaita Sans"), "Inter");
    }
}
