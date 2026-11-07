//! La pantalla compartida fuera de Hyprland, por PipeWire: igual para cualquier compositor que
//! comparta por un portal (o por PipeWire directo, como niri). Hyprland tiene su evento
//! `screencast` y sigue con ese.
//!
//! Se comparte cuando una fuente de pantalla tiene un enlace: alguien está tomando sus cuadros.
//! Que la fuente exista no alcanza (OBS deja la suya armada sin grabar, y el portal la crea
//! antes de que el programa se conecte). Todo sale de lo que PipeWire anuncia de cada objeto
//! (`global_keys` de `impl-node.c`, 1.6.10: `media.class`, `media.role`, `node.name`; y de
//! cada enlace, `link.output.node`), sin pedirle nada más: no se ve contenido ni cuadros.
//!
//! Las fuentes de pantalla, según sus fuentes:
//! - el portal de Umbriel: `media.role = Screen` (`src/pipewire/pipewire.cpp`);
//! - el de Hyprland: `xdph-streaming-…` (`src/portals/Screencopy.cpp`);
//! - el de wlroots (sway, river…): `xdpw-stream-…` (`src/screencast/pipewire_screencast.c`);
//! - niri: `niri-screen-cast-src` (`src/screencasting/pw_utils.rs`).
//!
//! Un hilo propio con el loop de PipeWire; si PipeWire se reinicia, se reconecta.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use pipewire as pw;
use pw::types::ObjectType;

const NOMBRES: &[&str] = &[
    "xdph-streaming",       // no-tr
    "xdpw-stream",          // no-tr
    "niri-screen-cast-src", // no-tr
];

/// Una fuente de video que es una pantalla (no una cámara).
pub fn es_pantalla(clase: &str, rol: &str, nombre: &str) -> bool {
    clase.contains("Video")
        && !clase.contains("Audio")
        && (rol == "Screen" || NOMBRES.iter().any(|n| nombre.starts_with(n)))
}

/// Lo visto de PipeWire: qué nodos son pantallas y de qué nodo sale cada enlace.
#[derive(Default)]
pub struct Grafo {
    pantallas: std::collections::HashSet<u32>,
    enlaces: HashMap<u32, u32>,
}

impl Grafo {
    pub fn nodo(&mut self, id: u32, pantalla: bool) {
        if pantalla {
            self.pantallas.insert(id);
        }
    }

    pub fn enlace(&mut self, id: u32, desde: u32) {
        self.enlaces.insert(id, desde);
    }

    pub fn quitar(&mut self, id: u32) {
        self.pantallas.remove(&id);
        self.enlaces.remove(&id);
    }

    pub fn compartiendo(&self) -> bool {
        self.enlaces.values().any(|n| self.pantallas.contains(n))
    }
}

/// Arranca el hilo; `aviso` recibe cada cambio (empieza o termina), nunca dos iguales seguidos.
pub fn escuchar(aviso: async_channel::Sender<bool>) {
    let r = std::thread::Builder::new()
        .name("morgan-pipewire".into()) // no-tr
        .spawn(move || {
            pw::init();
            loop {
                if let Err(e) = conectar(&aviso) {
                    eprintln!("morgan: pipewire: {e}");
                }
                // Desconectado: lo que se compartía, ya no se ve.
                if aviso.is_closed() || aviso.send_blocking(false).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(3));
            }
        });
    if let Err(e) = r {
        eprintln!("morgan: pipewire: {e}");
    }
}

/// Corre hasta que PipeWire se va.
fn conectar(aviso: &async_channel::Sender<bool>) -> Result<(), pw::Error> {
    let bucle = pw::main_loop::MainLoopRc::new(None)?;
    let contexto = pw::context::ContextRc::new(&bucle, None)?;
    let core = contexto.connect_rc(None)?;

    let debil = bucle.downgrade();
    let _core = core
        .add_listener_local()
        .error(move |id, _, _, mensaje| {
            if id == pw::core::PW_ID_CORE {
                eprintln!("morgan: pipewire: {mensaje}");
                if let Some(b) = debil.upgrade() {
                    b.quit();
                }
            }
        })
        .register();

    let grafo = Rc::new(RefCell::new(Grafo::default()));
    let antes = Rc::new(RefCell::new(false));
    let avisar = {
        let (grafo, antes, aviso) = (grafo.clone(), antes.clone(), aviso.clone());
        move || {
            let ahora = grafo.borrow().compartiendo();
            if antes.replace(ahora) != ahora {
                let _ = aviso.send_blocking(ahora);
            }
        }
    };
    let avisar = Rc::new(avisar);

    let registro = core.get_registry_rc()?;
    let _registro = registro
        .add_listener_local()
        .global({
            let (grafo, avisar) = (grafo.clone(), avisar.clone());
            move |g| {
                let Some(p) = g.props else { return };
                match g.type_ {
                    ObjectType::Node => {
                        let pantalla = es_pantalla(
                            p.get("media.class").unwrap_or(""), // no-tr
                            p.get("media.role").unwrap_or(""),  // no-tr
                            p.get("node.name").unwrap_or(""),   // no-tr
                        );
                        grafo.borrow_mut().nodo(g.id, pantalla);
                    }
                    ObjectType::Link => {
                        let Some(desde) = p
                            .get("link.output.node") // no-tr
                            .and_then(|v| v.parse().ok())
                        else {
                            return;
                        };
                        grafo.borrow_mut().enlace(g.id, desde);
                    }
                    _ => return,
                }
                avisar();
            }
        })
        .global_remove({
            let (grafo, avisar) = (grafo.clone(), avisar.clone());
            move |id| {
                grafo.borrow_mut().quitar(id);
                avisar();
            }
        })
        .register();

    bucle.run();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_pantallas_de_cada_portal() {
        assert!(es_pantalla(
            "Video/Source",
            "Screen",
            "umbriel-screen-capture"
        ));
        assert!(es_pantalla("Video/Source", "", "xdph-streaming-42"));
        assert!(es_pantalla("Video/Source", "", "xdpw-stream-AbCdEf"));
        assert!(es_pantalla(
            "Stream/Output/Video",
            "",
            "niri-screen-cast-src"
        ));
        // Una cámara no es una pantalla.
        assert!(!es_pantalla(
            "Video/Source",
            "Camera",
            "v4l2_input.pci-0000"
        ));
        assert!(!es_pantalla("Audio/Source", "Screen", "xdph-streaming-1"));
    }

    #[test]
    fn se_comparte_con_un_enlace_a_la_pantalla() {
        let mut g = Grafo::default();
        g.nodo(10, true);
        g.nodo(11, false);
        // La fuente armada, sin nadie que la tome (OBS sin grabar).
        assert!(!g.compartiendo());
        g.enlace(20, 11);
        assert!(!g.compartiendo());
        g.enlace(21, 10);
        assert!(g.compartiendo());
        g.quitar(21);
        assert!(!g.compartiendo());
        g.enlace(22, 10);
        g.quitar(10);
        assert!(!g.compartiendo());
    }
}
