//! Cómo llega cada archivo del tema activo a su app, y cómo se le avisa a la que ya corre.
//!
//! Las propias (`PROPIAS`) son las de apps comunes y valen si la app está instalada. Las del
//! usuario van en `~/.config/morgan/temas.toml`:
//!
//! ```toml
//! apagadas = ["vesktop"]                    # propias que no se tocan
//! despues = [["mi-script", "--recargar"]]  # comandos al terminar, sin esperarlos
//!
//! [[integracion]]
//! plantilla = "obsidian.css"
//! enlazar = ["~/algo/theme.css"]            # symlinks al archivo renderizado
//! copiar = ["~/vaults/*/.obsidian/snippets/theme.css"]   # para apps que no siguen symlinks
//! json = [{ destino = "~/.app/settings.json", claves = [["workbench.colorCustomizations"]] }]
//! aviso = ["hyprctl", "reload", "config-only"]
//! ```
//!
//! `json` copia esas claves (cada una, un camino de claves) del archivo renderizado al
//! destino sin tocarle el resto; `*` en `copiar` es un nombre cualquiera (no los ocultos).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

use crate::{Informe, Rutas, hay_programa, lanzar};

#[derive(Clone, Copy, PartialEq)]
enum Aviso {
    Nada,
    /// kitty relee la config con SIGUSR1: lo que hace `kitten @ load-config`, sin pedir
    /// `allow_remote_control`.
    Kitty,
    Gtk,
    /// Vencord vigila la carpeta `themes/`: reescribir el destino del symlink no da evento
    /// ahí, tocar el symlink sí.
    Vesktop,
    /// bat no lee el `.tmTheme` sino un caché compilado (~0,35 s): se recompila si cambió.
    Bat,
    /// starship no tiene includes y su config es del usuario: la paleta va entre marcadores.
    Starship,
    /// `config-only`: un reload a secas reaplica los monitores, y reaplicar una salida
    /// apagada la prende y apaga un instante.
    Hyprland,
}

struct Propia {
    nombre: &'static str,
    plantilla: &'static str,
    programas: &'static [&'static str],
    enlaces: &'static [&'static str],
    aviso: Aviso,
}

const PROPIAS: &[Propia] = &[
    // morgan vigila el symlink y su destino: repinta solo.
    Propia { nombre: "morgan", plantilla: "morgan.css", programas: &[], enlaces: &["morgan/theme.css"], aviso: Aviso::Nada },
    // El gtk.css de cada versión lo importa.
    Propia { nombre: "gtk", plantilla: "gtk.css", programas: &[], enlaces: &["gtk-3.0/theme.css", "gtk-4.0/theme.css"], aviso: Aviso::Gtk },
    // qt5ct.conf y qt6ct.conf apuntan acá con `color_scheme_path`.
    Propia { nombre: "qt", plantilla: "qt.conf", programas: &["qt5ct", "qt6ct"], enlaces: &["qt5ct/colors/theme.conf", "qt6ct/colors/theme.conf"], aviso: Aviso::Nada },
    // kitty.conf: `include ./themes/theme.conf`.
    Propia { nombre: "kitty", plantilla: "kitty.conf", programas: &["kitty"], enlaces: &["kitty/themes/theme.conf"], aviso: Aviso::Kitty },
    // btop.conf: `color_theme = "theme"`.
    Propia { nombre: "btop", plantilla: "btop.theme", programas: &["btop"], enlaces: &["btop/themes/theme.theme"], aviso: Aviso::Nada },
    // El resaltado (syntect) de bat y del previsualizador de yazi: el mismo archivo.
    Propia { nombre: "bat", plantilla: "tmtheme.xml", programas: &["bat"], enlaces: &["bat/themes/theme.tmTheme"], aviso: Aviso::Bat },
    Propia { nombre: "yazi", plantilla: "tmtheme.xml", programas: &["yazi"], enlaces: &["yazi/flavors/theme.yazi/tmtheme.xml"], aviso: Aviso::Nada },
    Propia { nombre: "yazi", plantilla: "yazi-flavor.toml", programas: &["yazi"], enlaces: &["yazi/flavors/theme.yazi/flavor.toml"], aviso: Aviso::Nada },
    // El nombre importa: es el de `enabledThemes` en el settings.json de Vesktop.
    Propia { nombre: "vesktop", plantilla: "vesktop.css", programas: &["vesktop"], enlaces: &["vesktop/themes/theme.css"], aviso: Aviso::Vesktop },
    Propia { nombre: "starship", plantilla: "starship.toml", programas: &["starship"], enlaces: &[], aviso: Aviso::Starship },
    // `morgan.lua` (lo escribe morgan) lo carga desde el render: no hace falta enlace.
    Propia { nombre: "hyprland", plantilla: "hyprland.lua", programas: &["Hyprland"], enlaces: &[], aviso: Aviso::Hyprland },
];

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DelUsuario {
    #[serde(default)]
    apagadas: Vec<String>,
    #[serde(default)]
    despues: Vec<Vec<String>>,
    #[serde(default)]
    integracion: Vec<Integracion>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Integracion {
    plantilla: String,
    #[serde(default)]
    enlazar: Vec<String>,
    #[serde(default)]
    copiar: Vec<String>,
    #[serde(default)]
    json: Vec<Json>,
    #[serde(default)]
    aviso: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Json {
    destino: String,
    claves: Vec<Vec<String>>,
}

fn del_usuario(r: &Rutas, inf: &mut Informe) -> DelUsuario {
    let ruta = r.config.join("morgan/temas.toml");
    match std::fs::read_to_string(&ruta) {
        Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
            inf.avisos.push(format!("{}: {e}", ruta.display()));
            DelUsuario::default()
        }),
        Err(_) => DelUsuario::default(),
    }
}

/// Nada de esto corta: lo que falla se anota y se sigue con la próxima.
pub fn desplegar(r: &Rutas) -> Informe {
    let mut inf = Informe::default();
    let usuario = del_usuario(r, &mut inf);
    let render = r.render();
    let mut avisos: Vec<Aviso> = Vec::new();

    for p in PROPIAS {
        let src = render.join(p.plantilla);
        if usuario.apagadas.iter().any(|a| a == p.nombre)
            || !src.is_file()
            || !(p.programas.is_empty() || p.programas.iter().any(|n| hay_programa(n)))
        {
            continue;
        }
        for e in p.enlaces {
            anotar(&mut inf, enlazar(&src, &r.config.join(e)));
        }
        if !avisos.contains(&p.aviso) {
            avisos.push(p.aviso);
        }
    }

    let mut comandos: Vec<Vec<String>> = Vec::new();
    for i in &usuario.integracion {
        let src = render.join(&i.plantilla);
        if !src.is_file() {
            inf.avisos.push(format!("temas.toml: no hay plantilla {}", i.plantilla));
            continue;
        }
        for d in &i.enlazar {
            anotar(&mut inf, enlazar(&src, &r.expandir(d)));
        }
        for d in i.copiar.iter().flat_map(|d| expandir_comodines(&r.expandir(d))) {
            anotar(&mut inf, copiar(&src, &d));
        }
        for j in &i.json {
            anotar(&mut inf, inyectar_json(&src, &r.expandir(&j.destino), &j.claves));
        }
        if !i.aviso.is_empty() && !comandos.contains(&i.aviso) {
            comandos.push(i.aviso.clone());
        }
    }

    // Primero lo que está en pantalla; lo que tarda, al final y sin esperarlo.
    for a in avisos {
        let res = match a {
            Aviso::Nada => Ok(()),
            Aviso::Kitty => lanzar(&["pkill", "-USR1", "-x", "kitty"]).map_err(|e| format!("kitty: {e}")),
            Aviso::Gtk => gtk(r),
            Aviso::Vesktop => {
                let link = r.config.join("vesktop/themes/theme.css");
                lanzar(&["touch".as_ref(), "-h".as_ref(), link.as_os_str()]).map_err(|e| format!("vesktop: {e}"))
            }
            Aviso::Bat => bat(r),
            Aviso::Starship => starship(&render.join("starship.toml"), &r.config.join("starship.toml")),
            Aviso::Hyprland => {
                lanzar(&["hyprctl", "reload", "config-only"]).map_err(|e| format!("hyprland: {e}"))
            }
        };
        anotar(&mut inf, res);
    }
    for c in comandos.iter().chain(&usuario.despues) {
        anotar(&mut inf, lanzar(c).map_err(|e| format!("{}: {e}", c.join(" "))));
    }
    inf
}

fn anotar(inf: &mut Informe, r: Result<(), String>) {
    if let Err(e) = r {
        inf.avisos.push(e);
    }
}

/// `ln -sfn`, pero atómico: el symlink nuevo se arma al lado y se renombra encima.
fn enlazar(src: &Path, dst: &Path) -> Result<(), String> {
    if std::fs::read_link(dst).is_ok_and(|a| a == src) {
        return Ok(());
    }
    let err = |e: std::io::Error| format!("{}: {e}", dst.display());
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d).map_err(err)?;
    }
    if dst.is_dir() && !dst.is_symlink() {
        return Err(format!("{}: es una carpeta", dst.display()));
    }
    let tmp = dst.with_file_name(format!(".{}.morgan", dst.file_name().unwrap_or_default().to_string_lossy()));
    let _ = std::fs::remove_file(&tmp);
    std::os::unix::fs::symlink(src, &tmp).map_err(err)?;
    std::fs::rename(&tmp, dst).map_err(err)
}

/// Para las apps que vigilan su carpeta y no ven un cambio a través de un symlink.
fn copiar(src: &Path, dst: &Path) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", dst.display());
    let nuevo = std::fs::read(src).map_err(err)?;
    if std::fs::read(dst).is_ok_and(|v| v == nuevo) {
        return Ok(());
    }
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d).map_err(err)?;
    }
    std::fs::write(dst, nuevo).map_err(err)
}

fn expandir_comodines(ruta: &Path) -> Vec<PathBuf> {
    let mut v = vec![PathBuf::new()];
    for c in ruta.components() {
        let s = c.as_os_str().to_string_lossy();
        if !s.contains('*') {
            v.iter_mut().for_each(|p| p.push(c));
            continue;
        }
        v = v
            .iter()
            .flat_map(|base| {
                let mut hijos: Vec<PathBuf> = std::fs::read_dir(base)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| {
                        let n = e.file_name().to_string_lossy().into_owned();
                        !n.starts_with('.') && calza(&s, &n)
                    })
                    .map(|e| e.path())
                    .collect();
                hijos.sort();
                hijos
            })
            .collect();
    }
    v
}

fn calza(patron: &str, nombre: &str) -> bool {
    let partes: Vec<&str> = patron.split('*').collect();
    let (primera, ultima) = (partes[0], partes[partes.len() - 1]);
    if !nombre.starts_with(primera) || nombre.len() < primera.len() + ultima.len() || !nombre.ends_with(ultima) {
        return false;
    }
    let mut resto = &nombre[primera.len()..nombre.len() - ultima.len()];
    for p in &partes[1..partes.len() - 1] {
        match resto.find(p) {
            Some(i) => resto = &resto[i + p.len()..],
            None => return false,
        }
    }
    true
}

/// Se escribe solo si cambió, y sin reemplazar el archivo: si es un symlink, sigue siéndolo.
fn inyectar_json(src: &Path, dst: &Path, claves: &[Vec<String>]) -> Result<(), String> {
    let err = |e: String| format!("{}: {e}", dst.display());
    if !dst.exists() {
        return match dst.parent() {
            Some(d) if d.is_dir() => copiar(src, dst),
            _ => Ok(()),
        };
    }
    let leer = |p: &Path| -> Result<Value, String> {
        let s = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
        serde_json::from_str(&s).map_err(|e| e.to_string())
    };
    let fuente = leer(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let antes = std::fs::read_to_string(dst).map_err(|e| err(e.to_string()))?;
    let mut destino: Value = serde_json::from_str(&antes).map_err(|e| err(e.to_string()))?;
    for camino in claves {
        let Some(v) = camino.iter().try_fold(&fuente, |v, k| v.get(k)) else {
            continue;
        };
        let Some((ultima, padres)) = camino.split_last() else { continue };
        let mut obj = &mut destino;
        for k in padres {
            if !obj.get(k).is_some_and(Value::is_object) {
                obj[k.as_str()] = Value::Object(Default::default());
            }
            obj = &mut obj[k.as_str()];
        }
        if !obj.is_object() {
            return Err(err("la raíz no es un objeto".into()));
        }
        obj[ultima.as_str()] = v.clone();
    }
    let despues = serde_json::to_string_pretty(&destino).map_err(|e| err(e.to_string()))? + "\n";
    if despues != antes {
        std::fs::write(dst, despues).map_err(|e| err(e.to_string()))?;
    }
    Ok(())
}

const INICIO: &str = "# >>> THEME STARSHIP PALETTE >>>";
const FIN: &str = "# <<< THEME STARSHIP PALETTE <<<";

fn bloque_starship(texto: &str, bloque: &str) -> String {
    let mut t = texto.to_owned();
    if let Some(i) = t.find(INICIO) {
        t = match t[i..].find(FIN) {
            Some(j) => format!("{}\n{}", t[..i].trim_end(), t[i + j + FIN.len()..].trim_start_matches('\n'))
                .trim_end()
                .to_owned()
                + "\n",
            None => t[..i].trim_end().to_owned() + "\n",
        };
    }
    let t = format!("{}\n\n{INICIO}\n{}\n{FIN}\n", t.trim_end(), bloque.trim());
    let mut lineas: Vec<&str> = t.lines().collect();
    match lineas.iter().position(|l| l.trim().starts_with("palette ") && l.contains('=')) {
        Some(n) => lineas[n] = "palette = \"theme\"",
        None => lineas.insert(0, "palette = \"theme\""),
    }
    lineas.join("\n") + "\n"
}

fn starship(src: &Path, dst: &Path) -> Result<(), String> {
    let Ok(antes) = std::fs::read_to_string(dst) else {
        return Ok(()); // sin config de starship, nada que hacer
    };
    let bloque = std::fs::read_to_string(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let despues = bloque_starship(&antes, &bloque);
    if despues != antes {
        std::fs::write(dst, despues).map_err(|e| format!("{}: {e}", dst.display()))?;
    }
    Ok(())
}

fn bat(r: &Rutas) -> Result<(), String> {
    let tm = r.config.join("bat/themes/theme.tmTheme");
    let marca = r.cache.join("morgan/bat.tmTheme");
    let (Ok(actual), previo) = (std::fs::read(&tm), std::fs::read(&marca)) else {
        return Ok(());
    };
    if previo.is_ok_and(|p| p == actual) {
        return Ok(());
    }
    let _ = std::fs::create_dir_all(r.cache.join("morgan"));
    lanzar(&[
        "sh".as_ref(),
        "-c".as_ref(),
        "bat cache --build && cp \"$1\" \"$2\"".as_ref(),
        "sh".as_ref(),
        tm.as_os_str(),
        marca.as_os_str(),
    ])
    .map_err(|e| format!("bat: {e}"))
}

/// GTK3 (y Chromium en modo GTK) lee `gtk-3.0/gtk.css` una vez, pero relee el tema con
/// nombre cuando el nombre cambia. Si el tema GTK es uno de un par idéntico `<x>-a`/`<x>-b`
/// que importa `gtk-3.0/theme.css`, se pasa al otro y todo repinta en vivo. Sin el par, nada.
fn gtk(r: &Rutas) -> Result<(), String> {
    const CLAVE: [&str; 2] = ["org.gnome.desktop.interface", "gtk-theme"];
    let Ok(out) = Command::new("gsettings").arg("get").args(CLAVE).output() else {
        return Ok(());
    };
    let actual = String::from_utf8_lossy(&out.stdout).trim().trim_matches('\'').to_owned();
    let otro = match actual.strip_suffix("-a").map(|b| (b, "-b")).or(actual.strip_suffix("-b").map(|b| (b, "-a"))) {
        Some((base, sufijo)) => format!("{base}{sufijo}"),
        None => return Ok(()),
    };
    let existe = [r.datos[0].join("themes"), r.home.join(".themes")].iter().any(|d| d.join(&otro).is_dir());
    if !existe {
        return Ok(());
    }
    Command::new("gsettings")
        .arg("set")
        .args(CLAVE)
        .arg(&otro)
        .status()
        .map_err(|e| format!("gsettings: {e}"))?;
    // settings.ini, para lo que no lee gsettings (X11).
    let ini = r.config.join("gtk-3.0/settings.ini");
    if let Ok(s) = std::fs::read_to_string(&ini) {
        let n: Vec<String> = s
            .lines()
            .map(|l| match l.starts_with("gtk-theme-name=") {
                true => format!("gtk-theme-name={otro}"),
                false => l.to_owned(),
            })
            .collect();
        let n = n.join("\n") + if s.ends_with('\n') { "\n" } else { "" };
        if n != s {
            std::fs::write(&ini, n).map_err(|e| format!("{}: {e}", ini.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(nombre: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("morgan-temas-{nombre}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn comodines() {
        assert!(calza("*", "x"));
        assert!(calza("a*z", "abcz"));
        assert!(!calza("a*z", "az-"));
        assert!(calza("a*b*c", "a-b-c"));
        assert!(!calza("ab*ba", "aba"));
        let d = tmp("glob");
        for v in ["uno", "dos", ".oculto"] {
            std::fs::create_dir_all(d.join(v).join(".obsidian")).unwrap();
        }
        let v = expandir_comodines(&d.join("*/.obsidian/snippets/theme.css"));
        assert_eq!(v, [d.join("dos/.obsidian/snippets/theme.css"), d.join("uno/.obsidian/snippets/theme.css")]);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn enlaza_atomico_y_pisa() {
        let d = tmp("enlace");
        let (a, b) = (d.join("a"), d.join("b"));
        std::fs::write(&a, "x").unwrap();
        let dst = d.join("sub/theme.css");
        enlazar(&a, &dst).unwrap();
        assert_eq!(std::fs::read_link(&dst).unwrap(), a);
        std::fs::write(&b, "y").unwrap();
        enlazar(&b, &dst).unwrap();
        assert_eq!(std::fs::read_link(&dst).unwrap(), b);
        std::fs::remove_file(&dst).unwrap();
        std::fs::write(&dst, "archivo suelto").unwrap();
        enlazar(&a, &dst).unwrap();
        assert_eq!(std::fs::read_link(&dst).unwrap(), a);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn json_sin_tocar_lo_demas() {
        let d = tmp("json");
        let (src, dst) = (d.join("src.json"), d.join("dst.json"));
        std::fs::write(&src, r#"{"u": {"oscuro": {"a": 1}, "claro": 2}, "otra": 9}"#).unwrap();
        std::fs::write(&dst, r#"{"z": 1, "u": {"zz": true, "oscuro": {"b": 0}}, "a.b": 3}"#).unwrap();
        let claves = [vec!["u".into(), "oscuro".into()], vec!["u".into(), "claro".into()], vec!["falta".into()]];
        inyectar_json(&src, &dst, &claves).unwrap();
        let s = std::fs::read_to_string(&dst).unwrap();
        // El orden del destino se conserva y lo nuevo va al final.
        assert_eq!(
            s,
            "{\n  \"z\": 1,\n  \"u\": {\n    \"zz\": true,\n    \"oscuro\": {\n      \"a\": 1\n    },\n    \"claro\": 2\n  },\n  \"a.b\": 3\n}\n"
        );
        // Sin destino y con su carpeta, se copia entero.
        let nuevo = d.join("nuevo.json");
        inyectar_json(&src, &nuevo, &claves).unwrap();
        assert_eq!(std::fs::read(&nuevo).unwrap(), std::fs::read(&src).unwrap());
        // Sin carpeta, nada.
        inyectar_json(&src, &d.join("no/hay.json"), &claves).unwrap();
        assert!(!d.join("no").exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn starship() {
        let bloque = "\n[palettes.theme]\nfg = \"#fff\"\n";
        let inicial = "format = \"$all\"\npalette = \"viejo\"\n";
        let una = bloque_starship(inicial, bloque);
        assert_eq!(
            una,
            format!("format = \"$all\"\npalette = \"theme\"\n\n{INICIO}\n[palettes.theme]\nfg = \"#fff\"\n{FIN}\n")
        );
        assert_eq!(bloque_starship(&una, bloque), una, "idempotente");
        assert!(bloque_starship("format = 1\n", bloque).starts_with("palette = \"theme\"\nformat = 1\n"));
    }

    #[test]
    fn usuario() {
        let u: DelUsuario = toml::from_str(
            r#"
apagadas = ["vesktop"]
despues = [["mi-script", "--recargar"]]
[[integracion]]
plantilla = "x.json"
json = [{ destino = "~/a.json", claves = [["k"]] }]
"#,
        )
        .unwrap();
        assert_eq!(u.integracion[0].json[0].claves, [vec!["k".to_owned()]]);
        assert!(toml::from_str::<DelUsuario>("[[integracion]]\nplantilla = \"x\"\nenlace = []\n").is_err());
    }
}
