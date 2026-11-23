//! El driver de Umbriel (`docs/user/ipc.md` del repo `noctalia-dev/umbriel`, leído el
//! 2026-11-06 contra `umbriel-git 0.1.0.r1525`). Una conexión suscripta a `workspaces`,
//! `windows`, `outputs` y `submap`: primero llega el estado de cada familia y después una foto
//! entera con cada cambio, así que se reemplaza lo guardado y se compara con lo anterior.
//! Los pedidos, cada uno en su conexión y en un hilo aparte.
//!
//! Los workspaces de Umbriel son por salida (`DP-3:1`, `HDMI-A-1:1`). morgan los numera como
//! Hyprland: los de la primera salida (por nombre) son 1, 2, 3…; los de la segunda, 101,
//! 102…; en la barra se ve el número de cada uno en su salida.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gio::prelude::*;
use serde::Deserialize;
use serde_json::json;

use morgan_ipc::umbriel::{accion, pedir_ya, respuesta_de};

use super::{CambioVentanas, Cliente, Compositor, Espacio, Leido, Rect, Salida};

/// El salto entre los workspaces de una salida y los de la siguiente.
const POR_SALIDA: i64 = 100;
/// Las de un scratchpad: como el `special:` de Hyprland, el dock las muestra y el lienzo no.
const WS_SCRATCHPAD: i64 = -2;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Ventana {
    pub id: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub pid: Option<i64>,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default)]
    pub w: i32,
    #[serde(default)]
    pub h: i32,
    #[serde(default)]
    pub output: String,
    #[serde(default)]
    pub workspace: String,
    /// Tiene el foco del asiento.
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub minimized: bool,
    #[serde(default)]
    pub scratchpad: String,
    #[serde(default)]
    pub visible: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Workspace {
    pub id: String,
    pub index: i64,
    #[serde(default)]
    pub name: String,
    pub output: String,
    #[serde(default)]
    pub window_count: u32,
    #[serde(default)]
    pub urgent: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Output {
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub active_workspace: Option<String>,
    #[serde(default)]
    pub position: Punto,
    #[serde(default)]
    pub logical_size: Tamano,
    #[serde(default = "uno")]
    pub scale: f64,
    #[serde(default)]
    pub modes: Vec<Modo>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Punto {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Tamano {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Modo {
    pub width: i32,
    pub height: i32,
    #[serde(default)]
    pub current: bool,
}

fn uno() -> f64 {
    1.0
}

/// La última foto de cada familia.
#[derive(Debug, Default)]
pub struct Estado {
    pub ventanas: Vec<Ventana>,
    pub workspaces: Vec<Workspace>,
    pub outputs: Vec<Output>,
    pub submap: Option<String>,
    /// Los ids por uso: el primero es el que tuvo el foco último.
    pub historia: Vec<String>,
}

impl Estado {
    /// Las salidas por nombre: su lugar da el número de sus workspaces.
    fn orden(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .outputs
            .iter()
            .map(|o| o.name.as_str())
            .chain(self.workspaces.iter().map(|w| w.output.as_str()))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    pub fn numero(&self, ws: &str) -> Option<i64> {
        let w = self.workspaces.iter().find(|w| w.id == ws)?;
        let lugar = self.orden().iter().position(|o| *o == w.output)? as i64;
        Some(lugar * POR_SALIDA + w.index)
    }

    /// Al revés: la salida y la posición del workspace en ella.
    pub fn de_numero(&self, n: i64) -> Option<(String, i64)> {
        let orden = self.orden();
        let salida = orden.get(usize::try_from(n.div_euclid(POR_SALIDA)).ok()?)?;
        Some(((*salida).to_owned(), n.rem_euclid(POR_SALIDA)))
    }

    pub fn leido(&self) -> Leido {
        let mut espacios: Vec<Espacio> = self
            .workspaces
            .iter()
            .filter_map(|w| {
                Some(Espacio {
                    id: self.numero(&w.id)?,
                    monitor: w.output.clone(),
                    ventanas: w.window_count,
                    nombre: w.index.to_string(),
                })
            })
            .collect();
        espacios.sort_by_key(|e| e.id);
        let prendidas = self.outputs.iter().filter(|o| o.enabled);
        let activos = prendidas
            .clone()
            .filter_map(|o| {
                let n = self.numero(o.active_workspace.as_deref()?)?;
                Some((o.name.clone(), n))
            })
            .collect();
        let enfocada = prendidas
            .clone()
            .find(|o| o.focused)
            .map(|o| o.name.clone());
        (espacios, activos, enfocada)
    }

    pub fn urgentes(&self) -> Vec<i64> {
        self.workspaces
            .iter()
            .filter(|w| w.urgent)
            .filter_map(|w| self.numero(&w.id))
            .collect()
    }

    pub fn activa(&self) -> Option<&Ventana> {
        self.ventanas.iter().find(|v| v.active)
    }

    /// Anota la que tiene el foco al frente de la historia y saca las que ya no están. La
    /// primera foto trae las ventanas por uso (la enfocada primero): sirve de historia inicial.
    fn al_dia(&mut self) {
        let ids: Vec<&String> = self.ventanas.iter().map(|v| &v.id).collect();
        self.historia.retain(|h| ids.contains(&h));
        for v in &self.ventanas {
            if !self.historia.contains(&v.id) {
                self.historia.push(v.id.clone());
            }
        }
        if let Some(a) = self.ventanas.iter().find(|v| v.active)
            && let Some(i) = self.historia.iter().position(|h| *h == a.id)
        {
            let id = self.historia.remove(i);
            self.historia.insert(0, id);
        }
    }

    pub fn clientes(&self) -> Vec<Cliente> {
        self.ventanas
            .iter()
            .filter(|v| !v.minimized)
            .filter_map(|v| {
                let ws = if v.scratchpad.is_empty() {
                    self.numero(&v.workspace)?
                } else if v.visible {
                    // Un scratchpad a la vista queda sobre el workspace activo de su salida.
                    WS_SCRATCHPAD
                } else {
                    return None;
                };
                Some(Cliente {
                    id: v.id.clone(),
                    clase: v.app_id.clone(),
                    titulo: v.title.clone(),
                    ws,
                    rect: Rect {
                        x: v.x,
                        y: v.y,
                        w: v.w,
                        h: v.h,
                    },
                    salida: v.output.clone(),
                    historia: self
                        .historia
                        .iter()
                        .position(|h| *h == v.id)
                        .map_or(i32::MAX, |i| i as i32),
                    juego: super::es_juego(&v.app_id, &v.content_type, None, |_| false),
                    pid: v.pid.and_then(|p| u32::try_from(p).ok()).filter(|p| *p > 1),
                })
            })
            .collect()
    }

    pub fn salidas(&self) -> Vec<Salida> {
        self.outputs
            .iter()
            .filter(|o| o.enabled)
            .map(|o| {
                let modo = o.modes.iter().find(|m| m.current);
                Salida {
                    nombre: o.name.clone(),
                    rect: Rect {
                        x: o.position.x,
                        y: o.position.y,
                        w: o.logical_size.width,
                        h: o.logical_size.height,
                    },
                    escala: o.scale,
                    ancho: modo.map_or(o.logical_size.width, |m| m.width),
                    alto: modo.map_or(o.logical_size.height, |m| m.height),
                    enfocada: o.focused,
                    activo: o
                        .active_workspace
                        .as_deref()
                        .and_then(|w| self.numero(w))
                        .unwrap_or(0),
                }
            })
            .collect()
    }
}

/// Lo que cambió entre dos fotos de `windows`, para el dock.
pub fn cambio(antes: &[Ventana], ahora: &[Ventana]) -> Vec<CambioVentanas> {
    let mut v = Vec::new();
    let lugar = |l: &[Ventana]| {
        let mut x: Vec<(String, String, String)> = l
            .iter()
            .map(|v| (v.id.clone(), v.workspace.clone(), v.scratchpad.clone()))
            .collect();
        x.sort();
        x
    };
    if lugar(antes) != lugar(ahora) {
        v.push(CambioVentanas::Lista);
    }
    let activa = |l: &[Ventana]| l.iter().find(|v| v.active).map(|v| v.id.clone());
    if activa(antes) != activa(ahora) {
        v.push(CambioVentanas::Foco(activa(ahora).unwrap_or_default()));
    }
    let titulos = |l: &[Ventana]| {
        let mut x: Vec<(String, String)> =
            l.iter().map(|v| (v.id.clone(), v.title.clone())).collect();
        x.sort();
        x
    };
    if titulos(antes) != titulos(ahora) {
        v.push(CambioVentanas::Titulo);
    }
    v
}

#[derive(Clone)]
pub struct Umbriel {
    ruta: PathBuf,
    estado: Rc<RefCell<Estado>>,
}

impl Umbriel {
    pub fn new() -> Option<Self> {
        let Some(ruta) = morgan_ipc::umbriel::socket() else {
            eprintln!("morgan: sin el socket de Umbriel");
            return None;
        };
        Some(Umbriel {
            ruta,
            estado: Rc::default(),
        })
    }

    pub fn arrancar(&self, c: &Compositor) {
        let (u, c) = (self.clone(), c.clone());
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = u.seguir(&c).await {
                eprintln!("morgan: eventos de Umbriel: {e}");
            }
        });
    }

    async fn pedir(&self, pedido: serde_json::Value) -> Result<serde_json::Value, String> {
        let ruta = self.ruta.clone();
        gio::spawn_blocking(move || pedir_ya(&ruta, &pedido))
            .await
            .unwrap_or_else(|_| Err(std::io::Error::other("el hilo del pedido entró en pánico"))) // no-tr
            .map_err(|e| e.to_string())
    }

    async fn hacer(&self, nombre: &str, campos: serde_json::Value) {
        if let Err(e) = self.pedir(accion(nombre, campos)).await {
            eprintln!("morgan: umbriel {nombre}: {e}");
        }
    }

    pub async fn clientes(&self) -> Option<Vec<Cliente>> {
        Some(self.estado.borrow().clientes())
    }

    pub async fn salidas(&self) -> Option<Vec<Salida>> {
        Some(self.estado.borrow().salidas())
    }

    /// Preguntada en el momento: la foto de `windows` puede venir atrasada respecto de un
    /// pedido de foco recién hecho.
    pub async fn activa(&self) -> Option<String> {
        let v = self.pedir(json!({"cmd": "focused-window"})).await.ok()?;
        v["id"].as_str().map(str::to_owned)
    }

    /// `window-focus` no lleva el puntero: el dock y el lienzo quedan igual.
    pub async fn enfocar(&self, id: &str) {
        self.hacer("window-focus", json!({ "window": id })).await;
    }

    pub async fn cerrar(&self, id: &str) {
        self.hacer("window-close", json!({ "window": id })).await;
    }

    pub async fn ir_a(&self, n: i64) {
        let Some((salida, posicion)) = self.estado.borrow().de_numero(n) else {
            return eprintln!("morgan: umbriel: no hay workspace {n}");
        };
        self.hacer(
            "workspace-switch",
            json!({ "workspace": posicion, "output": salida }),
        )
        .await;
    }

    pub async fn overview(&self) {
        self.hacer("overview-toggle", json!({})).await;
    }

    pub async fn dpms(&self, prender: bool) {
        let nombre = if prender { "dpms-on" } else { "dpms-off" };
        self.hacer(nombre, json!({})).await;
    }

    pub async fn submap(&self) -> Option<String> {
        self.estado.borrow().submap.clone()
    }

    pub async fn salir_del_submap(&self) {
        if self.estado.borrow().submap.is_some() {
            self.hacer("submap", json!({ "submap": "reset" })).await;
        }
    }

    async fn seguir(&self, c: &Compositor) -> Result<(), glib::Error> {
        let conexion = gio::SocketClient::new()
            .connect_future(&gio::UnixSocketAddress::new(&self.ruta))
            .await?;
        let pedido = json!({
            "cmd": "subscribe",
            "events": ["outputs", "workspaces", "windows", "submap"],
        });
        let mut linea = pedido.to_string();
        linea.push('\n');
        conexion
            .output_stream()
            .write_all_future(linea.into_bytes(), glib::Priority::DEFAULT)
            .await
            .map_err(|(_, e)| e)?;
        let lineas = gio::DataInputStream::new(&conexion.input_stream());
        while let Some(linea) = lineas.read_line_future(glib::Priority::DEFAULT).await? {
            let linea = String::from_utf8_lossy(&linea);
            if let Err(e) = self.evento(c, &linea) {
                eprintln!("morgan: umbriel: evento ilegible: {e}");
            }
        }
        eprintln!("morgan: Umbriel cerró el socket de eventos");
        Ok(())
    }

    fn evento(&self, c: &Compositor, linea: &str) -> Result<(), String> {
        #[derive(Deserialize)]
        struct E {
            event: Option<String>,
            #[serde(default)]
            data: serde_json::Value,
        }
        if linea.trim().is_empty() {
            return Ok(());
        }
        let e: E = serde_json::from_str(linea).map_err(|e| e.to_string())?;
        let Some(familia) = e.event else {
            // Un `err` de la suscripción.
            return respuesta_de(linea).map(|_| ()).map_err(|e| e.to_string());
        };
        fn de<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Result<T, String> {
            serde_json::from_value(v).map_err(|e| e.to_string())
        }
        match familia.as_str() {
            "outputs" => {
                self.estado.borrow_mut().outputs = de(e.data)?;
                self.publicar_espacios(c);
                let nombres = self.estado.borrow().salidas().into_iter().map(|s| s.nombre);
                c.salidas_cambiaron(nombres.collect());
            }
            "workspaces" => {
                self.estado.borrow_mut().workspaces = de(e.data)?;
                self.publicar_espacios(c);
            }
            "windows" => {
                let ahora: Vec<Ventana> = de(e.data)?;
                let antes = std::mem::replace(&mut self.estado.borrow_mut().ventanas, ahora);
                self.estado.borrow_mut().al_dia();
                let cambios = cambio(&antes, &self.estado.borrow().ventanas);
                let juego = self.estado.borrow().activa().is_some_and(|v| {
                    super::es_juego(&v.app_id, &v.content_type, v.pid, |p| {
                        super::lanzado_por_steam(p, super::leer_proc)
                    })
                });
                c.poner_juego(juego);
                for cambio in cambios {
                    if let CambioVentanas::Foco(id) = &cambio {
                        c.enfocada(id);
                    }
                    c.ventanas_cambiaron(cambio);
                }
            }
            "submap" => {
                self.estado.borrow_mut().submap = de(e.data)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn publicar_espacios(&self, c: &Compositor) {
        let (leido, urgentes) = {
            let e = self.estado.borrow();
            (e.leido(), e.urgentes())
        };
        c.releer_espacios(leido, true);
        for id in urgentes {
            c.marcar_urgente(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fotos de verdad de Umbriel anidado (2026-11-06), recortadas.
    const VENTANAS: &str = r#"[
        {"active":true,"app_id":"kitty","content_type":"none","focused":true,"fullscreen":false,
         "h":700,"id":"ee73","minimized":false,"output":"HEADLESS-1","pid":133711,
         "scratchpad":"","title":"fish","visible":true,"w":640,"workspace":"HEADLESS-1:1",
         "x":630,"y":10},
        {"active":false,"app_id":"steam_app_730","content_type":"none","focused":false,
         "h":700,"id":"e360","minimized":false,"output":"HEADLESS-1","pid":-1,
         "scratchpad":"","title":"cs2","visible":true,"w":640,"workspace":"DP-3:2",
         "x":-22,"y":10},
        {"active":false,"app_id":"mpv","content_type":"video","h":10,"id":"s1",
         "minimized":false,"output":"HEADLESS-1","scratchpad":"musica","title":"m",
         "visible":false,"w":10,"workspace":"","x":0,"y":0}
    ]"#;
    const WORKSPACES: &str = r#"[
        {"active":true,"focused":true,"focused_window":"ee73","id":"HEADLESS-1:1","index":1,
         "layout":"scrolling","name":"1","named":false,"occupied":true,"output":"HEADLESS-1",
         "urgent":false,"window_count":1},
        {"active":false,"focused":false,"focused_window":"","id":"HEADLESS-1:2","index":2,
         "name":"2","output":"HEADLESS-1","urgent":true,"window_count":0},
        {"active":true,"focused":false,"id":"DP-3:2","index":2,"name":"2","output":"DP-3",
         "urgent":false,"window_count":1}
    ]"#;
    const SALIDAS: &str = r#"[
        {"active_workspace":"HEADLESS-1:1","enabled":true,"focused":true,
         "logical_size":{"height":720,"width":1280},"name":"HEADLESS-1",
         "modes":[{"current":true,"height":1440,"width":2560,"refresh_mhz":0}],
         "position":{"x":2560,"y":0},"powered":true,"scale":2.0},
        {"active_workspace":"DP-3:2","enabled":true,"focused":false,
         "logical_size":{"height":1440,"width":2560},"name":"DP-3","modes":[],
         "position":{"x":0,"y":0},"scale":1.0},
        {"active_workspace":null,"enabled":false,"name":"HDMI-A-1",
         "logical_size":{"height":0,"width":0}}
    ]"#;

    fn estado() -> Estado {
        let mut e = Estado {
            ventanas: serde_json::from_str(VENTANAS).unwrap(),
            workspaces: serde_json::from_str(WORKSPACES).unwrap(),
            outputs: serde_json::from_str(SALIDAS).unwrap(),
            ..Default::default()
        };
        e.al_dia();
        e
    }

    #[test]
    fn numeros_por_salida() {
        let e = estado();
        // Por nombre: DP-3, HDMI-A-1, HEADLESS-1.
        assert_eq!(e.numero("DP-3:2"), Some(2));
        assert_eq!(e.numero("HEADLESS-1:1"), Some(201));
        assert_eq!(e.numero("nada:1"), None);
        assert_eq!(e.de_numero(202), Some(("HEADLESS-1".into(), 2)));
        assert_eq!(e.de_numero(2), Some(("DP-3".into(), 2)));
        assert_eq!(e.de_numero(901), None);
    }

    #[test]
    fn espacios_activos_y_urgentes() {
        let e = estado();
        let (espacios, activos, enfocada) = e.leido();
        let ids: Vec<_> = espacios
            .iter()
            .map(|w| (w.id, w.monitor.as_str(), w.nombre.as_str()))
            .collect();
        assert_eq!(
            ids,
            [
                (2, "DP-3", "2"),
                (201, "HEADLESS-1", "1"),
                (202, "HEADLESS-1", "2")
            ]
        );
        // La apagada no figura.
        assert_eq!(
            activos,
            [("HEADLESS-1".to_string(), 201), ("DP-3".to_string(), 2)]
        );
        assert_eq!(enfocada.as_deref(), Some("HEADLESS-1"));
        assert_eq!(e.urgentes(), [202]);
    }

    #[test]
    fn clientes_sin_el_scratchpad_escondido() {
        let e = estado();
        let c = e.clientes();
        let ids: Vec<_> = c
            .iter()
            .map(|c| (c.id.as_str(), c.ws, c.historia))
            .collect();
        assert_eq!(ids, [("ee73", 201, 0), ("e360", 2, 1)]);
        assert!(c[1].juego);
        assert_eq!(
            c[0].rect,
            Rect {
                x: 630,
                y: 10,
                w: 640,
                h: 700
            }
        );
    }

    #[test]
    fn salidas_prendidas_con_su_modo() {
        let s = estado().salidas();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].nombre, "HEADLESS-1");
        assert_eq!((s[0].rect.x, s[0].rect.w, s[0].ancho), (2560, 1280, 2560));
        assert_eq!(s[0].activo, 201);
        assert_eq!(s[1].ancho, 2560);
    }

    #[test]
    fn historia_por_foco() {
        let mut e = estado();
        assert_eq!(e.historia, ["ee73", "e360", "s1"]);
        for v in &mut e.ventanas {
            v.active = v.id == "e360";
        }
        e.al_dia();
        assert_eq!(e.historia, ["e360", "ee73", "s1"]);
        e.ventanas.retain(|v| v.id != "ee73");
        e.al_dia();
        assert_eq!(e.historia, ["e360", "s1"]);
    }

    #[test]
    fn cambios_para_el_dock() {
        let antes: Vec<Ventana> = serde_json::from_str(VENTANAS).unwrap();
        assert!(cambio(&antes, &antes).is_empty());
        let mut titulo = antes.clone();
        titulo[0].title = "vim".into();
        assert_eq!(cambio(&antes, &titulo), [CambioVentanas::Titulo]);
        let mut foco = antes.clone();
        foco[0].active = false;
        foco[1].active = true;
        assert_eq!(cambio(&antes, &foco), [CambioVentanas::Foco("e360".into())]);
        let mut movida = antes.clone();
        movida[0].workspace = "HEADLESS-1:2".into();
        assert_eq!(cambio(&antes, &movida), [CambioVentanas::Lista]);
        assert_eq!(
            cambio(&antes, &antes[..1]),
            [CambioVentanas::Lista, CambioVentanas::Titulo]
        );
        // Ninguna con el foco.
        let mut nada = antes.clone();
        nada[0].active = false;
        assert_eq!(cambio(&antes, &nada), [CambioVentanas::Foco(String::new())]);
    }
}
