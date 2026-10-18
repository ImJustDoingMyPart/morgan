//! Los programas se lanzan todos juntos (`comando::salidas`): una página tarda lo que su
//! lectura más lenta, no la suma.

use std::path::{Path, PathBuf};

use gtk::prelude::*;
use gtk::{gio, glib};

use super::arbol::{self, Seccion};
use super::{atajos, compositor, cursores, sistema};
use crate::comando;

pub async fn leer(args: &[&str]) -> String {
    comando::salida(args).await.unwrap_or_else(|e| {
        eprintln!("morgan: menú: {e}");
        String::new()
    })
}

#[derive(Debug, Default)]
pub struct Listas {
    opciones: crate::apariencia::Opciones,
    muestras_cursor: arbol::MuestrasCursor,
}

pub async fn apariencia(
    listas: Option<std::rc::Rc<Listas>>,
    fondos: &crate::config::Fondos,
) -> (arbol::Lecturas, Option<Listas>) {
    // Lo puesto en el hilo principal: usa gsettings.
    let p = crate::apariencia::puesto();
    let nuevas = match listas {
        Some(_) => None,
        None => {
            let lado = LADO_LEIDO * escala();
            gio::spawn_blocking(move || {
                let opciones = crate::apariencia::opciones();
                let muestras = cursores::muestras(&opciones.cursores, lado);
                (opciones, muestras)
            })
            .await
            .ok()
            .map(|(opciones, muestras)| Listas {
                opciones,
                muestras_cursor: muestras
                    .into_iter()
                    .map(|(t, m)| (t, std::rc::Rc::from(m)))
                    .collect(),
            })
        }
    };
    let vacia = Listas::default();
    let l: &Listas = listas.as_deref().or(nuevas.as_ref()).unwrap_or(&vacia);
    let o = &l.opciones;

    let rutas = morgan_temas::Rutas::del_entorno();
    let temas = morgan_temas::catalogo(&rutas);
    let tema_activo = morgan_temas::activo(&rutas);

    let carpeta = fondos.raiz();
    let lecturas = arbol::Lecturas {
        temas,
        tema_activo,
        carpeta_vista: con_tilde(&carpeta.to_string_lossy(), &glib::home_dir()),
        carpeta,
        duracion: match fondos.rotar {
            0 => "off".into(),
            h => h.to_string(),
        },
        fuente_sistema: p.fuente_sistema,
        fuentes_sistema: o.fuentes_sistema.clone(),
        fuente_terminal: p.fuente_terminal,
        fuentes_terminal: o.fuentes_terminal.clone(),
        icono: p.iconos,
        iconos: o.iconos.clone(),
        cursor: p.cursor,
        cursores: o.cursores.clone(),
        muestras_cursor: l.muestras_cursor.clone(),
    };
    (lecturas, nuevas)
}

pub const ALTO_MUESTRA: u32 = 16;
/// El doble del alto, para achicar en vez de agrandar.
const LADO_LEIDO: u32 = 32;

fn escala() -> u32 {
    let Some(d) = gtk::gdk::Display::default() else {
        return 1;
    };
    let m = d.monitors();
    (0..m.n_items())
        .filter_map(|i| m.item(i)?.downcast::<gtk::gdk::Monitor>().ok())
        .map(|m| m.scale_factor().max(1) as u32)
        .max()
        .unwrap_or(1)
}

pub async fn compositor(cambios: &Path) -> Vec<Seccion> {
    let actuales =
        compositor::de_batch(&leer(&["hyprctl", "--batch", &compositor::pedido_batch()]).await);
    let cambios = compositor::Cambios::leer(cambios).unwrap_or_else(|e| {
        eprintln!("morgan: menú: {e}");
        Default::default()
    });
    let evdev = std::fs::read_to_string("/usr/share/X11/xkb/rules/evdev.lst").unwrap_or_default();
    compositor::pagina(&actuales, &cambios, &compositor::distribuciones(&evdev))
}


pub async fn atajos(c: &crate::config::Atajos) -> Vec<Seccion> {
    let json = leer(&["hyprctl", "-j", "binds"]).await;
    let binds = crate::launcher::fuentes::binds(&json).unwrap_or_else(|e| {
        eprintln!("morgan: menú: {e}");
        Vec::new()
    });
    let mut archivos = Vec::new();
    archivos_lua(&glib::user_config_dir().join("hypr"), 0, &mut archivos);
    archivos.push(crate::hypr_cfg::ruta());
    archivos.sort();
    archivos.dedup();
    let grupos: Vec<_> = archivos
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .flat_map(|t| atajos::grupos_de_binds_lua(&t))
        .collect();
    atajos::pagina(&binds, &grupos, c)
}

/// Hasta tres niveles: no se pierde en un symlink que vuelve atrás.
fn archivos_lua(dir: &std::path::Path, nivel: u32, v: &mut Vec<std::path::PathBuf>) {
    let Ok(it) = std::fs::read_dir(dir) else { return };
    for e in it.flatten() {
        let p = e.path();
        if p.is_dir() && nivel < 3 {
            archivos_lua(&p, nivel + 1, v);
        } else if p.extension().is_some_and(|x| x == "lua") {
            v.push(p);
        }
    }
}

/// Síncrono: las consultas de GIO tardan milisegundos.
pub fn aplicaciones(capturas: &crate::config::Capturas) -> Vec<Seccion> {
    let pred: Vec<sistema::Predeterminada> = sistema::PREDETERMINADAS
        .iter()
        .map(|(_, mimes)| {
            let mime = mimes[0];
            let mut candidatas: Vec<sistema::App> = Vec::new();
            for a in gio::AppInfo::all_for_type(mime) {
                let Some(id) = a.id() else { continue };
                if !a.should_show() || candidatas.iter().any(|c| c.id == id) {
                    continue;
                }
                candidatas.push(sistema::App {
                    id: id.into(),
                    nombre: a.display_name().into(),
                });
            }
            // La puesta puede ser una app escondida del launcher (`NoDisplay`, como
            // `kitty-open`): igual va en la lista, con su nombre.
            let puesta = gio::AppInfo::default_for_type(mime, false);
            if let Some(p) = &puesta
                && let Some(id) = p.id()
                && !candidatas.iter().any(|c| c.id == id)
            {
                candidatas.push(sistema::App {
                    id: id.into(),
                    nombre: p.display_name().into(),
                });
            }
            candidatas.sort_by_key(|c| c.nombre.to_lowercase());
            sistema::Predeterminada {
                candidatas,
                puesta: puesta.and_then(|a| a.id()).map(Into::into),
            }
        })
        .collect();
    let imagenes = glib::user_special_dir(glib::UserDirectory::Pictures)
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    sistema::aplicaciones(&pred, &autoarranque(), capturas, &imagenes)
}

const AUTOSTART_SISTEMA: &str = "/etc/xdg/autostart";

fn autostart_usuario() -> PathBuf {
    glib::user_config_dir().join("autostart")
}

fn autoarranque() -> Vec<sistema::Autoarranque> {
    let mut archivos: Vec<String> = Vec::new();
    for dir in [PathBuf::from(AUTOSTART_SISTEMA), autostart_usuario()] {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.ends_with(".desktop") && !archivos.contains(&n) {
                archivos.push(n);
            }
        }
    }
    let escritorios: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .map(str::to_lowercase)
        .collect();
    let lista = |kf: &glib::KeyFile, k: &str| -> Option<Vec<String>> {
        kf.string_list("Desktop Entry", k)
            .ok()
            .map(|v| v.iter().map(|s| s.to_lowercase()).collect())
    };
    let mut v: Vec<sistema::Autoarranque> = archivos
        .into_iter()
        .filter_map(|archivo| {
            let usuario = autostart_usuario().join(&archivo);
            let ruta = if usuario.exists() {
                usuario
            } else {
                PathBuf::from(AUTOSTART_SISTEMA).join(&archivo)
            };
            let kf = glib::KeyFile::new();
            kf.load_from_file(&ruta, glib::KeyFileFlags::NONE).ok()?;
            let le_toca = lista(&kf, "OnlyShowIn")
                .is_none_or(|l| l.iter().any(|d| escritorios.contains(d)))
                && !lista(&kf, "NotShowIn")
                    .is_some_and(|l| l.iter().any(|d| escritorios.contains(d)));
            if !le_toca {
                return None;
            }
            let verdad = |k: &str| kf.boolean("Desktop Entry", k).ok();
            let activo = verdad("Hidden") != Some(true)
                && verdad("X-GNOME-Autostart-enabled") != Some(false);
            Some(sistema::Autoarranque {
                nombre: kf
                    .locale_string("Desktop Entry", "Name", None)
                    .map(Into::into)
                    .unwrap_or_else(|_| archivo.trim_end_matches(".desktop").to_owned()),
                comentario: kf
                    .locale_string("Desktop Entry", "Comment", None)
                    .map(Into::into)
                    .unwrap_or_default(),
                archivo,
                activo,
            })
        })
        .collect();
    v.sort_by_key(|a| a.nombre.to_lowercase());
    v
}

/// Con `Hidden`, que respeta el generador de systemd que usa uwsm.
pub fn poner_autoarranque(archivo: &str, activar: bool) -> Result<(), String> {
    let usuario = autostart_usuario().join(archivo);
    let origen = if usuario.exists() {
        usuario.clone()
    } else {
        PathBuf::from(AUTOSTART_SISTEMA).join(archivo)
    };
    let kf = glib::KeyFile::new();
    kf.load_from_file(
        &origen,
        glib::KeyFileFlags::KEEP_COMMENTS | glib::KeyFileFlags::KEEP_TRANSLATIONS,
    )
    .map_err(|e| e.to_string())?;
    kf.set_boolean("Desktop Entry", "Hidden", !activar);
    if kf
        .has_key("Desktop Entry", "X-GNOME-Autostart-enabled")
        .unwrap_or(false)
    {
        kf.set_boolean("Desktop Entry", "X-GNOME-Autostart-enabled", activar);
    }
    std::fs::create_dir_all(autostart_usuario()).map_err(|e| e.to_string())?;
    kf.save_to_file(&usuario).map_err(|e| e.to_string())
}

pub async fn sistema() -> Vec<Seccion> {
    let archivo = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
    let una = |p: &str| archivo(p).trim().to_owned();
    let equipo = [
        una("/sys/devices/virtual/dmi/id/sys_vendor"),
        una("/sys/devices/virtual/dmi/id/product_name"),
    ]
    .join(" ")
    .trim()
    .to_owned();
    let placa = [
        una("/sys/devices/virtual/dmi/id/board_vendor"),
        una("/sys/devices/virtual/dmi/id/board_name"),
    ]
    .join(" ")
    .trim()
    .to_owned();
    let mut s = comando::salidas(&[
        &["hyprctl", "version", "-j"],
        &["lspci", "-mm"],
        &["cliphist", "list"],
        &[
            "snapper",
            "--csvout",
            "--no-headers",
            "-c",
            "root",
            "list",
            "--disable-used-space",
            "--columns",
            sistema::COLUMNAS_SNAPPER,
        ],
    ])
    .await
    .into_iter();
    let mut sig = || s.next().and_then(Result::ok);
    let (version, lspci, clip) = (
        sig().unwrap_or_default(),
        sig().unwrap_or_default(),
        sig().unwrap_or_default(),
    );
    // Sin permiso, snapper sale con error ("Sin permisos.").
    let snapshots = sistema::snapshots(sig().as_deref());
    let hyprland = serde_json::from_str::<serde_json::Value>(&version)
        .ok()
        .and_then(|v| v.get("tag")?.as_str().map(str::to_owned))
        .unwrap_or_default();
    // En una PC armada, el equipo es la placa: se dice una vez.
    let placa = if placa == equipo { String::new() } else { placa };
    let equipo = sistema::equipo(&[
        ("Nombre", una("/proc/sys/kernel/hostname")),
        ("Equipo", equipo),
        ("Placa madre", placa),
        ("Procesador", sistema::procesador(&archivo("/proc/cpuinfo"))),
        ("Gráficos", sistema::graficas(&lspci)),
        ("Memoria", sistema::memoria(&archivo("/proc/meminfo"))),
        ("Sistema", sistema::distribucion(&archivo("/etc/os-release"))),
        ("Kernel", una("/proc/sys/kernel/osrelease")),
        ("Hyprland", hyprland),
        ("morgan", env!("CARGO_PKG_VERSION").to_owned()),
        ("Encendida hace", sistema::encendida(&archivo("/proc/uptime"))),
    ]);
    let recientes = gtk::RecentManager::default().items().len();
    let privacidad =
        arbol::privacidad(clip.lines().filter(|l| !l.is_empty()).count(), recientes);
    arbol::armar(arbol::Categoria::Sistema, vec![equipo, snapshots, privacidad])
}

fn con_tilde(ruta: &str, home: &std::path::Path) -> String {
    match std::path::Path::new(ruta).strip_prefix(home) {
        Ok(r) if r.as_os_str().is_empty() => "~".into(),
        Ok(r) => format!("~/{}", r.display()),
        Err(_) => ruta.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acorta_el_home() {
        let h = std::path::Path::new("/home/a");
        assert_eq!(con_tilde("/home/a/Imágenes/walls", h), "~/Imágenes/walls");
        assert_eq!(con_tilde("/home/a", h), "~");
        assert_eq!(con_tilde("/home/ab", h), "/home/ab");
        assert_eq!(con_tilde("/mnt/x", h), "/mnt/x");
    }
}
