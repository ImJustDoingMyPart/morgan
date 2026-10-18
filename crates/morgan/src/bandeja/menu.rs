//! El árbol se lee entero con `GetLayout(0, -1, [])` cada vez que se abre el menú: lo
//! marcado (casillas, radios) sale de lo que la app responde, nunca de un click anterior.
//! Elegir una fila manda `Event(id, "clicked")`.

use std::collections::HashMap;

use zbus::zvariant::{OwnedValue, Structure, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marca {
    Casilla(bool),
    Radio(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nodo {
    pub id: i32,
    pub etiqueta: String,
    pub separador: bool,
    pub habilitado: bool,
    pub marca: Option<Marca>,
    pub hijos: Vec<Nodo>,
}

pub fn nodo(v: &Value<'_>) -> Option<Nodo> {
    let v = desenvolver(v);
    let s = Structure::try_from(v.try_clone().ok()?).ok()?;
    let [id, props, hijos] = s.fields() else {
        return None;
    };
    let id = i32::try_from(desenvolver(id)).ok()?;
    let props: HashMap<String, OwnedValue> = HashMap::try_from(props.try_clone().ok()?).ok()?;
    let hijos: Vec<Value<'_>> = match hijos {
        Value::Array(a) => a.iter().filter_map(|h| h.try_clone().ok()).collect(),
        _ => Vec::new(),
    };
    armar_nodo(id, &props, &hijos)
}

pub fn armar_nodo(
    id: i32,
    props: &HashMap<String, OwnedValue>,
    hijos: &[Value<'_>],
) -> Option<Nodo> {
    let prop = |k: &str| props.get(k).and_then(|v| desenvolver(v).try_clone().ok());
    let texto = |k: &str| prop(k).and_then(|v| String::try_from(v).ok());
    let bandera = |k: &str, def: bool| prop(k).and_then(|v| bool::try_from(v).ok()).unwrap_or(def);
    let puesta = prop("toggle-state").and_then(|v| i32::try_from(v).ok()) == Some(1);
    let marca = match texto("toggle-type").as_deref() {
        Some("checkmark") => Some(Marca::Casilla(puesta)),
        Some("radio") => Some(Marca::Radio(puesta)),
        _ => None,
    };
    let hijos = hijos.iter().filter_map(nodo).collect();
    let visible = bandera("visible", true);
    visible.then(|| Nodo {
        id,
        etiqueta: texto("label").unwrap_or_default(),
        separador: texto("type").as_deref() == Some("separator"),
        habilitado: bandera("enabled", true),
        marca,
        hijos,
    })
}

/// A veces viene envuelto dos veces.
fn desenvolver<'a>(v: &'a Value<'a>) -> &'a Value<'a> {
    match v {
        Value::Value(inner) => desenvolver(inner),
        v => v,
    }
}

/// Las secciones vacías (dos separadores seguidos, uno al principio o al final) no cuentan.
pub fn secciones(hijos: &[Nodo]) -> Vec<Vec<&Nodo>> {
    let mut v: Vec<Vec<&Nodo>> = vec![Vec::new()];
    for h in hijos {
        if h.separador {
            v.push(Vec::new());
        } else {
            v.last_mut().expect("al menos una").push(h);
        }
    }
    v.retain(|s| !s.is_empty());
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{Array, StructureBuilder};

    fn fila(
        id: i32,
        props: &[(&str, Value<'static>)],
        hijos: Vec<Value<'static>>,
    ) -> Value<'static> {
        let mut d: HashMap<String, Value<'static>> = HashMap::new();
        for (k, v) in props {
            d.insert((*k).into(), Value::Value(Box::new(v.try_clone().unwrap())));
        }
        let mut arr = Array::new(&zbus::zvariant::Signature::Variant);
        for h in hijos {
            arr.append(Value::Value(Box::new(h))).unwrap();
        }
        Value::Structure(
            StructureBuilder::new()
                .add_field(id)
                .add_field(d)
                .append_field(Value::Array(arr))
                .build()
                .unwrap(),
        )
    }

    #[test]
    fn arbol_de_cachy_update() {
        let raiz = fila(
            0,
            &[("children-display", Value::from("submenu"))],
            vec![
                fila(
                    1,
                    &[
                        ("enabled", Value::from(false)),
                        ("label", Value::from("El sistema está actualizado")),
                    ],
                    vec![],
                ),
                fila(4, &[("type", Value::from("separator"))], vec![]),
                fila(5, &[("label", Value::from("_Ejecutar"))], vec![]),
                fila(
                    6,
                    &[
                        ("label", Value::from("Oculta")),
                        ("visible", Value::from(false)),
                    ],
                    vec![],
                ),
                fila(
                    7,
                    &[
                        ("label", Value::from("Arrancar solo")),
                        ("toggle-type", Value::from("checkmark")),
                        ("toggle-state", Value::from(1i32)),
                    ],
                    vec![],
                ),
            ],
        );
        let n = nodo(&raiz).unwrap();
        assert_eq!(n.id, 0);
        let ids: Vec<i32> = n.hijos.iter().map(|h| h.id).collect();
        assert_eq!(ids, [1, 4, 5, 7]);
        assert!(!n.hijos[0].habilitado);
        assert!(n.hijos[1].separador);
        assert_eq!(n.hijos[2].etiqueta, "_Ejecutar");
        assert_eq!(n.hijos[2].marca, None);
        assert_eq!(n.hijos[3].marca, Some(Marca::Casilla(true)));
        let s = secciones(&n.hijos);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].len(), 1);
        assert_eq!(s[1].len(), 2);
    }

    #[test]
    fn submenu_anidado() {
        let raiz = fila(
            0,
            &[],
            vec![fila(
                12,
                &[
                    ("label", Value::from("Profiles")),
                    ("children-display", Value::from("submenu")),
                ],
                vec![fila(14, &[("label", Value::from("synthwave"))], vec![])],
            )],
        );
        let n = nodo(&raiz).unwrap();
        assert_eq!(n.hijos[0].hijos[0].etiqueta, "synthwave");
        assert_eq!(n.hijos[0].hijos[0].id, 14);
    }

    #[test]
    fn separadores_de_mas() {
        let sep = |id| Nodo {
            id,
            etiqueta: String::new(),
            separador: true,
            habilitado: true,
            marca: None,
            hijos: vec![],
        };
        let item = |id| Nodo {
            separador: false,
            ..sep(id)
        };
        assert_eq!(
            secciones(&[sep(1), item(2), sep(3), sep(4), item(5), sep(6)]).len(),
            2
        );
        assert!(secciones(&[]).is_empty());
    }
}
