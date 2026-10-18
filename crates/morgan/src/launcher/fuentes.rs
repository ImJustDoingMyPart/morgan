
use serde::Deserialize;

/// Un símbolo de los CSV de rofimoji: `😀 grinning face <small>(face, grin)</small>`.
#[derive(Debug, Clone, PartialEq)]
pub struct Simbolo {
    pub caracter: String,
    pub nombre: String,
    pub claves: String,
}

pub fn simbolos(texto: &str) -> Vec<Simbolo> {
    texto
        .lines()
        .filter_map(|l| {
            let (caracter, resto) = l.split_once(' ')?;
            let (nombre, claves) = match resto.split_once(" <small>(") {
                Some((n, c)) => (n, c.trim_end_matches(")</small>")),
                None => (resto, ""),
            };
            (!caracter.is_empty()).then(|| Simbolo {
                caracter: caracter.into(),
                nombre: nombre.trim().into(),
                claves: claves.into(),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Recorte {
    pub linea: String,
    pub vista: String,
}

pub fn recortes(texto: &str) -> Vec<Recorte> {
    texto
        .lines()
        .filter_map(|l| {
            let (_, vista) = l.split_once('\t')?;
            Some(Recorte {
                linea: l.into(),
                vista: vista.into(),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Atajo {
    pub descripcion: String,
    pub teclas: String,
    /// Todo bind escrito con `hl.bind` la tiene. Vale hasta que Hyprland relee la config.
    pub referencia: Option<u32>,
}

#[derive(Deserialize)]
struct Bind {
    modmask: u32,
    key: String,
    description: String,
    #[serde(default)]
    dispatcher: String,
    #[serde(default)]
    arg: String,
    #[serde(default)]
    submap: String,
}

pub fn atajos(json: &str) -> Result<Vec<Atajo>, String> {
    let mut v = binds(json)?;
    v.sort_by(|a, b| a.descripcion.cmp(&b.descripcion));
    Ok(v)
}

pub fn binds(json: &str) -> Result<Vec<Atajo>, String> {
    let binds: Vec<Bind> = serde_json::from_str(json).map_err(|e| format!("binds: {e}"))?;
    Ok(binds
        .into_iter()
        .filter(|b| !b.description.is_empty())
        .map(|b| {
            let mut teclas = modificadores(b.modmask);
            teclas.push(b.key);
            Atajo {
                descripcion: b.description,
                teclas: teclas.join(" + "),
                referencia: (b.dispatcher == "__lua").then(|| b.arg.parse().ok()).flatten(),
            }
        })
        .collect())
}

pub fn ocupadas(json: &str) -> Result<Vec<Atajo>, String> {
    let binds: Vec<Bind> = serde_json::from_str(json).map_err(|e| format!("binds: {e}"))?;
    Ok(binds
        .into_iter()
        .filter(|b| b.submap.is_empty() && !b.key.is_empty())
        .map(|b| {
            let mut teclas = modificadores(b.modmask);
            teclas.push(b.key);
            Atajo {
                descripcion: b.description,
                teclas: teclas.join(" + "),
                referencia: None,
            }
        })
        .collect())
}

fn modificadores(mascara: u32) -> Vec<String> {
    [(1, "SHIFT"), (4, "CTRL"), (8, "ALT"), (64, "SUPER")]
        .iter()
        .filter(|(bit, _)| mascara & bit != 0)
        .map(|(_, n)| (*n).to_owned())
        .collect()
}

/// `hyprctl binds` no da la acción de un bind de Lua, solo su referencia en el registro, y el
/// dispatcher `__lua` no se puede pedir por `hyprctl dispatch`: se la busca en el registro y
/// se la despacha, como hace Hyprland al apretar la tecla (`LuaBindingsRegistration.cpp`,
/// 0.56.2). Lo que no es una función ahí (una referencia vieja) no se toca.
pub fn ejecutar_bind(referencia: u32) -> String {
    format!("local f = debug.getregistry()[{referencia}]; if type(f) == 'function' then hl.dispatch(f) end")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_los_csv_de_rofimoji() {
        let s = simbolos(
            "😀 grinning face <small>(face, grin)</small>\n👩‍💻 woman technologist\n\n→ rightwards arrow <small>(arrow)</small>\n",
        );
        assert_eq!(
            s,
            [
                Simbolo {
                    caracter: "😀".into(),
                    nombre: "grinning face".into(),
                    claves: "face, grin".into()
                },
                Simbolo {
                    caracter: "👩‍💻".into(),
                    nombre: "woman technologist".into(),
                    claves: "".into()
                },
                Simbolo {
                    caracter: "→".into(),
                    nombre: "rightwards arrow".into(),
                    claves: "arrow".into()
                },
            ]
        );
    }

    #[test]
    fn lee_cliphist() {
        let r = recortes("12\thola mundo\n3\t[[ binary data 2 KiB png 10x10 ]]\nbasura\n");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].linea, "12\thola mundo");
        assert_eq!(r[0].vista, "hola mundo");
        assert_eq!(r[1].vista, "[[ binary data 2 KiB png 10x10 ]]");
    }

    #[test]
    fn lee_los_binds() {
        let json = r#"[
            {"modmask": 64, "key": "space", "description": "Launcher", "dispatcher": "__lua", "arg": "5", "otro": 1},
            {"modmask": 72, "key": "C", "description": "Calculadora", "dispatcher": "exec", "arg": "qalc"},
            {"modmask": 65, "key": "E", "description": ""}
        ]"#;
        assert_eq!(
            atajos(json).unwrap(),
            [
                Atajo {
                    descripcion: "Calculadora".into(),
                    teclas: "ALT + SUPER + C".into(),
                    referencia: None,
                },
                Atajo {
                    descripcion: "Launcher".into(),
                    teclas: "SUPER + space".into(),
                    referencia: Some(5),
                },
            ]
        );
        assert!(atajos("no es json").is_err());
    }

    #[test]
    fn ejecuta_por_la_referencia() {
        assert_eq!(
            ejecutar_bind(11),
            "local f = debug.getregistry()[11]; if type(f) == 'function' then hl.dispatch(f) end"
        );
    }
}
