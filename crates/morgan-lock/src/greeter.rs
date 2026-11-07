//! Lo que el greeter sabe del sistema sin entrar a ningún home: las cuentas (`/etc/passwd`),
//! las sesiones (`/usr/share/wayland-sessions`), el teclado (`00-keyboard.conf` de
//! localed), las copias de cada cuenta en `/var/lib/morgan-greet/usuarios/` y la config del
//! Hyprland donde corre.
//!
//! Como DMS Greeter: `morgan-greet` lanza el compositor con una config que arma en el
//! momento, y el compositor lanza la pantalla; al terminar la pantalla, el compositor sale y
//! greetd arranca la sesión.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::perfil::Rutas;

pub const BASE: &str = "/var/lib/morgan-greet";

#[derive(Debug, Clone, PartialEq)]
pub struct Usuario {
    pub nombre: String,
    pub uid: u32,
}

/// Las cuentas de personas: uid entre 1000 y 60000 (los límites de `login.defs` por
/// defecto) y un shell de login, no `nologin` ni `false`.
pub fn usuarios(passwd: &str) -> Vec<Usuario> {
    passwd
        .lines()
        .filter_map(|l| {
            let c: Vec<&str> = l.split(':').collect();
            if c.len() < 7 {
                return None;
            }
            let uid: u32 = c[2].parse().ok()?;
            let shell = c[6];
            let login =
                !shell.is_empty() && !shell.ends_with("nologin") && !shell.ends_with("false");
            ((1000..60000).contains(&uid) && login).then(|| Usuario {
                nombre: c[0].to_owned(),
                uid,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sesion {
    /// El nombre del archivo sin `.desktop`: lo que se recuerda.
    pub id: String,
    pub nombre: String,
    pub cmd: Vec<String>,
    /// `DesktopNames`, para `XDG_CURRENT_DESKTOP`.
    pub escritorios: Vec<String>,
}

impl Sesion {
    /// Lo que se suma al entorno que arma PAM.
    pub fn entorno(&self) -> Vec<String> {
        let mut e = vec!["XDG_SESSION_TYPE=wayland".to_owned()];
        if !self.escritorios.is_empty() {
            e.push(format!(
                "XDG_CURRENT_DESKTOP={}",
                self.escritorios.join(":")
            ));
            e.push(format!("XDG_SESSION_DESKTOP={}", self.escritorios[0]));
        }
        e
    }
}

/// Una entrada de `wayland-sessions`. Sin `Exec`, oculta o con un `TryExec` que no está:
/// no se ofrece.
pub fn leer_sesion(id: &str, texto: &str, existe: impl Fn(&str) -> bool) -> Option<Sesion> {
    let mut en_entrada = false;
    let (mut nombre, mut exec, mut escritorios) = (None, None, Vec::new());
    for l in texto.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            en_entrada = l == "[Desktop Entry]";
            continue;
        }
        if !en_entrada {
            continue;
        }
        let Some((k, v)) = l.split_once('=') else {
            continue;
        };
        match k.trim() {
            "Name" => nombre = Some(v.trim().to_owned()),
            "Exec" => exec = Some(v.trim().to_owned()),
            "DesktopNames" => {
                escritorios = v
                    .split(';')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            "NoDisplay" | "Hidden" if v.trim() == "true" => return None,
            "TryExec" if !existe(v.trim()) => return None,
            _ => {}
        }
    }
    let cmd = partir_exec(&exec?);
    if cmd.is_empty() {
        return None;
    }
    Some(Sesion {
        id: id.to_owned(),
        nombre: nombre.unwrap_or_else(|| id.to_owned()),
        cmd,
        escritorios,
    })
}

/// `Exec` de la especificación de entradas de escritorio: palabras separadas por espacios,
/// comillas dobles con `\` de escape, y los códigos de campo (`%f`, `%U`…) fuera, que en una
/// sesión no tienen con qué reemplazarse.
pub fn partir_exec(exec: &str) -> Vec<String> {
    let mut partes = Vec::new();
    let mut actual = String::new();
    let (mut comillas, mut hay) = (false, false);
    let mut cs = exec.chars();
    while let Some(c) = cs.next() {
        match c {
            '"' => {
                comillas = !comillas;
                hay = true;
            }
            '\\' if comillas => {
                if let Some(n) = cs.next() {
                    actual.push(n);
                }
            }
            c if c.is_whitespace() && !comillas => {
                if hay {
                    partes.push(std::mem::take(&mut actual));
                    hay = false;
                }
            }
            c => {
                actual.push(c);
                hay = true;
            }
        }
    }
    if hay {
        partes.push(actual);
    }
    partes.retain(|p| !(p.len() == 2 && p.starts_with('%')));
    partes.iter_mut().for_each(|p| *p = p.replace("%%", "%"));
    partes
}

/// Las sesiones de las carpetas, sin repetir id (gana la primera carpeta), por nombre.
pub fn sesiones(carpetas: &[PathBuf]) -> Vec<Sesion> {
    let mut v: Vec<Sesion> = Vec::new();
    for d in carpetas {
        let Ok(entradas) = std::fs::read_dir(d) else {
            continue;
        };
        let mut archivos: Vec<_> = entradas.flatten().map(|e| e.path()).collect();
        archivos.sort();
        for p in archivos {
            if p.extension().is_none_or(|e| e != "desktop") {
                continue;
            }
            let Some(id) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if v.iter().any(|s| s.id == id) {
                continue;
            }
            let Ok(texto) = std::fs::read_to_string(&p) else {
                continue;
            };
            if let Some(s) = leer_sesion(id, &texto, en_el_path) {
                v.push(s);
            }
        }
    }
    v.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    v
}

pub fn en_el_path(prog: &str) -> bool {
    if prog.contains('/') {
        return Path::new(prog).is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
}

/// La que se elige si no hay una recordada: la que maneja uwsm, si la hay (arranca la sesión
/// como unidades de systemd, como el inicio automático de hoy); si no, la primera.
pub fn por_defecto(s: &[Sesion]) -> usize {
    s.iter().position(|s| s.id.ends_with("-uwsm")).unwrap_or(0)
}

/// Lo último que entró, para empezar por ahí. Lo escribe el greeter (dueño de `BASE`).
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Recuerdo {
    pub usuario: Option<String>,
    pub sesion: Option<String>,
}

impl Recuerdo {
    pub fn leer() -> Recuerdo {
        std::fs::read_to_string(Path::new(BASE).join("estado.toml"))
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn guardar(&self) -> std::io::Result<()> {
        let t = toml::to_string(self).map_err(std::io::Error::other)?;
        let ruta = Path::new(BASE).join("estado.toml");
        let tmp = ruta.with_extension("toml.nuevo");
        std::fs::write(&tmp, t)?;
        std::fs::rename(tmp, ruta)
    }
}

/// Las copias de una cuenta (`usuarios/<nombre>/`). Solo vale un archivo cuyo dueño es esa
/// cuenta: la carpeta es de todos (sticky bit) y nadie puede ponerle un fondo a otro. Uno que
/// no cumple se cambia por una ruta que no existe, y queda el valor de fábrica.
pub fn rutas_de(u: &Usuario, base: &Path) -> Rutas {
    let dir = base.join("usuarios").join(&u.nombre);
    let propia = |nombre: &str| {
        let p = dir.join(nombre);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_file() && m.uid() == u.uid => p,
            _ => PathBuf::from("/nonexistent/morgan-greet").join(nombre),
        }
    };
    Rutas {
        tema: propia("theme.css"),
        fondo: propia("fondo.png"),
        config: propia("bloqueo.toml"),
    }
}

/// El teclado del sistema, de lo que escribe `localectl set-x11-keymap`.
#[derive(Debug, Default, PartialEq)]
pub struct Teclado {
    pub layout: String,
    pub variant: String,
    pub options: String,
}

pub fn teclado(conf: &str) -> Teclado {
    let mut t = Teclado::default();
    for l in conf.lines() {
        let mut p = l.split('"').skip(1).step_by(2);
        let l = l.trim();
        if !l.starts_with("Option") {
            continue;
        }
        let (Some(k), Some(v)) = (p.next(), p.next()) else {
            continue;
        };
        match k {
            "XkbLayout" => t.layout = v.to_owned(),
            "XkbVariant" => t.variant = v.to_owned(),
            "XkbOptions" => t.options = v.to_owned(),
            _ => {}
        }
    }
    t
}

/// Una cadena de Lua entre corchetes largos, con tantos `=` como haga falta para que el
/// contenido no la cierre.
fn lua(s: &str) -> String {
    let mut n = 0;
    while s.contains(&format!("]{}]", "=".repeat(n))) {
        n += 1;
    }
    let e = "=".repeat(n);
    format!("[{e}[{s}]{e}]")
}

/// La config del Hyprland del greeter: todas las salidas, sin logo ni fondo de Hyprland, el
/// teclado del sistema, y la pantalla al arrancar; cuando la pantalla termina, Hyprland sale.
/// Las claves salen de `hl.meta.lua` de Hyprland 0.57. `highrr` no está ahí: sale del fuente
/// de 0.57.1 (`config/shared/monitor/Parser.cpp`, que también lee `hl.monitor`): la frecuencia
/// más alta de cada salida y, entre esas, la mayor resolución. Con `preferred` el monitor
/// elige, y uno de 300 Hz suele pedir 60.
pub fn config_hyprland(pantalla: &[String], t: &Teclado) -> String {
    let cmd = pantalla
        .iter()
        .map(|a| format!("'{}'", a.replace('\'', r"'\''")))
        .collect::<Vec<_>>()
        .join(" ");
    let cmd = format!("{cmd}; hyprctl dispatch 'hl.dsp.exit()'");
    format!(
        r#"-- Generado por morgan-greet: se reescribe en cada arranque.
hl.monitor({{ output = "", mode = "highrr", position = "auto", scale = 1 }})
hl.config({{
    misc = {{
        disable_hyprland_logo = true,
        disable_splash_rendering = true,
        disable_hyprland_guiutils_check = true,
        disable_watchdog_warning = true,
        force_default_wallpaper = 0,
        background_color = "0x000000",
    }},
    animations = {{ enabled = false }},
    input = {{ kb_layout = {}, kb_variant = {}, kb_options = {} }},
}})
hl.on("hyprland.start", function()
    hl.exec_cmd({})
end)
"#,
        lua(&t.layout),
        lua(&t.variant),
        lua(&t.options),
        lua(&cmd),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_cuentas_de_personas() {
        let p = "root:x:0:0::/root:/bin/bash\n\
                 greeter:x:959:959::/var/lib/greeter:/usr/bin/nologin\n\
                 anon:x:1000:1000:anon:/home/anon:/usr/bin/fish\n\
                 nobody:x:65534:65534::/:/usr/bin/nologin\n\
                 servicio:x:1001:1001::/srv:/bin/false\n\
                 roto:x:1002\n";
        assert_eq!(
            usuarios(p),
            [Usuario {
                nombre: "anon".into(),
                uid: 1000
            }]
        );
    }

    #[test]
    fn la_sesion_de_uwsm_de_esta_maquina() {
        let t = "[Desktop Entry]\nName=Hyprland (uwsm-managed)\nComment=x\n\
                 Exec=uwsm start -e -D Hyprland hyprland.desktop\nTryExec=uwsm\n\
                 DesktopNames=Hyprland\nType=Application\n";
        let s = leer_sesion("hyprland-uwsm", t, |p| p == "uwsm").unwrap();
        assert_eq!(
            s.cmd,
            ["uwsm", "start", "-e", "-D", "Hyprland", "hyprland.desktop"]
        );
        assert_eq!(
            s.entorno(),
            [
                "XDG_SESSION_TYPE=wayland",
                "XDG_CURRENT_DESKTOP=Hyprland",
                "XDG_SESSION_DESKTOP=Hyprland"
            ]
        );
        assert!(
            leer_sesion("x", t, |_| false).is_none(),
            "TryExec que no está"
        );
        assert!(leer_sesion("x", "[Desktop Entry]\nExec=a\nNoDisplay=true\n", |_| true).is_none());
        assert!(leer_sesion("x", "[Desktop Entry]\nName=sin exec\n", |_| true).is_none());
        let otra = "[Desktop Entry]\nExec=a\n[Desktop Action x]\nExec=b\nName=no\n";
        let s = leer_sesion("x", otra, |_| true).unwrap();
        assert_eq!((s.cmd, s.nombre), (vec!["a".to_owned()], "x".to_owned()));
    }

    #[test]
    fn exec_con_comillas_y_codigos() {
        assert_eq!(
            partir_exec(r#"sh -c "echo \"hola mundo\"" %U 100%%"#),
            ["sh", "-c", r#"echo "hola mundo""#, "100%"]
        );
        assert_eq!(partir_exec(r#"a "" b"#), ["a", "", "b"]);
    }

    #[test]
    fn uwsm_primero_si_esta() {
        let s = |id: &str| Sesion {
            id: id.into(),
            nombre: id.into(),
            cmd: vec!["x".into()],
            escritorios: vec![],
        };
        assert_eq!(por_defecto(&[s("hyprland"), s("hyprland-uwsm")]), 1);
        assert_eq!(por_defecto(&[s("niri")]), 0);
    }

    #[test]
    fn el_teclado_de_localed() {
        let c = "Section \"InputClass\"\n  Identifier \"system-keyboard\"\n\
                 Option \"XkbLayout\" \"latam\"\n  Option \"XkbOptions\" \"caps:escape\"\nEndSection\n";
        assert_eq!(
            teclado(c),
            Teclado {
                layout: "latam".into(),
                variant: String::new(),
                options: "caps:escape".into()
            }
        );
    }

    #[test]
    fn la_config_no_se_rompe_con_lo_que_lleva_adentro() {
        assert_eq!(lua("a]]b"), "[=[a]]b]=]");
        let c = config_hyprland(
            &[
                "/usr/lib/morgan/morgan-greet".into(),
                "pantalla".into(),
                "it's".into(),
            ],
            &Teclado {
                layout: "latam".into(),
                ..Default::default()
            },
        );
        assert!(c.contains(r#"hl.exec_cmd([['/usr/lib/morgan/morgan-greet' 'pantalla' 'it'\''s'; hyprctl dispatch 'hl.dsp.exit()']])"#), "{c}");
        assert!(c.contains("kb_layout = [[latam]]"));
        assert!(
            c.contains(r#"mode = "highrr""#),
            "a la frecuencia más alta, no a la que propone el monitor"
        );
    }

    #[test]
    fn solo_valen_las_copias_propias() {
        let base = std::env::temp_dir().join(format!("morgan-greet-test-{}", std::process::id()));
        let dir = base.join("usuarios/yo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("theme.css"), "x").unwrap();
        let mio = Usuario {
            nombre: "yo".into(),
            uid: unsafe { libc::getuid() },
        };
        let r = rutas_de(&mio, &base);
        assert_eq!(r.tema, dir.join("theme.css"));
        assert!(!r.fondo.exists());
        let otro = Usuario {
            uid: mio.uid + 1,
            ..mio
        };
        assert!(
            !rutas_de(&otro, &base).tema.exists(),
            "de otro dueño no vale"
        );
        std::fs::remove_dir_all(base).unwrap();
    }
}
