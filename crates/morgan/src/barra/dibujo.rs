//! La barra en una `Scene` de vello. Las medidas y los colores son los de la versión con GTK
//! (`.barra` en `estilo.css` hasta 0.1.1):
//!
//! - 28 px de alto; a la izquierda los workspaces (10 px del borde, cada uno con 3 px de relleno
//!   a los lados y 2 arriba, y 1 y 2 de margen), al medio «fecha › hora» (rellenos de 5, 3 y 5,
//!   y 2 abajo) y a la derecha, desde 10 px del borde: la campana (4 y 3 de relleno, 2 de
//!   margen), el cajón (la flecha con 4 por lado y la bandeja que se desliza, con 2 y 8 de
//!   relleno e íconos de 14 cada 24) y, si hay algo que avisar, la raya, la grabación y los
//!   glifos de estado, a `ENTRE_AVISOS` de tinta a tinta;
//! - la grabación es un glifo y un texto a `ENTRE_GLIFO_Y_TEXTO` de tinta a tinta, sin depender
//!   del ancho del glifo;
//! - entre workspaces, a los lados del `›` y entre avisos, 11 px de tinta a tinta (Roberto,
//!   2026-11-23, elegido en vivo entre 10, 11 y 12; fuera de la grilla de 4 a propósito);
//! - al pasar el puntero o con lo suyo abierto, un paso por el degradé del anillo en 150 ms;
//!   apretado, al 92 % de su tamaño (suelto, vuelve en 150 ms);
//! - el globo de ayuda es el del dock: el marco y las medidas de `forma::menu`.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use morgan_motor::color::{Color, Paleta, con_alfa, mezclar, pincel};
use morgan_motor::curva::{Transicion, ease_in_out};
use morgan_motor::forma::menu::{
    BORDE_Y_RELLENO, FILA_Y, GLOBO_TEXTO_MAX, GLOBO_X, PX as PX_GLOBO, SOMBRA, ancho_globo,
};
use morgan_motor::forma::{degrade_horizontal, marco};
use morgan_motor::parley::Layout;
use morgan_motor::texto::{self, Estilo, Textos, pt};
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Point, Rect, RoundedRect};
use morgan_motor::vello::peniko::{Fill, ImageBrush, ImageData};

use super::modelo::{self, Boton};
use crate::compositor::Espacios;

pub const ALTO: u32 = 28;
const ROL: u8 = 0;
const PX: f32 = 14.0;
const PX_SEPARADOR: f32 = 13.0;
const PX_GLIFO: f32 = 15.5;
const ICONO: f64 = 14.0;
/// El lado de los íconos de la bandeja, para quien los arma.
pub const LADO_BANDEJA: u32 = ICONO as u32;
const ENTRE_ICONOS: f64 = 10.0;
/// Las apps abiertas, con el dock en la barra: del tamaño y a la distancia de los íconos de la
/// bandeja, y apartadas de lo demás a `ENTRE_AVISOS` de tinta, como todo en la barra.
const ICONO_APP: f64 = ICONO;
const ENTRE_APPS: f64 = ENTRE_ICONOS;
/// Lo mínimo que queda libre antes del reloj: lo que no entra va al «+N».
const ANTES_DEL_RELOJ: f64 = 16.0;
/// La raya de la enfocada: la del dock, más corta.
const RAYA_FOCO: (f64, f64) = (8.0, 2.0);
/// Las rayas que apartan (la tira de los workspaces, la bandeja de los avisos): 1 px y 12 de alto.
const RAYA_ALTO: f64 = 12.0;
/// Entre la tinta de un aviso (grabación, cada glifo de estado, la raya) y la del siguiente. Cada
/// glifo de estado va aparte: en un solo texto el espacio es más angosto que los glifos y se
/// pisaban, y con lugares iguales el micrófono (angosto) quedaba con más aire que la pantalla.
const ENTRE_AVISOS: f64 = 11.0;
/// La distancia que el espacio de la fuente dejaba entre el glifo de la grabación y su tiempo.
const ENTRE_GLIFO_Y_TEXTO: f64 = 9.0;
const APRETADO: f32 = 0.92;
pub const TRANSICION: Duration = Duration::from_millis(150);
/// El `Revealer` de la bandeja.
pub const REVELAR: Duration = Duration::from_millis(300);

const CAMPANA: &str = "󰨝";
const FLECHA: &str = "󰅁";
/// Lo que ocupa la sombra alrededor del globo, de cada lado.
pub const GLOBO_SOMBRA: f64 = SOMBRA;
/// La imagen va a 14 px lógicos, sea del tamaño que sea (se arma para la escala más alta).
#[derive(Clone)]
pub struct Icono {
    pub clave: String,
    pub tooltip: String,
    pub imagen: Option<ImageData>,
}

/// Una app abierta, con el dock en la barra.
#[derive(Clone)]
pub struct App {
    pub clave: String,
    pub nombre: String,
    pub imagen: Option<ImageData>,
    pub ventanas: Vec<Ventana>,
}

#[derive(Clone)]
pub struct Ventana {
    pub ws: i64,
    pub foco: bool,
}

/// Lo que se ve en la barra, igual en todas las salidas salvo los workspaces.
#[derive(Default)]
pub struct Estado {
    pub espacios: Espacios,
    pub fijos: BTreeMap<String, Vec<i64>>,
    pub fecha: String,
    pub hora: String,
    pub avisos: usize,
    pub no_molestar: bool,
    pub centro: bool,
    pub privacidad: Vec<&'static str>,
    pub tooltip_privacidad: String,
    pub grabacion: Option<String>,
    pub bandeja: Vec<Icono>,
    /// La clave del ícono con su menú abierto.
    pub menu: Option<String>,
    /// Con el dock en la barra; si no, vacío.
    pub apps: Vec<App>,
    /// Cada app junto a su workspace, en lugar de en tira.
    pub por_espacio: bool,
    /// La bandeja siempre a la vista, sin la flecha.
    pub bandeja_fija: bool,
    /// El dock va en la barra: los avisos junto a la campana y la bandeja después de la raya.
    pub dock_en_barra: bool,
    /// La clave de la app con su menú abierto.
    pub menu_app: Option<String>,
}

pub const TOOLTIP_GRABACION: &str = tr_const!("Grabando la pantalla\nClick: parar");

impl Estado {
    pub fn tooltip(&self, q: Que, rot: Option<&Rotulos>) -> Option<String> {
        let t = match q {
            // Solo el nombre, como el globo del dock de una app sin ventanas: las ventanas
            // están en el menú.
            Que::App { app, .. } => self.apps.get(app)?.nombre.clone(),
            Que::Mas(desde) => rot?
                .apps
                .get(desde..)?
                .iter()
                .filter_map(|e| self.apps.get(e.app).map(|a| a.nombre.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
            Que::Campana => modelo::campana(self.avisos, self.no_molestar).1,
            Que::Privacidad => self.tooltip_privacidad.clone(),
            Que::Grabacion => morgan_idioma::t(TOOLTIP_GRABACION).into(),
            Que::Icono(i) => self.bandeja.get(i)?.tooltip.clone(),
            _ => return None,
        };
        (!t.is_empty()).then_some(t)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Que {
    Espacio(i64),
    /// La app `app` de `Estado::apps`; junto a un workspace, `ws` es su id, y si no, 0.
    App {
        app: usize,
        ws: i64,
    },
    /// Las apps que no entran, desde esa entrada de `Rotulos::apps`.
    Mas(usize),
    Reloj,
    Grabacion,
    Privacidad,
    Flecha,
    Icono(usize),
    Campana,
}

impl Que {
    /// Lo que no se aprieta ni cambia con el puntero.
    pub fn quieto(self) -> bool {
        matches!(self, Que::Privacidad | Que::Flecha | Que::Mas(_))
    }
}

/// Los textos de una salida, en píxeles del buffer. Se rehacen cuando cambia el estado.
pub struct Rotulos {
    pub version: u64,
    pub escala: f32,
    espacios: Vec<(Boton, Layout<u8>)>,
    fecha: Layout<u8>,
    separador: Layout<u8>,
    hora: Layout<u8>,
    campana: Layout<u8>,
    flecha: Layout<u8>,
    privacidad: Vec<Layout<u8>>,
    grabacion: Option<Partes>,
    /// Las apps de esta salida, en orden; junto a su workspace, agrupadas por él.
    pub apps: Vec<Entrada>,
    por_espacio: bool,
    bandeja_fija: bool,
    dock_en_barra: bool,
    /// «+1», «+2»…: lo que no entra.
    mas: Vec<Layout<u8>>,
}

/// Una app en la barra de una salida.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entrada {
    pub app: usize,
    /// El workspace junto al que va, o 0 en la tira.
    pub ws: i64,
    pub foco: bool,
}

/// Las apps de una salida (las que tienen ventanas en sus workspaces, `ids`): en tira, una por
/// app; junto a su workspace, una por app en cada workspace donde tiene ventanas.
pub fn entradas(apps: &[App], ids: &[i64], por_espacio: bool) -> Vec<Entrada> {
    if por_espacio {
        ids.iter()
            .flat_map(|&ws| {
                apps.iter().enumerate().filter_map(move |(app, a)| {
                    let suyas = a.ventanas.iter().filter(|v| v.ws == ws);
                    let mut suyas = suyas.peekable();
                    suyas.peek()?;
                    let foco = suyas.any(|v| v.foco);
                    Some(Entrada { app, ws, foco })
                })
            })
            .collect()
    } else {
        apps.iter()
            .enumerate()
            .filter(|(_, a)| a.ventanas.iter().any(|v| ids.contains(&v.ws)))
            .map(|(app, a)| Entrada {
                app,
                ws: 0,
                foco: a.ventanas.iter().any(|v| v.foco && ids.contains(&v.ws)),
            })
            .collect()
    }
}

/// Un aviso hecho de pedazos de texto en la misma línea, cada uno a su distancia de tinta del
/// anterior (`entre[i]`, antes del pedazo `i + 1`).
struct Partes {
    textos: Vec<Layout<u8>>,
    entre: Vec<f64>,
}

impl Rotulos {
    pub fn armar(t: &mut Textos, e: &Estado, salida: &str, escala: f32, version: u64) -> Self {
        let fijos = e.fijos.get(salida).map(Vec::as_slice).unwrap_or_default();
        let botones = modelo::botones(&e.espacios, salida, fijos);
        let ids: Vec<i64> = botones.iter().map(|b| b.id).collect();
        let apps = entradas(&e.apps, &ids, e.por_espacio);
        let mas = (1..=apps.len())
            .map(|n| t.armar(&[(&format!("+{n}"), ROL)], PX, escala))
            .collect();
        let espacios = botones
            .into_iter()
            .map(|b| {
                let peso = if b.activo { 700.0 } else { 500.0 };
                let l = t.armar_con_peso(&[(&b.etiqueta, ROL)], PX, peso, escala);
                (b, l)
            })
            .collect();
        let partes = |t: &mut Textos, textos: &[&str], entre: &[f64]| Partes {
            textos: textos.iter().map(|s| t.armar(&[(s, ROL)], PX, escala)).collect(),
            entre: entre.to_vec(),
        };
        Rotulos {
            version,
            escala,
            espacios,
            fecha: t.armar_con_peso(&[(&e.fecha, ROL)], pt(10.0), 500.0, escala),
            separador: t.armar(&[("›", ROL)], PX_SEPARADOR, escala),
            hora: t.armar_con_peso(&[(&e.hora, ROL)], pt(10.0), 700.0, escala),
            campana: t.armar(&[(CAMPANA, ROL)], PX_GLIFO, escala),
            flecha: t.armar(&[(FLECHA, ROL)], PX, escala),
            privacidad: e
                .privacidad
                .iter()
                .map(|g| t.armar(&[(g, ROL)], PX_GLIFO, escala))
                .collect(),
            grabacion: e
                .grabacion
                .as_deref()
                .map(|g| partes(t, &["󰑋", g], &[ENTRE_GLIFO_Y_TEXTO])),
            apps,
            por_espacio: e.por_espacio,
            bandeja_fija: e.bandeja_fija,
            dock_en_barra: e.dock_en_barra,
            mas,
        }
    }

    fn ancho(&self, l: &Layout<u8>) -> f64 {
        texto::caja(l).0 / f64::from(self.escala)
    }

    /// Dónde empieza y termina la tinta de `l`, desde su borde izquierdo, en píxeles lógicos:
    /// los píxeles que toca, aunque sea en parte (el borde suavizado también se ve).
    fn tinta_x(&self, l: &Layout<u8>) -> (f64, f64) {
        let e = f64::from(self.escala);
        texto::tinta(l).map_or((0.0, self.ancho(l)), |[x0, _, x1, _]| {
            (x0.floor() / e, x1.ceil() / e)
        })
    }

    /// La caja de `l` (como etiqueta, `alto` de alto) para que su tinta termine en `derecha`, y
    /// dónde empieza esa tinta.
    ///
    /// La esquina se elige acá, en un píxel entero del buffer, y la caja se arma para que
    /// `texto_en` caiga en esa misma: si cada uno redondeara por su lado, los espacios entre
    /// tintas variarían en un píxel.
    fn rotulo_hasta(&self, l: &Layout<u8>, derecha: f64, alto: f64) -> (Rect, f64) {
        let e = f64::from(self.escala);
        let (t0, t1) = self.tinta_x(l);
        let esquina = (derecha * e - t1 * e).round();
        // Un cuarto de píxel adentro, para que el `floor` de `texto::esquina` no caiga al anterior.
        let x0 = (esquina + 0.25) / e;
        (
            Rect::new(x0, 0.0, x0 + self.ancho(l), alto),
            esquina / e + t0,
        )
    }

    /// Los pedazos de `p`, el último con la tinta terminando en `derecha`: la caja de cada uno y
    /// dónde empieza la tinta del primero.
    fn partes_hasta(&self, p: &Partes, mut derecha: f64, alto: f64) -> (Vec<Rect>, f64) {
        let mut cajas = Vec::new();
        for (i, l) in p.textos.iter().enumerate().rev() {
            let (r, izq) = self.rotulo_hasta(l, derecha, alto);
            cajas.push(r);
            derecha = izq;
            if let Some(j) = i.checked_sub(1) {
                derecha -= p.entre.get(j).copied().unwrap_or(0.0);
            }
        }
        cajas.reverse();
        (cajas, derecha)
    }

    /// Lo mismo para un glifo suelto (`texto_en` con `glifo`, que lo centra por su tinta): la
    /// caja de su tinta, con la tinta terminando en `derecha`.
    fn glifo_hasta(&self, l: &Layout<u8>, derecha: f64, alto: f64) -> (Rect, f64) {
        let e = f64::from(self.escala);
        let (t0, t1) = self.tinta_x(l);
        let esquina = (derecha * e - t1 * e).round() / e;
        (
            Rect::new(esquina + t0, 0.0, esquina + t1, alto),
            esquina + t0,
        )
    }
}

pub struct Pieza {
    pub que: Que,
    /// Lo que ocupa en la barra (donde responde al puntero), en píxeles lógicos.
    pub caja: Rect,
    /// Sin el relleno: donde va el texto o la imagen.
    pub contenido: Rect,
}

/// Dónde cae cada cosa en una barra de `ancho` píxeles lógicos.
pub struct Lugar {
    pub piezas: Vec<Pieza>,
    /// La flecha y lo que se ve de la bandeja: pasar por acá la abre.
    pub cajon: Rect,
    /// Lo que se ve de la bandeja mientras se desliza.
    pub revelador: Rect,
    reloj: [Rect; 3],
    /// La tinta de cada glifo de estado.
    estados: Vec<Rect>,
    /// La caja de cada pedazo de la grabación.
    grabacion: Vec<Rect>,
    /// La raya entre los workspaces y la tira de apps.
    raya_apps: Option<Rect>,
    /// La raya que cierra la bandeja, con el dock en la barra: se desliza con ella.
    raya_bandeja: Option<Rect>,
    /// La raya entre los avisos y la campana, con el dock en la barra.
    raya_avisos: Option<Rect>,
    /// La raya bajo la app enfocada.
    foco: Option<Rect>,
}

fn raya(x0: f64, h: f64) -> Rect {
    Rect::new(x0, (h - RAYA_ALTO) / 2.0, x0 + 1.0, (h + RAYA_ALTO) / 2.0)
}

/// Los workspaces y las apps, desde el borde izquierdo, dejando afuera las últimas `ocultas`
/// entradas (van al «+N»). Entre la tinta de un workspace, la raya, un grupo de íconos y el «+N»,
/// `ENTRE_AVISOS`; entre íconos, `ENTRE_APPS`. Devuelve dónde termina lo último.
fn izquierda(rot: &Rotulos, ocultas: usize, l: &mut Lugar) -> f64 {
    let h = f64::from(ALTO);
    let visibles = rot.apps.len() - ocultas.min(rot.apps.len());
    let lado = ICONO_APP;
    let y = ((h - lado) / 2.0).floor();
    let app = |l: &mut Lugar, x: f64, e: &Entrada| {
        let medio = ENTRE_APPS / 2.0;
        l.piezas.push(Pieza {
            que: Que::App {
                app: e.app,
                ws: e.ws,
            },
            caja: Rect::new(x - medio, 0.0, x + lado + medio, h),
            contenido: Rect::new(x, y, x + lado, y + lado),
        });
        if e.foco {
            let (w, g) = RAYA_FOCO;
            let cx = (x + lado / 2.0).round();
            l.foco = Some(Rect::new(cx - w / 2.0, h - 2.0 - g, cx + w / 2.0, h - 2.0));
        }
        x + lado
    };
    // Dónde termina la última tinta: la de un workspace, un ícono o la raya.
    let mut tinta = 10.0;
    let mut tras_icono = false;
    let mut x = 10.0;
    let mut k = 0;
    for (b, lay) in &rot.espacios {
        let (t0, t1) = rot.tinta_x(lay);
        if tras_icono {
            // Que la tinta del romano quede a `ENTRE_AVISOS` del ícono: 1 de margen y 3 de relleno.
            x = tinta + ENTRE_AVISOS - 4.0 - t0;
        }
        x += 1.0;
        let w = 3.0 + rot.ancho(lay) + 3.0;
        l.piezas.push(Pieza {
            que: Que::Espacio(b.id),
            caja: Rect::new(x, 0.0, x + w, h),
            contenido: Rect::new(x + 3.0, 2.0, x + w - 3.0, h),
        });
        tinta = x + 3.0 + t1;
        tras_icono = false;
        // 1 + 2 de margen: 11 px de tinta a tinta con los romanos.
        x += w + 2.0;
        if rot.por_espacio {
            while k < visibles && rot.apps[k].ws == b.id {
                let ix = if tras_icono {
                    tinta + ENTRE_APPS
                } else {
                    tinta + ENTRE_AVISOS
                };
                tinta = app(l, ix, &rot.apps[k]);
                tras_icono = true;
                k += 1;
            }
        }
    }
    if !rot.por_espacio && visibles > 0 {
        let r = raya((tinta + ENTRE_AVISOS).round(), h);
        l.raya_apps = Some(r);
        tinta = r.x1;
        for (i, e) in rot.apps[..visibles].iter().enumerate() {
            let entre = if i > 0 { ENTRE_APPS } else { ENTRE_AVISOS };
            tinta = app(l, tinta + entre, e);
        }
    }
    if visibles < rot.apps.len()
        && let Some(lay) = rot.mas.get(rot.apps.len() - visibles - 1)
    {
        let (t0, t1) = rot.tinta_x(lay);
        let x0 = tinta + ENTRE_AVISOS - t0;
        let w = rot.ancho(lay);
        let medio = ENTRE_AVISOS / 2.0;
        l.piezas.push(Pieza {
            que: Que::Mas(visibles),
            caja: Rect::new(x0 + t0 - medio, 0.0, x0 + t1 + medio, h),
            contenido: Rect::new(x0, 0.0, x0 + w, h - 2.0),
        });
        tinta = x0 + t1;
    }
    tinta
}

/// `revelado`: cuánto se ve de la bandeja, de 0 a 1, ya con su curva.
pub fn ubicar(ancho: f64, rot: &Rotulos, iconos: usize, revelado: f64) -> Lugar {
    let h = f64::from(ALTO);
    let mut l = Lugar {
        piezas: Vec::new(),
        cajon: Rect::ZERO,
        revelador: Rect::ZERO,
        reloj: [Rect::ZERO; 3],
        estados: Vec::new(),
        grabacion: Vec::new(),
        raya_apps: None,
        raya_bandeja: None,
        raya_avisos: None,
        foco: None,
    };

    let (fw, sw, hw) = (
        rot.ancho(&rot.fecha),
        rot.ancho(&rot.separador),
        rot.ancho(&rot.hora),
    );
    let rw = fw + sw + hw + 26.0;
    // `GtkCenterBox` centra en píxeles enteros.
    let x0 = ((ancho - rw) / 2.0).floor();
    // Lo que no entra antes del reloj se va, de a una desde la última, al «+N».
    for ocultas in 0..=rot.apps.len() {
        l.piezas.clear();
        (l.raya_apps, l.foco) = (None, None);
        if izquierda(rot, ocultas, &mut l) <= x0 - ANTES_DEL_RELOJ {
            break;
        }
    }
    let mut piezas = std::mem::take(&mut l.piezas);
    piezas.push(Pieza {
        que: Que::Reloj,
        caja: Rect::new(x0, 0.0, x0 + rw, h),
        contenido: Rect::new(x0, 0.0, x0 + rw, h - 2.0),
    });
    // 11 px de tinta a cada lado del `›`, como entre los workspaces y entre los avisos.
    let xs = x0 + 10.0 + fw + 3.0;
    let xh = xs + sw + 3.0 + 5.0;
    let reloj = [
        Rect::new(x0 + 5.0, 0.0, x0 + 5.0 + fw, h - 2.0),
        Rect::new(xs, 0.0, xs + sw, h - 2.0),
        Rect::new(xh, 0.0, xh + hw, h - 2.0),
    ];

    let mut x = ancho - 10.0;
    let cw = 4.0 + rot.ancho(&rot.campana) + 3.0;
    piezas.push(Pieza {
        que: Que::Campana,
        caja: Rect::new(x - cw, 0.0, x, h),
        contenido: Rect::new(x - cw + 4.0, 0.0, x - 3.0, h),
    });
    x -= cw + 2.0;
    // Dónde empieza la tinta de un glifo de `pieza` (que lo centra por su tinta).
    let tinta_de = |piezas: &[Pieza], q: Que, lay: &Layout<u8>| {
        piezas.iter().find(|p| p.que == q).map(|p| {
            let (t0, t1) = rot.tinta_x(lay);
            p.contenido.center().x - (t1 - t0) / 2.0
        })
    };
    let fija = rot.bandeja_fija;
    let lugar = if rot.dock_en_barra {
        // Con el dock en la barra: los avisos junto a la campana, la raya y la bandeja, de
        // derecha a izquierda, todo a `ENTRE_AVISOS` de tinta. Desplegable, la flecha va a la
        // izquierda de la bandeja y la bandeja se abre hacia ella.
        let borde = tinta_de(&piezas, Que::Campana, &rot.campana).unwrap_or(x);
        let mut lugar = avisos(rot, borde, &mut piezas);
        if iconos > 0 {
            // La raya de la bandeja es un elemento más de ella: fija, siempre; desplegable, entra
            // con los íconos, y cerrada la flecha queda junto a los avisos (o a la campana).
            let derecha = lugar.borde - ENTRE_AVISOS;
            let r = raya(derecha.round() - 1.0, h);
            let bw = iconos as f64 * ICONO
                + iconos.saturating_sub(1) as f64 * ENTRE_ICONOS
                + ENTRE_AVISOS
                + 1.0;
            let rv = if fija {
                bw
            } else {
                ((bw + ENTRE_AVISOS) * revelado.clamp(0.0, 1.0)).ceil()
            };
            lugar.revelador = Rect::new(r.x1 - rv, 0.0, r.x1, h);
            l.raya_bandeja = Some(r);
            poner_iconos(&mut piezas, r.x1 - bw, iconos, lugar.revelador);
            lugar.borde = r.x1 - rv;
            if !fija {
                let (caja, izq) = rot.glifo_hasta(&rot.flecha, r.x1 - rv, h);
                piezas.push(Pieza {
                    que: Que::Flecha,
                    caja: Rect::new(caja.x0 - 4.0, 0.0, caja.x1 + 4.0, h),
                    contenido: caja,
                });
                lugar.cajon = Rect::new(caja.x0 - 4.0, 0.0, r.x1, h);
                lugar.borde = izq;
            }
        }
        lugar
    } else {
        // La bandeja entra desde la derecha (`SlideLeft`): su borde derecho queda fijo y el
        // revelador la recorta a la izquierda. Sin íconos (los muestra el dock), no hay cajón: los
        // avisos quedan junto a la campana.
        let bw = if iconos == 0 {
            0.0
        } else {
            10.0 + iconos as f64 * ICONO + iconos.saturating_sub(1) as f64 * ENTRE_ICONOS
        };
        let rv = (bw * revelado.clamp(0.0, 1.0)).ceil();
        let revelador = Rect::new(x - rv, 0.0, x, h);
        poner_iconos(&mut piezas, x - bw + 2.0, iconos, revelador);
        x -= rv;
        let flw = if iconos == 0 {
            0.0
        } else {
            4.0 + rot.ancho(&rot.flecha) + 4.0
        };
        if iconos > 0 {
            piezas.push(Pieza {
                que: Que::Flecha,
                caja: Rect::new(x - flw, 0.0, x, h),
                contenido: Rect::new(x - flw + 4.0, 0.0, x - 4.0, h),
            });
        }
        let cajon = Rect::new(x - flw, 0.0, revelador.x1, h);
        x -= flw;
        // De acá a la izquierda, lo que avisa: se parte del borde izquierdo de la tinta de la
        // flecha del cajón o, sin cajón, de la campana.
        let borde = if iconos > 0 {
            tinta_de(&piezas, Que::Flecha, &rot.flecha)
        } else {
            tinta_de(&piezas, Que::Campana, &rot.campana)
        }
        .unwrap_or(x);
        let mut lugar = avisos(rot, borde, &mut piezas);
        (lugar.revelador, lugar.cajon) = (revelador, cajon);
        lugar
    };
    Lugar {
        piezas,
        cajon: lugar.cajon,
        revelador: lugar.revelador,
        reloj,
        raya_avisos: lugar.raya_avisos,
        estados: lugar.estados,
        grabacion: lugar.grabacion,
        ..l
    }
}

/// Lo de la derecha que no es la campana.
#[derive(Default)]
struct Derecha {
    /// Dónde empieza lo último que se puso.
    borde: f64,
    cajon: Rect,
    revelador: Rect,
    /// La raya entre los avisos y lo que sigue (la campana, la bandeja o su flecha).
    raya_avisos: Option<Rect>,
    estados: Vec<Rect>,
    grabacion: Vec<Rect>,
}

/// Los íconos de la bandeja desde `origen`, salvo los que el revelador tapa enteros.
fn poner_iconos(piezas: &mut Vec<Pieza>, origen: f64, iconos: usize, revelador: Rect) {
    let h = f64::from(ALTO);
    let y = ((h - ICONO) / 2.0).floor();
    for i in 0..iconos {
        let ix = origen + i as f64 * (ICONO + ENTRE_ICONOS);
        if ix + ICONO <= revelador.x0 {
            continue;
        }
        piezas.push(Pieza {
            que: Que::Icono(i),
            caja: Rect::new(ix.max(revelador.x0), 0.0, ix + ICONO, h),
            contenido: Rect::new(ix, y, ix + ICONO, y + ICONO),
        });
    }
}

/// Lo que avisa, de `borde` (una tinta) a la izquierda: la raya, la grabación y los glifos de
/// estado. Entre una tinta y la siguiente, siempre lo mismo (`ENTRE_AVISOS`), mida lo que mida
/// cada glifo. La raya y no un `‹` (Roberto, 2026-11-27): la flecha es solo la de la bandeja.
fn avisos(rot: &Rotulos, mut borde: f64, piezas: &mut Vec<Pieza>) -> Derecha {
    let h = f64::from(ALTO);
    let medio = ENTRE_AVISOS / 2.0;
    let mut d = Derecha::default();
    if !rot.privacidad.is_empty() || rot.grabacion.is_some() {
        let r = raya((borde - ENTRE_AVISOS).round() - 1.0, h);
        d.raya_avisos = Some(r);
        borde = r.x0;
    }
    if let Some(p) = &rot.grabacion {
        let derecha = borde - ENTRE_AVISOS;
        let (cajas, izq) = rot.partes_hasta(p, derecha, h);
        piezas.push(Pieza {
            que: Que::Grabacion,
            caja: Rect::new(izq - medio, 0.0, derecha + medio, h),
            contenido: Rect::new(izq, 0.0, derecha, h),
        });
        d.grabacion = cajas;
        borde = izq;
    }
    if !rot.privacidad.is_empty() {
        let derecha = borde - ENTRE_AVISOS;
        for l in rot.privacidad.iter().rev() {
            let (caja, izq) = rot.glifo_hasta(l, borde - ENTRE_AVISOS, h);
            d.estados.push(caja);
            borde = izq;
        }
        d.estados.reverse();
        piezas.push(Pieza {
            que: Que::Privacidad,
            caja: Rect::new(borde - medio, 0.0, derecha + medio, h),
            contenido: Rect::new(borde, 0.0, derecha, h),
        });
    }
    d.borde = borde;
    d
}

impl Lugar {
    pub fn bajo(&self, x: f64, y: f64) -> Option<Que> {
        let p = Point::new(x, y);
        self.piezas
            .iter()
            .find(|q| q.caja.contains(p))
            .map(|q| q.que)
    }

    pub fn pieza(&self, q: Que) -> Option<&Pieza> {
        self.piezas.iter().find(|p| p.que == q)
    }
}

/// El puntero encima (o lo suyo abierto) y lo apretado de cada pieza, con sus transiciones.
#[derive(Default)]
pub struct Animos(Vec<(Que, Transicion, Transicion)>);

impl Animos {
    fn entrada(&mut self, q: Que) -> &mut (Que, Transicion, Transicion) {
        let i = match self.0.iter().position(|(x, ..)| *x == q) {
            Some(i) => i,
            None => {
                self.0.push((q, Transicion::new(0.0), Transicion::new(1.0)));
                self.0.len() - 1
            }
        };
        &mut self.0[i]
    }

    /// Hacia encendido (1) o no (0).
    pub fn encender(&mut self, q: Que, si: bool, ahora: Instant) {
        let v = if si { 1.0 } else { 0.0 };
        self.entrada(q)
            .1
            .ir(v, crate::animaciones::dur(TRANSICION), ahora);
    }

    /// Apretar achica de golpe; soltar vuelve con su transición.
    pub fn apretar(&mut self, q: Que, si: bool, ahora: Instant) {
        let e = self.entrada(q);
        if si {
            e.2.ir(APRETADO, Duration::ZERO, ahora);
        } else {
            e.2.ir(1.0, crate::animaciones::dur(TRANSICION), ahora);
        }
    }

    fn valores(&self, q: Que, ahora: Instant) -> (f32, f32) {
        self.0
            .iter()
            .find(|(x, ..)| *x == q)
            .map_or((0.0, 1.0), |(_, h, e)| (h.valor(ahora), e.valor(ahora)))
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        self.0
            .iter()
            .any(|(_, h, e)| h.anda(ahora) || e.anda(ahora))
    }
}

/// La opacidad del latido de la grabación (`grabando`: 2 s, de 1 a 0,55 y de vuelta).
pub fn latido(desde: Instant, ahora: Instant) -> f32 {
    let f = (ahora.saturating_duration_since(desde).as_secs_f32() / 2.0).fract();
    let (a, b, t) = if f < 0.5 {
        (1.0, 0.55, f * 2.0)
    } else {
        (0.55, 1.0, f * 2.0 - 1.0)
    };
    a + (b - a) * ease_in_out(t)
}

/// Un color con la opacidad del elemento encima de la suya.
fn opaco(c: &Color, op: f32) -> Color {
    con_alfa(c, c[3] * op)
}

pub struct Pintura<'a> {
    pub estado: &'a Estado,
    pub rotulos: &'a Rotulos,
    pub lugar: &'a Lugar,
    pub animos: &'a Animos,
    pub paleta: &'a Paleta,
    pub opacidad: f32,
    pub ahora: Instant,
    /// La opacidad de la grabación en este cuadro.
    pub latido: f32,
}

impl Pintura<'_> {
    /// La barra entera, de `ancho` × `alto` lógicos, en píxeles del buffer.
    pub fn pintar(&self, s: &mut Scene, (ancho, alto): (u32, u32), escala: f64) {
        s.reset();
        let p = self.paleta;
        let tr = Affine::scale(escala);
        let fondo = con_alfa(&p.surface, p.surface[3] * self.opacidad);
        s.fill(
            Fill::NonZero,
            tr,
            pincel(&fondo),
            None,
            &Rect::new(0.0, 0.0, f64::from(ancho), f64::from(alto)),
        );
        for r in [self.lugar.raya_apps, self.lugar.raya_avisos].into_iter().flatten() {
            s.fill(Fill::NonZero, tr, pincel(&p.outline_variant), None, &r);
        }
        // La de la bandeja se ve lo que se ve la bandeja.
        if let Some(r) = self.lugar.raya_bandeja {
            let r = Rect::new(r.x0.max(self.lugar.revelador.x0), r.y0, r.x1, r.y1);
            if r.width() > 0.0 {
                s.fill(Fill::NonZero, tr, pincel(&p.outline_variant), None, &r);
            }
        }
        if let Some(r) = self.lugar.foco {
            s.fill(
                Fill::NonZero,
                tr,
                &degrade_horizontal(r, p.primary, p.tertiary),
                None,
                &RoundedRect::from_rect(r, r.height() / 2.0),
            );
        }
        for pieza in &self.lugar.piezas {
            let (hv, k) = self.animos.valores(pieza.que, self.ahora);
            // Apretado: todo lo de la pieza se achica hacia su centro.
            let c = pieza.caja.center();
            let k = f64::from(k);
            let t = Affine::translate((c.x * escala, c.y * escala))
                * Affine::scale(k)
                * Affine::translate((-c.x * escala, -c.y * escala));
            match pieza.que {
                Que::Espacio(id) => {
                    let Some((b, l)) = self.rotulos.espacios.iter().find(|(b, _)| b.id == id)
                    else {
                        continue;
                    };
                    let base = if b.urgente {
                        p.error
                    } else if b.activo {
                        p.primary
                    } else {
                        p.on_surface
                    };
                    let encima = if b.urgente {
                        p.error
                    } else if b.activo {
                        p.tertiary
                    } else {
                        p.primary
                    };
                    let op = if b.urgente || b.activo || !b.vacio {
                        1.0
                    } else {
                        0.4
                    };
                    let color = opaco(&mezclar(&base, &encima, hv), op + (1.0 - op) * hv);
                    texto_en(s, l, pieza.contenido, escala, t, color, false);
                }
                Que::Reloj => {
                    let medio = mezclar(&p.primary, &p.tertiary, 0.5);
                    let r = self.rotulos;
                    let [f, sep, h] = self.lugar.reloj;
                    texto_en(
                        s,
                        &r.fecha,
                        f,
                        escala,
                        t,
                        mezclar(&p.on_surface, &p.primary, hv),
                        false,
                    );
                    texto_en(
                        s,
                        &r.separador,
                        sep,
                        escala,
                        t,
                        opaco(&mezclar(&p.primary, &medio, hv), 0.45 + 0.45 * hv),
                        false,
                    );
                    texto_en(
                        s,
                        &r.hora,
                        h,
                        escala,
                        t,
                        mezclar(&p.primary, &p.tertiary, hv),
                        false,
                    );
                }
                Que::Campana => {
                    let e = self.estado;
                    let con_avisos = e.avisos > 0;
                    let base = if e.no_molestar {
                        opaco(&p.on_surface_variant, 0.5)
                    } else if con_avisos {
                        p.primary
                    } else {
                        opaco(&p.on_surface, 0.9)
                    };
                    let encima = if con_avisos && !e.no_molestar {
                        p.tertiary
                    } else {
                        p.primary
                    };
                    texto_en(
                        s,
                        &self.rotulos.campana,
                        pieza.contenido,
                        escala,
                        t,
                        mezclar(&base, &encima, hv),
                        true,
                    );
                }
                Que::Flecha => {
                    let c = opaco(
                        &mezclar(&p.on_surface_variant, &p.primary, hv),
                        0.65 + 0.35 * hv,
                    );
                    texto_en(s, &self.rotulos.flecha, pieza.contenido, escala, t, c, true);
                }
                Que::App { app, .. } => {
                    let Some(img) = self.estado.apps.get(app).and_then(|a| a.imagen.as_ref())
                    else {
                        continue;
                    };
                    let r = pieza.contenido;
                    let e = r.width() / f64::from(img.width.max(1));
                    let pos = tr
                        * Affine::translate((c.x, c.y))
                        * Affine::scale(k)
                        * Affine::translate((r.x0 - c.x, r.y0 - c.y))
                        * Affine::scale(e);
                    // Como la bandeja: descansa al 0,8 y se enciende.
                    let brocha = ImageBrush::new(img.clone()).with_alpha(0.8 + 0.2 * hv);
                    s.draw_image(&brocha, pos);
                }
                Que::Mas(desde) => {
                    let n = self.rotulos.apps.len().saturating_sub(desde);
                    if let Some(l) = self.rotulos.mas.get(n.saturating_sub(1)) {
                        let c = opaco(&mezclar(&p.on_surface_variant, &p.primary, hv), 0.9);
                        texto_en(s, l, pieza.contenido, escala, t, c, false);
                    }
                }
                Que::Icono(i) => {
                    let Some(img) = self.estado.bandeja.get(i).and_then(|b| b.imagen.as_ref())
                    else {
                        continue;
                    };
                    s.push_clip_layer(Fill::NonZero, tr, &self.lugar.revelador);
                    let r = pieza.contenido;
                    let e = ICONO / f64::from(img.width.max(1));
                    let pos = tr
                        * Affine::translate((c.x, c.y))
                        * Affine::scale(k)
                        * Affine::translate((r.x0 - c.x, r.y0 - c.y))
                        * Affine::scale(e);
                    let brocha = ImageBrush::new(img.clone()).with_alpha(0.8 + 0.2 * hv);
                    s.draw_image(&brocha, pos);
                    s.pop_layer();
                }
                Que::Privacidad => {
                    for (l, r) in self.rotulos.privacidad.iter().zip(&self.lugar.estados) {
                        texto_en(s, l, *r, escala, t, p.error, true);
                    }
                }
                Que::Grabacion => {
                    if let Some(partes) = &self.rotulos.grabacion {
                        let c = opaco(&p.error, self.latido);
                        for (l, r) in partes.textos.iter().zip(&self.lugar.grabacion) {
                            texto_en(s, l, *r, escala, t, c, false);
                        }
                    }
                }
            }
        }
    }
}

/// `l` en `r` (lógicos), como una etiqueta de GTK; un glifo suelto, centrado por su tinta.
fn texto_en(s: &mut Scene, l: &Layout<u8>, r: Rect, escala: f64, t: Affine, c: Color, glifo: bool) {
    let centro = r.center();
    let (cx, cy) = (centro.x * escala, centro.y * escala);
    // De alto, la caja de una etiqueta de GTK (parley la da un píxel corta).
    let (x, y) = if glifo {
        texto::esquina_por_tinta(l, cx, cy)
    } else {
        let (x, _) = texto::esquina(l, cx, cy);
        (x, (cy - texto::alto_de_linea(l) / 2.0).floor())
    };
    texto::dibujar_en(s, l, t * Affine::translate((x, y)), |_| c, 1.0);
}

/// El texto de un globo, en píxeles del buffer: como en el del dock, el primer renglón en negrita.
/// Lo que no entra baja de renglón: acá no hay otro lugar donde leerlo.
pub fn armar_globo(t: &mut Textos, texto: &str, escala: f32) -> Layout<u8> {
    let corte = texto.find('\n').unwrap_or(texto.len());
    let (nombre, resto) = texto.split_at(corte);
    let estilo = |negrita| Estilo {
        rol: ROL,
        negrita,
        cursiva: false,
        subrayado: false,
    };
    let ancho = (GLOBO_TEXTO_MAX * f64::from(escala)) as f32;
    t.parrafo(
        &[(nombre, estilo(true)), (resto, estilo(false))],
        PX_GLOBO,
        escala,
        ancho,
        0,
    )
}

/// El globo sin la sombra, en lógicos.
pub fn globo_caja(l: &Layout<u8>, escala: f32) -> (f64, f64) {
    let e = f64::from(escala);
    let renglones = l.lines().count().max(1) as f64;
    let w = texto::caja(l).0 / e;
    let h = (texto::alto_de_linea(l) * renglones / e).ceil();
    (ancho_globo(w), h + 2.0 * (BORDE_Y_RELLENO + FILA_Y))
}

/// El tamaño de la superficie del globo, con la sombra.
pub fn globo_tam(l: &Layout<u8>, escala: f32) -> (u32, u32) {
    let (w, h) = globo_caja(l, escala);
    ((w + 2.0 * GLOBO_SOMBRA) as u32, (h + 2.0 * GLOBO_SOMBRA) as u32)
}

pub fn pintar_globo(s: &mut Scene, l: &Layout<u8>, p: &Paleta, opacidad: f32, escala: f64) {
    s.reset();
    let tr = Affine::scale(escala);
    let (w, h) = globo_caja(l, escala as f32);
    let m = GLOBO_SOMBRA;
    let r = Rect::new(m, m, m + w, m + h);
    marco(s, tr, escala, p, opacidad, r);
    let (x, y) = (
        (r.x0 + GLOBO_X) * escala,
        (r.y0 + BORDE_Y_RELLENO + FILA_Y) * escala,
    );
    texto::dibujar_en(
        s,
        l,
        Affine::translate((x.round(), y.round())),
        |_| p.on_surface,
        1.0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use morgan_motor::forma::menu::GLOBO_MAX;

    #[test]
    fn un_globo_corto_es_corto() {
        let l = armar_globo(&mut Textos::new(), "OpenRGB", 1.0);
        let (w, _) = globo_caja(&l, 1.0);
        assert_eq!(w, ancho_globo(texto::caja(&l).0));
        assert!(w < 150.0, "{w}");
        assert_eq!(l.lines().count(), 1);
    }

    #[test]
    fn lo_largo_baja_de_renglon_sin_pasar_el_tope() {
        // Largo de sobra para cualquier fuente: el test corre también con las Noto del release.
        let t = "Cachy-Update\nEl sistema está actualizado y no hay paquetes nuevos para instalar";
        let l = armar_globo(&mut Textos::new(), t, 1.0);
        assert!(globo_caja(&l, 1.0).0 <= GLOBO_MAX);
        assert!(l.lines().count() >= 3, "el nombre y al menos dos de texto");
    }

    fn app(nombre: &str, ventanas: &[(i64, bool)]) -> App {
        App {
            clave: nombre.into(),
            nombre: nombre.into(),
            imagen: None,
            ventanas: ventanas
                .iter()
                .map(|&(ws, foco)| Ventana { ws, foco })
                .collect(),
        }
    }

    fn rotulos(apps: Vec<App>, por_espacio: bool, bandeja_fija: bool) -> Rotulos {
        let e = Estado {
            fijos: BTreeMap::from([("DP-3".into(), vec![1, 2, 3])]),
            fecha: "27 de noviembre".into(),
            hora: "14:32".into(),
            apps,
            por_espacio,
            bandeja_fija,
            dock_en_barra: true,
            // Sin la grabación: sus pedazos se miden aparte (`partes_hasta`).
            privacidad: vec!["󰔂", "󰍬"],
            ..Estado::default()
        };
        Rotulos::armar(&mut Textos::new(), &e, "DP-3", 1.0, 1)
    }

    #[test]
    fn cada_barra_muestra_las_apps_de_su_salida() {
        let apps = [
            app("kitty", &[(1, true), (5, false)]),
            app("brave", &[(2, false)]),
            app("steam", &[(5, false)]),
        ];
        let tira = entradas(&apps, &[1, 2, 3], false);
        assert_eq!(tira.iter().map(|e| e.app).collect::<Vec<_>>(), [0, 1]);
        assert!(tira[0].foco && !tira[1].foco);
        // En la otra salida, kitty va sin la raya: la ventana con el foco no está ahí.
        let otra = entradas(&apps, &[4, 5, 6], false);
        assert_eq!(
            otra.iter().map(|e| (e.app, e.foco)).collect::<Vec<_>>(),
            [(0, false), (2, false)]
        );
        let grupos = entradas(&apps, &[1, 2, 5], true);
        let v: Vec<_> = grupos.iter().map(|e| (e.ws, e.app)).collect();
        assert_eq!(v, [(1, 0), (2, 1), (5, 0), (5, 2)]);
    }

    #[test]
    fn lo_que_no_entra_va_al_mas_sin_pisar_el_reloj() {
        let apps: Vec<App> = (0..40)
            .map(|i| app(&format!("a{i}"), &[(1, false)]))
            .collect();
        for por_espacio in [false, true] {
            let rot = rotulos(apps.clone(), por_espacio, false);
            let l = ubicar(1252.0, &rot, 0, 0.0);
            let reloj = l.pieza(Que::Reloj).unwrap().caja.x0;
            let mas = l
                .piezas
                .iter()
                .find(|p| matches!(p.que, Que::Mas(_)))
                .unwrap();
            assert!(mas.caja.x1 <= reloj - ANTES_DEL_RELOJ + ENTRE_APPS / 2.0);
            let Que::Mas(desde) = mas.que else {
                unreachable!()
            };
            let visibles = l
                .piezas
                .iter()
                .filter(|p| matches!(p.que, Que::App { .. }))
                .count();
            assert_eq!(visibles, desde);
            assert!(visibles > 10, "{visibles}");
        }
        let pocas = rotulos(apps[..3].to_vec(), false, false);
        let l = ubicar(1252.0, &pocas, 0, 0.0);
        assert!(!l.piezas.iter().any(|p| matches!(p.que, Que::Mas(_))));
        assert!(l.raya_apps.is_some());
    }

    /// Dónde empieza y termina la tinta de una pieza de texto o glifo, o la caja de un ícono.
    fn tintas(l: &Lugar, rot: &Rotulos) -> Vec<(f64, f64)> {
        let mut v: Vec<(f64, f64)> = Vec::new();
        for p in &l.piezas {
            match p.que {
                Que::Icono(_) | Que::App { .. } => v.push((p.contenido.x0, p.contenido.x1)),
                Que::Campana => {
                    let (t0, t1) = rot.tinta_x(&rot.campana);
                    let x = p.contenido.center().x - (t1 - t0) / 2.0;
                    v.push((x, x + t1 - t0));
                }
                _ => {}
            }
        }
        v.extend(l.estados.iter().map(|r| (r.x0, r.x1)));
        let rayas = [l.raya_apps, l.raya_bandeja, l.raya_avisos];
        v.extend(rayas.iter().flatten().map(|r| (r.x0, r.x1)));
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v
    }

    #[test]
    fn con_el_dock_en_la_barra_todo_va_a_once_de_tinta() {
        let apps = vec![app("kitty", &[(1, true)]), app("brave", &[(2, false)])];
        let rot = rotulos(apps, false, true);
        let l = ubicar(1252.0, &rot, 3, 0.0);
        // A la derecha, de izquierda a derecha: la bandeja, la raya, los avisos y la campana.
        let raya = l.raya_bandeja.unwrap();
        let ult_icono = l.pieza(Que::Icono(2)).unwrap().contenido;
        assert!(ult_icono.x1 < raya.x0);
        assert!(l.estados.iter().all(|e| e.x0 > raya.x1));
        let t = tintas(&l, &rot);
        let derecha: Vec<_> = t.iter().filter(|(x, _)| *x >= ult_icono.x1 - 1.0).collect();
        for par in derecha.windows(2) {
            let d = par[1].0 - par[0].1;
            assert!((d - ENTRE_AVISOS).abs() <= 1.0, "{par:?}: {d}");
        }
        // Entre íconos de la bandeja, los suyos.
        let i0 = l.pieza(Que::Icono(0)).unwrap().contenido;
        let i1 = l.pieza(Que::Icono(1)).unwrap().contenido;
        assert_eq!(i1.x0 - i0.x1, ENTRE_ICONOS);
        // A la izquierda: la tinta del último workspace, la raya y la primera app.
        let r = l.raya_apps.unwrap();
        let ws = l.pieza(Que::Espacio(3)).unwrap().contenido;
        let (_, t1) = rot.tinta_x(&rot.espacios[2].1);
        assert!((r.x0 - (ws.x0 + t1) - ENTRE_AVISOS).abs() <= 0.5);
        let a0 = l.pieza(Que::App { app: 0, ws: 0 }).unwrap().contenido;
        let a1 = l.pieza(Que::App { app: 1, ws: 0 }).unwrap().contenido;
        assert_eq!(a0.x0 - r.x1, ENTRE_AVISOS);
        assert_eq!(a1.x0 - a0.x1, ENTRE_APPS);
        assert_eq!(a0.width(), ICONO);
        assert!(l.foco.is_some());
    }

    #[test]
    fn la_bandeja_fija_no_tiene_flecha_y_se_ve_entera() {
        let fija = ubicar(1252.0, &rotulos(Vec::new(), false, true), 3, 0.0);
        assert!(fija.pieza(Que::Flecha).is_none());
        assert_eq!((0..3).filter(|&i| fija.pieza(Que::Icono(i)).is_some()).count(), 3);
        let cajon = ubicar(1252.0, &rotulos(Vec::new(), false, false), 3, 0.0);
        let flecha = cajon.pieza(Que::Flecha).unwrap().contenido;
        assert!(cajon.pieza(Que::Icono(0)).is_none());
        // Cerrada, la flecha queda a once del primer aviso y la raya de la bandeja no se ve.
        let aviso = cajon.estados[0];
        assert!((aviso.x0 - flecha.x1 - ENTRE_AVISOS).abs() <= 1.0);
        assert!(cajon.revelador.width() == 0.0);
        // Abierta: flecha, íconos, la raya de la bandeja y los avisos, cada uno a once.
        let abierta = ubicar(1252.0, &rotulos(Vec::new(), false, false), 3, 1.0);
        let f = abierta.pieza(Que::Flecha).unwrap().contenido;
        let i0 = abierta.pieza(Que::Icono(0)).unwrap().contenido;
        let i2 = abierta.pieza(Que::Icono(2)).unwrap().contenido;
        let r = abierta.raya_bandeja.unwrap();
        assert!((i0.x0 - f.x1 - ENTRE_AVISOS).abs() <= 1.0);
        assert_eq!(r.x0 - i2.x1, ENTRE_AVISOS);
        assert!((abierta.estados[0].x0 - r.x1 - ENTRE_AVISOS).abs() <= 1.0);
        // Sin avisos, la flecha va junto a la campana.
        let mut sin = rotulos(Vec::new(), false, false);
        sin.privacidad.clear();
        let l = ubicar(1252.0, &sin, 3, 0.0);
        let f = l.pieza(Que::Flecha).unwrap().contenido;
        let (t0, t1) = sin.tinta_x(&sin.campana);
        let c = l.pieza(Que::Campana).unwrap().contenido.center().x - (t1 - t0) / 2.0;
        assert!((c - f.x1 - ENTRE_AVISOS).abs() <= 1.0);
        assert!(l.raya_avisos.is_none());
    }

    #[test]
    fn sin_el_dock_en_la_barra_los_avisos_tambien_van_tras_una_raya() {
        // Dock abajo: la bandeja es del dock y no hay flecha; dock apagado: la bandeja vuelve a
        // la barra con su flecha. En los dos, entre los avisos y lo demás va la raya, no un `‹`.
        let mut rot = rotulos(Vec::new(), false, false);
        rot.dock_en_barra = false;
        for iconos in [0, 3] {
            let l = ubicar(1252.0, &rot, iconos, 0.0);
            let r = l.raya_avisos.expect("la raya de los avisos");
            assert!((r.x0 - l.estados.last().unwrap().x1 - ENTRE_AVISOS).abs() <= 1.0);
            assert_eq!(l.pieza(Que::Flecha).is_some(), iconos > 0);
        }
    }

    #[test]
    fn el_latido_va_y_vuelve() {
        let t0 = Instant::now();
        let s = Duration::from_secs_f32;
        assert!((latido(t0, t0) - 1.0).abs() < 1e-4);
        assert!((latido(t0, t0 + s(1.0)) - 0.55).abs() < 1e-4);
        assert!((latido(t0, t0 + s(2.0)) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn apretar_achica_de_golpe_y_soltar_vuelve() {
        let t0 = Instant::now();
        let mut a = Animos::default();
        a.apretar(Que::Reloj, true, t0);
        assert_eq!(a.valores(Que::Reloj, t0).1, APRETADO);
        a.apretar(Que::Reloj, false, t0);
        assert!(a.anda(t0));
        assert_eq!(a.valores(Que::Reloj, t0 + Duration::from_secs(1)).1, 1.0);
        assert_eq!(a.valores(Que::Campana, t0), (0.0, 1.0));
    }
}
