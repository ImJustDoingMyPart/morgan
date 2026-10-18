//! Los eventos van con gio sobre el main loop de glib (cero sondeo); cada pedido, en un hilo
//! del pool de gio (ver [`pedir`]). El evento solo avisa que algo pudo cambiar: el dato sale de
//! preguntarle a Hyprland. La excepción es `urgent`: Hyprland no guarda esa marca en ningún
//! lado que se pueda consultar, así que la lleva morgan hasta que el workspace se activa.
//!
//! Verificado contra Hyprland 0.56.2: `activewindowv2>>DIRECCIÓN` se reemite en cada cambio de
//! título de la ventana enfocada.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use serde::Deserialize;

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

type Oyentes = Rc<RefCell<Vec<Box<dyn Fn(bool)>>>>;
pub type Activos = Vec<(String, i64)>;
pub type Leido = (Vec<Espacio>, Activos, Option<String>);
type OyentesEspacios = Rc<RefCell<Vec<Box<dyn Fn(&Espacios)>>>>;
type OyentesFoco = Rc<RefCell<Vec<Box<dyn Fn(&str)>>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Espacio {
    pub id: i64,
    pub monitor: String,
    pub ventanas: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Espacios {
    pub espacios: Vec<Espacio>,
    pub activos: Activos,
    pub enfocada: Option<String>,
    pub urgentes: BTreeSet<i64>,
}

impl Espacios {
    pub fn activo(&self, id: i64) -> bool {
        self.activos.iter().any(|(_, a)| *a == id)
    }

    /// `j/monitors` no lista las apagadas.
    pub fn prendida(&self, salida: &str) -> bool {
        !salida.is_empty() && self.activos.iter().any(|(m, _)| m == salida)
    }

    fn releido(&mut self, (espacios, activos, enfocada): Leido) {
        self.espacios = espacios;
        self.activos = activos;
        self.enfocada = enfocada;
        let urgentes = std::mem::take(&mut self.urgentes);
        self.urgentes = urgentes
            .into_iter()
            .filter(|id| !self.activo(*id) && self.espacios.iter().any(|e| e.id == *id))
            .collect();
    }

    fn urgente(&mut self, id: i64) -> bool {
        !self.activo(id) && self.urgentes.insert(id)
    }
}

#[derive(Clone, Default)]
pub struct Hyprland {
    dir: Option<PathBuf>,
    juego: Rc<Cell<bool>>,
    oyentes: Oyentes,
    espacios: Rc<RefCell<Espacios>>,
    oyentes_espacios: OyentesEspacios,
    capturas: Rc<Cell<u32>>,
    oyentes_compartir: Oyentes,
    oyentes_foco: OyentesFoco,
    oyentes_evento: OyentesFoco,
}

impl Hyprland {
    /// Fuera de Hyprland avisa una vez y queda como "sin juego": el OSD se comporta como siempre.
    pub fn new() -> Self {
        let Some(dir) = dir_from(|k| std::env::var_os(k)) else {
            eprintln!("morgan: sin HYPRLAND_INSTANCE_SIGNATURE; modo juego apagado");
            return Hyprland::default();
        };
        let hyprland = Hyprland {
            dir: Some(dir),
            ..Default::default()
        };
        let h = hyprland.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = h.seguir().await {
                eprintln!("morgan: eventos de Hyprland: {e}");
            }
        });
        hyprland
    }

    pub fn juego_en_foco(&self) -> bool {
        self.juego.get()
    }

    pub fn al_cambiar(&self, f: impl Fn(bool) + 'static) {
        self.oyentes.borrow_mut().push(Box::new(f));
    }

    pub fn espacios(&self) -> Espacios {
        self.espacios.borrow().clone()
    }

    pub fn al_cambiar_espacios(&self, f: impl Fn(&Espacios) + 'static) {
        self.oyentes_espacios.borrow_mut().push(Box::new(f));
    }

    pub fn al_compartir(&self, f: impl Fn(bool) + 'static) {
        self.oyentes_compartir.borrow_mut().push(Box::new(f));
    }

    pub fn al_enfocar(&self, f: impl Fn(&str) + 'static) {
        self.oyentes_foco.borrow_mut().push(Box::new(f));
    }

    pub fn al_evento(&self, f: impl Fn(&str) + 'static) {
        self.oyentes_evento.borrow_mut().push(Box::new(f));
    }

    pub async fn consultar(&self, comando: &str) -> Option<String> {
        let dir = self.dir.clone()?;
        match pedir(&dir, comando).await {
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

    fn captura(&self, empieza: bool) {
        let antes = self.capturas.get();
        let ahora = if empieza {
            antes + 1
        } else {
            antes.saturating_sub(1)
        };
        self.capturas.set(ahora);
        if (antes > 0) != (ahora > 0) {
            for f in self.oyentes_compartir.borrow().iter() {
                f(ahora > 0);
            }
        }
    }

    /// Lo que se marca después sale del evento que llegue, no de este pedido.
    pub fn ir_a(&self, id: i64) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        glib::MainContext::default().spawn_local(async move {
            let lua = format!("eval hl.dispatch(hl.dsp.focus({{ workspace = {id} }}))");
            match pedir(&dir, &lua).await {
                Ok(r) if r.trim() == "ok" || r.trim().is_empty() => {}
                Ok(r) => eprintln!("morgan: ir al workspace {id}: {}", r.trim()),
                Err(e) => eprintln!("morgan: ir al workspace {id}: {e}"),
            }
        });
    }

    /// Con la config en Lua, `dispatch` recibe una expresión: la sintaxis vieja `dpms off` no
    /// existe.
    pub fn dpms(&self, prender: bool) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let accion = if prender { "enable" } else { "disable" };
        glib::MainContext::default().spawn_local(async move {
            let lua = format!("eval hl.dispatch(hl.dsp.dpms({{ action = \"{accion}\" }}))");
            match pedir(&dir, &lua).await {
                Ok(r) if r.trim() == "ok" || r.trim().is_empty() => {}
                Ok(r) => eprintln!("morgan: dpms {accion}: {}", r.trim()),
                Err(e) => eprintln!("morgan: dpms {accion}: {e}"),
            }
        });
    }

    async fn seguir(&self) -> Result<(), glib::Error> {
        let dir = self.dir.as_deref().expect("seguir sin socket");
        // Primero la suscripción y después la consulta: al revés, un cambio entre las dos se
        // perdería; así queda encolado en el socket.
        let eventos = conectar(&dir.join(".socket2.sock")).await?;
        let lineas = gio::DataInputStream::new(&eventos.input_stream());
        self.actualizar_juego(dir).await;
        self.actualizar_espacios(dir).await;

        let mut ultima: Option<String> = None;
        while let Some(linea) = lineas.read_line_future(glib::Priority::DEFAULT).await? {
            let linea = String::from_utf8_lossy(&linea);
            for f in self.oyentes_evento.borrow().iter() {
                f(&linea);
            }
            if relevante(&linea, &mut ultima) {
                self.actualizar_juego(dir).await;
                if linea.starts_with("activewindowv2>>")
                    && let Some(d) = ultima.as_deref()
                {
                    let d = format!("0x{}", d.trim().trim_start_matches("0x"));
                    for f in self.oyentes_foco.borrow().iter() {
                        f(&d);
                    }
                }
            }
            if let Some(empieza) = de_screencast(&linea) {
                self.captura(empieza);
            }
            match de_espacios(&linea) {
                Some(Evento::Releer) => self.actualizar_espacios(dir).await,
                Some(Evento::Urgente(direccion)) => self.marcar_urgente(dir, direccion).await,
                None => {}
            }
        }
        eprintln!("morgan: Hyprland cerró el socket de eventos");
        Ok(())
    }

    async fn actualizar_juego(&self, dir: &Path) {
        match pedir(dir, "j/activewindow").await {
            Ok(json) => match en_juego(&json, |p| lanzado_por_steam(p, leer_proc)) {
                Ok(v) => {
                    if self.juego.replace(v) != v {
                        for f in self.oyentes.borrow().iter() {
                            f(v);
                        }
                    }
                }
                Err(e) => eprintln!("morgan: j/activewindow ilegible: {e}"),
            },
            Err(e) => eprintln!("morgan: j/activewindow: {e}"),
        }
    }

    async fn actualizar_espacios(&self, dir: &Path) {
        let leido = async {
            let (w_res, m_res) = futures_lite::future::zip(
                pedir(dir, "j/workspaces"),
                pedir(dir, "j/monitors"),
            )
            .await;
            let w = w_res.map_err(|e| e.to_string())?;
            let m = m_res.map_err(|e| e.to_string())?;
            leer_espacios(&w, &m).map_err(|e| format!("ilegible: {e}"))
        };
        match leido.await {
            Ok(leido) => {
                let mut nuevo = self.espacios.borrow().clone();
                nuevo.releido(leido);
                self.publicar(nuevo);
            }
            Err(e) => eprintln!("morgan: workspaces de Hyprland: {e}"),
        }
    }

    async fn marcar_urgente(&self, dir: &Path, direccion: String) {
        let id = match pedir(dir, "j/clients").await {
            Ok(json) => match espacio_de(&json, &direccion) {
                Ok(Some(v)) => v,
                Ok(None) => return,
                Err(e) => return eprintln!("morgan: j/clients ilegible: {e}"),
            },
            Err(e) => return eprintln!("morgan: j/clients: {e}"),
        };
        let mut nuevo = self.espacios.borrow().clone();
        if nuevo.urgente(id) {
            self.publicar(nuevo);
        }
    }

    fn publicar(&self, nuevo: Espacios) {
        if *self.espacios.borrow() == nuevo {
            return;
        }
        self.espacios.replace(nuevo.clone());
        for f in self.oyentes_espacios.borrow().iter() {
            f(&nuevo);
        }
    }
}

pub fn dir_from(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let runtime = env("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())?;
    let firma = env("HYPRLAND_INSTANCE_SIGNATURE").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(runtime).join("hypr").join(firma))
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

fn pedir_ya(ruta: &Path, comando: &str) -> std::io::Result<String> {
    use std::io::{Read, Write};
    let mut s = std::os::unix::net::UnixStream::connect(ruta)?;
    s.write_all(comando.as_bytes())?;
    // Tope por si no contesta: el hilo no queda colgado para siempre.
    s.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    let mut respuesta = Vec::new();
    s.read_to_end(&mut respuesta)?;
    Ok(String::from_utf8_lossy(&respuesta).into_owned())
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

pub fn espacio_de(clientes: &str, direccion: &str) -> Result<Option<i64>, serde_json::Error> {
    #[derive(Deserialize)]
    struct C {
        address: String,
        workspace: Workspace,
    }
    #[derive(Deserialize)]
    struct Workspace {
        id: i64,
    }
    Ok(serde_json::from_str::<Vec<C>>(clientes)?
        .into_iter()
        .find(|c| c.address.trim_start_matches("0x") == direccion)
        .filter(|c| c.workspace.id > 0)
        .map(|c| c.workspace.id))
}

/// Por la ventana y no por dónde está: un workspace fijo para los juegos es una convención de
/// cada config, y una ventana cualquiera movida ahí no es un juego. `contentType` lo declara
/// la app (`content-type-v1`) o una regla de Hyprland (`content = "game"`), para lo que no
/// lanza Steam. Sin ventana enfocada, Hyprland contesta `{}`.
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
    Ok(v.contenido == "game"
        || v.class == "gamescope"
        || v.class.starts_with("steam_app_")
        || v.pid
            .and_then(|p| u32::try_from(p).ok())
            .is_some_and(|p| p > 1 && de_steam(p)))
}

/// Lo que Steam antepone a todo juego que lanza, nativo o por Proton.
const MARCA_STEAM: &[u8] = b"SteamLaunch\0AppId=";

/// El `reaper` es subreaper: lo que se desprende de un lanzador (wine, un script) igual queda
/// colgando de él.
pub fn lanzado_por_steam(pid: u32, leer: impl Fn(u32) -> Option<(u32, Vec<u8>)>) -> bool {
    let mut actual = pid;
    // Tope por si `/proc` cambia a mitad de camino y arma un ciclo.
    for _ in 0..64 {
        let Some((padre, cmdline)) = leer(actual) else {
            return false;
        };
        if cmdline.windows(MARCA_STEAM.len()).any(|w| w == MARCA_STEAM) {
            return true;
        }
        if padre <= 1 {
            return false;
        }
        actual = padre;
    }
    false
}

pub fn leer_proc(pid: u32) -> Option<(u32, Vec<u8>)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // El nombre (campo 2) va entre paréntesis y puede tener espacios: se corta en el último.
    let padre = stat
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some((padre, cmdline))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_pedido_llega_entero_sin_el_main_loop() {
        let dir = std::env::temp_dir().join(format!("morgan-pedir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join(".socket.sock");
        let escucha = std::os::unix::net::UnixListener::bind(&ruta).unwrap();
        // Como Hyprland: lee el pedido apenas acepta, contesta y cierra.
        let hypr = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut c, _) = escucha.accept().unwrap();
            let mut b = [0u8; 64];
            let n = c.read(&mut b).unwrap();
            c.write_all(b"ok").unwrap();
            String::from_utf8_lossy(&b[..n]).into_owned()
        });
        assert_eq!(pedir_ya(&ruta, "j/monitors").unwrap(), "ok");
        assert_eq!(hypr.join().unwrap(), "j/monitors");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dir_de_la_instancia() {
        let env = |k: &str| match k {
            "XDG_RUNTIME_DIR" => Some("/run/user/1000".into()),
            "HYPRLAND_INSTANCE_SIGNATURE" => Some("abc_1_2".into()),
            _ => None,
        };
        assert_eq!(
            dir_from(env),
            Some(PathBuf::from("/run/user/1000/hypr/abc_1_2"))
        );
        assert_eq!(dir_from(|_| None), None);
        assert_eq!(
            dir_from(|k| (k == "XDG_RUNTIME_DIR").then(|| "/run".into())),
            None
        );
    }

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
    fn el_reaper_de_steam_en_la_ascendencia() {
        // 900 game.exe ← 800 wine ← 700 reaper SteamLaunch ← 600 steam ← 1
        let arbol = |pid| -> Option<(u32, Vec<u8>)> {
            Some(match pid {
                900 => (800, b"Z:\\juego\\game.exe\0".to_vec()),
                800 => (700, b"wine64\0".to_vec()),
                700 => (
                    600,
                    b"/home/x/.steam/ubuntu12_32/reaper\0SteamLaunch\0AppId=730\0--\0".to_vec(),
                ),
                600 => (1, b"steam\0".to_vec()),
                50 => (1, b"kitty\0".to_vec()),
                _ => return None,
            })
        };
        assert!(lanzado_por_steam(900, arbol));
        assert!(lanzado_por_steam(700, arbol));
        // Steam mismo no es un juego, ni lo que no cuelga del reaper.
        assert!(!lanzado_por_steam(600, arbol));
        assert!(!lanzado_por_steam(50, arbol));
        // Un proceso que ya no existe.
        assert!(!lanzado_por_steam(12345, arbol));
        // Un ciclo no cuelga.
        assert!(!lanzado_por_steam(5, |p| Some((p, b"x\0".to_vec()))));
    }

    #[test]
    fn el_padre_sale_de_stat() {
        let yo = std::process::id();
        let (padre, cmdline) = leer_proc(yo).unwrap();
        assert_eq!(padre, std::os::unix::process::parent_id());
        assert!(!cmdline.is_empty());
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
        let h = Hyprland::default();
        let visto = Rc::new(RefCell::new(Vec::new()));
        let v = visto.clone();
        h.al_compartir(move |b| v.borrow_mut().push(b));
        // Dos capturas: termina una y el indicador sigue; termina la otra y se apaga.
        h.captura(true);
        h.captura(true);
        h.captura(false);
        h.captura(false);
        h.captura(false);
        assert_eq!(*visto.borrow(), [true, false]);
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
        assert_eq!(activos, [("DP-3".to_string(), 1)]);
        assert!(leer_espacios("ok", m).is_err());
    }

    #[test]
    fn tele_prendida_si_la_salida_esta() {
        let e = Espacios {
            activos: vec![("DP-3".into(), 1), ("HDMI-A-1".into(), 4)],
            ..Default::default()
        };
        assert!(e.prendida("HDMI-A-1"));
        assert!(!e.prendida("DP-1"));
        // Sin TV configurada no hay nada prendido, aunque haya una salida sin nombre.
        assert!(!e.prendida(""));
        assert!(!Espacios::default().prendida("HDMI-A-1"));
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
    fn urgente_hasta_que_se_mira() {
        let espacio = |id| Espacio {
            id,
            monitor: "DP-3".into(),
            ventanas: 1,
        };
        let mut e = Espacios::default();
        e.releido((
            vec![espacio(1), espacio(10)],
            vec![("DP-3".into(), 1)],
            None,
        ));
        // El que se está viendo no se marca.
        assert!(!e.urgente(1));
        assert!(e.urgente(10));
        assert!(!e.urgente(10));
        // Sigue marcado mientras no se active.
        e.releido((
            vec![espacio(1), espacio(10)],
            vec![("DP-3".into(), 1)],
            None,
        ));
        assert!(e.urgentes.contains(&10));
        e.releido((
            vec![espacio(1), espacio(10)],
            vec![("DP-3".into(), 10)],
            None,
        ));
        assert!(e.urgentes.is_empty());
        // Un workspace que desaparece se lleva su marca.
        e.releido((
            vec![espacio(1), espacio(10)],
            vec![("DP-3".into(), 1)],
            None,
        ));
        assert!(e.urgente(10));
        e.releido((vec![espacio(1)], vec![("DP-3".into(), 1)], None));
        assert!(e.urgentes.is_empty());
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
