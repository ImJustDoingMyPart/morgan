//! El calendario en una `Scene` de vello: su estado (el mes a la vista, el día elegido, el
//! clima y los eventos), dónde cae cada cosa y el dibujo. Las medidas son las de la versión con
//! GTK (`.calendario` y `.control-center` en `estilo.css` hasta 0.1.1):
//!
//! - el panel mide 380 px con sus márgenes de 8 (364 de borde a borde), centrado y 8 px debajo
//!   de la barra; 2 de borde y 14 de relleno;
//! - la cabecera (el mes, y «hoy», ‹ y › a la derecha, cada 12 px), 8 px, la caja del mes (8 de
//!   relleno; los días de la semana, 4 px, y la grilla de días con 2 px entre celdas), 8 px y
//!   las filas del detalle (el clima del día elegido y los eventos del mes), 8 px entre ellas;
//! - una celda: 4 px arriba y abajo, el número y debajo el glifo del clima; el punto de los
//!   eventos arriba a la derecha.

use std::time::{Duration, Instant};

use gtk::glib;
use morgan_motor::color::{Color, Paleta, con_alfa, pincel};
use morgan_motor::curva::Transicion;
use morgan_motor::forma::{panel, sombra};
use morgan_motor::parley::Layout;
use morgan_motor::texto::{self, Textos};
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Point, Rect, RoundedRect};
use morgan_motor::vello::peniko::{Fill, Gradient};

use super::modelo::{self, Clima, Pronostico};
use crate::agenda::Evento;
use crate::centro::marquesina;

pub type Dia = (i32, i32, i32);

/// El ancho con los márgenes, como el `size_request` de GTK.
const ANCHO: f64 = 380.0;
const MARGEN: f64 = 8.0;
const BORDE_Y_RELLENO: f64 = 16.0;
const ENTRE: f64 = 8.0;
const RELLENO_MES: f64 = 8.0;
const ENTRE_CELDAS: f64 = 2.0;
const CUADRO: f64 = 36.0;
const RADIO: f64 = 4.0;
const ROL: u8 = 0;

pub enum Orden {
    Abrir,
    Cerrar,
    Clima(Pronostico),
    Eventos(Vec<Evento>),
}

pub fn hoy() -> Dia {
    let h = glib::DateTime::now_local().expect("reloj local");
    (h.year(), h.month(), h.day_of_month())
}

#[derive(Default)]
pub struct Cal {
    pub mes: (i32, i32),
    pub elegido: Dia,
    pub clima: Vec<Clima>,
    pub ahora: Option<String>,
    pub eventos: Vec<Evento>,
    /// Sube con cada cambio de lo que se ve: la hoja se rearma cuando cambia.
    pub version: u64,
}

impl Cal {
    pub fn elegir(&mut self, d: Dia) {
        self.elegido = d;
        self.mes = (d.0, d.1);
        self.version += 1;
    }

    pub fn correr_mes(&mut self, n: i32) {
        self.mes = modelo::correr_mes(self.mes, n);
        self.version += 1;
    }

    pub fn tocar(&mut self, o: Objetivo) {
        match o {
            Objetivo::Hoy => self.elegir(hoy()),
            Objetivo::Antes => self.correr_mes(-1),
            Objetivo::Despues => self.correr_mes(1),
            Objetivo::Dia(d) => self.elegir(d),
        }
    }
}

/// Lo que responde a un click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Objetivo {
    Hoy,
    Antes,
    Despues,
    Dia(Dia),
}

struct Texto {
    l: Layout<u8>,
    /// Donde va: arriba a la izquierda, en lógicos.
    x: f64,
    y: f64,
}

struct Boton {
    que: Objetivo,
    texto: Texto,
    caja: Rect,
}

struct Celda {
    d: Dia,
    caja: Rect,
    numero: Texto,
    glifo: Option<(Layout<u8>, Rect)>,
    marca: bool,
    hoy: bool,
    elegido: bool,
    pasado: bool,
}

struct Fila {
    caja: Rect,
    glifo: Layout<u8>,
    evento: bool,
    elegido: bool,
    pasado: bool,
    /// El título, en marquesina si no entra: el recorte y cuánto sobra.
    titulo: (Texto, Rect, f64),
    cuando: Option<Texto>,
    debajo: Option<(Texto, Rect, f64)>,
}

/// Todo lo que se dibuja, ya ubicado. Se rearma cuando cambia el estado.
pub struct Hoja {
    pub version: u64,
    pub escala: f32,
    pub ancho: u32,
    pub panel: Rect,
    titulo: Texto,
    botones: Vec<Boton>,
    semana: Vec<Texto>,
    mes: Rect,
    celdas: Vec<Celda>,
    filas: Vec<Fila>,
}

/// Columnas iguales en `total`, como `GtkGrid` homogénea: el resto, un píxel a cada una de
/// las primeras.
fn columnas(x0: f64, total: f64, n: usize, entre: f64) -> Vec<(f64, f64)> {
    let libre = (total - entre * (n - 1) as f64).max(0.0) as i64;
    let (base, resto) = (libre / n as i64, libre % n as i64);
    let mut x = x0;
    (0..n as i64)
        .map(|i| {
            let w = (base + i64::from(i < resto)) as f64;
            let c = (x, w);
            x += w + entre;
            c
        })
        .collect()
}

impl Hoja {
    /// `ancho`: el de la superficie (la salida), en lógicos.
    pub fn armar(t: &mut Textos, cal: &Cal, ancho: u32, escala: f32) -> Hoja {
        let e = f64::from(escala);
        let armar = |t: &mut Textos, s: &str, px: f32, peso: f32| {
            t.armar_con_peso(&[(s, ROL)], px, peso, escala)
        };
        let medida = |l: &Layout<u8>| (texto::caja(l).0 / e, texto::alto_de_linea(l) / e);
        let hoy = hoy();
        let x0 = ((f64::from(ancho) - ANCHO) / 2.0).floor() + MARGEN;
        let ancho_panel = ANCHO - 2.0 * MARGEN;
        let cx = x0 + BORDE_Y_RELLENO;
        let cw = ancho_panel - 2.0 * BORDE_Y_RELLENO;
        let mut y = MARGEN + BORDE_Y_RELLENO;

        // La cabecera.
        let (año, mes) = cal.mes;
        let titulo = armar(
            t,
            &format!("{} {año}", modelo::MESES[(mes - 1) as usize]),
            12.0,
            700.0,
        );
        let (_, th) = medida(&titulo);
        let mut botones = Vec::new();
        let ver_hoy = cal.elegido != hoy || (año, mes) != (hoy.0, hoy.1);
        let mut x = cx + cw;
        for (que, s) in [
            (Objetivo::Despues, "󰅂"),
            (Objetivo::Antes, "󰅁"),
            (Objetivo::Hoy, "hoy"),
        ] {
            if que == Objetivo::Hoy && !ver_hoy {
                continue;
            }
            let l = armar(t, s, 12.0, 400.0);
            let (w, h) = medida(&l);
            x -= w;
            botones.push(Boton {
                que,
                texto: Texto { l, x, y },
                caja: Rect::new(x, y, x + w, y + h),
            });
            x -= 12.0;
        }
        let alto_cab = botones.iter().map(|b| b.caja.height()).fold(th, f64::max);
        let titulo = Texto {
            l: titulo,
            x: cx,
            y: y + ((alto_cab - th) / 2.0).floor(),
        };
        for b in &mut botones {
            let dy = ((alto_cab - b.caja.height()) / 2.0).floor();
            b.texto.y += dy;
            b.caja = b.caja + morgan_motor::vello::kurbo::Vec2::new(0.0, dy);
        }
        y += alto_cab + ENTRE;

        // El mes.
        let mes_y = y;
        let gx = cx + RELLENO_MES;
        let gw = cw - 2.0 * RELLENO_MES;
        y += RELLENO_MES;
        let mut semana = Vec::new();
        let mut alto_semana: f64 = 0.0;
        for (d, (x, w)) in ["lu", "ma", "mi", "ju", "vi", "sá", "do"]
            .iter()
            .zip(columnas(gx, gw, 7, 0.0))
        {
            let l = armar(t, d, 12.0, 400.0);
            let (lw, lh) = medida(&l);
            alto_semana = alto_semana.max(lh);
            semana.push(Texto {
                l,
                x: x + ((w - lw) / 2.0).floor(),
                y,
            });
        }
        y += alto_semana + 4.0;

        let zona = glib::TimeZone::local();
        let con_eventos = modelo::dias_con_eventos(&cal.eventos, cal.mes, &zona);
        let (vacios, dias) = modelo::mes(año, mes);
        let cols = columnas(gx, gw, 7, ENTRE_CELDAS);
        let mut celdas = Vec::new();
        let mut alto_fila = 0.0;
        for n in 1..=dias {
            let d = (año, mes, n as i32);
            let lugar = (vacios + n - 1) as usize;
            let (col, fila) = (lugar % 7, lugar / 7);
            let desde_hoy = modelo::distancia(hoy, d);
            let numero = armar(t, &n.to_string(), 14.0, 400.0);
            let (nw, nh) = medida(&numero);
            // El glifo del clima ocupa su renglón (14 px como mínimo) aunque no haya.
            let glifo = usize::try_from(desde_hoy)
                .ok()
                .and_then(|k| cal.clima.get(k))
                .map(|c| armar(t, &c.glifo, 12.0, 400.0));
            let gh = medida(&armar(t, " ", 12.0, 400.0)).1.max(14.0);
            alto_fila = 8.0 + nh + gh;
            let (x, w) = cols[col];
            let cy = y + fila as f64 * (alto_fila + ENTRE_CELDAS);
            let caja = Rect::new(x, cy, x + w, cy + alto_fila);
            celdas.push(Celda {
                d,
                caja,
                numero: Texto {
                    l: numero,
                    x: x + ((w - nw) / 2.0).floor(),
                    y: cy + 4.0,
                },
                glifo: glifo.map(|l| (l, Rect::new(x, cy + 4.0 + nh, x + w, cy + 4.0 + nh + gh))),
                marca: con_eventos.contains(&n),
                hoy: d == hoy,
                elegido: d == cal.elegido,
                pasado: desde_hoy < 0,
            });
        }
        let filas_mes = (vacios + dias).div_ceil(7) as f64;
        y += filas_mes * alto_fila + (filas_mes - 1.0) * ENTRE_CELDAS + RELLENO_MES;
        let caja_mes = Rect::new(cx, mes_y, cx + cw, y);

        // El detalle.
        let mut filas = Vec::new();
        let tx = cx + 8.0 + CUADRO + 10.0;
        let tw = cx + cw - 8.0 - tx;
        let d = cal.elegido;
        let desde_hoy = modelo::distancia(hoy, d);
        let renglones = |t: &mut Textos,
                         arriba: &str,
                         a_la_derecha: Option<&str>,
                         abajo: Option<&str>,
                         fy: f64| {
            let l = armar(t, arriba, 14.0, 400.0);
            let (_, h1) = medida(&l);
            let cuando = a_la_derecha.map(|s| armar(t, s, 12.0, 400.0));
            let abajo = abajo.map(|s| armar(t, s, 12.0, 400.0));
            let h2 = abajo.as_ref().map_or(0.0, |l| medida(l).1);
            let alto = (h1 + h2).max(CUADRO);
            let ty = fy + 8.0 + ((alto - h1 - h2) / 2.0).floor();
            let cuando = cuando.map(|l| {
                let (w, h) = medida(&l);
                Texto {
                    x: tx + tw - w,
                    y: ty + ((h1 - h) / 2.0).floor(),
                    l,
                }
            });
            let libre = cuando.as_ref().map_or(tw, |c| tw - medida(&c.l).0 - 8.0);
            let recorte = Rect::new(tx, ty, tx + libre, ty + h1);
            let sobra = (medida(&l).0 - libre).max(0.0);
            let titulo = (Texto { l, x: tx, y: ty }, recorte, sobra);
            let debajo = abajo.map(|l| {
                let (w, h) = medida(&l);
                let r = Rect::new(tx, ty + h1, tx + tw, ty + h1 + h);
                (
                    Texto {
                        l,
                        x: tx,
                        y: ty + h1,
                    },
                    r,
                    (w - tw).max(0.0),
                )
            });
            (titulo, cuando, debajo, alto + 16.0)
        };
        if let Some(c) = usize::try_from(desde_hoy)
            .ok()
            .and_then(|k| cal.clima.get(k))
        {
            y += ENTRE;
            let estado = match cal.ahora.as_deref() {
                Some(a) if d == hoy => format!("ahora {a} · {}", c.temperaturas),
                _ => c.temperaturas.clone(),
            };
            let (titulo, _, debajo, alto) = renglones(t, &c.descripcion, None, Some(&estado), y);
            filas.push(Fila {
                caja: Rect::new(cx, y, cx + cw, y + alto),
                glifo: armar(t, &c.glifo, 18.0, 400.0),
                evento: false,
                elegido: false,
                pasado: false,
                // La descripción del clima es corta: sin marquesina.
                titulo: (titulo.0, titulo.1, 0.0),
                cuando: None,
                debajo: debajo.map(|(t, r, _)| (t, r, 0.0)),
            });
            y += alto;
        }
        for ev in modelo::del_mes(&cal.eventos, cal.mes, &zona) {
            y += ENTRE;
            let cuando = modelo::cuando(ev, &zona);
            let lugar = (!ev.lugar.is_empty()).then_some(ev.lugar.as_str());
            let (titulo, cuando, debajo, alto) = renglones(t, &ev.resumen, Some(&cuando), lugar, y);
            let dia = modelo::dia(ev, &zona);
            filas.push(Fila {
                caja: Rect::new(cx, y, cx + cw, y + alto),
                glifo: armar(t, "󰃭", 18.0, 400.0),
                evento: true,
                elegido: dia == Some(d),
                pasado: dia != Some(d) && dia.is_some_and(|x| modelo::distancia(hoy, x) < 0),
                titulo,
                cuando,
                debajo,
            });
            y += alto;
        }
        y += BORDE_Y_RELLENO;

        Hoja {
            version: cal.version,
            escala,
            ancho,
            panel: Rect::new(x0, MARGEN, x0 + ancho_panel, y),
            titulo,
            botones,
            semana,
            mes: caja_mes,
            celdas,
            filas,
        }
    }

    pub fn bajo(&self, x: f64, y: f64) -> Option<Objetivo> {
        let p = Point::new(x, y);
        self.botones
            .iter()
            .find(|b| b.caja.contains(p))
            .map(|b| b.que)
            .or_else(|| {
                self.celdas
                    .iter()
                    .find(|c| c.caja.contains(p))
                    .map(|c| Objetivo::Dia(c.d))
            })
    }

    pub fn dentro(&self, x: f64, y: f64) -> bool {
        self.panel.contains(Point::new(x, y))
    }

    /// Si hay algún título que no entra y anda en marquesina.
    pub fn marquesina(&self) -> bool {
        self.filas
            .iter()
            .any(|f| f.titulo.2 > 0.0 || f.debajo.as_ref().is_some_and(|d| d.2 > 0.0))
    }

    /// Dónde cae `o`, para saber si el puntero sigue encima al soltar.
    pub fn caja(&self, o: Objetivo) -> Option<Rect> {
        match o {
            Objetivo::Dia(d) => self.celdas.iter().find(|c| c.d == d).map(|c| c.caja),
            _ => self.botones.iter().find(|b| b.que == o).map(|b| b.caja),
        }
    }
}

/// El puntero encima y lo apretado de cada objetivo, con sus transiciones.
#[derive(Default)]
pub struct Animos(Vec<(Objetivo, Transicion, Transicion)>);

/// `.cal-dia`: el fondo en 150 ms y el tamaño en 100; `.cc-seccion button`: la opacidad en
/// 100 y el tamaño en 150.
fn tiempos(o: Objetivo) -> (Duration, Duration, f32) {
    match o {
        Objetivo::Dia(_) => (Duration::from_millis(150), Duration::from_millis(100), 0.94),
        _ => (Duration::from_millis(100), Duration::from_millis(150), 0.9),
    }
}

impl Animos {
    fn entrada(&mut self, o: Objetivo) -> &mut (Objetivo, Transicion, Transicion) {
        let i = match self.0.iter().position(|(x, ..)| *x == o) {
            Some(i) => i,
            None => {
                self.0.push((o, Transicion::new(0.0), Transicion::new(1.0)));
                self.0.len() - 1
            }
        };
        &mut self.0[i]
    }

    pub fn encima(&mut self, o: Objetivo, si: bool, ahora: Instant) {
        let d = crate::animaciones::dur(tiempos(o).0);
        self.entrada(o).1.ir(if si { 1.0 } else { 0.0 }, d, ahora);
    }

    pub fn apretar(&mut self, o: Objetivo, si: bool, ahora: Instant) {
        let (_, d, k) = tiempos(o);
        let e = self.entrada(o);
        if si {
            e.2.ir(k, Duration::ZERO, ahora);
        } else {
            e.2.ir(1.0, crate::animaciones::dur(d), ahora);
        }
    }

    fn valores(&self, o: Objetivo, ahora: Instant) -> (f32, f32) {
        self.0
            .iter()
            .find(|(x, ..)| *x == o)
            .map_or((0.0, 1.0), |(_, h, e)| (h.valor(ahora), e.valor(ahora)))
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        self.0
            .iter()
            .any(|(_, h, e)| h.anda(ahora) || e.anda(ahora))
    }
}

fn opaco(c: &Color, op: f32) -> Color {
    con_alfa(c, c[3] * op)
}

/// `linear-gradient(135deg, a, b)` de CSS sobre `r`.
fn degrade_135(r: Rect, a: Color, b: Color) -> Gradient {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let largo = (r.width() + r.height()) * s;
    let c = r.center();
    let d = s * largo / 2.0;
    Gradient::new_linear(Point::new(c.x - d, c.y - d), Point::new(c.x + d, c.y + d))
        .with_stops([pincel(&a), pincel(&b)].as_slice())
}

pub struct Pintura<'a> {
    pub hoja: &'a Hoja,
    pub animos: &'a Animos,
    pub paleta: &'a Paleta,
    pub opacidad: f32,
    pub ahora: Instant,
    /// Cuánto falta bajar al panel (la entrada), en lógicos.
    pub bajada: f64,
    /// Segundos desde que arrancó la marquesina.
    pub marquesina: f64,
}

impl Pintura<'_> {
    pub fn pintar(&self, s: &mut Scene, escala: f64) {
        s.reset();
        let h = self.hoja;
        let p = self.paleta;
        let tr = Affine::scale(escala) * Affine::translate((0.0, -self.bajada.round()));
        // Lo que se ve está corrido: el texto, en píxeles del buffer, también.
        let tt = Affine::translate((0.0, -self.bajada.round() * escala));
        panel(s, tr, p, self.opacidad, h.panel);
        let texto = |s: &mut Scene, t: &Texto, c: Color, k: Affine| {
            let pos = Affine::translate(((t.x * escala).round(), (t.y * escala).round()));
            texto::dibujar_en(s, &t.l, tt * k * pos, |_| c, 1.0);
        };
        // Achicar hacia el centro de `r`, en píxeles del buffer.
        let hacia = |r: Rect, k: f32| {
            let c = r.center();
            let (x, y) = (c.x * escala, c.y * escala);
            Affine::translate((x, y)) * Affine::scale(f64::from(k)) * Affine::translate((-x, -y))
        };

        texto(s, &h.titulo, opaco(&p.on_surface, 0.5), Affine::IDENTITY);
        for b in &h.botones {
            let (hv, k) = self.animos.valores(b.que, self.ahora);
            // El botón a 0,5 (1 con el puntero) y su etiqueta a 0,5 encima.
            let c = opaco(&p.on_surface, 0.5 * (0.5 + 0.5 * hv));
            let k = hacia(b.caja, k);
            if b.que == Objetivo::Hoy {
                texto(s, &b.texto, c, k);
            } else {
                let r = b.caja;
                let c0 = r.center();
                let (x, y) = texto::esquina_por_tinta(&b.texto.l, c0.x * escala, c0.y * escala);
                texto::dibujar_en(
                    s,
                    &b.texto.l,
                    tt * k * Affine::translate((x, y)),
                    |_| c,
                    1.0,
                );
            }
        }

        s.fill(
            Fill::NonZero,
            tr,
            pincel(&opaco(&p.on_surface, 0.06)),
            None,
            &RoundedRect::from_rect(h.mes, RADIO),
        );
        for t in &h.semana {
            texto(s, t, opaco(&p.on_surface, 0.5), Affine::IDENTITY);
        }
        for c in &h.celdas {
            let (hv, k) = self.animos.valores(Objetivo::Dia(c.d), self.ahora);
            let ka = hacia(c.caja, k);
            let m = c.caja.center();
            let tc = tr
                * Affine::translate((m.x, m.y))
                * Affine::scale(f64::from(k))
                * Affine::translate((-m.x, -m.y));
            let rr = RoundedRect::from_rect(c.caja, RADIO);
            if c.hoy {
                sombra(
                    s,
                    tc,
                    c.caja,
                    RADIO,
                    con_alfa(&p.tertiary, 0.3),
                    (0.0, 0.0),
                    0.0,
                    6.0,
                );
                s.fill(
                    Fill::NonZero,
                    tc,
                    &degrade_135(c.caja, p.primary, p.tertiary),
                    None,
                    &rr,
                );
            } else {
                let a = if c.elegido { 0.2 } else { 0.14 * hv };
                if a > 0.0 {
                    s.fill(Fill::NonZero, tc, pincel(&opaco(&p.primary, a)), None, &rr);
                }
            }
            let tinta = if c.hoy { p.surface } else { p.on_surface };
            let op = if c.pasado { 0.5 } else { 1.0 };
            texto(s, &c.numero, opaco(&tinta, op), ka);
            if let Some((l, r)) = &c.glifo {
                let color = if c.hoy { p.surface } else { p.tertiary };
                let m = r.center();
                let (x, y) = texto::esquina_por_tinta(l, m.x * escala, m.y * escala);
                texto::dibujar_en(s, l, tt * ka * Affine::translate((x, y)), |_| color, 1.0);
            }
            if c.marca {
                let m = Rect::new(
                    c.caja.x1 - 10.0,
                    c.caja.y0 + 6.0,
                    c.caja.x1 - 6.0,
                    c.caja.y0 + 10.0,
                );
                let color = if c.hoy { p.surface } else { p.tertiary };
                s.fill(
                    Fill::NonZero,
                    tc,
                    pincel(&color),
                    None,
                    &RoundedRect::from_rect(m, 2.0),
                );
            }
        }

        for f in &h.filas {
            let fondo = if f.elegido {
                opaco(&p.primary, 0.2)
            } else {
                opaco(&p.on_surface, 0.06)
            };
            s.fill(
                Fill::NonZero,
                tr,
                pincel(&fondo),
                None,
                &RoundedRect::from_rect(f.caja, RADIO),
            );
            let op = if f.pasado { 0.5 } else { 1.0 };
            let cuadro = Rect::new(
                f.caja.x0 + 8.0,
                f.caja.y0 + 8.0,
                f.caja.x0 + 8.0 + CUADRO,
                f.caja.y0 + 8.0 + CUADRO,
            );
            let (fondo_cuadro, tinta) = if f.evento {
                (opaco(&p.tertiary, 0.12 * op), opaco(&p.tertiary, op))
            } else {
                (opaco(&p.on_surface, 0.08 * op), opaco(&p.on_surface, op))
            };
            s.fill(
                Fill::NonZero,
                tr,
                pincel(&fondo_cuadro),
                None,
                &RoundedRect::from_rect(cuadro, RADIO),
            );
            let m = cuadro.center();
            let (x, y) = texto::esquina_por_tinta(&f.glifo, m.x * escala, m.y * escala);
            texto::dibujar_en(s, &f.glifo, tt * Affine::translate((x, y)), |_| tinta, 1.0);

            let t_marq = self.marquesina;
            let recortado = |s: &mut Scene, (t, r, sobra): &(Texto, Rect, f64), c: Color| {
                let corrido = marquesina::posicion(t_marq, *sobra).round();
                s.push_clip_layer(Fill::NonZero, tr, r);
                let pos =
                    Affine::translate((((t.x - corrido) * escala).round(), (t.y * escala).round()));
                texto::dibujar_en(s, &t.l, tt * pos, |_| c, 1.0);
                s.pop_layer();
            };
            recortado(s, &f.titulo, opaco(&p.on_surface, op));
            if let Some(c) = &f.cuando {
                texto(s, c, opaco(&p.on_surface, 0.5 * op), Affine::IDENTITY);
            }
            if let Some(d) = &f.debajo {
                recortado(s, d, opaco(&p.on_surface, 0.5 * op));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columnas_como_gtk() {
        // 304 px en 7 columnas con 2 entre ellas: las tres primeras llevan el resto.
        let c = columnas(0.0, 316.0, 7, 2.0);
        let anchos: Vec<f64> = c.iter().map(|(_, w)| *w).collect();
        assert_eq!(anchos, [44.0, 44.0, 44.0, 43.0, 43.0, 43.0, 43.0]);
        assert_eq!(c[1].0, 46.0);
        assert_eq!(c[6].0 + c[6].1, 316.0);
    }
}
