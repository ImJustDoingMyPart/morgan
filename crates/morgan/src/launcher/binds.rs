//! `morgan.lua` va al final de la config de Hyprland, así que cada bind pisa lo que hubiera en
//! su tecla (`hl.unbind` y el nuevo): con dos binds en la misma tecla Hyprland corre los dos.
//!
//! Las cabeceras `-- ─── … ───` son las que agrupan la página de Atajos del menú.

use crate::aplicacion::App;
use crate::config::{AtajoPropio, Atajos, Capturas, Cuando, Modo};

type Accion<'a> = (&'static str, &'a str, &'static str, &'static str, bool);

fn acciones(a: &Atajos) -> [(&'static str, Vec<Accion<'_>>); 2] {
    [
        (
            "Menús y lanzadores (morgan)", // no-tr
            vec![
                ("launcher", &a.launcher, "morganctl launcher toggle", tr!("Launcher"), false), // no-tr
                ("symbols", &a.simbolos, "morganctl launcher toggle symbols", tr!("Símbolos"), false), // no-tr
                ("clipboard", &a.portapapeles, "morganctl launcher toggle clipboard", tr!("Portapapeles"), false), // no-tr
                ("keybinds", &a.atajos, "morganctl launcher toggle keybinds", tr!("Atajos de teclado"), false), // no-tr
                ("calculator", &a.calculadora, "morganctl calculator toggle", tr!("Calculadora"), false), // no-tr
                ("menu", &a.menu, "morganctl menu toggle", tr!("Menú del sistema"), false), // no-tr
                ("wallpapers", &a.fondos, "morganctl wallpapers toggle", tr!("Selector de fondos"), false), // no-tr
                ("menu_wallpapers", &a.menu_fondos, "morganctl menu toggle wallpapers", tr!("Fondos de pantalla"), false), // no-tr
                ("center", &a.centro, "morganctl center toggle", tr!("Centro de control"), false), // no-tr
                ("dnd", &a.no_molestar, "morganctl dnd toggle", tr!("No molestar"), false), // no-tr
                ("session", &a.sesion, "morganctl session", tr!("Apagar, reiniciar, salir"), false), // no-tr
            ],
        ),
        (
            "Bloqueo (morgan)", // no-tr
            vec![
                ("lock", &a.bloquear, "loginctl lock-session", tr!("Bloquear pantalla"), false), // no-tr
                // Anda con la sesión bloqueada: es para cuando morgan-lock se cayó.
                ("rescue", &a.rescate, "systemctl --user start morgan-lock.service", tr!("Volver a lanzar el bloqueo"), true), // no-tr
            ],
        ),
    ]
}

#[derive(Debug, Clone, PartialEq)]
pub struct Propio {
    pub id: &'static str,
    pub cabecera: &'static str,
    pub desc: &'static str,
    pub tecla: String,
}

pub fn propios(a: &Atajos) -> Vec<Propio> {
    let mut v: Vec<Propio> = acciones(a)
        .into_iter()
        .flat_map(|(cabecera, binds)| {
            binds.into_iter().map(move |(id, tecla, _, desc, _)| Propio {
                id,
                cabecera,
                desc,
                tecla: tecla.to_owned(),
            })
        })
        .collect();
    v.push(Propio {
        id: "windows", // no-tr
        cabecera: "Lienzo de ventanas (morgan)", // no-tr
        desc: tr!("Lienzo de ventanas"),
        tecla: a.ventanas.clone(),
    });
    v
}

/// Ahí Hyprland no resuelve ningún bind (salvo los `submap_universal`) y la combinación le
/// llega entera al menú.
pub const SUBMAP_GRABAR: &str = "morgan-record"; // no-tr

/// Con `false` sale de él si es el puesto (al arrancar morgan, por si quedó adentro).
pub fn submap_grabar(entrar: bool) -> String {
    if entrar {
        format!("hl.dispatch(hl.dsp.submap(\"{SUBMAP_GRABAR}\"))") // no-tr
    } else {
        format!(
            "if hl.get_current_submap() == \"{SUBMAP_GRABAR}\" then hl.dispatch(hl.dsp.submap(\"reset\")) end" // no-tr
        )
    }
}

/// `super + h` y `SUPER + H` son la misma para Hyprland.
pub fn normalizar(tecla: &str) -> String {
    let mut mods = [false; 4];
    let mut resto = Vec::new();
    for p in tecla.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match p.to_uppercase().as_str() {
            "SUPER" | "MOD4" | "WIN" | "META" => mods[0] = true, // no-tr
            "CTRL" | "CONTROL" => mods[1] = true,                // no-tr
            "ALT" | "MOD1" => mods[2] = true,                    // no-tr
            "SHIFT" => mods[3] = true,                           // no-tr
            otra => resto.push(otra.to_owned()),
        }
    }
    combinacion(mods, &resto.join(" + ")).to_uppercase()
}

pub fn combinacion(mods: [bool; 4], tecla: &str) -> String {
    let nombres = ["SUPER", "CTRL", "ALT", "SHIFT"]; // no-tr
    let mut v: Vec<String> = nombres
        .iter()
        .zip(mods)
        .filter(|(_, m)| *m)
        .map(|(n, _)| (*n).to_owned())
        .collect();
    let mut c = tecla.chars();
    v.push(match (c.next(), c.next()) {
        (Some(l), None) => l.to_uppercase().collect(),
        _ => tecla.to_owned(),
    });
    v.join(" + ")
}

/// Todas `locked`: el volumen se cambia también con la pantalla bloqueada.
fn multimedia() -> [(&'static str, &'static str, &'static str, bool); 10] {
    [
        ("XF86AudioRaiseVolume", "morganctl volume up", tr!("Subir volumen"), true), // no-tr
        ("XF86AudioLowerVolume", "morganctl volume down", tr!("Bajar volumen"), true), // no-tr
        ("XF86AudioMute", "morganctl volume mute", tr!("Silenciar audio"), false), // no-tr
        ("XF86AudioMicMute", "morganctl volume mute --mic", tr!("Silenciar micrófono"), false), // no-tr
        ("XF86AudioNext", "morganctl player next", tr!("Siguiente pista"), false), // no-tr
        ("XF86AudioPrev", "morganctl player previous", tr!("Pista anterior"), false), // no-tr
        ("XF86AudioPlay", "morganctl player play-pause", tr!("Reproducir o pausar"), false), // no-tr
        ("XF86AudioPause", "morganctl player play-pause", tr!("Reproducir o pausar"), false), // no-tr
        ("XF86MonBrightnessUp", "morganctl brightness up", tr!("Subir brillo"), true), // no-tr
        ("XF86MonBrightnessDown", "morganctl brightness down", tr!("Bajar brillo"), true), // no-tr
    ]
}

const CAPTURAS: [(&str, &str, &str); 3] = [
    ("Print", tr_const!("Captura región"), "-m region"), // no-tr
    ("CTRL + Print", tr_const!("Captura pantalla"), "-m output"), // no-tr
    ("ALT + Print", tr_const!("Captura ventana activa"), "-m window -m active"), // no-tr
];

/// Lo de morgan va por el socket de eventos (`ipc::de_evento`): el compositor no lanza un
/// proceso por tecla. El resto, un comando.
fn accion(cmd: &str) -> String {
    match cmd.strip_prefix("morganctl ") { // no-tr
        Some(args) => format!("hl.dsp.event({})", cadena(&format!("morgan {args}"))), // no-tr
        None => format!("hl.dsp.exec_cmd({})", cadena(cmd)), // no-tr
    }
}

fn modificador(tecla: &str) -> Option<(&'static str, u32, u32)> {
    let primero = tecla.split('+').next()?.trim().to_uppercase();
    match primero.as_str() {
        "SUPER" | "MOD4" | "WIN" | "META" => Some(("Super", 133, 134)), // no-tr
        "ALT" | "MOD1" => Some(("Alt", 64, 108)),                        // no-tr
        "CTRL" | "CONTROL" => Some(("Control", 37, 105)),                // no-tr
        "SHIFT" => Some(("Shift", 50, 62)),                              // no-tr
        _ => None,
    }
}

fn cadena(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn entre_comillas(ruta: &str) -> String {
    format!("'{}'", ruta.replace('\'', "'\\''"))
}

fn carpeta_capturas() -> String {
    gtk::glib::user_special_dir(gtk::glib::UserDirectory::Pictures)
        .unwrap_or_else(gtk::glib::home_dir)
        .join("Screenshots") // no-tr
        .to_string_lossy()
        .into_owned()
}

fn comando_captura(c: &Capturas, modo: &str) -> String {
    let destino = if c.portapapeles {
        // Sin archivo hyprshot no corre el comando de `--`: que avise él.
        "--clipboard-only".to_owned()
    } else {
        let carpeta = entre_comillas(&if c.carpeta.is_empty() {
            carpeta_capturas()
        } else {
            c.carpeta.clone()
        });
        format!("-o {carpeta} -s -- morgan-captura") // no-tr
    };
    let congelar = if c.congelar && modo == "-m region" { "-z " } else { "" };
    format!("hyprshot {congelar}{modo} {destino}") // no-tr
}

/// Sin un modificador que soltar no hay submap: la tecla solo pide el lienzo.
fn lua_ventanas(tecla: &str, capturas: Option<&Capturas>) -> String {
    let desc = tr!("Lienzo de ventanas");
    let Some((nombre, codigo_l, codigo_r)) = modificador(tecla) else {
        return format!(
            "\n-- ─── Lienzo de ventanas (morgan) ───\natajo({}, hl.dsp.event(\"morgan windows\"), {{ desc = {} }})\n", // no-tr
            cadena(tecla),
            cadena(desc)
        );
    };
    let base = tecla.split('+').next().unwrap_or_default().trim();
    let escape = if nombre == "Alt" { format!("{base} + CTRL + Escape") } else { format!("{base} + ALT + Escape") }; // no-tr
    // Con el modificador apretado, Print llega como `<mod> + Print`: la pantalla entera
    // (elegir una región obligaría a soltarlo, y eso cierra el lienzo).
    let captura = capturas
        .map(|c| {
            format!(
                "\n    hl.bind({}, hl.dsp.exec_cmd({}))", // no-tr
                cadena(&format!("{base} + Print")), // no-tr
                cadena(&comando_captura(c, "-m output"))
            )
        })
        .unwrap_or_default();
    include_str!("ventanas.lua")
        .replace("{tecla}", tecla)
        .replace("{mod}", nombre)
        .replace("{codigo_l}", &codigo_l.to_string())
        .replace("{codigo_r}", &codigo_r.to_string())
        .replace("{escape}", &escape)
        .replace("{captura}", &captura)
        .replace("{desc}", desc)
}

pub fn lua(a: &Atajos, c: &Capturas, hyprshot: bool) -> String {
    lua_con(a, c, hyprshot, &crate::aplicacion::buscar)
}


/// El nombre con que se muestra: el de la app o el comando.
pub fn nombre_propio(p: &AtajoPropio, app: Option<&App>) -> String {
    match app {
        Some(a) => a.nombre.clone(),
        None if !p.app.is_empty() => p.app.trim_end_matches(".desktop").to_owned(), // no-tr
        None => p.command.clone(),
    }
}

/// La acción, las opciones y la descripción de un atajo propio; `None` si no hace nada (sin
/// tecla, una app que ya no está, un comando vacío).
fn partes_propio(p: &AtajoPropio, buscar: &dyn Fn(&str) -> Option<App>) -> Option<(String, Vec<String>)> {
    if p.key.is_empty() {
        return None;
    }
    let app = (!p.app.is_empty()).then(|| buscar(&p.app)).flatten();
    let accion = match (&app, p.app.is_empty()) {
        (Some(a), _) if p.new_window => format!("hl.dsp.exec_cmd({})", cadena(&a.lanzar)), // no-tr
        (Some(a), _) => format!(
            "morgan_buscar({{ {} }}, {})", // no-tr
            a.clases.iter().map(|c| cadena(c)).collect::<Vec<_>>().join(", "),
            cadena(&a.lanzar)
        ),
        (None, false) => return None,
        (None, true) if p.command.trim().is_empty() => return None,
        (None, true) => format!("hl.dsp.exec_cmd({})", cadena(&p.command)), // no-tr
    };
    let mut o: Vec<String> = Vec::new();
    match p.when {
        Cuando::Press if p.repeat => o.push("repeating = true".into()), // no-tr
        Cuando::Press => {}
        Cuando::Release => o.push("release = true".into()), // no-tr
        Cuando::LongPress => o.push("long_press = true".into()), // no-tr
    }
    if p.locked {
        o.push("locked = true".into()); // no-tr
    }
    if p.non_consuming {
        o.push("non_consuming = true".into()); // no-tr
    }
    o.push(format!("desc = {}", cadena(&nombre_propio(p, app.as_ref())))); // no-tr
    Some((accion, o))
}

/// El submap de un modo: el índice, así renombrarlo no cambia nada.
pub fn submap_modo(m: usize) -> String {
    format!("morgan-mode-{m}") // no-tr
}

/// Los atajos propios y los modos: `morgan_buscar` va una vez para los dos.
fn lua_propios(propios: &[AtajoPropio], modos: &[Modo], buscar: &dyn Fn(&str) -> Option<App>) -> String {
    let mut out = String::new();
    for p in propios {
        let Some((accion, o)) = partes_propio(p, buscar) else { continue };
        out.push_str(&format!(
            "atajo({}, {}, {{ {} }})\n", // no-tr
            cadena(&p.key),
            accion,
            o.join(", ")
        ));
    }
    for (m, modo) in modos.iter().enumerate().filter(|(_, m)| !m.key.is_empty()) {
        let nombre = submap_modo(m);
        // Escape siempre sale: un modo sin él dejaría el teclado atrapado.
        let mut adentro = format!(
            "    hl.bind(\"Escape\", hl.dsp.submap(\"reset\"), {{ desc = {} }})\n", // no-tr
            cadena(tr!("Salir del modo"))
        );
        for p in &modo.binds {
            let Some((accion, o)) = partes_propio(p, buscar) else { continue };
            let accion = if modo.once {
                format!("function() hl.dispatch({accion}) hl.dispatch(hl.dsp.submap(\"reset\")) end") // no-tr
            } else {
                accion
            };
            adentro.push_str(&format!(
                "    hl.bind({}, {}, {{ {} }})\n", // no-tr
                cadena(&p.key),
                accion,
                o.join(", ")
            ));
        }
        out.push_str(&format!(
            "hl.define_submap({}, function()\n{adentro}end)\natajo({}, hl.dsp.submap({}), {{ desc = {} }})\n", // no-tr
            cadena(&nombre),
            cadena(&modo.key),
            cadena(&nombre),
            cadena(&trf!("Modo: {}", modo.name))
        ));
    }
    if out.is_empty() {
        return out;
    }
    format!("\n-- ─── Atajos propios (menú) ───\n{}{out}", include_str!("buscar.lua")) // no-tr
}

pub fn lua_con(
    a: &Atajos,
    c: &Capturas,
    hyprshot: bool,
    buscar: &dyn Fn(&str) -> Option<App>,
) -> String {
    let mut out = String::from(concat!(
        "\n-- Los binds de morgan (morgan/config.toml, [keybinds] y [screenshots]): pisan los que\n", // no-tr
        "-- hubiera en esas teclas.\n",                                                          // no-tr
        "local function atajo(tecla, accion, opts)\n",                                           // no-tr
        "    hl.unbind(tecla)\n",                                                                // no-tr
        "    hl.bind(tecla, accion, opts)\n",                                                    // no-tr
        "end\n",
    ));
    let opciones = |desc: &str, locked: bool, repeating: bool| {
        let mut o = Vec::new();
        if locked {
            o.push("locked = true".to_owned()); // no-tr
        }
        if repeating {
            o.push("repeating = true".to_owned()); // no-tr
        }
        o.push(format!("desc = {}", cadena(desc))); // no-tr
        format!("{{ {} }}", o.join(", "))
    };
    for (cabecera, binds) in acciones(a) {
        let binds: Vec<_> = binds.into_iter().filter(|(_, t, ..)| !t.is_empty()).collect();
        if binds.is_empty() {
            continue;
        }
        out.push_str(&format!("\n-- ─── {cabecera} ───\n")); // no-tr
        for (_, tecla, cmd, desc, locked) in binds {
            out.push_str(&format!(
                "atajo({}, {}, {})\n", // no-tr
                cadena(tecla),
                accion(cmd),
                opciones(desc, locked, false)
            ));
        }
    }
    if !a.ventanas.is_empty() {
        out.push_str(&lua_ventanas(&a.ventanas, hyprshot.then_some(c)));
    }
    if a.multimedia {
        out.push_str("\n-- ─── Multimedia y brillo (morgan) ───\n"); // no-tr
        for (tecla, cmd, desc, repeating) in multimedia() {
            out.push_str(&format!(
                "atajo({}, {}, {})\n", // no-tr
                cadena(tecla),
                accion(cmd),
                opciones(desc, true, repeating)
            ));
        }
        // No consumidor: la tecla sigue haciendo lo suyo, esto solo avisa. Al soltar, cuando
        // xkb ya cambió el estado que morgan lee en `j/devices`; `ignore_mods` porque al
        // prenderse, el propio Bloq Mayús es un modificador activo. Sin descripción: no es un
        // atajo.
        out.push_str(
            "atajo(\"Caps_Lock\", hl.dsp.event(\"morgan capslock\"), { release = true, non_consuming = true, ignore_mods = true })\n", // no-tr
        );
    }
    if hyprshot {
        out.push_str("\n-- ─── Capturas de pantalla (hyprshot) ───\n"); // no-tr
        for (tecla, desc, modo) in CAPTURAS {
            out.push_str(&format!(
                "atajo({}, hl.dsp.exec_cmd({}), {{ desc = {} }})\n", // no-tr
                cadena(tecla),
                cadena(&comando_captura(c, modo)),
                cadena(morgan_idioma::t(desc))
            ));
        }
    }
    out.push_str(&lua_propios(&a.propios, &a.modos, buscar));
    // Uno vacío no se registra («submap doesn't exist»): lleva un bind de relleno en una
    // tecla que nadie aprieta.
    out.push_str(concat!(
        "\n-- Mientras el menú graba una tecla (Atajos), sin binds.\n", // no-tr
        "hl.define_submap(\"morgan-record\", function()\n",              // no-tr
        "    hl.bind(\"SUPER + CTRL + ALT + SHIFT + F24\", hl.dsp.submap(\"reset\"))\n", // no-tr
        "end)\n",
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::atajos::grupos_de_binds_lua;

    #[test]
    fn los_defaults_arman_todo_y_la_pagina_los_agrupa() {
        let lua = lua(&Atajos::default(), &Capturas::default(), true);
        assert!(lua.contains(
            r#"atajo("SUPER + space", hl.dsp.event("morgan launcher toggle"), { desc = "Launcher" })"#
        ));
        assert!(lua.contains(
            r#"atajo("SUPER + ALT + SHIFT + L", hl.dsp.exec_cmd("systemctl --user start morgan-lock.service"), { locked = true, desc = "Volver a lanzar el bloqueo" })"#
        ));
        assert!(lua.contains(
            r#"atajo("XF86AudioRaiseVolume", hl.dsp.event("morgan volume up"), { locked = true, repeating = true, desc = "Subir volumen" })"#
        ));
        assert!(!lua.contains("morgan launcher toggle keybinds"), "sin tecla, sin bind");
        assert!(!lua.contains("exec_cmd(\"morganctl"), "lo de morgan va por eventos");
        // Las cabeceras ponen cada bind en su grupo de la página de Atajos.
        let grupos = grupos_de_binds_lua(&lua);
        let grupo = |d: &str| grupos.iter().find(|(x, ..)| x == d).map(|g| g.2);
        assert_eq!(grupo("Launcher"), Some(0));
        assert_eq!(grupo("Bloquear pantalla"), Some(0));
        assert_eq!(grupo("Lienzo de ventanas"), Some(2));
        assert_eq!(grupo("Subir brillo"), Some(3));
        assert_eq!(grupo("Captura región"), Some(4));
    }

    #[test]
    fn el_lienzo_suelta_el_modificador_de_su_tecla() {
        let lua = lua_ventanas("SUPER + Tab", Some(&Capturas::default()));
        for m in ["{tecla}", "{mod}", "{codigo_l}", "{codigo_r}", "{escape}", "{captura}", "{desc}"] {
            assert!(!lua.contains(m), "quedó {m}");
        }
        assert!(lua.contains(r#"hl.is_key_down("Super_L") or hl.is_key_down("Super_R")"#));
        assert!(lua.contains("keycode == 133 or keycode == 134"));
        assert!(lua.contains(r#"hl.bind("SUPER + ALT + Escape", soltar_ventanas)"#));
        assert!(lua.contains(r#"hl.bind("SUPER + Print", hl.dsp.exec_cmd("hyprshot -m output -o "#));
        let alt = lua_ventanas("ALT + Tab", None);
        assert!(alt.contains("keycode == 64 or keycode == 108"));
        assert!(alt.contains(r#"hl.bind("ALT + CTRL + Escape", soltar_ventanas)"#));
        assert!(!alt.contains("Print"));
        let suelta = lua_ventanas("F9", None);
        assert!(!suelta.contains("define_submap"));
        assert!(suelta.contains(r#"atajo("F9", hl.dsp.event("morgan windows"), { desc = "Lienzo de ventanas" })"#));
        assert!(!lua.contains("morganctl windows"));
    }

    #[test]
    fn las_capturas_arman_los_tres_binds() {
        let c = Capturas {
            congelar: false,
            carpeta: "/home/r/Mis capturas".into(),
            portapapeles: false,
        };
        let lua = lua(&Atajos::default(), &c, true);
        assert!(lua.contains(
            r#"atajo("Print", hl.dsp.exec_cmd("hyprshot -m region -o '/home/r/Mis capturas' -s -- morgan-captura"), { desc = "Captura región" })"#
        ));
        let c = Capturas {
            portapapeles: true,
            ..Default::default()
        };
        let lua = super::lua(&Atajos::default(), &c, true);
        assert!(lua.contains("hyprshot -z -m region --clipboard-only"));
        assert!(lua.contains("hyprshot -m window -m active --clipboard-only"));
        let lua = super::lua(&Atajos::default(), &c, false);
        assert!(!lua.contains("hyprshot"), "sin hyprshot, sin capturas");
    }

    #[test]
    fn todo_apagado() {
        let a = Atajos {
            launcher: String::new(),
            simbolos: String::new(),
            portapapeles: String::new(),
            atajos: String::new(),
            calculadora: String::new(),
            menu: String::new(),
            fondos: String::new(),
            menu_fondos: String::new(),
            ventanas: String::new(),
            centro: String::new(),
            no_molestar: String::new(),
            retirada_notas: None,
            sesion: String::new(),
            bloquear: String::new(),
            rescate: String::new(),
            multimedia: false,
            propios: Vec::new(),
            modos: Vec::new(),
        };
        let lua = lua(&a, &Capturas::default(), false);
        assert!(!lua.contains("atajo(\""), "{lua}");
    }

    #[test]
    fn las_teclas_grabadas_se_comparan_y_se_escriben_como_los_defaults() {
        assert_eq!(combinacion([true, false, true, false], "u"), "SUPER + ALT + U");
        assert_eq!(combinacion([false, true, false, true], "Print"), "CTRL + SHIFT + Print");
        assert_eq!(combinacion([true; 4], "comma"), "SUPER + CTRL + ALT + SHIFT + comma");
        // Lo que da `j/binds` (SHIFT primero) y lo que se escribe a mano son la misma tecla.
        assert_eq!(normalizar("SHIFT + SUPER + y"), normalizar("SUPER + SHIFT + Y"));
        assert_eq!(normalizar("mod4 + control + Tab"), normalizar("SUPER + CTRL + tab"));
        assert_ne!(normalizar("SUPER + Y"), normalizar("SUPER + SHIFT + Y"));
        assert_eq!(normalizar(""), "");
    }

    #[test]
    fn los_propios_van_con_o_sin_tecla_y_el_submap_de_grabar_siempre() {
        let a = Atajos {
            calculadora: String::new(),
            ..Atajos::default()
        };
        let p = propios(&a);
        assert_eq!(p.len(), 14, "las trece de acciones y el lienzo");
        let calc = p.iter().find(|p| p.id == "calculator").unwrap();
        assert_eq!((calc.desc, calc.tecla.as_str()), ("Calculadora", ""));
        // Cada uno se edita por su clave en [keybinds].
        let mut otra = a.clone();
        for x in &p {
            assert_eq!(a.tecla(x.id).as_deref(), Some(x.tecla.as_str()), "{}", x.id);
            *otra.tecla_mut(x.id).unwrap() = "F1".into();
        }
        assert!(propios(&otra).iter().all(|x| x.tecla == "F1"));
        // Todo apagado igual define el submap: el menú puede grabar una tecla nueva.
        let vacio = Atajos {
            multimedia: false,
            ..otra.clone()
        };
        for (id, ..) in acciones(&vacio).iter().flat_map(|(_, b)| b.clone()) {
            assert!(vacio.tecla(id).is_some(), "{id}");
        }
        let lua = lua(&vacio, &Capturas::default(), false);
        assert!(lua.contains(&format!("hl.define_submap(\"{SUBMAP_GRABAR}\", function()")));
        assert!(submap_grabar(false).contains("get_current_submap"));
    }

    fn app_de_prueba(id: &str) -> Option<App> {
        (id == "firefox.desktop").then(|| App {
            nombre: "Firefox".into(),
            clases: vec!["firefox".into()],
            lanzar: "uwsm app -- 'firefox.desktop'".into(),
        })
    }

    #[test]
    fn propios_app_comando_y_opciones() {
        let p = |key: &str, app: &str, command: &str| AtajoPropio {
            key: key.into(),
            app: app.into(),
            command: command.into(),
            ..Default::default()
        };
        let propios = vec![
            p("SUPER + F", "firefox.desktop", ""),
            AtajoPropio {
                new_window: true,
                ..p("SUPER + SHIFT + F", "firefox.desktop", "")
            },
            AtajoPropio {
                repeat: true,
                locked: true,
                ..p("SUPER + T", "", "kitty -e btop")
            },
            // Repetir no va con soltar: Hyprland lo rechaza.
            AtajoPropio {
                when: Cuando::Release,
                repeat: true,
                non_consuming: true,
                ..p("SUPER + R", "", "true")
            },
            // Sin tecla, una app que ya no está o un comando vacío: nada.
            p("", "", "algo"),
            p("SUPER + G", "ya-no.desktop", ""),
            p("SUPER + H", "", "  "),
        ];
        let l = lua_propios(&propios, &[], &app_de_prueba);
        assert!(l.contains("local function morgan_buscar(clases, cmd)"), "{l}");
        let binds: Vec<&str> = l.lines().filter(|x| x.starts_with("atajo(")).collect();
        assert_eq!(
            binds,
            [
                r#"atajo("SUPER + F", morgan_buscar({ "firefox" }, "uwsm app -- 'firefox.desktop'"), { desc = "Firefox" })"#,
                r#"atajo("SUPER + SHIFT + F", hl.dsp.exec_cmd("uwsm app -- 'firefox.desktop'"), { desc = "Firefox" })"#,
                r#"atajo("SUPER + T", hl.dsp.exec_cmd("kitty -e btop"), { repeating = true, locked = true, desc = "kitty -e btop" })"#,
                r#"atajo("SUPER + R", hl.dsp.exec_cmd("true"), { release = true, non_consuming = true, desc = "true" })"#,
            ]
        );
        assert_eq!(lua_propios(&[], &[], &app_de_prueba), "");
    }

    #[test]
    fn modos_con_escape_y_de_una_tecla() {
        let p = |key: &str, command: &str| AtajoPropio {
            key: key.into(),
            command: command.into(),
            ..Default::default()
        };
        let modos = vec![
            Modo {
                name: "Ventanas".into(),
                key: "SUPER + R".into(),
                once: false,
                binds: vec![p("H", "uno"), p("", "sin tecla")],
            },
            Modo {
                name: "Abrir".into(),
                key: "SUPER + O".into(),
                once: true,
                binds: vec![AtajoPropio {
                    app: "firefox.desktop".into(),
                    ..p("F", "")
                }],
            },
            // Sin tecla para entrar: no se arma.
            Modo {
                name: "Nada".into(),
                ..Default::default()
            },
        ];
        let l = lua_propios(&[], &modos, &app_de_prueba);
        let esperado = concat!(
            "hl.define_submap(\"morgan-mode-0\", function()\n",
            "    hl.bind(\"Escape\", hl.dsp.submap(\"reset\"), { desc = \"Salir del modo\" })\n",
            "    hl.bind(\"H\", hl.dsp.exec_cmd(\"uno\"), { desc = \"uno\" })\n",
            "end)\n",
            "atajo(\"SUPER + R\", hl.dsp.submap(\"morgan-mode-0\"), { desc = \"Modo: Ventanas\" })\n",
            "hl.define_submap(\"morgan-mode-1\", function()\n",
            "    hl.bind(\"Escape\", hl.dsp.submap(\"reset\"), { desc = \"Salir del modo\" })\n",
            "    hl.bind(\"F\", function() hl.dispatch(morgan_buscar({ \"firefox\" }, \"uwsm app -- 'firefox.desktop'\")) hl.dispatch(hl.dsp.submap(\"reset\")) end, { desc = \"Firefox\" })\n",
            "end)\n",
            "atajo(\"SUPER + O\", hl.dsp.submap(\"morgan-mode-1\"), { desc = \"Modo: Abrir\" })\n",
        );
        assert!(l.contains(esperado), "{l}");
        assert!(!l.contains("morgan-mode-2"));
        assert!(l.contains("local function morgan_buscar"));
    }
}
