//! El driver de Hyprland. Los eventos van con gio sobre el main loop de glib (cero sondeo);
//! cada pedido, en un hilo del pool de gio (ver [`pedir`]). El evento solo avisa que algo pudo
//! cambiar: el dato sale de preguntarle a Hyprland. La excepción es `urgent`: Hyprland no
//! guarda esa marca en ningún lado que se pueda consultar, así que la lleva morgan hasta que el
//! workspace se activa.
//!
//! Verificado contra Hyprland 0.56.2: `activewindowv2>>DIRECCIÓN` se reemite en cada cambio de
//! título de la ventana enfocada.

use std::path::{Path, PathBuf};

use gio::prelude::*;
use serde::Deserialize;

use morgan_ipc::hyprland::{dir_from, pedir_ya};

use super::{CambioVentanas, Cliente, Compositor, Espacio, Leido, Rect, Salida};

/// `activewindowv2` aparte: solo cuenta si cambió la dirección.
const EVENTOS: &[&str] = &[
    "movewindowv2",
    "workspacev2",
    "focusedmonv2",
    "openwindow",
    "closewindow",
    "monitoraddedv2",
    "monitorremovedv2",
];

/// `urgent` aparte.
const EVENTOS_ESPACIOS: &[&str] = &[
    "workspacev2",
    "createworkspacev2",
    "destroyworkspacev2",
    "moveworkspacev2",
    "renameworkspace",
    "focusedmonv2",
    "openwindow",
    "closewindow",
    "movewindowv2",
    "monitoraddedv2",
    "monitorremovedv2",
];

#[derive(Clone)]
pub struct Hyprland {
    dir: PathBuf,
}

impl Hyprland {
    pub fn new() -> Option<Self> {
        let Some(dir) = dir_from(|k| std::env::var_os(k)) else {
            eprintln!("morgan: sin HYPRLAND_INSTANCE_SIGNATURE");
            return None;
        };
        Some(Hyprland { dir })
    }

    pub fn arrancar(&self, c: &Compositor) {
        let (h, c) = (self.clone(), c.clone());
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = h.seguir(&c).await {
                eprintln!("morgan: eventos de Hyprland: {e}");
            }
        });
    }

    pub async fn consultar(&self, comando: &str) -> Option<String> {
        match pedir(&self.dir, comando).await {
            Ok(r) => Some(r),
            Err(e) => {
                eprintln!("morgan: hyprctl {comando}: {e}");
                None
            }
        }
    }

    pub async fn eval(&self, lua: &str) {
        if let Some(r) = self.consultar(&format!("eval {lua}")).await
            && !(r.trim() == "ok" || r.trim().is_empty())
        {
            eprintln!("morgan: eval {lua}: {}", r.trim());
        }
    }

    pub async fn clientes(&self) -> Option<Vec<Cliente>> {
        let (c, m) =
            futures_lite::future::zip(self.consultar("j/clients"), self.consultar("j/monitors"))
                .await;
        match leer_clientes(&c?, &m?) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("morgan: j/clients ilegible: {e}");
                None
            }
        }
    }

    pub async fn salidas(&self) -> Option<Vec<Salida>> {
        match leer_salidas(&self.consultar("j/monitors").await?) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("morgan: j/monitors ilegible: {e}");
                None
            }
        }
    }

    pub async fn activa(&self) -> Option<String> {
        let json = self.consultar("j/activewindow").await?;
        let v: serde_json::Value = serde_json::from_str(&json).ok()?;
        v["address"].as_str().map(str::to_owned)
    }

    /// Enfocar una ventana lleva el puntero a su centro. `quieto`: `no_warps` vale solo
    /// durante el pedido y vuelve a lo que el usuario tenga puesto.
    pub async fn enfocar(&self, id: &str, quieto: bool) {
        let foco = format!("hl.dispatch(hl.dsp.focus({{ window = \"address:{id}\" }}))");
        if !quieto {
            return self.eval(&foco).await;
        }
        let antes = self
            .consultar("j/getoption cursor:no_warps")
            .await
            .is_some_and(|o| opcion_activa(&o));
        self.eval(&format!(
            "hl.config({{ cursor = {{ no_warps = true }} }}) {foco} \
             hl.config({{ cursor = {{ no_warps = {antes} }} }})"
        ))
        .await;
    }

    pub async fn ir_a(&self, id: i64) {
        let lua = format!("eval hl.dispatch(hl.dsp.focus({{ workspace = {id} }}))");
        match pedir(&self.dir, &lua).await {
            Ok(r) if r.trim() == "ok" || r.trim().is_empty() => {}
            Ok(r) => eprintln!("morgan: ir al workspace {id}: {}", r.trim()),
            Err(e) => eprintln!("morgan: ir al workspace {id}: {e}"),
        }
    }

    /// Con la config en Lua, `dispatch` recibe una expresión: la sintaxis vieja `dpms off` no
    /// existe.
    pub async fn dpms(&self, prender: bool) {
        let accion = if prender { "enable" } else { "disable" };
        let lua = format!("eval hl.dispatch(hl.dsp.dpms({{ action = \"{accion}\" }}))");
        match pedir(&self.dir, &lua).await {
            Ok(r) if r.trim() == "ok" || r.trim().is_empty() => {}
            Ok(r) => eprintln!("morgan: dpms {accion}: {}", r.trim()),
            Err(e) => eprintln!("morgan: dpms {accion}: {e}"),
        }
    }

    pub async fn submap(&self) -> Option<String> {
        let s = self.consultar("submap").await?;
        let s = s.trim();
        (!s.is_empty() && s != "default").then(|| s.to_owned())
    }

    pub async fn salir_del_submap(&self) {
        self.eval("hl.dispatch(hl.dsp.submap(\"reset\"))").await;
    }

    pub async fn bloq_mayus(&self) -> Option<bool> {
        crate::teclas::bloq_mayus(&self.consultar("j/devices").await?)
    }

    async fn seguir(&self, c: &Compositor) -> Result<(), glib::Error> {
        let dir = &self.dir;
        // Primero la suscripción y después la consulta: al revés, un cambio entre las dos se
        // perdería; así queda encolado en el socket.
        let eventos = conectar(&dir.join(".socket2.sock")).await?;
        let lineas = gio::DataInputStream::new(&eventos.input_stream());
        self.actualizar_juego(c).await;
        self.actualizar_espacios(c).await;
        self.actualizar_salidas(c).await;

        let mut ultima: Option<String> = None;
        while let Some(linea) = lineas.read_line_future(glib::Priority::DEFAULT).await? {
            let linea = String::from_utf8_lossy(&linea);
            c.linea_hyprland(&linea);
            if relevante(&linea, &mut ultima) {
                self.actualizar_juego(c).await;
                if linea.starts_with("activewindowv2>>")
                    && let Some(d) = ultima.as_deref()
                {
                    let d = d.trim().trim_start_matches("0x");
                    let d = if d.is_empty() {
                        String::new()
                    } else {
                        format!("0x{d}")
                    };
                    c.enfocada(&d);
                    c.ventanas_cambiaron(CambioVentanas::Foco(d));
                }
            }
            if let Some(cambio) = de_ventanas(&linea) {
                c.ventanas_cambiaron(cambio);
            }
            if let Some(empieza) = de_screencast(&linea) {
                c.captura(empieza);
            }
            if ["monitoradded", "monitorremoved", "configreloaded"]
                .iter()
                .any(|e| linea.starts_with(e))
            {
                self.actualizar_salidas(c).await;
            }
            match de_espacios(&linea) {
                Some(Evento::Releer) => self.actualizar_espacios(c).await,
                Some(Evento::Urgente(direccion)) => self.marcar_urgente(c, direccion).await,
                None => {}
            }
        }
        eprintln!("morgan: Hyprland cerró el socket de eventos");
        Ok(())
    }

    async fn actualizar_juego(&self, c: &Compositor) {
        match pedir(&self.dir, "j/activewindow").await {
            Ok(json) => match en_juego(&json, |p| super::lanzado_por_steam(p, super::leer_proc)) {
                Ok(v) => c.poner_juego(v),
                Err(e) => eprintln!("morgan: j/activewindow ilegible: {e}"),
            },
            Err(e) => eprintln!("morgan: j/activewindow: {e}"),
        }
    }

    async fn actualizar_salidas(&self, c: &Compositor) {
        if let Some(s) = self.salidas().await {
            c.salidas_cambiaron(s.into_iter().map(|s| s.nombre).collect());
        }
    }

    async fn actualizar_espacios(&self, c: &Compositor) {
        let dir = &self.dir;
        let leido = async {
            let (w_res, m_res) =
                futures_lite::future::zip(pedir(dir, "j/workspaces"), pedir(dir, "j/monitors"))
                    .await;
            let w = w_res.map_err(|e| e.to_string())?;
            let m = m_res.map_err(|e| e.to_string())?;
            leer_espacios(&w, &m).map_err(|e| format!("ilegible: {e}"))
        };
        match leido.await {
            Ok(leido) => c.releer_espacios(leido, false),
            Err(e) => eprintln!("morgan: workspaces de Hyprland: {e}"),
        }
    }

    async fn marcar_urgente(&self, c: &Compositor, direccion: String) {
        let id = match pedir(&self.dir, "j/clients").await {
            Ok(json) => match espacio_de(&json, &direccion) {
                Ok(Some(v)) => v,
                Ok(None) => return,
                Err(e) => return eprintln!("morgan: j/clients ilegible: {e}"),
            },
            Err(e) => return eprintln!("morgan: j/clients: {e}"),
        };
        c.marcar_urgente(id);
    }
}

async fn conectar(path: &Path) -> Result<gio::SocketConnection, glib::Error> {
    gio::SocketClient::new()
        .connect_future(&gio::UnixSocketAddress::new(path))
        .await
}

/// Conectar, escribir y leer van juntos en otro hilo. Hyprland, después de aceptar, traba su
/// loop hasta 5 s esperando el pedido (`HyprCtl.cpp`, `poll(…, 5000)`, v0.56.2): si entre el
/// connect y el write el hilo principal espera a Hyprland por Wayland (la ida y vuelta de
/// layer-shell al mapear una capa), cada uno espera al otro hasta que vence, con el escritorio
/// congelado.
pub async fn pedir(dir: &Path, comando: &str) -> Result<String, glib::Error> {
    let ruta = dir.join(".socket.sock");
    let comando = comando.to_owned();
    gio::spawn_blocking(move || pedir_ya(&ruta, &comando))
        .await
        .unwrap_or_else(|_| Err(std::io::Error::other("el hilo del pedido entró en pánico")))
        .map_err(|e| glib::Error::new(gio::IOErrorEnum::Failed, &e.to_string()))
}

/// `ultima` descarta los `activewindowv2` de cambio de título.
pub fn relevante(linea: &str, ultima: &mut Option<String>) -> bool {
    let Some((nombre, datos)) = linea.split_once(">>") else {
        return false;
    };
    if nombre == "activewindowv2" {
        if ultima.as_deref() == Some(datos) {
            return false;
        }
        *ultima = Some(datos.to_owned());
        return true;
    }
    EVENTOS.contains(&nombre)
}

/// Lo que le importa al dock, salvo el foco (sale de [`relevante`], sin los repetidos).
pub fn de_ventanas(linea: &str) -> Option<CambioVentanas> {
    match linea.split_once(">>")?.0 {
        "openwindow" | "closewindow" | "movewindowv2" => Some(CambioVentanas::Lista),
        "windowtitlev2" => Some(CambioVentanas::Titulo),
        _ => None,
    }
}

pub fn de_screencast(linea: &str) -> Option<bool> {
    let datos = linea.strip_prefix("screencast>>")?;
    match datos.split(',').next()? {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Evento {
    Releer,
    Urgente(String),
}

pub fn de_espacios(linea: &str) -> Option<Evento> {
    let (nombre, datos) = linea.split_once(">>")?;
    if nombre == "urgent" {
        return Some(Evento::Urgente(datos.trim_start_matches("0x").to_owned()));
    }
    EVENTOS_ESPACIOS.contains(&nombre).then_some(Evento::Releer)
}

pub fn leer_espacios(workspaces: &str, monitores: &str) -> Result<Leido, serde_json::Error> {
    #[derive(Deserialize)]
    struct W {
        id: i64,
        #[serde(default)]
        name: String,
        monitor: String,
        windows: u32,
    }
    #[derive(Deserialize)]
    struct M {
        name: String,
        #[serde(rename = "activeWorkspace")]
        activo: Workspace,
        #[serde(default)]
        disabled: bool,
        #[serde(default)]
        focused: bool,
    }
    #[derive(Deserialize)]
    struct Workspace {
        id: i64,
    }
    let mut espacios: Vec<Espacio> = serde_json::from_str::<Vec<W>>(workspaces)?
        .into_iter()
        .filter(|w| w.id > 0)
        .map(|w| Espacio {
            id: w.id,
            monitor: w.monitor,
            ventanas: w.windows,
            nombre: if w.name.is_empty() {
                w.id.to_string()
            } else {
                w.name
            },
        })
        .collect();
    espacios.sort_by_key(|e| e.id);
    let monitores: Vec<M> = serde_json::from_str::<Vec<M>>(monitores)?
        .into_iter()
        .filter(|m| !m.disabled)
        .collect();
    let enfocada = monitores.iter().find(|m| m.focused).map(|m| m.name.clone());
    let activos = monitores
        .into_iter()
        .map(|m| (m.name, m.activo.id))
        .collect();
    Ok((espacios, activos, enfocada))
}

#[derive(Deserialize)]
struct Monitor {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
    width: i32,
    height: i32,
    #[serde(default = "uno")]
    scale: f64,
    #[serde(default)]
    transform: i32,
    #[serde(default)]
    focused: bool,
    #[serde(default)]
    disabled: bool,
    #[serde(rename = "activeWorkspace")]
    activo: Option<IdWs>,
}

#[derive(Deserialize)]
struct IdWs {
    id: i64,
}

fn uno() -> f64 {
    1.0
}

impl Monitor {
    /// El tamaño lógico: los píxeles sobre la escala, girados si el monitor lo está.
    fn salida(&self) -> Salida {
        let (mut w, mut h) = (
            (f64::from(self.width) / self.scale).round() as i32,
            (f64::from(self.height) / self.scale).round() as i32,
        );
        if self.transform % 2 == 1 {
            std::mem::swap(&mut w, &mut h);
        }
        Salida {
            nombre: self.name.clone(),
            rect: Rect {
                x: self.x,
                y: self.y,
                w,
                h,
            },
            escala: self.scale,
            ancho: self.width,
            alto: self.height,
            enfocada: self.focused,
            activo: self.activo.as_ref().map_or(0, |w| w.id),
        }
    }
}

/// Las prendidas.
pub fn leer_salidas(monitores: &str) -> Result<Vec<Salida>, serde_json::Error> {
    Ok(serde_json::from_str::<Vec<Monitor>>(monitores)?
        .iter()
        .filter(|m| !m.disabled)
        .map(Monitor::salida)
        .collect())
}

/// Las de `j/clients` que existen de verdad: mapeadas, a la vista y en un workspace.
pub fn leer_clientes(clientes: &str, monitores: &str) -> Result<Vec<Cliente>, serde_json::Error> {
    #[derive(Deserialize)]
    struct C {
        address: String,
        mapped: bool,
        hidden: bool,
        at: [i32; 2],
        size: [i32; 2],
        workspace: IdWs,
        monitor: i64,
        class: String,
        title: String,
        #[serde(rename = "focusHistoryID")]
        historia: i32,
        #[serde(default, rename = "contentType")]
        contenido: String,
    }
    let monitores: Vec<Monitor> = serde_json::from_str(monitores)?;
    Ok(serde_json::from_str::<Vec<C>>(clientes)?
        .into_iter()
        .filter(|c| c.mapped && !c.hidden && c.workspace.id != -1)
        .map(|c| Cliente {
            juego: super::es_juego(&c.class, &c.contenido, None, |_| false),
            salida: monitores
                .iter()
                .find(|m| m.id == c.monitor)
                .map(|m| m.name.clone())
                .unwrap_or_default(),
            id: c.address,
            clase: c.class,
            titulo: c.title,
            ws: c.workspace.id,
            rect: Rect {
                x: c.at[0],
                y: c.at[1],
                w: c.size[0],
                h: c.size[1],
            },
            historia: c.historia,
        })
        .collect())
}

pub fn espacio_de(clientes: &str, direccion: &str) -> Result<Option<i64>, serde_json::Error> {
    #[derive(Deserialize)]
    struct C {
        address: String,
        workspace: IdWs,
    }
    Ok(serde_json::from_str::<Vec<C>>(clientes)?
        .into_iter()
        .find(|c| c.address.trim_start_matches("0x") == direccion)
        .filter(|c| c.workspace.id > 0)
        .map(|c| c.workspace.id))
}

/// `contentType` lo declara la app (`content-type-v1`) o una regla de Hyprland
/// (`content = "game"`). Sin ventana enfocada, Hyprland contesta `{}`.
pub fn en_juego(json: &str, de_steam: impl Fn(u32) -> bool) -> Result<bool, serde_json::Error> {
    #[derive(Deserialize)]
    struct Ventana {
        #[serde(default)]
        class: String,
        #[serde(default, rename = "contentType")]
        contenido: String,
        pid: Option<i64>,
    }
    let v: Ventana = serde_json::from_str(json)?;
    Ok(super::es_juego(&v.class, &v.contenido, v.pid, de_steam))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn juego_por_la_ventana_y_no_por_el_workspace() {
        let ventana = |clase: &str, contenido: &str| {
            format!(
                r#"{{"address":"0x1","class":"{clase}","contentType":"{contenido}","workspace":{{"id":10,"name":"10"}},"fullscreen":2}}"#
            )
        };
        let nunca = |_| false;
        assert!(!en_juego(&ventana("kitty", "none"), nunca).unwrap());
        assert!(en_juego(&ventana("steam_app_default", "none"), nunca).unwrap());
        assert!(en_juego(&ventana("gamescope", "none"), nunca).unwrap());
        assert!(en_juego(&ventana("lutris-juego", "game"), nunca).unwrap());
        assert!(!en_juego(&ventana("mpv", "video"), nunca).unwrap());
        // Sin ventana enfocada.
        assert!(!en_juego("{}", nunca).unwrap());
        assert!(en_juego("ok", nunca).is_err());
    }

    #[test]
    fn juego_es_lo_que_lanzo_steam_en_cualquier_workspace() {
        let ventana =
            |pid| format!(r#"{{"address":"0x1","workspace":{{"id":1,"name":"1"}},"pid":{pid}}}"#);
        let steam = |p| p == 4242;
        assert!(en_juego(&ventana(4242), steam).unwrap());
        assert!(!en_juego(&ventana(77), steam).unwrap());
        // Sin proceso conocido (Hyprland da -1) no se pregunta.
        assert!(!en_juego(&ventana(-1), |_| true).unwrap());
    }

    #[test]
    fn el_cambio_de_titulo_no_cuenta() {
        let mut ultima = None;
        assert!(relevante("activewindowv2>>5572a90e2070", &mut ultima));
        assert!(!relevante("activewindowv2>>5572a90e2070", &mut ultima));
        assert!(!relevante("windowtitlev2>>5572a90e2070,fish", &mut ultima));
        assert!(!relevante("activewindow>>kitty,fish", &mut ultima));
        assert!(relevante("activewindowv2>>99", &mut ultima));
        // Sin ventana enfocada la dirección viene vacía: también es un cambio.
        assert!(relevante("activewindowv2>>", &mut ultima));
    }

    #[test]
    fn lo_que_mira_el_dock() {
        assert_eq!(
            de_ventanas("openwindow>>5572a90e2070,1,kitty,kitty"),
            Some(CambioVentanas::Lista)
        );
        assert_eq!(de_ventanas("closewindow>>55"), Some(CambioVentanas::Lista));
        assert_eq!(
            de_ventanas("movewindowv2>>55,2,2"),
            Some(CambioVentanas::Lista)
        );
        assert_eq!(
            de_ventanas("windowtitlev2>>55,fish"),
            Some(CambioVentanas::Titulo)
        );
        assert_eq!(de_ventanas("activewindowv2>>55"), None);
        assert_eq!(de_ventanas("workspacev2>>2,2"), None);
        assert_eq!(de_ventanas("basura"), None);
    }

    #[test]
    fn eventos_de_los_workspaces() {
        assert_eq!(de_espacios("workspacev2>>2,2"), Some(Evento::Releer));
        assert_eq!(de_espacios("createworkspacev2>>4,4"), Some(Evento::Releer));
        assert_eq!(
            de_espacios("closewindow>>5572a90e2070"),
            Some(Evento::Releer)
        );
        assert_eq!(
            de_espacios("urgent>>5572a90e2070"),
            Some(Evento::Urgente("5572a90e2070".into()))
        );
        assert_eq!(de_espacios("activewindowv2>>5572a90e2070"), None);
        assert_eq!(de_espacios("windowtitlev2>>1,fish"), None);
        assert_eq!(de_espacios("basura"), None);
    }

    #[test]
    fn capturas_de_pantalla() {
        assert_eq!(de_screencast("screencast>>1,0"), Some(true));
        assert_eq!(de_screencast("screencast>>0,1"), Some(false));
        assert_eq!(de_screencast("screencastv2>>1,0,obs"), None);
        assert_eq!(de_screencast("screencast>>x"), None);
    }

    #[test]
    fn lectura_de_workspaces_y_salidas() {
        let w = r#"[
            {"id":3,"name":"3","monitor":"DP-3","monitorID":2,"windows":0},
            {"id":1,"name":"1","monitor":"DP-3","monitorID":2,"windows":8},
            {"id":-98,"name":"special:magic","monitor":"DP-3","windows":1},
            {"id":4,"name":"4","monitor":"HDMI-A-1","monitorID":3,"windows":2}
        ]"#;
        let m = r#"[
            {"name":"DP-3","activeWorkspace":{"id":1,"name":"1"},"disabled":false,"focused":true},
            {"name":"HDMI-A-1","activeWorkspace":{"id":4,"name":"4"},"disabled":true}
        ]"#;
        let (espacios, activos, enfocada) = leer_espacios(w, m).unwrap();
        assert_eq!(enfocada.as_deref(), Some("DP-3"));
        let ids: Vec<_> = espacios.iter().map(|e| (e.id, e.ventanas)).collect();
        assert_eq!(ids, [(1, 8), (3, 0), (4, 2)]);
        assert_eq!(espacios[0].nombre, "1");
        assert_eq!(activos, [("DP-3".to_string(), 1)]);
        assert!(leer_espacios("ok", m).is_err());
    }

    #[test]
    fn salidas_logicas_y_las_apagadas_no() {
        let m = r#"[
            {"id":0,"name":"DP-3","x":0,"y":0,"width":3840,"height":2160,"scale":2.0,
             "transform":1,"focused":true,"activeWorkspace":{"id":2}},
            {"id":1,"name":"HDMI-A-1","x":1920,"y":0,"width":1920,"height":1080,"scale":1.0,
             "disabled":true,"activeWorkspace":{"id":4}}
        ]"#;
        let s = leer_salidas(m).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].rect.w, s[0].rect.h), (1080, 1920));
        assert_eq!((s[0].ancho, s[0].alto, s[0].activo), (3840, 2160, 2));
        assert!(s[0].enfocada);
    }

    #[test]
    fn clientes_con_su_salida() {
        let m = r#"[{"id":0,"name":"DP-1","width":1920,"height":1080},
                    {"id":1,"name":"DP-2","width":1920,"height":1080}]"#;
        let c = r#"[
            {"address":"0xa","mapped":true,"hidden":false,"at":[1940,20],"size":[900,1000],
             "workspace":{"id":5},"monitor":1,"class":"kitty","title":"t","focusHistoryID":0},
            {"address":"0xb","mapped":true,"hidden":false,"at":[0,0],"size":[9,9],
             "workspace":{"id":-98},"monitor":0,"class":"x","title":"t","focusHistoryID":1,
             "contentType":"game"},
            {"address":"0xc","mapped":false,"hidden":false,"at":[0,0],"size":[9,9],
             "workspace":{"id":1},"monitor":0,"class":"x","title":"t","focusHistoryID":2},
            {"address":"0xd","mapped":true,"hidden":false,"at":[0,0],"size":[9,9],
             "workspace":{"id":-1},"monitor":0,"class":"x","title":"t","focusHistoryID":3}
        ]"#;
        let v = leer_clientes(c, m).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].id.as_str(), v[0].salida.as_str()), ("0xa", "DP-2"));
        assert_eq!(v[0].rect.x, 1940);
        assert!(!v[0].juego);
        // El scratchpad sí (el dock lo muestra); el lienzo lo filtra.
        assert_eq!(v[1].ws, -98);
        assert!(v[1].juego);
    }

    #[test]
    fn workspace_de_una_ventana() {
        let c = r#"[{"address":"0x55aa","workspace":{"id":10,"name":"10"},"pid":4242},
                    {"address":"0x66cc","workspace":{"id":1,"name":"1"},"pid":-1},
                    {"address":"0x77bb","workspace":{"id":-98,"name":"special:magic"}}]"#;
        assert_eq!(espacio_de(c, "55aa").unwrap(), Some(10));
        assert_eq!(espacio_de(c, "66cc").unwrap(), Some(1));
        assert_eq!(espacio_de(c, "77bb").unwrap(), None);
        assert_eq!(espacio_de(c, "99").unwrap(), None);
    }

    #[test]
    fn opcion_de_getoption() {
        assert!(opcion_activa(r#"{"option":"x","bool":true,"set":true}"#));
        assert!(!opcion_activa(r#"{"option":"x","bool":false,"set":false}"#));
        assert!(!opcion_activa("no es json"));
    }

    #[test]
    fn eventos_que_mueven_el_foco() {
        let mut ultima = None;
        for l in [
            "movewindowv2>>5572a90e2070,10,10",
            "workspacev2>>10,10",
            "focusedmonv2>>DP-3,10",
            "openwindow>>5572a90e2070,1,kitty,kitty",
            "closewindow>>5572a90e2070",
            "monitorremovedv2>>2,HDMI-A-1,TV",
        ] {
            assert!(relevante(l, &mut ultima), "{l}");
        }
        for l in [
            "createworkspacev2>>10,10",
            "urgent>>5572a90e2070",
            "basura",
            "",
        ] {
            assert!(!relevante(l, &mut ultima), "{l}");
        }
    }
}
