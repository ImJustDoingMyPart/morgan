//! El menú de sistema con las piezas del centro: la caja de los paneles centrada en la salida;
//! a la izquierda, el índice (con el buscador del launcher) en su panel con el tinte de las
//! cajas; a la derecha, la página entera de la categoría, con las filas del centro y sus
//! controles (casilla, lista desplegable, campo). Qué se lee, qué se busca y qué hace cada
//! fila lo decide el hilo de GTK (`menu/mod.rs`); acá vive lo escrito, lo elegido, lo
//! desplegado, los dos scrolls y el puntero.
//!
//! Las medidas, de la versión con GTK (`.menu` y lo que cuelga de ella en `estilo.css` hasta
//! 0.1.1): la caja de 960 × 620 con tope en el 70 % de la salida; el índice de 240 con 8 de
//! relleno, 16 hasta la página; filas del índice de 8 y 10 de relleno (las secciones, de 4 y
//! 40 a la izquierda); la página con 8 entre lo que tiene, el título a 18 px, cada sección a
//! 12 en `primary` con 16 arriba, y las filas de 10 y 12 de relleno.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use morgan_motor::color::{Color, Paleta, con_alfa, mezclar};
use morgan_motor::curva::{Revelado, Transicion};
use morgan_motor::forma::{Borde, panel};
use morgan_motor::texto;
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Point, Rect};
use morgan_motor::vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};
use smithay_client_toolkit::seat::keyboard::Keysym;

use super::campo::Entrada;
use super::launcher::{Recorrido, avance, ease_out, tramos};
use super::trazo::{self, Degrade, Trazo};
use super::{Animos, Hoja, Objetivo, RADIO, Rotulos};
use crate::centro::vista::{EventoAjustes, IndiceAjustes, Lugar, PaginaAjustes};
use crate::menu::arbol::{Control, Fila, Opcion, Que, Seccion};
use crate::menu::monitores::{Pantalla, encajar};

const BORDE_Y_RELLENO: f64 = 16.0;
/// El lugar del brillo, alrededor de la caja.
const MARGEN: f64 = 8.0;

const ANCHO_INDICE: f64 = 240.0;
const RELLENO_INDICE: f64 = 8.0;
const ENTRE_LADOS: f64 = 16.0;
const BUSCADOR_X: f64 = 12.0;
const BUSCADOR_Y: f64 = 8.0;
const TRAS_LUPA: f64 = 8.0;
const LUPA: &str = "\u{F0349}";

const FILA_INDICE_X: f64 = 10.0;
const FILA_INDICE_Y: f64 = 8.0;
const MIN_FILA_INDICE: f64 = 20.0;
const COLUMNA_GLIFO: f64 = 20.0;
const TRAS_GLIFO: f64 = 10.0;
const SANGRIA_SECCION: f64 = 40.0;
const SECCION_Y: f64 = 4.0;
const ENTRE_FILAS_INDICE: f64 = 4.0;

const ENTRE: f64 = 8.0;
const PAGINA_ARRIBA: f64 = 4.0;
const PAGINA_DERECHA: f64 = 4.0;
const PAGINA_ABAJO: f64 = 16.0;
const SOBRE_SECCION: f64 = 16.0;
const FILA_X: f64 = 12.0;
const FILA_Y: f64 = 10.0;
const DATO_Y: f64 = 4.0;
const ENTRE_TEXTOS: f64 = 2.0;
const AL_CONTROL: f64 = 10.0;
const CASILLA: f64 = 14.0;
const OPCION_X: f64 = 12.0;
const OPCION_Y: f64 = 6.0;
const COLUMNAS: usize = 3;
const ALTO_DIBUJO: f64 = 180.0;
/// Aire alrededor del dibujo, para poder arrastrar una pantalla más allá de las otras.
const AIRE_DIBUJO: f64 = 28.0;
/// Menos que esto es un toque, no un arrastre.
const TOQUE: f64 = 3.0;

const FLECHA_DERECHA: &str = "\u{F0142}";
const FLECHA_ABAJO: &str = "\u{F0140}";
const PAPELERA: &str = "\u{F0A7A}";
const LAPIZ: &str = "\u{F03EB}";
const TILDE: &str = "\u{F012C}";
const TILDE_OPCION: &str = "󰄬";
const ATENCION: &str = "\u{F0026}";

const ENTRA: Duration = Duration::from_millis(180);
const DESLIZ: Duration = Duration::from_millis(200);
const DESPLIEGUE_INDICE: Duration = Duration::from_millis(150);
const DESPLIEGUE: Duration = Duration::from_millis(200);
const RESALTE: Duration = Duration::from_millis(900);
const RESALTE_FUNDIDO: Duration = Duration::from_millis(400);
const AVISO: Duration = Duration::from_secs(4);
const LATIDO: Duration = Duration::from_secs(1);

fn letra() -> f32 {
    texto::pt(10.0)
}

fn op(c: &Color, a: f32) -> Color {
    con_alfa(c, c[3] * a)
}

#[derive(Clone, Copy, PartialEq)]
enum Zona {
    Indice,
    Pagina,
}

/// Lo que se dice en el título de una fila un rato, en `error`.
#[derive(Clone, Copy, PartialEq)]
enum Aviso {
    Confirmando,
    Fallo,
}

impl Aviso {
    fn texto(self) -> &'static str {
        match self {
            Aviso::Confirmando => tr!("¿Seguro? Tocá de nuevo para borrar"),
            Aviso::Fallo => tr!("No se pudo: el detalle está en el journal"),
        }
    }
}

/// Lo que pide el menú a su hilo.
pub enum Pedido {
    Gtk(EventoAjustes),
    /// Lo del portapapeles al campo que tiene el foco.
    Pegar,
}

/// Un monitor que se arrastra en el dibujo a escala.
#[derive(Clone, Copy)]
struct Arrastre {
    lugar: Lugar,
    /// Dónde se apretó, en la superficie.
    desde: (f64, f64),
    ahora: (f64, f64),
}

/// El dibujo a escala de una fila de monitores: píxeles del escritorio a lógicos de la página.
#[derive(Clone, Copy)]
struct Escala {
    escala: f64,
    origen: (f64, f64),
}

impl Escala {
    fn a_dibujo(&self, x: i32, y: i32) -> (f64, f64) {
        (
            self.origen.0 + f64::from(x) * self.escala,
            self.origen.1 + f64::from(y) * self.escala,
        )
    }

    fn a_escritorio(&self, x: f64, y: f64) -> (f64, f64) {
        (
            (x - self.origen.0) / self.escala,
            (y - self.origen.1) / self.escala,
        )
    }
}

/// El campo de una fila con `Control::Texto`, abierto debajo de ella.
struct Campo {
    lugar: Lugar,
    entrada: Entrada,
    /// Abre con lo puesto elegido: lo primero que se escribe lo reemplaza.
    reemplazar: bool,
    corrido: f64,
}

pub struct AjustesUi {
    pub paleta: Paleta,
    pub opacidad: f32,
    pub borde: Borde,
    rotulos: Rotulos,
    animos: Animos,
    pub tam: (u32, u32),

    indice: IndiceAjustes,
    scroll_indice: f64,
    /// Las secciones de la categoría recién abierta bajan.
    despliegue_indice: Revelado,
    buscador: Entrada,
    corrido_buscador: f64,

    titulo: String,
    secciones: Option<Rc<Vec<Seccion>>>,
    version: u64,
    entrada: Option<Instant>,
    scroll: Recorrido,
    /// Adónde ir en la página cuando esté armada: sección, fila y si se desliza.
    ir: Option<(usize, Option<usize>, bool)>,
    resalte: Option<((usize, usize), Instant)>,
    /// Lo elegido con el teclado después de releer la página, por su lugar entre los elegibles.
    elegir: Option<usize>,

    zona: Zona,
    elegido: Option<Objetivo>,
    /// La fila con la lista desplegada (todas las listas son de un grupo: abrir una cierra las
    /// otras) y cómo se despliega cada una.
    abierta: Option<Lugar>,
    revelados: HashMap<Lugar, Revelado>,
    campo: Option<Campo>,
    aplicando: HashSet<Lugar>,
    latido: Instant,
    avisos: HashMap<Lugar, (Aviso, Instant)>,
    pub grabando: bool,

    puntero: Option<(f64, f64)>,
    encima: Option<Objetivo>,
    apretado: Option<Objetivo>,
    afuera: bool,
    arrastre: Option<Arrastre>,
    /// El dibujo de cada fila de monitores, por sección y fila.
    escalas: HashMap<(usize, usize), Escala>,
    /// Las muestras de los cursores, como imágenes (por la dirección de las suyas).
    cursores: HashMap<usize, Vec<ImageData>>,

    /// El panel del índice y el buscador, que no se scrollean.
    fijo: Hoja,
    hoja_indice: Hoja,
    hoja: Hoja,
    /// Dónde empieza cada sección de la página.
    cabeceras: Vec<f64>,
    caja: Rect,
    buscador_caja: Rect,
    vista_indice: Rect,
    vista_pagina: Rect,
}

impl AjustesUi {
    pub fn new(paleta: Paleta, opacidad: f32, borde: Borde) -> Self {
        AjustesUi {
            paleta,
            opacidad,
            borde,
            rotulos: Rotulos::new(),
            animos: Animos::default(),
            tam: (0, 0),
            indice: IndiceAjustes {
                filas: Vec::new(),
                elegida: 0,
                desplegar: false,
                version: 0,
            },
            scroll_indice: 0.0,
            despliegue_indice: Revelado::new(true),
            buscador: Entrada::default(),
            corrido_buscador: 0.0,
            titulo: String::new(),
            secciones: None,
            version: 0,
            entrada: None,
            scroll: Recorrido::new(DESLIZ),
            ir: None,
            resalte: None,
            elegir: None,
            zona: Zona::Indice,
            elegido: None,
            abierta: None,
            revelados: HashMap::new(),
            campo: None,
            aplicando: HashSet::new(),
            latido: Instant::now(),
            avisos: HashMap::new(),
            grabando: false,
            puntero: None,
            encima: None,
            apretado: None,
            afuera: false,
            arrastre: None,
            escalas: HashMap::new(),
            cursores: HashMap::new(),
            fijo: Hoja::default(),
            hoja_indice: Hoja::default(),
            hoja: Hoja::default(),
            cabeceras: Vec::new(),
            caja: Rect::ZERO,
            buscador_caja: Rect::ZERO,
            vista_indice: Rect::ZERO,
            vista_pagina: Rect::ZERO,
        }
    }

    /// Cada apertura empieza con el buscador vacío, en el índice.
    pub fn abrir(&mut self) {
        self.buscador = Entrada::default();
        self.corrido_buscador = 0.0;
        self.zona = Zona::Indice;
        self.elegido = None;
        self.campo = None;
        self.apretado = None;
        self.arrastre = None;
        self.puntero = None;
        self.encima = None;
        self.grabando = false;
    }

    pub fn cerrar(&mut self) {
        self.abrir();
        self.abierta = None;
        self.revelados.clear();
        self.aplicando.clear();
        self.avisos.clear();
        self.cursores.clear();
    }

    /// Arma `p` una vez, sin mostrarla: lo que cuesta la primera vez (las fuentes) ya está.
    pub fn precalentar(&mut self, p: PaginaAjustes) {
        self.poner_pagina(p);
        self.tam = (2560, 1440);
        self.armar(1.0, Instant::now());
    }

    pub fn poner_indice(&mut self, i: IndiceAjustes) {
        if i.desplegar {
            self.despliegue_indice = Revelado::new(false);
            self.despliegue_indice.ir(
                true,
                crate::animaciones::dur(DESPLIEGUE_INDICE),
                Instant::now(),
            );
        }
        self.indice = i;
        self.mostrar_en_indice(self.indice.elegida);
    }

    pub fn poner_pagina(&mut self, p: PaginaAjustes) {
        let ahora = Instant::now();
        // Lo de antes de releer: la fila elegida sigue elegida, por su lugar.
        let antes = self.elegido;
        self.titulo = p.titulo;
        self.secciones = p.secciones.map(Rc::new);
        self.version = p.version;
        // Una lista desplegada se cierra al releer, como al rehacer la página con GTK.
        self.abierta = None;
        self.revelados.clear();
        self.campo = None;
        self.aplicando.clear();
        self.cursores.clear();
        if p.entra {
            self.entrada = Some(ahora);
            self.scroll.poner(0.0);
            self.elegido = None;
            self.avisos.clear();
            self.resalte = None;
        } else {
            self.elegido = match antes {
                Some(Objetivo::OpcionAjuste(l)) => {
                    Some(Objetivo::Ajuste(Lugar { opcion: None, ..l }))
                }
                o => o,
            };
        }
        self.elegir = p.elegir;
        if self.zona == Zona::Indice {
            self.elegido = None;
        }
    }

    /// Ir a una sección (o a una fila, con su resalte) de la página que se ve.
    pub fn ir_a(&mut self, seccion: usize, fila: Option<usize>, suave: bool) {
        self.ir = Some((seccion, fila, suave));
    }

    /// Lo aplicado terminó: deja de latir y, si falló, la fila lo dice un rato.
    pub fn hecho(&mut self, lugar: Lugar, ok: bool) {
        self.aplicando.remove(&lugar);
        if !ok {
            self.avisos.insert(lugar, (Aviso::Fallo, Instant::now()));
        }
    }

    pub fn panel(&self) -> Rect {
        self.caja
    }

    fn escala(&self) -> f64 {
        f64::from(self.rotulos.escala())
    }

    fn ancho(&self, l: &morgan_motor::parley::Layout<u8>) -> f64 {
        texto::caja(l).0 / self.escala()
    }

    fn alto(&self, l: &morgan_motor::parley::Layout<u8>) -> f64 {
        texto::alto_de_linea(l) / self.escala()
    }

    fn max_scroll(&self) -> f64 {
        (self.hoja.alto - self.vista_pagina.height()).max(0.0)
    }

    fn max_scroll_indice(&self) -> f64 {
        (self.hoja_indice.alto - self.vista_indice.height()).max(0.0)
    }

    pub fn armar(&mut self, escala: f32, ahora: Instant) {
        self.rotulos.poner_escala(escala);
        let (w, h) = (f64::from(self.tam.0), f64::from(self.tam.1));
        let (aw, ah) = crate::menu::arbol::tamano(self.tam.0 as i32, self.tam.1 as i32);
        // El tamaño pedido incluye el margen del brillo (como el `size_request` de GTK).
        let (aw, ah) = (f64::from(aw) - 2.0 * MARGEN, f64::from(ah) - 2.0 * MARGEN);
        // Centrada como un widget de GTK: la caja con su margen, en un píxel entero.
        let x0 = ((w - aw - 2.0 * MARGEN) / 2.0).floor() + MARGEN;
        let y0 = ((h - ah - 2.0 * MARGEN) / 2.0).floor() + MARGEN;
        self.caja = Rect::new(x0, y0, x0 + aw, y0 + ah);
        let c = self.caja.inset(-BORDE_Y_RELLENO);
        let lateral = Rect::new(c.x0, c.y0, c.x0 + ANCHO_INDICE, c.y1);
        let adentro = lateral.inset(-RELLENO_INDICE);
        let lupa = self.rotulos.linea(LUPA, 14.0, 400.0);
        let alto_buscador = 2.0 * BUSCADOR_Y + self.alto(&lupa).max(self.alto_letra());
        self.buscador_caja = Rect::new(
            adentro.x0,
            adentro.y0,
            adentro.x1,
            adentro.y0 + alto_buscador,
        );
        self.vista_indice = Rect::new(
            adentro.x0,
            self.buscador_caja.y1 + ENTRE,
            adentro.x1,
            adentro.y1,
        );
        self.vista_pagina = Rect::new(lateral.x1 + ENTRE_LADOS, c.y0, c.x1, c.y1);
        self.armar_buscador(lateral, ahora);

        self.armar_pagina(ahora);
        // Lo pedido para ir, con la página ya medida.
        if let Some((s, f, suave)) = self.ir.take() {
            self.ir_ahora(s, f, suave, ahora);
        }
        if let Some(n) = self.elegir.take()
            && self.zona == Zona::Pagina
        {
            let e = &self.hoja.enfocables;
            self.elegido = e.get(n).or(e.last()).copied();
        }
        if self
            .elegido
            .is_some_and(|o| !self.hoja.presentes.contains(&o))
        {
            self.elegido = None;
        }
        self.armar_indice(ahora);
        let mut vivos: HashSet<Objetivo> = self.hoja.presentes.iter().copied().collect();
        vivos.extend(self.hoja_indice.presentes.iter().copied());
        self.animos.retener(&vivos);
        self.scroll_indice = self.scroll_indice.clamp(0.0, self.max_scroll_indice());
        if let Some((x, y)) = self.puntero {
            self.encima = self.bajo(x, y);
        }
        let t = Instant::now();
        self.avisos
            .retain(|_, (_, t0)| t.saturating_duration_since(*t0) < AVISO);
        self.rotulos.barrer();
    }

    /// El panel del índice y, arriba, el buscador: la lupa y lo escrito.
    fn armar_buscador(&mut self, lateral: Rect, ahora: Instant) {
        let p = self.paleta;
        let mut f = Hoja::default();
        f.trazos.push(Trazo::Caja {
            r: lateral,
            radio: RADIO,
            color: op(&p.on_surface, 0.04),
        });
        let b = self.buscador_caja;
        f.trazos.push(Trazo::Caja {
            r: b,
            radio: RADIO,
            color: op(&p.on_surface, 0.06),
        });
        let lupa = self.rotulos.linea(LUPA, 14.0, 400.0);
        let (wl, al) = (self.ancho(&lupa), self.alto(&lupa));
        let alto = b.height() - 2.0 * BUSCADOR_Y;
        f.trazos.push(Trazo::Texto {
            l: lupa,
            x: b.x0 + BUSCADOR_X,
            y: b.y0 + BUSCADOR_Y + ((alto - al) / 2.0).floor(),
            color: p.primary,
        });
        let at = self.alto_letra();
        let zy = b.y0 + BUSCADOR_Y + ((alto - at) / 2.0).floor();
        let zona = Rect::new(
            b.x0 + BUSCADOR_X + wl + TRAS_LUPA,
            zy,
            b.x1 - BUSCADOR_X,
            zy + at,
        );
        let e = std::mem::take(&mut self.buscador);
        let mut corrido = self.corrido_buscador;
        // Con el campo de una fila abierto, el cursor está allá.
        let quieto = self.campo.is_some();
        let mut copia = Entrada::default();
        copia.insertar(&e.texto);
        let mostrado = if quieto { &copia } else { &e };
        self.armar_escrito(&mut f, mostrado, tr!("Buscar…"), zona, &mut corrido, ahora);
        if quieto && let Some(Trazo::Caja { .. }) = f.trazos.iter().rev().nth(1) {
            // Sin cursor: el último trazo antes del fin de recorte es el cursor.
            let n = f.trazos.len();
            f.trazos.remove(n - 2);
        }
        self.buscador = e;
        self.corrido_buscador = corrido;
        self.fijo = f;
    }

    fn alto_letra(&mut self) -> f64 {
        let l = self.rotulos.linea("M", letra(), 400.0);
        self.alto(&l)
    }

    // ── El índice ──

    /// La sección de la página que está a la vista: la última cuyo título pasó el borde de
    /// arriba (al fondo, la última que se ve: puede que su título nunca llegue arriba).
    fn a_la_vista(&self, ahora: Instant) -> usize {
        let valor = self.scroll.valor(ahora);
        let pagina = self.vista_pagina.height();
        if valor < 1.0 {
            return 0;
        }
        let al_fondo = valor >= self.max_scroll() - 1.0;
        let limite = if al_fondo {
            valor + pagina - 24.0
        } else {
            valor + 24.0
        };
        self.cabeceras
            .iter()
            .rposition(|y| *y <= limite)
            .unwrap_or(0)
    }

    fn armar_indice(&mut self, ahora: Instant) {
        let p = self.paleta;
        let mut h = Hoja::default();
        let ancho = self.vista_indice.width();
        let a_la_vista = self.a_la_vista(ahora);
        let f = self.despliegue_indice.valor(ahora);
        let filas = self.indice.filas.clone();
        let mut y = 0.0;
        for (i, fila) in filas.iter().enumerate() {
            let es_seccion = fila.seccion.is_some();
            if i > 0 && !es_seccion {
                y += ENTRE_FILAS_INDICE;
            }
            let o = Objetivo::Indice(i);
            let elegida = i == self.indice.elegida;
            let luz = self.animos.luz(o, self.encima == Some(o), ahora);
            let fondo = if elegida {
                op(&p.primary, 0.14)
            } else {
                op(&p.on_surface, 0.05 * luz)
            };
            let mut sub = Hoja::default();
            let (alto, x) = if es_seccion {
                (
                    SECCION_Y * 2.0 + MIN_FILA_INDICE.max(self.alto_letra()),
                    SANGRIA_SECCION,
                )
            } else if fila.glifo.is_some() {
                let g = self
                    .rotulos
                    .linea(&fila.glifo.unwrap_or(' ').to_string(), 16.0, 400.0);
                let a =
                    FILA_INDICE_Y * 2.0 + MIN_FILA_INDICE.max(self.alto(&g)).max(self.alto_letra());
                (a, FILA_INDICE_X + COLUMNA_GLIFO + TRAS_GLIFO)
            } else {
                let chica = self.rotulos.linea("M", letra() / 1.2, 400.0);
                let a = FILA_INDICE_Y * 2.0 + self.alto_letra() + self.alto(&chica);
                (a, FILA_INDICE_X)
            };
            let r = Rect::new(0.0, 0.0, ancho, alto);
            sub.objetivo(o, r, false);
            sub.trazos.push(Trazo::Caja {
                r,
                radio: RADIO,
                color: fondo,
            });
            let max = ancho - x - FILA_INDICE_X;
            let color = if es_seccion {
                if fila.seccion == Some(a_la_vista) {
                    p.tertiary
                } else {
                    p.on_surface_variant
                }
            } else {
                p.on_surface
            };
            let partes = tramos(&fila.titulo, &fila.resaltes, 0, 1, letra());
            let partes: Vec<(&str, u8, f32)> = partes
                .iter()
                .map(|(s, r, px)| (s.as_str(), *r, *px))
                .collect();
            let l = self.rotulos.tramos(&partes, max);
            let al = self.alto_letra();
            if let Some(d) = &fila.donde {
                let chica = letra() / 1.2;
                let partes = tramos(d, &fila.resaltes_donde, 2, 3, chica);
                let partes: Vec<(&str, u8, f32)> = partes
                    .iter()
                    .map(|(s, r, px)| (s.as_str(), *r, *px))
                    .collect();
                let ld = self.rotulos.tramos(&partes, max);
                let ad = self.alto(&ld);
                sub.trazos.push(Trazo::Tramos {
                    l,
                    x,
                    y: FILA_INDICE_Y,
                    colores: vec![color, p.tertiary, op(&color, 0.55), op(&p.tertiary, 0.55)],
                });
                sub.trazos.push(Trazo::Tramos {
                    l: ld,
                    x,
                    y: FILA_INDICE_Y + al,
                    colores: vec![color, p.tertiary, op(&color, 0.55), op(&p.tertiary, 0.55)],
                });
                let _ = ad;
            } else {
                let ty = ((alto - al) / 2.0).floor();
                sub.trazos.push(Trazo::Tramos {
                    l,
                    x,
                    y: ty,
                    colores: vec![color, p.tertiary],
                });
            }
            if let Some(g) = fila.glifo {
                let gl = self.rotulos.linea(&g.to_string(), 16.0, 400.0);
                let col = Rect::new(FILA_INDICE_X, 0.0, FILA_INDICE_X + COLUMNA_GLIFO, alto);
                sub.trazos.push(Trazo::Glifo {
                    l: gl,
                    c: col.center(),
                    color: p.primary,
                });
            }
            // Las secciones de la categoría recién abierta bajan, cada una desde cero.
            let visto = if es_seccion { (alto * f).round() } else { alto };
            h.incrustar(
                sub,
                0.0,
                y + visto - alto,
                Rect::new(-1.0, y, ancho + 1.0, y + visto),
            );
            y += visto;
        }
        h.alto = y;
        self.hoja_indice = h;
    }

    fn mostrar_en_indice(&mut self, i: usize) {
        let Some(r) = self.hoja_indice.caja(Objetivo::Indice(i)) else {
            return;
        };
        let alto = self.vista_indice.height();
        if r.y0 < self.scroll_indice {
            self.scroll_indice = r.y0;
        } else if r.y1 > self.scroll_indice + alto {
            self.scroll_indice = r.y1 - alto;
        }
    }

    // ── La página ──

    fn armar_pagina(&mut self, ahora: Instant) {
        let p = self.paleta;
        let mut h = Hoja::default();
        self.cabeceras.clear();
        let ancho = self.vista_pagina.width() - PAGINA_DERECHA;
        let mut y = PAGINA_ARRIBA;
        let t = self.rotulos.recortada(&self.titulo, 18.0, ancho);
        let at = self.alto(&t);
        h.trazos.push(Trazo::Texto {
            l: t,
            x: 0.0,
            y,
            color: p.on_surface,
        });
        y += at;
        match self.secciones.clone() {
            None => {
                y += ENTRE;
                let l = self.rotulos.linea(tr!("Leyendo…"), 12.0, 400.0);
                let a = self.alto(&l);
                h.trazos.push(Trazo::Texto {
                    l,
                    x: 0.0,
                    y,
                    color: p.on_surface_variant,
                });
                y += a;
            }
            Some(secciones) => {
                for (si, s) in secciones.iter().enumerate() {
                    y += ENTRE + SOBRE_SECCION;
                    self.cabeceras.push(y);
                    let l = self.rotulos.recortada(s.titulo, 12.0, ancho);
                    let a = self.alto(&l);
                    h.trazos.push(Trazo::Texto {
                        l,
                        x: 0.0,
                        y,
                        color: p.primary,
                    });
                    y += a;
                    for (fi, f) in s.filas.iter().enumerate() {
                        y += ENTRE;
                        y = match f {
                            Fila::Ajuste {
                                titulo,
                                sub,
                                control,
                                nota,
                            } => self.armar_ajuste(
                                &mut h, si, fi, titulo, sub, control, nota, ancho, y, ahora,
                            ),
                            Fila::Galeria { titulo, opciones } => self.armar_galeria(
                                &mut h,
                                si,
                                fi,
                                titulo.as_deref(),
                                opciones,
                                ancho,
                                y,
                                ahora,
                            ),
                            Fila::Disposicion(pantallas) => {
                                self.armar_dibujo(&mut h, si, fi, pantallas, ancho, y, ahora)
                            }
                        };
                    }
                }
            }
        }
        h.alto = y + PAGINA_ABAJO;
        self.hoja = h;
    }

    /// El fondo de una fila: el tinte de reposo; el del puntero o lo elegido; el de lo
    /// desplegado; el resalte de una búsqueda; y el aviso, en `error`.
    fn fondo_fila(&mut self, o: Objetivo, lugar: Lugar, ahora: Instant) -> Color {
        let p = self.paleta;
        if self.avisos.contains_key(&lugar) {
            return op(&p.error, 0.18);
        }
        if self.abierta == Some(lugar) {
            return op(&p.primary, 0.2);
        }
        let si = self.encima == Some(o) || self.elegido == Some(o);
        let luz = self.animos.luz(o, si, ahora);
        let base = mezclar(&op(&p.on_surface, 0.06), &op(&p.primary, 0.14), luz);
        match self.resalte {
            Some((fila, t0)) if fila == (lugar.seccion, lugar.fila) && lugar.opcion.is_none() => {
                let d = crate::animaciones::dur(RESALTE_FUNDIDO);
                let mut t = Transicion::new(0.0);
                t.ir(1.0, d, t0);
                let fin = t0 + crate::animaciones::dur(RESALTE);
                if ahora >= fin {
                    t.ir(0.0, d, fin);
                }
                mezclar(&base, &op(&p.tertiary, 0.25), t.valor(ahora))
            }
            _ => base,
        }
    }

    /// Lo que se aplica late hasta que el sistema responde.
    fn alfa_latido(&self, lugar: Lugar, ahora: Instant) -> f32 {
        if !self.aplicando.contains(&lugar) {
            return 1.0;
        }
        let ciclo = crate::animaciones::dur(LATIDO).as_secs_f32().max(0.001);
        let t = ahora.saturating_duration_since(self.latido).as_secs_f32() % ciclo / ciclo;
        let ida = if t < 0.5 { t * 2.0 } else { 2.0 - t * 2.0 };
        1.0 - 0.45 * morgan_motor::curva::ease_in_out(ida)
    }

    #[allow(clippy::too_many_arguments)]
    fn armar_ajuste(
        &mut self,
        h: &mut Hoja,
        si: usize,
        fi: usize,
        titulo: &str,
        sub: &str,
        control: &Control,
        nota: &str,
        ancho: f64,
        y: f64,
        ahora: Instant,
    ) -> f64 {
        let p = self.paleta;
        let lugar = Lugar {
            seccion: si,
            fila: fi,
            opcion: None,
        };
        let o = Objetivo::Ajuste(lugar);
        let dato = matches!(control, Control::Nada);
        let aviso = self.avisos.get(&lugar).map(|(a, _)| *a);
        let encendida = matches!(control, Control::Casilla { activa: true, .. });
        // Lo de la derecha: la casilla o la flecha de lo que hace.
        let flecha = match control {
            Control::Boton(_) => Some(FLECHA_DERECHA),
            Control::Lista(_) => Some(FLECHA_ABAJO),
            Control::Confirmar(_) => Some(PAPELERA),
            Control::Texto { .. } => Some(LAPIZ),
            _ => None,
        }
        .map(|g| self.rotulos.linea(g, 16.0, 400.0));
        let casilla = matches!(control, Control::Casilla { .. });
        let w_der = flecha
            .as_ref()
            .map(|f| self.ancho(f))
            .or(casilla.then_some(CASILLA));
        let h_der = flecha
            .as_ref()
            .map(|f| self.alto(f))
            .or(casilla.then_some(CASILLA))
            .unwrap_or(0.0);
        let pad_y = if dato { DATO_Y } else { FILA_Y };
        let max = (ancho - 2.0 * FILA_X - w_der.map_or(0.0, |w| w + AL_CONTROL)).max(0.0);
        let (px_titulo, px_sub) = if dato { (12.0, 14.0) } else { (14.0, 12.0) };
        let texto_titulo = aviso.map_or(titulo, |a| a.texto());
        let lt = self.rotulos.recortada(texto_titulo, px_titulo, max);
        let ls = (!sub.is_empty()).then(|| self.rotulos.recortada(sub, px_sub, max));
        let ln = (!nota.is_empty()).then(|| {
            let t = super::armar::glib_escape(&format!("{ATENCION}  {nota}"));
            self.rotulos.parrafo(&t, 12.0, max, 0)
        });
        let mut alto_textos = self.alto(&lt);
        if let Some(l) = &ls {
            alto_textos += ENTRE_TEXTOS + self.alto(l);
        }
        if let Some(l) = &ln {
            alto_textos += ENTRE_TEXTOS + l.height() as f64 / self.escala();
        }
        let alto_contenido = alto_textos.max(h_der);
        let alto = 2.0 * pad_y + alto_contenido;
        let r = Rect::new(0.0, y, ancho, y + alto);
        let fondo = if dato {
            [0.0; 4]
        } else {
            self.fondo_fila(o, lugar, ahora)
        };
        h.objetivo(o, r, !dato);
        h.trazos.push(Trazo::Grupo {
            r,
            escala: self.animos.escala(o, ahora),
            dx: 0.0,
            dy: 0.0,
            alfa: self.alfa_latido(lugar, ahora),
        });
        h.trazos.push(Trazo::Caja {
            r,
            radio: RADIO,
            color: fondo,
        });
        let mut ty = y + pad_y + ((alto_contenido - alto_textos) / 2.0).floor();
        let color_titulo = match (aviso, dato) {
            (Some(_), _) => p.error,
            (None, true) => p.on_surface_variant,
            _ => p.on_surface,
        };
        let a = self.alto(&lt);
        h.trazos.push(Trazo::Texto {
            l: lt,
            x: FILA_X,
            y: ty,
            color: color_titulo,
        });
        ty += a;
        if let Some(l) = ls {
            let a = self.alto(&l);
            let color = if dato {
                p.on_surface
            } else if encendida {
                op(&p.tertiary, 0.9)
            } else {
                op(&p.on_surface, 0.5)
            };
            h.trazos.push(Trazo::Texto {
                l,
                x: FILA_X,
                y: ty + ENTRE_TEXTOS,
                color,
            });
            ty += ENTRE_TEXTOS + a;
        }
        if let Some(l) = ln {
            h.trazos.push(Trazo::Texto {
                l,
                x: FILA_X,
                y: ty + ENTRE_TEXTOS,
                color: op(&p.on_surface, 0.5),
            });
        }
        let xd = ancho - FILA_X;
        let cy = y + pad_y + alto_contenido / 2.0;
        if let Some(f) = flecha {
            let w = self.ancho(&f);
            let caja = Rect::new(xd - w, cy - 10.0, xd, cy + 10.0);
            let color = if aviso == Some(Aviso::Confirmando) {
                p.error
            } else {
                p.on_surface_variant
            };
            h.trazos.push(Trazo::Glifo {
                l: f,
                c: caja.center(),
                color,
            });
        }
        if casilla {
            let cy = (cy - CASILLA / 2.0).floor();
            let c = Rect::new(xd - CASILLA, cy, xd, cy + CASILLA);
            if encendida {
                h.trazos.push(Trazo::Sombra {
                    r: c,
                    radio: 3.0,
                    color: op(&p.tertiary, 0.3),
                    spread: 0.0,
                    blur: 6.0,
                });
                h.trazos.push(Trazo::Degrade {
                    r: c,
                    radio: 3.0,
                    a: p.primary,
                    b: p.tertiary,
                    forma: Degrade::Diagonal,
                });
            } else {
                h.trazos.push(Trazo::Borde {
                    r: c,
                    radio: 3.0,
                    ancho: 1.0,
                    color: op(&p.on_surface, 0.35),
                });
            }
        }
        h.trazos.push(Trazo::Fin);
        let mut y = y + alto;
        match control {
            Control::Lista(opciones) => {
                y = self.armar_desplegable(h, lugar, opciones, ancho, y, ahora);
            }
            Control::Texto { pista, .. }
                if self.campo.as_ref().is_some_and(|c| c.lugar == lugar) =>
            {
                y = self.armar_campo(h, pista, ancho, y + 4.0, ahora);
            }
            _ => {}
        }
        y
    }

    /// Las opciones de una lista, debajo de su fila (`cc-lista`, con 8 arriba).
    fn armar_desplegable(
        &mut self,
        h: &mut Hoja,
        lugar: Lugar,
        opciones: &[Opcion],
        ancho: f64,
        y: f64,
        ahora: Instant,
    ) -> f64 {
        let f = self.revelados.get(&lugar).map_or(0.0, |r| r.valor(ahora));
        if f <= 0.0 || opciones.is_empty() {
            return y;
        }
        let p = self.paleta;
        let mut sub = Hoja::default();
        let mut sy = ENTRE + 4.0;
        for (k, o) in opciones.iter().enumerate() {
            let l = Lugar {
                opcion: Some(k),
                ..lugar
            };
            let obj = Objetivo::OpcionAjuste(l);
            let tilde = o
                .activa
                .then(|| self.rotulos.linea(TILDE_OPCION, 14.0, 400.0));
            let x = 4.0;
            let w = ancho - 8.0;
            let w_tilde = tilde.as_ref().map_or(0.0, |t| self.ancho(t) + 10.0);
            let tx = x + OPCION_X + TRAS_GLIFO;
            let lt =
                self.rotulos
                    .recortada(&o.titulo, 14.0, (x + w - OPCION_X - w_tilde - tx).max(0.0));
            let a = self
                .alto(&lt)
                .max(tilde.as_ref().map_or(0.0, |t| self.alto(t)));
            let r = Rect::new(x, sy, x + w, sy + 2.0 * OPCION_Y + a);
            let si = self.encima == Some(obj) || self.elegido == Some(obj);
            let luz = self.animos.luz(obj, si, ahora);
            sub.objetivo(obj, r, true);
            sub.trazos.push(Trazo::Caja {
                r,
                radio: RADIO,
                color: op(&p.primary, 0.14 * luz),
            });
            sub.trazos.push(Trazo::Texto {
                l: lt,
                x: tx,
                y: sy + OPCION_Y,
                color: p.on_surface,
            });
            if let Some(t) = tilde {
                let tw = self.ancho(&t);
                let c = Rect::new(
                    r.x1 - OPCION_X - tw,
                    sy + OPCION_Y,
                    r.x1 - OPCION_X,
                    sy + OPCION_Y + a,
                );
                sub.trazos.push(Trazo::Glifo {
                    l: t,
                    c: c.center(),
                    color: p.tertiary,
                });
            }
            sy = r.y1;
        }
        sy += 4.0;
        sub.trazos.insert(
            0,
            Trazo::Caja {
                r: Rect::new(0.0, ENTRE, ancho, sy),
                radio: RADIO,
                color: op(&p.on_surface, 0.04),
            },
        );
        let v = (sy * f).round();
        h.incrustar(
            sub,
            0.0,
            y + v - sy,
            Rect::new(-10.0, y, ancho + 10.0, y + v),
        );
        y + v
    }

    /// El campo de una fila que se escribe: la caja del buscador, con el lápiz.
    fn armar_campo(
        &mut self,
        h: &mut Hoja,
        pista: &str,
        ancho: f64,
        y: f64,
        ahora: Instant,
    ) -> f64 {
        let p = self.paleta;
        let lapiz = self.rotulos.linea(LAPIZ, letra(), 400.0);
        let al = self.alto(&lapiz).max(self.alto_letra());
        let r = Rect::new(0.0, y, ancho, y + 2.0 * BUSCADOR_Y + al);
        h.trazos.push(Trazo::Caja {
            r,
            radio: RADIO,
            color: op(&p.on_surface, 0.06),
        });
        let wl = self.ancho(&lapiz);
        h.trazos.push(Trazo::Texto {
            l: lapiz,
            x: BUSCADOR_X,
            y: y + BUSCADOR_Y,
            color: p.primary,
        });
        let zona = Rect::new(
            BUSCADOR_X + wl + TRAS_LUPA,
            y + BUSCADOR_Y,
            ancho - BUSCADOR_X,
            y + BUSCADOR_Y + al,
        );
        let Some(mut c) = self.campo.take() else {
            return r.y1;
        };
        self.armar_escrito(h, &c.entrada, pista, zona, &mut c.corrido, ahora);
        self.campo = Some(c);
        r.y1
    }

    /// Lo escrito en `zona` (o el placeholder), con el cursor, corrido para que se vea.
    fn armar_escrito(
        &mut self,
        h: &mut Hoja,
        e: &Entrada,
        placeholder: &str,
        zona: Rect,
        corrido: &mut f64,
        ahora: Instant,
    ) {
        let p = self.paleta;
        h.trazos.push(Trazo::Recorte {
            r: zona.inflate(1.0, 0.0),
            radio: 0.0,
        });
        let encendido = e.encendido(ahora);
        let cursor = |x: f64| Trazo::Caja {
            r: Rect::new(x, zona.y0, x + 1.0, zona.y1),
            radio: 0.0,
            color: p.primary,
        };
        if e.texto.is_empty() {
            let l = self.rotulos.linea(placeholder, letra(), 400.0);
            h.trazos.push(Trazo::Texto {
                l,
                x: zona.x0,
                y: zona.y0,
                color: p.on_surface_variant,
            });
            if encendido {
                h.trazos.push(cursor(zona.x0));
            }
            *corrido = 0.0;
        } else {
            let lt = self.rotulos.linea(&e.texto, letra(), 400.0);
            let xc = if e.cursor == 0 {
                0.0
            } else {
                f64::from(
                    self.rotulos
                        .linea(&e.texto[..e.cursor], letra(), 400.0)
                        .width(),
                ) / self.escala()
            };
            if xc - *corrido > zona.width() - 1.0 {
                *corrido = xc - zona.width() + 1.0;
            } else if xc < *corrido {
                *corrido = xc;
            }
            *corrido = corrido.max(0.0);
            h.trazos.push(Trazo::Texto {
                l: lt,
                x: zona.x0 - corrido.round(),
                y: zona.y0,
                color: p.on_surface,
            });
            if encendido {
                h.trazos.push(cursor((zona.x0 + xc - *corrido).round()));
            }
        }
        h.trazos.push(Trazo::Fin);
    }

    /// Lo que se elige mirando, en una grilla de tres columnas parejas.
    #[allow(clippy::too_many_arguments)]
    fn armar_galeria(
        &mut self,
        h: &mut Hoja,
        si: usize,
        fi: usize,
        titulo: Option<&str>,
        opciones: &[Opcion],
        ancho: f64,
        y: f64,
        ahora: Instant,
    ) -> f64 {
        let p = self.paleta;
        let mut y = y;
        if let Some(t) = titulo {
            y += 4.0;
            let l = self.rotulos.recortada(t, 12.0, ancho);
            let a = self.alto(&l);
            h.trazos.push(Trazo::Texto {
                l,
                x: 0.0,
                y,
                color: p.on_surface_variant,
            });
            y += a + ENTRE;
        }
        let columnas = parejas(ancho, COLUMNAS, ENTRE);
        for (fila, grupo) in opciones.chunks(COLUMNAS).enumerate() {
            // El alto de la fila de la grilla es el de su opción más alta.
            let mut medidas = Vec::new();
            for (c, o) in grupo.iter().enumerate() {
                let w = columnas[c].1;
                medidas.push(self.medir_opcion(o, w));
            }
            let alto = medidas.iter().map(|m| m.alto).fold(0.0, f64::max);
            for (c, (o, m)) in grupo.iter().zip(medidas).enumerate() {
                let k = fila * COLUMNAS + c;
                let (x, w) = columnas[c];
                let lugar = Lugar {
                    seccion: si,
                    fila: fi,
                    opcion: Some(k),
                };
                let obj = Objetivo::Ajuste(lugar);
                let r = Rect::new(x, y, x + w, y + alto);
                let fondo = self.fondo_fila(obj, lugar, ahora);
                h.objetivo(obj, r, true);
                h.trazos.push(Trazo::Grupo {
                    r,
                    escala: self.animos.escala(obj, ahora),
                    dx: 0.0,
                    dy: 0.0,
                    alfa: self.alfa_latido(lugar, ahora),
                });
                h.trazos.push(Trazo::Caja {
                    r,
                    radio: RADIO,
                    color: fondo,
                });
                let ty = y + ((alto - m.textos) / 2.0).floor();
                let aviso = self.avisos.contains_key(&lugar);
                let color = if aviso {
                    p.error
                } else if o.activa {
                    p.tertiary
                } else {
                    p.on_surface
                };
                let a = self.alto(&m.titulo);
                h.trazos.push(Trazo::Texto {
                    l: m.titulo,
                    x: x + FILA_X,
                    y: ty,
                    color,
                });
                if let Some(c) = &o.cursor {
                    let imgs = self.imagenes_cursor(c);
                    let mut cx = x + FILA_X;
                    let alto_muestra = f64::from(crate::menu::lectura::ALTO_MUESTRA);
                    let tope = x + w - FILA_X;
                    h.trazos.push(Trazo::Recorte {
                        r: Rect::new(x + FILA_X, y, tope, y + alto),
                        radio: 0.0,
                    });
                    for img in imgs {
                        let (iw, ih) = (f64::from(img.width.max(1)), f64::from(img.height.max(1)));
                        let mw = (alto_muestra * iw / ih).ceil();
                        let iy = ty + a + ENTRE_TEXTOS;
                        h.trazos.push(Trazo::Imagen {
                            img,
                            r: Rect::new(cx, iy, cx + mw, iy + alto_muestra),
                            radio: 0.0,
                            alfa: 1.0,
                        });
                        cx += mw + 8.0;
                    }
                    h.trazos.push(Trazo::Fin);
                } else if let Some(l) = m.sub {
                    let color = if o.activa {
                        op(&p.tertiary, 0.9)
                    } else {
                        op(&p.on_surface, 0.5)
                    };
                    h.trazos.push(Trazo::Texto {
                        l,
                        x: x + FILA_X,
                        y: ty + a + ENTRE_TEXTOS,
                        color,
                    });
                }
                if o.activa {
                    let t = self.rotulos.linea(TILDE, letra(), 400.0);
                    let tw = self.ancho(&t);
                    let caja = Rect::new(x + w - FILA_X - tw, y, x + w - FILA_X, y + alto);
                    h.trazos.push(Trazo::Glifo {
                        l: t,
                        c: caja.center(),
                        color: p.tertiary,
                    });
                }
                h.trazos.push(Trazo::Fin);
            }
            y += alto + ENTRE;
        }
        y - ENTRE
    }

    fn medir_opcion(&mut self, o: &Opcion, w: f64) -> MedidaOpcion {
        let w_tilde = if o.activa {
            let t = self.rotulos.linea(TILDE, letra(), 400.0);
            self.ancho(&t) + AL_CONTROL
        } else {
            0.0
        };
        let max = (w - 2.0 * FILA_X - w_tilde).max(0.0);
        let titulo = match &o.fuente {
            Some(f) => self.rotulos.recortada_en(&o.titulo, f, 14.0, max),
            None => self.rotulos.recortada(&o.titulo, 14.0, max),
        };
        let mut textos = self.alto(&titulo);
        let sub = if o.cursor.is_some() {
            textos += ENTRE_TEXTOS + f64::from(crate::menu::lectura::ALTO_MUESTRA);
            None
        } else if !o.sub.is_empty() {
            let l = self.rotulos.recortada(&o.sub, 12.0, max);
            textos += ENTRE_TEXTOS + self.alto(&l);
            Some(l)
        } else {
            None
        };
        MedidaOpcion {
            titulo,
            sub,
            textos,
            alto: textos + 2.0 * FILA_Y,
        }
    }

    fn imagenes_cursor(&mut self, c: &Arc<[crate::menu::cursores::Muestra]>) -> Vec<ImageData> {
        let clave = Arc::as_ptr(c) as *const u8 as usize;
        self.cursores
            .entry(clave)
            .or_insert_with(|| {
                c.iter()
                    .map(|m| ImageData {
                        data: Blob::new(Arc::new(m.bgra.clone())),
                        format: ImageFormat::Bgra8,
                        alpha_type: ImageAlphaType::AlphaPremultiplied,
                        width: m.ancho,
                        height: m.alto,
                    })
                    .collect()
            })
            .clone()
    }

    /// Los monitores a escala, para ubicarlos con el mouse: cada uno es una tarjeta que se
    /// arrastra y, al soltarla, se pega al lado más cercano de otro (`monitores::encajar`).
    #[allow(clippy::too_many_arguments)]
    fn armar_dibujo(
        &mut self,
        h: &mut Hoja,
        si: usize,
        fi: usize,
        pantallas: &[Pantalla],
        ancho: f64,
        y: f64,
        ahora: Instant,
    ) -> f64 {
        let p = self.paleta;
        let area = Rect::new(0.0, y, ancho, y + ALTO_DIBUJO);
        let x0 = pantallas.iter().map(|p| p.x).min().unwrap_or(0) as f64;
        let y0 = pantallas.iter().map(|p| p.y).min().unwrap_or(0) as f64;
        let x1 = pantallas.iter().map(|p| p.x + p.ancho).max().unwrap_or(1) as f64;
        let y1 = pantallas.iter().map(|p| p.y + p.alto).max().unwrap_or(1) as f64;
        let e = ((ancho - 2.0 * AIRE_DIBUJO) / (x1 - x0).max(1.0))
            .min((ALTO_DIBUJO - 2.0 * AIRE_DIBUJO) / (y1 - y0).max(1.0))
            .max(0.001);
        let ox = (ancho - (x1 - x0) * e) / 2.0 - x0 * e;
        let oy = y + (ALTO_DIBUJO - (y1 - y0) * e) / 2.0 - y0 * e;
        let escala = Escala {
            escala: e,
            origen: (ox, oy),
        };
        self.escalas.insert((si, fi), escala);
        h.trazos.push(Trazo::Recorte {
            r: area,
            radio: 0.0,
        });
        for (n, pa) in pantallas.iter().enumerate() {
            let lugar = Lugar {
                seccion: si,
                fila: fi,
                opcion: Some(n),
            };
            let o = Objetivo::Pantalla(lugar);
            let (px, py) = escala.a_dibujo(pa.x, pa.y);
            let (mut px, mut py) = (px.round(), py.round());
            let arrastrando = self.arrastre.filter(|a| a.lugar == lugar);
            if let Some(a) = arrastrando {
                px += a.ahora.0 - a.desde.0;
                py += a.ahora.1 - a.desde.1;
            }
            let r = Rect::new(
                px,
                py,
                px + (f64::from(pa.ancho) * e).round(),
                py + (f64::from(pa.alto) * e).round(),
            );
            let fondo = if arrastrando.is_some() {
                op(&p.primary, 0.2)
            } else {
                let luz = self.animos.luz(o, self.encima == Some(o), ahora);
                mezclar(&op(&p.on_surface, 0.06), &op(&p.primary, 0.14), luz)
            };
            h.objetivo(o, r, false);
            h.trazos.push(Trazo::Grupo {
                r,
                escala: 1.0,
                dx: 0.0,
                dy: 0.0,
                alfa: if pa.apagada { 0.6 } else { 1.0 },
            });
            h.trazos.push(Trazo::Caja {
                r,
                radio: RADIO,
                color: fondo,
            });
            h.trazos.push(Trazo::Borde {
                r,
                radio: RADIO,
                ancho: 1.0,
                color: op(&p.on_surface, 0.12),
            });
            h.trazos.push(Trazo::Recorte {
                r: r.inset(-1.0),
                radio: RADIO,
            });
            let max = (r.width() - 10.0).max(0.0);
            let lt = self.rotulos.recortada(&pa.titulo, 12.0, max);
            let sub = if pa.apagada {
                tr!("apagado").to_owned()
            } else {
                format!("{} × {}", pa.ancho, pa.alto) // no-tr
            };
            let ls = self.rotulos.recortada(&sub, 11.0, max);
            let (at, as_) = (self.alto(&lt), self.alto(&ls));
            let ty = (r.center().y - (at + ENTRE_TEXTOS + as_) / 2.0).floor();
            let (wt, ws) = (self.ancho(&lt), self.ancho(&ls));
            h.trazos.push(Trazo::Texto {
                l: lt,
                x: (r.center().x - wt / 2.0).floor(),
                y: ty,
                color: p.on_surface,
            });
            h.trazos.push(Trazo::Texto {
                l: ls,
                x: (r.center().x - ws / 2.0).floor(),
                y: ty + at + ENTRE_TEXTOS,
                color: p.on_surface_variant,
            });
            h.trazos.push(Trazo::Fin);
            h.trazos.push(Trazo::Fin);
        }
        h.trazos.push(Trazo::Fin);
        area.y1
    }

    pub fn pintar(&self, s: &mut Scene, escala: f64, bajada: f64) {
        let p = self.paleta;
        let ahora = Instant::now();
        s.reset();
        let tr = Affine::scale(escala) * Affine::translate((0.0, -bajada.round()));
        panel(s, tr, &p, self.opacidad, self.caja, self.borde);
        trazo::pintar(s, &self.fijo.trazos, tr, escala);
        // El índice y la página, cada uno recortado a lo suyo y corrido por su scroll.
        let fill = morgan_motor::vello::peniko::Fill::NonZero;
        s.push_clip_layer(fill, tr, &self.vista_indice);
        let base = tr
            * Affine::translate((
                self.vista_indice.x0,
                self.vista_indice.y0 - self.scroll_indice.round(),
            ));
        trazo::pintar(s, &self.hoja_indice.trazos, base, escala);
        s.pop_layer();
        s.push_clip_layer(fill, tr, &self.vista_pagina);
        // Una página nueva entra subiendo 8 px mientras se enciende.
        let (dy, alfa) = match self.entrada {
            Some(t0) => {
                let f = ease_out(avance(t0, ENTRA, ahora));
                ((8.0 * (1.0 - f)).round(), f as f32)
            }
            None => (0.0, 1.0),
        };
        let base = tr
            * Affine::translate((
                self.vista_pagina.x0,
                self.vista_pagina.y0 - self.scroll.valor(ahora).round() + dy,
            ));
        if alfa < 1.0 {
            s.push_layer(
                fill,
                morgan_motor::vello::peniko::BlendMode::default(),
                alfa,
                tr,
                &self.vista_pagina,
            );
        }
        trazo::pintar(s, &self.hoja.trazos, base, escala);
        if alfa < 1.0 {
            s.pop_layer();
        }
        s.pop_layer();
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        self.animos.anda(ahora)
            || self.scroll.anda(ahora)
            || self.despliegue_indice.anda(ahora)
            || self.revelados.values().any(|r| r.anda(ahora))
            || self.entrada.is_some_and(|t| avance(t, ENTRA, ahora) < 1.0)
            || !self.aplicando.is_empty()
            || !self.avisos.is_empty()
            || self.resalte.is_some_and(|(_, t)| {
                ahora.saturating_duration_since(t)
                    < crate::animaciones::dur(RESALTE + RESALTE_FUNDIDO)
            })
    }

    pub fn proximo_parpadeo(&self, ahora: Instant) -> Option<Instant> {
        match &self.campo {
            Some(c) => c.entrada.proximo_cambio(ahora),
            None => self.buscador.proximo_cambio(ahora),
        }
    }

    /// Sobre un campo: el cursor de texto; sobre un monitor del dibujo: la mano.
    pub fn forma_puntero(&self) -> Forma {
        let Some((x, y)) = self.puntero else {
            return Forma::Normal;
        };
        if self.buscador_caja.contains((x, y)) {
            return Forma::Texto;
        }
        match self.encima {
            Some(Objetivo::Pantalla(_)) => Forma::Mano,
            _ => Forma::Normal,
        }
    }

    // ── Ir y mostrar ──

    fn ir_ahora(&mut self, seccion: usize, fila: Option<usize>, suave: bool, ahora: Instant) {
        let destino = match fila {
            Some(f) => self
                .hoja
                .objetivos
                .iter()
                .find(|(o, _)| {
                    matches!(o, Objetivo::Ajuste(l) | Objetivo::Pantalla(l)
                        if l.seccion == seccion && l.fila == f)
                })
                .map(|(_, r)| r.y0 - 40.0),
            None if seccion > 0 => self.cabeceras.get(seccion).map(|y| y - 8.0),
            None => Some(0.0),
        };
        let v = destino.unwrap_or(0.0).clamp(0.0, self.max_scroll());
        if suave {
            self.scroll.ir(v, ahora);
        } else {
            self.scroll.poner(v);
        }
        if let Some(f) = fila {
            self.resalte = Some(((seccion, f), ahora));
        }
    }

    /// Que lo elegido quede a la vista (sin deslizar).
    fn mostrar_elegido(&mut self) {
        let Some(o) = self.elegido else { return };
        let Some(r) = self.hoja.caja(o) else { return };
        let alto = self.vista_pagina.height();
        let valor = self.scroll.hasta;
        let v = if r.y0 < valor {
            r.y0
        } else if r.y1 > valor + alto {
            r.y1 - alto
        } else {
            return;
        };
        self.scroll.poner(v.clamp(0.0, self.max_scroll()));
    }

    // ── Teclado ──

    #[allow(clippy::too_many_arguments)]
    pub fn tecla(
        &mut self,
        k: Keysym,
        utf8: Option<&str>,
        base: u32,
        mods: [bool; 4],
    ) -> Vec<Pedido> {
        let mut pedidos = Vec::new();
        let ctrl = mods[1];
        if self.grabando {
            pedidos.push(Pedido::Gtk(EventoAjustes::Tecla {
                keysym: k.raw(),
                base,
                mods,
            }));
            return pedidos;
        }
        // Escribiendo en el campo de una fila, las teclas son de él.
        if let Some(mut c) = self.campo.take() {
            match k {
                Keysym::Escape => return pedidos,
                Keysym::Return | Keysym::KP_Enter | Keysym::ISO_Enter => {
                    let nuevo = c.entrada.texto.trim().to_owned();
                    if let Some(Fila::Ajuste {
                        control: Control::Texto { valor, que, .. },
                        ..
                    }) = self.fila(c.lugar)
                        && nuevo != valor
                    {
                        let que = que.con_texto(&nuevo);
                        self.hacer(c.lugar, que, &mut pedidos);
                    }
                    return pedidos;
                }
                _ => {
                    editar(
                        &mut c.entrada,
                        k,
                        utf8,
                        ctrl,
                        &mut c.reemplazar,
                        &mut pedidos,
                    );
                    self.campo = Some(c);
                    return pedidos;
                }
            }
        }
        let en_pagina = self.zona == Zona::Pagina;
        let tipeando = !en_pagina && !self.buscador.texto.is_empty();
        match k {
            Keysym::Left | Keysym::Right if tipeando => {
                let mut no = false;
                editar(&mut self.buscador, k, utf8, ctrl, &mut no, &mut pedidos);
            }
            Keysym::Escape if en_pagina => self.a_la_zona(Zona::Indice),
            Keysym::Escape if !self.buscador.texto.is_empty() => {
                self.buscador = Entrada::default();
                pedidos.push(Pedido::Gtk(EventoAjustes::Texto(String::new())));
            }
            Keysym::Escape => pedidos.push(Pedido::Gtk(EventoAjustes::Cerrado)),
            Keysym::Left | Keysym::ISO_Left_Tab if en_pagina => self.a_la_zona(Zona::Indice),
            Keysym::Return | Keysym::KP_Enter | Keysym::ISO_Enter | Keysym::space if en_pagina => {
                if let Some(o) = self.elegido {
                    self.activar(o, &mut pedidos);
                }
            }
            Keysym::Down | Keysym::Tab if en_pagina => self.mover_en_pagina(1),
            Keysym::Up if en_pagina => self.mover_en_pagina(-1),
            Keysym::Page_Down | Keysym::Page_Up => {
                let paso = if k == Keysym::Page_Down { 1.0 } else { -1.0 };
                let v = self.scroll.hasta + paso * self.vista_pagina.height() * 0.9;
                self.scroll.poner(v.clamp(0.0, self.max_scroll()));
            }
            Keysym::Right | Keysym::Tab | Keysym::Return | Keysym::KP_Enter | Keysym::ISO_Enter => {
                self.a_la_zona(Zona::Pagina);
            }
            Keysym::Left => {
                // De una sección, a su categoría.
                let i = self.indice.elegida;
                if self
                    .indice
                    .filas
                    .get(i)
                    .is_some_and(|f| f.seccion.is_some())
                    && let Some(c) = (0..i).rev().find(|&j| self.indice.filas[j].glifo.is_some())
                {
                    self.elegir_indice(c, &mut pedidos);
                }
            }
            Keysym::Down => self.mover_en_indice(1, &mut pedidos),
            Keysym::Up | Keysym::ISO_Left_Tab => self.mover_en_indice(-1, &mut pedidos),
            _ => {
                let antes = self.buscador.texto.clone();
                let mut no = false;
                editar(&mut self.buscador, k, utf8, ctrl, &mut no, &mut pedidos);
                if self.buscador.texto != antes {
                    self.zona = Zona::Indice;
                    self.elegido = None;
                    pedidos.push(Pedido::Gtk(EventoAjustes::Texto(
                        self.buscador.texto.clone(),
                    )));
                }
            }
        }
        pedidos
    }

    pub fn pegar(&mut self, texto: &str) -> Vec<Pedido> {
        let linea = texto.lines().next().unwrap_or("");
        if let Some(c) = &mut self.campo {
            if c.reemplazar {
                c.entrada = Entrada::default();
                c.reemplazar = false;
            }
            c.entrada.insertar(linea);
            return Vec::new();
        }
        let antes = self.buscador.texto.clone();
        self.buscador.insertar(linea);
        if self.buscador.texto == antes {
            return Vec::new();
        }
        self.zona = Zona::Indice;
        self.elegido = None;
        vec![Pedido::Gtk(EventoAjustes::Texto(
            self.buscador.texto.clone(),
        ))]
    }

    fn a_la_zona(&mut self, zona: Zona) {
        self.zona = zona;
        match zona {
            Zona::Indice => self.elegido = None,
            Zona::Pagina => {
                // Lo primero que se toca de la sección (o la fila) elegida en el índice.
                let destino = self
                    .indice
                    .filas
                    .get(self.indice.elegida)
                    .and_then(|f| f.destino);
                let e = &self.hoja.enfocables;
                let lugar = |o: &Objetivo| match o {
                    Objetivo::Ajuste(l) | Objetivo::OpcionAjuste(l) => Some(*l),
                    _ => None,
                };
                let primero = destino
                    .and_then(|(s, f)| {
                        e.iter()
                            .find(|o| {
                                lugar(o).is_some_and(|l| (l.seccion, l.fila) >= (s, f.unwrap_or(0)))
                            })
                            .copied()
                    })
                    .or_else(|| e.first().copied());
                if primero.is_none() {
                    self.zona = Zona::Indice;
                }
                self.elegido = primero;
                self.mostrar_elegido();
            }
        }
    }

    fn mover_en_pagina(&mut self, paso: i64) {
        let e = &self.hoja.enfocables;
        if e.is_empty() {
            return;
        }
        let n = e.len() as i64;
        let nueva = match self.elegido.and_then(|a| e.iter().position(|x| *x == a)) {
            Some(p) => (p as i64 + paso).clamp(0, n - 1),
            None => 0,
        };
        self.elegido = Some(e[nueva as usize]);
        self.mostrar_elegido();
    }

    fn mover_en_indice(&mut self, paso: i64, pedidos: &mut Vec<Pedido>) {
        let n = self.indice.filas.len() as i64;
        if n == 0 {
            return;
        }
        let nueva = (self.indice.elegida as i64 + paso).rem_euclid(n) as usize;
        self.elegir_indice(nueva, pedidos);
    }

    fn elegir_indice(&mut self, i: usize, pedidos: &mut Vec<Pedido>) {
        self.indice.elegida = i;
        self.mostrar_en_indice(i);
        pedidos.push(Pedido::Gtk(EventoAjustes::Navegar {
            version: self.indice.version,
            indice: i,
        }));
    }

    // ── Lo que hace cada cosa ──

    fn fila(&self, l: Lugar) -> Option<Fila> {
        self.secciones
            .as_ref()?
            .get(l.seccion)?
            .filas
            .get(l.fila)
            .cloned()
    }

    fn hacer(&mut self, lugar: Lugar, que: Que, pedidos: &mut Vec<Pedido>) {
        self.zona = Zona::Pagina;
        self.aplicando.insert(lugar);
        if self.aplicando.len() == 1 {
            self.latido = Instant::now();
        }
        pedidos.push(Pedido::Gtk(EventoAjustes::Hacer {
            version: self.version,
            lugar,
            que,
        }));
    }

    fn activar(&mut self, o: Objetivo, pedidos: &mut Vec<Pedido>) {
        let ahora = Instant::now();
        match o {
            Objetivo::Indice(i) => self.elegir_indice(i, pedidos),
            Objetivo::OpcionAjuste(l) => {
                let fila = self.fila(Lugar { opcion: None, ..l });
                if let Some(Fila::Ajuste {
                    control: Control::Lista(ops),
                    ..
                }) = fila
                    && let Some(op) = l.opcion.and_then(|k| ops.get(k))
                {
                    self.elegido = Some(o);
                    self.hacer(Lugar { opcion: None, ..l }, op.que.clone(), pedidos);
                }
            }
            Objetivo::Ajuste(l) => {
                self.elegido = Some(o);
                self.zona = Zona::Pagina;
                match self.fila(Lugar { opcion: None, ..l }) {
                    Some(Fila::Galeria { opciones, .. }) => {
                        if let Some(op) = l.opcion.and_then(|k| opciones.get(k)) {
                            self.hacer(l, op.que.clone(), pedidos);
                        }
                    }
                    Some(Fila::Ajuste { control, .. }) => match control {
                        Control::Casilla { que, .. } | Control::Boton(que) => {
                            self.hacer(l, que, pedidos);
                        }
                        // El primer toque pide confirmar (por 4 s); el segundo lo hace.
                        Control::Confirmar(que) => {
                            if self
                                .avisos
                                .get(&l)
                                .is_some_and(|(a, _)| *a == Aviso::Confirmando)
                            {
                                self.avisos.remove(&l);
                                self.hacer(l, que, pedidos);
                            } else {
                                self.avisos.insert(l, (Aviso::Confirmando, ahora));
                            }
                        }
                        Control::Texto { valor, .. } => {
                            self.cerrar_listas(ahora);
                            let mut e = Entrada::default();
                            e.insertar(&valor);
                            self.campo = Some(Campo {
                                lugar: l,
                                entrada: e,
                                reemplazar: true,
                                corrido: 0.0,
                            });
                        }
                        Control::Lista(_) => {
                            let abrir = self.abierta != Some(l);
                            self.cerrar_listas(ahora);
                            if abrir {
                                self.abierta = Some(l);
                                self.revelados
                                    .entry(l)
                                    .or_insert_with(|| Revelado::new(false))
                                    .ir(true, crate::animaciones::dur(DESPLIEGUE), ahora);
                            }
                        }
                        Control::Nada => {}
                    },
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn cerrar_listas(&mut self, ahora: Instant) {
        if let Some(a) = self.abierta.take()
            && let Some(r) = self.revelados.get_mut(&a)
        {
            r.ir(false, crate::animaciones::dur(DESPLIEGUE), ahora);
        }
    }

    // ── Puntero ──

    /// Lo que hay bajo el puntero, en el índice o en la página.
    fn bajo(&self, x: f64, y: f64) -> Option<Objetivo> {
        if self.vista_indice.contains((x, y)) {
            let p = Point::new(
                x - self.vista_indice.x0,
                y - self.vista_indice.y0 + self.scroll_indice.round(),
            );
            return self
                .hoja_indice
                .objetivos
                .iter()
                .rev()
                .find(|(_, r)| r.contains(p))
                .map(|(o, _)| *o);
        }
        if self.vista_pagina.contains((x, y)) {
            let p = self.en_pagina(x, y);
            return self
                .hoja
                .objetivos
                .iter()
                .rev()
                .find(|(_, r)| r.contains(p))
                .map(|(o, _)| *o);
        }
        None
    }

    fn en_pagina(&self, x: f64, y: f64) -> Point {
        Point::new(
            x - self.vista_pagina.x0,
            y - self.vista_pagina.y0 + self.scroll.valor(Instant::now()).round(),
        )
    }

    pub fn mover(&mut self, x: f64, y: f64) {
        self.puntero = Some((x, y));
        if let Some(a) = &mut self.arrastre {
            a.ahora = (x, y);
            return;
        }
        self.encima = self.bajo(x, y);
    }

    pub fn salir(&mut self) {
        self.puntero = None;
        self.encima = None;
    }

    pub fn apretar(&mut self, x: f64, y: f64) {
        let o = self.bajo(x, y);
        self.afuera = !self.caja.contains((x, y));
        self.apretado = o;
        if let Some(o) = o {
            self.animos.apretar(o, true, Instant::now());
            if let Objetivo::Pantalla(l) = o {
                self.arrastre = Some(Arrastre {
                    lugar: l,
                    desde: (x, y),
                    ahora: (x, y),
                });
            }
        }
        // Tocar otra cosa deja el campo de una fila sin guardar.
        if self
            .campo
            .as_ref()
            .is_some_and(|c| o != Some(Objetivo::Ajuste(c.lugar)))
        {
            self.campo = None;
        }
    }

    pub fn soltar(&mut self, x: f64, y: f64, izquierdo: bool) -> Vec<Pedido> {
        let mut pedidos = Vec::new();
        let ahora = Instant::now();
        let apretado = self.apretado.take();
        if let Some(o) = apretado {
            self.animos.apretar(o, false, ahora);
        }
        if let Some(a) = self.arrastre.take() {
            self.soltar_pantalla(a, &mut pedidos);
            return pedidos;
        }
        if apretado.is_none() && self.afuera && !self.caja.contains((x, y)) {
            pedidos.push(Pedido::Gtk(EventoAjustes::Cerrado));
            return pedidos;
        }
        if let Some(o) = apretado
            && izquierdo
            && self.bajo(x, y) == Some(o)
        {
            self.activar(o, &mut pedidos);
        }
        pedidos
    }

    fn soltar_pantalla(&mut self, a: Arrastre, pedidos: &mut Vec<Pedido>) {
        let Some(Fila::Disposicion(pantallas)) = self.fila(Lugar {
            opcion: None,
            ..a.lugar
        }) else {
            return;
        };
        let (Some(n), Some(e)) = (
            a.lugar.opcion,
            self.escalas.get(&(a.lugar.seccion, a.lugar.fila)).copied(),
        ) else {
            return;
        };
        let Some(p) = pantallas.get(n) else { return };
        let (dx, dy) = (a.ahora.0 - a.desde.0, a.ahora.1 - a.desde.1);
        if dx.hypot(dy) < TOQUE {
            return;
        }
        let (px, py) = e.a_dibujo(p.x, p.y);
        let (lx, ly) = e.a_escritorio(px + dx, py + dy);
        let (x, y) = encajar(&pantallas, n, lx, ly);
        if (x, y) != (p.x, p.y) {
            let que = Que::Monitor(p.nombre.clone(), p.lugar(x, y));
            self.hacer(a.lugar, que, pedidos);
        }
    }

    pub fn rueda(&mut self, dy: f64, discreto: bool) {
        let Some((x, y)) = self.puntero else { return };
        if self.vista_indice.contains((x, y)) {
            let paso = if discreto {
                self.vista_indice.height().powf(2.0 / 3.0)
            } else {
                1.0
            };
            self.scroll_indice =
                (self.scroll_indice + dy * paso).clamp(0.0, self.max_scroll_indice());
        } else {
            let paso = if discreto {
                self.vista_pagina.height().powf(2.0 / 3.0)
            } else {
                1.0
            };
            let v = (self.scroll.hasta + dy * paso).clamp(0.0, self.max_scroll());
            self.scroll.poner(v);
        }
        self.encima = self.bajo(x, y);
    }
}

pub enum Forma {
    Normal,
    Texto,
    Mano,
}

struct MedidaOpcion {
    titulo: Rc<morgan_motor::parley::Layout<u8>>,
    sub: Option<Rc<morgan_motor::parley::Layout<u8>>>,
    textos: f64,
    alto: f64,
}

/// Las columnas de una grilla pareja (`column-homogeneous`): dónde empieza cada una y su
/// ancho, en enteros, el resto a las primeras.
pub(super) fn parejas(total: f64, n: usize, espacio: f64) -> Vec<(f64, f64)> {
    let libre = (total - espacio * (n as f64 - 1.0)).max(0.0) as i64;
    let n_i = n as i64;
    let mut x = 0.0;
    (0..n_i)
        .map(|i| {
            let w = (libre / n_i + i64::from(i < libre % n_i)) as f64;
            let c = (x, w);
            x += w + espacio;
            c
        })
        .collect()
}

/// Una tecla de edición en `e` (`reemplazar`: lo puesto está elegido entero).
fn editar(
    e: &mut Entrada,
    k: Keysym,
    utf8: Option<&str>,
    ctrl: bool,
    reemplazar: &mut bool,
    pedidos: &mut Vec<Pedido>,
) {
    let antes = std::mem::replace(reemplazar, false);
    match k {
        Keysym::BackSpace | Keysym::Delete if antes => *e = Entrada::default(),
        Keysym::BackSpace if ctrl => e.borrar_palabra(),
        Keysym::BackSpace => e.borrar_atras(),
        Keysym::Delete => e.borrar_adelante(),
        Keysym::Left => e.izquierda(),
        Keysym::Right => e.derecha(),
        Keysym::Home => e.inicio(),
        Keysym::End => e.fin(),
        Keysym::v | Keysym::V if ctrl => {
            *reemplazar = antes;
            pedidos.push(Pedido::Pegar);
        }
        Keysym::u if ctrl => {
            e.inicio();
            e.texto.clear();
        }
        _ if ctrl => *reemplazar = antes,
        _ => match utf8.filter(|s| !s.chars().all(char::is_control)) {
            Some(s) => {
                if antes {
                    *e = Entrada::default();
                }
                e.insertar(s);
            }
            None => *reemplazar = antes,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columnas_parejas_con_el_resto_a_las_primeras() {
        let c = parejas(100.0, 3, 8.0);
        assert_eq!(c, vec![(0.0, 28.0), (36.0, 28.0), (72.0, 28.0)]);
        let c = parejas(102.0, 3, 8.0);
        assert_eq!(
            c.iter().map(|x| x.1).collect::<Vec<_>>(),
            [29.0, 29.0, 28.0]
        );
    }

    #[test]
    fn lo_puesto_elegido_se_reemplaza_al_escribir() {
        let mut e = Entrada::default();
        e.insertar("viejo");
        let mut r = true;
        let mut p = Vec::new();
        editar(&mut e, Keysym::n, Some("n"), false, &mut r, &mut p);
        assert_eq!(e.texto, "n");
        assert!(!r);
        let mut r = true;
        editar(&mut e, Keysym::End, None, false, &mut r, &mut p);
        assert_eq!(e.texto, "n");
        editar(&mut e, Keysym::a, Some("a"), false, &mut r, &mut p);
        assert_eq!(e.texto, "na");
    }
}
