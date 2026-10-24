//! El launcher se dibuja en el hilo del centro (`centro/dibujo/launcher.rs`), con lo escrito,
//! la elegida y el scroll; acá quedan los catálogos, la búsqueda y lo que hace cada fila. Lo
//! escrito llega con su turno y la lista vuelve con él: Enter espera a la lista de lo que se ve.
//!
//! Las apps se lanzan en su propia unidad (`uwsm app`, o un scope propio sin uwsm): hijas de
//! morgan, morirían con él.

pub(crate) mod busqueda;
pub mod binds;
pub mod fuentes;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gio::prelude::*;
use morgan_ipc::ModoLauncher as Modo;
use morgan_motor::vello::peniko::ImageData;
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::centro::Centro;
use crate::centro::hilo::Orden;
use crate::centro::vista::{
    EventoLauncher, FilaLauncher, IconoFila, ListaLauncher, TipoFila, Turno,
};
use crate::pixeles;
use crate::{comando, icono};
use busqueda::{Entrada, Historial, Orden as OrdenBusqueda};

/// Lo que se espera a que el foco vuelva a la ventana de antes para escribirle un símbolo o
/// correr un atajo.
const DEVOLVER_FOCO: Duration = Duration::from_millis(250);

const PAUSA_CIUDAD: Duration = Duration::from_millis(300);

/// El lado de los íconos de las filas.
const ICONO: i32 = 24;

#[derive(Clone)]
pub struct Launcher {
    inner: Rc<Inner>,
}

/// Los grupos de Unicode, en su orden: sin texto, los emoji se ven por carpeta.
const GRUPOS: [(&str, &str); 9] = [
    ("Smileys & Emotion", tr_const!("Caras y emociones")), // no-tr
    ("People & Body", tr_const!("Personas y cuerpo")), // no-tr
    ("Animals & Nature", tr_const!("Animales y naturaleza")), // no-tr
    ("Food & Drink", tr_const!("Comida y bebida")), // no-tr
    ("Travel & Places", tr_const!("Viajes y lugares")), // no-tr
    ("Activities", tr_const!("Actividades")), // no-tr
    ("Objects", tr_const!("Objetos")), // no-tr
    ("Symbols", tr_const!("Símbolos")), // no-tr
    ("Flags", tr_const!("Banderas")), // no-tr
];
/// Los usados, arriba de las carpetas.
const RECIENTES: usize = 8;
/// Una búsqueda corta («a») no vuelve a traer la lista entera.
const TOPE_BUSQUEDA: usize = 200;

struct Inner {
    hilo: Sender<Orden>,
    modo: Cell<Modo>,
    abierto: Cell<bool>,
    apps: RefCell<Catalogo>,
    apps_sucias: Cell<bool>,
    simbolos: RefCell<Catalogo>,
    portapapeles: RefCell<Catalogo>,
    atajos: RefCell<Catalogo>,
    ciudades: RefCell<Catalogo>,
    calendario: RefCell<Catalogo>,
    agenda: crate::agenda::Agenda,
    al_menu: RefCell<Option<AlMenu>>,
    visibles: RefCell<Vec<usize>>,
    uso_apps: RefCell<Uso>,
    uso_simbolos: RefCell<Uso>,
    /// La carpeta de emoji abierta, si hay.
    carpeta: Cell<Option<usize>>,
    /// Sube con cada carga o cálculo pedido: una respuesta vieja que llega tarde se descarta.
    carga: Cell<u64>,
    /// Lo escrito en el launcher y su turno. La sesión sube con cada apertura o vaciado: lo
    /// escrito antes, que llegue tarde, no cuenta.
    texto: RefCell<String>,
    turno: Cell<Turno>,
    sesion: Cell<u64>,
    /// Sube con cada lista mandada: lo elegido se reconoce por ella.
    version: Cell<u64>,
    /// El campo se vacía con la próxima lista, con este placeholder.
    vaciar: RefCell<Option<String>>,
    /// Los íconos del tema por nombre (los de las filas que explican, el teclado de los atajos).
    iconos: RefCell<HashMap<String, Option<ImageData>>>,
    _monitor: gio::AppInfoMonitor,
}

pub enum ParaElMenu {
    Ciudad(Option<crate::config::Clima>),
    Calendario,
}

type AlMenu = Rc<dyn Fn(ParaElMenu)>;

#[derive(Default)]
struct Catalogo {
    entradas: Vec<Entrada>,
    filas: Vec<FilaLauncher>,
    acciones: Vec<Accion>,
    /// Solo en los emoji: el grupo de cada uno.
    grupos: Vec<Option<usize>>,
    cargado: bool,
}

impl Catalogo {
    fn agregar(&mut self, entrada: Entrada, fila: FilaLauncher, accion: Accion) {
        self.agregar_en(entrada, fila, accion, None);
    }

    fn agregar_en(
        &mut self,
        entrada: Entrada,
        fila: FilaLauncher,
        accion: Accion,
        grupo: Option<usize>,
    ) {
        self.entradas.push(entrada);
        self.filas.push(fila);
        self.acciones.push(accion);
        self.grupos.push(grupo);
    }
}

#[derive(Clone)]
enum Accion {
    App(String),
    Simbolo(String),
    /// Una carpeta de emoji: la abre, no cierra el launcher.
    Carpeta(usize),
    Recorte(String),
    /// La referencia del bind se vuelve a buscar al ejecutarlo.
    Atajo(String, String),
    Ciudad(crate::clima::Lugar),
    /// Lo pegado, tal cual: se busca así en el historial del portapapeles.
    Calendario(String),
    QuitarCalendario(String),
    /// Una fila que explica.
    Ninguna,
}

impl Accion {
    fn tipo(&self) -> TipoFila {
        match self {
            Accion::Carpeta(_) | Accion::Calendario(_) => TipoFila::Queda,
            Accion::Ninguna => TipoFila::Nada,
            _ => TipoFila::Cierra,
        }
    }
}

struct Uso {
    historial: Historial,
    archivo: Option<PathBuf>,
}

impl Uso {
    fn sumar(&mut self, id: &str) {
        self.historial.sumar(id);
        if let Some(a) = &self.archivo {
            guardar(a, &self.historial.texto());
        }
    }
}

enum Icono<'a> {
    App(Option<&'a gio::Icon>),
    Nombre(&'a str),
    Ninguno,
}

impl Launcher {
    pub fn new(centro: &Centro, agenda: &crate::agenda::Agenda) -> Self {
        let estado = crate::notificaciones::dir_from(|k| std::env::var_os(k));
        let archivo = |n: &str| estado.as_ref().map(|d| d.join(n));
        let uso_apps = Uso {
            historial: leer_historial(archivo("launcher-apps").as_deref()),
            archivo: archivo("launcher-apps"),
        };
        let uso_simbolos = Uso {
            historial: leer_historial(archivo("launcher-simbolos").as_deref()),
            archivo: archivo("launcher-simbolos"),
        };
        let monitor = gio::AppInfoMonitor::get();
        let inner = Rc::new(Inner {
            hilo: centro.hilo(),
            modo: Cell::new(Modo::Apps),
            abierto: Cell::new(false),
            apps: RefCell::default(),
            apps_sucias: Cell::new(true),
            simbolos: RefCell::default(),
            portapapeles: RefCell::default(),
            atajos: RefCell::default(),
            ciudades: RefCell::default(),
            calendario: RefCell::default(),
            agenda: agenda.clone(),
            al_menu: RefCell::default(),
            visibles: RefCell::default(),
            uso_apps: RefCell::new(uso_apps),
            uso_simbolos: RefCell::new(uso_simbolos),
            carpeta: Cell::new(None),
            carga: Cell::new(0),
            texto: RefCell::default(),
            turno: Cell::new((0, 0)),
            sesion: Cell::new(0),
            version: Cell::new(0),
            vaciar: RefCell::default(),
            iconos: RefCell::default(),
            _monitor: monitor.clone(),
        });
        let l = Launcher { inner };
        // Cerrado no hace falta rehacer nada: se marca y se lee al abrir.
        let weak = Rc::downgrade(&l.inner);
        monitor.connect_changed(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.apps_sucias.set(true);
                let l = Launcher { inner };
                if l.abierto() && l.inner.modo.get() == Modo::Apps {
                    l.leer_apps();
                    l.filtrar();
                }
            }
        });
        let weak = Rc::downgrade(&l.inner);
        centro.al_launcher(move |e| {
            if let Some(inner) = weak.upgrade() {
                Launcher { inner }.evento(e);
            }
        });
        l
    }

    /// Las apps con sus íconos, leídas antes de la primera apertura: que abra tan rápido como
    /// las siguientes.
    pub fn precalentar(&self) {
        let yo = self.clone();
        glib::idle_add_local_once(move || {
            if yo.inner.apps_sucias.get() {
                yo.leer_apps();
            }
        });
    }

    pub fn al_menu(&self, f: impl Fn(ParaElMenu) + 'static) {
        self.inner.al_menu.replace(Some(Rc::new(f)));
    }

    fn volver_al_menu(&self, p: ParaElMenu) {
        let f = self.inner.al_menu.borrow().clone();
        if let Some(f) = f {
            f(p);
        }
    }

    fn mandar(&self, o: Orden) {
        // Si el hilo del centro terminó (sin Wayland o sin GPU) ya lo dijo al terminar.
        let _ = self.inner.hilo.send(o);
    }

    pub fn abierto(&self) -> bool {
        self.inner.abierto.get()
    }

    pub fn alternar(&self, modo: Modo) {
        if self.abierto() && self.inner.modo.get() == modo {
            self.cerrar();
        } else {
            self.abrir(modo);
        }
    }

    pub fn abrir(&self, modo: Modo) {
        let i = &self.inner;
        if i.abierto.get() && i.modo.get() == modo {
            return;
        }
        i.abierto.set(true);
        i.modo.set(modo);
        i.carpeta.set(None);
        i.carga.set(i.carga.get() + 1);
        self.mandar(Orden::LauncherAbrir {
            prompt: modo.prompt().to_owned(),
            simbolos: modo == Modo::Simbolos,
        });
        // Cada apertura (o cambio de modo) empieza de cero.
        self.vaciar_texto(match modo {
            Modo::Ciudad => tr!("Nombre de la ciudad…"),
            Modo::Calendario => tr!("Dirección iCal secreta…"),
            _ => tr!("Buscar…"),
        });
        match modo {
            Modo::Apps => {
                if i.apps_sucias.get() {
                    self.leer_apps();
                }
                self.filtrar();
            }
            Modo::Simbolos => {
                if !i.simbolos.borrow().cargado {
                    self.leer_simbolos();
                }
                self.filtrar();
            }
            Modo::Portapapeles | Modo::Atajos => {
                self.catalogo(modo).replace(Catalogo::default());
                self.pintar(Vec::new(), "", 0);
                self.cargar(modo);
            }
            // Se llena al tipear (`buscar_ciudad`).
            Modo::Ciudad => {
                i.ciudades.replace(Catalogo::default());
                self.pintar(Vec::new(), "", 0);
            }
            Modo::Calendario => self.url_pegada(),
        }
    }

    pub fn cerrar(&self) {
        let i = &self.inner;
        if !i.abierto.replace(false) {
            return;
        }
        self.mandar(Orden::LauncherCerrar);
        self.cerrado();
    }

    /// Lo de después de cerrar, lo haya cerrado quien sea.
    fn cerrado(&self) {
        let i = &self.inner;
        i.carga.set(i.carga.get() + 1);
        // Lo pegado no se queda guardado.
        if i.modo.get() == Modo::Calendario {
            i.texto.borrow_mut().clear();
            i.calendario.replace(Catalogo::default());
        }
    }

    fn evento(&self, e: EventoLauncher) {
        let i = &self.inner;
        match e {
            EventoLauncher::Texto(t, s) => {
                if !i.abierto.get() || t.0 != i.sesion.get() {
                    return;
                }
                i.turno.set(t);
                i.texto.replace(s);
                self.filtrar();
            }
            EventoLauncher::Elegido { version, indice } => {
                if i.abierto.get() && version == i.version.get() {
                    self.elegir(indice);
                }
            }
            EventoLauncher::SalirCarpeta => {
                self.cerrar_carpeta();
            }
            EventoLauncher::Cerrado { escape } => {
                if !i.abierto.replace(false) {
                    return;
                }
                let modo = i.modo.get();
                self.cerrado();
                // Desde el menú: Escape vuelve a él, como en la banda de fondos.
                if escape {
                    match modo {
                        Modo::Ciudad => self.volver_al_menu(ParaElMenu::Ciudad(None)),
                        Modo::Calendario => self.volver_al_menu(ParaElMenu::Calendario),
                        _ => {}
                    }
                }
            }
        }
    }

    /// El campo vuelve a vacío (con la próxima lista) y lo escrito antes no cuenta.
    fn vaciar_texto(&self, placeholder: &str) {
        let i = &self.inner;
        i.sesion.set(i.sesion.get() + 1);
        i.turno.set((i.sesion.get(), 0));
        i.texto.borrow_mut().clear();
        i.vaciar.replace(Some(placeholder.to_owned()));
    }

    fn texto(&self) -> String {
        self.inner.texto.borrow().clone()
    }

    fn catalogo(&self, modo: Modo) -> &RefCell<Catalogo> {
        let i = &self.inner;
        match modo {
            Modo::Apps => &i.apps,
            Modo::Simbolos => &i.simbolos,
            Modo::Portapapeles => &i.portapapeles,
            Modo::Atajos => &i.atajos,
            Modo::Ciudad => &i.ciudades,
            Modo::Calendario => &i.calendario,
        }
    }

    fn fila(
        &self,
        icono: Icono,
        nombre: String,
        generico: Option<String>,
        accion: &Accion,
    ) -> FilaLauncher {
        let icono = match icono {
            Icono::App(i) => {
                icono::de_gicon(i, ICONO as u32, pixeles::escala() as u32).map(IconoFila::Imagen)
            }
            Icono::Nombre(n) => {
                let img = self
                    .inner
                    .iconos
                    .borrow_mut()
                    .entry(n.to_owned())
                    .or_insert_with(|| {
                        icono::de_nombre(n, ICONO as u32, pixeles::escala() as u32, [1.0; 4])
                    })
                    .clone();
                // Los simbólicos se pintan del color del texto, como los pintaba GTK.
                img.map(|i| {
                    if n.ends_with("-symbolic") {
                        IconoFila::Simbolico(i)
                    } else {
                        IconoFila::Imagen(i)
                    }
                })
            }
            Icono::Ninguno => None,
        };
        FilaLauncher {
            icono,
            nombre,
            generico,
            resaltes: Vec::new(),
            resaltes_generico: Vec::new(),
            tipo: accion.tipo(),
            carpeta: matches!(accion, Accion::Carpeta(_)),
        }
    }

    fn agregar(
        &self,
        c: &mut Catalogo,
        entrada: Entrada,
        icono: Icono,
        nombre: String,
        generico: Option<String>,
        accion: Accion,
    ) {
        let f = self.fila(icono, nombre, generico, &accion);
        c.agregar(entrada, f, accion);
    }

    fn leer_apps(&self) {
        let i = &self.inner;
        i.apps_sucias.set(false);
        let mut c = Catalogo::default();
        for a in gio::AppInfo::all() {
            if !a.should_show() {
                continue;
            }
            let Ok(d) = a.downcast::<gio_unix::DesktopAppInfo>() else {
                continue;
            };
            let Some(id) = d.id() else {
                continue;
            };
            let nombre = d.display_name().to_string();
            let generico = d
                .generic_name()
                .map(|g| g.to_string())
                .filter(|g| !g.is_empty() && *g != nombre);
            let palabras: Vec<String> = d.keywords().iter().map(|k| k.to_string()).collect();
            let categorias = d.categories().map(|c| c.to_string()).unwrap_or_default();
            let ejecutable = d.executable();
            let ejecutable = ejecutable
                .file_name()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut campos: Vec<&str> =
                vec![generico.as_deref().unwrap_or(""), &categorias, &ejecutable];
            campos.extend(palabras.iter().map(String::as_str));
            let entrada = Entrada::new(&id, &nombre, &campos);
            self.agregar(
                &mut c,
                entrada,
                Icono::App(d.icon().as_ref()),
                nombre,
                generico,
                Accion::App(id.to_string()),
            );
        }
        c.cargado = true;
        i.apps.replace(c);
    }

    fn leer_simbolos(&self) {
        let mut c = Catalogo {
            cargado: true,
            ..Default::default()
        };
        let ingles = morgan_idioma::actual() == morgan_idioma::Idioma::En;
        let mut muestra: [Option<String>; GRUPOS.len()] = Default::default();
        let mut cuantos = [0usize; GRUPOS.len()];
        for s in fuentes::simbolos(fuentes::SIMBOLOS, ingles) {
            let grupo = GRUPOS.iter().position(|(g, _)| *g == s.grupo);
            if let Some(g) = grupo {
                cuantos[g] += 1;
                muestra[g].get_or_insert_with(|| s.caracter.clone());
            }
            let vista = format!("{}  {}", s.caracter, s.nombre);
            let generico = (!s.claves.is_empty()).then(|| s.claves.clone());
            let accion = Accion::Simbolo(s.caracter.clone());
            let f = self.fila(Icono::Ninguno, vista.clone(), generico, &accion);
            c.agregar_en(
                Entrada::new(&s.caracter, &vista, &[&s.claves, &s.otro]),
                f,
                accion,
                grupo,
            );
        }
        for (g, (_, nombre)) in GRUPOS.iter().enumerate() {
            let nombre = morgan_idioma::t(nombre);
            let vista = format!("{}  {nombre}", muestra[g].as_deref().unwrap_or("·"));
            self.agregar(
                &mut c,
                Entrada::new(&format!("carpeta:{g}"), &vista, &[]), // no-tr
                Icono::Ninguno,
                vista,
                Some(trf!("{} emoji", cuantos[g])),
                Accion::Carpeta(g),
            );
        }
        self.inner.simbolos.replace(c);
    }

    /// Portapapeles y atajos se releen en cada apertura: cambian todo el tiempo.
    fn cargar(&self, modo: Modo) {
        let carga = self.inner.carga.get();
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let args: &[&str] = match modo {
                Modo::Portapapeles => &["cliphist", "list"],
                _ => &["hyprctl", "-j", "binds"],
            };
            let salida = comando::salida(args).await;
            if yo.inner.carga.get() != carga {
                return;
            }
            let mut c = Catalogo {
                cargado: true,
                ..Default::default()
            };
            match (modo, salida) {
                (Modo::Portapapeles, Ok(t)) => {
                    for r in fuentes::recortes(&t).into_iter().take(250) {
                        yo.agregar(
                            &mut c,
                            Entrada::new(&r.linea, &r.vista, &[]),
                            Icono::Ninguno,
                            r.vista,
                            None,
                            Accion::Recorte(r.linea),
                        );
                    }
                }
                (_, Ok(t)) => match fuentes::atajos(&t) {
                    Ok(v) => {
                        for a in v {
                            yo.agregar(
                                &mut c,
                                Entrada::new(&a.descripcion, &a.descripcion, &[&a.teclas]),
                                Icono::Nombre("input-keyboard"),
                                a.descripcion.clone(),
                                Some(a.teclas.clone()),
                                Accion::Atajo(a.descripcion, a.teclas),
                            );
                        }
                    }
                    Err(e) => eprintln!("morgan: launcher: {e}"),
                },
                (_, Err(e)) => eprintln!("morgan: launcher: {e}"),
            }
            yo.catalogo(modo).replace(c);
            yo.filtrar();
        });
    }

    fn filtrar(&self) {
        match self.inner.modo.get() {
            Modo::Apps => {
                let uso = self.inner.uso_apps.borrow();
                self.mostrar(OrdenBusqueda::Uso(&uso.historial), true);
            }
            Modo::Simbolos => self.mostrar_simbolos(),
            Modo::Portapapeles => self.mostrar(OrdenBusqueda::Original, true),
            // Ya vienen ordenados por descripción.
            Modo::Atajos => self.mostrar(OrdenBusqueda::Original, true),
            Modo::Ciudad => self.buscar_ciudad(),
            Modo::Calendario => self.url_pegada(),
        }
    }

    /// Sin texto, para qué sirve y los que ya están; con texto, si sirve. La URL completa no
    /// se muestra: es una credencial.
    fn url_pegada(&self) {
        let texto = self.texto().trim().to_owned();
        let mut c = Catalogo {
            cargado: true,
            ..Default::default()
        };
        let agregar = |c: &mut Catalogo, icono, nombre: &str, generico: Option<String>, accion| {
            self.agregar(
                c,
                Entrada::new(nombre, nombre, &[]),
                Icono::Nombre(icono),
                nombre.to_owned(),
                generico,
                accion,
            );
        };
        if texto.is_empty() {
            let calendarios = crate::agenda::calendarios();
            agregar(
                &mut c,
                "x-office-calendar-symbolic",
                tr!("Muestra los eventos de un calendario en línea"),
                Some(tr!("solo lectura").into()),
                Accion::Ninguna,
            );
            agregar(
                &mut c,
                "edit-paste-symbolic",
                tr!("Pegar acá la dirección iCal secreta"),
                Some(tr!("Google, Outlook, iCloud…").into()),
                Accion::Ninguna,
            );
            agregar(
                &mut c,
                "dialog-information-symbolic",
                tr!("En Google: Integrar el calendario"),
                Some(tr!("Dirección secreta en formato iCal").into()),
                Accion::Ninguna,
            );
            for cal in calendarios {
                let leido = match cal.leido.and_then(|d| d.format("%H:%M").ok()) {
                    Some(h) => trf!("en uso · leído a las {}", h),
                    None => tr!("en uso · todavía sin leer").into(),
                };
                agregar(
                    &mut c,
                    "list-remove-symbolic",
                    &trf!("Quitar {}", cal.nombre),
                    Some(leido),
                    Accion::QuitarCalendario(cal.id),
                );
            }
        } else if let Some(dominio) = crate::agenda::dominio(&texto) {
            agregar(
                &mut c,
                "list-add-symbolic",
                tr!("Agregar este calendario"),
                Some(dominio),
                Accion::Calendario(texto),
            );
        } else {
            agregar(
                &mut c,
                "dialog-warning-symbolic",
                tr!("No es una dirección de calendario"),
                Some(tr!("empieza con https:// o webcal://").into()),
                Accion::Ninguna,
            );
        }
        self.inner.calendario.replace(c);
        self.mostrar(OrdenBusqueda::Original, false);
    }

    /// Se prueba antes de guardarla; mientras, una fila lo dice, y si falla, por qué.
    fn agregar_calendario(&self, texto: String) {
        let i = &self.inner;
        i.carga.set(i.carga.get() + 1);
        let carga = i.carga.get();
        self.aviso_calendario("content-loading-symbolic", tr!("Probando la dirección…"), None);
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let r = yo.inner.agenda.agregar(&texto).await;
            if yo.inner.carga.get() != carga || !yo.abierto() {
                return;
            }
            match r {
                Ok(()) => {
                    yo.cerrar();
                    yo.volver_al_menu(ParaElMenu::Calendario);
                }
                Err(e) => yo.aviso_calendario(
                    "dialog-warning-symbolic",
                    tr!("No se agregó"),
                    Some(e),
                ),
            }
        });
    }

    fn aviso_calendario(&self, icono: &'static str, nombre: &str, generico: Option<String>) {
        let mut c = Catalogo {
            cargado: true,
            ..Default::default()
        };
        self.agregar(
            &mut c,
            Entrada::new(nombre, nombre, &[]),
            Icono::Nombre(icono),
            nombre.to_owned(),
            generico,
            Accion::Ninguna,
        );
        self.inner.calendario.replace(c);
        self.mostrar(OrdenBusqueda::Original, false);
    }

    /// Cuando se deja de tipear un momento: una consulta por tecla sería un pedido a la red por
    /// letra.
    fn buscar_ciudad(&self) {
        let i = &self.inner;
        i.carga.set(i.carga.get() + 1);
        let carga = i.carga.get();
        let texto = self.texto();
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            glib::timeout_future(PAUSA_CIUDAD).await;
            if yo.inner.carga.get() != carga {
                return;
            }
            let lugares = crate::clima::buscar(&texto).await;
            if yo.inner.carga.get() != carga {
                return;
            }
            let mut c = Catalogo {
                cargado: true,
                ..Default::default()
            };
            for l in lugares {
                let region = (!l.region.is_empty()).then(|| l.region.clone());
                yo.agregar(
                    &mut c,
                    Entrada::new(&l.nombre, &l.nombre, &[]),
                    Icono::Nombre("mark-location-symbolic"),
                    l.nombre.clone(),
                    region,
                    Accion::Ciudad(l),
                );
            }
            yo.inner.ciudades.replace(c);
            // El geocoder ya filtró (y entiende cosas que la búsqueda local no, como «Bs As»).
            yo.mostrar(OrdenBusqueda::Original, false);
        });
    }

    fn mostrar(&self, orden: OrdenBusqueda, filtrando: bool) {
        let i = &self.inner;
        let consulta = if filtrando { self.texto() } else { String::new() };
        let visibles = busqueda::filtrar(
            &self.catalogo(i.modo.get()).borrow().entradas,
            &consulta,
            orden,
        );
        self.pintar(visibles, &consulta, 0);
    }

    /// Con texto, los que coinciden de todos los grupos (hasta un tope); sin texto, la carpeta
    /// abierta o, si no hay, los usados y las carpetas.
    fn mostrar_simbolos(&self) {
        let visibles = self.visibles_simbolos();
        self.pintar(visibles, &self.texto(), 0);
    }

    fn visibles_simbolos(&self) -> Vec<usize> {
        let i = &self.inner;
        let consulta = self.texto();
        let uso = i.uso_simbolos.borrow();
        let c = i.simbolos.borrow();
        let es_carpeta = |&v: &usize| matches!(c.acciones[v], Accion::Carpeta(_));
        if !consulta.trim().is_empty() {
            busqueda::filtrar(&c.entradas, &consulta, OrdenBusqueda::Uso(&uso.historial))
                .into_iter()
                .filter(|v| !es_carpeta(v))
                .take(TOPE_BUSQUEDA)
                .collect()
        } else if let Some(g) = i.carpeta.get() {
            (0..c.grupos.len()).filter(|&v| c.grupos[v] == Some(g)).collect()
        } else {
            let mut usados: Vec<usize> = (0..c.entradas.len())
                .filter(|&v| !es_carpeta(&v) && uso.historial.usos(&c.entradas[v].id) > 0)
                .collect();
            usados.sort_by_key(|&v| std::cmp::Reverse(uso.historial.usos(&c.entradas[v].id)));
            usados.truncate(RECIENTES);
            usados.extend((0..c.acciones.len()).filter(es_carpeta));
            usados
        }
    }

    /// Le manda al hilo del centro las filas de `visibles`, con lo que coincide con `consulta`.
    fn pintar(&self, visibles: Vec<usize>, consulta: &str, elegida: usize) {
        let i = &self.inner;
        let c = self.catalogo(i.modo.get()).borrow();
        let filas = visibles
            .iter()
            .filter_map(|&v| c.filas.get(v))
            .map(|f| {
                let mut f = f.clone();
                if !consulta.is_empty() {
                    f.resaltes = busqueda::resaltar(&f.nombre, consulta);
                    f.resaltes_generico = f
                        .generico
                        .as_deref()
                        .map(|g| busqueda::resaltar(g, consulta))
                        .unwrap_or_default();
                }
                f
            })
            .collect();
        drop(c);
        i.visibles.replace(visibles);
        i.version.set(i.version.get() + 1);
        self.mandar(Orden::LauncherLista(ListaLauncher {
            texto: i.turno.get(),
            version: i.version.get(),
            filas,
            elegida,
            en_carpeta: i.carpeta.get().is_some(),
            vaciar: i.vaciar.take(),
        }));
    }

    fn abrir_carpeta(&self, g: usize) {
        self.inner.carpeta.set(Some(g));
        self.vaciar_texto(morgan_idioma::t(GRUPOS[g].1));
        self.mostrar_simbolos();
    }

    /// De vuelta a las carpetas, con la que estaba abierta elegida.
    fn cerrar_carpeta(&self) {
        let i = &self.inner;
        let Some(g) = i.carpeta.take() else { return };
        self.vaciar_texto(tr!("Buscar…"));
        let visibles = self.visibles_simbolos();
        let id = format!("carpeta:{g}"); // no-tr
        let posicion = {
            let c = i.simbolos.borrow();
            visibles.iter().position(|&v| c.entradas[v].id == id)
        };
        self.pintar(visibles, "", posicion.unwrap_or(0));
    }

    fn elegir(&self, posicion: usize) {
        let i = &self.inner;
        let modo = i.modo.get();
        let Some(accion) = i
            .visibles
            .borrow()
            .get(posicion)
            .copied()
            .and_then(|e| self.catalogo(modo).borrow().acciones.get(e).cloned())
        else {
            return;
        };
        self.ejecutar(accion);
    }

    fn ejecutar(&self, accion: Accion) {
        let i = &self.inner;
        match accion {
            // Quedan abiertos: se prueba, o se abre la carpeta.
            Accion::Calendario(texto) => {
                self.agregar_calendario(texto);
                return;
            }
            Accion::Carpeta(g) => {
                self.abrir_carpeta(g);
                return;
            }
            Accion::Ninguna => return,
            _ => {}
        }
        self.cerrar();
        match accion {
            Accion::App(id) => {
                comando::lanzar_app(&id, &[]);
                i.uso_apps.borrow_mut().sumar(&id);
            }
            // Se escribe en la ventana que tenía el foco.
            Accion::Simbolo(s) => {
                i.uso_simbolos.borrow_mut().sumar(&s);
                glib::timeout_add_local_once(DEVOLVER_FOCO, move || {
                    comando::lanzar(&["wtype", "--", &s]);
                });
            }
            Accion::Recorte(linea) => comando::lanzar_con_entrada(
                &["sh", "-c", "cliphist decode | wl-copy"],
                linea.as_bytes(),
            ),
            Accion::Ciudad(l) => self.volver_al_menu(ParaElMenu::Ciudad(Some(crate::config::Clima {
                lugar: l.nombre,
                latitud: Some(l.latitud),
                longitud: Some(l.longitud),
            }))),
            Accion::QuitarCalendario(id) => {
                if let Err(e) = i.agenda.quitar(&id) {
                    eprintln!("morgan: agenda: {e}");
                }
                self.volver_al_menu(ParaElMenu::Calendario);
            }
            Accion::Calendario(_) | Accion::Carpeta(_) | Accion::Ninguna => {}
            // Releídos al ejecutar: entre abrir y elegir, Hyprland pudo releer la config y
            // la referencia de entonces ser de otra acción.
            Accion::Atajo(descripcion, teclas) => {
                glib::timeout_add_local_once(DEVOLVER_FOCO, move || {
                    glib::MainContext::default().spawn_local(async move {
                        let binds = comando::salida(&["hyprctl", "-j", "binds"]).await;
                        let atajo = binds.and_then(|t| fuentes::binds(&t)).ok().and_then(|v| {
                            v.into_iter()
                                .find(|a| a.descripcion == descripcion && a.teclas == teclas)
                        });
                        match atajo.and_then(|a| a.referencia) {
                            Some(r) => comando::lanzar(&["hyprctl", "eval", &fuentes::ejecutar_bind(r)]),
                            None => eprintln!("morgan: launcher: {descripcion}: el bind ya no está"),
                        }
                    });
                });
            }
        }
    }
}

fn leer_historial(archivo: Option<&Path>) -> Historial {
    archivo
        .and_then(|a| std::fs::read_to_string(a).ok())
        .map(|t| Historial::parse(&t))
        .unwrap_or_default()
}

/// Escribe a un temporal y lo renombra: un corte a mitad de camino no deja el archivo roto.
fn guardar(archivo: &Path, texto: &str) {
    let r = (|| {
        if let Some(dir) = archivo.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = archivo.with_extension("tmp");
        std::fs::write(&tmp, texto)?;
        std::fs::rename(&tmp, archivo)
    })();
    if let Err(e) = r {
        eprintln!(
            "morgan: launcher: no se pudo guardar {}: {e}",
            archivo.display()
        );
    }
}
