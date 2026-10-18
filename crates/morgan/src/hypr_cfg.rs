//! Se escribe solo si el texto cambió; antes de recargar se verifica la config entera, y si
//! queda inválida vuelve el archivo de antes.

use std::path::{Path, PathBuf};

use crate::animaciones;
use crate::comando;
use crate::config::Config;
use crate::menu::compositor::Cambios;

/// El color del glow no se declara: queda el que pone el tema, que se carga antes. La negra
/// lo pisa.
pub const SOMBRAS: &[(&str, &str, &str, &str)] = &[
    (
        "black",
        "Sombra negra",
        "ceñida, solo en la ventana con foco",
        "hl.config({ decoration = { shadow = {\n    enabled = true, range = 6, render_power = 2,\n    \
         color = \"rgba(000000ff)\", color_inactive = \"rgba(00000000)\", offset = { 0, 0 },\n} } })\n",
    ),
    (
        "glow",
        "Glow colorido",
        "el degradé del tema, gira con el borde",
        // `range` en px lógicos; `render_power 3` concentra la luz pegada al borde.
        "hl.config({ decoration = { shadow = {\n    enabled = true, range = 4, render_power = 3,\n    \
         color_inactive = \"rgba(00000000)\", offset = { 0, 0 },\n} } })\n",
    ),
];

fn sombra(id: &str) -> &'static (&'static str, &'static str, &'static str, &'static str) {
    SOMBRAS.iter().find(|s| s.0 == id).unwrap_or(&SOMBRAS[0])
}

/// `MORGAN_HYPR_CFG` lo cambia, para que un Hyprland anidado de pruebas no toque la config
/// real.
pub fn ruta() -> PathBuf {
    std::env::var_os("MORGAN_HYPR_CFG")
        .map(PathBuf::from)
        .unwrap_or_else(|| gtk::glib::user_config_dir().join("hypr/morgan.lua"))
}

fn colores_tema() -> PathBuf {
    morgan_temas::Rutas::del_entorno().render().join("hyprland.lua")
}

pub fn ruta_cambios(ruta_config: &Path) -> PathBuf {
    ruta_config.with_file_name("hyprland.toml")
}

/// Blur con `ignore_alpha` 0,5: cada capa cubre la salida y el umbral recorta el desenfoque a
/// lo que se ve. Las que baja morgan (`bajada.rs`) entran con `fade`: `slide` correría la capa
/// entera, que cubre la salida, y lo que se ve asomaría tarde.
const CAPAS: &str = r#"-- Las capas de morgan: blur recortado a lo que se ve, y cómo entra cada una.
for ns, regla in pairs({
    ["morgan-centro"]         = { animation = "fade",         ignore_alpha = 0.5 },
    ["morgan-calendario"]     = { animation = "fade",         ignore_alpha = 0.5 },
    ["morgan-bandeja"]        = { animation = "fade",         ignore_alpha = 0.5 },
    ["morgan-calculadora"]    = { animation = "fade",         ignore_alpha = 0.5 },
    ["morgan-notificaciones"] = { animation = "fade",         ignore_alpha = 0.5 },
    ["morgan-notas"]          = { animation = "fade",         ignore_alpha = 0.5 },
    ["morgan-osd"]            = { animation = "slide bottom", ignore_alpha = 0.5 },
    ["morgan-launcher"]       = { animation = "fade",         ignore_alpha = 0.5 },
    ["morgan-menu"]           = { animation = "fade",         ignore_alpha = 0.5 },
    ["morgan-fondos"]         = { animation = "slide bottom", ignore_alpha = 0.5 },
    ["morgan-ventanas"]       = { animation = "fade",         ignore_alpha = 0.5 },
    ["morgan-barra"]          = {},
}) do
    hl.layer_rule({
        name         = ns,
        match        = { namespace = "^" .. ns .. "$" },
        animation    = regla.animation,
        blur         = true,
        xray         = false,
        ignore_alpha = regla.ignore_alpha,
    })
end
"#;

/// Con uwsm la unidad ya arrancó con la sesión y esto no hace nada; sin uwsm, el gestor de
/// systemd no sabe de la sesión gráfica y esto la arranca. Un Hyprland anidado de pruebas no lo
/// lleva: pisaría el entorno de la sesión de afuera.
fn arranque() -> String {
    if std::env::var_os("MORGAN_HYPR_CFG").is_some() {
        return String::new();
    }
    format!(
        "-- Arranque de morgan.\nhl.on(\"hyprland.start\", function()\n    hl.exec_cmd(\"systemctl --user import-environment {}; systemctl --user start morgan.service\")\nend)\n\n", // no-tr
        crate::setup::VARIABLES.join(" ")
    )
}

/// Los colores del tema van primero: la sombra los usa.
pub fn lua(c: &Config, cambios: &Cambios, tema: &Path, hyprshot: bool) -> String {
    let (_, nombre, _, lua_sombra) = sombra(&c.apariencia.sombra);
    let binds = crate::launcher::binds::lua(&c.atajos, &c.capturas, hyprshot);
    format!(
        "-- Lo genera morgan: no se edita a mano, se reescribe entero. Lo guardado está en\n\
         -- morgan/config.toml y en morgan/hyprland.toml; se cambia desde su menú de sistema.\n\n\
         {}\
         -- Colores del tema activo (morganctl theme).\n\
         pcall(dofile, {:?})\n\n\
         {CAPAS}\n\
         {}\n-- Sombra: {nombre}.\n{lua_sombra}\n{}{binds}",
        arranque(),
        tema.to_string_lossy(),
        animaciones::lua(
            animaciones::por_id(&c.apariencia.animacion),
            animaciones::velocidad(&c.apariencia.velocidad),
        ),
        cambios.lua(),
    )
}

pub async fn aplicar(c: &Config, ruta_config: &Path) -> Result<(), String> {
    let cambios = Cambios::leer(&ruta_cambios(ruta_config))?;
    let destino = ruta();
    let hyprshot = gtk::glib::find_program_in_path("hyprshot").is_some();
    let nuevo = lua(c, &cambios, &colores_tema(), hyprshot);
    let antes = std::fs::read_to_string(&destino).ok();
    if antes.as_deref() == Some(nuevo.as_str()) {
        return Ok(());
    }
    escribir(&destino, &nuevo)?;
    if comando::salida(&["Hyprland", "--verify-config"]).await.is_err() {
        let _ = match &antes {
            Some(a) => escribir(&destino, a),
            None => std::fs::remove_file(&destino).map_err(|e| e.to_string()),
        };
        return Err("la config de Hyprland quedó inválida; se dejó la de antes".into());
    }
    // `config-only`: sin eso el reload reaplica los monitores y una salida apagada se prende un
    // instante.
    comando::salida(&["hyprctl", "reload", "config-only"])
        .await
        .map(drop)
}

fn escribir(ruta: &Path, texto: &str) -> Result<(), String> {
    if let Some(dir) = ruta.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // Primero a un temporal y después el cambio de nombre: Hyprland nunca lee uno a medias.
    let tmp = ruta.with_extension("tmp");
    std::fs::write(&tmp, texto).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, ruta).map_err(|e| format!("{}: {e}", ruta.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animacion_sombra_y_compositor_en_ese_orden() {
        let mut c = Config::default();
        c.apariencia.sombra = "glow".into();
        let lua = lua(&c, &Cambios::default(), Path::new("/e/tema/hyprland.lua"), true);
        let anim = lua.find("-- Animación").unwrap();
        let sombra = lua.find("-- Sombra: Glow colorido").unwrap();
        assert!(anim < sombra);
        assert!(!lua[sombra..].contains("rgba(000000ff)"));
        // Lo que ya no existe cae en el default.
        c.apariencia = crate::config::Apariencia {
            animacion: "x".into(),
            velocidad: "y".into(),
            sombra: "black".into(),
        };
        let lua = super::lua(&c, &Cambios::default(), Path::new("/t"), false);
        assert!(lua.contains("-- Animación: Precisa (Crece y se desliza), velocidad Media"));
        assert!(lua.contains("-- Sombra: Sombra negra"));
        assert!(!lua.contains("hyprshot"), "sin hyprshot, sin binds de captura");
    }

    #[test]
    fn los_colores_del_tema_primero_y_las_capturas_al_final() {
        let lua = lua(&Config::default(), &Cambios::default(), Path::new("/e/tema/hyprland.lua"), true);
        let tema = lua.find(r#"pcall(dofile, "/e/tema/hyprland.lua")"#).unwrap();
        assert!(tema < lua.find("-- Animación").unwrap());
        assert!(lua.find("-- Sombra").unwrap() < lua.find("atajo(\"Print\", ").unwrap());
    }
}
