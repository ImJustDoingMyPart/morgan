//! Las copias para el greeter (`morgan-greet`), que corre como el usuario `greeter` y no
//! puede leer el home: el tema, el fondo del bloqueo y lo que el bloqueo lee de la config
//! (`[lock]` y `[appearance] shadow`), en `/var/lib/morgan-greet/usuarios/<cuenta>/`. Así el
//! inicio de sesión se ve como el bloqueo.
//!
//! La carpeta la crea `morgan greet install`, con sticky bit (como `/tmp`): cada cuenta
//! escribe la suya y nadie la de otro, y el greeter solo usa los archivos cuyo dueño es esa
//! cuenta. Sin la carpeta (greeter no instalado), no se hace nada.
//!
//! Se copia al arrancar, al cambiar el tema, al guardar la config y al cambiar el fondo; en
//! otro hilo, y de a una copia a la vez.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const BASE: &str = "/var/lib/morgan-greet/usuarios";

static CONFIG: OnceLock<PathBuf> = OnceLock::new();
static UNA_A_LA_VEZ: Mutex<()> = Mutex::new(());

/// La config que se copia (la que usa este morgan). Se llama una vez, al arrancar.
pub fn iniciar(config: &Path) {
    let _ = CONFIG.set(config.to_owned());
    copiar();
}

pub fn copiar() {
    if !Path::new(BASE).is_dir() {
        return;
    }
    let Some(config) = CONFIG.get().cloned() else {
        return;
    };
    std::thread::spawn(move || {
        let _uno = UNA_A_LA_VEZ.lock();
        if let Err(e) = sincronizar(Path::new(BASE), &config) {
            eprintln!("morgan: greeter: {e}");
        }
    });
}

fn sincronizar(base: &Path, config: &Path) -> Result<(), String> {
    let dir = base.join(glib::user_name());
    if !dir.is_dir() {
        std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let configs = glib::user_config_dir();
    let estado = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| glib::home_dir().join(".local/state"));
    // El tema es un symlink al del tema activo: se copia lo que apunta.
    if let Ok(tema) = std::fs::read(configs.join("morgan/theme.css")) {
        escribir(&dir.join("theme.css"), &tema)?;
    }
    if let Ok(fondo) = std::fs::read(estado.join("wallpaper-lock.png")) {
        escribir(&dir.join("fondo.png"), &fondo)?;
    }
    let texto = std::fs::read_to_string(config).unwrap_or_default();
    escribir(&dir.join("bloqueo.toml"), bloqueo(&texto).as_bytes())?;
    Ok(())
}

/// Solo lo que lee el bloqueo; el resto de la config no sale del home.
fn bloqueo(config: &str) -> String {
    let tabla: toml::Table = config.parse().unwrap_or_default();
    let mut t = toml::Table::new();
    if let Some(s) = tabla
        .get("appearance")
        .and_then(|a| a.get("shadow"))
        .cloned()
    {
        let mut a = toml::Table::new();
        a.insert("shadow".into(), s);
        t.insert("appearance".into(), a.into());
    }
    if let Some(l) = tabla.get("lock").cloned() {
        t.insert("lock".into(), l);
    }
    toml::to_string(&t).unwrap_or_default()
}

/// A un temporal y después se renombra: el greeter nunca lee uno a medias. Si es igual al
/// que ya está, no se toca.
fn escribir(ruta: &Path, datos: &[u8]) -> Result<(), String> {
    if std::fs::read(ruta).is_ok_and(|v| v == datos) {
        return Ok(());
    }
    let tmp = ruta.with_extension("nuevo");
    std::fs::write(&tmp, datos)
        .and_then(|()| std::fs::rename(&tmp, ruta))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", ruta.display())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_sale_lo_del_bloqueo() {
        let c = "[shell]\nopacity = 0.9\n[appearance]\nshadow = \"glow\"\nspeed = 1\n\
                 [lock]\ntyping = \"dots\"\n[agenda]\nurl = \"https://privado\"\n";
        let b = bloqueo(c);
        assert!(b.contains("shadow = \"glow\"") && b.contains("typing = \"dots\""), "{b}");
        assert!(!b.contains("privado") && !b.contains("opacity") && !b.contains("speed"));
        assert_eq!(bloqueo("no es toml ["), "");
    }

    #[test]
    fn copia_los_tres_y_no_reescribe_lo_igual() {
        let base = std::env::temp_dir().join(format!("morgan-greeter-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let config = base.join("config.toml");
        std::fs::write(&config, "[lock]\nspark = false\n").unwrap();
        sincronizar(&base, &config).unwrap();
        let dir = base.join(glib::user_name());
        let b = dir.join("bloqueo.toml");
        assert!(std::fs::read_to_string(&b).unwrap().contains("spark = false"));
        let antes = std::fs::metadata(&b).unwrap().modified().unwrap();
        sincronizar(&base, &config).unwrap();
        assert_eq!(std::fs::metadata(&b).unwrap().modified().unwrap(), antes);
        std::fs::remove_dir_all(base).unwrap();
    }
}
