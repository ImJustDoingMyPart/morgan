//! Lo que el dock muestra, sin Wayland ni GTK: qué apps van, en qué orden, sus marcas y dónde
//! cae cada una. Una app es una pieza: primero las más usadas, en un orden que no cambia
//! mientras morgan corre; después las otras abiertas, en el orden en que se abrieron; y al
//! final, después de la raya, la sección de la bandeja: lo que está en el dock solo por tener
//! ícono de bandeja. El ícono de bandeja de una app que ya tiene pieza se le junta a ella.

use std::collections::{BTreeSet, HashSet};

use morgan_motor::vello::kurbo::Rect;

use crate::barra::modelo::etiqueta;
use crate::compositor::{CambioVentanas, Cliente};

pub const CELDA_ANCHO: f64 = 60.0;
pub const CELDA_ALTO: f64 = 56.0;
pub const ICONO: f64 = 44.0;
const ENTRE_CELDAS: f64 = 6.0;
/// Antes y después de la raya que aparta la sección de la bandeja.
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
    pub pid: Option<u32>,
}

/// Las que el dock puede mostrar: sin clase no hay app con la que juntarlas.
pub fn de_clientes(clientes: &[Cliente]) -> Vec<Abierta> {
    clientes
        .iter()
        .filter(|c| !c.clase.is_empty())
        .map(|c| Abierta {
            juego: c.juego,
            direccion: c.id.clone(),
            clase: c.clase.clone(),
            titulo: c.titulo.clone(),
            ws: c.ws,
            historia: c.historia,
            pid: c.pid,
        })
        .collect()
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
    /// Corre de fondo, sin ventanas (tiene ícono de bandeja): un punto hueco.
    Fondo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seccion {
    /// Una de las más usadas.
    Usada,
    /// Abierta y no entre las más usadas.
    Abierta,
    /// Está en el dock por su ícono de bandeja.
    Bandeja,
}

/// Un ícono de la bandeja y la app a la que se le junta.
#[derive(Debug, Clone, PartialEq)]
pub struct DeBandeja {
    /// La clave del ítem (bus y ruta).
    pub item: String,
    pub app: Conocida,
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
    pub seccion: Seccion,
    /// El ítem de la bandeja que se le juntó.
    pub bandeja: Option<String>,
}

fn etiqueta_de(ws: i64) -> String {
    // Los workspaces especiales tienen id negativo y ningún número que mostrar.
    if ws > 0 {
        etiqueta(ws)
    } else {
        "·".to_owned()
    }
}

pub fn armar(
    frecuentes: &[Conocida],
    abiertas: &[(Conocida, Abierta)],
    activos: &BTreeSet<i64>,
    bandeja: &[DeBandeja],
) -> Vec<Pieza> {
    let item_de = |clave: &str| {
        bandeja
            .iter()
            .find(|b| b.app.clave == clave)
            .map(|b| b.item.clone())
    };
    let pieza = |app: &Conocida, seccion: Seccion| {
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
        let item = item_de(&app.clave);
        if marcas.is_empty() && item.is_some() {
            marcas.push(Marca::Fondo);
        }
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
            seccion,
            bandeja: item,
        }
    };
    let mut v: Vec<Pieza> = frecuentes
        .iter()
        .map(|a| pieza(a, Seccion::Usada))
        .collect();
    let mut vistas: HashSet<&str> = frecuentes.iter().map(|a| a.clave.as_str()).collect();
    // Lo que tiene ícono de bandeja va a su sección aunque abra ventanas: no se muda.
    let de_bandeja: HashSet<&str> = bandeja.iter().map(|b| b.app.clave.as_str()).collect();
    for (c, _) in abiertas {
        if !de_bandeja.contains(c.clave.as_str()) && vistas.insert(&c.clave) {
            v.push(pieza(c, Seccion::Abierta));
        }
    }
    for b in bandeja {
        if vistas.insert(&b.app.clave) {
            v.push(pieza(&b.app, Seccion::Bandeja));
        }
    }
    v
}

/// Cuántas celdas entran en una salida de `ancho`.
pub fn tope(ancho: f64) -> usize {
    let libre = ancho - 2.0 * (MARGEN + RELLENO_X) - (2.0 * ENTRE_RAYA + 1.0);
    (((libre + ENTRE_CELDAS) / (CELDA_ANCHO + ENTRE_CELDAS))
        .floor()
        .max(0.0)) as usize
}

/// Las que se muestran si no entran todas (índices de `piezas`): primero se van las usadas que
/// no están abiertas ni de fondo, las últimas primero, y recién después las otras.
pub fn entran(piezas: &[Pieza], tope: usize) -> Vec<usize> {
    let mut v: Vec<usize> = (0..piezas.len()).collect();
    while v.len() > tope {
        let i = v
            .iter()
            .rposition(|&i| piezas[i].filas.is_empty() && piezas[i].bandeja.is_none())
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

/// La isla centrada abajo en una capa de `ancho`, con una celda por cada una de `bandeja`
/// (si es de la sección de la bandeja, que va al final).
pub fn disponer(ancho: f64, bandeja: &[bool]) -> Disposicion {
    if bandeja.is_empty() {
        return Disposicion::default();
    }
    let con_raya = bandeja.iter().any(|b| !*b) && bandeja.iter().any(|b| *b);
    let n = bandeja.len() as f64;
    let w = 2.0 * RELLENO_X
        + n * CELDA_ANCHO
        + (n - 1.0) * ENTRE_CELDAS
        + if con_raya {
            2.0 * ENTRE_RAYA + 1.0 - ENTRE_CELDAS
        } else {
            0.0
        };
    let x0 = ((ancho - w) / 2.0).floor();
    let y0 = MARGEN;
    let isla = Rect::new(x0, y0, x0 + w, y0 + ALTO_ISLA);
    let mut raya = None;
    let mut x = x0 + RELLENO_X;
    let mut celdas = Vec::with_capacity(bandeja.len());
    for (i, b) in bandeja.iter().enumerate() {
        if con_raya && i > 0 && !bandeja[i - 1] && *b {
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
    let total: f64 =
        marcas.iter().map(ancho).sum::<f64>() + ENTRE * marcas.len().saturating_sub(1) as f64;
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
    filas
        .get(i % filas.len().max(1))
        .map(|f| f.direccion.as_str())
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

/// Lo que obliga a releer las ventanas. Un título que cambia importa solo con el globo
/// abierto.
#[derive(Debug, Default)]
pub struct Filtro;

impl Filtro {
    pub fn importa(&mut self, cambio: &CambioVentanas, globo_abierto: bool) -> bool {
        match cambio {
            CambioVentanas::Lista | CambioVentanas::Foco(_) => true,
            CambioVentanas::Titulo => globo_abierto,
        }
    }
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

/// La unidad de systemd de un proceso (la última línea de `/proc/PID/cgroup`) dice con qué app
/// se lanzó: `app-steam@autostart.service`, `app-vesktop-104722.scope` (el número es del que lo
/// lanzó), `app-gnome-foo-12.scope` (con el lanzador adelante). Devuelve los `.desktop` posibles
/// y si es un scope: lo que corre en un scope puede ser algo que se abrió desde otra app (una
/// terminal), así que hay que confirmarlo.
pub fn apps_de_unidad(cgroup: &str) -> Option<(Vec<String>, bool)> {
    let unidad = cgroup.lines().last()?.rsplit('/').next()?;
    let unidad = unidad.replace("\\x2d", "-");
    let resto = unidad.strip_prefix("app-")?;
    let (nombre, scope) = if let Some(r) = resto.strip_suffix(".scope") {
        // El número de proceso del final.
        let r = r
            .rsplit_once('-')
            .filter(|(_, n)| n.chars().all(|c| c.is_ascii_digit()))
            .map_or(r, |(a, _)| a);
        (r, true)
    } else {
        let r = resto.strip_suffix(".service")?;
        (r.split_once('@').map_or(r, |(a, _)| a), false)
    };
    if nombre.is_empty() {
        return None;
    }
    let mut ids = vec![format!("{nombre}.desktop")];
    if let Some((_, sin_lanzador)) = nombre.split_once('-') {
        ids.push(format!("{sin_lanzador}.desktop"));
    }
    Some((ids, scope))
}

/// `pid` es `ancestro` o un proceso suyo. `padre` lee el padre de un proceso.
pub fn desciende(pid: u32, ancestro: u32, padre: impl Fn(u32) -> Option<u32>) -> bool {
    let mut actual = pid;
    // Tope por si `/proc` cambia a mitad de camino y arma un ciclo.
    for _ in 0..64 {
        if actual == ancestro {
            return true;
        }
        match padre(actual) {
            Some(p) if p > 1 => actual = p,
            _ => return false,
        }
    }
    false
}

/// La app de un ícono de bandeja por sus ventanas: la que es del mismo proceso, o de uno que
/// lanzó (Steam y su `steamwebhelper`). No al revés: lo que se abrió desde una terminal no es
/// de la terminal. Los juegos no cuentan, aunque Steam los lance.
pub fn de_ventanas(
    pid: u32,
    abiertas: &[(Conocida, Abierta)],
    padre: impl Fn(u32) -> Option<u32>,
) -> Option<Conocida> {
    let sin_juegos = || abiertas.iter().filter(|(_, a)| !a.juego);
    sin_juegos()
        .find(|(_, a)| a.pid == Some(pid))
        .or_else(|| sin_juegos().find(|(_, a)| a.pid.is_some_and(|p| desciende(p, pid, &padre))))
        .map(|(c, _)| c.clone())
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
            pid: None,
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
    fn sin_clase_no_entra() {
        let c = |id: &str, clase: &str| Cliente {
            id: id.into(),
            clase: clase.into(),
            titulo: String::new(),
            ws: 1,
            rect: Default::default(),
            salida: String::new(),
            historia: 0,
            juego: clase.starts_with("steam_app_"),
            pid: None,
        };
        let v = de_clientes(&[c("0x1", "kitty"), c("0x5", ""), c("0x6", "steam_app_1")]);
        assert_eq!(
            v.iter().map(|a| a.direccion.as_str()).collect::<Vec<_>>(),
            ["0x1", "0x6"]
        );
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
        let v = armar(&[], &abiertas, &activos(&[1]), &[]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].marcas, [Marca::Foco, Marca::Aca, Marca::Otro]);
        assert!(v[0].foco);
        assert_eq!(v[0].filas[2].espacio, "III");
    }

    #[test]
    fn sin_foco_en_lo_que_se_ve_no_hay_raya() {
        // La última enfocada quedó en otro workspace: la de este, vacío, no tiene foco.
        let abiertas = [par("kitty", abierta("0x1", "kitty", 2, 0))];
        let v = armar(&[], &abiertas, &activos(&[1]), &[]);
        assert_eq!(v[0].marcas, [Marca::Otro]);
        assert!(!v[0].foco);
    }

    #[test]
    fn las_marcas_tienen_tope_y_la_raya_no_se_pierde() {
        let abiertas: Vec<_> = (0..6)
            .map(|i| par("k", abierta(&format!("0x{i}"), "k", 2, 5 - i)))
            .collect();
        let v = armar(&[], &abiertas, &activos(&[2]), &[]);
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
        let v = armar(&frecuentes, &abiertas, &activos(&[1]), &[]);
        let claves: Vec<_> = v.iter().map(|p| p.clave.as_str()).collect();
        assert_eq!(claves, ["vesktop", "obsidian", "kitty", "brave"]);
        assert!(v[0].marcas.is_empty(), "una usada cerrada no lleva marcas");
        assert_eq!(v[1].marcas.len(), 1, "una usada abierta no se repite");
        assert_eq!(
            v.iter().map(|p| p.seccion).collect::<Vec<_>>(),
            [
                Seccion::Usada,
                Seccion::Usada,
                Seccion::Abierta,
                Seccion::Abierta
            ]
        );
    }

    #[test]
    fn si_no_entran_se_van_primero_las_cerradas() {
        let frecuentes = [app("a"), app("b"), app("c")];
        let abiertas = [
            par("b", abierta("0x1", "b", 1, 0)),
            par("d", abierta("0x2", "d", 1, 1)),
        ];
        let v = armar(&frecuentes, &abiertas, &activos(&[1]), &[]);
        assert_eq!(entran(&v, 4), [0, 1, 2, 3]);
        assert_eq!(entran(&v, 3), [0, 1, 3], "se va c, la última cerrada");
        assert_eq!(entran(&v, 2), [1, 3], "después a");
        assert_eq!(entran(&v, 1), [1], "con todas abiertas, la última");
    }

    #[test]
    fn la_isla_queda_centrada_y_las_celdas_adentro() {
        let d = disponer(1000.0, &[false, false, true]);
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
            let con_raya: Vec<bool> = (0..n).map(|i| i >= mitad).collect();
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
        assert!(
            c.nuevas(&v).is_empty(),
            "las de la primera lectura ya estaban"
        );
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
    fn un_titulo_que_cambia_no_despierta_con_el_globo_cerrado() {
        let mut f = Filtro;
        assert!(f.importa(&CambioVentanas::Foco("abc".into()), false));
        assert!(!f.importa(&CambioVentanas::Titulo, false));
        assert!(
            f.importa(&CambioVentanas::Titulo, true),
            "con el globo se ve"
        );
        assert!(f.importa(&CambioVentanas::Lista, false));
    }

    fn de_bandeja(item: &str, clave: &str) -> DeBandeja {
        DeBandeja {
            item: item.into(),
            app: app(clave),
        }
    }

    #[test]
    fn la_bandeja_se_junta_a_su_app_o_va_al_final() {
        let frecuentes = [app("steam"), app("kitty")];
        let abiertas = [
            par("kitty", abierta("0x1", "kitty", 1, 0)),
            par("brave", abierta("0x2", "brave", 1, 1)),
            par("openrgb", abierta("0x3", "openrgb", 2, 2)),
        ];
        let bandeja = [
            de_bandeja(":1.5/steam", "steam"),
            de_bandeja(":1.6/rgb", "openrgb"),
            de_bandeja(":1.7/cachy", "bandeja:Cachy-Update"),
        ];
        let v = armar(&frecuentes, &abiertas, &activos(&[1]), &bandeja);
        let claves: Vec<_> = v.iter().map(|p| p.clave.as_str()).collect();
        assert_eq!(
            claves,
            ["steam", "kitty", "brave", "openrgb", "bandeja:Cachy-Update"]
        );
        assert_eq!(
            v[0].bandeja.as_deref(),
            Some(":1.5/steam"),
            "fundida en su pieza"
        );
        assert_eq!(v[0].marcas, [Marca::Fondo], "de fondo, sin ventanas");
        assert_eq!(v[1].marcas, [Marca::Foco]);
        assert_eq!(v[2].seccion, Seccion::Abierta);
        assert_eq!(v[3].seccion, Seccion::Bandeja, "con ventana no se muda");
        assert_eq!(
            v[3].marcas,
            [Marca::Otro],
            "con ventana, su punto de siempre"
        );
        assert_eq!(v[4].seccion, Seccion::Bandeja);
        assert_eq!(v[4].marcas, [Marca::Fondo]);
    }

    #[test]
    fn si_no_entran_lo_de_fondo_se_queda() {
        let frecuentes = [app("a"), app("b")];
        let bandeja = [de_bandeja(":1.5/a", "a")];
        let v = armar(&frecuentes, &[], &activos(&[1]), &bandeja);
        assert_eq!(entran(&v, 1), [0], "se va b, cerrada; a corre de fondo");
    }

    #[test]
    fn la_raya_aparta_solo_la_bandeja() {
        assert_eq!(disponer(1000.0, &[false, false]).raya, None);
        assert_eq!(disponer(1000.0, &[true, true]).raya, None);
        let d = disponer(1000.0, &[false, false, true, true]);
        let raya = d.raya.unwrap();
        assert!(raya > d.celdas[1].x1 && raya < d.celdas[2].x0);
    }

    #[test]
    fn la_unidad_de_systemd_dice_la_app() {
        let c =
            |u: &str| format!("0::/user.slice/user-1000.slice/user@1000.service/app.slice/{u}\n");
        assert_eq!(
            apps_de_unidad(&c("app-steam@autostart.service")),
            Some((vec!["steam.desktop".into()], false))
        );
        assert_eq!(
            apps_de_unidad(&c("app-vesktop-104722.scope")),
            Some((vec!["vesktop.desktop".into()], true))
        );
        assert_eq!(
            apps_de_unidad(&c("app-arch\\x2dupdate\\x2dtray@autostart.service")),
            Some((
                vec![
                    "arch-update-tray.desktop".into(),
                    "update-tray.desktop".into()
                ],
                false
            ))
        );
        assert_eq!(
            apps_de_unidad(&c("app-gnome-org.gnome.Nautilus-12.scope")).map(|(v, _)| v),
            Some(vec![
                "gnome-org.gnome.Nautilus.desktop".into(),
                "org.gnome.Nautilus.desktop".into()
            ])
        );
        assert_eq!(apps_de_unidad(&c("openrgb.service")), None);
    }

    #[test]
    fn la_bandeja_se_reconoce_por_el_proceso_de_sus_ventanas() {
        // 10 lanza a 11 (Steam y su steamwebhelper) y a 12 (un juego); 20 es una terminal que
        // lanzó a 21.
        let padre = |p: u32| match p {
            11 | 12 => Some(10),
            21 => Some(20),
            _ => Some(1),
        };
        let con_pid = |dir: &str, clase: &str, pid: u32, juego: bool| {
            let mut a = abierta(dir, clase, 1, 0);
            a.pid = Some(pid);
            a.juego = juego;
            par(clase, a)
        };
        let abiertas = [
            con_pid("0x1", "steam_app_1", 12, true),
            con_pid("0x2", "steam", 11, false),
            con_pid("0x3", "kitty", 20, false),
        ];
        assert_eq!(de_ventanas(10, &abiertas, padre).unwrap().clave, "steam");
        assert_eq!(de_ventanas(20, &abiertas, padre).unwrap().clave, "kitty");
        assert_eq!(
            de_ventanas(21, &abiertas, padre),
            None,
            "no es de la terminal"
        );
        assert_eq!(
            de_ventanas(12, &abiertas[..1], padre),
            None,
            "los juegos no cuentan"
        );
    }

    #[test]
    fn las_mas_usadas_saltean_lo_desinstalado() {
        let usos = [
            ("a.desktop", 1),
            ("b.desktop", 9),
            ("c.desktop", 5),
            ("d.desktop", 5),
        ];
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
