//! Los nombres de archivos y carpetas de `~/.config/morgan` que eran castellano hasta 0.1.1
//! pasaron al inglés, como el resto de la API pública. Se renombran una vez al arrancar, si
//! el nombre nuevo todavía no existe; lo de adentro (las claves viejas de un `palette.toml` o
//! de `themes.toml`) se sigue leyendo tal cual.

use std::path::Path;

const RENOMBRES: [(&str, &str); 5] = [
    ("temas", "themes"),
    ("plantillas", "templates"),
    ("calendarios", "calendars"),
    ("temas.toml", "themes.toml"),
    ("monitores.toml", "monitors.toml"),
];

/// Devuelve lo que renombró, para el journal.
pub fn nombres_viejos(config: &Path) -> Vec<String> {
    let mut hecho = Vec::new();
    let mut renombrar = |viejo: &Path, nuevo: &Path| {
        if viejo.exists() && !nuevo.exists() {
            match std::fs::rename(viejo, nuevo) {
                Ok(()) => hecho.push(format!("{} → {}", viejo.display(), nuevo.display())),
                Err(e) => eprintln!("morgan: no se pudo renombrar {}: {e}", viejo.display()),
            }
        }
    };
    for (viejo, nuevo) in RENOMBRES {
        renombrar(&config.join(viejo), &config.join(nuevo));
    }
    if let Ok(temas) = std::fs::read_dir(config.join("themes")) {
        for t in temas.flatten() {
            renombrar(&t.path().join("paleta.toml"), &t.path().join("palette.toml"));
        }
    }
    hecho
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renombra_lo_viejo_sin_pisar_lo_nuevo() {
        let d = std::env::temp_dir().join(format!("morgan-migracion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("temas/mio")).unwrap();
        std::fs::write(d.join("temas/mio/paleta.toml"), "fondo0 = \"#000000\"").unwrap();
        std::fs::create_dir_all(d.join("plantillas")).unwrap();
        std::fs::write(d.join("calendarios"), "id\turl\n").unwrap();
        std::fs::write(d.join("temas.toml"), "apagadas = []").unwrap();
        std::fs::write(d.join("monitores.toml"), "[DP-1]\n").unwrap();
        // Ya hay uno nuevo: el viejo queda donde está.
        std::fs::write(d.join("themes.toml"), "disabled = []").unwrap();

        let hecho = nombres_viejos(&d);
        assert_eq!(hecho.len(), 5, "{hecho:?}");
        assert!(d.join("monitors.toml").is_file());
        assert!(d.join("themes/mio/palette.toml").is_file());
        assert!(d.join("templates").is_dir());
        assert!(d.join("calendars").is_file());
        assert!(d.join("temas.toml").is_file());
        assert_eq!(std::fs::read_to_string(d.join("themes.toml")).unwrap(), "disabled = []");
        // La segunda vez no hay nada que hacer.
        assert!(nombres_viejos(&d).is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
