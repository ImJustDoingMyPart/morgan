//! Lo que morgan sabe del compositor y le pide, igual para todos: workspaces por salida,
//! ventanas, la ventana con el foco, salidas, modo juego, ir a un workspace, enfocar una
//! ventana, apagar las pantallas. Un driver por compositor (`hyprland`, `umbriel`) llena el
//! estado común y atiende los pedidos; cada uno publica el estado entero, nunca deltas.
//!
//! Lo que solo tiene Hyprland (la config desde el menú) el menú lo muestra deshabilitado con el
//! porqué, nunca escondido (`menu/arbol.rs`, `solo_hyprland`).

pub mod compartir;
pub mod hyprland;
pub mod umbriel;

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

pub use morgan_ipc::compositor::Tipo;

type Oyentes = Rc<RefCell<Vec<Box<dyn Fn(bool)>>>>;
pub type Activos = Vec<(String, i64)>;
pub type Leido = (Vec<Espacio>, Activos, Option<String>);
type OyentesEspacios = Rc<RefCell<Vec<Box<dyn Fn(&Espacios)>>>>;
type OyentesTexto = Rc<RefCell<Vec<Box<dyn Fn(&str)>>>>;
type OyentesVentanas = Rc<RefCell<Vec<Box<dyn Fn(CambioVentanas)>>>>;
type OyentesSalidas = Rc<RefCell<Vec<Box<dyn Fn(&[String])>>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Espacio {
    pub id: i64,
    pub monitor: String,
    pub ventanas: u32,
    /// Lo que se muestra en la barra: el número del workspace en su salida.
    pub nombre: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Espacios {
    pub espacios: Vec<Espacio>,
    pub activos: Activos,
    pub enfocada: Option<String>,
    pub urgentes: BTreeSet<i64>,
    /// Cada salida numera los suyos (Umbriel): el número de la salida va en las centenas.
    pub por_salida: bool,
}

impl Espacios {
    pub fn activo(&self, id: i64) -> bool {
        self.activos.iter().any(|(_, a)| *a == id)
    }

    /// Las apagadas no figuran en `activos`.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Una ventana mapeada y a la vista de algún workspace (las de un scratchpad escondido o
/// minimizadas no). `rect` en coordenadas del layout, como las salidas.
#[derive(Debug, Clone, PartialEq)]
pub struct Cliente {
    pub id: String,
    pub clase: String,
    pub titulo: String,
    /// Negativo: un workspace especial (el scratchpad de Hyprland).
    pub ws: i64,
    pub rect: Rect,
    pub salida: String,
    /// 0 es la que tuvo el foco último.
    pub historia: i32,
    pub juego: bool,
    /// El proceso de la ventana, si el compositor lo sabe.
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Salida {
    pub nombre: String,
    /// El tamaño lógico (sobre la escala y girado), en coordenadas del layout.
    pub rect: Rect,
    pub escala: f64,
    /// Los píxeles del modo.
    pub ancho: i32,
    pub alto: i32,
    pub enfocada: bool,
    pub activo: i64,
}

/// Lo que mira el dock para volver a leer las ventanas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CambioVentanas {
    /// Se abrió, se cerró o se movió de workspace alguna.
    Lista,
    Foco(String),
    Titulo,
}

/// Cada [`Driver::pedir`] va en su hilo, así que dos seguidos pueden llegar al revés: un
/// "apagar" que llega después de un "prender" deja la pantalla negra y sin nada que la
/// despierte. Va uno por vez, y de los que esperan solo importa el último.
#[derive(Clone, Default)]
struct UltimoGana<T> {
    pendiente: Rc<Cell<Option<T>>>,
    enviando: Rc<Cell<bool>>,
}

impl<T> UltimoGana<T> {
    /// `true` si no hay nadie enviando y hay que llamar a [`Self::vaciar`].
    fn anotar(&self, valor: T) -> bool {
        self.pendiente.set(Some(valor));
        !self.enviando.replace(true)
    }

    async fn vaciar<F: Future>(&self, enviar: impl Fn(T) -> F) {
        while let Some(valor) = self.pendiente.take() {
            enviar(valor).await;
        }
        self.enviando.set(false);
    }
}

#[derive(Clone, Default)]
enum Driver {
    #[default]
    Ninguno,
    Hyprland(hyprland::Hyprland),
    Umbriel(umbriel::Umbriel),
}

#[derive(Clone, Default)]
pub struct Compositor {
    driver: Driver,
    juego: Rc<Cell<bool>>,
    oyentes: Oyentes,
    espacios: Rc<RefCell<Espacios>>,
    oyentes_espacios: OyentesEspacios,
    capturas: Rc<Cell<u32>>,
    oyentes_compartir: Oyentes,
    oyentes_foco: OyentesTexto,
    oyentes_linea: OyentesTexto,
    oyentes_ventanas: OyentesVentanas,
    oyentes_salidas: OyentesSalidas,
    salidas: Rc<RefCell<Vec<String>>>,
    dpms: UltimoGana<bool>,
}

impl Compositor {
    /// Sin compositor conocido avisa una vez y queda como "sin juego": el OSD se comporta
    /// como siempre.
    pub fn new() -> Self {
        let driver = match morgan_ipc::compositor::actual() {
            Tipo::Hyprland => match hyprland::Hyprland::new() {
                Some(h) => Driver::Hyprland(h),
                None => Driver::Ninguno,
            },
            Tipo::Umbriel => match umbriel::Umbriel::new() {
                Some(u) => Driver::Umbriel(u),
                None => Driver::Ninguno,
            },
            Tipo::Otro => Driver::Ninguno,
        };
        let c = Compositor {
            driver,
            ..Default::default()
        };
        match &c.driver {
            Driver::Ninguno => eprintln!("morgan: compositor desconocido; modo juego apagado"),
            Driver::Hyprland(h) => h.arrancar(&c),
            Driver::Umbriel(u) => u.arrancar(&c),
        }
        // Hyprland avisa la pantalla compartida con su evento; los demás, por PipeWire.
        if !matches!(c.driver, Driver::Hyprland(_)) {
            let (tx, rx) = async_channel::unbounded();
            compartir::escuchar(tx);
            let yo = c.clone();
            glib::MainContext::default().spawn_local(async move {
                while let Ok(empieza) = rx.recv().await {
                    yo.captura(empieza);
                }
            });
        }
        c
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

    /// Cambió la ventana con el foco (su id; vacío si no hay).
    pub fn al_enfocar(&self, f: impl Fn(&str) + 'static) {
        self.oyentes_foco.borrow_mut().push(Box::new(f));
    }

    pub fn al_ventanas(&self, f: impl Fn(CambioVentanas) + 'static) {
        self.oyentes_ventanas.borrow_mut().push(Box::new(f));
    }

    /// Las salidas prendidas, con cada cambio (una que se conecta, una que se va, una recarga
    /// de la config). La primera vez, las del arranque.
    pub fn al_salidas(&self, f: impl Fn(&[String]) + 'static) {
        self.oyentes_salidas.borrow_mut().push(Box::new(f));
    }

    /// Cada línea del socket de eventos de Hyprland: los binds de morgan avisan por ahí
    /// (`custom>>morgan …`). En los otros compositores los binds llaman a `morganctl`.
    pub fn al_linea_hyprland(&self, f: impl Fn(&str) + 'static) {
        self.oyentes_linea.borrow_mut().push(Box::new(f));
    }

    pub async fn clientes(&self) -> Option<Vec<Cliente>> {
        match &self.driver {
            Driver::Hyprland(h) => h.clientes().await,
            Driver::Umbriel(u) => u.clientes().await,
            Driver::Ninguno => None,
        }
    }

    pub async fn salidas(&self) -> Option<Vec<Salida>> {
        match &self.driver {
            Driver::Hyprland(h) => h.salidas().await,
            Driver::Umbriel(u) => u.salidas().await,
            Driver::Ninguno => None,
        }
    }

    /// El id de la ventana con el foco.
    pub async fn activa(&self) -> Option<String> {
        match &self.driver {
            Driver::Hyprland(h) => h.activa().await,
            Driver::Umbriel(u) => u.activa().await,
            Driver::Ninguno => None,
        }
    }

    /// Le pide a la ventana que se cierre (la app puede preguntar antes, como con el atajo).
    pub async fn cerrar(&self, id: &str) {
        match &self.driver {
            Driver::Hyprland(h) => h.cerrar(id).await,
            Driver::Umbriel(u) => u.cerrar(id).await,
            Driver::Ninguno => {}
        }
    }

    /// `quieto`: el puntero no va a la ventana (el dock: el mouse se queda en el dock).
    pub async fn enfocar(&self, id: &str, quieto: bool) {
        match &self.driver {
            Driver::Hyprland(h) => h.enfocar(id, quieto).await,
            Driver::Umbriel(u) => u.enfocar(id).await,
            Driver::Ninguno => {}
        }
    }

    /// Lo que se marca después sale del evento que llegue, no de este pedido.
    pub fn ir_a(&self, id: i64) {
        let d = self.driver.clone();
        glib::MainContext::default().spawn_local(async move {
            match d {
                Driver::Hyprland(h) => h.ir_a(id).await,
                Driver::Umbriel(u) => u.ir_a(id).await,
                Driver::Ninguno => {}
            }
        });
    }

    /// Abre o cierra el overview del compositor, donde lo hay (ver
    /// `morgan_ipc::compositor::overview_propio`).
    pub fn overview(&self) {
        let d = self.driver.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Driver::Umbriel(u) = d {
                u.overview().await;
            }
        });
    }

    pub fn dpms(&self, prender: bool) {
        if matches!(self.driver, Driver::Ninguno) || !self.dpms.anotar(prender) {
            return;
        }
        let (cola, d) = (self.dpms.clone(), self.driver.clone());
        glib::MainContext::default().spawn_local(async move {
            cola.vaciar(|prender| {
                let d = d.clone();
                async move {
                    match d {
                        Driver::Hyprland(h) => h.dpms(prender).await,
                        Driver::Umbriel(u) => u.dpms(prender).await,
                        Driver::Ninguno => {}
                    }
                }
            })
            .await;
        });
    }

    /// El submap de atajos en el que está el compositor (`None` sin submaps o sin ninguno).
    pub async fn submap(&self) -> Option<String> {
        match &self.driver {
            Driver::Hyprland(h) => h.submap().await,
            Driver::Umbriel(u) => u.submap().await,
            Driver::Ninguno => None,
        }
    }

    pub async fn salir_del_submap(&self) {
        match &self.driver {
            Driver::Hyprland(h) => h.salir_del_submap().await,
            Driver::Umbriel(u) => u.salir_del_submap().await,
            Driver::Ninguno => {}
        }
    }

    /// Bloq Mayús: Hyprland lo dice por teclado; en los demás, el LED del kernel.
    pub async fn bloq_mayus(&self) -> Option<bool> {
        if let Driver::Hyprland(h) = &self.driver {
            return h.bloq_mayus().await;
        }
        bloq_mayus_led()
    }

    // Lo que llaman los drivers.

    fn poner_juego(&self, v: bool) {
        if self.juego.replace(v) != v {
            for f in self.oyentes.borrow().iter() {
                f(v);
            }
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

    fn enfocada(&self, id: &str) {
        for f in self.oyentes_foco.borrow().iter() {
            f(id);
        }
    }

    fn ventanas_cambiaron(&self, c: CambioVentanas) {
        for f in self.oyentes_ventanas.borrow().iter() {
            f(c.clone());
        }
    }

    fn linea_hyprland(&self, linea: &str) {
        for f in self.oyentes_linea.borrow().iter() {
            f(linea);
        }
    }

    fn salidas_cambiaron(&self, nombres: Vec<String>) {
        self.salidas.replace(nombres.clone());
        for f in self.oyentes_salidas.borrow().iter() {
            f(&nombres);
        }
    }

    fn releer_espacios(&self, leido: Leido, por_salida: bool) {
        let mut nuevo = self.espacios.borrow().clone();
        nuevo.releido(leido);
        nuevo.por_salida = por_salida;
        self.publicar(nuevo);
    }

    fn marcar_urgente(&self, id: i64) {
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

/// Escribe la config de morgan para el compositor de la sesión (atajos, capas y lo que se
/// cambia desde el menú) y se la hace releer.
pub async fn aplicar_config(
    c: &crate::config::Config,
    ruta_config: &std::path::Path,
) -> Result<(), String> {
    match morgan_ipc::compositor::actual() {
        Tipo::Hyprland => crate::hypr_cfg::aplicar(c, ruta_config).await,
        Tipo::Umbriel => crate::umbriel_cfg::aplicar(c, ruta_config).await,
        Tipo::Otro => Ok(()),
    }
}

/// Sin esperar al main loop: para el arranque.
pub fn salidas_ya() -> Option<Vec<Salida>> {
    match morgan_ipc::compositor::actual() {
        Tipo::Hyprland => {
            let dir = morgan_ipc::hyprland::dir_from(|k| std::env::var_os(k))?;
            let json =
                morgan_ipc::hyprland::pedir_ya(&dir.join(".socket.sock"), "j/monitors").ok()?;
            hyprland::leer_salidas(&json).ok()
        }
        Tipo::Umbriel => {
            let ruta = morgan_ipc::umbriel::socket()?;
            let v = morgan_ipc::umbriel::pedir_ya(&ruta, &serde_json::json!({"cmd": "outputs"}))
                .ok()?;
            let e = umbriel::Estado {
                outputs: serde_json::from_value(v).ok()?,
                ..Default::default()
            };
            Some(e.salidas())
        }
        Tipo::Otro => None,
    }
}

/// Las salidas que aparecieron en `ahora` y no estaban en `antes`.
pub fn nuevas<'a>(antes: &[String], ahora: &'a [String]) -> Vec<&'a str> {
    ahora
        .iter()
        .filter(|n| !antes.contains(n))
        .map(String::as_str)
        .collect()
}

/// Cualquier teclado con el LED prendido.
fn bloq_mayus_led() -> Option<bool> {
    let mut alguno = None;
    for e in std::fs::read_dir("/sys/class/leds").ok()?.flatten() {
        if !e.file_name().to_string_lossy().ends_with("::capslock") {
            continue;
        }
        let v = std::fs::read_to_string(e.path().join("brightness")).ok()?;
        let prendido = v.trim() != "0";
        alguno = Some(alguno.unwrap_or(false) || prendido);
    }
    alguno
}

/// Por la ventana y no por dónde está: un workspace fijo para los juegos es una convención de
/// cada config, y una ventana cualquiera movida ahí no es un juego. El tipo de contenido lo
/// declara la app (`content-type-v1`) o una regla del compositor, para lo que no lanza Steam.
pub fn es_juego(
    clase: &str,
    contenido: &str,
    pid: Option<i64>,
    de_steam: impl Fn(u32) -> bool,
) -> bool {
    contenido == "game"
        || clase == "gamescope"
        || clase.starts_with("steam_app_")
        || pid
            .and_then(|p| u32::try_from(p).ok())
            .is_some_and(|p| p > 1 && de_steam(p))
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
    fn dpms_en_orden_y_gana_el_ultimo() {
        let cola = UltimoGana::default();
        let vistos = RefCell::new(Vec::new());
        assert!(cola.anotar(false));
        futures_lite::future::block_on(cola.vaciar(|v| {
            vistos.borrow_mut().push(v);
            if vistos.borrow().len() == 1 {
                // Mientras se envía el primero: no arranca otro envío y el del medio se pisa.
                assert!(!cola.anotar(false));
                assert!(!cola.anotar(true));
            }
            async {}
        }));
        assert_eq!(*vistos.borrow(), [false, true]);
        assert!(
            cola.anotar(true),
            "al terminar, el siguiente vuelve a enviar"
        );
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
    fn capturas_de_pantalla() {
        let c = Compositor::default();
        let visto = Rc::new(RefCell::new(Vec::new()));
        let v = visto.clone();
        c.al_compartir(move |b| v.borrow_mut().push(b));
        // Dos capturas: termina una y el indicador sigue; termina la otra y se apaga.
        c.captura(true);
        c.captura(true);
        c.captura(false);
        c.captura(false);
        c.captura(false);
        assert_eq!(*visto.borrow(), [true, false]);
    }

    #[test]
    fn salidas_nuevas() {
        let antes = vec!["DP-3".to_string()];
        let ahora = vec!["DP-3".to_string(), "HDMI-A-1".to_string()];
        assert_eq!(nuevas(&antes, &ahora), ["HDMI-A-1"]);
        assert!(nuevas(&ahora, &antes).is_empty());
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
    fn urgente_hasta_que_se_mira() {
        let espacio = |id| Espacio {
            id,
            monitor: "DP-3".into(),
            ventanas: 1,
            nombre: id.to_string(),
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
}
