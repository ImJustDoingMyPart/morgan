//! La config de morgan para Umbriel, en `~/.config/umbriel/morgan.toml`: sus atajos, las
//! reglas de sus capas, el tema, el borde y la sombra, y lo cambiado desde el menú (Escritorio
//! y monitores). Umbriel la relee sola al cambiar el archivo y, si queda inválida, sigue con la
//! última buena; igual se valida antes con `umbriel config validate` y, si falla, queda la de
//! antes.
//!
//! La config de Umbriel es un cargador (`morgan setup`): incluye `user.toml`, la escrita a
//! mano, y después `morgan.toml`. Los incluidos se aplican en el orden de la lista y el último
//! que pone una clave gana (`configuration.md`, «Include»): lo de morgan pisa lo del usuario,
//! como `morgan.lua` en Hyprland. Por eso de lo cambiado desde el menú va solo lo cambiado.

use std::path::{Path, PathBuf};

use crate::animaciones;
use crate::config::{AtajoPropio, Config};
use crate::menu::compositor::Cambios;
use crate::menu::monitores::{Campo, Reglas};
use crate::launcher::binds;

/// `MORGAN_UMBRIEL_CFG` lo cambia, para que un Umbriel anidado de pruebas no toque la config
/// real.
pub fn ruta() -> PathBuf {
    std::env::var_os("MORGAN_UMBRIEL_CFG")
        .map(PathBuf::from)
        .unwrap_or_else(|| glib::user_config_dir().join("umbriel/morgan.toml"))
}

/// Las capas que se abren sobre ventanas desenfocan lo que tienen detrás; la barra, solo el
/// fondo (el desenfoque optimizado, más barato).
const CAPAS: &str = r#"
# Las capas de morgan: desenfoque recortado a lo que se ve.
[[layer_rule]]
match.namespace = "^morgan-"
blur = true
blur_popups = true
blur_ignore_alpha = 0.5

[[layer_rule]]
match.namespace = "^morgan-(center|calendar|tray|calculator|notifications|osd|launcher|polkit|menu|wallpapers|windows|dock|dock-globo)$"
blur_optimized = false
"#; // no-tr

/// Lo cambiado desde Escritorio con Umbriel. Las claves no son las de Hyprland
/// (`hyprland.toml`): cada compositor guarda lo suyo.
pub fn ruta_cambios(ruta_config: &Path) -> PathBuf {
    ruta_config.with_file_name("umbriel.toml")
}

/// El shader del borde, al lado de `morgan.toml`: el preset lo nombra relativo a ese archivo.
const SHADER: &str = "morgan-borde.glsl"; // no-tr
const PRESET: &str = "morgan-borde"; // no-tr

/// `SUPER + ALT + L` de Hyprland a `Super+Alt+L`. `None` para lo que Umbriel no tiene igual
/// (un código de tecla, un botón del mouse con nombre de Hyprland).
pub fn tecla(hypr: &str) -> Option<String> {
    let mut partes = Vec::new();
    for p in hypr.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        let nombre = match p.to_uppercase().as_str() {
            "SUPER" | "MOD4" | "WIN" | "META" => "Super".to_owned(), // no-tr
            "CTRL" | "CONTROL" => "Ctrl".to_owned(),                 // no-tr
            "ALT" | "MOD1" => "Alt".to_owned(),                      // no-tr
            "SHIFT" => "Shift".to_owned(),                           // no-tr
            _ if p.contains(':') => return None,
            _ => p.to_owned(),
        };
        partes.push(nombre);
    }
    (!partes.is_empty()).then(|| partes.join("+"))
}

/// Una cadena básica de TOML en una línea (las de varias no van en una tabla en línea). Las
/// secuencias de escape de JSON valen en TOML.
fn cadena(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

fn bind(tecla_hypr: &str, accion: &str, extra: &[&str]) -> Option<String> {
    let t = tecla(tecla_hypr)?;
    let mut campos = vec![format!("action = {}", cadena(accion))]; // no-tr
    campos.extend(extra.iter().map(|e| (*e).to_owned()));
    Some(format!("{} = {{ {} }}\n", cadena(&t), campos.join(", ")))
}

fn accion_propio(p: &AtajoPropio) -> Option<String> {
    if !p.app.is_empty() {
        let app = crate::aplicacion::buscar(&p.app)?;
        return Some(format!("spawn {}", app.lanzar)); // no-tr
    }
    let cmd = p.command.trim();
    (!cmd.is_empty()).then(|| format!("spawn {cmd}")) // no-tr
}

fn extra_propio(p: &AtajoPropio) -> Vec<&'static str> {
    let mut v = Vec::new();
    if !p.repeat {
        v.push("repeat = false"); // no-tr
    }
    if p.locked {
        v.push("allow_when_locked = true"); // no-tr
    }
    v
}

/// Los colores del tema (`templates/umbriel.toml`, renderizado por el motor de temas).
fn colores_tema() -> PathBuf {
    morgan_temas::Rutas::del_entorno()
        .render()
        .join("umbriel.toml")
}

/// Lo que ocupa la sombra afuera del anillo, en px lógicos: el `padding` del efecto. La negra
/// (spread 1, sigma 3) baja de 0,001 a los 10 px; el glow (offset 1, spread 1, sigma 2,5), a
/// los 9,5.
const MARGEN: u32 = 10;

/// Si la sombra elegida en el menú (Apariencia) es el glow. Las dos copian la de los paneles
/// (`morgan_motor::forma::panel`), no la de Hyprland (`hypr_cfg::SOMBRAS`).
fn glow(c: &Config) -> bool {
    c.apariencia.sombra == "glow" // no-tr
}

/// El shader del borde con la sombra elegida. GLSL ES 1.00 no convierte enteros a float: van
/// con `.0`.
pub fn shader(c: &Config) -> String {
    include_str!("umbriel_borde.glsl")
        .replace("@MARGEN@", &format!("{MARGEN}.0"))
        .replace("@GLOW@", if glow(c) { "1.0" } else { "0.0" }) // no-tr
}

fn tabla(texto: &str) -> toml::Table {
    toml::from_str(texto).expect("TOML de morgan")
}

/// La animación elegida en el menú (Apariencia), con lo que Umbriel tiene: sin `popin` a
/// medida; las curvas son las mismas de Hyprland (`animaciones.rs`). El borde en degradé y la
/// sombra los dibuja el efecto (`umbriel_borde.glsl`): la sombra nativa es de un color y va en
/// todas las ventanas, y en Hyprland solo la tiene la del foco.
fn apariencia(c: &Config) -> toml::Table {
    let comp = animaciones::por_id(&c.apariencia.animacion);
    let t = animaciones::velocidad(&c.apariencia.velocidad).tiempos();
    let estilo = match comp.id {
        "fold" => "zoom",   // no-tr
        "fade" => "fade",   // no-tr
        _ => "popin",       // no-tr
    };
    tabla(&format!(
        "[animation]\nenabled = true\n\n\
         [animation.beziers]\n\
         morgan_decel = [0.05, 0.7, 0.1, 1.0]\n\
         morgan_expo = [0.16, 1.0, 0.3, 1.0]\n\
         morgan_rebote = [0.34, 1.3, 0.64, 1.0]\n\n\
         [animation.windows_in]\nenabled = true\nstyle = \"{estilo}\"\ncurve = \"{c_v}\"\nduration_ms = {e}\n\n\
         [animation.windows_out]\nenabled = true\nstyle = \"fade\"\ncurve = \"morgan_decel\"\nduration_ms = {s}\n\n\
         [animation.windows_move]\nenabled = true\ncurve = \"{c_v}\"\nduration_ms = {m}\n\n\
         [animation.workspaces]\nenabled = true\nstyle = \"slide\"\ncurve = \"{c_w}\"\nduration_ms = {w}\n\n\
         [animation.layers]\nenabled = true\ncurve = \"morgan_decel\"\nduration_ms = {e}\n\n\
         [appearance.shadow]\nenabled = false\n\n\
         [effects.preset.{PRESET}]\nkind = \"border\"\nshader = \"{SHADER}\"\npalette = true\nanimated = false\npadding = {MARGEN}\n\n\
         [effects]\nborder = \"{PRESET}\"\n", // no-tr
        c_v = comp.curva,
        c_w = comp.curva_workspace,
        e = t.entrar,
        s = t.salir,
        m = t.mover,
        w = t.workspace,
    ))
}

/// `[output."DP-3"]` por salida con solo lo cambiado desde el menú: las tablas se juntan por
/// clave, así que lo demás de la salida en `user.toml` (VRR, ejes) sigue valiendo.
fn salidas(reglas: &Reglas) -> toml::Table {
    let mut out = toml::Table::new();
    for (nombre, r) in &reglas.0 {
        let mut t = toml::Table::new();
        if let Some(m) = &r.mode {
            t.insert("mode".into(), m.clone().into()); // no-tr
        }
        if let Some((x, y)) = r.position.as_deref().and_then(|p| p.split_once('x'))
            && let (Ok(x), Ok(y)) = (x.parse::<i64>(), y.parse::<i64>())
        {
            t.insert("position".into(), vec![x, y].into()); // no-tr
        }
        if let Some(e) = r.scale {
            t.insert("scale".into(), e.into()); // no-tr
        }
        if let Some(g) = r.transform {
            let g = match g % 4 {
                1 => "90",     // no-tr
                2 => "180",    // no-tr
                3 => "270",    // no-tr
                _ => "normal", // no-tr
            };
            t.insert("transform".into(), g.into()); // no-tr
        }
        if let Some(d) = r.disabled {
            t.insert("enabled".into(), (!d).into()); // no-tr
        }
        if !t.is_empty() {
            out.insert(nombre.clone(), t.into());
        }
    }
    let mut raiz = toml::Table::new();
    if !out.is_empty() {
        raiz.insert("output".into(), out.into()); // no-tr
    }
    raiz
}

/// Lo que morgan lee para escribir su archivo, fuera de su config.
#[derive(Default)]
pub struct Menu {
    pub cambios: Cambios,
    pub monitores: Reglas,
    /// El tema de cursor y su tamaño (Apariencia), si el tema está instalado. Umbriel lo toma
    /// de `[input.cursor]` y lo cambia al releer: no hay `hyprctl setcursor`.
    pub cursor: Option<(String, u32)>,
}

impl Menu {
    pub fn leer(ruta_config: &Path) -> Result<Menu, String> {
        let (tema, tamano) = crate::apariencia::cursores::actual();
        let instalado = crate::apariencia::cursores::listar().contains(&tema);
        Ok(Menu {
            cambios: Cambios::leer(&ruta_cambios(ruta_config))?,
            monitores: Reglas::leer(&crate::hypr_cfg::ruta_monitores(ruta_config))?,
            cursor: instalado.then_some((tema, tamano)),
        })
    }
}

fn cursor(c: &(String, u32)) -> toml::Table {
    tabla(&format!("[input.cursor]\ntheme = {}\nsize = {}\n", cadena(&c.0), c.1)) // no-tr
}

pub fn toml(c: &Config, con_grim: bool) -> String {
    toml_con(c, con_grim, &colores_tema(), &Menu::default())
}

pub fn toml_con(c: &Config, con_grim: bool, tema: &Path, menu: &Menu) -> String {
    let a = &c.atajos;
    let mut out = String::from(concat!(
        "# morgan: sus atajos, las reglas de sus capas, el tema, el borde y la sombra, y lo\n", // no-tr
        "# cambiado desde su menú. Lo escribe morgan con cada cambio de su config\n", // no-tr
        "# (morgan/config.toml, umbriel.toml, monitors.toml): no editar a mano. Va último en\n", // no-tr
        "# ~/.config/umbriel/config.toml, así que gana sobre user.toml.\n\n", // no-tr
    ));
    let mut t = toml::Table::new();
    t.insert(
        "include".into(), // no-tr
        tabla(&format!(
            "[optional]\nfiles = [{}]\n", // no-tr
            cadena(&tema.to_string_lossy())
        ))
        .into(),
    );
    crate::umbriel_valores::juntar(&mut t, apariencia(c));
    crate::umbriel_valores::juntar(&mut t, menu.cambios.tabla_toml());
    crate::umbriel_valores::juntar(&mut t, salidas(&menu.monitores));
    if let Some(c) = &menu.cursor {
        crate::umbriel_valores::juntar(&mut t, cursor(c));
    }
    // Umbriel trae su overview y morgan no arma el lienzo: la esquina de `[shell] hot_corner`
    // abre el de Umbriel. Apagada no se escribe, así queda la que ponga user.toml.
    if c.shell.esquina {
        crate::umbriel_valores::juntar(
            &mut t,
            tabla("[hot_corners.top_left]\nenabled = true\naction = \"overview-toggle\"\n"), // no-tr
        );
    }
    out.push_str(&toml::to_string(&t).expect("TOML de morgan"));
    out.push_str("\n[keybinds]\n"); // no-tr
    for (cabecera, lista) in binds::acciones(a) {
        out.push_str(&format!("# {cabecera}\n"));
        for (_, t, cmd, _, locked) in lista {
            let extra: &[&str] = if locked {
                &["repeat = false", "allow_when_locked = true"] // no-tr
            } else {
                &["repeat = false"] // no-tr
            };
            if let Some(b) = bind(t, &format!("spawn {cmd}"), extra) { // no-tr
                out.push_str(&b);
            }
        }
    }
    // La tecla de ventanas abre el overview de Umbriel, no el lienzo de morgan.
    if let Some(b) = bind(&a.ventanas, "overview-toggle", &["repeat = false"]) { // no-tr
        // no-tr
        out.push_str("# Ventanas: el overview de Umbriel\n");
        out.push_str(&b);
    }
    if a.multimedia {
        out.push_str("# Multimedia y brillo (morgan)\n");
        for (t, cmd, _, repite) in binds::multimedia() {
            let extra: &[&str] = if repite {
                &["allow_when_locked = true"] // no-tr
            } else {
                &["allow_when_locked = true", "repeat = false"] // no-tr
            };
            if let Some(b) = bind(t, &format!("spawn {cmd}"), extra) { // no-tr
                out.push_str(&b);
            }
        }
    }
    if con_grim {
        out.push_str("# Capturas de pantalla (morgan-capturar)\n");
        for (t, _, que) in binds::CAPTURAS {
            if let Some(b) = bind(
                t,
                &format!("spawn {}", binds::comando_captura(&c.capturas, que)), // no-tr
                &["repeat = false"], // no-tr
            ) {
                // no-tr
                out.push_str(&b);
            }
        }
    }
    let propios: Vec<String> = a
        .propios
        .iter()
        .filter(|p| !p.key.is_empty())
        .filter_map(|p| bind(&p.key, &accion_propio(p)?, &extra_propio(p)))
        .collect();
    if !propios.is_empty() {
        out.push_str("# Atajos propios (menú)\n");
        out.extend(propios);
    }
    for (m, modo) in a
        .modos
        .iter()
        .enumerate()
        .filter(|(_, m)| !m.key.is_empty())
    {
        let nombre = binds::submap_modo(m);
        out.push_str(&format!("# Modo: {}\n", modo.name));
        if let Some(b) = bind(&modo.key, &format!("submap {nombre}"), &["repeat = false"]) { // no-tr
            // no-tr
            out.push_str(&b);
        }
        // Escape siempre sale: un modo sin él dejaría el teclado atrapado.
        out.push_str(&format!(
            "{} = \"submap reset\"\n", // no-tr
            cadena(&format!("submap[{nombre}],Escape"))
        )); // no-tr
        for p in &modo.binds {
            let (Some(t), Some(accion)) = (tecla(&p.key), accion_propio(p)) else {
                continue;
            };
            let mut extra: Vec<&str> = extra_propio(p);
            if modo.once {
                extra.push("submap = \"reset\""); // no-tr
            }
            let mut campos = vec![format!("action = {}", cadena(&accion))]; // no-tr
            campos.extend(extra.iter().map(|e| (*e).to_owned()));
            out.push_str(&format!(
                "{} = {{ {} }}\n",
                cadena(&format!("submap[{nombre}],{t}")),
                campos.join(", ")
            ));
        }
    }
    // Mientras el menú graba una tecla (Atajos): un submap sin nada, para que la combinación le
    // llegue entera. Umbriel no conoce un submap sin binds: lleva uno de relleno.
    out.push_str(&format!(
        "# Mientras el menú graba una tecla\n{} = \"submap reset\"\n", // no-tr
        cadena(&format!(
            "submap[{}],Super+Ctrl+Alt+Shift+F24",
            binds::SUBMAP_GRABAR
        ))  // no-tr
    ));
    out.push_str(&crate::menu::reglas::toml(&c.reglas, &crate::aplicacion::buscar));
    out.push_str(CAPAS);
    out
}

pub async fn aplicar(c: &Config, ruta_config: &Path) -> Result<(), String> {
    aplicar_con(c, ruta_config, &[]).await
}

/// Con `prueba`, monitores a probar encima de lo guardado: no se guardan, y el próximo
/// `aplicar` vuelve a lo de antes.
pub async fn aplicar_con(
    c: &Config,
    ruta_config: &Path,
    prueba: &[(String, Campo)],
) -> Result<(), String> {
    let destino = ruta();
    let con_grim = binds::hay_capturas();
    let mut menu = Menu::leer(ruta_config)?;
    for (salida, campo) in prueba {
        menu.monitores.poner(salida, campo);
    }
    // El shader primero: el preset lo nombra y la validación lo compila.
    let glsl = destino.with_file_name(SHADER);
    let shader = shader(c);
    if std::fs::read_to_string(&glsl).ok().as_deref() != Some(shader.as_str()) {
        escribir(&glsl, &shader)?;
    }
    let nuevo = toml_con(c, con_grim, &colores_tema(), &menu);
    if std::fs::read_to_string(&destino).ok().as_deref() == Some(nuevo.as_str()) {
        return Ok(());
    }
    // Se valida aparte, antes de ponerlo donde Umbriel lo mira.
    let prueba = destino.with_extension("prueba.toml");
    escribir(&prueba, &nuevo)?;
    let valida = crate::comando::salida(&[
        "umbriel",
        "config",
        "validate",
        "-c",
        &prueba.to_string_lossy(),
    ])
    .await; // no-tr
    let _ = std::fs::remove_file(&prueba);
    valida.map_err(|e| {
        trf!("la config de morgan para Umbriel quedó inválida ({}); se dejó la de antes", e)
    })?;
    escribir(&destino, &nuevo)
}

fn escribir(ruta: &Path, texto: &str) -> Result<(), String> {
    if let Some(dir) = ruta.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // Primero a un temporal y después el cambio de nombre: Umbriel nunca lee uno a medias.
    let tmp = ruta.with_extension("tmp");
    std::fs::write(&tmp, texto).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, ruta).map_err(|e| format!("{}: {e}", ruta.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teclas_de_hyprland_a_umbriel() {
        assert_eq!(tecla("SUPER + space").as_deref(), Some("Super+space"));
        assert_eq!(
            tecla("SUPER + ALT + SHIFT + L").as_deref(),
            Some("Super+Alt+Shift+L")
        );
        assert_eq!(tecla("CTRL + Print").as_deref(), Some("Ctrl+Print"));
        assert_eq!(tecla("XF86AudioMute").as_deref(), Some("XF86AudioMute"));
        assert_eq!(tecla("SUPER + code:10"), None);
        assert_eq!(tecla(""), None);
    }

    #[test]
    fn los_defaults_son_toml_valido_con_las_capas() {
        let t = toml_con(&Config::default(), true, Path::new("/e/tema/umbriel.toml"), &Menu::default());
        assert!(t.contains("files = [\"/e/tema/umbriel.toml\"]"));
        let v: toml::Table = toml::from_str(&t).expect("TOML válido");
        let binds = v["keybinds"].as_table().unwrap();
        assert_eq!(
            binds["Super+space"]["action"].as_str(),
            Some("spawn morganctl launcher toggle")
        );
        assert_eq!(
            binds["XF86AudioRaiseVolume"]["allow_when_locked"].as_bool(),
            Some(true)
        );
        // freeze viene prendido: la región se elige sobre la pantalla congelada.
        assert!(binds["Print"]["action"].as_str().unwrap().starts_with("spawn morgan-capturar -z -d "));
        assert!(binds["Print"]["action"].as_str().unwrap().ends_with(" region"));
        assert!(!binds["Ctrl+Print"]["action"].as_str().unwrap().contains("-z"));
        assert_eq!(v["layer_rule"].as_array().unwrap().len(), 2);
        // Sin grim, sin capturas.
        let t = toml_con(&Config::default(), false, Path::new("/t"), &Menu::default());
        assert!(!t.contains("Print"));
        assert_eq!(v["animation"]["windows_in"]["duration_ms"].as_integer(), Some(200));
    }

    #[test]
    fn ventanas_y_esquina_abren_el_overview_de_umbriel() {
        let mut c = Config::default();
        let v: toml::Table = toml::from_str(&toml_con(&c, false, Path::new("/t"), &Menu::default())).unwrap();
        assert_eq!(v["keybinds"]["Super+Tab"]["action"].as_str(), Some("overview-toggle"));
        let esquina = &v["hot_corners"]["top_left"];
        assert_eq!(esquina["enabled"].as_bool(), Some(true));
        assert_eq!(esquina["action"].as_str(), Some("overview-toggle"));
        // Apagada no se escribe: manda la de user.toml, si hay.
        c.shell.esquina = false;
        let v: toml::Table = toml::from_str(&toml_con(&c, false, Path::new("/t"), &Menu::default())).unwrap();
        assert!(v.get("hot_corners").is_none());
    }

    #[test]
    fn un_modo_es_un_submap_con_escape() {
        let mut c = Config::default();
        c.atajos.modos.push(crate::config::Modo {
            name: "Ventanas".into(),
            key: "SUPER + R".into(),
            once: true,
            binds: vec![AtajoPropio {
                key: "H".into(),
                command: "umbriel msg window-focus-left".into(),
                ..Default::default()
            }],
        });
        let v: toml::Table = toml::from_str(&toml_con(&c, false, Path::new("/t"), &Menu::default())).unwrap();
        let binds = v["keybinds"].as_table().unwrap();
        assert_eq!(
            binds["Super+R"]["action"].as_str(),
            Some("submap morgan-mode-0")
        );
        assert_eq!(
            binds["submap[morgan-mode-0],Escape"].as_str(),
            Some("submap reset")
        );
        let h = &binds["submap[morgan-mode-0],H"];
        assert_eq!(
            h["action"].as_str(),
            Some("spawn umbriel msg window-focus-left")
        );
        assert_eq!(h["submap"].as_str(), Some("reset"));
    }

    #[test]
    fn el_borde_la_sombra_y_lo_del_menu() {
        let mut c = Config::default();
        c.apariencia.sombra = "glow".into();
        let mut menu = Menu::default();
        menu.cambios.poner("appearance.blur.radius", Some(crate::menu::compositor::Valor::Int(6)));
        menu.cambios.poner(
            "animation.dim_unfocused.enabled",
            Some(crate::menu::compositor::Valor::Bool(true)),
        );
        menu.monitores.poner(
            "HDMI-A-1",
            &Campo::Lugar(320, -1080, (1920, 1080, 60.0)),
        );
        menu.monitores.poner("HDMI-A-1", &Campo::Encendida(false));
        menu.monitores.poner("DP-3", &Campo::Rotacion(1));
        let v: toml::Table =
            toml::from_str(&toml_con(&c, false, Path::new("/t"), &menu)).expect("TOML válido");
        assert_eq!(v["appearance"]["shadow"]["enabled"].as_bool(), Some(false));
        assert_eq!(v["effects"]["border"].as_str(), Some(PRESET));
        let preset = &v["effects"]["preset"][PRESET];
        assert_eq!(preset["shader"].as_str(), Some(SHADER));
        assert_eq!(preset["padding"].as_integer(), Some(MARGEN.into()));
        // Lo del menú se junta con las tablas de morgan, no las pisa.
        assert_eq!(v["appearance"]["blur"]["radius"].as_integer(), Some(6));
        assert_eq!(v["animation"]["dim_unfocused"]["enabled"].as_bool(), Some(true));
        assert_eq!(v["animation"]["windows_in"]["duration_ms"].as_integer(), Some(200));
        let tv = &v["output"]["HDMI-A-1"];
        assert_eq!(tv["mode"].as_str(), Some("1920x1080@60.00"));
        assert_eq!(tv["position"].as_array().unwrap()[1].as_integer(), Some(-1080));
        assert_eq!(tv["enabled"].as_bool(), Some(false));
        assert_eq!(v["output"]["DP-3"]["transform"].as_str(), Some("90"));
        assert!(v["keybinds"].as_table().is_some());
    }

    #[test]
    fn el_cursor_y_las_reglas() {
        let mut menu = Menu::default();
        let sin: toml::Table =
            toml::from_str(&toml_con(&Config::default(), false, Path::new("/t"), &menu)).unwrap();
        assert!(sin.get("input").is_none());
        menu.cursor = Some(("Bibata-Modern-Ice".into(), 32));
        let mut c = Config::default();
        // Una app que no está no deja regla.
        c.reglas.push(crate::config::ReglaApp { app: "ya-no.desktop".into(), flotante: true, ..Default::default() });
        let v: toml::Table = toml::from_str(&toml_con(&c, false, Path::new("/t"), &menu)).unwrap();
        assert_eq!(v["input"]["cursor"]["theme"].as_str(), Some("Bibata-Modern-Ice"));
        assert_eq!(v["input"]["cursor"]["size"].as_integer(), Some(32));
        assert!(v.get("window_rule").is_none());
        assert!(v["layer_rule"].as_array().is_some());
    }

    /// Lo que se escribe lo acepta el Umbriel instalado (si está).
    #[test]
    fn umbriel_lo_valida() {
        if glib::find_program_in_path("umbriel").is_none() {
            return;
        }
        let menu = Menu { cursor: Some(("Adwaita".into(), 24)), ..Default::default() };
        let mut t = toml_con(&Config::default(), false, Path::new("/no-esta.toml"), &menu);
        t.push_str(&crate::menu::reglas::toml(
            &[crate::config::ReglaApp {
                app: "x.desktop".into(),
                workspace: 3,
                flotante: true,
                tamano: 60,
                opacidad: 0.9,
                sin_animacion: true,
            }],
            &|_| Some(crate::aplicacion::App { nombre: "x".into(), clases: vec!["x".into()], lanzar: String::new() }),
        ));
        let dir = std::env::temp_dir().join(format!("morgan-umbriel-valida-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("config.toml");
        std::fs::write(&ruta, &t).unwrap();
        std::fs::write(dir.join(SHADER), shader(&Config::default())).unwrap();
        let s = std::process::Command::new("umbriel")
            .args(["config", "validate", "-c"])
            .arg(&ruta)
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(s.status.success(), "{}\n{}", String::from_utf8_lossy(&s.stdout), String::from_utf8_lossy(&s.stderr));
    }

    #[test]
    fn el_shader_de_cada_sombra() {
        let mut c = Config::default();
        let negra = shader(&c);
        assert!(negra.contains("const float MARGEN = 10.0;"));
        assert!(negra.contains("const float GLOW = 0.0;"));
        c.apariencia.sombra = "glow".into();
        let glow = shader(&c);
        assert!(glow.contains("const float GLOW = 1.0;"));
        assert!(!glow.contains('@'));
        assert!(glow.contains("vec4 border(vec2 uv)"));
    }
}
