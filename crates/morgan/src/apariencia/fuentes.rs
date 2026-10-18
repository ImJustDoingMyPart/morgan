//! No toca la fuente de morgan, que declara la suya en su hoja: sus íconos son glifos Nerd
//! Font. El tamaño no se toca nunca: se cambia la familia y nada más.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use gtk::gio;
use gtk::prelude::*;

use super::archivos::{self, reescribir};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ambito {
    Sistema,
    Terminal,
}

fn config(rel: &str) -> PathBuf {
    gtk::glib::user_config_dir().join(rel)
}

/// Una sola llamada para las dos listas (hay cientos de familias instaladas). `:lang=es`
/// descarta de un saque las escrituras que no se usan, las de símbolos sin letras y los alias
/// de peso de las Nerd Fonts. Se piden `weight` y `slant` y no `style` porque son números: el
/// nombre del estilo viene traducido según el locale.
pub fn fc_list() -> String {
    std::process::Command::new("fc-list")
        .args([":lang=es", "--format", "%{family[0]}\t%{spacing}\t%{weight}\t%{slant}\n"])
        .stderr(std::process::Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Las palabras que Pango se come al final de una font description como atributo.
/// Verificado con `Pango.FontDescription.from_string`.
const PALABRAS_PANGO: &[&str] = &[
    "normal", "roman", "oblique", "italic", "thin", "light", "book", "regular", "medium", "bold",
    "heavy", "black", "condensed", "expanded", "small-caps", "unicase", "south", "north", "east",
    "west",
];

/// - `%{spacing}` separa las listas: vacío en las proporcionales y 100 en las monoespaciadas
///   (vacío no es 0: filtrar por `:spacing=0` no devuelve nada).
/// - Afuera las CJK (cubren el español por traer el latino, pero nadie las quiere de interfaz),
///   y del lado proporcional las `Nerd Font`, `Mono` y `Math`, que son de terminal.
/// - En `sistema`, afuera la familia "impronunciable": la fuente de sistema se guarda como font
///   description de Pango, que se come la última palabra si es una de sus keywords
///   ("Times New Roman 11" es la familia "Times New", que no existe, y GTK cae en silencio al
///   sans genérico). Se descarta si además el tronco no está instalado ("Arial Black" sigue:
///   "Arial" existe y da la cara correcta). En `terminal` no aplica: kitty recibe la familia
///   en su propio campo.
pub fn candidatas(fc: &str, ambito: Ambito) -> Vec<(String, String)> {
    let mut instaladas: HashSet<&str> = HashSet::new();
    // familia → (variantes, negrita, itálica)
    let mut datos: BTreeMap<&str, (u32, bool, bool)> = BTreeMap::new();
    for l in fc.lines() {
        let mut c = l.split('\t');
        let (Some(fam), Some(sp)) = (c.next(), c.next()) else {
            continue;
        };
        if fam.is_empty() {
            continue;
        }
        instaladas.insert(fam);
        if fam.contains("CJK") || fam.contains("WenQuanYi") {
            continue;
        }
        let mono = sp == "100";
        let entra = match ambito {
            Ambito::Terminal => mono,
            Ambito::Sistema => {
                !mono && !["Nerd Font", "Mono", "Math"].iter().any(|x| fam.contains(x))
            }
        };
        if !entra {
            continue;
        }
        let peso: f64 = c.next().and_then(|w| w.parse().ok()).unwrap_or(0.0);
        let inclinacion: f64 = c.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let d = datos.entry(fam).or_default();
        d.0 += 1;
        // En la escala de fontconfig, Regular es 80 y Bold 200; slant 0 es redonda.
        d.1 |= peso >= 200.0;
        d.2 |= inclinacion > 0.0;
    }
    let impronunciable = |f: &str| {
        let Some((tronco, ultima)) = f.rsplit_once(char::is_whitespace) else {
            return false;
        };
        PALABRAS_PANGO.contains(&ultima.to_lowercase().as_str()) && !instaladas.contains(tronco)
    };
    let mut v: Vec<(String, String)> = datos
        .into_iter()
        .filter(|(f, _)| ambito == Ambito::Terminal || !impronunciable(f))
        .map(|(f, (n, negrita, italica))| {
            let mut sub = format!("{n} variante{}", if n == 1 { "" } else { "s" });
            sub.push_str(match (negrita, italica) {
                (true, true) => " · negrita e itálica",
                (true, false) => " · solo negrita",
                (false, true) => " · solo itálica",
                (false, false) => " · sin negrita ni itálica",
            });
            // "Mono" mete cada glifo de ícono en una celda; la otra los deja ocupar dos.
            if f.ends_with("Nerd Font Mono") {
                sub.push_str(" · Nerd Font, íconos de una celda");
            } else if f.ends_with("Nerd Font") {
                sub.push_str(" · Nerd Font, íconos anchos");
            }
            (f.to_owned(), sub)
        })
        .collect();
    v.sort_by_key(|(f, _)| f.to_lowercase());
    v
}

fn interfaz() -> gio::Settings {
    gio::Settings::new("org.gnome.desktop.interface")
}

/// De la fuente real (gsettings, kitty.conf), no de un estado propio que se desincronizaría
/// en cuanto alguien edite la config a mano.
pub fn actual(ambito: Ambito) -> String {
    match ambito {
        Ambito::Sistema => archivos::sin_tamano(&interfaz().string("font-name")).to_owned(),
        Ambito::Terminal => std::fs::read_to_string(config("kitty/kitty.conf"))
            .ok()
            .and_then(|t| archivos::kitty_familia(&t))
            .unwrap_or_default(),
    }
}

fn gsettings_familia(clave: &str, familia: &str) {
    let s = interfaz();
    let nuevo = archivos::con_tamano(familia, &s.string(clave));
    if let Err(e) = s.set_string(clave, &nuevo) {
        eprintln!("morgan: apariencia: gsettings {clave}: {e}");
    }
}

/// fontconfig nunca falla con una fuente que no existe: hace fallback en silencio.
pub fn validar(ambito: Ambito, familia: &str) -> Result<(), String> {
    if candidatas(&fc_list(), ambito).iter().any(|(f, _)| f == familia) {
        Ok(())
    } else {
        Err(format!("«{familia}» no está entre las familias instaladas"))
    }
}

/// En el hilo principal: usa gsettings.
pub fn aplicar(ambito: Ambito, familia: &str) {
    match ambito {
        // El chrome de toda app GTK y Qt. GTK toma gsettings en caliente; Qt lee qt6ct.conf
        // al arrancar.
        Ambito::Sistema => {
            gsettings_familia("font-name", familia);
            gsettings_familia("document-font-name", familia);
            for ini in ["gtk-3.0/settings.ini", "gtk-4.0/settings.ini"] {
                reescribir(&config(ini), |t| archivos::ini_fuente(t, familia));
            }
            for qt in ["qt5ct/qt5ct.conf", "qt6ct/qt6ct.conf"] {
                reescribir(&config(qt), |t| archivos::qt(t, "general", familia));
            }
        }
        // kitty y la monoespaciada del sistema (código y logs en GTK/Qt).
        Ambito::Terminal => {
            let kitty = config("kitty/kitty.conf");
            reescribir(&kitty, |t| archivos::kitty(t, familia));
            gsettings_familia("monospace-font-name", familia);
            for qt in ["qt5ct/qt5ct.conf", "qt6ct/qt6ct.conf"] {
                reescribir(&config(qt), |t| archivos::qt(t, "fixed", familia));
            }
            // kitty relee su config con SIGUSR1 (lo mismo que `kitten @ load-config`, sin
            // necesitar remote control).
            if kitty.exists() && !super::seco() {
                crate::comando::lanzar(&["pkill", "-SIGUSR1", "-x", "kitty"]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FC: &str = "Inter\t\t80\t0\nInter\t\t200\t0\nInter\t\t80\t100\n\
        Times New Roman\t\t80\t0\nArial Black\t\t210\t0\nArial\t\t80\t0\n\
        JetBrainsMono Nerd Font Mono\t100\t80\t0\nJetBrainsMono Nerd Font\t100\t80\t0\n\
        Noto Sans CJK JP\t\t80\t0\nNoto Sans Mono\t\t80\t0\nFira Code\t100\t80\t0\n";

    #[test]
    fn sistema() {
        let v = candidatas(FC, Ambito::Sistema);
        let nombres: Vec<&str> = v.iter().map(|(f, _)| f.as_str()).collect();
        // Sin "Times New Roman" (Pango la lee "Times New", que no existe), sin CJK ni Mono;
        // "Arial Black" queda: "Arial" está.
        assert_eq!(nombres, ["Arial", "Arial Black", "Inter"]);
        assert_eq!(v[2].1, "3 variantes · negrita e itálica");
        assert_eq!(v[1].1, "1 variante · solo negrita");
    }

    #[test]
    fn terminal() {
        let v = candidatas(FC, Ambito::Terminal);
        let nombres: Vec<&str> = v.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(nombres, ["Fira Code", "JetBrainsMono Nerd Font", "JetBrainsMono Nerd Font Mono"]);
        assert!(v[2].1.ends_with("íconos de una celda"));
        assert!(v[1].1.ends_with("íconos anchos"));
    }
}
