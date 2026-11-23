//! Lo que la escena toma de la cuenta: la paleta, el fondo, `[lock]` y la sombra, y la hora
//! local. El bloqueo lo lee del home (`Rutas::del_home`); el greeter, de las copias que morgan
//! deja fuera del home. Cualquier problema deja los valores de fábrica: ni la config ni el
//! fondo pueden impedir que la pantalla aparezca.

use std::path::{Path, PathBuf};
use std::time::Duration;

use morgan_ipc::bloqueo::Bloqueo;
use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

use crate::dibujo::Fondo;
use crate::escena::{Borde, Paleta};
use crate::fondo;

/// Dónde están los tres archivos de una cuenta.
pub struct Rutas {
    /// `theme.css` de morgan.
    pub tema: PathBuf,
    /// El fondo ya preparado para el bloqueo.
    pub fondo: PathBuf,
    /// La config de morgan; solo se leen `[lock]` y `[appearance] shadow`.
    pub config: PathBuf,
}

impl Rutas {
    /// Las del usuario de este proceso, por `XDG_CONFIG_HOME`, `XDG_STATE_HOME` y `HOME`.
    pub fn del_home() -> Option<Rutas> {
        let config = config_dir()?;
        let estado = std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
        Some(Rutas {
            tema: config.join("morgan/theme.css"),
            fondo: estado.join("wallpaper-lock.png"),
            config: config.join("morgan/config.toml"),
        })
    }
}

pub fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

pub fn paleta(ruta: &Path) -> Paleta {
    Paleta::desde_css(&std::fs::read_to_string(ruta).unwrap_or_default())
}

pub fn cargar_fondo(ruta: &Path) -> Option<Fondo> {
    let img = match fondo::cargar(ruta) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("morgan-lock: {}: {e}", ruta.display());
            return None;
        }
    };
    // La imagen grande ya se soltó: que glibc devuelva esa memoria.
    // SAFETY: sin argumentos que validar; solo recorta el heap.
    unsafe { libc::malloc_trim(0) };
    Some(Fondo {
        imagen: ImageData {
            data: Blob::new(std::sync::Arc::new(img.pixeles)),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: img.ancho,
            height: img.alto,
        },
        achicada: img.achicada as f64,
    })
}

/// `[lock]` de la config de morgan, y la sombra de `[appearance]`. Sin archivo, TOML roto o
/// un valor desconocido: los de fábrica.
pub fn estilo(ruta: &Path) -> (Bloqueo, Borde) {
    let Ok(texto) = std::fs::read_to_string(ruta) else {
        return (Bloqueo::default(), Borde::Sombra);
    };
    let estilo = leer_estilo(&texto).unwrap_or_else(|e| {
        eprintln!(
            "morgan-lock: {}: [lock]: {e}; con los valores de fábrica",
            ruta.display()
        );
        Bloqueo::default()
    });
    (estilo, leer_borde(&texto))
}

/// `[appearance] shadow`: la misma que la de las ventanas y los paneles de morgan.
fn leer_borde(texto: &str) -> Borde {
    let tabla: toml::Table = texto.parse().unwrap_or_default();
    let id = tabla
        .get("appearance")
        .and_then(|a| a.get("shadow"))
        .and_then(|s| s.as_str())
        .unwrap_or("black");
    Borde::de(id)
}

fn leer_estilo(texto: &str) -> Result<Bloqueo, String> {
    let tabla: toml::Table = texto.parse().map_err(|e: toml::de::Error| e.to_string())?;
    match tabla.get("lock") {
        Some(v) => v
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| e.to_string()),
        None => Ok(Bloqueo::default()),
    }
}

/// Hora, minutos, fecha («jueves 22 de octubre») y lo que falta para el próximo minuto.
pub fn ahora_local() -> (String, String, String, Duration) {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: punteros a structs locales, del tamaño que piden.
    let tm = unsafe {
        libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts);
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&ts.tv_sec, &mut tm);
        tm
    };
    let formato = |f: &std::ffi::CStr| {
        let mut buf = [0u8; 128];
        // SAFETY: `buf` tiene el tamaño que se pasa; `tm` salió de localtime_r.
        let n = unsafe { libc::strftime(buf.as_mut_ptr().cast(), buf.len(), f.as_ptr(), &tm) };
        String::from_utf8_lossy(&buf[..n]).into_owned()
    };
    let falta = 60_000 - (tm.tm_sec as u64 * 1000 + ts.tv_nsec as u64 / 1_000_000).min(59_999);
    (
        formato(c"%H"),
        formato(c"%M"),
        formato(&std::ffi::CString::new(tr!("%A %-d de %B")).unwrap_or_default()),
        Duration::from_millis(falta.max(1)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use morgan_ipc::bloqueo::Escritura;

    #[test]
    fn el_borde_sigue_a_la_sombra() {
        assert_eq!(
            leer_borde("[appearance]\nshadow = \"glow\"\n"),
            Borde::Brillo
        );
        assert_eq!(
            leer_borde("[appearance]\nshadow = \"black\"\n"),
            Borde::Sombra
        );
        assert_eq!(leer_borde(""), Borde::Sombra);
        assert_eq!(leer_borde("no es toml ["), Borde::Sombra);
    }

    #[test]
    fn el_estilo_sale_de_su_seccion_y_lo_roto_no_impide_leer() {
        let b = leer_estilo("[shell]\nopacity = 0.9\n[lock]\ntyping = \"dots\"\n").unwrap();
        assert_eq!(b.escritura, Escritura::Puntos);
        assert!(b.chispa);
        assert_eq!(leer_estilo("[shell]\n").unwrap(), Bloqueo::default());
        assert!(leer_estilo("[lock]\ntyping = \"nada\"\n").is_err());
        assert!(leer_estilo("no es toml [").is_err());
    }
}
