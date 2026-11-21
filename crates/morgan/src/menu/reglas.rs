//! Reglas por app (Escritorio › Reglas por app): solo las que se crean acá, guardadas en
//! `[[window_rules]]` de config.toml; las de la config de Hyprland del usuario no se leen.
//! Cada una es un `hl.window_rule` en `morgan.lua`, que va al final y pisa lo de antes.

use super::arbol::{Cambio, Control, Fila, Opcion, Que, Seccion, ajuste, opcion};
use crate::aplicacion::{self, App};
use crate::config::ReglaApp;

pub const TAMANOS: [u32; 4] = [0, 40, 60, 80];
pub const OPACIDADES: [f64; 5] = [0.0, 1.0, 0.95, 0.9, 0.8];

#[derive(Debug, Clone, PartialEq)]
pub enum Campo {
    Workspace(u32),
    Flotante(bool),
    Tamano(u32),
    Opacidad(f64),
    SinAnimacion(bool),
}

impl Campo {
    pub fn aplicar(&self, r: &mut ReglaApp) {
        match *self {
            Campo::Workspace(w) => r.workspace = w,
            Campo::Flotante(f) => r.flotante = f,
            Campo::Tamano(t) => r.tamano = t,
            Campo::Opacidad(o) => r.opacidad = o,
            Campo::SinAnimacion(s) => r.sin_animacion = s,
        }
    }
}

pub fn lua(reglas: &[ReglaApp], buscar: &dyn Fn(&str) -> Option<App>) -> String {
    let mut out = String::new();
    for (n, r) in reglas.iter().enumerate() {
        let Some(app) = buscar(&r.app) else { continue };
        let mut campos = Vec::new();
        if r.workspace > 0 {
            campos.push(format!("workspace = \"{}\"", r.workspace)); // no-tr
        }
        if r.flotante {
            campos.push("float = true".to_owned()); // no-tr
            if r.tamano > 0 {
                let f = f64::from(r.tamano) / 100.0;
                campos.push(format!("size = \"monitor_w*{f} monitor_h*{f}\"")); // no-tr
                campos.push("center = true".to_owned()); // no-tr
            }
        }
        if r.opacidad > 0.0 {
            // `override`: pisa la opacidad general en vez de multiplicarse con ella.
            let o = r.opacidad;
            campos.push(format!("opacity = \"{o} override {o} override\"")); // no-tr
        }
        if r.sin_animacion {
            campos.push("no_anim = true".to_owned()); // no-tr
        }
        if campos.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "hl.window_rule({{ name = \"morgan-regla-{n}\", match = {{ class = {:?} }}, {} }})\n", // no-tr
            aplicacion::patron(&app.clases),
            campos.join(", ")
        ));
    }
    if out.is_empty() {
        return out;
    }
    format!("\n-- ─── Reglas por app (menú) ───\n{out}") // no-tr
}

/// Las mismas reglas para Umbriel, como `[[window_rule]]` de `morgan.toml`. Umbriel no tiene
/// una clave para «sin animación»: esa parte no se escribe (`window-rules.md`). Un flotante
/// sin posición abre centrado.
pub fn toml(reglas: &[ReglaApp], buscar: &dyn Fn(&str) -> Option<App>) -> String {
    let mut out = String::new();
    for r in reglas {
        let Some(app) = buscar(&r.app) else { continue };
        let mut campos = Vec::new();
        if r.workspace > 0 {
            campos.push(format!("default_workspace = {}", r.workspace)); // no-tr
        }
        if r.flotante {
            campos.push("default_floating = true".to_owned()); // no-tr
            if r.tamano > 0 {
                let f = f64::from(r.tamano) / 100.0;
                campos.push(format!("default_floating_size = {{ width = {f}, height = {f} }}")); // no-tr
            }
        }
        if r.opacidad > 0.0 {
            campos.push(format!("opacity = {}", r.opacidad)); // no-tr
        }
        if campos.is_empty() {
            continue;
        }
        let patron = serde_json::to_string(&aplicacion::patron(&app.clases)).unwrap_or_default();
        out.push_str(&format!("\n[[window_rule]]\nmatch.app_id = {patron}\n{}\n", campos.join("\n"))); // no-tr
    }
    if out.is_empty() {
        return out;
    }
    format!("\n# Reglas por app (menú)\n{}", out.trim_start_matches('\n')) // no-tr
}

fn porcentaje(v: f64) -> String {
    format!("{} %", (v * 100.0).round())
}

fn texto_tamano(t: u32) -> String {
    if t == 0 {
        tr!("el que pida la app").to_owned()
    } else {
        trf!("{} % de la pantalla, centrada", t)
    }
}

fn texto_opacidad(o: f64) -> String {
    if o == 0.0 {
        tr!("la de siempre").to_owned()
    } else {
        porcentaje(o)
    }
}

/// Lo que tiene puesto una regla, en una línea.
pub fn resumen(r: &ReglaApp) -> String {
    let mut v = Vec::new();
    if r.workspace > 0 {
        v.push(trf!("workspace {}", r.workspace));
    }
    if r.flotante {
        v.push(tr!("flotante").to_owned());
        if r.tamano > 0 {
            v.push(trf!("{} %", r.tamano));
        }
    }
    if r.opacidad > 0.0 {
        v.push(trf!("opacidad {}", porcentaje(r.opacidad)));
    }
    if r.sin_animacion {
        v.push(tr!("sin animación").to_owned());
    }
    if v.is_empty() {
        return tr!("sin nada puesto todavía").to_owned();
    }
    v.join(" · ")
}

/// Una regla es un renglón con todo en su lista: cada valor marcado es el puesto, y elegir
/// otro lo cambia; «Quitar la regla» la borra.
/// `animacion`: si el compositor puede sacarle la animación (Umbriel no).
fn fila(n: usize, r: &ReglaApp, nombre: &str, animacion: bool) -> Fila {
    let cambio = |c: Campo| Que::Config(Cambio::Regla(n, c));
    let mut o: Vec<Opcion> = Vec::new();
    for w in 0..=10 {
        let t = if w == 0 {
            tr!("Workspace: el que toque").to_owned()
        } else {
            trf!("Workspace {}", w)
        };
        o.push(opcion(&t, "", cambio(Campo::Workspace(w))).activa(r.workspace == w));
    }
    o.push(opcion(tr!("Flotante"), "", cambio(Campo::Flotante(!r.flotante))).activa(r.flotante));
    if r.flotante {
        for t in TAMANOS {
            o.push(
                opcion(
                    &trf!("Tamaño: {}", texto_tamano(t)),
                    "",
                    cambio(Campo::Tamano(t)),
                )
                .activa(r.tamano == t),
            );
        }
    }
    for op in OPACIDADES {
        o.push(
            opcion(
                &trf!("Opacidad: {}", texto_opacidad(op)),
                "",
                cambio(Campo::Opacidad(op)),
            )
            .activa(r.opacidad == op),
        );
    }
    if animacion {
        o.push(
            opcion(
                tr!("Sin animación"),
                "",
                cambio(Campo::SinAnimacion(!r.sin_animacion)),
            )
            .activa(r.sin_animacion),
        );
    }
    o.push(opcion(
        tr!("Quitar la regla"),
        "",
        Que::Config(Cambio::QuitarRegla(n)),
    ));
    ajuste(nombre, &resumen(r), Control::Lista(o))
}

/// `apps`: id y nombre de las que muestra el launcher.
pub fn filas(reglas: &[ReglaApp], apps: &[(String, String)]) -> Vec<Fila> {
    filas_con(reglas, apps, true)
}

fn filas_con(reglas: &[ReglaApp], apps: &[(String, String)], animacion: bool) -> Vec<Fila> {
    let nombre = |id: &str| {
        apps.iter().find(|(i, _)| i == id).map_or_else(
            || id.trim_end_matches(".desktop").to_owned(),
            |(_, n)| n.clone(),
        )
    };
    let mut v: Vec<Fila> = reglas
        .iter()
        .enumerate()
        .map(|(n, r)| fila(n, r, &nombre(&r.app), animacion))
        .collect();
    let libres: Vec<Opcion> = apps
        .iter()
        .filter(|(id, _)| !reglas.iter().any(|r| &r.app == id))
        .map(|(id, n)| opcion(n, "", Que::Config(Cambio::NuevaRegla(id.clone()))))
        .collect();
    v.push(ajuste(
        tr!("Nueva regla"),
        tr!("para las ventanas de una app: workspace, flotante, tamaño, opacidad"),
        Control::Lista(libres),
    ));
    v
}

pub fn seccion(reglas: &[ReglaApp], apps: &[(String, String)]) -> Seccion {
    Seccion {
        titulo: morgan_idioma::t(tr_const!("Reglas por app")),
        filas: filas(reglas, apps),
    }
}

/// En Umbriel, sin «Sin animación».
pub fn seccion_umbriel(reglas: &[ReglaApp], apps: &[(String, String)]) -> Seccion {
    Seccion {
        titulo: morgan_idioma::t(tr_const!("Reglas por app")),
        filas: filas_con(reglas, apps, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str) -> Option<App> {
        (id == "mpv.desktop").then(|| App {
            nombre: "mpv".into(),
            clases: vec!["mpv".into()],
            lanzar: String::new(),
        })
    }

    #[test]
    fn lua_de_las_reglas() {
        let reglas = vec![
            ReglaApp {
                app: "mpv.desktop".into(),
                workspace: 3,
                flotante: true,
                tamano: 60,
                opacidad: 0.9,
                sin_animacion: true,
            },
            // Sin nada puesto, o de una app que ya no está: nada.
            ReglaApp {
                app: "mpv.desktop".into(),
                ..Default::default()
            },
            ReglaApp {
                app: "ya-no.desktop".into(),
                flotante: true,
                ..Default::default()
            },
        ];
        let l = lua(&reglas, &app);
        let lineas: Vec<&str> = l
            .lines()
            .filter(|x| x.starts_with("hl.window_rule"))
            .collect();
        assert_eq!(
            lineas,
            [concat!(
                r#"hl.window_rule({ name = "morgan-regla-0", match = { class = "^(mpv)$" }, "#,
                r#"workspace = "3", float = true, size = "monitor_w*0.6 monitor_h*0.6", center = true, "#,
                r#"opacity = "0.9 override 0.9 override", no_anim = true })"#
            )]
        );
        assert_eq!(lua(&[], &app), "");
    }

    #[test]
    fn toml_de_las_reglas() {
        let reglas = vec![
            ReglaApp {
                app: "mpv.desktop".into(),
                workspace: 3,
                flotante: true,
                tamano: 60,
                opacidad: 0.9,
                sin_animacion: true,
            },
            ReglaApp { app: "mpv.desktop".into(), sin_animacion: true, ..Default::default() },
            ReglaApp { app: "ya-no.desktop".into(), flotante: true, ..Default::default() },
        ];
        let t = toml(&reglas, &app);
        assert_eq!(
            t,
            "\n# Reglas por app (menú)\n[[window_rule]]\nmatch.app_id = \"^(mpv)$\"\ndefault_workspace = 3\ndefault_floating = true\ndefault_floating_size = { width = 0.6, height = 0.6 }\nopacity = 0.9\n"
        );
        // Lo lee cualquier TOML (y `umbriel config validate`, en `umbriel_cfg`).
        let v: ::toml::Table = ::toml::from_str(&t).unwrap();
        assert_eq!(v["window_rule"].as_array().unwrap().len(), 1);
        assert_eq!(toml(&[], &app), "");
        // Sin «Sin animación» en Umbriel; en Hyprland sí.
        let tiene = |f: &[Fila]| format!("{f:?}").contains(tr!("Sin animación"));
        assert!(!tiene(&filas_con(&reglas[..1], &[], false)));
        assert!(tiene(&filas(&reglas[..1], &[])));
    }

    #[test]
    fn el_tamano_solo_flotante() {
        let r = ReglaApp {
            app: "mpv.desktop".into(),
            tamano: 60,
            ..Default::default()
        };
        assert!(!lua(std::slice::from_ref(&r), &app).contains("size"));
        let f = filas(&[r], &[("mpv.desktop".into(), "mpv".into())]);
        let Fila::Ajuste {
            control: Control::Lista(o),
            ..
        } = &f[0]
        else {
            panic!()
        };
        assert!(!o.iter().any(|o| o.titulo.starts_with("Tamaño")));
    }

    #[test]
    fn nueva_regla_sin_las_que_ya_tienen() {
        let apps = vec![
            ("mpv.desktop".to_owned(), "mpv".to_owned()),
            ("kitty.desktop".to_owned(), "kitty".to_owned()),
        ];
        let r = ReglaApp {
            app: "mpv.desktop".into(),
            ..Default::default()
        };
        let f = filas(&[r], &apps);
        assert_eq!(f.len(), 2);
        let Fila::Ajuste {
            control: Control::Lista(o),
            ..
        } = &f[1]
        else {
            panic!()
        };
        let n: Vec<_> = o.iter().map(|o| o.titulo.as_str()).collect();
        assert_eq!(n, ["kitty"]);
    }
}
