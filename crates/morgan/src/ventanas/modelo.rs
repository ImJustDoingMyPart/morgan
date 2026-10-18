//! Coordenadas: las de Hyprland, relativas al origen del monitor de cada workspace. Así el
//! viewport de todas las franjas es el mismo rectángulo y queda alineado en columna; una
//! columna de un layout scrolling fuera de pantalla tiene x negativa o mayor que el ancho.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn centro(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    pub fn toca(&self, otro: &Rect) -> bool {
        self.x < otro.x + otro.w
            && otro.x < self.x + self.w
            && self.y < otro.y + otro.h
            && otro.y < self.y + self.h
    }

    fn misma_columna(&self, otro: &Rect) -> bool {
        self.x < otro.x + otro.w && otro.x < self.x + self.w
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ventana {
    pub direccion: String,
    pub clase: String,
    pub titulo: String,
    pub ws: i64,
    pub rect: Rect,
    pub monitor: Rect,
    pub historia: i32,
}

impl Ventana {
    /// Hyprland no entrega cuadros de una ventana que no toca el rectángulo de su monitor, y la
    /// petición queda colgada.
    pub fn capturable(&self) -> bool {
        self.rect.toca(&self.monitor)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Franja {
    pub ws: i64,
    pub ventanas: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Lienzo {
    pub ventanas: Vec<Ventana>,
    pub franjas: Vec<Franja>,
    pub recientes: Vec<usize>,
    pub ws_actual: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Izquierda,
    Derecha,
    Arriba,
    Abajo,
}

pub fn leer(clientes: &str, monitores: &str) -> Result<Lienzo, serde_json::Error> {
    #[derive(Deserialize)]
    struct C {
        address: String,
        mapped: bool,
        hidden: bool,
        at: [i32; 2],
        size: [i32; 2],
        workspace: W,
        monitor: i64,
        class: String,
        title: String,
        #[serde(rename = "focusHistoryID")]
        historia: i32,
    }
    #[derive(Deserialize)]
    struct W {
        id: i64,
    }
    #[derive(Deserialize)]
    struct M {
        id: i64,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        scale: f64,
        transform: i32,
        focused: bool,
        #[serde(rename = "activeWorkspace")]
        activo: W,
    }
    let monitores: Vec<M> = serde_json::from_str(monitores)?;
    let ws_actual = monitores.iter().find(|m| m.focused).map(|m| m.activo.id);
    let mut ventanas = Vec::new();
    for c in serde_json::from_str::<Vec<C>>(clientes)? {
        if !c.mapped || c.hidden || c.workspace.id <= 0 {
            continue;
        }
        let Some(m) = monitores.iter().find(|m| m.id == c.monitor) else {
            continue;
        };
        // El tamaño lógico: los píxeles sobre la escala, girados si el monitor lo está.
        let (mut w, mut h) = (
            (f64::from(m.width) / m.scale).round() as i32,
            (f64::from(m.height) / m.scale).round() as i32,
        );
        if m.transform % 2 == 1 {
            std::mem::swap(&mut w, &mut h);
        }
        ventanas.push(Ventana {
            direccion: c.address,
            clase: c.class,
            titulo: c.title,
            ws: c.workspace.id,
            rect: Rect {
                x: c.at[0] - m.x,
                y: c.at[1] - m.y,
                w: c.size[0],
                h: c.size[1],
            },
            monitor: Rect { x: 0, y: 0, w, h },
            historia: c.historia,
        });
    }
    let mut franjas: Vec<Franja> = Vec::new();
    let mut ids: Vec<i64> = ventanas.iter().map(|v| v.ws).collect();
    ids.sort_unstable();
    ids.dedup();
    for ws in ids {
        let mut indices: Vec<usize> = (0..ventanas.len())
            .filter(|&i| ventanas[i].ws == ws)
            .collect();
        indices.sort_by_key(|&i| (ventanas[i].rect.x, ventanas[i].rect.y));
        franjas.push(Franja {
            ws,
            ventanas: indices,
        });
    }
    let mut recientes: Vec<usize> = (0..ventanas.len()).collect();
    recientes.sort_by_key(|&i| ventanas[i].historia);
    Ok(Lienzo {
        ventanas,
        franjas,
        recientes,
        ws_actual,
    })
}

impl Lienzo {
    pub fn inicial(&self) -> Option<usize> {
        self.recientes.get(1).or(self.recientes.first()).copied()
    }

    pub fn reciente(&self, sel: usize) -> usize {
        let n = self.recientes.len();
        match self.recientes.iter().position(|&r| r == sel) {
            Some(i) => self.recientes[(i + 1) % n],
            None => sel,
        }
    }

    fn franja_de(&self, i: usize) -> Option<usize> {
        self.franjas
            .iter()
            .position(|f| f.ws == self.ventanas[i].ws)
    }

    /// Arriba y abajo, primero dentro de la misma columna; si no hay, la franja de al lado (los
    /// viewports están alineados).
    pub fn vecina(&self, sel: usize, dir: Dir) -> usize {
        let Some(f) = self.franja_de(sel) else {
            return sel;
        };
        let r = self.ventanas[sel].rect;
        let (cx, cy) = r.centro();
        let distancia = |i: usize| {
            let (x, y) = self.ventanas[i].rect.centro();
            ((x - cx).abs(), (y - cy).abs())
        };
        let mejor = |candidatas: Vec<usize>, clave: &dyn Fn(usize) -> (i32, i32)| {
            candidatas.into_iter().min_by_key(|&i| clave(i))
        };
        let propias = &self.franjas[f].ventanas;
        let elegida = match dir {
            Dir::Izquierda | Dir::Derecha => {
                let lado: Vec<usize> = propias
                    .iter()
                    .copied()
                    .filter(|&i| {
                        let x = self.ventanas[i].rect.centro().0;
                        if dir == Dir::Izquierda {
                            x < cx
                        } else {
                            x > cx
                        }
                    })
                    .collect();
                mejor(lado, &distancia)
            }
            Dir::Arriba | Dir::Abajo => {
                let columna: Vec<usize> = propias
                    .iter()
                    .copied()
                    .filter(|&i| {
                        let o = self.ventanas[i].rect;
                        let y = o.centro().1;
                        i != sel
                            && o.misma_columna(&r)
                            && if dir == Dir::Arriba { y < cy } else { y > cy }
                    })
                    .collect();
                let invertida = |i: usize| {
                    let (dx, dy) = distancia(i);
                    (dy, dx)
                };
                mejor(columna, &invertida).or_else(|| {
                    let otra = if dir == Dir::Arriba {
                        f.checked_sub(1)?
                    } else {
                        f + 1
                    };
                    let vecinas = self.franjas.get(otra)?.ventanas.clone();
                    // En la otra franja, la de x más cercana; empates, la más cercana al
                    // borde por el que se entra.
                    let entrada = |i: usize| {
                        let o = self.ventanas[i].rect;
                        let borde = if dir == Dir::Arriba {
                            -(o.y + o.h)
                        } else {
                            o.y
                        };
                        ((o.centro().0 - cx).abs(), borde)
                    };
                    mejor(vecinas, &entrada)
                })
            }
        };
        elegida.unwrap_or(sel)
    }

    pub fn extension(&self) -> (i32, i32) {
        let mut min = 0;
        let mut max = 0;
        for v in &self.ventanas {
            min = min.min(v.rect.x).min(v.monitor.x);
            max = max.max(v.rect.x + v.rect.w).max(v.monitor.x + v.monitor.w);
        }
        (min, max)
    }

    pub fn alto_monitor(&self) -> i32 {
        self.ventanas.iter().map(|v| v.monitor.h).max().unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Medidas {
    pub escala: f64,
    pub alto_franja: i32,
}

/// Con un tope por franja: un solo workspace no ocupa la pantalla entera.
pub fn medidas(
    franjas: usize,
    alto_monitor: i32,
    alto_disponible: i32,
    separacion: i32,
    alto_maximo: i32,
) -> Medidas {
    if franjas == 0 || alto_monitor <= 0 {
        return Medidas {
            escala: 0.0,
            alto_franja: 0,
        };
    }
    let n = franjas as i32;
    let alto = ((alto_disponible - separacion * (n - 1)) / n).clamp(1, alto_maximo.max(1));
    Medidas {
        escala: f64::from(alto) / f64::from(alto_monitor),
        alto_franja: alto,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITORES: &str = r#"[{"id":0,"name":"DP-1","x":0,"y":0,"width":2560,"height":1440,
        "scale":1.0,"transform":0,"focused":true,"activeWorkspace":{"id":3,"name":"3"}}]"#;

    fn cliente(dir: &str, ws: i64, x: i32, y: i32, w: i32, h: i32, historia: i32) -> String {
        format!(
            r#"{{"address":"{dir}","mapped":true,"hidden":false,"at":[{x},{y}],"size":[{w},{h}],
            "workspace":{{"id":{ws},"name":"{ws}"}},"monitor":0,"class":"c{dir}","title":"t",
            "focusHistoryID":{historia}}}"#
        )
    }

    fn lienzo(clientes: &[String]) -> Lienzo {
        leer(&format!("[{}]", clientes.join(",")), MONITORES).unwrap()
    }

    fn dir(l: &Lienzo, i: usize) -> &str {
        &l.ventanas[i].direccion
    }

    fn ejemplo() -> Lienzo {
        lienzo(&[
            cliente("a", 3, -3759, 21, 1253, 1398, 4),
            cliente("b", 3, -2494, 21, 1248, 1398, 3),
            cliente("c", 3, -1234, 21, 1248, 1398, 2),
            cliente("d", 3, 26, 21, 1248, 1398, 1),
            cliente("e", 3, 1286, 21, 1253, 1398, 0),
            cliente("f", 2, 21, 21, 1253, 1398, 5),
            cliente("g", 2, 1286, 21, 1253, 1398, 6),
        ])
    }

    #[test]
    fn franjas_por_workspace_y_por_x() {
        let l = ejemplo();
        assert_eq!(l.franjas.len(), 2);
        assert_eq!(l.franjas[0].ws, 2);
        let orden: Vec<&str> = l.franjas[1].ventanas.iter().map(|&i| dir(&l, i)).collect();
        assert_eq!(orden, ["a", "b", "c", "d", "e"]);
        assert_eq!(l.ws_actual, Some(3));
    }

    #[test]
    fn fuera_de_pantalla_no_se_captura() {
        let l = ejemplo();
        let capturables: Vec<&str> = (0..l.ventanas.len())
            .filter(|&i| l.ventanas[i].capturable())
            .map(|i| dir(&l, i))
            .collect();
        // c asoma 14 px: se captura.
        assert_eq!(capturables, ["c", "d", "e", "f", "g"]);
    }

    #[test]
    fn ocultas_especiales_y_sin_mapear_quedan_afuera() {
        let mut oculta = cliente("x", 1, 0, 0, 10, 10, 7);
        oculta = oculta.replace(r#""hidden":false"#, r#""hidden":true"#);
        let especial = cliente("y", -98, 0, 0, 10, 10, 8);
        let sin_mapear =
            cliente("z", 1, 0, 0, 10, 10, 9).replace(r#""mapped":true"#, r#""mapped":false"#);
        let l = lienzo(&[
            oculta,
            especial,
            sin_mapear,
            cliente("w", 1, 0, 0, 10, 10, 0),
        ]);
        assert_eq!(l.ventanas.len(), 1);
        assert_eq!(dir(&l, 0), "w");
    }

    #[test]
    fn coordenadas_relativas_al_monitor() {
        let monitores = r#"[{"id":0,"x":0,"y":0,"width":1920,"height":1080,"scale":1.0,
            "transform":0,"focused":false,"activeWorkspace":{"id":1}},
            {"id":1,"x":1920,"y":0,"width":3840,"height":2160,"scale":2.0,"transform":0,
            "focused":true,"activeWorkspace":{"id":5}}]"#;
        let c = cliente("t", 5, 1940, 20, 900, 1000, 0).replace(r#""monitor":0"#, r#""monitor":1"#);
        let l = leer(&format!("[{c}]"), monitores).unwrap();
        assert_eq!(l.ventanas[0].rect.x, 20);
        assert_eq!(
            l.ventanas[0].monitor,
            Rect {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080
            }
        );
    }

    #[test]
    fn tab_va_por_uso_reciente_con_vuelta() {
        let l = ejemplo();
        let orden: Vec<&str> = l.recientes.iter().map(|&i| dir(&l, i)).collect();
        assert_eq!(orden, ["e", "d", "c", "b", "a", "f", "g"]);
        let ini = l.inicial().unwrap();
        assert_eq!(dir(&l, ini), "d");
        assert_eq!(dir(&l, l.reciente(ini)), "c");
        let ultima = l.recientes[l.recientes.len() - 1];
        assert_eq!(dir(&l, ultima), "g");
        assert_eq!(dir(&l, l.reciente(ultima)), "e");
    }

    #[test]
    fn una_sola_ventana_o_ninguna() {
        assert_eq!(lienzo(&[]).inicial(), None);
        let l = lienzo(&[cliente("u", 1, 0, 0, 10, 10, 0)]);
        assert_eq!(l.inicial(), Some(0));
        assert_eq!(l.reciente(0), 0);
    }

    #[test]
    fn flechas_en_la_cinta_sin_vuelta() {
        let l = ejemplo();
        let i = |d: &str| l.ventanas.iter().position(|v| v.direccion == d).unwrap();
        assert_eq!(dir(&l, l.vecina(i("c"), Dir::Izquierda)), "b");
        assert_eq!(dir(&l, l.vecina(i("c"), Dir::Derecha)), "d");
        assert_eq!(dir(&l, l.vecina(i("a"), Dir::Izquierda)), "a");
        assert_eq!(dir(&l, l.vecina(i("e"), Dir::Derecha)), "e");
    }

    #[test]
    fn arriba_y_abajo_cruzan_de_franja_por_x() {
        let l = ejemplo();
        let i = |d: &str| l.ventanas.iter().position(|v| v.direccion == d).unwrap();
        // De la franja del 3 a la del 2 (arriba), la más cercana en x.
        assert_eq!(dir(&l, l.vecina(i("d"), Dir::Arriba)), "f");
        assert_eq!(dir(&l, l.vecina(i("e"), Dir::Arriba)), "g");
        assert_eq!(dir(&l, l.vecina(i("a"), Dir::Arriba)), "f");
        assert_eq!(dir(&l, l.vecina(i("g"), Dir::Abajo)), "e");
        // Sin franja más allá, se queda.
        assert_eq!(dir(&l, l.vecina(i("f"), Dir::Arriba)), "f");
        assert_eq!(dir(&l, l.vecina(i("d"), Dir::Abajo)), "d");
    }

    #[test]
    fn arriba_y_abajo_primero_dentro_de_la_columna() {
        // Una columna con dos ventanas apiladas, y otra franja abajo.
        let l = lienzo(&[
            cliente("arriba", 1, 20, 20, 1200, 690, 0),
            cliente("abajo", 1, 20, 730, 1200, 690, 1),
            cliente("lado", 1, 1240, 20, 1200, 1400, 2),
            cliente("otra", 2, 20, 20, 1200, 1400, 3),
        ]);
        let i = |d: &str| l.ventanas.iter().position(|v| v.direccion == d).unwrap();
        assert_eq!(dir(&l, l.vecina(i("arriba"), Dir::Abajo)), "abajo");
        assert_eq!(dir(&l, l.vecina(i("abajo"), Dir::Arriba)), "arriba");
        assert_eq!(dir(&l, l.vecina(i("abajo"), Dir::Abajo)), "otra");
        assert_eq!(dir(&l, l.vecina(i("lado"), Dir::Izquierda)), "arriba");
    }

    #[test]
    fn extension_incluye_el_viewport() {
        let l = ejemplo();
        assert_eq!(l.extension(), (-3759, 2560));
        let l = lienzo(&[cliente("u", 1, 100, 0, 10, 10, 0)]);
        assert_eq!(l.extension(), (0, 2560));
    }

    #[test]
    fn escala_con_tope_por_franja() {
        // Una franja: el tope manda.
        let m = medidas(1, 1440, 1100, 8, 320);
        assert_eq!(m.alto_franja, 320);
        assert!((m.escala - 320.0 / 1440.0).abs() < 1e-9);
        // Cinco: reparten el alto con 8 px entre cada una.
        let m = medidas(5, 1440, 1100, 8, 320);
        assert_eq!(m.alto_franja, (1100 - 32) / 5);
        assert_eq!(medidas(0, 1440, 1100, 8, 320).alto_franja, 0);
    }
}
