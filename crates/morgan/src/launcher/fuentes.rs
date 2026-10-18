
use serde::Deserialize;

/// Un emoji de `simbolos.tsv` (Unicode y CLDR, ver `dev/simbolos.py`), con el nombre y las
/// palabras clave en el idioma de la sesión; el nombre en el otro también se busca.
#[derive(Debug, Clone, PartialEq)]
pub struct Simbolo {
    pub caracter: String,
    pub nombre: String,
    pub claves: String,
    pub otro: String,
}

pub const SIMBOLOS: &str = include_str!("simbolos.tsv");

/// `emoji \t nombre es \t claves es \t nombre en \t claves en`; `#` es comentario.
pub fn simbolos(texto: &str, ingles: bool) -> Vec<Simbolo> {
    texto
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            let [caracter, nombre_es, claves_es, nombre_en, claves_en] = c.as_slice() else {
                return None;
            };
            let (nombre, claves, otro) = if ingles {
                (nombre_en, claves_en, nombre_es)
            } else {
                (nombre_es, claves_es, nombre_en)
            };
            (!caracter.is_empty()).then(|| Simbolo {
                caracter: (*caracter).into(),
                nombre: (*nombre).into(),
                claves: (*claves).into(),
                otro: (*otro).into(),
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
    fn lee_la_tabla_de_simbolos() {
        let t = "# comentario\n😀\tcara sonriendo\tcara, feliz\tgrinning face\tface, grin\nroto\n";
        let es = simbolos(t, false);
        assert_eq!(
            es,
            [Simbolo {
                caracter: "😀".into(),
                nombre: "cara sonriendo".into(),
                claves: "cara, feliz".into(),
                otro: "grinning face".into(),
            }]
        );
        assert_eq!(simbolos(t, true)[0].nombre, "grinning face");
        assert_eq!(simbolos(t, true)[0].otro, "cara sonriendo");
    }

    #[test]
    fn la_tabla_embebida_esta_entera() {
        let s = simbolos(SIMBOLOS, false);
        assert!(s.len() > 1800, "{}", s.len());
        assert!(s.iter().all(|x| !x.nombre.is_empty()));
        assert!(s.iter().any(|x| x.caracter == "👍" && x.nombre == "pulgar hacia arriba"));
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
