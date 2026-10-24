//! Monitores desde el menú: un dibujo a escala para ubicarlos, y por salida su nombre,
//! resolución, frecuencia, escala, rotación, posición y encendida.
//!
//! Lo que se elige se guarda en `monitors.toml` (al lado de `hyprland.toml`) y va a
//! `morgan.lua` como un `hl.monitor` por salida con **solo los campos cambiados**: Hyprland
//! junta las reglas de una misma salida, así que lo que el usuario puso a mano (VRR,
//! profundidad de color, HDR) sigue valiendo. Lo puesto sale de `hyprctl monitors all`.
//!
//! Lo que puede dejar la pantalla negra o ilegible (resolución, frecuencia, escala, rotación)
//! se prueba antes: se aplica en vivo con `hyprctl eval` y, si nadie dice «Mantener», un
//! `reload config-only` vuelve a lo de los archivos.
//!
//! Una salida apagada no dice ni su tamaño ni su lugar (`hyprctl` da 0 × 0 en -1 × -1): se
//! toman de lo elegido en el menú, si no de la última vez que se la vio prendida
//! ([`Vistos`]), y si nunca se la vio, de su modo preferido. En ese último caso, ubicarla
//! guarda también ese modo: lo dibujado es lo que va a ocupar al prenderse.
//!
//! El nombre propio (apodo) es solo de morgan: Hyprland sigue nombrando la salida por su
//! conector.

use std::collections::BTreeMap;
use std::path::Path;

use morgan_idioma::{tr, trf};

use super::arbol::{Control, Fila, Que, ajuste, opcion};

/// Cuánto espera la prueba antes de volver sola.
pub const SEGUNDOS_PRUEBA: u32 = 15;

const ESCALAS: [f64; 8] = [1.0, 1.25, 1.333_333, 1.5, 1.6, 1.75, 2.0, 3.0];

#[derive(Debug, Clone, PartialEq)]
pub struct Salida {
    pub nombre: String,
    pub descripcion: String,
    pub ancho: u32,
    pub alto: u32,
    pub hz: f64,
    pub escala: f64,
    pub x: i32,
    pub y: i32,
    pub rotacion: u8,
    pub apagada: bool,
    /// `(ancho, alto, hz)`, como los da `availableModes`.
    pub modos: Vec<(u32, u32, f64)>,
    /// El nombre que le puso el usuario.
    pub apodo: Option<String>,
    /// Apagada y nunca vista: el tamaño sale de este modo, que va con la posición elegida.
    pub supuesto: Option<(u32, u32, f64)>,
}

impl Salida {
    /// Lo que ocupa en el escritorio: girada de costado, ancho y alto se cruzan.
    fn logico(&self) -> (i32, i32) {
        let (a, b) = if self.rotacion % 2 == 1 {
            (self.alto, self.ancho)
        } else {
            (self.ancho, self.alto)
        };
        (
            (a as f64 / self.escala).round() as i32,
            (b as f64 / self.escala).round() as i32,
        )
    }

    fn titulo(&self) -> String {
        match &self.apodo {
            Some(a) => format!("{a} · {}", self.nombre),
            None if self.descripcion.is_empty() => self.nombre.clone(),
            None => format!("{} · {}", self.descripcion, self.nombre),
        }
    }

    /// Como se la nombra al lado de otra: «a la derecha de TV».
    fn corto(&self) -> &str {
        self.apodo.as_deref().unwrap_or(&self.nombre)
    }

    /// La posición elegida para esta salida: si su tamaño es supuesto, el modo va con ella.
    fn lugar(&self, x: i32, y: i32) -> Campo {
        match self.supuesto {
            Some(m) => Campo::Lugar(x, y, m),
            None => Campo::Posicion(x, y),
        }
    }
}

/// `hyprctl -j monitors all`.
pub fn de_json(texto: &str) -> Vec<Salida> {
    let Ok(serde_json::Value::Array(v)) = serde_json::from_str(texto) else {
        return Vec::new();
    };
    v.iter()
        .filter_map(|m| {
            let n = |k: &str| m.get(k).and_then(|x| x.as_f64());
            let modos = m
                .get("availableModes")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|s| s.as_str().and_then(modo)).collect())
                .unwrap_or_default();
            Some(Salida {
                nombre: m.get("name")?.as_str()?.to_owned(),
                descripcion: descripcion(m),
                ancho: n("width")? as u32,
                alto: n("height")? as u32,
                hz: n("refreshRate").unwrap_or(60.0),
                escala: n("scale").unwrap_or(1.0),
                x: n("x").unwrap_or(0.0) as i32,
                y: n("y").unwrap_or(0.0) as i32,
                rotacion: n("transform").unwrap_or(0.0) as u8 % 4,
                apagada: m.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false),
                modos,
                apodo: None,
                supuesto: None,
            })
        })
        .collect()
}

/// Marca y modelo; la descripción completa repite además el conector y el número de serie.
/// Una EDID con el fabricante en cero da `@@@` (cada letra es su código más `@`): sin marca.
fn descripcion(m: &serde_json::Value) -> String {
    let s = |k: &str| m.get(k).and_then(|x| x.as_str()).unwrap_or("").trim();
    let marca = s("make");
    let marca = if marca.chars().all(|c| c == '@') {
        ""
    } else {
        marca
    };
    let marca_modelo = format!("{marca} {}", s("model"));
    marca_modelo.trim().to_owned()
}

/// Cómo estaba cada salida la última vez que se la vio prendida: de ahí salen el tamaño y el
/// lugar de una apagada. Vive en la caché; perderla solo hace volver al modo preferido.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Vistos(pub BTreeMap<String, Visto>);

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Visto {
    pub ancho: u32,
    pub alto: u32,
    pub escala: f64,
    pub rotacion: u8,
    pub x: i32,
    pub y: i32,
}

impl Vistos {
    pub fn leer(ruta: &Path) -> Vistos {
        std::fs::read_to_string(ruta)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Anota las prendidas; `true` si algo cambió y hay que guardarla.
    pub fn anotar(&mut self, salidas: &[Salida]) -> bool {
        let mut cambio = false;
        for s in salidas.iter().filter(|s| !s.apagada) {
            let v = Visto {
                ancho: s.ancho,
                alto: s.alto,
                escala: s.escala,
                rotacion: s.rotacion,
                x: s.x,
                y: s.y,
            };
            if self.0.get(&s.nombre) != Some(&v) {
                self.0.insert(s.nombre.clone(), v);
                cambio = true;
            }
        }
        cambio
    }

    pub fn guardar(&self, ruta: &Path) -> Result<(), String> {
        if let Some(d) = ruta.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        let texto = toml::to_string(self).map_err(|e| e.to_string())?;
        std::fs::write(ruta, texto).map_err(|e| format!("{}: {e}", ruta.display()))
    }
}

/// Pone los apodos y, a las apagadas, el tamaño y el lugar que van a tener al prenderse.
pub fn completar(salidas: &mut [Salida], reglas: &Reglas, vistos: &Vistos) {
    for s in salidas.iter_mut() {
        let regla = reglas.0.get(&s.nombre);
        s.apodo = regla.and_then(|r| r.apodo.clone());
        if !s.apagada {
            continue;
        }
        let visto = vistos.0.get(&s.nombre);
        let modo = regla.and_then(|r| r.mode.as_deref()).and_then(modo_regla);
        match (modo, visto) {
            (Some((a, b, hz)), _) => (s.ancho, s.alto, s.hz) = (a, b, hz),
            (None, Some(v)) => (s.ancho, s.alto) = (v.ancho, v.alto),
            (None, None) => {
                if let Some(&(a, b, hz)) = s.modos.first() {
                    (s.ancho, s.alto, s.hz) = (a, b, hz);
                    s.supuesto = Some((a, b, hz));
                }
            }
        }
        s.escala = regla
            .and_then(|r| r.scale)
            .or(visto.map(|v| v.escala))
            .unwrap_or(1.0);
        s.rotacion = regla
            .and_then(|r| r.transform)
            .or(visto.map(|v| v.rotacion))
            .unwrap_or(0);
        let lugar = regla
            .and_then(|r| r.position.as_deref())
            .and_then(posicion_regla)
            .or(visto.map(|v| (v.x, v.y)));
        (s.x, s.y) = lugar.unwrap_or((i32::MIN, i32::MIN));
    }
    // Sin lugar conocido: a la derecha de todo lo demás.
    let mut derecha = salidas
        .iter()
        .filter(|s| s.x != i32::MIN)
        .map(|s| s.x + s.logico().0)
        .max()
        .unwrap_or(0);
    for s in salidas.iter_mut().filter(|s| s.x == i32::MIN) {
        (s.x, s.y) = (derecha, 0);
        derecha += s.logico().0;
    }
}

/// `"1920x1080@60"`, como se escribe en la regla.
fn modo_regla(s: &str) -> Option<(u32, u32, f64)> {
    let (tam, hz) = s.split_once('@').unwrap_or((s, "60"));
    let (a, b) = tam.split_once('x')?;
    Some((a.parse().ok()?, b.parse().ok()?, hz.parse().ok()?))
}

/// `"0x-1080"`.
fn posicion_regla(s: &str) -> Option<(i32, i32)> {
    let (x, y) = s.split_once('x')?;
    Some((x.parse().ok()?, y.parse().ok()?))
}

/// `"2560x1440@299.94Hz"`.
fn modo(s: &str) -> Option<(u32, u32, f64)> {
    let (tam, hz) = s.trim_end_matches("Hz").split_once('@')?;
    let (a, b) = tam.split_once('x')?;
    Some((a.parse().ok()?, b.parse().ok()?, hz.parse().ok()?))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Campo {
    /// Resolución y frecuencia van juntas: Hyprland las pide en el mismo `mode`.
    Modo(u32, u32, f64),
    Escala(f64),
    Rotacion(u8),
    Posicion(i32, i32),
    /// La posición de una apagada nunca vista, con el modo con que se la dibujó.
    Lugar(i32, i32, (u32, u32, f64)),
    Encendida(bool),
    /// El nombre propio; vacío vuelve al de fábrica.
    Apodo(String),
    /// Vuelve a lo de los archivos del usuario: borra lo guardado desde el menú, menos el
    /// nombre, que no es de Hyprland.
    Restablecer,
}

impl Campo {
    /// Lo que se prueba antes de guardarlo.
    pub fn a_prueba(&self) -> bool {
        matches!(
            self,
            Campo::Modo(..) | Campo::Escala(_) | Campo::Rotacion(_)
        )
    }
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Regla {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Solo de morgan: no va al Lua.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apodo: Option<String>,
}

impl Regla {
    fn poner(&mut self, campo: &Campo) {
        match campo {
            Campo::Modo(a, b, hz) => self.mode = Some(texto_modo(*a, *b, *hz)),
            Campo::Escala(e) => self.scale = Some(*e),
            Campo::Rotacion(r) => self.transform = Some(*r),
            Campo::Posicion(x, y) => self.position = Some(format!("{x}x{y}")),
            Campo::Lugar(x, y, (a, b, hz)) => {
                self.position = Some(format!("{x}x{y}"));
                self.mode = Some(texto_modo(*a, *b, *hz));
            }
            Campo::Encendida(v) => self.disabled = Some(!v),
            Campo::Apodo(t) => {
                let t = t.trim();
                self.apodo = (!t.is_empty()).then(|| t.to_owned());
            }
            Campo::Restablecer => {
                *self = Regla {
                    apodo: self.apodo.take(),
                    ..Regla::default()
                }
            }
        }
    }

    fn vacia(&self) -> bool {
        *self == Regla::default()
    }

    /// Tiene algo que va a Hyprland (no solo el nombre).
    fn de_hypr(&self) -> bool {
        Regla {
            apodo: None,
            ..self.clone()
        } != Regla::default()
    }

    /// `hl.monitor({ output = "DP-3", scale = 1.5 })`, solo con lo que tiene.
    fn lua(&self, salida: &str) -> String {
        let mut campos = vec![format!("output = {salida:?}")];
        if let Some(m) = &self.mode {
            campos.push(format!("mode = {m:?}"));
        }
        if let Some(p) = &self.position {
            campos.push(format!("position = {p:?}"));
        }
        if let Some(s) = self.scale {
            campos.push(format!("scale = {}", numero(s)));
        }
        if let Some(t) = self.transform {
            campos.push(format!("transform = {t}"));
        }
        if let Some(d) = self.disabled {
            campos.push(format!("disabled = {d}"));
        }
        format!("hl.monitor({{ {} }})", campos.join(", "))
    }
}

fn texto_modo(a: u32, b: u32, hz: f64) -> String {
    format!("{a}x{b}@{hz:.2}")
}

/// Sin ceros de más: `1.5`, `2`, `1.333333`.
fn numero(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Reglas(pub BTreeMap<String, Regla>);

impl Reglas {
    pub fn leer(ruta: &Path) -> Result<Reglas, String> {
        match std::fs::read_to_string(ruta) {
            Ok(t) => toml::from_str(&t).map_err(|e| format!("{}: {e}", ruta.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Reglas::default()),
            Err(e) => Err(format!("{}: {e}", ruta.display())),
        }
    }

    pub fn guardar(&self, ruta: &Path) -> Result<(), String> {
        let texto = toml::to_string(self).map_err(|e| e.to_string())?;
        super::compositor::escribir(ruta, &texto)
    }

    pub fn poner(&mut self, salida: &str, campo: &Campo) {
        let r = self.0.entry(salida.to_owned()).or_default();
        r.poner(campo);
        if r.vacia() {
            self.0.remove(salida);
        }
    }

    pub fn lua(&self) -> String {
        if !self.0.values().any(Regla::de_hypr) {
            return String::new();
        }
        let mut out = String::from("-- Monitores cambiados desde el menú.\n");
        for (salida, r) in self.0.iter().filter(|(_, r)| r.de_hypr()) {
            out.push_str(&r.lua(salida));
            out.push('\n');
        }
        out.push('\n');
        out
    }
}

/// El Lua de la prueba en vivo, para `hyprctl eval`.
pub fn prueba(salida: &str, campo: &Campo) -> String {
    let mut r = Regla::default();
    r.poner(campo);
    r.lua(salida)
}

/// Las escalas que dejan un tamaño entero: Hyprland corrige las otras a la más cercana, y lo
/// marcado dejaría de ser lo elegido.
fn escalas(s: &Salida) -> Vec<f64> {
    let entera = |v: f64| (v - v.round()).abs() < 0.01;
    ESCALAS
        .iter()
        .copied()
        .filter(|e| entera(s.ancho as f64 / e) && entera(s.alto as f64 / e))
        .collect()
}

/// Para leer: `1.33`, no `1.333333`.
fn corto(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn mismo(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

fn texto_hz(hz: f64) -> String {
    if mismo(hz, hz.round()) {
        format!("{} Hz", hz.round())
    } else {
        format!("{hz:.2} Hz")
    }
}

fn texto_rotacion(r: u8) -> String {
    match r {
        1 => "90°".to_owned(),  // no-tr
        2 => "180°".to_owned(), // no-tr
        3 => "270°".to_owned(), // no-tr
        _ => tr!("normal").to_owned(),
    }
}

fn que(s: &Salida, campo: Campo) -> Que {
    Que::Monitor(s.nombre.clone(), campo)
}

/// Las filas de la sección. `pendiente`: hay una prueba esperando respuesta, y la sección solo
/// ofrece mantenerla o volver.
pub fn filas(salidas: &[Salida], reglas: &Reglas, pendiente: bool) -> Vec<Fila> {
    if pendiente {
        return vec![
            ajuste(
                tr!("¿Mantener estos cambios?"),
                &trf!("si no, vuelve sola a lo de antes en {} s", SEGUNDOS_PRUEBA),
                Control::Boton(Que::MantenerMonitores),
            ),
            ajuste(
                tr!("Volver a lo de antes"),
                "",
                Control::Boton(Que::VolverMonitores),
            ),
        ];
    }
    let encendidas = salidas.iter().filter(|s| !s.apagada).count();
    let mut out = Vec::new();
    if salidas.len() > 1 {
        out.push(Fila::Disposicion(salidas.iter().map(pantalla).collect()));
    }
    for s in salidas {
        out.push(ajuste(&s.titulo(), &estado(s), Control::Nada));
        out.push(nombre(s));
        if s.apagada {
            if let Some(f) = posicion(s, salidas) {
                out.push(f);
            }
            out.push(encendida(s, encendidas));
            continue;
        }
        out.push(resolucion(s));
        out.push(frecuencia(s));
        out.push(escala(s));
        out.push(rotacion(s));
        if let Some(f) = posicion(s, salidas) {
            out.push(f);
        }
        out.push(encendida(s, encendidas));
        if reglas.0.get(&s.nombre).is_some_and(Regla::de_hypr) {
            out.push(ajuste(
                tr!("Volver a la config de Hyprland"),
                tr!("olvida lo cambiado desde el menú para este monitor"),
                Control::Confirmar(que(s, Campo::Restablecer)),
            ));
        }
    }
    out
}

fn estado(s: &Salida) -> String {
    if s.apagada {
        return tr!("apagado").to_owned();
    }
    let (a, b) = s.logico();
    trf!(
        "{} × {} · {} · se ve como {} × {}",
        s.ancho,
        s.alto,
        texto_hz(s.hz),
        a,
        b
    )
}

fn resolucion(s: &Salida) -> Fila {
    let mut tamanos: Vec<(u32, u32)> = Vec::new();
    for &(a, b, _) in &s.modos {
        if !tamanos.contains(&(a, b)) {
            tamanos.push((a, b));
        }
    }
    tamanos.sort_by_key(|t| std::cmp::Reverse(t.0 * t.1));
    let opciones = tamanos
        .iter()
        .map(|&(a, b)| {
            // Con la frecuencia más alta que tenga ese tamaño.
            let hz = s
                .modos
                .iter()
                .filter(|m| (m.0, m.1) == (a, b))
                .map(|m| m.2)
                .fold(0.0, f64::max);
            opcion(&format!("{a} × {b}"), "", que(s, Campo::Modo(a, b, hz)))
                .activa((a, b) == (s.ancho, s.alto))
        })
        .collect();
    ajuste(
        tr!("Resolución"),
        &format!("{} × {}", s.ancho, s.alto),
        Control::Lista(opciones),
    )
}

fn frecuencia(s: &Salida) -> Fila {
    let mut hzs: Vec<f64> = s
        .modos
        .iter()
        .filter(|m| (m.0, m.1) == (s.ancho, s.alto))
        .map(|m| m.2)
        .collect();
    hzs.sort_by(|a, b| b.total_cmp(a));
    hzs.dedup_by(|a, b| mismo(*a, *b));
    let opciones = hzs
        .iter()
        .map(|&hz| {
            opcion(&texto_hz(hz), "", que(s, Campo::Modo(s.ancho, s.alto, hz)))
                .activa(mismo(hz, s.hz))
        })
        .collect();
    ajuste(tr!("Frecuencia"), &texto_hz(s.hz), Control::Lista(opciones))
}

fn escala(s: &Salida) -> Fila {
    let opciones = escalas(s)
        .into_iter()
        .map(|e| {
            let (a, b) = Salida {
                escala: e,
                ..s.clone()
            }
            .logico();
            opcion(
                &format!("{} ×", corto(e)),
                &trf!("se ve como {} × {}", a, b),
                que(s, Campo::Escala(e)),
            )
            .activa(mismo(e, s.escala))
        })
        .collect();
    ajuste(
        tr!("Escala"),
        &trf!(
            "{} × · más grande es más legible, entra menos",
            corto(s.escala)
        ),
        Control::Lista(opciones),
    )
}

fn rotacion(s: &Salida) -> Fila {
    let opciones = (0..4)
        .map(|r| opcion(&texto_rotacion(r), "", que(s, Campo::Rotacion(r))).activa(r == s.rotacion))
        .collect();
    ajuste(
        tr!("Rotación"),
        &texto_rotacion(s.rotacion),
        Control::Lista(opciones),
    )
}

fn nombre(s: &Salida) -> Fila {
    let fabrica = if s.descripcion.is_empty() {
        &s.nombre
    } else {
        &s.descripcion
    };
    let sub = match &s.apodo {
        Some(_) => trf!("el de fábrica es {}", fabrica),
        None => tr!("ponele uno que se reconozca: «TV», «el de la izquierda»").to_owned(),
    };
    ajuste(
        tr!("Nombre"),
        &sub,
        Control::Texto {
            valor: s.apodo.clone().unwrap_or_default(),
            pista: fabrica.clone(),
            que: que(s, Campo::Apodo(String::new())),
        },
    )
}

/// Un monitor en el dibujo, en píxeles del escritorio.
#[derive(Debug, Clone, PartialEq)]
pub struct Pantalla {
    pub nombre: String,
    pub titulo: String,
    pub x: i32,
    pub y: i32,
    pub ancho: i32,
    pub alto: i32,
    pub apagada: bool,
    supuesto: Option<(u32, u32, f64)>,
}

impl Pantalla {
    /// Lo que se guarda al soltarla en `(x, y)`.
    pub fn lugar(&self, x: i32, y: i32) -> Campo {
        match self.supuesto {
            Some(m) => Campo::Lugar(x, y, m),
            None => Campo::Posicion(x, y),
        }
    }

    fn pisa(&self, x: i32, y: i32, o: &Pantalla) -> bool {
        x < o.x + o.ancho && o.x < x + self.ancho && y < o.y + o.alto && o.y < y + self.alto
    }
}

fn pantalla(s: &Salida) -> Pantalla {
    let (ancho, alto) = s.logico();
    Pantalla {
        nombre: s.nombre.clone(),
        titulo: s.corto().to_owned(),
        x: s.x,
        y: s.y,
        ancho,
        alto,
        apagada: s.apagada,
        supuesto: s.supuesto,
    }
}

/// Dónde queda la pantalla `i` soltada con su esquina en `(x, y)`: pegada al lado más cercano
/// de alguna otra, sin pisar ninguna. A lo largo del borde se corre libre, pero si queda cerca
/// de alinear un borde o el centro con la otra, se alinea.
pub fn encajar(p: &[Pantalla], i: usize, x: f64, y: f64) -> (i32, i32) {
    let s = &p[i];
    let (a, b) = (s.ancho, s.alto);
    let mut mejor: Option<((i32, i32), f64)> = None;
    for o in p
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != i)
        .map(|(_, o)| o)
    {
        // A lo largo del borde: que se toquen al menos un poco, y alineado si está cerca.
        let a_lo_largo = |v: f64, desde: i32, largo: i32, propio: i32| -> i32 {
            let v = (v.round() as i32).clamp(desde - propio + 1, desde + largo - 1);
            let umbral = largo.min(propio) / 8;
            [desde, desde + largo - propio, desde + (largo - propio) / 2]
                .into_iter()
                .min_by_key(|c| (c - v).abs())
                .filter(|c| (c - v).abs() <= umbral)
                .unwrap_or(v)
        };
        let ladox = a_lo_largo(x, o.x, o.ancho, a);
        let ladoy = a_lo_largo(y, o.y, o.alto, b);
        for c in [
            (o.x + o.ancho, ladoy),
            (o.x - a, ladoy),
            (ladox, o.y - b),
            (ladox, o.y + o.alto),
        ] {
            if p.iter()
                .enumerate()
                .any(|(k, q)| k != i && s.pisa(c.0, c.1, q))
            {
                continue;
            }
            let d = (c.0 as f64 - x).hypot(c.1 as f64 - y);
            if mejor.is_none_or(|(_, m)| d < m) {
                mejor = Some((c, d));
            }
        }
    }
    mejor.map_or((s.x, s.y), |(c, _)| c)
}

/// Al lado de cada uno de los otros encendidos, alineado por el borde de arriba o el de la
/// izquierda.
fn posicion(s: &Salida, salidas: &[Salida]) -> Option<Fila> {
    let otras: Vec<&Salida> = salidas
        .iter()
        .filter(|o| !o.apagada && o.nombre != s.nombre)
        .collect();
    if otras.is_empty() {
        return None;
    }
    let (a, b) = s.logico();
    let mut opciones = Vec::new();
    let mut puesta = None;
    for o in otras {
        let (oa, ob) = o.logico();
        for (texto, x, y) in [
            (trf!("a la derecha de {}", o.corto()), o.x + oa, o.y),
            (trf!("a la izquierda de {}", o.corto()), o.x - a, o.y),
            (trf!("arriba de {}", o.corto()), o.x, o.y - b),
            (trf!("debajo de {}", o.corto()), o.x, o.y + ob),
        ] {
            let activa = (x, y) == (s.x, s.y);
            if activa {
                puesta = Some(texto.clone());
            }
            opciones.push(opcion(&texto, "", que(s, s.lugar(x, y))).activa(activa));
        }
    }
    let sub = puesta.unwrap_or_else(|| trf!("en {} × {}", s.x, s.y));
    Some(ajuste(tr!("Posición"), &sub, Control::Lista(opciones)))
}

fn encendida(s: &Salida, encendidas: usize) -> Fila {
    if !s.apagada && encendidas <= 1 {
        return ajuste(
            tr!("Encendido"),
            tr!("es el único encendido"),
            Control::Nada,
        );
    }
    ajuste(
        tr!("Encendido"),
        "",
        Control::Casilla {
            activa: !s.apagada,
            que: que(s, Campo::Encendida(s.apagada)),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"[
        {"name": "DP-3", "make": "KTC", "model": "H27S17", "width": 2560, "height": 1440,
         "refreshRate": 299.99, "x": 0, "y": 0, "scale": 1.0, "transform": 0, "disabled": false,
         "availableModes": ["2560x1440@299.99Hz", "2560x1440@59.95Hz", "1920x1080@60.00Hz"]},
        {"name": "HDMI-A-1", "make": "", "model": "", "width": 1920, "height": 1080,
         "refreshRate": 60.0, "x": 0, "y": -1080, "scale": 1.0, "transform": 0, "disabled": true,
         "availableModes": ["1920x1080@60.00Hz"]}
    ]"#;

    fn listas(f: &[Fila], titulo: &str) -> Vec<(String, bool, Que)> {
        f.iter()
            .find_map(|f| match f {
                Fila::Ajuste {
                    titulo: t,
                    control: Control::Lista(o),
                    ..
                } if t == titulo => Some(
                    o.iter()
                        .map(|o| (o.titulo.clone(), o.activa, o.que.clone()))
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default()
    }

    #[test]
    fn lee_lo_que_da_hyprctl() {
        let s = de_json(JSON);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].descripcion, "KTC H27S17");
        assert_eq!(s[0].modos[0], (2560, 1440, 299.99));
        assert!(s[1].apagada);
        assert_eq!(s[1].titulo(), "HDMI-A-1");
        assert!(de_json("no").is_empty());
    }

    #[test]
    fn la_regla_lleva_solo_lo_cambiado() {
        let mut r = Reglas::default();
        r.poner("DP-3", &Campo::Escala(1.5));
        r.poner("DP-3", &Campo::Modo(2560, 1440, 299.99));
        assert_eq!(
            r.lua(),
            "-- Monitores cambiados desde el menú.\nhl.monitor({ output = \"DP-3\", mode = \"2560x1440@299.99\", scale = 1.5 })\n\n"
        );
        r.poner("DP-3", &Campo::Restablecer);
        assert_eq!(r.lua(), "", "restablecer borra la salida entera");
        assert_eq!(
            prueba("X", &Campo::Encendida(false)),
            "hl.monitor({ output = \"X\", disabled = true })"
        );
        let t = toml::to_string(&{
            let mut r = Reglas::default();
            r.poner("DP-3", &Campo::Posicion(0, -1080));
            r
        })
        .unwrap();
        assert_eq!(
            toml::from_str::<Reglas>(&t).unwrap().0["DP-3"]
                .position
                .as_deref(),
            Some("0x-1080")
        );
    }

    #[test]
    fn solo_escalas_que_dan_un_tamano_entero() {
        let s = &de_json(JSON)[0];
        let e = escalas(s);
        assert!(e.contains(&1.25) && e.contains(&1.6) && e.contains(&2.0));
        assert!(!e.contains(&1.75), "2560 / 1,75 no es entero");
        assert!(!e.contains(&1.5), "1440 / 1,5 sí, pero 2560 / 1,5 no");
    }

    #[test]
    fn resolucion_y_frecuencia_marcan_lo_puesto() {
        let s = de_json(JSON);
        let f = filas(&s, &Reglas::default(), false);
        let r = listas(&f, "Resolución");
        assert_eq!(r[0].0, "2560 × 1440");
        assert!(r[0].1);
        // Cambiar de resolución pide la frecuencia más alta de esa resolución.
        assert_eq!(
            r[1].2,
            Que::Monitor("DP-3".into(), Campo::Modo(1920, 1080, 60.0))
        );
        let hz = listas(&f, "Frecuencia");
        assert_eq!(
            hz.iter().map(|h| h.0.as_str()).collect::<Vec<_>>(),
            ["300 Hz", "59.95 Hz"]
        );
        assert!(hz[0].1);
    }

    #[test]
    fn el_unico_encendido_no_se_apaga_y_el_apagado_solo_se_prende() {
        let s = de_json(JSON);
        let f = filas(&s, &Reglas::default(), false);
        let encendido: Vec<&Control> = f
            .iter()
            .filter_map(|f| match f {
                Fila::Ajuste {
                    titulo, control, ..
                } if titulo == "Encendido" => Some(control),
                _ => None,
            })
            .collect();
        assert_eq!(encendido[0], &Control::Nada);
        assert_eq!(
            encendido[1],
            &Control::Casilla {
                activa: false,
                que: Que::Monitor("HDMI-A-1".into(), Campo::Encendida(true))
            }
        );
        // El único encendido no tiene al lado de quién ponerse; el apagado sí, para cuando
        // se prenda.
        let p = listas(&f, "Posición");
        assert!(
            p.iter()
                .all(|o| matches!(&o.2, Que::Monitor(s, _) if s == "HDMI-A-1"))
        );
        assert!(p.iter().any(|o| o.0 == "arriba de DP-3" && o.1));
    }

    #[test]
    fn el_fabricante_en_cero_no_se_muestra() {
        let s = de_json(
            r#"[{"name": "HDMI-A-1", "make": "@@@", "model": "SMART TV", "width": 0, "height": 0}]"#,
        );
        assert_eq!(s[0].descripcion, "SMART TV");
    }

    #[test]
    fn el_nombre_es_solo_de_morgan() {
        let mut r = Reglas::default();
        r.poner("HDMI-A-1", &Campo::Apodo("  TV  ".into()));
        assert_eq!(r.0["HDMI-A-1"].apodo.as_deref(), Some("TV"));
        assert_eq!(r.lua(), "", "el nombre no va a Hyprland");
        r.poner("HDMI-A-1", &Campo::Escala(2.0));
        r.poner("HDMI-A-1", &Campo::Restablecer);
        assert_eq!(
            r.0["HDMI-A-1"].apodo.as_deref(),
            Some("TV"),
            "restablecer lo deja"
        );
        assert_eq!(r.lua(), "");

        let mut s = de_json(JSON);
        s[1].apagada = false;
        completar(&mut s, &r, &Vistos::default());
        assert_eq!(s[1].titulo(), "TV · HDMI-A-1");
        let f = filas(&s, &r, false);
        assert!(listas(&f, "Posición").iter().any(|o| o.0 == "debajo de TV"));
        // Sin nada que ir a Hyprland, no hay config que restablecer.
        assert!(!f.iter().any(|f| matches!(f, Fila::Ajuste { titulo, .. }
            if titulo == "Volver a la config de Hyprland")));

        r.poner("HDMI-A-1", &Campo::Apodo(" ".into()));
        assert!(r.0.is_empty(), "vacío vuelve al de fábrica");
        assert_eq!(
            Que::Monitor("X".into(), Campo::Apodo(String::new())).con_texto("Tele"),
            Que::Monitor("X".into(), Campo::Apodo("Tele".into()))
        );
    }

    #[test]
    fn la_apagada_toma_tamano_y_lugar_de_lo_que_se_sabe() {
        let json = JSON.replace(r#""x": 0, "y": -1080"#, r#""x": -1, "y": -1"#);
        // Nunca vista: el modo preferido, y ubicarla lo guarda con la posición.
        let mut s = de_json(&json);
        completar(&mut s, &Reglas::default(), &Vistos::default());
        assert_eq!((s[1].ancho, s[1].alto), (1920, 1080));
        assert_eq!((s[1].x, s[1].y), (2560, 0), "a la derecha de todo");
        let p = listas(&filas(&s, &Reglas::default(), false), "Posición");
        assert!(p.iter().any(|o| o.0 == "a la derecha de DP-3" && o.1));
        assert!(p.iter().any(|o| o.2
            == Que::Monitor(
                "HDMI-A-1".into(),
                Campo::Lugar(0, -1080, (1920, 1080, 60.0))
            )));

        // Vista prendida: su tamaño y su lugar de entonces, sin modo que guardar.
        let mut prendida = de_json(JSON);
        prendida[1].apagada = false;
        prendida[1].escala = 2.0;
        let mut v = Vistos::default();
        assert!(v.anotar(&prendida));
        assert!(!v.anotar(&prendida), "lo mismo no se vuelve a guardar");
        let mut s = de_json(&json);
        completar(&mut s, &Reglas::default(), &v);
        assert_eq!(s[1].logico(), (960, 540));
        assert_eq!((s[1].x, s[1].y), (0, -1080));
        assert_eq!(s[1].supuesto, None);

        // Lo elegido en el menú manda.
        let mut r = Reglas::default();
        r.poner("HDMI-A-1", &Campo::Lugar(-1920, 0, (1920, 1080, 60.0)));
        let mut s = de_json(&json);
        completar(&mut s, &r, &v);
        assert_eq!((s[1].x, s[1].y, s[1].ancho), (-1920, 0, 1920));
    }

    fn dos() -> Vec<Pantalla> {
        let mut s = de_json(JSON);
        s[1].apagada = false;
        s.iter().map(pantalla).collect()
    }

    #[test]
    fn soltada_se_pega_al_lado_mas_cercano() {
        let p = dos();
        // Arriba de DP-3, un poco corrida: se alinea a la izquierda.
        assert_eq!(encajar(&p, 1, 40.0, -1200.0), (0, -1080));
        // Cerca del centro de arriba: centrada.
        assert_eq!(encajar(&p, 1, 300.0, -900.0), (320, -1080));
        // A la derecha, lejos de alinear: queda donde se soltó, pegada al borde.
        assert_eq!(encajar(&p, 1, 2700.0, 500.0), (2560, 500));
        // Encima de DP-3 no queda: sale por el lado más cercano.
        let (x, y) = encajar(&p, 1, 1500.0, 1000.0);
        assert!(!p[1].pisa(x, y, &p[0]));
        assert_eq!((x, y), (1500, 1440));
    }

    #[test]
    fn la_posicion_se_ofrece_al_lado_de_los_otros() {
        let mut s = de_json(JSON);
        s[1].apagada = false;
        let f = filas(&s, &Reglas::default(), false);
        let p = listas(&f, "Posición");
        // La primera lista es la de DP-3: arriba de la TV no está, la TV está arriba de él.
        assert!(p.iter().any(|o| o.0 == "debajo de HDMI-A-1" && o.1));
        assert!(
            p.iter()
                .any(|o| o.2 == Que::Monitor("DP-3".into(), Campo::Posicion(1920, -1080)))
        );
    }

    #[test]
    fn con_una_prueba_pendiente_solo_mantener_o_volver() {
        let f = filas(&de_json(JSON), &Reglas::default(), true);
        assert_eq!(f.len(), 2);
        assert!(matches!(
            &f[0],
            Fila::Ajuste {
                control: Control::Boton(Que::MantenerMonitores),
                ..
            }
        ));
    }

    #[test]
    fn a_prueba_lo_que_puede_dejar_la_pantalla_ilegible() {
        assert!(Campo::Escala(2.0).a_prueba());
        assert!(Campo::Rotacion(1).a_prueba());
        assert!(!Campo::Posicion(0, 0).a_prueba());
        assert!(!Campo::Encendida(false).a_prueba());
    }
}
