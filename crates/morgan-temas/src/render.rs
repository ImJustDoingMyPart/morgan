//! La config de matugen no se escribe a mano: se arma acá con cada plantilla que haya y se
//! tira al terminar. Dos cosas verificadas contra el binario (4.1.0): la tabla `[config]`
//! es obligatoria aunque quede vacía, y `input_path`/`output_path` tienen que ser absolutos.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::{MATUGEN, Rutas};

/// La escala de las cuotas: tres colores que se distingan a simple vista, y un esquema
/// tonal se arma alrededor de un solo matiz. `blend` los tiñe hacia la paleta del
/// wallpaper sin que el verde deje de ser verde. Un tema curado los trae de su paleta.
const CUOTAS: [(&str, &str); 3] =
    [("quota_ok", "#4caf50"), ("quota_warn", "#ffb300"), ("quota_crit", "#e53935")];

/// Las plantillas por nombre de archivo; la del usuario tapa a la del paquete.
pub fn plantillas(r: &Rutas) -> BTreeMap<String, PathBuf> {
    let mut v = BTreeMap::new();
    for dir in r.fuentes("templates").into_iter().rev() {
        let Ok(it) = std::fs::read_dir(&dir) else { continue };
        for e in it.flatten() {
            if e.path().is_file()
                && let Some(n) = e.file_name().to_str()
                && !n.starts_with('.')
            {
                v.insert(n.to_owned(), e.path());
            }
        }
    }
    v
}

fn config_matugen(plantillas: &BTreeMap<String, PathBuf>, salida: &Path) -> String {
    let mut cuotas = toml::Table::new();
    for (k, c) in CUOTAS {
        let mut t = toml::Table::new();
        t.insert("color".into(), c.into());
        t.insert("blend".into(), true.into());
        cuotas.insert(k.into(), t.into());
    }
    let mut config = toml::Table::new();
    config.insert("custom_colors".into(), cuotas.into());
    let mut templates = toml::Table::new();
    for (nombre, ruta) in plantillas {
        let mut t = toml::Table::new();
        t.insert("input_path".into(), ruta.to_string_lossy().into_owned().into());
        t.insert("output_path".into(), salida.join(nombre).to_string_lossy().into_owned().into());
        templates.insert(nombre.replace(|c: char| !c.is_ascii_alphanumeric(), "-"), t.into());
    }
    let mut raiz = toml::Table::new();
    raiz.insert("config".into(), config.into());
    raiz.insert("templates".into(), templates.into());
    toml::to_string(&raiz).unwrap_or_default()
}

struct Temporal(PathBuf);

impl Temporal {
    fn nueva() -> Result<Temporal, String> {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let d = base.join(format!("morgan-tema.{}", std::process::id()));
        std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        Ok(Temporal(d))
    }
}

impl Drop for Temporal {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn correr(args: &[&std::ffi::OsStr]) -> Result<Vec<u8>, String> {
    let out = Command::new(MATUGEN)
        .args(args)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("no se pudo correr matugen: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("matugen falló: {}", err.trim()));
    }
    Ok(out.stdout)
}

pub fn renderizar(r: &Rutas, plantillas: &BTreeMap<String, PathBuf>, datos: &Value) -> Result<(), String> {
    let salida = r.render();
    std::fs::create_dir_all(&salida).map_err(|e| format!("{}: {e}", salida.display()))?;
    let tmp = Temporal::nueva()?;
    let (config, paleta) = (tmp.0.join("config.toml"), tmp.0.join("paleta.json"));
    let escribir = |p: &Path, s: String| std::fs::write(p, s).map_err(|e| format!("{}: {e}", p.display()));
    escribir(&config, config_matugen(plantillas, &salida))?;
    escribir(&paleta, datos.to_string())?;
    correr(&["-c".as_ref(), config.as_os_str(), "json".as_ref(), paleta.as_os_str(), "--quiet".as_ref()])
        .map(drop)
}

fn ajustes_matugen(r: &Rutas) -> [String; 3] {
    let config: toml::Table = std::fs::read_to_string(r.config.join("morgan/config.toml"))
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_default();
    let m = config.get("matugen").and_then(toml::Value::as_table);
    let leer = |k: &str, def: &str| match m.and_then(|m| m.get(k)) {
        Some(toml::Value::String(s)) => s.clone(),
        Some(toml::Value::Integer(i)) => i.to_string(),
        Some(toml::Value::Float(f)) => f.to_string(),
        _ => def.to_owned(),
    };
    [leer("tipo", "scheme-tonal-spot"), leer("modo", "dark"), leer("contraste", "0")]
}

fn wallpaper(r: &Rutas) -> Option<PathBuf> {
    if let Ok(s) = std::fs::read_to_string(r.estado.join("wallpaper"))
        && let Some(l) = s.lines().next().filter(|l| !l.is_empty())
    {
        return Some(PathBuf::from(l));
    }
    let out = Command::new("awww").arg("query").stderr(Stdio::null()).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.split_once("currently displaying: image: ").map(|(_, p)| PathBuf::from(p)))
}

pub fn datos_matugen(r: &Rutas, imagen: Option<&Path>) -> Result<Value, String> {
    let imagen = imagen
        .map(Path::to_path_buf)
        .or_else(|| wallpaper(r))
        .filter(|p| p.is_file())
        .ok_or("no hay un wallpaper legible")?;
    let [tipo, modo, contraste] = ajustes_matugen(r);
    let tmp = Temporal::nueva()?;
    let config = tmp.0.join("config.toml");
    std::fs::write(&config, config_matugen(&BTreeMap::new(), &tmp.0)).map_err(|e| e.to_string())?;
    // `--prefer` no es opcional: con varios colores fuente candidatos, matugen 4 pregunta
    // cuál usar y sin terminal aborta.
    let volcado = correr(&[
        "-c".as_ref(),
        config.as_os_str(),
        "image".as_ref(),
        imagen.as_os_str(),
        "--prefer".as_ref(),
        "saturation".as_ref(),
        "--type".as_ref(),
        tipo.as_ref(),
        "--mode".as_ref(),
        modo.as_ref(),
        "--contrast".as_ref(),
        contraste.as_ref(),
        "--dry-run".as_ref(),
        "-j".as_ref(),
        "hex".as_ref(),
        "--quiet".as_ref(),
    ])?;
    let v: Value = serde_json::from_slice(&volcado).map_err(|e| format!("volcado de matugen: {e}"))?;
    crate::paleta::json_matugen(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config() {
        let mut p = BTreeMap::new();
        p.insert("kitty.conf".to_owned(), PathBuf::from("/p/kitty.conf"));
        p.insert("con \"comillas\".css".to_owned(), PathBuf::from("/p/x"));
        let t: toml::Table = config_matugen(&p, Path::new("/s")).parse().unwrap();
        assert!(t["config"]["custom_colors"]["quota_ok"]["blend"].as_bool().unwrap());
        assert_eq!(t["templates"]["kitty-conf"]["output_path"].as_str(), Some("/s/kitty.conf"));
        assert_eq!(t["templates"]["con--comillas--css"]["input_path"].as_str(), Some("/p/x"));
    }
}
