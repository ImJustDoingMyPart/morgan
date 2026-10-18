//! El motor de temas de morgan.
//!
//! Un tema es un `paleta.toml` (curado) o el wallpaper pasado por matugen (`matugen`). Los
//! dos caminos terminan en el mismo `matugen json` con las mismas plantillas, así un tema
//! sale completo para todas las apps por construcción. Se renderiza solo el tema activo, en
//! un directorio fijo (`render`); las apps miran ahí por las integraciones (`integraciones`).
//!
//! Dónde está cada cosa (la primera que exista gana, por nombre):
//!
//! | Qué | Del usuario | Del paquete |
//! |---|---|---|
//! | temas curados | `~/.config/morgan/temas/<tema>/paleta.toml` | `<datos>/morgan/temas/…` |
//! | plantillas | `~/.config/morgan/plantillas/<archivo>` | `<datos>/morgan/plantillas/…` |
//! | integraciones propias | `~/.config/morgan/temas.toml` | las de `integraciones::PROPIAS` |
//!
//! `<datos>` es `$XDG_DATA_HOME` y después cada `$XDG_DATA_DIRS`. Lo renderizado y el tema
//! activo van en `$XDG_STATE_HOME/morgan/`.

mod integraciones;
pub mod paleta;
mod render;

use std::collections::BTreeSet;
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const MATUGEN: &str = "matugen";

/// La paleta del tema propio, embebida: morgan tiene colores aunque no haya nada renderizado.
const PALETA_POR_DEFECTO: &str = include_str!("../../../dist/share/morgan/temas/morgan/paleta.toml");

const ROLES_MORGAN: [&str; 9] = [
    "surface_container_low",
    "primary",
    "tertiary",
    "on_surface",
    "on_surface_variant",
    "outline_variant",
    "error",
    "surface",
    "surface_container",
];

/// No se aplica sola a las demás apps: eso les reemplaza archivos de su config.
pub fn colores_por_defecto() -> String {
    let roles = paleta::roles_curado(PALETA_POR_DEFECTO).unwrap_or_default();
    ROLES_MORGAN
        .iter()
        .filter_map(|r| Some(format!("@define-color {r} {};\n", roles.get(*r)?)))
        .collect()
}

#[derive(Clone, Debug)]
pub struct Rutas {
    pub home: PathBuf,
    pub config: PathBuf,
    pub estado: PathBuf,
    pub cache: PathBuf,
    pub datos: Vec<PathBuf>,
}

impl Rutas {
    pub fn del_entorno() -> Rutas {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let var = |v: &str, def: &str| match std::env::var_os(v) {
            Some(p) if Path::new(&p).is_absolute() => PathBuf::from(p),
            _ => home.join(def),
        };
        let mut datos = vec![var("XDG_DATA_HOME", ".local/share")];
        let dirs = std::env::var("XDG_DATA_DIRS")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        datos.extend(dirs.split(':').filter(|d| d.starts_with('/')).map(PathBuf::from));
        Rutas {
            config: var("XDG_CONFIG_HOME", ".config"),
            estado: var("XDG_STATE_HOME", ".local/state"),
            cache: var("XDG_CACHE_HOME", ".cache"),
            home,
            datos,
        }
    }

    fn fuentes(&self, sub: &str) -> Vec<PathBuf> {
        let mut v = vec![self.config.join("morgan").join(sub)];
        v.extend(self.datos.iter().map(|d| d.join("morgan").join(sub)));
        v
    }

    /// Fijo: las integraciones apuntan acá para siempre.
    pub fn render(&self) -> PathBuf {
        self.estado.join("morgan/tema")
    }

    fn archivo_activo(&self) -> PathBuf {
        self.estado.join("morgan/tema-activo")
    }

    fn expandir(&self, ruta: &str) -> PathBuf {
        match ruta.strip_prefix("~/") {
            Some(r) => self.home.join(r),
            None => PathBuf::from(ruta),
        }
    }
}

pub fn catalogo(r: &Rutas) -> Vec<String> {
    let mut v = BTreeSet::new();
    for dir in r.fuentes("temas") {
        let Ok(it) = std::fs::read_dir(&dir) else { continue };
        for e in it.flatten() {
            if e.path().join("paleta.toml").is_file()
                && let Some(n) = e.file_name().to_str()
            {
                v.insert(n.to_owned());
            }
        }
    }
    if hay_programa(MATUGEN) {
        v.insert(MATUGEN.to_owned());
    }
    v.into_iter().collect()
}

pub fn activo(r: &Rutas) -> String {
    std::fs::read_to_string(r.archivo_activo())
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

#[derive(Debug, Default)]
pub struct Informe {
    pub avisos: Vec<String>,
}

pub fn aplicar(r: &Rutas, tema: &str, imagen: Option<&Path>) -> Result<Informe, String> {
    let _lock = Lock::tomar(r)?;
    let datos = if tema == MATUGEN {
        render::datos_matugen(r, imagen)?
    } else {
        let paleta = r
            .fuentes("temas")
            .into_iter()
            .map(|d| d.join(tema).join("paleta.toml"))
            .find(|p| p.is_file())
            .ok_or_else(|| format!("no existe el tema '{tema}'"))?;
        let texto = std::fs::read_to_string(&paleta).map_err(|e| format!("{}: {e}", paleta.display()))?;
        paleta::json_curado(&paleta::roles_curado(&texto).map_err(|e| format!("{tema}: {e}"))?)
    };
    let plantillas = render::plantillas(r);
    if plantillas.is_empty() {
        return Err("no hay plantillas (~/.config/morgan/plantillas)".into());
    }
    render::renderizar(r, &plantillas, &datos)?;
    escribir(&r.archivo_activo(), &format!("{tema}\n"))?;
    Ok(integraciones::desplegar(r))
}

pub fn recargar(r: &Rutas) -> Result<Informe, String> {
    match activo(r).as_str() {
        "" => Err("no hay un tema activo".into()),
        t => aplicar(r, t, None),
    }
}

pub fn wallpaper(r: &Rutas, imagen: Option<&Path>) -> Result<Option<Informe>, String> {
    if activo(r) != MATUGEN {
        return Ok(None);
    }
    aplicar(r, MATUGEN, imagen).map(Some)
}

fn hay_programa(nombre: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(nombre).is_file()))
}

/// Escribe sin reemplazar el archivo: si es un symlink, sigue siéndolo.
fn escribir(ruta: &Path, texto: &str) -> Result<(), String> {
    if let Some(d) = ruta.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::write(ruta, texto).map_err(|e| format!("{}: {e}", ruta.display()))
}

/// Lanza un comando sin esperarlo ni heredarle la salida: el aviso sigue aunque quien
/// aplicó ya haya terminado.
fn lanzar(args: &[impl AsRef<std::ffi::OsStr>]) -> io::Result<()> {
    let (prog, resto) = args.split_first().ok_or(io::ErrorKind::InvalidInput)?;
    Command::new(prog)
        .args(resto)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(drop)
}

/// Un cambio de tema a la vez: un wallpaper que cambia mientras se elige un tema no deja
/// las apps a medias entre los dos.
struct Lock(#[allow(dead_code)] std::fs::File);

impl Lock {
    fn tomar(r: &Rutas) -> Result<Lock, String> {
        let ruta = r.estado.join("morgan/tema.lock");
        let _ = std::fs::create_dir_all(r.estado.join("morgan"));
        let f = std::fs::File::create(&ruta).map_err(|e| format!("{}: {e}", ruta.display()))?;
        // SAFETY: flock sobre un descriptor abierto que vive lo que vive `Lock`.
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(format!("{}: {}", ruta.display(), io::Error::last_os_error()));
        }
        Ok(Lock(f))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_colores_por_defecto_cubren_la_plantilla_de_morgan() {
        let css = colores_por_defecto();
        assert_eq!(css.lines().count(), ROLES_MORGAN.len(), "la paleta resuelve todo");
        let plantilla = include_str!("../../../dist/share/morgan/plantillas/morgan.css");
        let pedidos: Vec<&str> = plantilla
            .split("{{colors.")
            .skip(1)
            .filter_map(|t| t.split('.').next())
            .collect();
        assert!(!pedidos.is_empty());
        for r in pedidos {
            assert!(css.contains(&format!("@define-color {r} #")), "falta {r}");
        }
    }
}
