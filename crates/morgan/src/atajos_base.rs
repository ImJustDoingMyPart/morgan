//! Los atajos de ventanas de morgan: foco y movimiento de columnas, workspaces, monitores,
//! layout scrolling, apps que ciclan y grabación. Se declaran una vez y de acá salen los dos
//! formatos (`morgan-atajos.lua` para Hyprland y `morgan-atajos.toml` para Umbriel), así que los
//! dos compositores quedan iguales por construcción.
//!
//! Los escribe `morgan setup` si el usuario los quiere, como archivos aparte que se pueden
//! editar. Nada acá pisa los binds propios de morgan (`launcher::binds`): ninguna tecla se
//! repite con ellos.
//!
//! Las teclas se escriben con la notación de Umbriel (`Mod+Shift+Left`); `a_hypr` las pasa a la
//! de Hyprland.

use morgan_idioma::tr;

/// Lo que depende de qué hay instalado en la máquina.
#[derive(Clone, Copy, Default)]
pub struct Opciones {
    /// `gsr-ui-cli` (GPU Screen Recorder): sin él no se agregan los atajos de grabación.
    pub grabador: bool,
    /// `uwsm`: Ctrl+Alt+Supr cierra la sesión con `uwsm stop`. Sin él, en Hyprland no hay bind.
    pub uwsm: bool,
}

impl Opciones {
    pub fn de_la_maquina() -> Self {
        Self {
            grabador: glib::find_program_in_path("gsr-ui-cli").is_some(), // no-tr
            uwsm: glib::find_program_in_path("uwsm").is_some(),           // no-tr
        }
    }
}

/// Un atajo: la tecla, para qué sirve y qué hace en cada compositor. Sin acción en uno, ese
/// compositor no lo recibe.
pub struct Bind {
    pub tecla: String,
    pub desc: String,
    /// Expresión Lua que va como acción de `hl.bind`.
    pub hypr: Option<String>,
    /// Lo que va a la derecha del `=` en `[keybinds]`.
    pub umbriel: Option<String>,
}

/// Las apps con ciclo: `Mod+tecla` enfoca la siguiente ventana de la clase, o la lanza si no
/// hay; con Shift, una nueva.
const APPS: &[(&str, &str, &str, &str)] = &[
    ("Mod+Return", "kitty", "kitty", "Kitty"),                                     // no-tr
    ("Mod+B", "brave-origin", "brave-origin", "Brave"),                            // no-tr
    ("Mod+E", "yazi", "kitty --class yazi yazi", "Yazi"),                          // no-tr
    ("Mod+Shift+E", "org.gnome.Nautilus", "nautilus", "Nautilus"),                 // no-tr
    ("Mod+Z", "vesktop", "vesktop", "Vesktop"),                                    // no-tr
    ("Mod+S", "steam", "steam", "Steam"),                                          // no-tr
    ("Mod+O", "md.obsidian.Obsidian", "obsidian", "Obsidian"),                     // no-tr
];

/// Las cuatro direcciones con sus dos teclas (flecha y vim): sufijo de Hyprland, nombre en
/// Umbriel y teclas.
const DIRECCIONES: [(&str, &str, [&str; 2]); 4] = [
    ("l", "left", ["Left", "H"]),   // no-tr
    ("r", "right", ["Right", "L"]), // no-tr
    ("u", "up", ["Up", "K"]),       // no-tr
    ("d", "down", ["Down", "J"]),   // no-tr
];

fn norep(accion: &str) -> String {
    format!("{{ action = \"{accion}\", repeat = false }}") // no-tr
}

fn simple(accion: &str) -> String {
    format!("\"{accion}\"")
}

struct Tabla(Vec<Bind>);

impl Tabla {
    fn ambos(&mut self, tecla: &str, desc: &str, hypr: &str, umbriel: String) {
        self.0.push(Bind {
            tecla: tecla.to_owned(),
            desc: desc.to_owned(),
            hypr: Some(hypr.to_owned()),
            umbriel: Some(umbriel),
        });
    }

    fn solo_hypr(&mut self, tecla: &str, desc: &str, hypr: &str) {
        self.0.push(Bind { tecla: tecla.to_owned(), desc: desc.to_owned(), hypr: Some(hypr.to_owned()), umbriel: None });
    }

    fn solo_umbriel(&mut self, tecla: &str, desc: &str, umbriel: String) {
        self.0.push(Bind { tecla: tecla.to_owned(), desc: desc.to_owned(), hypr: None, umbriel: Some(umbriel) });
    }
}

pub fn tabla(o: &Opciones) -> Vec<Bind> {
    let mut t = Tabla(Vec::new());

    for (tecla, clase, comando, nombre) in APPS {
        t.ambos(
            tecla,
            &format!("{nombre} (Cycle)"), // no-tr
            &format!("run_or_cycle({clase:?}, {comando:?})"), // no-tr
            norep(&format!("spawn umbriel-ciclar {clase} {comando}")), // no-tr
        );
    }
    t.ambos("Mod+Shift+Return", "Kitty (New)", "hl.dsp.exec_cmd(\"kitty\")", norep("spawn kitty")); // no-tr
    t.ambos("Mod+Shift+B", "Brave (New)", "hl.dsp.exec_cmd(\"brave-origin\")", norep("spawn brave-origin")); // no-tr
    t.ambos(
        "Mod+M",
        "System Monitor", // no-tr
        "hl.dsp.exec_cmd(\"kitty --class btop btop\")", // no-tr
        norep("spawn kitty --class btop btop"), // no-tr
    );
    t.ambos(
        "Mod+Shift+M",
        tr!("Silenciar micrófono"),
        "hl.dsp.event(\"morgan volume mute --mic\")", // no-tr
        norep("spawn morganctl volume mute --mic"), // no-tr
    );

    if o.grabador {
        let g = |c: &str| format!("hl.dsp.exec_cmd(\"gsr-ui-cli {c}\")"); // no-tr
        let u = |c: &str| norep(&format!("spawn gsr-ui-cli {c}")); // no-tr
        t.ambos("Mod+G", tr!("Grabar pantalla: arrancar/parar"), &g("toggle-record"), u("toggle-record")); // no-tr
        t.ambos("Mod+R", tr!("Guardar Replay (la jugada)"), &g("replay-save"), u("replay-save")); // no-tr
        t.ambos("Mod+Alt+R", tr!("Replay: arrancar/parar búfer"), &g("toggle-replay"), u("toggle-replay")); // no-tr
        t.ambos("Mod+Alt+Shift+G", tr!("GPU Screen Recorder (overlay)"), &g("toggle-show"), u("toggle-show")); // no-tr
    }

    t.ambos("Mod+Q", tr!("Cerrar ventana"), "hl.dsp.window.close()", norep("window-close"));

    // Foco, mover columna y mover entre monitores: cada dirección con su flecha y su tecla vim.
    for (h, u, teclas) in DIRECCIONES {
        let desc_foco = match u {
            "left" => tr!("Foco izquierda"),
            "right" => tr!("Foco derecha"),
            "up" => tr!("Foco arriba"),
            _ => tr!("Foco abajo"),
        };
        let desc_monitor = match u {
            "left" => tr!("Foco monitor izquierda"),
            "right" => tr!("Foco monitor derecha"),
            "up" => tr!("Foco monitor arriba"),
            _ => tr!("Foco monitor abajo"),
        };
        let desc_mover_monitor = match u {
            "left" => tr!("Mover ventana monitor izquierda"),
            "right" => tr!("Mover ventana monitor derecha"),
            "up" => tr!("Mover ventana monitor arriba"),
            _ => tr!("Mover ventana monitor abajo"),
        };
        for k in teclas {
            t.ambos(&format!("Mod+{k}"), desc_foco, &format!("hl.dsp.layout(\"focus {h}\")"), simple(&format!("window-focus-{u}"))); // no-tr
            t.ambos(
                &format!("Mod+Ctrl+{k}"),
                desc_monitor,
                &format!("hl.dsp.focus({{ monitor = \"{h}\" }})"), // no-tr
                simple(&format!("output-focus-{u}")),
            );
            t.ambos(
                &format!("Mod+Ctrl+Shift+{k}"),
                desc_mover_monitor,
                &format!("hl.dsp.window.move({{ monitor = \"{h}\" }})"), // no-tr
                simple(&format!("window-move-to-output-{u}")),
            );
        }
    }
    // Shift: izquierda y derecha mueven la columna; arriba y abajo, la ventana de workspace.
    for (k, h, u, desc) in [
        ("Left", "l", "left", tr!("Mover columna izquierda")), // no-tr
        ("H", "l", "left", tr!("Mover columna izquierda")),
        ("Right", "r", "right", tr!("Mover columna derecha")), // no-tr
        ("L", "r", "right", tr!("Mover columna derecha")),
    ] {
        t.ambos(&format!("Mod+Shift+{k}"), desc, &format!("hl.dsp.layout(\"swapcol {h}\")"), simple(&format!("column-move-{u}"))); // no-tr
    }
    for k in ["Up", "K", "I"] { // no-tr
        t.ambos(
            &format!("Mod+Shift+{k}"),
            tr!("Mover ventana workspace arriba"),
            "ws_relativo(-1, true)", // no-tr
            simple("window-move-to-workspace-previous"),
        );
    }
    for k in ["Down", "J", "U"] { // no-tr
        t.ambos(
            &format!("Mod+Shift+{k}"),
            tr!("Mover ventana workspace abajo"),
            "ws_relativo(1, true)", // no-tr
            simple("window-move-to-workspace-next"),
        );
    }

    // Workspaces relativos: I y U, Re Pág y Av Pág, y la rueda.
    for k in ["I", "Page_Up"] {
        t.ambos(&format!("Mod+{k}"), tr!("Workspace anterior"), "ws_relativo(-1)", simple("workspace-previous"));
    }
    for k in ["U", "Page_Down"] {
        t.ambos(&format!("Mod+{k}"), tr!("Workspace siguiente"), "ws_relativo(1)", simple("workspace-next"));
    }
    t.ambos(
        "Mod+WheelUp",
        tr!("Workspace anterior (rueda)"),
        "ws_relativo(-1)",
        "{ action = \"workspace-previous\", cooldown_ms = 300 }".into(), // no-tr
    );
    t.ambos(
        "Mod+WheelDown",
        tr!("Workspace siguiente (rueda)"),
        "ws_relativo(1)",
        "{ action = \"workspace-next\", cooldown_ms = 300 }".into(), // no-tr
    );
    t.ambos(
        "Mod+Shift+WheelUp",
        tr!("Columna izquierda (rueda)"),
        "hl.dsp.focus({ direction = \"left\" })", // no-tr
        simple("window-focus-left"),
    );
    t.ambos(
        "Mod+Shift+WheelDown",
        tr!("Columna derecha (rueda)"),
        "hl.dsp.focus({ direction = \"right\" })", // no-tr
        simple("window-focus-right"),
    );

    // Workspaces del 1 al 9 y el 0, que es el 10 (el de los juegos).
    for n in 0..=9 {
        let ws = if n == 0 { 10 } else { n };
        let (ir, mover) = if n == 0 {
            (tr!("Workspace de juegos"), tr!("Mover a workspace de juegos"))
        } else {
            (tr!("Workspace"), tr!("Mover a workspace"))
        };
        let con_n = |d: &str| if n == 0 { d.to_owned() } else { format!("{d} {n}") }; // no-tr
        t.ambos(&format!("Mod+{n}"), &con_n(ir), &format!("hl.dsp.focus({{ workspace = {ws} }})"), simple(&format!("workspace-switch {ws}"))); // no-tr
        for mods in ["Ctrl", "Shift"] { // no-tr
            t.ambos(
                &format!("Mod+{mods}+{n}"),
                &con_n(mover),
                &format!("hl.dsp.window.move({{ workspace = {ws} }})"), // no-tr
                simple(&format!("window-move-to-workspace {ws}")), // no-tr
            );
        }
    }

    // Layout scrolling.
    t.ambos("Mod+F", tr!("Maximizar columna (scrolling)"), "hl.dsp.window.fullscreen({ mode = \"maximized\" })", norep("window-toggle-maximize")); // no-tr
    t.ambos("Mod+Shift+F", tr!("Pantalla completa"), "hl.dsp.window.fullscreen()", norep("window-toggle-fullscreen"));
    t.ambos("Mod+T", tr!("Alternar flotante"), "hl.dsp.window.float()", norep("window-toggle-floating"));
    t.ambos("Mod+W", tr!("Alternar pestaña/grupo"), "hl.dsp.group.toggle()", norep("column-toggle-tabbed"));
    t.ambos("Mod+C", tr!("Centrar columna"), "hl.dsp.layout(\"center\")", norep("column-center"));
    t.solo_hypr("Mod+Ctrl+C", tr!("Ajustar columnas visibles"), "hl.dsp.layout(\"fit visible\")"); // no-tr
    t.solo_hypr("Mod+Ctrl+F", tr!("Expandir columna a espacio disponible"), "hl.dsp.layout(\"fit expand\")"); // no-tr
    t.ambos("Mod+Minus", tr!("Reducir ancho columna"), "hl.dsp.layout(\"colresize -0.1\")", simple("window-modify-primary-extent -0.1")); // no-tr
    t.ambos("Mod+plus", tr!("Aumentar ancho columna"), "hl.dsp.layout(\"colresize +0.1\")", simple("window-modify-primary-extent +0.1")); // no-tr
    t.ambos(
        "Mod+Shift+Minus",
        tr!("Reducir alto ventana"),
        "hl.dsp.window.resize({ x = 0, y = -40, relative = true })", // no-tr
        simple("window-modify-secondary-extent -0.05"), // no-tr
    );
    t.ambos(
        "Mod+Shift+plus",
        tr!("Aumentar alto ventana"),
        "hl.dsp.window.resize({ x = 0, y = 40, relative = true })", // no-tr
        simple("window-modify-secondary-extent +0.05"), // no-tr
    );
    // Cada workspace de Umbriel guarda su layout; Hyprland tiene uno solo para todos.
    t.solo_umbriel("Mod+D", tr!("Alternar el workspace entre scrolling y mosaico"), norep("workspace-set-layout toggle scrolling dwindle")); // no-tr

    // Sesión.
    t.solo_umbriel("Ctrl+Alt+Delete", tr!("Cerrar sesión"), norep("session-quit skip-confirmation")); // no-tr
    if o.uwsm {
        t.solo_hypr("Ctrl+Alt+Delete", tr!("Cerrar sesión"), "hl.dsp.exec_cmd(\"uwsm stop\")"); // no-tr
    }
    // Si un juego se queda con el teclado, esto devuelve los atajos.
    t.solo_umbriel(
        "Mod+Shift+Escape",
        tr!("Devolver los atajos si un juego se quedó con el teclado"),
        "{ action = \"shortcuts-inhibit-toggle\", allow_when_inhibited = true, repeat = false }".into(), // no-tr
    );
    t.0
}

/// La tecla en la notación de Hyprland: `Mod+Shift+WheelUp` es `SUPER + SHIFT + mouse_up`.
pub fn a_hypr(tecla: &str) -> String {
    tecla
        .split('+')
        .map(|p| match p {
            "Mod" => "SUPER", // no-tr
            "Shift" => "SHIFT", // no-tr
            "Ctrl" => "CTRL", // no-tr
            "Alt" => "ALT", // no-tr
            "Minus" => "minus", // no-tr
            "WheelUp" => "mouse_up",
            "WheelDown" => "mouse_down",
            otra => otra,
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Dos nombres para la misma tecla: sin esto, `Mod+Space` y `Super+space` serían dos.
fn clave(tecla: &str) -> String {
    crate::setup_umbriel::normalizar(tecla)
}

const CABECERA_LUA: &str = "-- morgan-atajos: los atajos de ventanas de morgan (foco y movimiento de columnas, workspaces,\n\
-- monitores, layout scrolling, apps que ciclan). Lo escribe `morgan setup` una vez y no lo\n\
-- reescribe: se edita a mano. morgan.lua lo carga, y lo que morgan pone después pisa sus teclas.\n\n";

/// Las funciones que usan los atajos de Hyprland y el layout que esperan.
const AYUDAS_LUA: &str = r#"-- Los atajos mueven columnas: piden el layout scrolling.
hl.config({
    general = { layout = "scrolling" },
    scrolling = { focus_fit_method = 1, wrap_focus = false, wrap_swapcol = false },
})

-- Enfoca la app si ya tiene ventanas, ciclando entre ellas en orden de creación, y la lanza
-- si no. Corre adentro del compositor, sin fork ni hyprctl.
local function run_or_cycle(class, cmd)
    return function()
        local wins = {}
        for _, w in ipairs(hl.get_windows()) do
            if w.mapped and (w.class == class or w.initial_class == class) then
                wins[#wins + 1] = w
            end
        end
        if #wins == 0 then
            hl.exec_cmd(cmd)
            return
        end
        table.sort(wins, function(a, b) return a.stable_id < b.stable_id end)
        local active, idx = hl.get_active_window(), 0
        if active then
            for i, w in ipairs(wins) do
                if w.stable_id == active.stable_id then idx = i break end
            end
        end
        hl.dispatch(hl.dsp.focus({ window = wins[idx % #wins + 1] }))
    end
end

-- Workspaces relativos con tope: del 1 al 9, sin crear otros ni dar la vuelta. El 10 (el de
-- los juegos) se entra con Mod+0.
local WS_MAX = 9
local function ws_relativo(delta, mover)
    return function()
        local w = hl.get_active_workspace()
        if not w or w.special then return end
        local destino = w.id + delta
        if destino < 1 or destino > WS_MAX then return end
        if mover then
            hl.dispatch(hl.dsp.window.move({ workspace = destino }))
        else
            hl.dispatch(hl.dsp.focus({ workspace = destino }))
        end
    end
end

"#;

/// `morgan-atajos.lua`, sin las teclas de `excluidas` (las que el usuario ya tiene).
pub fn lua(o: &Opciones, excluidas: &[String]) -> String {
    let mut s = String::from(CABECERA_LUA);
    s.push_str(AYUDAS_LUA);
    for b in tabla(o) {
        let Some(accion) = &b.hypr else { continue };
        if excluidas.contains(&clave(&b.tecla)) {
            continue;
        }
        s.push_str(&format!("hl.bind({:?}, {accion}, {{ desc = {:?} }})\n", a_hypr(&b.tecla), b.desc)); // no-tr
    }
    s
}

const CABECERA_TOML: &str = "# morgan-atajos: los atajos de ventanas de morgan (foco y movimiento de columnas, workspaces,\n\
# monitores, layout scrolling, apps que ciclan). Lo escribe `morgan setup` una vez y no lo\n\
# reescribe: se edita a mano. El cargador lo incluye antes que user.toml, así que lo que está\n\
# en user.toml pisa estas teclas. Las apps que ciclan usan `umbriel-ciclar` (pide `jq`).\n\n";

/// `morgan-atajos.toml`.
pub fn toml(o: &Opciones) -> String {
    let mut s = String::from(CABECERA_TOML);
    s.push_str("[keybinds]\n");
    for b in tabla(o) {
        if let Some(accion) = &b.umbriel {
            s.push_str(&format!("{:?} = {accion}\n", b.tecla)); // no-tr
        }
    }
    s
}

/// Las teclas que ocupa en Umbriel: la plantilla del paquete comenta las suyas que chocan.
pub fn teclas_umbriel(o: &Opciones) -> Vec<String> {
    tabla(o).into_iter().filter(|b| b.umbriel.is_some()).map(|b| b.tecla).collect()
}

/// Las teclas que el usuario ya usa en Hyprland, de leer sus `.lua` (menos los de morgan).
/// Entiende `hl.bind("SUPER + Q", …)` y `hl.bind(mod .. " + Q", …)`; lo que arma la tecla de
/// otra forma no se ve, y ahí quedan las dos acciones.
pub fn teclas_de_usuario_hypr(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    for archivo in archivos_lua(dir, 0) {
        let nombre = archivo.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if nombre == "morgan.lua" || nombre == "morgan-atajos.lua" { // no-tr
            continue;
        }
        let Ok(texto) = std::fs::read_to_string(&archivo) else { continue };
        for linea in texto.lines().map(str::trim_start).filter(|l| !l.starts_with("--")) {
            if let Some(i) = linea.find("hl.bind(") { // no-tr
                if let Some(t) = tecla_del_bind(&linea[i + "hl.bind(".len()..]) { // no-tr
                    out.push(clave(&t));
                }
            }
        }
    }
    out
}

fn archivos_lua(dir: &std::path::Path, hondo: u8) -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return v };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() && hondo < 2 {
            v.extend(archivos_lua(&p, hondo + 1));
        } else if p.extension().is_some_and(|x| x == "lua") { // no-tr
            v.push(p);
        }
    }
    v
}

/// La tecla de lo que sigue a `hl.bind(`: un literal, o `variable .. "literal"`.
fn tecla_del_bind(resto: &str) -> Option<String> {
    let resto = resto.trim_start();
    let (previo, literal) = if resto.starts_with('"') {
        ("", resto)
    } else {
        let (var, tras) = resto.split_once("..")?;
        let var = var.trim();
        if var.is_empty() || !var.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        // Una variable que se llama `mod` (o parecido) es la tecla Super: es lo que hay en una
        // config típica.
        ("SUPER", tras.trim_start())
    };
    let literal = literal.strip_prefix('"')?;
    let fin = literal.find('"')?;
    Some(format!("{previo}{}", &literal[..fin]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todo() -> Opciones {
        Opciones { grabador: true, uwsm: true }
    }

    #[test]
    fn las_teclas_de_hyprland() {
        assert_eq!(a_hypr("Mod+Shift+WheelUp"), "SUPER + SHIFT + mouse_up");
        assert_eq!(a_hypr("Mod+Minus"), "SUPER + minus");
        assert_eq!(a_hypr("Mod+plus"), "SUPER + plus");
        assert_eq!(a_hypr("Ctrl+Alt+Delete"), "CTRL + ALT + Delete");
        assert_eq!(a_hypr("Mod+Page_Up"), "SUPER + Page_Up");
    }

    #[test]
    fn ninguna_tecla_se_repite_en_un_compositor() {
        let t = tabla(&todo());
        for (nombre, f) in [("hypr", (|b: &Bind| b.hypr.is_some()) as fn(&Bind) -> bool), ("umbriel", |b| b.umbriel.is_some())] {
            let mut vistas = std::collections::HashSet::new();
            for b in t.iter().filter(|b| f(b)) {
                assert!(vistas.insert(clave(&b.tecla)), "{nombre}: {} está dos veces", b.tecla);
            }
        }
    }

    #[test]
    fn no_pisa_las_teclas_de_morgan() {
        let a = crate::config::Atajos::default();
        let propias: Vec<String> = crate::launcher::binds::propios(&a).into_iter().map(|p| clave(&p.tecla)).collect();
        for b in tabla(&todo()) {
            assert!(!propias.contains(&clave(&b.tecla)), "{} es de morgan", b.tecla);
        }
    }

    #[test]
    fn el_toml_es_valido_y_trae_las_teclas() {
        let t: toml::Table = toml::from_str(&toml(&todo())).unwrap();
        let k = t["keybinds"].as_table().unwrap();
        assert!(k.contains_key("Mod+Q"));
        assert!(k.contains_key("Mod+Return"));
        assert!(k.contains_key("Mod+WheelUp"));
        assert!(k.contains_key("Mod+D"));
    }

    #[test]
    fn el_lua_deja_afuera_lo_que_el_usuario_ya_tiene() {
        let sin = lua(&todo(), &[clave("SUPER + Q")]);
        assert!(!sin.contains("\"SUPER + Q\""));
        assert!(lua(&todo(), &[]).contains("hl.bind(\"SUPER + Q\""));
    }

    #[test]
    fn sin_grabador_ni_uwsm_faltan_esos_atajos() {
        let poco = Opciones::default();
        assert!(!lua(&poco, &[]).contains("gsr-ui-cli"));
        assert!(!lua(&poco, &[]).contains("uwsm stop"));
        assert!(!toml(&poco).contains("gsr-ui-cli"));
        assert!(toml(&poco).contains("session-quit"));
    }

    #[test]
    fn la_tecla_de_un_bind_del_usuario() {
        assert_eq!(tecla_del_bind("\"SUPER + Q\", fn)").as_deref(), Some("SUPER + Q"));
        assert_eq!(tecla_del_bind("mod .. \" + Q\", fn)").as_deref(), Some("SUPER + Q"));
        assert_eq!(tecla_del_bind("armar(x), fn)"), None);
    }
}
