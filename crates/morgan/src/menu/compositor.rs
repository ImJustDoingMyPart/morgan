//! morgan no toca los archivos de Hyprland escritos a mano: lo cambiado desde el menú se
//! guarda en `hyprland.toml` y de ahí se genera entero el bloque de `morgan.lua`, que
//! `hyprland.lua` carga al final y pisa los valores de los otros archivos. Restablecer es
//! borrar la clave. Lo que se muestra como puesto sale de `hyprctl getoption`, nunca de lo
//! guardado.

use std::collections::BTreeMap;
use std::path::Path;

use super::arbol::{Control, Fila, Opcion, Que, Seccion, ajuste, opcion};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Valor {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl Valor {
    /// Las separaciones vienen como `"css": "8 8 8 8"`: vale la primera.
    fn de_json(v: &serde_json::Value) -> Option<Valor> {
        if let Some(b) = v.get("bool").and_then(|x| x.as_bool()) {
            return Some(Valor::Bool(b));
        }
        if let Some(i) = v.get("int").and_then(|x| x.as_i64()) {
            return Some(Valor::Int(i));
        }
        if let Some(f) = v.get("float").and_then(|x| x.as_f64()) {
            return Some(Valor::Float(f));
        }
        if let Some(c) = v.get("css").and_then(|x| x.as_str()) {
            return c.split_whitespace().next()?.parse().ok().map(Valor::Int);
        }
        // Un texto vacío viene como `[[EMPTY]]`.
        v.get("str")
            .and_then(|x| x.as_str())
            .map(|s| Valor::Str(if s == "[[EMPTY]]" { "" } else { s }.to_owned()))
    }

    fn lua(&self) -> String {
        match self {
            Valor::Bool(b) => b.to_string(),
            Valor::Int(i) => i.to_string(),
            Valor::Float(f) => format!("{f:?}"),
            Valor::Str(s) => format!("{s:?}"),
        }
    }

    /// Dos valores son el mismo aunque uno sea entero y el otro no (`1` y `1.0`).
    fn igual(&self, otro: &Valor) -> bool {
        match (self.numero(), otro.numero()) {
            (Some(a), Some(b)) => (a - b).abs() < 1e-6,
            _ => self == otro,
        }
    }

    fn numero(&self) -> Option<f64> {
        match self {
            Valor::Int(i) => Some(*i as f64),
            Valor::Float(f) => Some(*f),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Cambios(pub BTreeMap<String, Valor>);

impl Cambios {
    pub fn leer(ruta: &Path) -> Result<Cambios, String> {
        match std::fs::read_to_string(ruta) {
            Ok(t) => toml::from_str(&t).map_err(|e| format!("{}: {e}", ruta.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Cambios::default()),
            Err(e) => Err(format!("{}: {e}", ruta.display())),
        }
    }

    pub fn guardar(&self, ruta: &Path) -> Result<(), String> {
        let texto = toml::to_string(self).map_err(|e| e.to_string())?;
        escribir(ruta, &texto)
    }

    pub fn poner(&mut self, clave: &str, valor: Option<Valor>) {
        match valor {
            Some(v) => self.0.insert(clave.to_owned(), v),
            None => self.0.remove(clave),
        };
    }

    /// La clave `input:touchpad:tap-to-click` es `input.touchpad.tap_to_click` en Lua.
    pub fn lua(&self) -> String {
        #[derive(Default)]
        struct Nodo {
            hojas: BTreeMap<String, String>,
            hijos: BTreeMap<String, Nodo>,
        }
        fn escribir(n: &Nodo, sangria: usize, out: &mut String) {
            let s = "    ".repeat(sangria);
            for (k, v) in &n.hojas {
                out.push_str(&format!("{s}{k} = {v},\n"));
            }
            for (k, h) in &n.hijos {
                out.push_str(&format!("{s}{k} = {{\n"));
                escribir(h, sangria + 1, out);
                out.push_str(&format!("{s}}},\n"));
            }
        }
        let mut raiz = Nodo::default();
        for (clave, valor) in &self.0 {
            let partes: Vec<String> = clave.split(':').map(|p| p.replace('-', "_")).collect();
            let Some((hoja, camino)) = partes.split_last() else {
                continue;
            };
            let mut n = &mut raiz;
            for p in camino {
                n = n.hijos.entry(p.clone()).or_default();
            }
            n.hojas.insert(hoja.clone(), valor.lua());
        }
        let mut out = String::new();
        if !self.0.is_empty() {
            out.push_str("\n-- Compositor (menú › Escritorio; lo guardado está en morgan/hyprland.toml).\n");
            out.push_str("hl.config({\n");
            escribir(&raiz, 1, &mut out);
            out.push_str("})\n");
        }
        out
    }
}

pub(super) fn escribir(ruta: &Path, texto: &str) -> Result<(), String> {
    if let Some(dir) = ruta.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // Primero a un temporal y después el cambio de nombre: Hyprland nunca lee uno a medias.
    let tmp = ruta.with_extension("tmp");
    std::fs::write(&tmp, texto).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, ruta).map_err(|e| format!("{}: {e}", ruta.display()))
}

pub enum Tipo {
    Casilla,
    Valores(&'static [(Valor, &'static str)]),
    /// Un `Valor::Str` no se puede armar en una constante.
    Textos(&'static [(&'static str, &'static str)]),
    Leidos,
}

pub struct Opt {
    pub clave: &'static str,
    pub titulo: &'static str,
    pub sub: &'static str,
    pub tipo: Tipo,
}

const fn opt(clave: &'static str, titulo: &'static str, sub: &'static str, tipo: Tipo) -> Opt {
    Opt {
        clave,
        titulo,
        sub,
        tipo,
    }
}

macro_rules! enteros {
    ($($v:literal => $t:literal),* $(,)?) => { &[$((Valor::Int($v), $t)),*] };
}
macro_rules! reales {
    ($($v:literal => $t:literal),* $(,)?) => { &[$((Valor::Float($v), $t)),*] };
}

/// Cada clave existe en la 0.56.2 (`hyprctl descriptions`) y los valores están dentro de su
/// rango.
pub const SECCIONES: &[(&str, &[Opt])] = &[
    (
        "Ventanas",
        &[
            opt(
                "general:gaps_in",
                "Separación entre ventanas",
                "",
                Tipo::Valores(
                    enteros![0 => "0 px", 2 => "2 px", 4 => "4 px", 6 => "6 px", 8 => "8 px", 10 => "10 px", 12 => "12 px", 16 => "16 px", 20 => "20 px"],
                ),
            ),
            opt(
                "general:gaps_out",
                "Separación con el borde de la pantalla",
                "",
                Tipo::Valores(
                    enteros![0 => "0 px", 4 => "4 px", 8 => "8 px", 12 => "12 px", 16 => "16 px", 20 => "20 px", 24 => "24 px", 32 => "32 px"],
                ),
            ),
            opt(
                "general:border_size",
                "Grosor del borde",
                "",
                Tipo::Valores(
                    enteros![0 => "sin borde", 1 => "1 px", 2 => "2 px", 3 => "3 px", 4 => "4 px"],
                ),
            ),
            opt(
                "decoration:rounding",
                "Redondeo de las esquinas",
                "",
                Tipo::Valores(
                    enteros![0 => "rectas", 2 => "2 px", 4 => "4 px", 6 => "6 px", 8 => "8 px", 10 => "10 px", 12 => "12 px", 16 => "16 px", 20 => "20 px"],
                ),
            ),
            opt(
                "decoration:active_opacity",
                "Opacidad de la ventana activa",
                "",
                Tipo::Valores(
                    reales![0.8 => "80 %", 0.85 => "85 %", 0.9 => "90 %", 0.95 => "95 %", 1.0 => "opaca"],
                ),
            ),
            opt(
                "decoration:inactive_opacity",
                "Opacidad de las inactivas",
                "",
                Tipo::Valores(
                    reales![0.7 => "70 %", 0.75 => "75 %", 0.8 => "80 %", 0.85 => "85 %", 0.9 => "90 %", 0.95 => "95 %", 1.0 => "opaca"],
                ),
            ),
            opt(
                "decoration:dim_inactive",
                "Atenuar las inactivas",
                "oscurece las ventanas sin foco",
                Tipo::Casilla,
            ),
            opt(
                "decoration:dim_strength",
                "Cuánto se atenúan",
                "",
                Tipo::Valores(
                    reales![0.1 => "10 %", 0.2 => "20 %", 0.3 => "30 %", 0.4 => "40 %", 0.5 => "50 %"],
                ),
            ),
        ],
    ),
    (
        "Desenfoque",
        &[
            opt(
                "decoration:blur:enabled",
                "Desenfoque detrás de las ventanas",
                "lo que se ve a través de una ventana transparente",
                Tipo::Casilla,
            ),
            opt(
                "decoration:blur:size",
                "Tamaño",
                "",
                Tipo::Valores(
                    enteros![2 => "2", 4 => "4", 6 => "6", 8 => "8", 10 => "10", 12 => "12"],
                ),
            ),
            opt(
                "decoration:blur:passes",
                "Pasadas",
                "más pasadas, más suave y más costoso",
                Tipo::Valores(enteros![1 => "1", 2 => "2", 3 => "3", 4 => "4"]),
            ),
            opt(
                "decoration:blur:popups",
                "También en los menús de las apps",
                "menús contextuales y tooltips",
                Tipo::Casilla,
            ),
        ],
    ),
    (
        "Distribución",
        &[
            opt(
                "scrolling:focus_fit_method",
                "Al enfocar una columna",
                "",
                Tipo::Valores(enteros![1 => "ajustarla a la vista", 0 => "centrarla"]),
            ),
            opt(
                "scrolling:fullscreen_on_one_column",
                "Una sola columna ocupa toda la pantalla",
                "",
                Tipo::Casilla,
            ),
        ],
    ),
    (
        "Grupos",
        &[
            opt(
                "group:auto_group",
                "Las ventanas nuevas entran al grupo enfocado",
                "",
                Tipo::Casilla,
            ),
            opt(
                "group:insert_after_current",
                "Una pestaña nueva va al lado de la actual",
                "si no, al final",
                Tipo::Casilla,
            ),
            opt(
                "group:groupbar:enabled",
                "Barra de pestañas",
                "",
                Tipo::Casilla,
            ),
            opt(
                "group:groupbar:height",
                "Alto de la barra",
                "",
                Tipo::Valores(
                    enteros![14 => "14 px", 16 => "16 px", 18 => "18 px", 20 => "20 px", 22 => "22 px", 24 => "24 px"],
                ),
            ),
            opt(
                "group:groupbar:font_size",
                "Letra de las pestañas",
                "",
                Tipo::Valores(enteros![8 => "8", 9 => "9", 10 => "10", 11 => "11", 12 => "12"]),
            ),
            opt(
                "group:groupbar:gradients",
                "Pestañas con fondo",
                "si no, solo la rayita de la activa",
                Tipo::Casilla,
            ),
            opt(
                "group:groupbar:middle_click_close",
                "Click del medio cierra la pestaña",
                "",
                Tipo::Casilla,
            ),
            opt(
                "group:groupbar:scrolling",
                "La rueda cambia de pestaña",
                "",
                Tipo::Casilla,
            ),
        ],
    ),
    (
        "Foco y cursor",
        &[
            opt(
                "binds:workspace_back_and_forth",
                "Volver al workspace anterior",
                "ir al workspace en que ya se está vuelve al de antes",
                Tipo::Casilla,
            ),
            opt(
                "misc:focus_on_activate",
                "Enfocar la app que lo pide",
                "",
                Tipo::Casilla,
            ),
            opt(
                "input:follow_mouse",
                "El foco sigue al mouse",
                "con el layout scrolling, el foco salta solo (ver input.lua)",
                Tipo::Valores(
                    enteros![0 => "no", 1 => "sí", 2 => "separado del teclado", 3 => "separado del todo"],
                ),
            ),
            opt(
                "cursor:inactive_timeout",
                "Esconder el cursor quieto",
                "",
                Tipo::Valores(
                    reales![0.0 => "nunca", 3.0 => "a los 3 s", 5.0 => "a los 5 s", 10.0 => "a los 10 s", 20.0 => "a los 20 s"],
                ),
            ),
            opt(
                "cursor:hide_on_key_press",
                "Esconder el cursor al tipear",
                "vuelve al mover el mouse",
                Tipo::Casilla,
            ),
        ],
    ),
    (
        "Teclado",
        &[
            opt("input:kb_layout", "Distribución", "", Tipo::Leidos),
            opt(
                "input:repeat_delay",
                "Espera antes de repetir",
                "",
                Tipo::Valores(
                    enteros![200 => "200 ms", 250 => "250 ms", 300 => "300 ms", 400 => "400 ms", 500 => "500 ms", 600 => "600 ms"],
                ),
            ),
            opt(
                "input:repeat_rate",
                "Velocidad de repetición",
                "",
                Tipo::Valores(
                    enteros![15 => "15 por segundo", 20 => "20 por segundo", 25 => "25 por segundo", 30 => "30 por segundo", 40 => "40 por segundo", 50 => "50 por segundo"],
                ),
            ),
            opt(
                "input:numlock_by_default",
                "Bloq Num al arrancar",
                "",
                Tipo::Casilla,
            ),
        ],
    ),
    (
        "Mouse y touchpad",
        &[
            opt(
                "input:sensitivity",
                "Sensibilidad del mouse",
                "",
                Tipo::Valores(
                    reales![-0.5 => "-0,5", -0.25 => "-0,25", 0.0 => "0", 0.25 => "0,25", 0.5 => "0,5"],
                ),
            ),
            opt(
                "input:accel_profile",
                "Aceleración",
                "",
                Tipo::Textos(&[
                    ("", "la del dispositivo"),
                    ("adaptive", "adaptativa"),
                    ("flat", "plana, sin aceleración"),
                ]),
            ),
            opt(
                "input:touchpad:natural_scroll",
                "Scroll natural en el touchpad",
                "",
                Tipo::Casilla,
            ),
            opt(
                "input:touchpad:tap-to-click",
                "Tocar para hacer click",
                "",
                Tipo::Casilla,
            ),
        ],
    ),
];

pub fn claves() -> impl Iterator<Item = &'static str> {
    SECCIONES
        .iter()
        .flat_map(|(_, o)| o.iter().map(|o| o.clave))
}

/// 34 claves de a una tardan 100 ms; juntas, 4.
pub fn pedido_batch() -> String {
    claves()
        .map(|c| format!("j/getoption {c}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// Una clave que Hyprland no conoce responde texto suelto ("no such option"), que se
/// saltea; por eso se asocia por el campo `option` y no por el orden.
pub fn de_batch(salida: &str) -> BTreeMap<String, Valor> {
    salida
        .split("\n\n")
        .filter_map(|b| serde_json::from_str::<serde_json::Value>(b.trim()).ok())
        .filter_map(|v| {
            let clave = v.get("option")?.as_str()?.to_owned();
            Some((clave, Valor::de_json(&v)?))
        })
        .collect()
}

pub fn pagina(
    actuales: &BTreeMap<String, Valor>,
    cambios: &Cambios,
    distribuciones: &[(String, String)],
) -> Vec<Seccion> {
    SECCIONES
        .iter()
        .map(|(titulo, opts)| Seccion {
            titulo,
            filas: opts
                .iter()
                .map(|o| fila(o, actuales.get(o.clave), cambios, distribuciones))
                .collect(),
        })
        .collect()
}

fn poner(clave: &str, v: Valor) -> Que {
    Que::Hypr(clave.to_owned(), Some(v))
}

fn fila(
    o: &Opt,
    actual: Option<&Valor>,
    cambios: &Cambios,
    distribuciones: &[(String, String)],
) -> Fila {
    let cambiada = cambios.0.contains_key(o.clave);
    let marca = |s: String| {
        if cambiada {
            format!("{s} · cambiado desde el menú")
        } else {
            s
        }
    };
    match &o.tipo {
        Tipo::Casilla => {
            let si = matches!(actual, Some(Valor::Bool(true)) | Some(Valor::Int(1)));
            ajuste(
                o.titulo,
                &marca(o.sub.to_owned()),
                Control::Casilla {
                    activa: si,
                    que: poner(o.clave, Valor::Bool(!si)),
                },
            )
        }
        Tipo::Valores(_) | Tipo::Textos(_) => {
            let valores: Vec<(Valor, &str)> = match &o.tipo {
                Tipo::Valores(v) => v.to_vec(),
                Tipo::Textos(v) => v
                    .iter()
                    .map(|(v, t)| (Valor::Str((*v).into()), *t))
                    .collect(),
                _ => Vec::new(),
            };
            let mut opciones: Vec<Opcion> = valores
                .iter()
                .map(|(v, t)| {
                    opcion(t, "", poner(o.clave, v.clone()))
                        .activa(actual.is_some_and(|a| a.igual(v)))
                })
                .collect();
            let puesto = opciones
                .iter()
                .find(|x| x.activa)
                .map(|x| x.titulo.clone())
                .or_else(|| actual.map(texto))
                .unwrap_or_default();
            if cambiada {
                opciones.push(opcion(
                    "Volver a lo de los archivos de Hyprland",
                    "",
                    Que::Hypr(o.clave.to_owned(), None),
                ));
            }
            let sub = if o.sub.is_empty() {
                puesto
            } else {
                format!("{puesto} · {}", o.sub)
            };
            ajuste(o.titulo, &marca(sub), Control::Lista(opciones))
        }
        Tipo::Leidos => {
            let codigo = match actual {
                Some(Valor::Str(s)) => s.as_str(),
                _ => "",
            };
            let mut opciones: Vec<Opcion> = distribuciones
                .iter()
                .map(|(c, n)| {
                    opcion(
                        &format!("{n} ({c})"),
                        "",
                        poner(o.clave, Valor::Str(c.clone())),
                    )
                    .activa(c == codigo)
                })
                .collect();
            let nombre = distribuciones
                .iter()
                .find(|(c, _)| c == codigo)
                .map_or(codigo.to_owned(), |(_, n)| n.clone());
            if cambiada {
                opciones.push(opcion(
                    "Volver a lo de los archivos de Hyprland",
                    "",
                    Que::Hypr(o.clave.to_owned(), None),
                ));
            }
            ajuste(o.titulo, &marca(nombre), Control::Lista(opciones))
        }
    }
}

fn texto(v: &Valor) -> String {
    match v {
        Valor::Bool(b) => if *b { "sí" } else { "no" }.into(),
        Valor::Int(i) => i.to_string(),
        Valor::Float(f) => f.to_string(),
        Valor::Str(s) => s.clone(),
    }
}

pub fn distribuciones(evdev_lst: &str) -> Vec<(String, String)> {
    let mut v = Vec::new();
    let mut en_layout = false;
    for l in evdev_lst.lines() {
        if let Some(s) = l.strip_prefix('!') {
            en_layout = s.trim() == "layout";
            continue;
        }
        if !en_layout {
            continue;
        }
        let l = l.trim();
        if let Some((codigo, nombre)) = l.split_once(char::is_whitespace) {
            v.push((codigo.to_owned(), nombre.trim().to_owned()));
        }
    }
    v.sort_by_key(|a| a.1.to_lowercase());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_getoption() {
        let v = |j| Valor::de_json(&serde_json::from_str(j).unwrap());
        assert_eq!(
            v(r#"{"option": "general:gaps_in", "css": "8 8 8 8", "set": true }"#),
            Some(Valor::Int(8))
        );
        assert_eq!(
            v(r#"{"option": "x", "float": 1.000000, "set": true }"#),
            Some(Valor::Float(1.0))
        );
        assert_eq!(
            v(r#"{"option": "x", "int": 0, "set": true }"#),
            Some(Valor::Int(0))
        );
        assert_eq!(
            v(r#"{"option": "x", "bool": true, "set": true }"#),
            Some(Valor::Bool(true))
        );
        assert_eq!(
            v(r#"{"option": "x", "str": "latam", "set": true }"#),
            Some(Valor::Str("latam".into()))
        );
    }

    #[test]
    fn lee_el_batch_salteando_lo_que_no_existe() {
        // Copiado de `hyprctl --batch` de la 0.56.2.
        let salida = "{\"option\": \"input:touchpad:tap-to-click\", \"bool\": true, \"set\": true }\n\n\nno such option\n\n\n{\"option\": \"cursor:inactive_timeout\", \"float\": 0.000000, \"set\": false }";
        let m = de_batch(salida);
        assert_eq!(m.len(), 2);
        assert_eq!(m["input:touchpad:tap-to-click"], Valor::Bool(true));
        assert_eq!(m["cursor:inactive_timeout"], Valor::Float(0.0));
        assert_eq!(pedido_batch().matches("j/getoption ").count(), claves().count());
    }

    #[test]
    fn genera_el_lua_anidado() {
        let mut c = Cambios::default();
        assert!(!c.lua().contains("hl.config"));
        c.poner("decoration:blur:size", Some(Valor::Int(6)));
        c.poner("decoration:rounding", Some(Valor::Int(8)));
        c.poner("input:touchpad:tap-to-click", Some(Valor::Bool(false)));
        c.poner("input:kb_layout", Some(Valor::Str("us".into())));
        c.poner("decoration:active_opacity", Some(Valor::Float(1.0)));
        let lua = c.lua();
        let cuerpo = &lua[lua.find("hl.config").unwrap()..];
        assert_eq!(
            cuerpo,
            "hl.config({\n    decoration = {\n        active_opacity = 1.0,\n        rounding = 8,\n        blur = {\n            size = 6,\n        },\n    },\n    input = {\n        kb_layout = \"us\",\n        touchpad = {\n            tap_to_click = false,\n        },\n    },\n})\n"
        );
        c.poner("decoration:rounding", None);
        assert!(!c.lua().contains("rounding"));
    }

    #[test]
    fn guarda_y_relee() {
        let dir = std::env::temp_dir().join(format!("morgan-hypr-{}", std::process::id()));
        let ruta = dir.join("hyprland.toml");
        assert_eq!(Cambios::leer(&ruta).unwrap(), Cambios::default());
        let mut c = Cambios::default();
        c.poner("general:gaps_in", Some(Valor::Int(4)));
        c.poner("decoration:dim_strength", Some(Valor::Float(0.3)));
        c.poner("input:numlock_by_default", Some(Valor::Bool(false)));
        c.poner("input:kb_layout", Some(Valor::Str("es".into())));
        c.guardar(&ruta).unwrap();
        assert_eq!(Cambios::leer(&ruta).unwrap(), c);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn marca_lo_que_dice_hyprland() {
        let mut actuales = BTreeMap::new();
        actuales.insert("decoration:rounding".to_owned(), Valor::Int(4));
        actuales.insert("decoration:active_opacity".to_owned(), Valor::Float(1.0));
        actuales.insert("decoration:blur:enabled".to_owned(), Valor::Bool(true));
        let mut cambios = Cambios::default();
        cambios.poner("decoration:rounding", Some(Valor::Int(4)));
        let p = pagina(&actuales, &cambios, &[]);
        let buscar = |clave: &str| {
            let (s, f) = SECCIONES
                .iter()
                .enumerate()
                .find_map(|(s, (_, o))| o.iter().position(|o| o.clave == clave).map(|f| (s, f)))
                .unwrap();
            p[s].filas[f].clone()
        };
        let Fila::Ajuste {
            sub,
            control: Control::Lista(v),
            ..
        } = buscar("decoration:rounding")
        else {
            panic!()
        };
        assert_eq!(v.iter().filter(|o| o.activa).count(), 1);
        assert!(v.iter().find(|o| o.activa).unwrap().titulo == "4 px");
        assert!(sub.contains("cambiado desde el menú"));
        // Lo cambiado se puede restablecer.
        assert_eq!(
            v.last().unwrap().que,
            Que::Hypr("decoration:rounding".into(), None)
        );
        // 1 y 1.0 son el mismo.
        let Fila::Ajuste {
            control: Control::Lista(v),
            ..
        } = buscar("decoration:active_opacity")
        else {
            panic!()
        };
        assert_eq!(v.iter().find(|o| o.activa).unwrap().titulo, "opaca");
        let Fila::Ajuste {
            control: Control::Casilla { activa, que },
            ..
        } = buscar("decoration:blur:enabled")
        else {
            panic!()
        };
        assert!(activa);
        assert_eq!(
            que,
            Que::Hypr("decoration:blur:enabled".into(), Some(Valor::Bool(false)))
        );
    }

    #[test]
    fn lee_las_distribuciones() {
        let lst = "! model\n  pc105  Generic 105-key PC\n\n! layout\n  us              English (US)\n  latam           Spanish (Latin American)\n\n! variant\n  intl  us: English\n";
        assert_eq!(
            distribuciones(lst),
            [
                ("us".to_owned(), "English (US)".to_owned()),
                ("latam".to_owned(), "Spanish (Latin American)".to_owned())
            ]
        );
    }
}
