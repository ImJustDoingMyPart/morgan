//! Una salida entera en una `Scene` de vello: fondo, reloj, fecha, campo y aviso, a partir del
//! `Estado` y del instante del cuadro. Las medidas y los colores son los de la versión con GTK
//! (`vista.rs`, `lienzo.rs`, `estilo.css`); lo que GTK hacía con su blur se aproxima:
//!
//! - las sombras del campo, con el rectángulo redondeado desenfocado de vello (el mismo
//!   modelo que el `box-shadow` de GTK);
//! - el brillo de la onda (un blur de 4 sobre el trazo), con dos trazos más anchos y tenues;
//! - la sombra del texto (`text-shadow`) todavía no está.

use std::collections::HashMap;
use std::f32::consts::PI;

use vello::Scene;
use vello::kurbo::{Affine, BezPath, Cap, Circle, Join, Point, Rect, RoundedRect, Stroke};
use vello::peniko::{self, BlendMode, Fill, Gradient, ImageBrush, ImageData, ImageQuality};

use crate::caracteres;
use crate::escena::*;
use crate::sombra::Sombra;
use crate::texto;
use morgan_ipc::bloqueo::Escritura;

pub(crate) const BORDE: f64 = 2.0;
const RADIO: f64 = 4.0;
const ONDA_X0: f32 = 18.0;
const ONDA_X1: f32 = ANCHO - 18.0;

const BX: f32 = 1.0;
const BY: f32 = 1.0;
const BW: f32 = ANCHO - 2.0;
const BH: f32 = ALTO - 2.0;
pub(crate) const PERIMETRO: f32 = 2.0 * (BW + BH);

/// Dónde va el centro de cada pieza: tantos píxeles arriba del centro de la salida
/// (`en()` de `vista.rs`).
const Y_RELOJ: f64 = 170.0;
const Y_FECHA: f64 = 70.0;
const Y_CAMPO: f64 = -50.0;
const Y_AVISO: f64 = -105.0;

/// Roles de la paleta para el pincel del texto.
pub const ROL_PRIMARY: u8 = 0;
pub const ROL_TERTIARY: u8 = 1;
pub const ROL_VARIANTE: u8 = 2;
pub const ROL_TEXTO: u8 = 3;

fn pincel(c: &Color) -> peniko::Color {
    peniko::Color::new(*c)
}

pub fn rol(p: &Paleta, r: u8) -> Color {
    match r {
        ROL_PRIMARY => p.primary,
        ROL_TERTIARY => p.tertiary,
        ROL_VARIANTE => p.on_surface_variant,
        _ => p.on_surface,
    }
}

/// Los textos armados de una salida. Se rehacen cuando cambia la versión del estado.
pub struct Rotulos {
    pub version: u64,
    /// Hora, dos puntos y minutos, cada uno aparte como las etiquetas de GTK: juntos,
    /// JetBrains Mono sube los dos puntos entre números (`calt`) y quedaban más arriba.
    pub reloj: [parley::Layout<u8>; 3],
    pub fecha: parley::Layout<u8>,
    pub aviso: Option<parley::Layout<u8>>,
    /// Las sombras de las tres piezas del reloj, la fecha y el aviso, en ese orden; las pone
    /// el render (necesitan la GPU).
    pub sombras: [Vec<(Sombra, f64)>; 5],
    /// El texto del campo y su layout: cambia poco, se guarda el último.
    pub campo: Option<(String, parley::Layout<u8>)>,
    /// Los de la escritura `symbols`, armados la primera vez que salen.
    pub glifos: HashMap<char, parley::Layout<u8>>,
}

impl Rotulos {
    pub fn armar(t: &mut texto::Textos, e: &Estado, escala: f32) -> Self {
        let mut pieza =
            |s: &str, rol| t.armar_con_peso(&[(s, rol)], texto::pt(96.0), PESO_RELOJ, escala);
        let reloj = [
            pieza(&e.hora, ROL_PRIMARY),
            pieza(":", ROL_VARIANTE),
            pieza(&e.minutos, ROL_TERTIARY),
        ];
        let fecha = t.armar(&[(&e.fecha, ROL_VARIANTE)], texto::pt(20.0), escala);
        let aviso = if e.mayus {
            Some("󰘲  Bloq Mayús activado".to_owned())
        } else {
            e.aviso.clone()
        }
        .map(|a| t.armar(&[(&a, ROL_TEXTO)], texto::pt(13.0), escala));
        Rotulos {
            sombras: Default::default(),
            version: e.version_textos,
            reloj,
            fecha,
            aviso,
            campo: None,
            glifos: HashMap::new(),
        }
    }

    /// Lo que lleva sombra, en el orden de `sombras` y `SOMBRAS`.
    pub fn capas(&self) -> [Option<&parley::Layout<u8>>; 5] {
        let [h, d, m] = &self.reloj;
        [
            Some(h),
            Some(d),
            Some(m),
            Some(&self.fecha),
            self.aviso.as_ref(),
        ]
    }

    /// La versión incluye Bloq Mayús: cambia el aviso.
    pub fn version_de(e: &Estado) -> u64 {
        e.version_textos * 2 + e.mayus as u64
    }
}

/// El fondo ya desenfocado, como lo quiere vello.
pub struct Fondo {
    pub imagen: ImageData,
    pub achicada: f64,
}

#[allow(clippy::too_many_arguments)]
pub fn pintar(
    s: &mut Scene,
    e: &Estado,
    rot: &mut Rotulos,
    textos: &mut texto::Textos,
    fondo: Option<&Fondo>,
    (ancho, alto): (u32, u32),
    escala: f64,
    t: i64,
) {
    s.reset();
    let (w, h) = (ancho as f64, alto as f64);
    let todo = Rect::new(0.0, 0.0, w * escala, h * escala);
    let opacidad = e.opacidad(t);
    if opacidad <= 0.001 {
        return;
    }
    s.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        opacidad,
        Affine::IDENTITY,
        &todo,
    );

    match fondo {
        Some(f) => {
            let (iw, ih) = (
                f.imagen.width as f64 * f.achicada,
                f.imagen.height as f64 * f.achicada,
            );
            let k = (w * escala / iw).max(h * escala / ih);
            let tr = Affine::translate(((w * escala - iw * k) / 2.0, (h * escala - ih * k) / 2.0))
                * Affine::scale(k * f.achicada);
            let brocha = ImageBrush::new(f.imagen.clone()).with_quality(ImageQuality::Medium);
            s.draw_image(&brocha, tr);
        }
        None => s.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            pincel(&e.paleta.surface),
            None,
            &todo,
        ),
    }

    let (cx, cy) = (w * escala / 2.0, h * escala / 2.0);
    let color = |r| rol(&e.paleta, r);
    // El reloj es una fila de tres cajas centrada, como el `GtkBox` de antes. El medio píxel
    // sobrante va abajo y a la derecha, al revés que en la fecha: así coincide con la versión
    // con GTK (medido en el anidado; sin redondear, o hacia el otro lado, se corre).
    let total: f64 = rot.reloj.iter().map(|l| texto::caja(l).0).sum();
    let mut x = (cx - total / 2.0).round();
    let yr = cy - Y_RELOJ * escala;
    for (l, sm) in rot.reloj.iter().zip(&rot.sombras) {
        let (w, h) = texto::caja(l);
        let esq = (x, (yr - h / 2.0).round());
        con_relieve(s, l, sm, esq, color, Some(&RELIEVE_RELOJ), escala);
        x += w;
    }
    let esq = texto::esquina(&rot.fecha, cx, cy - Y_FECHA * escala);
    con_relieve(
        s,
        &rot.fecha,
        &rot.sombras[3],
        esq,
        color,
        Some(&RELIEVE_FECHA),
        escala,
    );
    if let Some(a) = &rot.aviso {
        let esq = texto::esquina(a, cx, cy - Y_AVISO * escala);
        con_relieve(s, a, &rot.sombras[4], esq, color, None, escala);
    }

    // El campo en sus coordenadas: (0, 0) es su esquina, en píxeles lógicos.
    let origen = Affine::scale(escala)
        * Affine::translate((
            (w / 2.0 - ANCHO as f64 / 2.0).round(),
            (h / 2.0 - Y_CAMPO - ALTO as f64 / 2.0).round(),
        ));
    campo(s, e, rot, textos, origen, escala as f32, t);

    s.pop_layer();
}

/// El volumen del texto: la «Q» de una muestra que eligió Roberto (2026-10-23), entre el
/// texto plano de antes y un biselado que fue demasiado. Un espesor de pocos píxeles hacia
/// abajo (copias oscuras apiladas), sombra en dos capas debajo de ese espesor (una fina y
/// pegada, una amplia), una copia clara apenas arriba (el filo) y la cara con un degradé
/// vertical, más clara arriba.
pub struct Relieve {
    /// (sigma, alfa, cuánto baja), en píxeles lógicos: sombras negras desenfocadas.
    pub sombras: &'static [(f32, f32, f64)],
    /// Cuántos píxeles de espesor (una copia oscura por píxel) y cuánto negro llevan.
    pub espesor: (u32, f32),
    /// Cuánto sube la copia clara y cuánto blanco lleva.
    pub filo: (f64, f32),
    /// Cuánto blanco arriba, cuánto negro abajo y entre qué fracciones del alto del texto.
    pub degrade: (f32, f32, f64, f64),
}

const PESO_RELOJ: f32 = 500.0;

pub const RELIEVE_RELOJ: Relieve = Relieve {
    sombras: &[(1.2, 0.55, 4.0), (10.0, 0.45, 6.0)],
    espesor: (2, 0.55),
    filo: (1.5, 0.6),
    degrade: (0.35, 0.12, 20.0 / 169.0, 150.0 / 169.0),
};

/// La fecha, con lo mismo en menor medida (y su grosor regular).
pub const RELIEVE_FECHA: Relieve = Relieve {
    sombras: &[(0.6, 0.45, 2.0), (4.0, 0.35, 2.5)],
    espesor: (1, 0.45),
    filo: (0.7, 0.4),
    degrade: (0.2, 0.08, 20.0 / 169.0, 150.0 / 169.0),
};

/// El aviso queda plano, con el `text-shadow` de 3 px de antes.
const SOMBRA_AVISO: &[(f32, f32, f64)] = &[(1.5, 0.6, 0.0)];

/// Las sombras de cada capa de `Rotulos::capas`, en el mismo orden.
pub fn sombras_de(i: usize) -> &'static [(f32, f32, f64)] {
    match i {
        0..=2 => RELIEVE_RELOJ.sombras,
        3 => RELIEVE_FECHA.sombras,
        _ => SOMBRA_AVISO,
    }
}

const NEGRO: Color = [0.0, 0.0, 0.0, 1.0];

fn con_relieve(
    s: &mut Scene,
    l: &parley::Layout<u8>,
    sombras: &[(Sombra, f64)],
    (x, y): (f64, f64),
    color: impl Fn(u8) -> Color,
    relieve: Option<&Relieve>,
    escala: f64,
) {
    for (sm, baja) in sombras {
        let brocha = ImageBrush::new(sm.imagen.clone());
        let en = (x - sm.margen, y - sm.margen + baja * escala);
        s.draw_image(&brocha, Affine::translate(en));
    }
    let Some(r) = relieve else {
        texto::dibujar_en(s, l, Affine::translate((x, y)), color, 1.0);
        return;
    };
    let (espesor, negro) = r.espesor;
    for i in (1..=espesor).rev() {
        let en = Affine::translate((x, y + i as f64 * escala));
        texto::dibujar_en(s, l, en, |b| mezclar(&color(b), &NEGRO, negro), 1.0);
    }
    let (sube, blanco) = r.filo;
    let en = Affine::translate((x, y - sube * escala));
    texto::dibujar_en(s, l, en, |b| mezclar(&color(b), &BLANCO, blanco), 1.0);
    let (claro, oscuro, y0, y1) = r.degrade;
    let alto = l.height() as f64;
    texto::dibujar_con(s, l, Affine::translate((x, y)), |b| {
        let c = color(b);
        let (arriba, abajo) = (mezclar(&c, &BLANCO, claro), mezclar(&c, &NEGRO, oscuro));
        Gradient::new_linear((0.0, alto * y0), (0.0, alto * y1))
            .with_stops([pincel(&arriba), pincel(&abajo)].as_slice())
            .into()
    });
}

fn sacudida(t: f32) -> f32 {
    const K: [(f32, f32); 7] = [
        (0.0, 0.0),
        (0.14, -11.0),
        (0.30, 9.0),
        (0.46, -6.0),
        (0.62, 4.0),
        (0.80, -2.0),
        (1.0, 0.0),
    ];
    if !(0.0..1.0).contains(&t) {
        return 0.0;
    }
    let i = K
        .iter()
        .rposition(|(k, _)| *k <= t)
        .unwrap_or(0)
        .min(K.len() - 2);
    let ((t0, v0), (t1, v1)) = (K[i], K[i + 1]);
    let f = (t - t0) / (t1 - t0);
    v0 + (v1 - v0) * (f * f * (3.0 - 2.0 * f))
}

fn campo(
    s: &mut Scene,
    e: &Estado,
    rot: &mut Rotulos,
    textos: &mut texto::Textos,
    origen: Affine,
    escala: f32,
    t: i64,
) {
    let p = e.paleta;
    let par = e.anillo.valor(t);
    let estilo = e.estilo;
    let tr = if estilo.sacudida {
        origen * Affine::translate((sacudida(seg(t - e.t_fallo) / 0.42) as f64, 0.0))
    } else {
        origen
    };

    let (alfa, extra) = match e.modo {
        Modo::Fallo(_) if estilo.rojo => {
            let k = (-seg(t - e.t_fallo) / 0.38).exp();
            (0.45 + 0.55 * k, k)
        }
        Modo::Saliendo if estilo.destello => {
            let k = (-(seg(t - e.t_acierto) - 0.2).max(0.0) / 0.3).exp();
            (0.45 + 0.55 * k, 0.6 * k)
        }
        _ => (0.45, 0.0),
    };
    caja(s, tr, &p, &par, alfa, extra);

    let latido = e.latido().then(|| seg(t - e.t_modo));
    match estilo.escritura {
        Escritura::Onda => onda(s, tr, e, t, &par),
        Escritura::Puntos => caracteres::puntos(s, tr, &e.fila, t, latido, &p),
        Escritura::Fijo => caracteres::fijos(s, tr, &e.fila, &p),
        Escritura::Simbolos => {
            let glifos = &mut rot.glifos;
            caracteres::simbolos(s, tr, &e.fila, t, latido, &p, |s, g, x, y, esc, c| {
                let l = glifos.entry(g).or_insert_with(|| {
                    textos.armar(&[(&g.to_string(), ROL_TEXTO)], texto::pt(13.0), escala)
                });
                let centro = tr * Point::new(x as f64, y as f64);
                let en = Affine::translate(centro.to_vec2())
                    * Affine::scale(esc as f64)
                    * Affine::translate((-l.width() as f64 / 2.0, -l.height() as f64 / 2.0));
                texto::dibujar_en(s, l, en, |_| c, 1.0);
            });
        }
    }

    // Mientras la onda late al verificar no hay texto: lo dice eso.
    let espera = match (estilo.reaccion, estilo.escritura) {
        (false, _) => 0.0,
        (true, Escritura::Onda) => FALLO_TEXTO,
        // Las × se apagan en 0,45 s y están donde va el texto.
        (true, Escritura::Simbolos) => 0.45,
        (true, _) => 0.25,
    };
    let texto = match e.modo {
        Modo::Normal if e.n == 0 => Some(("Contraseña".to_owned(), 1.0 - e.visible.valor(t))),
        Modo::Verificando if !estilo.verificar.late() => Some(("Verificando…".to_owned(), 1.0)),
        Modo::Fallo(n) if e.n == 0 => Some((
            format!("Autenticación fallida ({n})"),
            acotar((seg(t - e.t_fallo) - espera) / 0.2, 0.0, 1.0),
        )),
        _ => None,
    };
    if let Some((texto, a)) = texto
        && a > 0.01
    {
        if rot.campo.as_ref().is_none_or(|(t, _)| *t != texto) {
            let l = textos.armar(&[(&texto, ROL_TEXTO)], texto::pt(12.5), escala);
            rot.campo = Some((texto, l));
        }
        if let Some((_, l)) = &rot.campo {
            // En píxeles del buffer: el centro del campo, ya movido por la sacudida.
            let c = tr * Point::new(ANCHO as f64 / 2.0, ALTO as f64 / 2.0);
            texto::dibujar(s, l, c.x, c.y, |r| rol(&p, r), a);
        }
    }

    if e.modo == Modo::Verificando && estilo.verificar.gira() {
        let edad = seg(t - e.t_modo);
        let cabeza = PERIMETRO * 0.25 + edad * 520.0;
        let entrada = acotar(edad / 0.15, 0.0, 1.0);
        cometa(s, tr, cabeza, 1.0, 120.0, 0.95 * entrada, &par, 2.2);
        cometa(
            s,
            tr,
            cabeza + PERIMETRO / 2.0,
            1.0,
            120.0,
            0.95 * entrada,
            &par,
            2.2,
        );
    }
    for c in &e.chispas {
        let f = seg(t - c.t0) / 0.55;
        if f > 1.0 {
            continue;
        }
        let d = 495.0 * ease_out(f);
        cometa(
            s,
            tr,
            c.s0 + c.dir * d,
            c.dir,
            44.0 * c.fuerza,
            (1.0 - f) * c.fuerza,
            &par,
            2.0,
        );
    }
    if e.modo == Modo::Saliendo && estilo.destello {
        let edad = seg(t - e.t_acierto);
        let d = ease_out(edad / 0.26) * PERIMETRO / 2.0;
        let a = 1.0 - acotar((edad - 0.26) / 0.26, 0.0, 1.0);
        cometa(s, tr, BW / 2.0 + d, 1.0, d.max(8.0), a, &par, 2.4);
        cometa(s, tr, BW / 2.0 - d, -1.0, d.max(8.0), a, &par, 2.4);
    }
}

/// `box-shadow` de GTK: el rectángulo agrandado en `spread`, desplazado y desenfocado con
/// sigma = blur / 2.
fn sombra(s: &mut Scene, tr: Affine, c: Color, dx: f64, dy: f64, spread: f64, blur: f64) {
    let r = Rect::new(0.0, 0.0, ANCHO as f64, ALTO as f64).inflate(spread, spread)
        + vello::kurbo::Vec2::new(dx, dy);
    if blur <= 0.0 {
        let rr = RoundedRect::from_rect(r, RADIO + spread);
        s.fill(Fill::NonZero, tr, pincel(&c), None, &rr);
    } else {
        s.draw_blurred_rounded_rect(tr, r, pincel(&c), RADIO + spread, blur / 2.0);
    }
}

fn caja(s: &mut Scene, tr: Affine, p: &Paleta, par: &[Color; 2], alfa: f32, extra: f32) {
    let (w, h) = (ANCHO as f64, ALTO as f64);
    sombra(s, tr, [0.0, 0.0, 0.0, 0.5], 0.0, 0.0, 4.0, 10.0);
    sombra(s, tr, con_alfa(&p.surface, 0.85), 0.0, 0.0, 1.0, 0.0);
    let blur = 5.0 + 8.0 * extra as f64;
    sombra(s, tr, con_alfa(&par[0], alfa), -1.0, -1.0, 2.0, blur);
    sombra(s, tr, con_alfa(&par[1], alfa), 1.0, 1.0, 2.0, blur);

    let fuera = RoundedRect::new(0.0, 0.0, w, h, RADIO);
    s.fill(
        Fill::NonZero,
        tr,
        pincel(&[0.0, 0.0, 0.0, 0.9]),
        None,
        &fuera,
    );
    s.fill(
        Fill::NonZero,
        tr,
        pincel(&con_alfa(&p.surface_container, 0.85)),
        None,
        &fuera,
    );

    // El borde: arriba e izquierda con el primer color, abajo y derecha con el segundo; se
    // juntan en diagonal en las esquinas, como los bordes de GTK.
    let dentro = RoundedRect::new(BORDE, BORDE, w - BORDE, h - BORDE, RADIO - BORDE / 2.0);
    let mut anillo = BezPath::new();
    anillo.extend(vello::kurbo::Shape::path_elements(&fuera, 0.1));
    anillo.extend(vello::kurbo::Shape::path_elements(&dentro, 0.1));
    s.push_layer(Fill::EvenOdd, BlendMode::default(), 1.0, tr, &anillo);
    s.fill(Fill::NonZero, tr, pincel(&par[1]), None, &fuera.rect());
    let mut arriba = BezPath::new();
    arriba.move_to((0.0, 0.0));
    arriba.line_to((w, 0.0));
    arriba.line_to((w - BORDE, BORDE));
    arriba.line_to((BORDE, h - BORDE));
    arriba.line_to((0.0, h));
    arriba.close_path();
    s.fill(Fill::NonZero, tr, pincel(&par[0]), None, &arriba);
    s.pop_layer();
}

fn punto(s: f32) -> (f32, f32) {
    let mut s = s.rem_euclid(PERIMETRO);
    if s < BW {
        return (BX + s, BY);
    }
    s -= BW;
    if s < BH {
        return (BX + BW, BY + s);
    }
    s -= BH;
    if s < BW {
        return (BX + BW - s, BY + BH);
    }
    s -= BW;
    (BX, BY + BH - s)
}

/// La diagonal va suavizada para que la chispa no salte de color.
fn color_en(x: f32, y: f32, par: &[Color; 2]) -> Color {
    let t = acotar((x / ANCHO + y / ALTO) / 2.0, 0.0, 1.0);
    mezclar(&par[0], &par[1], ease_out(t))
}

fn circulo(s: &mut Scene, tr: Affine, x: f32, y: f32, r: f32, c: &Color) {
    let c0 = Circle::new((x as f64, y as f64), r as f64);
    s.fill(Fill::NonZero, tr, pincel(c), None, &c0);
}

#[allow(clippy::too_many_arguments)]
fn cometa(
    s: &mut Scene,
    tr: Affine,
    cabeza: f32,
    dir: f32,
    largo: f32,
    alfa: f32,
    par: &[Color; 2],
    grosor: f32,
) {
    if alfa <= 0.01 {
        return;
    }
    let pasos = (largo / 2.0).round().max(6.0) as usize;
    for i in (0..=pasos).rev() {
        let f = i as f32 / pasos as f32;
        let (x, y) = punto(cabeza - dir * f * largo);
        let c = mezclar(&color_en(x, y, par), &BLANCO, 0.45 * (1.0 - f));
        let a = alfa * (1.0 - f).powf(1.6);
        let r = grosor * (1.0 - 0.5 * f);
        circulo(s, tr, x, y, r * 2.6, &con_alfa(&c, a * 0.18));
        circulo(s, tr, x, y, r, &con_alfa(&c, a));
    }
}

fn onda(s: &mut Scene, tr: Affine, e: &Estado, t: i64, par: &[Color; 2]) {
    let vis = e.visible.valor(t);
    if vis < 0.01 {
        return;
    }
    let p = &e.paleta;
    let (cx, cy) = (ANCHO / 2.0, ALTO / 2.0);
    let mitad = (ONDA_X1 - ONDA_X0) / 2.0;
    let reloj = seg(t) / LENTO_TECLA;
    let respira = 0.5 + 0.5 * (seg(t - e.t_modo) / (1.1 * LENTO_EVENTO) * 2.0 * PI).sin();
    let ondula = e.n > 0 && matches!(e.modo, Modo::Normal | Modo::Fallo(_));

    let mut camino = BezPath::new();
    let mut x = ONDA_X0;
    while x <= ONDA_X1 + 0.01 {
        let d = (x - cx).abs();
        let borde = (PI * (x - ONDA_X0) / (ONDA_X1 - ONDA_X0))
            .sin()
            .max(0.0)
            .powf(0.6);
        let mut y = if ondula {
            0.7 * (x * 0.09 + reloj * 3.1).sin()
        } else {
            0.0
        };
        for i in &e.impulsos {
            let edad = seg(t - i.t0) / i.tipo.lento();
            let (frente, ancho, dec, k) = match i.tipo {
                Tipo::Sale => (430.0 * edad, 13.0, 0.42, 0.55),
                Tipo::Entra => (mitad - 430.0 * edad, 13.0, 0.42, 0.55),
                Tipo::Fallo => (260.0 * edad, 22.0, 0.5, 0.9),
                Tipo::Acierto => (380.0 * edad, 13.0, 0.42, 0.55),
            };
            let u = d - frente;
            y += i.amp * (-edad / dec).exp() * (-(u * u) / (ancho * ancho)).exp() * (u * k).sin();
        }
        if e.latido() {
            y += 4.5 * respira * (x * 0.055 - seg(t) * 4.2 / LENTO_EVENTO).sin();
        }
        let y = acotar(y * borde, -18.0, 18.0);
        let pt = (x as f64, (cy + y) as f64);
        if x == ONDA_X0 {
            camino.move_to(pt);
        } else {
            camino.line_to(pt);
        }
        x += 1.5;
    }

    // Con el golpe del fallo la onda es roja entera; se va con la visibilidad.
    let rojo = e
        .impulsos
        .iter()
        .find(|i| i.tipo == Tipo::Fallo)
        .map(|i| 1.0 - acotar((seg(t - i.t0) - FALLO_ONDA - 0.3) / 0.3, 0.0, 1.0))
        .unwrap_or(0.0);
    let mut c0 = mezclar(&par[0], &p.error, rojo);
    let mut c1 = mezclar(&par[1], &p.error, rojo);
    if e.modo == Modo::Saliendo {
        let blanco = 0.5 * (1.0 - acotar(seg(t - e.t_modo) / 0.3, 0.0, 1.0));
        c0 = mezclar(&c0, &BLANCO, blanco);
        c1 = mezclar(&c1, &BLANCO, blanco);
    }
    let degrade = |a: f32| {
        Gradient::new_linear((ONDA_X0 as f64, 0.0), (ONDA_X1 as f64, 0.0))
            .with_stops([pincel(&con_alfa(&c0, a)), pincel(&con_alfa(&c1, a))].as_slice())
    };
    let trazo = |ancho: f64| {
        Stroke::new(ancho)
            .with_caps(Cap::Round)
            .with_join(Join::Round)
    };
    // El brillo: en GTK, un blur de 4 sobre un trazo de 3 al 70 %.
    s.stroke(&trazo(7.0), tr, &degrade(0.12 * vis), None, &camino);
    s.stroke(&trazo(4.5), tr, &degrade(0.25 * vis), None, &camino);
    s.stroke(&trazo(2.0), tr, &degrade(vis), None, &camino);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_recorrido_da_la_vuelta_al_borde() {
        assert_eq!(punto(0.0), (BX, BY));
        assert_eq!(punto(BW), (BX + BW, BY));
        assert_eq!(punto(PERIMETRO), (BX, BY));
        let (x, y) = punto(BW + BH + BW / 2.0);
        assert!((x - (BX + BW / 2.0)).abs() < 1e-3 && (y - (BY + BH)).abs() < 1e-3);
    }

    #[test]
    fn la_sacudida_vuelve_a_cero() {
        assert_eq!(sacudida(0.0), 0.0);
        assert_eq!(sacudida(1.0), 0.0);
        assert!(sacudida(0.14) < -10.0);
    }
}
