//! Un aviso en una `Scene` de vello. Las medidas y los colores son los de la versión con GTK
//! (la hoja `.osd` de `estilo.css` hasta 0.1.1), medidos en el anidado:
//!
//! - la ventana mide `ANCHO` × `ALTO`; la tarjeta deja 10 px por lado para el brillo del borde
//!   y tiene 12 de relleno;
//! - a la izquierda, el cuadrito del ícono (36 px); a 10 px, la columna de texto: el título
//!   arriba (con el porcentaje a la derecha, a 6 px) y, 2 px más abajo, el estado o la barra
//!   (4 px más abajo todavía, de 6 de alto);
//! - cada texto en una caja de píxeles enteros, como las etiquetas de GTK.

use morgan_motor::color::{Color, Paleta, con_alfa, mezclar, pincel};
use morgan_motor::forma::{Borde, sombra};
use morgan_motor::parley::Layout;
use morgan_motor::texto::{self, Textos};
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, BezPath, Point, Rect, RoundedRect, Shape};
use morgan_motor::vello::peniko::{BlendMode, Fill, Gradient};

use super::{Aviso, Cuerpo, es_glifo, porcentaje};

/// El lugar del brillo del borde: el anillo asoma 2 px y el glow se apaga unos 8 más allá.
const MARGEN: f64 = 10.0;
const RELLENO: f64 = 12.0;
const RADIO: f64 = 4.0;
const CUADRO: f64 = 36.0;
const SEPARACION: f64 = 10.0;
/// El alto de dos renglones aunque haya uno solo: todos los avisos miden lo mismo.
const ALTO_TEXTO: f64 = 38.0;
const ENTRE_RENGLONES: f64 = 2.0;
const ENTRE_TITULO_Y_PORCENTAJE: f64 = 6.0;
const BARRA_ARRIBA: f64 = 4.0;
const BARRA_ALTO: f64 = 6.0;

const PX_TITULO: f32 = 14.0;
const PX_CHICO: f32 = 12.0;
const PX_GLIFO: f32 = 18.0;

/// El pincel de cada texto se elige al dibujar; el rol no se usa.
const ROL: u8 = 0;

/// Los textos armados de una ventana, en píxeles del buffer. Se rehacen con cada aviso.
pub struct Rotulos {
    pub version: u64,
    pub escala: f32,
    titulo: Layout<u8>,
    porcentaje: Option<Layout<u8>>,
    estado: Option<Layout<u8>>,
    glifo: Option<Layout<u8>>,
}

/// La tarjeta con su relleno y el lugar del brillo, alrededor de los dos renglones.
#[cfg(test)]
pub fn alto_ventana() -> f64 {
    2.0 * (MARGEN + RELLENO) + ALTO_TEXTO
}

/// Dónde empieza la columna de texto y cuánto mide, en píxeles lógicos.
fn columna(ancho: u32) -> (f64, f64) {
    let x = MARGEN + RELLENO + CUADRO + SEPARACION;
    (x, (ancho as f64 - MARGEN - RELLENO - x).max(0.0))
}

impl Rotulos {
    pub fn armar(t: &mut Textos, a: &Aviso, ancho: u32, escala: f32, version: u64) -> Self {
        let (_, ancho_col) = columna(ancho);
        let max = ancho_col as f32 * escala;
        let porcentaje = match &a.cuerpo {
            Cuerpo::Barra {
                porcentaje: p,
                apagada,
                ..
            } => Some(t.armar(&[(&porcentaje(*p, *apagada), ROL)], PX_CHICO, escala)),
            Cuerpo::Estado { .. } | Cuerpo::Fallo { .. } => None,
        };
        let libre = match &porcentaje {
            Some(p) => max - texto::caja(p).0 as f32 - ENTRE_TITULO_Y_PORCENTAJE as f32 * escala,
            None => max,
        };
        let titulo = texto::armar_recortado(t, &a.titulo, ROL, PX_TITULO, escala, libre);
        let estado = match &a.cuerpo {
            Cuerpo::Estado { texto: e, .. } | Cuerpo::Fallo { texto: e } if !e.is_empty() => {
                Some(texto::armar_recortado(t, e, ROL, PX_CHICO, escala, max))
            }
            _ => None,
        };
        // Un glifo va como texto; un nombre de ícono del tema no se dibuja (los avisos de
        // morgan y sus scripts mandan glifos).
        let glifo = a
            .icono
            .as_deref()
            .filter(|i| es_glifo(i))
            .map(|g| t.armar(&[(g, ROL)], PX_GLIFO, escala));
        Rotulos {
            version,
            escala,
            titulo,
            porcentaje,
            estado,
            glifo,
        }
    }
}

fn texto_en(s: &mut Scene, l: &Layout<u8>, (x, y): (f64, f64), c: Color) {
    texto::dibujar_en(s, l, Affine::translate((x, y)), |_| c, 1.0);
}

/// La ventana entera, de `ancho` × `alto` lógicos, en píxeles del buffer.
#[allow(clippy::too_many_arguments)]
pub fn pintar(
    s: &mut Scene,
    a: &Aviso,
    rot: &Rotulos,
    p: &Paleta,
    opacidad: f32,
    borde: Borde,
    (ancho, alto): (u32, u32),
    escala: f64,
) {
    s.reset();
    let tr = Affine::scale(escala);
    let (w, h) = (ancho as f64, alto as f64);
    let encendido = matches!(
        a.cuerpo,
        Cuerpo::Estado {
            encendido: true,
            ..
        }
    );
    let mudo = matches!(a.cuerpo, Cuerpo::Barra { apagada: true, .. });
    let fallo = matches!(a.cuerpo, Cuerpo::Fallo { .. });

    tarjeta(
        s,
        tr,
        p,
        opacidad,
        borde,
        Rect::new(MARGEN, MARGEN, w - MARGEN, h - MARGEN),
    );

    // El cuadrito del ícono, centrado en el alto del texto.
    let y0 = MARGEN + RELLENO;
    let x0 = MARGEN + RELLENO;
    let yc = y0 + (ALTO_TEXTO - CUADRO) / 2.0;
    let cuadro = Rect::new(x0, yc, x0 + CUADRO, yc + CUADRO);
    let rr = RoundedRect::from_rect(cuadro, RADIO);
    if encendido {
        sombra(
            s,
            tr,
            cuadro,
            RADIO,
            con_alfa(&p.tertiary, 0.3),
            (0.0, 0.0),
            0.0,
            6.0,
        );
        s.fill(
            Fill::NonZero,
            tr,
            &degrade_135(cuadro, p.primary, p.tertiary),
            None,
            &rr,
        );
    } else {
        s.fill(
            Fill::NonZero,
            tr,
            pincel(&con_alfa(&p.on_surface, 0.08)),
            None,
            &rr,
        );
    }
    if let Some(l) = &rot.glifo {
        let c = if encendido {
            p.surface
        } else if fallo {
            p.error
        } else {
            p.on_surface
        };
        let centro = cuadro.center();
        let esq = texto::esquina_por_tinta(l, centro.x * escala, centro.y * escala);
        texto_en(s, l, esq, c);
    }

    // La columna de texto, en píxeles del buffer: los textos ya están armados así.
    let (xc, ancho_col) = columna(ancho);
    let (xb, yb) = (xc * escala, y0 * escala);
    let alto_titulo = texto::alto_de_linea(&rot.titulo);
    texto_en(s, &rot.titulo, (xb, yb), p.on_surface);
    if let Some(l) = &rot.porcentaje {
        let (pw, ph) = (texto::caja(l).0, texto::alto_de_linea(l));
        let x = (xc + ancho_col) * escala - pw;
        let y = yb + ((alto_titulo - ph) / 2.0).floor();
        texto_en(s, l, (x, y), if mudo { p.error } else { p.tertiary });
    }
    let debajo = yb + alto_titulo + ENTRE_RENGLONES * escala;
    if let Some(l) = &rot.estado {
        let c = if encendido {
            con_alfa(&p.tertiary, 0.9)
        } else if fallo {
            p.error
        } else {
            con_alfa(&p.on_surface, 0.5)
        };
        texto_en(s, l, (xb, debajo), c);
    }
    if let Cuerpo::Barra { fraccion, .. } = a.cuerpo {
        // En lógicos otra vez, como la tarjeta.
        let y = debajo / escala + BARRA_ARRIBA;
        barra(
            s,
            tr,
            p,
            Rect::new(xc, y, xc + ancho_col, y + BARRA_ALTO),
            fraccion,
            mudo,
        );
    }
}

/// El fondo y el borde de la tarjeta. El borde es la pila de `box-shadow` de la hoja de GTK,
/// en su orden (GTK apila la última arriba): brillo primary, brillo tertiary (o la sombra
/// negra, según `borde`), el anillo de la mezcla, y los dos medios bordes corridos que lo
/// tiñen hacia cada esquina.
fn tarjeta(s: &mut Scene, tr: Affine, p: &Paleta, opacidad: f32, borde: Borde, r: Rect) {
    let rr = RoundedRect::from_rect(r, RADIO);
    // Las sombras van solo por fuera: el fondo es translúcido y no tienen que verse a través.
    let mut afuera = BezPath::new();
    afuera.extend(r.inflate(MARGEN, MARGEN).path_elements(0.1));
    afuera.extend(rr.path_elements(0.1));
    s.push_layer(Fill::EvenOdd, BlendMode::default(), 1.0, tr, &afuera);
    let medio = mezclar(&p.primary, &p.tertiary, 0.5);
    // Con la sombra negra, en vez de los dos brillos: la sombra suave y una línea negra de
    // 1 px por fuera del anillo (el de los toasts). La sombra cabe en el margen del brillo.
    let afuera_del_anillo = match borde {
        // El glow tenue de todo el shell, por fuera del anillo (que asoma 2 px).
        Borde::Brillo => [
            (con_alfa(&p.tertiary, 0.3), (1.0, 1.0), 3.0, 5.0),
            (con_alfa(&p.primary, 0.3), (-1.0, -1.0), 3.0, 5.0),
        ],
        Borde::Sombra => [
            ([0.0, 0.0, 0.0, 0.5], (0.0, 0.0), 3.0, 4.0),
            ([0.0, 0.0, 0.0, 1.0], (0.0, 0.0), 3.0, 0.0),
        ],
    };
    let anillo = [
        (medio, (0.0, 0.0), 2.0, 0.0),
        (p.tertiary, (1.0, 1.0), 1.0, 0.0),
        (p.primary, (-1.0, -1.0), 1.0, 0.0),
    ];
    for (c, d, spread, blur) in afuera_del_anillo.into_iter().chain(anillo) {
        sombra(s, tr, r, RADIO, c, d, spread, blur);
    }
    s.pop_layer();
    let fondo = con_alfa(
        &p.surface_container_low,
        p.surface_container_low[3] * opacidad,
    );
    s.fill(Fill::NonZero, tr, pincel(&fondo), None, &rr);
}

/// `linear-gradient(135deg, a, b)` de CSS sobre `r`: hacia abajo a la derecha, con la línea
/// del degradé tan larga como para que las esquinas caigan justo en `a` y en `b`.
fn degrade_135(r: Rect, a: Color, b: Color) -> Gradient {
    let (sx, sy) = (
        std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
    );
    let largo = r.width() * sx + r.height() * sy;
    let c = r.center();
    let (dx, dy) = (sx * largo / 2.0, sy * largo / 2.0);
    Gradient::new_linear(
        Point::new(c.x - dx, c.y - dy),
        Point::new(c.x + dx, c.y + dy),
    )
    .with_stops([pincel(&a), pincel(&b)].as_slice())
}

/// La barra del nivel: el canal y, encima, lo lleno con el degradé de izquierda a derecha y
/// su brillo. Silenciada, entera a media opacidad.
fn barra(s: &mut Scene, tr: Affine, p: &Paleta, r: Rect, fraccion: f64, apagada: bool) {
    let radio = r.height() / 2.0;
    if apagada {
        s.push_layer(
            Fill::NonZero,
            BlendMode::default(),
            0.5,
            tr,
            &r.inflate(4.0, 4.0),
        );
    }
    let canal = RoundedRect::from_rect(r, radio);
    s.fill(
        Fill::NonZero,
        tr,
        pincel(&con_alfa(&p.outline_variant, p.outline_variant[3] * 0.6)),
        None,
        &canal,
    );
    // GTK da a lo lleno un ancho entero, redondeado hacia abajo.
    let lleno = (r.width() * fraccion.clamp(0.0, 1.0)).floor();
    if lleno > 0.0 {
        let l = Rect::new(r.x0, r.y0, r.x0 + lleno, r.y1);
        // El brillo queda adentro del canal, que recorta lo que asoma.
        s.push_layer(Fill::NonZero, BlendMode::default(), 1.0, tr, &canal);
        sombra(
            s,
            tr,
            l,
            radio,
            con_alfa(&p.tertiary, 0.35),
            (0.0, 0.0),
            0.0,
            4.0,
        );
        s.pop_layer();
        let g = Gradient::new_linear(Point::new(l.x0, 0.0), Point::new(l.x1, 0.0))
            .with_stops([pincel(&p.primary), pincel(&p.tertiary)].as_slice());
        s.fill(
            Fill::NonZero,
            tr,
            &g,
            None,
            &RoundedRect::from_rect(l, radio.min(lleno / 2.0)),
        );
    }
    if apagada {
        s.pop_layer();
    }
}
