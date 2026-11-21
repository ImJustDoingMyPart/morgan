//! La parte de `morgan setup` que ofrece los atajos de ventanas (`atajos_base`): un archivo por
//! compositor, al lado de la config del usuario. Se escribe una vez y no se pisa: si ya existe,
//! es de quien lo editó. `--deshacer` lo aparta como `.bak`.

use std::path::{Path, PathBuf};

use morgan_idioma::{tr, trf};

use crate::atajos_base::{self, Opciones};
use crate::setup_umbriel;

pub const LUA: &str = "morgan-atajos.lua"; // no-tr
pub const TOML: &str = "morgan-atajos.toml"; // no-tr

#[derive(Clone, Copy, PartialEq)]
pub enum Cual {
    Hypr,
    Umbriel,
}

/// Un archivo que se puede escribir: de qué compositor y dónde.
pub struct Destino {
    pub cual: Cual,
    pub ruta: PathBuf,
}

impl Destino {
    pub fn resumen(&self) -> String {
        let quien = match self.cual {
            Cual::Hypr => "Hyprland",   // no-tr
            Cual::Umbriel => "Umbriel", // no-tr
        };
        trf!("Atajos de ventanas de morgan para {}: {}", quien, self.ruta.display())
    }
}

fn dir_umbriel() -> PathBuf {
    glib::user_config_dir().join("umbriel")
}

/// Los archivos que faltan: donde ya está el archivo, el usuario ya eligió (y pudo editarlo).
pub fn pendientes(hypr_dir: &Path, hay_hypr: bool, umbriel: bool) -> Vec<Destino> {
    let mut v = Vec::new();
    if hay_hypr && !hypr_dir.join(LUA).exists() {
        v.push(Destino { cual: Cual::Hypr, ruta: hypr_dir.join(LUA) });
    }
    if umbriel && !dir_umbriel().join(TOML).exists() {
        v.push(Destino { cual: Cual::Umbriel, ruta: dir_umbriel().join(TOML) });
    }
    v
}

/// Las teclas de Umbriel que se llevan los atajos: la plantilla del paquete comenta las suyas.
pub fn teclas_umbriel(elegidos: &[&Destino]) -> Vec<String> {
    if elegidos.iter().any(|d| d.cual == Cual::Umbriel) {
        atajos_base::teclas_umbriel(&Opciones::de_la_maquina())
    } else {
        Vec::new()
    }
}

/// Escribe los archivos elegidos. En Hyprland deja afuera las teclas que el usuario ya usa en
/// su config y las lista; en Umbriel no hace falta, porque `user.toml` se carga después.
pub fn instalar(elegidos: &[&Destino], hypr_dir: &Path, seco: bool) -> Result<(), String> {
    let o = Opciones::de_la_maquina();
    for d in elegidos {
        let (texto, propias) = match d.cual {
            Cual::Hypr => {
                let ya = atajos_base::teclas_de_usuario_hypr(hypr_dir);
                let propias = atajos_base::tabla(&o)
                    .into_iter()
                    .filter(|b| b.hypr.is_some())
                    .filter(|b| ya.contains(&crate::setup_umbriel::normalizar(&b.tecla)))
                    .map(|b| b.tecla)
                    .collect::<Vec<_>>();
                (atajos_base::lua(&o, &ya), propias)
            }
            Cual::Umbriel => (atajos_base::toml(&o), Vec::new()),
        };
        if seco {
            println!("  {}", trf!("Haría: escribir {}.", d.ruta.display()));
        } else {
            if let Some(dir) = d.ruta.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            std::fs::write(&d.ruta, texto).map_err(|e| format!("{}: {e}", d.ruta.display()))?;
            println!("  {}", trf!("Escrito {}.", d.ruta.display()));
        }
        if !propias.is_empty() {
            println!("  ! {}", trf!("Ya tenés estas teclas en tu config y no se pisaron: {}.", propias.join(", ")));
        }
    }
    if !seco && elegidos.iter().any(|d| d.cual == Cual::Umbriel) {
        cargar_en_umbriel()?;
    }
    Ok(())
}

/// Un cargador hecho antes de que existieran los atajos no los incluye: se agrega el archivo
/// antes que `user.toml`, para que lo del usuario lo pise.
fn cargar_en_umbriel() -> Result<(), String> {
    let ruta = dir_umbriel().join("config.toml");
    let Ok(texto) = std::fs::read_to_string(&ruta) else { return Ok(()) };
    match setup_umbriel::con_atajos(&texto) {
        Some(nuevo) => std::fs::write(&ruta, nuevo).map_err(|e| format!("{}: {e}", ruta.display())),
        None => Ok(()),
    }
}

/// Aparta los archivos de atajos (`.bak`), por si el usuario los editó.
pub fn deshacer(hypr_dir: &Path, seco: bool) {
    deshacer_en(hypr_dir, &dir_umbriel(), seco);
}

fn deshacer_en(hypr_dir: &Path, umbriel_dir: &Path, seco: bool) {
    for ruta in [hypr_dir.join(LUA), umbriel_dir.join(TOML)] {
        if !ruta.is_file() {
            continue;
        }
        let bak = ruta.with_extension(format!("{}.bak", ruta.extension().and_then(|e| e.to_str()).unwrap_or("")));
        println!("  {}: {}", if seco { tr!("haría") } else { tr!("hecho") }, trf!("mover {} a {}", ruta.display(), bak.display())); // no-tr
        if !seco {
            let _ = std::fs::rename(&ruta, &bak);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyprland_escribe_el_archivo_sin_pisar_lo_del_usuario() {
        let dir = std::env::temp_dir().join(format!("morgan-atajos-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("cfg")).unwrap();
        std::fs::write(
            dir.join("cfg/binds.lua"),
            "local mod = \"SUPER\"\nhl.bind(mod .. \" + Q\", algo)\n-- hl.bind(\"SUPER + W\", x)\nhl.bind(\"super + shift + Left\", otra)\n",
        )
        .unwrap();
        let pendientes = pendientes(&dir, true, false);
        assert_eq!(pendientes.len(), 1);
        let refs: Vec<&Destino> = pendientes.iter().collect();
        instalar(&refs, &dir, false).unwrap();
        let lua = std::fs::read_to_string(dir.join(LUA)).unwrap();
        assert!(!lua.contains("\"SUPER + Q\""), "Mod+Q es del usuario");
        assert!(!lua.contains("\"SUPER + SHIFT + Left\""), "la suya, en otro orden y en minúscula");
        assert!(lua.contains("\"SUPER + W\""), "la comentada no cuenta");
        assert!(lua.contains("\"SUPER + H\""));
        // Ya escrito: no se vuelve a ofrecer.
        assert!(super::pendientes(&dir, true, false).is_empty());
        // Deshacer lo aparta con .bak.
        deshacer_en(&dir, &dir, false);
        assert!(dir.join("morgan-atajos.lua.bak").is_file() && !dir.join(LUA).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
