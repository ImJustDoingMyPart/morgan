//! La banda de fondos con las piezas del centro: una tira de miniaturas centrada en la elegida,
//! con el anillo bicolor de `forma::anillo` y el glow de las sombras. Qué imágenes hay, y
//! aplicar la elegida, lo lleva el hilo de GTK (`fondos/`); acá viven la elegida, lo escrito en
//! el filtro, el desplazamiento y las miniaturas, que lee un hilo propio mientras la banda está
//! abierta (`trabajar`).
//!
//! Las medidas, de la versión con GTK (`.fondos` en `estilo.css` hasta 0.1.1): 14 de relleno
//! arriba, la cabecera con 24 a los costados y 10 entre el prompt y el filtro, la celda con 3
//! de anillo y 6 de radio (4 la foto), el glow de la elegida de 8 con spread 1.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use morgan_motor::color::{Paleta, con_alfa};
use morgan_motor::forma::Borde;
use morgan_motor::texto;
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Rect};
use morgan_motor::vello::peniko::ImageData;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use smithay_client_toolkit::seat::keyboard::Keysym;

use super::trazo::{self, Degrade, Trazo};
use super::{Animos, Objetivo, Rotulos};
use crate::centro::vista::{EventoFondos, VistaFondos};
use crate::fondos::{modelo, motor};

const RELLENO_ARRIBA: f64 = 14.0;
const RELLENO_X: f64 = 24.0;
const ENTRE_PROMPT: f64 = 10.0;
const ENTRE_CABECERA: f64 = 8.0;
const PX: f32 = 14.0;
const RADIO_CELDA: f64 = 6.0;
const RADIO_FOTO: f64 = 4.0;
const ANILLO: f64 = 3.0;
const DURACION: Duration = Duration::from_millis(220);
/// Cuántas celdas a cada lado de la elegida tienen su foto en memoria.
const CERCA: usize = 12;
/// Lo que suma la rueda de un touchpad (en puntos) para pasar de una imagen a la siguiente.
const PASO_RUEDA: f64 = 40.0;
/// El lado, en píxeles, con que se cargan las miniaturas (`motor::LADO_MINIATURA`).
const LADO: u32 = 256;

/// Una miniatura leída, de la apertura `generacion`.
pub struct Mini {
    pub generacion: u64,
    pub ruta: PathBuf,
    pub img: ImageData,
}

/// Lo que el lector de miniaturas tiene que hacer ahora: las de `orden` (más cercanas primero),
/// las primeras `ventana` con su foto.
struct Pedido {
    orden: Vec<PathBuf>,
    ventana: usize,
}

struct Banda {
    carpeta: String,
    todos: Vec<PathBuf>,
    visibles: Vec<usize>,
    filtro: String,
    sel: usize,
    actual: Option<PathBuf>,
    /// El desplazamiento en celdas (la posición de la tira): de dónde, a dónde y desde cuándo.
    anim: (f64, f64, Instant),
    alto_salida: i32,
    minis: HashMap<PathBuf, ImageData>,
    trabajo: mpsc::Sender<Pedido>,
    generacion: u64,
    volver: bool,
    apretado: Option<usize>,
    /// Lo que va sumando una rueda continua.
    rueda: f64,
    /// Las celdas del último cuadro, para responder al puntero.
    celdas: Vec<(usize, Rect)>,
    trazos: Vec<Trazo>,
}

pub struct FondosUi {
    pub paleta: Paleta,
    pub opacidad: f32,
    pub borde: Borde,
    pub tam: (u32, u32),
    rotulos: Rotulos,
    animos: Animos,
    abierta: Option<Banda>,
    generacion: u64,
}

impl FondosUi {
    pub fn new(paleta: Paleta, opacidad: f32, borde: Borde) -> Self {
        FondosUi {
            paleta,
            opacidad,
            borde,
            tam: (0, 0),
            rotulos: Rotulos::new(),
            animos: Animos::default(),
            abierta: None,
            generacion: 0,
        }
    }

    /// El alto de la banda en una salida de `alto_salida`.
    pub fn alto(alto_salida: i32) -> u32 {
        modelo::geometria(1, alto_salida).alto.max(1) as u32
    }

    /// Abre con la elegida en el fondo de ahora; las miniaturas llegan por `minis`.
    pub fn abrir(&mut self, v: VistaFondos, alto_salida: i32, minis: Sender<Mini>) {
        self.cerrar();
        self.generacion += 1;
        let (trabajo, rx) = mpsc::channel();
        let (cache, faltan, generacion) = (v.cache, v.faltan, self.generacion);
        let _ = std::thread::Builder::new()
            .name("morgan-fondos".into())
            .spawn(move || trabajar(&rx, &minis, generacion, &cache, faltan.into_iter().collect()));
        let sel = v
            .actual
            .as_ref()
            .and_then(|a| v.rutas.iter().position(|t| t == a))
            .unwrap_or(0);
        let visibles = (0..v.rutas.len()).collect();
        self.abierta = Some(Banda {
            carpeta: v.carpeta,
            todos: v.rutas,
            visibles,
            filtro: String::new(),
            sel,
            actual: v.actual,
            anim: (sel as f64, sel as f64, Instant::now()),
            alto_salida,
            minis: HashMap::new(),
            trabajo,
            generacion,
            volver: v.volver,
            apretado: None,
            rueda: 0.0,
            celdas: Vec::new(),
            trazos: Vec::new(),
        });
        self.pedir();
    }

    /// Lo de la apertura se suelta, y con ello el hilo que lee: termina al ver el canal cerrado.
    pub fn cerrar(&mut self) {
        if self.abierta.take().is_some() {
            self.animos = Animos::default();
            // glibc se queda con lo que se liberó al decodificar las miniaturas en vez de
            // devolverlo.
            unsafe {
                libc::malloc_trim(0);
            }
        }
    }

    pub fn mini(&mut self, m: Mini) {
        if let Some(b) = &mut self.abierta
            && b.generacion == m.generacion
        {
            b.minis.insert(m.ruta, m.img);
        }
    }

    /// Qué miniaturas hacen falta ahora: las de alrededor de la elegida, más cercanas primero.
    /// Lo que quedó lejos se suelta.
    fn pedir(&mut self) {
        let Some(b) = &mut self.abierta else { return };
        let n = b.visibles.len();
        let orden: Vec<PathBuf> = modelo::orden_de_carga(n, b.sel)
            .into_iter()
            .map(|i| b.todos[b.visibles[i]].clone())
            .collect();
        let ventana = (2 * CERCA + 1).min(n);
        let cerca: HashSet<&PathBuf> = orden.iter().take(ventana).collect();
        b.minis.retain(|r, _| cerca.contains(r));
        let _ = b.trabajo.send(Pedido { orden, ventana });
    }

    fn escala(&self) -> f64 {
        f64::from(self.rotulos.escala())
    }

    fn ancho(&self, l: &morgan_motor::parley::Layout<u8>) -> f64 {
        texto::caja(l).0 / self.escala()
    }

    fn alto_linea(&self, l: &morgan_motor::parley::Layout<u8>) -> f64 {
        texto::alto_de_linea(l) / self.escala()
    }

    /// La posición de la tira, en celdas, con la curva de salida suave.
    fn posicion(b: &Banda, ahora: Instant) -> f64 {
        let (desde, hasta, t0) = b.anim;
        let p = (ahora.saturating_duration_since(t0).as_secs_f64()
            / crate::animaciones::dur(DURACION).as_secs_f64().max(1e-6))
        .min(1.0);
        desde + (hasta - desde) * (1.0 - (1.0 - p).powi(3))
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        self.animos.anda(ahora)
            || self.abierta.as_ref().is_some_and(|b| {
                ahora.saturating_duration_since(b.anim.2) < crate::animaciones::dur(DURACION)
            })
    }

    pub fn armar(&mut self, escala: f32, ahora: Instant) {
        self.rotulos.poner_escala(escala);
        let Some(mut b) = self.abierta.take() else { return };
        let p = self.paleta;
        let (w, h) = (f64::from(self.tam.0), f64::from(self.tam.1));
        let g = modelo::geometria(self.tam.0 as i32, b.alto_salida);
        let mut t = Vec::new();
        let fondo = con_alfa(&p.surface, p.surface[3] * self.opacidad);
        t.push(Trazo::Caja {
            r: Rect::new(0.0, 0.0, w, h),
            radio: 0.0,
            color: fondo,
        });

        // La cabecera: la carpeta y, al lado, lo escrito (o «Buscar…» tenue).
        let prompt = self
            .rotulos
            .linea(&format!("󰸉  {}", b.carpeta), PX, 400.0);
        let (wp, ap) = (self.ancho(&prompt), self.alto_linea(&prompt));
        t.push(Trazo::Texto {
            l: prompt,
            x: RELLENO_X,
            y: RELLENO_ARRIBA,
            color: p.primary,
        });
        let (texto_filtro, color) = if b.filtro.is_empty() {
            ("Buscar…".to_owned(), p.on_surface_variant)
        } else {
            (b.filtro.clone(), p.on_surface)
        };
        let max = (w - RELLENO_X - wp - ENTRE_PROMPT - RELLENO_X).max(0.0);
        let filtro = self.rotulos.recortada_al_inicio(&texto_filtro, PX, max);
        t.push(Trazo::Texto {
            l: filtro,
            x: RELLENO_X + wp + ENTRE_PROMPT,
            y: RELLENO_ARRIBA,
            color,
        });

        // La tira, con la elegida en el centro.
        let arriba = RELLENO_ARRIBA + ap + ENTRE_CABECERA;
        let celda = f64::from(g.celda);
        let y0 = arriba + ((h - arriba - celda) / 2.0).floor();
        let pos = Self::posicion(&b, ahora);
        let mut vistas = Vec::new();
        let mut vivos = HashSet::new();
        for i in 0..b.visibles.len() {
            let x0 = f64::from(g.costado) + (i as f64 - pos) * f64::from(g.paso);
            if x0 + celda < 0.0 || x0 > w {
                continue;
            }
            let r = Rect::new(x0, y0, x0 + celda, y0 + celda);
            let ruta = &b.todos[b.visibles[i]];
            let o = Objetivo::Celda(i);
            vivos.insert(o);
            let elegida = i == b.sel;
            let luz = self.animos.luz(o, elegida, ahora);
            if elegida {
                t.push(Trazo::Sombra {
                    r,
                    radio: RADIO_CELDA,
                    color: con_alfa(&p.primary, 0.45 * luz),
                    spread: 1.0,
                    blur: 8.0,
                });
                t.push(Trazo::Degrade {
                    r,
                    radio: RADIO_CELDA,
                    a: p.primary,
                    b: p.tertiary,
                    forma: Degrade::Diagonal,
                });
            } else if b.actual.as_ref() == Some(ruta) {
                t.push(Trazo::Caja {
                    r,
                    radio: RADIO_CELDA,
                    color: p.outline_variant,
                });
            }
            if let Some(img) = b.minis.get(ruta) {
                t.push(Trazo::Imagen {
                    img: img.clone(),
                    r: r.inset(-ANILLO),
                    radio: RADIO_FOTO,
                    alfa: 1.0,
                });
            }
            vistas.push((i, r));
        }
        self.animos.retener(&vivos);
        b.celdas = vistas;
        b.trazos = t;
        self.abierta = Some(b);
        self.rotulos.barrer();
    }

    pub fn pintar(&self, s: &mut Scene, escala: f64) {
        s.reset();
        if let Some(b) = &self.abierta {
            trazo::pintar(s, &b.trazos, Affine::scale(escala), escala);
        }
    }

    // ── Teclado ──

    pub fn tecla(&mut self, k: Keysym, utf8: Option<&str>, ctrl: bool) -> Vec<EventoFondos> {
        let Some(b) = &self.abierta else {
            return Vec::new();
        };
        let n = b.visibles.len();
        let sel = b.sel;
        let anterior = sel + n.max(1) - 1;
        match k {
            Keysym::Escape => {
                return vec![EventoFondos::Cerrada { volver: b.volver }];
            }
            Keysym::Left | Keysym::KP_Left => self.seleccionar(anterior, true),
            Keysym::b | Keysym::B if ctrl => self.seleccionar(anterior, true),
            Keysym::Right | Keysym::KP_Right => self.seleccionar(sel + 1, true),
            Keysym::f | Keysym::F if ctrl => self.seleccionar(sel + 1, true),
            Keysym::Home => self.seleccionar(0, true),
            Keysym::End => self.seleccionar(n.saturating_sub(1), true),
            Keysym::Return | Keysym::KP_Enter => return self.aplicar(),
            Keysym::BackSpace => {
                if let Some(b) = &mut self.abierta
                    && b.filtro.pop().is_some()
                {
                    self.filtrar();
                }
            }
            _ => {
                let c = utf8
                    .and_then(|u| u.chars().next())
                    .filter(|c| !ctrl && !c.is_control());
                if let (Some(c), Some(b)) = (c, &mut self.abierta) {
                    b.filtro.push(c);
                    self.filtrar();
                }
            }
        }
        Vec::new()
    }

    fn seleccionar(&mut self, i: usize, animar: bool) {
        let Some(b) = &mut self.abierta else { return };
        let n = b.visibles.len();
        if n == 0 {
            return;
        }
        let ahora = Instant::now();
        let desde = Self::posicion(b, ahora);
        b.sel = i % n;
        b.anim = if animar {
            (desde, b.sel as f64, ahora)
        } else {
            (b.sel as f64, b.sel as f64, ahora)
        };
        self.pedir();
    }

    /// Lo elegido pasa a ser el fondo de ahora.
    fn aplicar(&mut self) -> Vec<EventoFondos> {
        let Some(b) = &mut self.abierta else {
            return Vec::new();
        };
        let Some(&i) = b.visibles.get(b.sel) else {
            return Vec::new();
        };
        let ruta = b.todos[i].clone();
        b.actual = Some(ruta.clone());
        vec![EventoFondos::Aplicar(ruta)]
    }

    /// Lo escrito cambió: queda lo que lo contiene, con la misma elegida si sigue estando.
    fn filtrar(&mut self) {
        let Some(b) = &mut self.abierta else { return };
        let anterior = b.visibles.get(b.sel).copied();
        let nuevos = modelo::filtrar(&b.todos, &b.filtro);
        let sel = anterior
            .and_then(|a| nuevos.iter().position(|i| *i == a))
            .unwrap_or(0);
        b.visibles = nuevos;
        self.seleccionar(sel, false);
    }

    // ── Puntero ──

    fn bajo(&self, x: f64, y: f64) -> Option<usize> {
        self.abierta
            .as_ref()?
            .celdas
            .iter()
            .find(|(_, r)| r.contains((x, y)))
            .map(|(i, _)| *i)
    }

    pub fn apretar(&mut self, x: f64, y: f64) {
        let o = self.bajo(x, y);
        if let Some(b) = &mut self.abierta {
            b.apretado = o;
        }
    }

    /// Un click en una imagen la elige y la aplica.
    pub fn soltar(&mut self, x: f64, y: f64) -> Vec<EventoFondos> {
        let bajo = self.bajo(x, y);
        let Some(b) = &mut self.abierta else {
            return Vec::new();
        };
        let Some(i) = b.apretado.take().filter(|i| Some(*i) == bajo) else {
            return Vec::new();
        };
        self.seleccionar(i, true);
        self.aplicar()
    }

    /// Una vuelta de rueda pasa a la imagen siguiente (o la anterior); un touchpad suma hasta
    /// juntar un paso.
    pub fn rueda(&mut self, paso: f64, discreto: bool) {
        let Some(b) = &mut self.abierta else { return };
        let pasos = if discreto {
            paso.signum() as i64
        } else {
            b.rueda += paso;
            let k = (b.rueda / PASO_RUEDA).trunc();
            b.rueda -= k * PASO_RUEDA;
            k as i64
        };
        let (n, sel) = (b.visibles.len() as i64, b.sel as i64);
        if n == 0 || pasos == 0 {
            return;
        }
        self.seleccionar((sel + pasos).rem_euclid(n) as usize, true);
    }
}

/// El hilo que lee las miniaturas: las del pedido más reciente, las más cercanas primero; con
/// la ventana lista, hace las que faltan en disco, también por cercanía. Termina cuando se
/// cierra el canal.
fn trabajar(
    rx: &mpsc::Receiver<Pedido>,
    salida: &Sender<Mini>,
    generacion: u64,
    cache: &Path,
    mut faltan: HashSet<PathBuf>,
) {
    let tanda_max = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let mut hechas: HashSet<PathBuf> = HashSet::new();
    let Ok(mut pedido) = rx.recv() else { return };
    loop {
        loop {
            match rx.try_recv() {
                Ok(nuevo) => pedido = nuevo,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
        let ventana: HashSet<&PathBuf> = pedido.orden.iter().take(pedido.ventana).collect();
        hechas.retain(|r| ventana.contains(r));
        let tanda: Vec<PathBuf> = pedido
            .orden
            .iter()
            .take(pedido.ventana)
            .filter(|r| !hechas.contains(*r))
            .take(tanda_max)
            .cloned()
            .collect();
        if !tanda.is_empty() {
            crear(cache, &tanda, &mut faltan);
            for ruta in tanda {
                if let Some(img) = leer(cache, &ruta)
                    && salida
                        .send(Mini {
                            generacion,
                            ruta: ruta.clone(),
                            img,
                        })
                        .is_err()
                {
                    return;
                }
                hechas.insert(ruta);
            }
            continue;
        }
        // La ventana está lista: lo que falta en disco, también, empezando por lo cercano.
        let mut fuera: Vec<PathBuf> = pedido
            .orden
            .iter()
            .skip(pedido.ventana)
            .filter(|r| faltan.contains(*r))
            .take(tanda_max)
            .cloned()
            .collect();
        if fuera.is_empty() {
            fuera = faltan.iter().take(tanda_max).cloned().collect();
        }
        if fuera.is_empty() {
            // Nada que hacer hasta el próximo pedido.
            match rx.recv() {
                Ok(nuevo) => pedido = nuevo,
                Err(_) => return,
            }
            continue;
        }
        crear(cache, &fuera, &mut faltan);
    }
}

/// Hace las miniaturas que falten de `rutas`.
fn crear(cache: &Path, rutas: &[PathBuf], faltan: &mut HashSet<PathBuf>) {
    let hacer: Vec<PathBuf> = rutas.iter().filter(|r| faltan.contains(*r)).cloned().collect();
    if hacer.is_empty() {
        return;
    }
    motor::crear_miniaturas(cache, &hacer);
    for r in &hacer {
        faltan.remove(r);
    }
}

/// La miniatura de `ruta`; si no se pudo hacer, la imagen misma (decodificar un 4K es lo caro
/// que las miniaturas evitan).
fn leer(cache: &Path, ruta: &Path) -> Option<ImageData> {
    let mini = modelo::miniatura(cache, ruta);
    let archivo = if mini.exists() { mini } else { ruta.to_owned() };
    crate::icono::dibujar(&archivo, LADO, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay_client_toolkit::reexports::calloop::channel;

    fn abierta(nombres: &[&str], actual: Option<&str>) -> FondosUi {
        let mut ui = FondosUi::new(Paleta::desde_css(""), 1.0, Borde::Brillo);
        let (tx, _rx) = channel::channel();
        ui.abrir(
            VistaFondos {
                salida: None,
                carpeta: "Imágenes".into(),
                rutas: nombres.iter().map(|n| PathBuf::from(format!("/f/{n}"))).collect(),
                actual: actual.map(|a| PathBuf::from(format!("/f/{a}"))),
                faltan: Vec::new(),
                cache: PathBuf::from("/nada"),
                volver: true,
            },
            1440,
            tx,
        );
        ui
    }

    fn sel(ui: &FondosUi) -> usize {
        ui.abierta.as_ref().unwrap().sel
    }

    #[test]
    fn abre_en_el_fondo_de_ahora() {
        let ui = abierta(&["a.jpg", "b.jpg", "c.jpg"], Some("c.jpg"));
        assert_eq!(sel(&ui), 2);
        assert_eq!(sel(&abierta(&["a.jpg"], Some("z.jpg"))), 0);
    }

    #[test]
    fn las_flechas_dan_la_vuelta() {
        let mut ui = abierta(&["a.jpg", "b.jpg", "c.jpg"], None);
        ui.tecla(Keysym::Left, None, false);
        assert_eq!(sel(&ui), 2);
        ui.tecla(Keysym::Right, None, false);
        assert_eq!(sel(&ui), 0);
        ui.tecla(Keysym::End, None, false);
        assert_eq!(sel(&ui), 2);
        ui.tecla(Keysym::b, None, true);
        assert_eq!(sel(&ui), 1);
        ui.tecla(Keysym::f, None, true);
        assert_eq!(sel(&ui), 2);
        ui.tecla(Keysym::Home, None, false);
        assert_eq!(sel(&ui), 0);
    }

    #[test]
    fn escribir_filtra_y_conserva_la_elegida() {
        let mut ui = abierta(&["mar.jpg", "monte.jpg", "luna.jpg"], Some("monte.jpg"));
        ui.tecla(Keysym::m, Some("m"), false);
        let b = ui.abierta.as_ref().unwrap();
        assert_eq!(b.visibles, [0, 1]);
        assert_eq!(b.sel, 1);
        ui.tecla(Keysym::o, Some("o"), false);
        assert_eq!(ui.abierta.as_ref().unwrap().visibles, [1]);
        ui.tecla(Keysym::BackSpace, None, false);
        ui.tecla(Keysym::BackSpace, None, false);
        let b = ui.abierta.as_ref().unwrap();
        assert_eq!((b.visibles.len(), b.sel), (3, 1));
        // Con Ctrl no es texto.
        ui.tecla(Keysym::x, Some("x"), true);
        assert!(ui.abierta.as_ref().unwrap().filtro.is_empty());
    }

    #[test]
    fn enter_aplica_y_marca_la_actual() {
        let mut ui = abierta(&["a.jpg", "b.jpg"], None);
        ui.tecla(Keysym::Right, None, false);
        let v = ui.tecla(Keysym::Return, None, false);
        assert!(matches!(&v[..], [EventoFondos::Aplicar(r)] if r == Path::new("/f/b.jpg")));
        assert_eq!(
            ui.abierta.as_ref().unwrap().actual.as_deref(),
            Some(Path::new("/f/b.jpg"))
        );
    }

    #[test]
    fn escape_vuelve_o_cierra() {
        let mut ui = abierta(&["a.jpg"], None);
        let v = ui.tecla(Keysym::Escape, None, false);
        assert!(matches!(&v[..], [EventoFondos::Cerrada { volver: true }]));
    }

    #[test]
    fn la_rueda_pasa_de_a_una() {
        let mut ui = abierta(&["a.jpg", "b.jpg", "c.jpg"], None);
        ui.rueda(1.0, true);
        assert_eq!(sel(&ui), 1);
        ui.rueda(-3.0, true);
        assert_eq!(sel(&ui), 0);
        ui.rueda(-1.0, true);
        assert_eq!(sel(&ui), 2);
        // Un touchpad junta de a poco.
        ui.rueda(25.0, false);
        assert_eq!(sel(&ui), 2);
        ui.rueda(25.0, false);
        assert_eq!(sel(&ui), 0);
    }

    #[test]
    fn sin_imagenes_no_hace_nada() {
        let mut ui = abierta(&[], None);
        ui.tecla(Keysym::Right, None, false);
        assert!(ui.tecla(Keysym::Return, None, false).is_empty());
        ui.rueda(1.0, true);
    }
}
