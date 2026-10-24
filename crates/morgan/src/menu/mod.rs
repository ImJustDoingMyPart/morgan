//! El menú se dibuja en el hilo del centro (`centro/dibujo/ajustes.rs`), con lo escrito, lo
//! elegido, lo desplegado y los scrolls; acá quedan las páginas (leídas de la fuente real), la
//! búsqueda, lo que hace cada fila y la grabación de un atajo.
//!
//! El menú lee lo puesto de la fuente real, aplica y vuelve a leer para marcar. Lo de morgan se
//! escribe en `config.toml`, se relee y se aplica en vivo.

pub(crate) mod arbol;
mod carpeta;
pub mod atajos;
pub mod compositor;
pub(crate) mod cursores;
mod hora;
pub(crate) mod lectura;
pub mod monitores;
pub mod reglas;
mod sistema;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gio::prelude::*;
use morgan_ipc::{ModoLauncher, NivelMenu};
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::centro::hilo::Orden;
use crate::centro::vista::{EventoAjustes, FilaIndice, IndiceAjustes, Lugar, PaginaAjustes};
use crate::comando;
use crate::config::{self, Config};
use crate::idle::Idle;
use crate::launcher::{binds, busqueda};
use crate::notificaciones::Notificaciones;
use crate::sonidos;
use crate::tema::Tema;
use arbol::{Categoria, Fila, Que, Resultado, Seccion};
use lectura::leer;

const SEGUIR: Duration = Duration::from_millis(120);
type AlLauncher = Rc<dyn Fn(ModoLauncher)>;

pub struct Servicios {
    pub config: Config,
    pub ruta_config: PathBuf,
    pub tema: Tema,
    pub notificaciones: Notificaciones,
    pub idle: Idle,
    pub audio: crate::audio::Audio,
    pub sonidos: crate::sonidos::Sonidos,
    pub luz: crate::luz::Luz,
    pub fondos: crate::fondos::Fondos,
    pub calendario: crate::calendario::Calendario,
    pub centro: crate::centro::Centro,
}

#[derive(Debug, Clone, PartialEq)]
enum Entrada {
    Categoria(Categoria),
    Seccion(Categoria, usize),
    Resultado(Resultado),
}

#[derive(Clone)]
pub struct Menu {
    inner: Rc<Inner>,
}

/// Los cambios a prueba (salida y campo) y el temporizador que los deshace.
type Prueba = (Vec<(String, monitores::Campo)>, glib::SourceId);

struct Inner {
    hilo: Sender<Orden>,
    /// Los cambios de monitor a prueba y la vuelta automática.
    prueba: RefCell<Option<Prueba>>,
    /// La región cuyas ciudades muestra «Fecha y hora»; sin elegir, la de la zona puesta.
    region_horaria: RefCell<Option<String>>,
    al_fondos: RefCell<Option<Rc<dyn Fn()>>>,
    al_launcher: RefCell<Option<AlLauncher>>,

    s: Servicios,
    config: RefCell<Config>,
    abierto: Cell<bool>,
    categoria: Cell<Categoria>,
    /// Sobrevive al cierre: al abrir se muestra al instante y se vuelve a leer todo atrás.
    paginas: RefCell<HashMap<Categoria, Vec<Seccion>>>,
    /// Una lectura que llega cuando ya se pidió otra de la misma página se descarta, así no pisa
    /// algo más nuevo.
    revisiones: RefCell<HashMap<Categoria, u64>>,
    pedidos: Cell<u64>,
    listas: RefCell<Option<Rc<lectura::Listas>>>,
    /// La página sigue al resultado elegido un momento después de la última tecla, no en cada una.
    seguir: RefCell<Option<glib::SourceId>>,
    seguido: RefCell<Option<Entrada>>,
    /// Propia, abierta mientras el menú lo está: la del centro se abre solo con el centro.
    mezclador: crate::mezclador::Mezclador,
    salidas: RefCell<Vec<(String, String)>>,
    /// Lo buscado y las filas del índice que se mandaron, con la elegida.
    texto: RefCell<String>,
    entradas: RefCell<Vec<Entrada>>,
    elegida: Cell<usize>,
    /// Suben con cada índice y cada página mandados: lo que vuelve de otra se descarta.
    version_indice: Cell<u64>,
    version_pagina: Cell<u64>,
    indice_abierta: Cell<Option<Categoria>>,
    grabando: RefCell<Option<Grabacion>>,
    grabaciones: Cell<u64>,
}

/// Hyprland resuelve los binds antes que el menú: mientras dure, está en el submap de grabar,
/// que no tiene ninguno, y sale siempre (al guardar, al cancelar, al cerrar el menú y al
/// arrancar). La fila lo dice en su subtítulo (y una nota, si la tecla ya es de otro).
struct Grabacion {
    id: String,
    numero: u64,
    categoria: Categoria,
    lugar: Lugar,
    sub: String,
    nota: Option<String>,
    ocupadas: Option<Vec<crate::launcher::fuentes::Atajo>>,
    pendiente: Option<(String, Option<String>)>,
}

impl Menu {
    pub fn new(s: Servicios) -> Self {
        let config = RefCell::new(s.config.clone());
        let hilo = s.centro.hilo();
        let centro = s.centro.clone();
        let inner = Rc::new(Inner {
            hilo,
            al_fondos: RefCell::default(),
            al_launcher: RefCell::default(),
            s,
            config,
            abierto: Cell::new(false),
            categoria: Cell::new(Categoria::Apariencia),
            paginas: RefCell::default(),
            revisiones: RefCell::default(),
            pedidos: Cell::new(0),
            listas: RefCell::default(),
            seguir: RefCell::default(),
            seguido: RefCell::default(),
            mezclador: crate::mezclador::Mezclador::new(),
            salidas: RefCell::default(),
            texto: RefCell::default(),
            entradas: RefCell::default(),
            elegida: Cell::new(0),
            version_indice: Cell::new(0),
            version_pagina: Cell::new(0),
            indice_abierta: Cell::new(None),
            grabando: RefCell::new(None),
            grabaciones: Cell::new(0),
            prueba: RefCell::new(None),
            region_horaria: RefCell::new(None),
        });
        let m = Menu { inner };
        m.conectar(&centro);
        // Por si morgan se cayó grabando una tecla: en el submap de grabar no anda ningún bind.
        comando::lanzar(&["hyprctl", "eval", &binds::submap_grabar(false)]);
        // La primera apertura no tiene «lo de la vez anterior»: sin esto se dibuja vacía y se
        // llena un instante después. Se lee al rato de arrancar, no en el arranque mismo.
        let yo = m.clone();
        glib::timeout_add_local_once(Duration::from_secs(2), move || {
            for c in Categoria::TODAS {
                yo.pedir(c);
            }
        });
        m
    }

    pub fn al_fondos(&self, f: impl Fn() + 'static) {
        self.inner.al_fondos.replace(Some(Rc::new(f)));
    }

    pub fn al_launcher(&self, f: impl Fn(ModoLauncher) + 'static) {
        self.inner.al_launcher.replace(Some(Rc::new(f)));
    }

    pub fn poner_ubicacion(&self, u: Option<crate::config::Clima>) {
        if let Some(u) = u
            && let Err(e) = self.cambiar_config(&arbol::Cambio::Ubicacion(u))
        {
            eprintln!("morgan: menú: {e}");
        }
        self.abrir(NivelMenu::Clima);
    }

    pub fn abierto(&self) -> bool {
        self.inner.abierto.get()
    }

    /// Apariencia, leída y armada antes de la primera apertura: que abra con lo de la vez
    /// anterior y sin el tirón de cargar las fuentes de su galería.
    pub fn precalentar(&self) {
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            glib::timeout_future(Duration::from_secs(3)).await;
            yo.refrescar(Categoria::Apariencia, None).await;
            let i = &yo.inner;
            if i.abierto.get() {
                return;
            }
            let secciones = i.paginas.borrow().get(&Categoria::Apariencia).cloned();
            yo.mandar(Orden::AjustesPrecalentar(PaginaAjustes {
                titulo: Categoria::Apariencia.titulo().to_owned(),
                secciones,
                entra: false,
                version: 0,
                elegir: None,
            }));
        });
    }

    pub fn alternar(&self, nivel: NivelMenu) {
        if self.abierto() {
            self.cerrar();
        } else {
            self.abrir(nivel);
        }
    }

    fn mandar(&self, o: Orden) {
        // Si el hilo del centro terminó (sin Wayland o sin GPU) ya lo dijo al terminar.
        let _ = self.inner.hilo.send(o);
    }

    pub fn abrir(&self, nivel: NivelMenu) {
        let i = &self.inner;
        let ya_abierto = i.abierto.replace(true);
        let (cat, seccion) = match nivel {
            NivelMenu::Raiz => (Categoria::Apariencia, None),
            NivelMenu::Fondos => (Categoria::Apariencia, Some("Fondos")),
            NivelMenu::Clima => (Categoria::Shell, Some("Preferencias")),
        };
        let seccion = seccion.and_then(|n| cat.secciones().iter().position(|s| *s == n));
        if !ya_abierto {
            i.indice_abierta.set(None);
            i.seguido.replace(None);
            i.region_horaria.replace(None);
            i.mezclador.abrir();
            i.listas.replace(None);
            // Se muestra lo de la vez anterior y se relee todo: la búsqueda lo necesita.
            for c in Categoria::TODAS {
                self.pedir(c);
            }
        }
        self.mandar(Orden::AjustesAbrir);
        i.texto.borrow_mut().clear();
        self.ver_categoria(cat, seccion.map(|s| (s, None)));
        self.pintar_indice(Some(match seccion {
            Some(s) => Entrada::Seccion(cat, s),
            None => Entrada::Categoria(cat),
        }));
    }

    pub fn cerrar(&self) {
        if !self.inner.abierto.get() {
            return;
        }
        self.mandar(Orden::AjustesCerrar);
        self.cerrado();
    }

    /// Lo de después de cerrar, lo haya cerrado quien sea.
    fn cerrado(&self) {
        let i = &self.inner;
        if !i.abierto.replace(false) {
            return;
        }
        self.dejar_de_grabar();
        if let Some(s) = i.seguir.take() {
            s.remove();
        }
        i.mezclador.cerrar();
    }

    fn conectar(&self, centro: &crate::centro::Centro) {
        let i = &self.inner;
        let weak = Rc::downgrade(&self.inner);
        centro.al_ajustes(move |e| {
            if let Some(inner) = weak.upgrade() {
                Menu { inner }.evento(e);
            }
        });

        // Sonido se relee cuando cambian las salidas (no en cada paso de volumen).
        let yo = self.clone();
        i.mezclador.al_cambiar(move || {
            let i = &yo.inner;
            let ahora: Vec<(String, String)> = i
                .mezclador
                .estado()
                .salidas
                .into_iter()
                .map(|s| (s.nombre, s.descripcion))
                .collect();
            if *i.salidas.borrow() != ahora {
                i.salidas.replace(ahora);
                yo.pedir(Categoria::Sonido);
            }
        });
    }

    fn evento(&self, e: EventoAjustes) {
        let i = &self.inner;
        if !i.abierto.get() {
            return;
        }
        match e {
            EventoAjustes::Texto(t) => {
                let buscando = !t.trim().is_empty();
                i.texto.replace(t);
                // Al borrar la búsqueda, el índice vuelve a la categoría que se está viendo.
                let actual = Entrada::Categoria(i.categoria.get());
                self.pintar_indice((!buscando).then_some(actual));
            }
            EventoAjustes::Navegar { version, indice } => {
                if version == i.version_indice.get() {
                    i.elegida.set(indice);
                    if let Some(s) = i.seguir.take() {
                        s.remove();
                    }
                    self.navegar(indice);
                }
            }
            EventoAjustes::Hacer {
                version,
                lugar,
                que,
            } => {
                if version == i.version_pagina.get() {
                    self.ejecutar(lugar, &que);
                } else {
                    self.mandar(Orden::AjustesHecho { lugar, ok: true });
                }
            }
            EventoAjustes::Tecla { keysym, base, mods } => self.tecla_grabada(keysym, base, mods),
            EventoAjustes::Cerrado => self.cerrado(),
        }
    }

    fn entrada_elegida(&self) -> Option<Entrada> {
        let i = &self.inner;
        i.entradas.borrow().get(i.elegida.get()).cloned()
    }

    fn navegar(&self, posicion: usize) {
        let i = &self.inner;
        let Some(e) = i.entradas.borrow().get(posicion).cloned() else {
            return;
        };
        i.seguido.replace(Some(e.clone()));
        match e {
            Entrada::Categoria(c) => {
                if c != i.categoria.get() {
                    self.ver_categoria(c, None);
                    self.pintar_indice(Some(Entrada::Categoria(c)));
                } else {
                    self.ir_a(0, None, true);
                }
            }
            Entrada::Seccion(c, s) => {
                if c != i.categoria.get() {
                    self.ver_categoria(c, Some((s, None)));
                } else {
                    self.ir_a(s, None, true);
                }
            }
            Entrada::Resultado(r) => {
                if r.categoria != i.categoria.get() {
                    self.ver_categoria(r.categoria, Some((r.seccion, r.fila)));
                } else {
                    self.ir_a(r.seccion, r.fila, true);
                }
            }
        }
    }

    fn ir_a(&self, seccion: usize, fila: Option<usize>, suave: bool) {
        self.mandar(Orden::AjustesIr {
            seccion,
            fila,
            suave,
        });
    }

    fn pintar_indice(&self, elegir: Option<Entrada>) {
        let i = &self.inner;
        let consulta = i.texto.borrow().clone();
        let buscando = !consulta.trim().is_empty();
        let mut desplegar = false;
        let entradas: Vec<Entrada> = if buscando {
            let paginas = i.paginas.borrow();
            let leidas: Vec<(Categoria, &[Seccion])> = Categoria::TODAS
                .iter()
                .filter_map(|c| paginas.get(c).map(|p| (*c, &p[..])))
                .collect();
            arbol::buscar(&leidas, &consulta)
                .into_iter()
                .map(Entrada::Resultado)
                .collect()
        } else {
            let abierta = i.categoria.get();
            desplegar = i
                .indice_abierta
                .replace(Some(abierta))
                .is_some_and(|a| a != abierta);
            let mut v = Vec::new();
            for c in Categoria::TODAS {
                v.push(Entrada::Categoria(c));
                if c == abierta && c.secciones().len() > 1 {
                    v.extend((0..c.secciones().len()).map(|s| Entrada::Seccion(c, s)));
                }
            }
            v
        };
        let filas = entradas.iter().map(|e| fila_indice(e, &consulta)).collect();
        let elegida = elegir
            .and_then(|el| entradas.iter().position(|e| *e == el))
            .unwrap_or(0);
        let hay = !entradas.is_empty();
        i.entradas.replace(entradas);
        i.elegida.set(elegida);
        i.version_indice.set(i.version_indice.get() + 1);
        self.mandar(Orden::AjustesIndice(IndiceAjustes {
            filas,
            elegida,
            desplegar,
            version: i.version_indice.get(),
        }));
        if hay && buscando {
            self.seguir_busqueda();
        }
    }

    /// Rehacer una página como Apariencia en cada tecla traba.
    fn seguir_busqueda(&self) {
        let i = &self.inner;
        if let Some(s) = i.seguir.take() {
            s.remove();
        }
        let yo = self.clone();
        let id = glib::timeout_add_local_once(SEGUIR, move || {
            let i = &yo.inner;
            i.seguir.take();
            let Some(e) = yo.entrada_elegida() else {
                return;
            };
            if i.seguido.borrow().as_ref() == Some(&e) {
                return;
            }
            yo.navegar(i.elegida.get());
        });
        i.seguir.replace(Some(id));
    }

    fn pedir(&self, cat: Categoria) {
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move { yo.refrescar(cat, None).await });
    }

    /// Si llega tarde (se pidió otra lectura de la misma página), se descarta.
    async fn refrescar(&self, cat: Categoria, elegido: Option<usize>) {
        let i = &self.inner;
        let revision = i.pedidos.get() + 1;
        i.pedidos.set(revision);
        i.revisiones.borrow_mut().insert(cat, revision);
        let secciones = self.leer(cat).await;
        if i.revisiones.borrow().get(&cat) != Some(&revision) {
            return;
        }
        let antes = i.paginas.borrow_mut().insert(cat, secciones);
        let cambio = antes.as_ref() != i.paginas.borrow().get(&cat);
        if !i.abierto.get() || !cambio {
            return;
        }
        if i.categoria.get() == cat {
            self.pintar_pagina(antes.is_none(), elegido);
        }
        // Una búsqueda incluye lo que llegó.
        if !i.texto.borrow().trim().is_empty() {
            let elegida = self.entrada_elegida();
            self.pintar_indice(elegida);
        }
    }

    async fn leer(&self, cat: Categoria) -> Vec<Seccion> {
        let c = self.inner.config.borrow().clone();
        match cat {
            Categoria::Shell | Categoria::Pantalla | Categoria::Notificaciones => {
                let apps = self.inner.s.notificaciones.apps();
                let mut p = arbol::pagina_fija(cat, &c, &apps).unwrap_or_default();
                if cat == Categoria::Shell
                    && let Some(f) = p.get_mut(1)
                {
                    let region = self.inner.region_horaria.borrow().clone();
                    f.filas =
                        hora::filas(&hora::leer().await, region.as_deref(), chrono::Utc::now());
                }
                if cat == Categoria::Pantalla
                    && let Some(m) = p.first_mut()
                {
                    let mut salidas =
                        monitores::de_json(&leer(&["hyprctl", "-j", "monitors", "all"]).await);
                    let reglas =
                        monitores::Reglas::leer(&self.ruta_monitores()).unwrap_or_else(|e| {
                            eprintln!("morgan: menú: {e}");
                            Default::default()
                        });
                    let ruta = glib::user_cache_dir().join("morgan/monitores-vistos.toml");
                    let mut vistos = monitores::Vistos::leer(&ruta);
                    if vistos.anotar(&salidas)
                        && let Err(e) = vistos.guardar(&ruta)
                    {
                        eprintln!("morgan: menú: {e}");
                    }
                    monitores::completar(&mut salidas, &reglas, &vistos);
                    m.filas =
                        monitores::filas(&salidas, &reglas, self.inner.prueba.borrow().is_some());
                }
                p
            }
            Categoria::Apariencia => {
                let listas = self.inner.listas.borrow().clone();
                let (lecturas, nuevas) = lectura::apariencia(listas, &c.fondos).await;
                if let Some(n) = nuevas {
                    self.inner.listas.replace(Some(Rc::new(n)));
                }
                arbol::apariencia(&lecturas, &c)
            }
            Categoria::Compositor => {
                lectura::compositor(&self.ruta_cambios_hypr(), &c.reglas).await
            }
            Categoria::Atajos => lectura::atajos(&c.atajos).await,
            Categoria::Aplicaciones => lectura::aplicaciones(&c.capturas),
            Categoria::Sistema => lectura::sistema().await,
            Categoria::Sonido => {
                let salidas = self.inner.salidas.borrow().clone();
                arbol::sonido(&c, &sonidos::temas(), &sonidos::tema_puesto(), &salidas)
            }
        }
    }

    fn ruta_monitores(&self) -> PathBuf {
        crate::hypr_cfg::ruta_monitores(&self.inner.s.ruta_config)
    }

    /// Prueba en vivo: si nadie la mantiene, a los `SEGUNDOS_PRUEBA` vuelve sola, con el menú
    /// abierto o no. Una segunda prueba se suma a la primera y reinicia la cuenta.
    async fn probar_monitor(&self, salida: String, campo: monitores::Campo) -> Result<(), String> {
        comando::salida(&["hyprctl", "eval", &monitores::prueba(&salida, &campo)]).await?;
        let i = &self.inner;
        let mut cambios = i.prueba.borrow_mut().take().map(|(c, id)| {
            id.remove();
            c
        });
        cambios.get_or_insert_with(Vec::new).push((salida, campo));
        let yo = self.clone();
        let id = glib::timeout_add_seconds_local_once(monitores::SEGUNDOS_PRUEBA, move || {
            yo.inner.prueba.borrow_mut().take();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = yo.volver_monitores().await {
                    eprintln!("morgan: menú: {e}");
                }
                yo.refrescar(Categoria::Pantalla, None).await;
            });
        });
        i.prueba.replace(cambios.map(|c| (c, id)));
        Ok(())
    }

    async fn mantener_monitores(&self) -> Result<(), String> {
        let Some((cambios, id)) = self.inner.prueba.borrow_mut().take() else {
            return Ok(());
        };
        id.remove();
        let ruta = self.ruta_monitores();
        let mut r = monitores::Reglas::leer(&ruta)?;
        for (salida, campo) in &cambios {
            r.poner(salida, campo);
        }
        r.guardar(&ruta)?;
        self.escribir_hypr(None).await
    }

    /// Lo de los archivos vuelve con la recarga: la prueba no se guardó en ninguno.
    async fn volver_monitores(&self) -> Result<(), String> {
        if let Some((_, id)) = self.inner.prueba.borrow_mut().take() {
            id.remove();
        }
        comando::salida(&["hyprctl", "reload", "config-only"])
            .await
            .map(drop)
    }

    async fn guardar_monitor(&self, salida: &str, campo: &monitores::Campo) -> Result<(), String> {
        let ruta = self.ruta_monitores();
        let mut r = monitores::Reglas::leer(&ruta)?;
        r.poner(salida, campo);
        r.guardar(&ruta)?;
        // El nombre es solo de morgan: Hyprland no tiene nada que releer.
        if matches!(campo, monitores::Campo::Apodo(_)) {
            return Ok(());
        }
        self.escribir_hypr(None).await
    }

    fn ruta_cambios_hypr(&self) -> PathBuf {
        crate::hypr_cfg::ruta_cambios(&self.inner.s.ruta_config)
    }

    fn ver_categoria(&self, cat: Categoria, ir_a: Option<(usize, Option<usize>)>) {
        self.inner.categoria.set(cat);
        self.pintar_pagina(true, None);
        if let Some((s, f)) = ir_a {
            self.ir_a(s, f, false);
        }
    }

    /// La página de la categoría que se ve, con lo que dice una grabación en curso.
    fn pintar_pagina(&self, entra: bool, elegir: Option<usize>) {
        let i = &self.inner;
        let cat = i.categoria.get();
        let mut secciones = i.paginas.borrow().get(&cat).cloned();
        if let Some(g) = i.grabando.borrow().as_ref()
            && g.categoria == cat
            && let Some(Fila::Ajuste { sub, nota, .. }) = secciones
                .as_mut()
                .and_then(|p| p.get_mut(g.lugar.seccion))
                .and_then(|s| s.filas.get_mut(g.lugar.fila))
        {
            sub.clone_from(&g.sub);
            if let Some(n) = &g.nota {
                nota.clone_from(n);
            }
        }
        i.version_pagina.set(i.version_pagina.get() + 1);
        self.mandar(Orden::AjustesPagina(PaginaAjustes {
            titulo: cat.titulo().to_owned(),
            secciones,
            entra,
            version: i.version_pagina.get(),
            elegir,
        }));
    }

    fn hecho(&self, lugar: Lugar, ok: bool) {
        self.mandar(Orden::AjustesHecho { lugar, ok });
    }

    /// Lo marcado cambia recién cuando el sistema lo dice: se aplica, se avisa cómo salió y se
    /// relee la página de la fila (aunque se haya cambiado de página mientras).
    fn ejecutar(&self, lugar: Lugar, que: &Que) {
        let i = &self.inner;
        // Tocar otra cosa mientras se graba deja la grabación.
        self.dejar_de_grabar();
        match que {
            Que::GrabarTecla(id) => {
                self.grabar(lugar, id);
                return;
            }
            Que::Fondos => {
                self.cerrar();
                let f = i.al_fondos.borrow().clone();
                if let Some(f) = f {
                    f();
                }
                return;
            }
            Que::Ubicacion | Que::Calendario => {
                self.cerrar();
                let modo = if *que == Que::Ubicacion {
                    ModoLauncher::Ciudad
                } else {
                    ModoLauncher::Calendario
                };
                let f = i.al_launcher.borrow().clone();
                if let Some(f) = f {
                    f(modo);
                }
                return;
            }
            Que::Carpetas(actual) => {
                self.examinar_carpeta(actual);
                return;
            }
            Que::Sonar(id) => {
                sonidos::sonar(id, None);
                self.hecho(lugar, true);
                return;
            }
            _ => {}
        }
        // Tras probar un monitor, lo elegido es «Mantener»: la fila tocada ya no está, y lo que
        // quedara en su lugar se tocaría con el próximo Enter.
        let elegir = matches!(que, Que::Monitor(_, c) if c.a_prueba()).then_some(0);
        let cat = i.categoria.get();
        let yo = self.clone();
        let que = que.clone();
        glib::MainContext::default().spawn_local(async move {
            let hecho = yo.aplicar(que).await;
            if let Err(e) = &hecho {
                eprintln!("morgan: menú: {e}");
            }
            yo.hecho(lugar, hecho.is_ok());
            yo.refrescar(cat, elegir).await;
        });
    }

    /// Hyprland entra al submap sin binds después de leer cuáles teclas están ocupadas.
    fn grabar(&self, lugar: Lugar, id: &str) {
        let i = &self.inner;
        let numero = i.grabaciones.get() + 1;
        i.grabaciones.set(numero);
        i.grabando.replace(Some(Grabacion {
            id: id.to_owned(),
            numero,
            categoria: i.categoria.get(),
            lugar,
            sub: tr!("Apretá la combinación · Escape cancela").to_owned(),
            nota: None,
            ocupadas: None,
            pendiente: None,
        }));
        self.mandar(Orden::AjustesGrabando(true));
        self.pintar_pagina(false, None);
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let ocupadas = comando::salida(&["hyprctl", "-j", "binds"])
                .await
                .and_then(|j| crate::launcher::fuentes::ocupadas(&j))
                .unwrap_or_else(|e| {
                    eprintln!("morgan: menú: {e}");
                    Vec::new()
                });
            let entrar = binds::submap_grabar(true);
            if let Err(e) = comando::salida(&["hyprctl", "eval", &entrar]).await {
                eprintln!("morgan: menú: submap de grabar: {e}");
            }
            let sigue = match &mut *yo.inner.grabando.borrow_mut() {
                Some(g) if g.numero == numero => {
                    g.ocupadas = Some(ocupadas);
                    true
                }
                _ => false,
            };
            // Se canceló mientras entraba: se sale de nuevo.
            if !sigue {
                comando::lanzar(&["hyprctl", "eval", &binds::submap_grabar(false)]);
            }
        });
    }

    fn dejar_de_grabar(&self) {
        let Some(g) = self.inner.grabando.take() else {
            return;
        };
        comando::lanzar(&["hyprctl", "eval", &binds::submap_grabar(false)]);
        self.mandar(Orden::AjustesGrabando(false));
        self.hecho(g.lugar, true);
        if self.inner.abierto.get() && self.inner.categoria.get() == g.categoria {
            self.pintar_pagina(false, None);
        }
    }

    fn tecla_grabada(&self, keysym: u32, base: u32, mods: [bool; 4]) {
        use smithay_client_toolkit::seat::keyboard::Keysym as Key;
        let i = &self.inner;
        let tecla = Key::new(keysym);
        let sin_mods = !mods.contains(&true);
        let (pendiente, cargada, lugar) = match &*i.grabando.borrow() {
            None => return,
            Some(g) => (g.pendiente.clone(), g.ocupadas.is_some(), g.lugar),
        };
        match tecla {
            Key::Escape if sin_mods => {
                self.dejar_de_grabar();
                return;
            }
            Key::Return | Key::KP_Enter | Key::ISO_Enter if sin_mods && pendiente.is_some() => {
                let (t, libera) = pendiente.unwrap_or_default();
                self.guardar_tecla(lugar, t, libera);
                return;
            }
            Key::Shift_L
            | Key::Shift_R
            | Key::Control_L
            | Key::Control_R
            | Key::Alt_L
            | Key::Alt_R
            | Key::Super_L
            | Key::Super_R
            | Key::Meta_L
            | Key::Meta_R
            | Key::Hyper_L
            | Key::Hyper_R
            | Key::ISO_Level3_Shift
            | Key::Caps_Lock
            | Key::Num_Lock => return,
            _ if !cargada => return,
            _ => {}
        }
        // Hyprland compara la tecla sin los modificadores puestos (Shift + 1 es `1`, no `!`).
        let nombre = xkbcommon::xkb::keysym_get_name(Key::new(base));
        let nueva = binds::combinacion(mods, &nombre);
        let comparable = binds::normalizar(&nueva);
        let (id, choque) = {
            let g = i.grabando.borrow();
            let Some(g) = g.as_ref() else { return };
            let choque = g
                .ocupadas
                .iter()
                .flatten()
                .find(|a| binds::normalizar(&a.teclas) == comparable)
                .cloned();
            (g.id.clone(), choque)
        };
        let config = i.config.borrow().atajos.clone();
        let propia = config.tecla(&id).unwrap_or_default();
        if binds::normalizar(&propia) == comparable {
            // La que ya tenía: nada que cambiar.
            self.dejar_de_grabar();
            return;
        }
        let Some(choque) = choque else {
            self.guardar_tecla(lugar, nueva, None);
            return;
        };
        // La usa otro: si es de morgan, al reasignar queda sin tecla.
        let libera = binds::propios(&config)
            .into_iter()
            .map(|p| (p.id.to_owned(), p.tecla))
            .chain(
                config
                    .propios
                    .iter()
                    .enumerate()
                    .map(|(n, p)| (format!("{}{n}", crate::config::PROPIO), p.key.clone())),
            )
            .chain(
                config
                    .modos
                    .iter()
                    .enumerate()
                    .map(|(m, modo)| (format!("{}{m}", crate::config::MODO), modo.key.clone())),
            )
            .find(|(otro, tecla)| *otro != id && binds::normalizar(tecla) == comparable)
            .map(|(otro, _)| otro);
        let quien = if choque.descripcion.is_empty() {
            tr!("un bind sin descripción").to_owned()
        } else {
            format!("«{}»", choque.descripcion)
        };
        if let Some(g) = i.grabando.borrow_mut().as_mut() {
            g.sub.clone_from(&nueva);
            g.nota = Some(trf!(
                "Ya la usa {}: Enter la reasigna, Escape cancela",
                quien
            ));
            g.pendiente = Some((nueva, libera));
        }
        self.pintar_pagina(false, None);
    }

    fn guardar_tecla(&self, lugar: Lugar, tecla: String, libera: Option<String>) {
        let Some(g) = self.inner.grabando.borrow().as_ref().map(|g| g.id.clone()) else {
            return;
        };
        let mut v = vec![(g, tecla)];
        v.extend(libera.map(|l| (l, String::new())));
        // `ejecutar` deja de grabar (y sale del submap) antes de aplicar.
        self.ejecutar(lugar, &Que::Config(arbol::Cambio::Teclas(v)));
    }

    /// El `Err` va al journal, y la fila lo avisa.
    async fn aplicar(&self, que: Que) -> Result<(), String> {
        match que {
            Que::Aplicar(args) => {
                let a: Vec<&str> = args.iter().map(String::as_str).collect();
                comando::salida(&a).await.map(drop)
            }
            Que::Config(c) => {
                self.cambiar_config(&c)?;
                match c {
                    arbol::Cambio::Capturas(_)
                    | arbol::Cambio::Sombra(_)
                    | arbol::Cambio::Teclas(_)
                    | arbol::Cambio::Multimedia(_)
                    | arbol::Cambio::Regla(..)
                    | arbol::Cambio::NuevaRegla(_)
                    | arbol::Cambio::QuitarRegla(_)
                    | arbol::Cambio::Propio(..)
                    | arbol::Cambio::NuevoPropio { .. }
                    | arbol::Cambio::QuitarPropio(_)
                    | arbol::Cambio::NuevoModo(_)
                    | arbol::Cambio::ModoUnaVez(..)
                    | arbol::Cambio::QuitarModo(_) => {
                        return self.escribir_hypr(None).await;
                    }
                    arbol::Cambio::Animacion(_) => return self.escribir_hypr(None).await,
                    // El shell sigue a la velocidad: el ritmo y la hoja con sus tiempos.
                    arbol::Cambio::Velocidad(id) => {
                        crate::animaciones::poner(&id);
                        let i = &self.inner;
                        i.s.tema.set_shell(&i.config.borrow().shell);
                        return self.escribir_hypr(None).await;
                    }
                    _ => {}
                }
                despues_de_cambiar(&c).await;
                Ok(())
            }
            Que::Hypr(clave, valor) => self.escribir_hypr(Some((clave, valor))).await,
            Que::Monitor(salida, campo) if campo.a_prueba() => {
                self.probar_monitor(salida, campo).await
            }
            Que::Monitor(salida, campo) => self.guardar_monitor(&salida, &campo).await,
            Que::Hora(hora::Cambio::Region(r)) => {
                self.inner.region_horaria.replace(Some(r));
                Ok(())
            }
            Que::Hora(c) => hora::aplicar(c).await,
            Que::MantenerMonitores => self.mantener_monitores().await,
            Que::VolverMonitores => self.volver_monitores().await,
            Que::Apariencia(c) => crate::apariencia::aplicar(c).await,
            Que::Predeterminada(id, mimes) => {
                let app = gio_unix::DesktopAppInfo::new(&id).ok_or(format!("no existe {id}"))?;
                mimes.iter().try_for_each(|m| {
                    app.set_as_default_for_type(m)
                        .map_err(|e| format!("{id} para {m}: {e}"))
                })
            }
            Que::ElegirTemaSonido(carpeta, demo) => {
                sonidos::poner_tema(&carpeta);
                sonidos::sonar(&demo, Some(&carpeta));
                Ok(())
            }
            Que::BorrarRecientes => lectura::borrar_recientes(),
            Que::Autoarranque(archivo, activar) => lectura::poner_autoarranque(&archivo, activar)
                .map_err(|e| format!("autoarranque de {archivo}: {e}")),
            Que::Fondos
            | Que::Ubicacion
            | Que::Calendario
            | Que::Carpetas(_)
            | Que::Sonar(_)
            | Que::GrabarTecla(_) => Ok(()),
        }
    }

    /// Va con el menú cerrado: una capa overlay con el teclado exclusivo lo taparía y le
    /// robaría las teclas.
    fn examinar_carpeta(&self, actual: &std::path::Path) {
        self.cerrar();
        let yo = self.clone();
        let actual = actual.to_owned();
        glib::MainContext::default().spawn_local(async move {
            let elegida = carpeta::elegir("Carpeta de fondos", "Elegir", &actual).await;
            if let Some(ruta) = elegida {
                let c = arbol::Cambio::CarpetaFondos(ruta.to_string_lossy().into_owned());
                if let Err(e) = yo.cambiar_config(&c) {
                    eprintln!("morgan: menú: {e}");
                }
            }
            yo.abrir(NivelMenu::Fondos);
        });
    }

    async fn escribir_hypr(
        &self,
        cambio: Option<(String, Option<compositor::Valor>)>,
    ) -> Result<(), String> {
        let ruta = self.ruta_cambios_hypr();
        if let Some((clave, valor)) = cambio {
            let mut c = compositor::Cambios::leer(&ruta)?;
            c.poner(&clave, valor);
            c.guardar(&ruta)?;
        }
        let config = self.inner.config.borrow().clone();
        crate::hypr_cfg::aplicar(&config, &self.inner.s.ruta_config).await
    }

    fn cambiar_config(&self, cambio: &arbol::Cambio) -> Result<(), String> {
        let i = &self.inner;
        let mut c = i.config.borrow().clone();
        cambio.aplicar(&mut c);
        config::guardar(&i.s.ruta_config, &c).map_err(|e| e.to_string())?;
        // Lo que vale es lo que quedó en el archivo, releído.
        let c = config::load(&i.s.ruta_config).map_err(|e| e.to_string())?;
        i.s.tema.set_shell(&c.shell);
        i.s.tema
            .set_borde(morgan_motor::forma::Borde::de(&c.apariencia.sombra));
        i.s.notificaciones.aplicar_config(&c);
        i.s.idle.aplicar_config(&c.idle);
        i.s.audio.aplicar_config(&c.sonido);
        i.s.sonidos.aplicar_config(&c.sonido);
        i.s.luz.aplicar_config(&c.luz);
        i.s.fondos.aplicar_config(&c.fondos);
        i.s.calendario.aplicar_config(&c.clima);
        i.s.centro.aplicar_config(&c.sonido);
        i.config.replace(c);
        Ok(())
    }
}

async fn despues_de_cambiar(c: &arbol::Cambio) {
    use arbol::Cambio::*;
    match c {
        // Regenera el tema si el activo es el del wallpaper; si no, no hace nada.
        MatugenTipo(_) | MatugenModo(_) | MatugenContraste(_) => {
            leer(&["morganctl", "theme", "wallpaper"]).await;
        }
        _ => {}
    }
}

/// Lo que muestra el índice de una entrada, con lo que coincide con `consulta`.
fn fila_indice(e: &Entrada, consulta: &str) -> FilaIndice {
    let resaltar = |t: &str| {
        if consulta.trim().is_empty() {
            Vec::new()
        } else {
            busqueda::resaltar(t, consulta)
        }
    };
    let (glifo, titulo, donde, seccion, destino) = match e {
        Entrada::Categoria(c) => (Some(c.glifo()), c.titulo().to_owned(), None, None, None),
        Entrada::Seccion(c, s) => (
            None,
            morgan_idioma::t(c.secciones()[*s]).to_owned(),
            None,
            Some(*s),
            Some((*s, None)),
        ),
        Entrada::Resultado(r) => (
            None,
            r.titulo.clone(),
            Some(r.donde.clone()),
            None,
            Some((r.seccion, r.fila)),
        ),
    };
    FilaIndice {
        glifo,
        resaltes: resaltar(&titulo),
        resaltes_donde: donde.as_deref().map(resaltar).unwrap_or_default(),
        titulo,
        donde,
        seccion,
        destino,
    }
}
