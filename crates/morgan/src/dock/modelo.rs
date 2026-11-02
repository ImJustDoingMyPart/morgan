//! Lo que el dock muestra, sin Wayland ni GTK: qué apps van, en qué orden, sus marcas y dónde
//! cae cada una. Una app es una pieza: primero las más usadas, en un orden que no cambia
//! mientras morgan corre, y después las otras abiertas, en el orden en que se abrieron.

use std::collections::{BTreeSet, HashSet};

use morgan_motor::vello::kurbo::Rect;
use serde::Deserialize;

use crate::barra::modelo::etiqueta;

pub const CELDA_ANCHO: f64 = 60.0;
pub const CELDA_ALTO: f64 = 56.0;
pub const ICONO: f64 = 44.0;
const ENTRE_CELDAS: f64 = 6.0;
/// Antes y después de la raya que separa las usadas de las otras abiertas.
const ENTRE_RAYA: f64 = 6.0;
const RELLENO_X: f64 = 10.0;
const RELLENO_ARRIBA: f64 = 8.0;
const RELLENO_ABAJO: f64 = 6.0;
const ENTRE_CELDA_Y_MARCAS: f64 = 4.0;
pub const ALTO_MARCAS: f64 = 12.0;
/// El lugar del brillo del anillo; también lo que queda libre hasta el borde de la pantalla.
const MARGEN: f64 = 10.0;

const ALTO_ISLA: f64 =
    RELLENO_ARRIBA + CELDA_ALTO + ENTRE_CELDA_Y_MARCAS + ALTO_MARCAS + RELLENO_ABAJO;
/// El alto de la capa, con el brillo arriba y abajo.
pub const ALTO_CAPA: u32 = (ALTO_ISLA + 2.0 * MARGEN) as u32;
/// Lo que se les quita a las ventanas: hasta el borde de arriba de la isla.
pub const ZONA: u32 = (ALTO_ISLA + MARGEN) as u32;

/// Con más ventanas no suman más puntos: ya no se cuentan de un vistazo.
const TOPE_MARCAS: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct Abierta {
    pub direccion: String,
    pub clase: String,
    pub titulo: String,
    pub ws: i64,
    /// `focusHistoryID`: 0 es la última que tuvo el foco.
    pub historia: i32,
    pub juego: bool,
}

/// Las ventanas de `j/clients` que existen de verdad.
pub fn leer(clientes: &str) -> Result<Vec<Abierta>, serde_json::Error> {
    #[derive(Deserialize)]
    struct C {
        address: String,
        mapped: bool,
        hidden: bool,
        workspace: W,
        class: String,
        title: String,
        #[serde(rename = "focusHistoryID")]
        historia: i32,
        #[serde(default, rename = "contentType")]
        contenido: String,
    }
    #[derive(Deserialize)]
    struct W {
        id: i64,
    }
    let v: Vec<C> = serde_json::from_str(clientes)?;
    Ok(v.into_iter()
        .filter(|c| c.mapped && !c.hidden && c.workspace.id != -1 && !c.class.is_empty())
        .map(|c| Abierta {
            juego: c.contenido == "game"
                || c.class == "gamescope"
                || c.class.starts_with("steam_app_"),
            direccion: c.address,
            clase: c.class,
            titulo: c.title,
            ws: c.workspace.id,
            historia: c.historia,
        })
        .collect())
}

/// Una app: `clave` es su `.desktop` o, si no tiene, la clase de sus ventanas.
#[derive(Debug, Clone, PartialEq)]
pub struct Conocida {
    pub clave: String,
    pub nombre: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marca {
    /// La ventana con el foco: una raya.
    Foco,
    /// En un workspace que se está viendo: un punto claro.
    Aca,
    /// En otro: un punto apagado.
    Otro,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fila {
    pub direccion: String,
    pub espacio: String,
    pub titulo: String,
    pub foco: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pieza {
    pub clave: String,
    pub nombre: String,
    pub filas: Vec<Fila>,
    pub marcas: Vec<Marca>,
    pub foco: bool,
    /// Es una de las más usadas: las otras van después de la raya.
    pub usada: bool,
}

fn etiqueta_de(ws: i64) -> String {
    // Los workspaces especiales tienen id negativo y ningún número que mostrar.
    if ws > 0 { etiqueta(ws) } else { "·".to_owned() }
}

pub fn armar(
    frecuentes: &[Conocida],
    abiertas: &[(Conocida, Abierta)],
    activos: &BTreeSet<i64>,
) -> Vec<Pieza> {
    let pieza = |app: &Conocida, usada: bool| {
        let suyas: Vec<&Abierta> = abiertas
            .iter()
            .filter(|(c, _)| c.clave == app.clave)
            .map(|(_, a)| a)
            .collect();
        let es_foco = |a: &Abierta| a.historia == 0 && activos.contains(&a.ws);
        let marca = |a: &&Abierta| {
            if es_foco(a) {
                Marca::Foco
            } else if activos.contains(&a.ws) {
                Marca::Aca
            } else {
                Marca::Otro
            }
        };
        // Si son más que el tope, la raya de la enfocada no se puede perder.
        let mut marcas: Vec<Marca> = suyas.iter().map(marca).collect();
        marcas.sort_by_key(|m| *m != Marca::Foco);
        marcas.truncate(TOPE_MARCAS);
        Pieza {
            clave: app.clave.clone(),
            nombre: app.nombre.clone(),
            filas: suyas
                .iter()
                .map(|a| Fila {
                    direccion: a.direccion.clone(),
                    espacio: etiqueta_de(a.ws),
                    titulo: a.titulo.clone(),
                    foco: es_foco(a),
                })
                .collect(),
            marcas,
            foco: suyas.iter().any(|a| es_foco(a)),
            usada,
        }
    };
    let mut v: Vec<Pieza> = frecuentes.iter().map(|a| pieza(a, true)).collect();
    let mut vistas: HashSet<&str> = frecuentes.iter().map(|a| a.clave.as_str()).collect();
    for (c, _) in abiertas {
        if vistas.insert(&c.clave) {
            v.push(pieza(c, false));
        }
    }
    v
}

/// Cuántas celdas entran en una salida de `ancho`.
pub fn tope(ancho: f64) -> usize {
    let libre = ancho - 2.0 * (MARGEN + RELLENO_X) - (2.0 * ENTRE_RAYA + 1.0);
    (((libre + ENTRE_CELDAS) / (CELDA_ANCHO + ENTRE_CELDAS)).floor().max(0.0)) as usize
}

/// Las que se muestran si no entran todas (índices de `piezas`): primero se van las usadas que
/// no están abiertas, las últimas primero, y recién después las abiertas.
pub fn entran(piezas: &[Pieza], tope: usize) -> Vec<usize> {
    let mut v: Vec<usize> = (0..piezas.len()).collect();
    while v.len() > tope {
        let i = v
            .iter()
            .rposition(|&i| piezas[i].filas.is_empty())
            .unwrap_or(v.len() - 1);
        v.remove(i);
    }
    v
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Disposicion {
    /// La caja del fondo; vacía si no hay nada que mostrar.
    pub isla: Option<Rect>,
    pub celdas: Vec<Rect>,
    /// La x de la raya.
    pub raya: Option<f64>,
}

/// La isla centrada abajo en una capa de `ancho`, con una celda por cada `usadas`.
pub fn disponer(ancho: f64, usadas: &[bool]) -> Disposicion {
    if usadas.is_empty() {
        return Disposicion::default();
    }
    let con_raya = usadas.iter().any(|u| *u) && usadas.iter().any(|u| !*u);
    let n = usadas.len() as f64;
    let w = 2.0 * RELLENO_X
        + n * CELDA_ANCHO
        + (n - 1.0) * ENTRE_CELDAS
        + if con_raya { 2.0 * ENTRE_RAYA + 1.0 - ENTRE_CELDAS } else { 0.0 };
    let x0 = ((ancho - w) / 2.0).floor();
    let y0 = MARGEN;
    let isla = Rect::new(x0, y0, x0 + w, y0 + ALTO_ISLA);
    let mut raya = None;
    let mut x = x0 + RELLENO_X;
    let mut celdas = Vec::with_capacity(usadas.len());
    for (i, u) in usadas.iter().enumerate() {
        if con_raya && i > 0 && usadas[i - 1] && !*u {
            let r = x - ENTRE_CELDAS + ENTRE_RAYA;
            raya = Some(r);
            x = r + 1.0 + ENTRE_RAYA;
        }
        let y = y0 + RELLENO_ARRIBA;
        celdas.push(Rect::new(x, y, x + CELDA_ANCHO, y + CELDA_ALTO));
        x += CELDA_ANCHO + ENTRE_CELDAS;
    }
    Disposicion {
        isla: Some(isla),
        celdas,
        raya,
    }
}

impl Disposicion {
    /// La celda bajo el puntero: sin huecos entre celdas y hasta el borde de la pantalla, para
    /// acertarle al ícono con solo tirar el mouse para abajo.
    pub fn celda_en(&self, x: f64, y: f64) -> Option<usize> {
        let isla = self.isla?;
        if y < isla.y0 {
            return None;
        }
        self.celdas
            .iter()
            .position(|c| x >= c.x0 - ENTRE_CELDAS / 2.0 && x < c.x1 + ENTRE_CELDAS / 2.0)
    }

    /// Lo que recibe el puntero. Oculta, una tira en el borde de abajo.
    pub fn region(&self, alto: f64, visible: bool) -> Option<Rect> {
        let isla = self.isla?;
        let y0 = if visible { isla.y0 } else { alto - 2.0 };
        Some(Rect::new(isla.x0, y0, isla.x1, alto))
    }
}

/// La marca sobre la que dibujar, centrada en `cx`: (x, ancho) de cada una.
pub fn marcas_en(cx: f64, marcas: &[Marca]) -> Vec<(f64, f64)> {
    const PUNTO: f64 = 5.0;
    const RAYA: f64 = 16.0;
    const ENTRE: f64 = 4.0;
    let ancho = |m: &Marca| if *m == Marca::Foco { RAYA } else { PUNTO };
    let total: f64 = marcas.iter().map(ancho).sum::<f64>()
        + ENTRE * marcas.len().saturating_sub(1) as f64;
    let mut x = cx - total / 2.0;
    marcas
        .iter()
        .map(|m| {
            let r = (x, ancho(m));
            x += r.1 + ENTRE;
            r
        })
        .collect()
}

/// A qué ventana va un click en la app: la siguiente a la enfocada, y si ninguna de las suyas
/// tiene el foco, la primera.
pub fn siguiente(filas: &[Fila]) -> Option<&str> {
    let i = filas.iter().position(|f| f.foco).map_or(0, |i| i + 1);
    filas.get(i % filas.len().max(1)).map(|f| f.direccion.as_str())
}

/// Las ventanas que aparecieron desde la última lectura, para contar el uso de cada app sin
/// importar desde dónde se abrió. Las que ya estaban en la primera lectura no cuentan: morgan
/// pudo reiniciarse, y no son ventanas nuevas.
#[derive(Debug, Default)]
pub struct Contador {
    vistas: Option<HashSet<String>>,
}

impl Contador {
    pub fn nuevas<'a>(&mut self, ventanas: &'a [Abierta]) -> Vec<&'a Abierta> {
        let actuales: HashSet<String> = ventanas.iter().map(|v| v.direccion.clone()).collect();
        let nuevas = match &self.vistas {
            Some(vistas) => ventanas
                .iter()
                .filter(|v| !vistas.contains(&v.direccion) && !v.juego)
                .collect(),
            None => Vec::new(),
        };
        self.vistas = Some(actuales);
        nuevas
    }
}

/// Un booleano de `j/getoption`. Si no se entiende, apagado: el valor de fábrica.
pub fn opcion_activa(json: &str) -> bool {
    #[derive(Deserialize)]
    struct O {
        #[serde(default, alias = "int")]
        bool: Option<serde_json::Value>,
    }
    serde_json::from_str::<O>(json)
        .ok()
        .and_then(|o| o.bool)
        .is_some_and(|v| v == serde_json::json!(true) || v == serde_json::json!(1))
}

/// Las `n` más usadas que siguen instaladas. `app` dice el nombre de una, o que ya no está. A
/// igual uso, por id: el orden no depende de nada más.
pub fn mas_usadas(
    usos: &[(&str, u32)],
    app: impl Fn(&str) -> Option<String>,
    n: usize,
) -> Vec<Conocida> {
    let mut v = usos.to_vec();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v.into_iter()
        .filter_map(|(id, _)| {
            Some(Conocida {
                clave: id.to_owned(),
                nombre: app(id)?,
            })
        })
        .take(n)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abierta(dir: &str, clase: &str, ws: i64, historia: i32) -> Abierta {
        Abierta {
            direccion: dir.into(),
            clase: clase.into(),
            titulo: format!("título de {dir}"),
            ws,
            historia,
            juego: false,
        }
    }

    fn app(clave: &str) -> Conocida {
        Conocida {
            clave: clave.into(),
            nombre: clave.to_uppercase(),
        }
    }

    fn par(clave: &str, a: Abierta) -> (Conocida, Abierta) {
        (app(clave), a)
    }

    fn activos(v: &[i64]) -> BTreeSet<i64> {
        v.iter().copied().collect()
    }

    #[test]
    fn lee_solo_las_ventanas_de_verdad() {
        let json = r#"[
          {"address":"0x1","mapped":true,"hidden":false,"workspace":{"id":2},"class":"kitty","title":"a","focusHistoryID":0,"contentType":"none"},
          {"address":"0x2","mapped":false,"hidden":false,"workspace":{"id":2},"class":"kitty","title":"b","focusHistoryID":1},
          {"address":"0x3","mapped":true,"hidden":true,"workspace":{"id":2},"class":"kitty","title":"c","focusHistoryID":2},
          {"address":"0x4","mapped":true,"hidden":false,"workspace":{"id":-1},"class":"kitty","title":"d","focusHistoryID":3},
          {"address":"0x5","mapped":true,"hidden":false,"workspace":{"id":3},"class":"","title":"e","focusHistoryID":4},
          {"address":"0x6","mapped":true,"hidden":false,"workspace":{"id":4},"class":"steam_app_1","title":"f","focusHistoryID":5}
        ]"#;
        let v = leer(json).unwrap();
        assert_eq!(v.iter().map(|a| a.direccion.as_str()).collect::<Vec<_>>(), ["0x1", "0x6"]);
        assert!(!v[0].juego);
        assert!(v[1].juego);
    }

    #[test]
    fn un_punto_por_ventana_y_una_raya_para_la_enfocada() {
        let abiertas = [
            par("kitty", abierta("0x1", "kitty", 1, 1)),
            par("kitty", abierta("0x2", "kitty", 1, 0)),
            par("kitty", abierta("0x3", "kitty", 3, 2)),
        ];
        let v = armar(&[], &abiertas, &activos(&[1]));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].marcas, [Marca::Foco, Marca::Aca, Marca::Otro]);
        assert!(v[0].foco);
        assert_eq!(v[0].filas[2].espacio, "III");
    }

    #[test]
    fn sin_foco_en_lo_que_se_ve_no_hay_raya() {
        // La última enfocada quedó en otro workspace: la de este, vacío, no tiene foco.
        let abiertas = [par("kitty", abierta("0x1", "kitty", 2, 0))];
        let v = armar(&[], &abiertas, &activos(&[1]));
        assert_eq!(v[0].marcas, [Marca::Otro]);
        assert!(!v[0].foco);
    }

    #[test]
    fn las_marcas_tienen_tope_y_la_raya_no_se_pierde() {
        let abiertas: Vec<_> = (0..6)
            .map(|i| par("k", abierta(&format!("0x{i}"), "k", 2, 5 - i)))
            .collect();
        let v = armar(&[], &abiertas, &activos(&[2]));
        assert_eq!(v[0].marcas.len(), TOPE_MARCAS);
        assert_eq!(v[0].marcas[0], Marca::Foco);
        assert_eq!(v[0].filas.len(), 6);
    }

    #[test]
    fn primero_las_usadas_en_su_orden_y_despues_las_otras_abiertas() {
        let frecuentes = [app("vesktop"), app("obsidian")];
        let abiertas = [
            par("kitty", abierta("0x1", "kitty", 1, 0)),
            par("obsidian", abierta("0x2", "obsidian", 1, 1)),
            par("brave", abierta("0x3", "brave", 1, 2)),
        ];
        let v = armar(&frecuentes, &abiertas, &activos(&[1]));
        let claves: Vec<_> = v.iter().map(|p| p.clave.as_str()).collect();
        assert_eq!(claves, ["vesktop", "obsidian", "kitty", "brave"]);
        assert!(v[0].marcas.is_empty(), "una usada cerrada no lleva marcas");
        assert_eq!(v[1].marcas.len(), 1, "una usada abierta no se repite");
        assert_eq!(
            v.iter().map(|p| p.usada).collect::<Vec<_>>(),
            [true, true, false, false]
        );
    }

    #[test]
    fn si_no_entran_se_van_primero_las_cerradas() {
        let frecuentes = [app("a"), app("b"), app("c")];
        let abiertas = [
            par("b", abierta("0x1", "b", 1, 0)),
            par("d", abierta("0x2", "d", 1, 1)),
        ];
        let v = armar(&frecuentes, &abiertas, &activos(&[1]));
        assert_eq!(entran(&v, 4), [0, 1, 2, 3]);
        assert_eq!(entran(&v, 3), [0, 1, 3], "se va c, la última cerrada");
        assert_eq!(entran(&v, 2), [1, 3], "después a");
        assert_eq!(entran(&v, 1), [1], "con todas abiertas, la última");
    }

    #[test]
    fn la_isla_queda_centrada_y_las_celdas_adentro() {
        let d = disponer(1000.0, &[true, true, false]);
        let isla = d.isla.unwrap();
        assert!((isla.x0 - (1000.0 - isla.x1)).abs() <= 1.0);
        assert_eq!(isla.height(), ALTO_ISLA);
        for c in &d.celdas {
            assert!(c.x0 >= isla.x0 && c.x1 <= isla.x1);
            assert_eq!(c.width(), CELDA_ANCHO);
        }
        let raya = d.raya.unwrap();
        assert!(raya > d.celdas[1].x1 && raya < d.celdas[2].x0);
    }

    #[test]
    fn con_de_un_solo_tipo_no_hay_raya() {
        assert_eq!(disponer(1000.0, &[true, true]).raya, None);
        assert_eq!(disponer(1000.0, &[false, false]).raya, None);
        assert_eq!(disponer(1000.0, &[]), Disposicion::default());
    }

    #[test]
    fn lo_que_entra_cabe_en_la_salida() {
        for ancho in [400.0, 1280.0, 2560.0] {
            let n = tope(ancho);
            let isla = disponer(ancho, &vec![true; n]).isla.unwrap();
            assert!(isla.x0 >= MARGEN, "{ancho}");
            let mitad = n / 2;
            let con_raya: Vec<bool> = (0..n).map(|i| i < mitad).collect();
            let isla = disponer(ancho, &con_raya).isla.unwrap();
            assert!(isla.x0 >= MARGEN, "{ancho} con raya");
        }
    }

    #[test]
    fn el_puntero_acierta_entre_celdas_y_contra_el_borde_de_abajo() {
        let d = disponer(1000.0, &[true, true]);
        let (a, b) = (d.celdas[0], d.celdas[1]);
        let abajo = ALTO_CAPA as f64 - 1.0;
        assert_eq!(d.celda_en(a.center().x, abajo), Some(0));
        assert_eq!(d.celda_en(a.x1 + 1.0, a.y0), Some(0));
        assert_eq!(d.celda_en(b.x0 - 1.0, a.y0), Some(1));
        assert_eq!(d.celda_en(a.center().x, 0.0), None, "arriba de la isla");
        assert_eq!(d.celda_en(0.0, abajo), None, "al costado");
    }

    #[test]
    fn oculta_recibe_el_puntero_solo_en_el_borde() {
        let d = disponer(1000.0, &[true]);
        let alto = f64::from(ALTO_CAPA);
        let isla = d.isla.unwrap();
        assert_eq!(d.region(alto, true).unwrap().y0, isla.y0);
        assert_eq!(d.region(alto, false).unwrap().y0, alto - 2.0);
        assert_eq!(d.region(alto, false).unwrap().y1, alto);
    }

    #[test]
    fn las_marcas_quedan_centradas() {
        let m = marcas_en(100.0, &[Marca::Foco, Marca::Aca]);
        let total = m[1].0 + m[1].1 - m[0].0;
        assert_eq!(m[0].0 - (100.0 - total / 2.0), 0.0);
        assert_eq!(m[0].1, 16.0);
        assert_eq!(m[1].1, 5.0);
        assert!(marcas_en(100.0, &[]).is_empty());
    }

    #[test]
    fn un_click_pasa_a_la_siguiente_y_da_la_vuelta() {
        let fila = |d: &str, foco| Fila {
            direccion: d.into(),
            espacio: "I".into(),
            titulo: String::new(),
            foco,
        };
        assert_eq!(siguiente(&[fila("a", false), fila("b", false)]), Some("a"));
        assert_eq!(siguiente(&[fila("a", true), fila("b", false)]), Some("b"));
        assert_eq!(siguiente(&[fila("a", false), fila("b", true)]), Some("a"));
        assert_eq!(siguiente(&[fila("a", true)]), Some("a"));
        assert_eq!(siguiente(&[]), None);
    }

    #[test]
    fn cuenta_solo_las_ventanas_nuevas_y_no_los_juegos() {
        let mut c = Contador::default();
        let mut v = vec![abierta("0x1", "kitty", 1, 0)];
        assert!(c.nuevas(&v).is_empty(), "las de la primera lectura ya estaban");
        assert!(c.nuevas(&v).is_empty());
        v.push(abierta("0x2", "brave", 1, 1));
        assert_eq!(c.nuevas(&v).len(), 1);
        assert!(c.nuevas(&v).is_empty(), "no cuenta dos veces");
        let mut juego = abierta("0x3", "steam_app_1", 1, 2);
        juego.juego = true;
        v.push(juego);
        assert!(c.nuevas(&v).is_empty());
        v.remove(0);
        assert!(c.nuevas(&v).is_empty(), "cerrar no suma");
    }

    #[test]
    fn lee_el_booleano_de_una_opcion() {
        assert!(opcion_activa(r#"{"option":"x","bool":true,"set":true}"#));
        assert!(!opcion_activa(r#"{"option":"x","bool":false,"set":false}"#));
        assert!(!opcion_activa("no es json"));
    }

    #[test]
    fn las_mas_usadas_saltean_lo_desinstalado() {
        let usos = [("a.desktop", 1), ("b.desktop", 9), ("c.desktop", 5), ("d.desktop", 5)];
        let v = mas_usadas(
            &usos,
            |id| (id != "b.desktop").then(|| id.to_uppercase()),
            2,
        );
        assert_eq!(
            v.iter().map(|c| c.clave.as_str()).collect::<Vec<_>>(),
            ["c.desktop", "d.desktop"]
        );
    }
}
