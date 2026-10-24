//! Los toasts, con las piezas del centro: cada uno es una tarjeta suelta con el anillo del OSD
//! (`forma::tarjeta`), en una columna pegada a la derecha, lo nuevo arriba. Lo que se toca es
//! como en la lista del centro: el click hace la acción por defecto (medio y derecho cierran),
//! los botones van a la vista debajo, a la derecha se descarta y a la izquierda se copia. El
//! puntero encima la agranda y frena su cuenta (la cuenta la lleva el hilo de GTK).
//!
//! Las medidas, de la hoja de GTK (`.floating-notifications` hasta 0.1.1): la columna de 304,
//! cada tarjeta a 16 de la izquierda, 12 de la derecha y 6 arriba y abajo (12 entre dos); 4 de
//! relleno; un ícono del tema a 24 en una columna de 36 (sin fondo) y 10 hasta el texto, una
//! foto a 40 con 4 alrededor; el título a 14 y el cuerpo a 12 en hasta 5 renglones; los botones
//! en fila si entran (`modelo::acciones_en_fila`), si no uno debajo del otro.

use std::collections::HashMap;
use std::time::Instant;

use morgan_motor::color::{Color, Paleta, con_alfa};
use morgan_motor::forma::{Anillo, Borde, tarjeta};
use morgan_motor::parley::Layout;
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Point, Rect, Vec2};
use morgan_motor::vello::peniko::{BlendMode, Fill, ImageData};

use super::armar::pista;
use super::avisos::tam_previa_en;
use super::toque::{Arrastre, IZQUIERDO, Lado, Paso};
use super::trazo::{self, Trazo};
use super::{Animos, Desliz, Hoja, Objetivo, RADIO, Rotulos};
use crate::centro::vista::{Aviso, EventoToast};
use crate::notificaciones::modelo::{self, Urgencia};

/// La columna, y la tarjeta adentro.
pub const ANCHO: u32 = 304;
const X0: f64 = 16.0;
const TARJETA: f64 = 276.0;
const ARRIBA: f64 = 6.0;
const ENTRE: f64 = 12.0;
const RELLENO: f64 = 4.0;
const CONTENIDO: f64 = TARJETA - 2.0 * RELLENO;
const CUADRO: f64 = 36.0;
const ICONO: f64 = 24.0;
const AL_TEXTO: f64 = 10.0;
const FOTO: f64 = 40.0;
const MARGEN_FOTO: f64 = 4.0;
const CHICO: f64 = 16.0;
const LINEAS: usize = 5;
/// La vista previa, de ancho fijo (el del contenido).
const ANCHO_PREVIA: f64 = 268.0;
/// Agrandada con el puntero encima (`transform: scale(1.02)`).
const AGRANDADA: f64 = 0.02;

fn op(c: &Color, a: f32) -> Color {
    con_alfa(c, c[3] * a)
}

/// Una tarjeta armada: su caja en la columna (sin el scroll) y lo de adentro desde su esquina.
struct Carta {
    id: u32,
    r: Rect,
    anillo: Anillo,
    escala: f64,
    dx: f64,
    alfa: f32,
    /// Lo que asoma debajo mientras se arrastra, en la columna.
    pista: Vec<Trazo>,
    trazos: Vec<Trazo>,
}

pub struct ToastsUi {
    pub avisos: Vec<Aviso>,
    pub paleta: Paleta,
    pub opacidad: f32,
    pub borde: Borde,
    rotulos: Rotulos,
    animos: Animos,
    deslices: HashMap<Objetivo, Desliz>,
    arrastre: Option<Arrastre>,
    /// Lo que responde bajo el puntero, y el toast entero bajo el puntero.
    encima: Option<Objetivo>,
    sobre: Option<u32>,
    /// El toast bajo el puntero que ya sabe el hilo de GTK.
    avisado: Option<u32>,
    apretado: Option<Objetivo>,
    puntero: Option<(f64, f64)>,
    scroll: f64,
    cartas: Vec<Carta>,
    /// Dónde responde cada cosa, en la columna.
    hoja: Hoja,
    alto: f64,
    /// La columna cambió debajo del puntero quieto: lo encendido se rehace en otro cuadro.
    rehacer: bool,
    pub tam: (u32, u32),
}

impl ToastsUi {
    pub fn new(paleta: Paleta, opacidad: f32, borde: Borde) -> Self {
        ToastsUi {
            avisos: Vec::new(),
            paleta,
            opacidad,
            borde,
            rotulos: Rotulos::new(),
            animos: Animos::default(),
            deslices: HashMap::new(),
            arrastre: None,
            encima: None,
            sobre: None,
            avisado: None,
            apretado: None,
            puntero: None,
            scroll: 0.0,
            cartas: Vec::new(),
            hoja: Hoja::default(),
            alto: 0.0,
            rehacer: false,
            tam: (0, 0),
        }
    }

    fn escala(&self) -> f64 {
        f64::from(self.rotulos.escala())
    }

    fn ancho(&self, l: &Layout<u8>) -> f64 {
        morgan_motor::texto::caja(l).0 / self.escala()
    }

    fn alto_de(&self, l: &Layout<u8>) -> f64 {
        morgan_motor::texto::alto_de_linea(l) / self.escala()
    }

    /// Hasta dónde llega lo que se ve (desde ahí baja al aparecer).
    pub fn fondo(&self) -> f64 {
        (self.alto - self.scroll).min(f64::from(self.tam.1))
    }

    fn max_scroll(&self) -> f64 {
        (self.alto - f64::from(self.tam.1)).max(0.0)
    }

    pub fn armar(&mut self, escala: f32, ahora: Instant) {
        self.rotulos.poner_escala(escala);
        let avisos = std::mem::take(&mut self.avisos);
        let mut hoja = Hoja::default();
        let mut cartas = Vec::with_capacity(avisos.len());
        let mut y = ARRIBA;
        for a in &avisos {
            let mut sub = Hoja::default();
            let alto = self.armar_toast(&mut sub, a, ahora);
            let r = Rect::new(X0, y, X0 + TARJETA, y + alto);
            let o = Objetivo::Aviso(a.id);
            let luz = self.animos.luz(o, self.sobre == Some(a.id), ahora);
            let dx = self.deslices.get(&o).map_or(0.0, |d| d.dx(ahora));
            let mut p = Hoja::default();
            if dx != 0.0 {
                pista(
                    &mut self.rotulos,
                    &self.paleta,
                    &mut p,
                    r,
                    dx,
                    Some(("󰆴", "descartar")),
                    Some(("󰆏", "copiar")),
                );
            }
            let mover = Vec2::new(r.x0, r.y0);
            for (o, rr) in sub.objetivos {
                hoja.objetivo(o, rr + mover, false);
            }
            hoja.presentes.extend(sub.presentes);
            hoja.deslizables.push((o, r));
            cartas.push(Carta {
                id: a.id,
                r,
                anillo: if a.urgencia == Urgencia::Critica {
                    Anillo::Liso(self.paleta.error)
                } else {
                    Anillo::Bicolor
                },
                escala: 1.0 + AGRANDADA * f64::from(luz),
                dx,
                alfa: crate::deslizable::opacidad(dx, TARJETA) as f32,
                pista: p.trazos,
                trazos: sub.trazos,
            });
            y += alto + ENTRE;
        }
        self.avisos = avisos;
        self.alto = y - ENTRE + ARRIBA;
        let vivos: std::collections::HashSet<Objetivo> = hoja
            .presentes
            .iter()
            .copied()
            .chain(cartas.iter().map(|c| Objetivo::Aviso(c.id)))
            .collect();
        self.animos.retener(&vivos);
        self.deslices
            .retain(|o, d| vivos.contains(o) && (d.dx(ahora) != 0.0 || d.anda(ahora)));
        self.hoja = hoja;
        self.cartas = cartas;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        // La columna cambió debajo del puntero quieto.
        self.rehacer = false;
        if let Some((x, y)) = self.puntero
            && !self.arrastre.is_some_and(|a| a.activo())
        {
            let antes = (self.sobre, self.encima);
            self.mirar(x, y);
            self.rehacer = antes != (self.sobre, self.encima);
        }
        self.rotulos.barrer();
    }

    /// Un toast desde su esquina; devuelve su alto.
    fn armar_toast(&mut self, h: &mut Hoja, a: &Aviso, ahora: Instant) -> f64 {
        let p = self.paleta;
        let (x, mut y) = (RELLENO, RELLENO);
        let (col_w, col_h) = columna(a);
        let tx = x + col_w;
        let tw = CONTENIDO - col_w;
        let titulo = if a.urgencia == Urgencia::Critica {
            self.rotulos.recortada_con_peso(&a.titulo, 14.0, 700.0, tw)
        } else {
            self.rotulos.recortada(&a.titulo, 14.0, tw)
        };
        let th = self.alto_de(&titulo);
        let cuerpo =
            (!a.cuerpo.is_empty()).then(|| self.rotulos.parrafo(&a.cuerpo, 12.0, tw, LINEAS));
        let ch = cuerpo.as_ref().map_or(0.0, |c| {
            (morgan_motor::texto::caja(c).1 / self.escala()).ceil()
        });
        let fila = col_h.max(th + ch);
        if let Some(img) = &a.imagen {
            let oy = y + ((fila - col_h) / 2.0).floor();
            if a.icono {
                // Sin el fondo del cuadrito (Roberto, 2026-10-24): solo su lugar.
                let cuadro = Rect::new(x, oy, x + CUADRO, oy + CUADRO);
                h.trazos.push(Trazo::Imagen {
                    img: img.clone(),
                    r: encajar(img, cuadro, ICONO),
                    radio: 0.0,
                    alfa: 1.0,
                });
            } else {
                let caja =
                    Rect::new(x, oy, x + FOTO, oy + FOTO) + Vec2::new(MARGEN_FOTO, MARGEN_FOTO);
                h.trazos.push(Trazo::Imagen {
                    img: img.clone(),
                    r: encajar(img, caja, FOTO),
                    radio: RADIO,
                    alfa: 1.0,
                });
            }
            if let Some(chico) = &a.chico {
                // En la esquina de abajo de la superposición (con los márgenes de la imagen),
                // 6 adentro.
                let (x1, y1) = (x + col_w - 6.0, oy + col_h - 6.0);
                let rc = Rect::new(x1 - CHICO, y1 - CHICO, x1, y1);
                // `-gtk-icon-shadow: 0 1px 4px black`, con la forma de un círculo.
                h.trazos.push(Trazo::Sombra {
                    r: rc + Vec2::new(0.0, 1.0),
                    radio: CHICO / 2.0,
                    color: [0.0, 0.0, 0.0, 0.6],
                    spread: 0.0,
                    blur: 4.0,
                });
                h.trazos.push(Trazo::Imagen {
                    img: chico.clone(),
                    r: rc,
                    radio: 0.0,
                    alfa: 1.0,
                });
            }
        }
        let ty = y + ((fila - th - ch) / 2.0).floor();
        h.trazos.push(Trazo::Texto {
            l: titulo,
            x: tx,
            y: ty,
            color: match a.urgencia {
                Urgencia::Critica => p.error,
                Urgencia::Baja => op(&p.on_surface, 0.6),
                Urgencia::Normal => p.on_surface,
            },
        });
        if let Some(c) = cuerpo {
            h.trazos.push(Trazo::Texto {
                l: c,
                x: tx,
                y: ty + th,
                color: op(&p.on_surface, 0.5),
            });
        }
        y += fila;
        if let Some((img, w, hh)) = &a.previa {
            let (pw, ph) = tam_previa_en(*w, *hh, ANCHO_PREVIA);
            y += 8.0;
            let px = x + ((CONTENIDO - pw) / 2.0).floor();
            h.trazos.push(Trazo::Imagen {
                img: img.clone(),
                r: Rect::new(px, y, px + pw, y + ph),
                radio: RADIO,
                alfa: 1.0,
            });
            y += ph;
        }
        if let Some(v) = a.progreso {
            y += 8.0 + 4.0;
            h.trazos.push(Trazo::Caja {
                r: Rect::new(x, y, x + CONTENIDO, y + 2.0),
                radio: 0.0,
                color: p.outline_variant,
            });
            let w = CONTENIDO * (f64::from(v) / 100.0).clamp(0.0, 1.0);
            h.trazos.push(Trazo::Caja {
                r: Rect::new(x, y, x + w, y + 2.0),
                radio: 0.0,
                color: p.primary,
            });
            y += 2.0;
        }
        y += RELLENO;
        // La acción por defecto: todo menos los botones.
        h.objetivo(
            Objetivo::Aviso(a.id),
            Rect::new(0.0, 0.0, TARJETA, y),
            false,
        );
        if a.acciones.is_empty() {
            return y;
        }
        y += 6.0;
        let en_fila = modelo::acciones_en_fila(
            a.acciones
                .iter()
                .map(|(_, etiqueta, g)| (g.is_some(), etiqueta.as_str())),
        );
        if en_fila {
            y = self.botones_en_fila(h, a, y, ahora) + 4.0;
        } else {
            y = self.botones_en_columna(h, a, y, ahora);
        }
        y
    }

    /// Cada botón mide lo que su texto (8 de relleno a los lados) y lo que sobra se reparte;
    /// el glifo va pegado al texto, a 8.
    fn botones_en_fila(&mut self, h: &mut Hoja, a: &Aviso, y: f64, ahora: Instant) -> f64 {
        let p = self.paleta;
        let (x0, x1) = (4.0, TARJETA - 4.0);
        let piezas: Vec<_> = a
            .acciones
            .iter()
            .map(|(_, etiqueta, g)| {
                let lg = g.map(|g| self.rotulos.linea(g, 14.0, 400.0));
                let lt = self.rotulos.linea(etiqueta, 14.0, 400.0);
                (lg, lt)
            })
            .collect();
        let anchos: Vec<f64> = piezas
            .iter()
            .map(|(lg, lt)| lg.as_ref().map_or(0.0, |g| self.ancho(g) + 8.0) + self.ancho(lt))
            .collect();
        let alto = 12.0
            + piezas
                .iter()
                .map(|(lg, lt)| {
                    lg.as_ref()
                        .map_or(0.0, |g| self.alto_de(g))
                        .max(self.alto_de(lt))
                })
                .fold(0.0, f64::max);
        let anchos: Vec<f64> = anchos.into_iter().map(f64::ceil).collect();
        let botones = repartir(&anchos, x1 - x0);
        h.trazos.push(Trazo::Caja {
            r: Rect::new(x0, y, x1, y + alto),
            radio: RADIO,
            color: op(&p.on_surface, 0.04),
        });
        let mut bx = x0;
        for (i, ((lg, lt), (w, bw))) in piezas
            .into_iter()
            .zip(anchos.into_iter().zip(botones))
            .enumerate()
        {
            let r = Rect::new(bx, y, bx + bw, y + alto);
            self.boton(h, a.id, i, r, ahora);
            let mut cx = bx + 8.0 + ((bw - 16.0 - w) / 2.0).floor();
            if let Some(g) = lg {
                let gw = self.ancho(&g);
                h.trazos.push(Trazo::Glifo {
                    c: Rect::new(cx, y + 6.0, cx + gw, y + alto - 6.0).center(),
                    l: g,
                    color: op(&p.on_surface, 0.7),
                });
                cx += gw + 8.0;
            }
            h.trazos.push(Trazo::Texto {
                y: y + 6.0 + ((alto - 12.0 - self.alto_de(&lt)) / 2.0).floor(),
                l: lt,
                x: cx,
                color: p.on_surface,
            });
            h.trazos.push(Trazo::Fin);
            bx += bw;
        }
        y + alto
    }

    /// A lo ancho de la tarjeta, sin relleno: con imagen, el glifo en su columna de 36 y el
    /// texto donde el título; sin imagen, el texto en el borde (o tras el glifo, a 8).
    fn botones_en_columna(&mut self, h: &mut Hoja, a: &Aviso, y0: f64, ahora: Instant) -> f64 {
        let p = self.paleta;
        let inicio = h.trazos.len();
        let mut y = y0;
        let con_imagen = a.imagen.is_some();
        for (i, (_, etiqueta, g)) in a.acciones.iter().enumerate() {
            let lg = g.map(|g| self.rotulos.linea(g, 14.0, 400.0));
            let tx = match (con_imagen, &lg) {
                (true, _) => RELLENO + columna(a).0,
                (false, Some(_)) => 4.0 + CUADRO + 8.0,
                (false, None) => 4.0,
            };
            let lt = self
                .rotulos
                .recortada(etiqueta, 14.0, (TARJETA - 4.0 - tx).max(0.0));
            let contenido = self
                .alto_de(&lt)
                .max(lg.as_ref().map_or(0.0, |g| self.alto_de(g)));
            let alto = 12.0 + contenido;
            let r = Rect::new(0.0, y, TARJETA, y + alto);
            self.boton(h, a.id, i, r, ahora);
            if let Some(g) = lg {
                h.trazos.push(Trazo::Glifo {
                    l: g,
                    c: Rect::new(4.0, y + 6.0, 4.0 + CUADRO, y + 6.0 + contenido).center(),
                    color: op(&p.on_surface, 0.7),
                });
            }
            h.trazos.push(Trazo::Texto {
                y: y + 6.0 + ((contenido - self.alto_de(&lt)) / 2.0).floor(),
                l: lt,
                x: tx,
                color: p.on_surface,
            });
            h.trazos.push(Trazo::Fin);
            y += alto;
        }
        h.trazos.insert(
            inicio,
            Trazo::Caja {
                r: Rect::new(0.0, y0, TARJETA, y),
                radio: RADIO,
                color: op(&p.on_surface, 0.04),
            },
        );
        y
    }

    /// El fondo de un botón (sobre el translúcido del toast, el resaltado del panel pesa el
    /// doble) y su escala; deja abierto el grupo que cierra quien lo llama.
    fn boton(&mut self, h: &mut Hoja, id: u32, i: usize, r: Rect, ahora: Instant) {
        let o = Objetivo::AccionAviso(id, i);
        let l = self.animos.luz(o, self.encima == Some(o), ahora);
        h.objetivo(o, r, false);
        h.trazos
            .push(Trazo::escalar(r, self.animos.escala(o, ahora)));
        h.trazos.push(Trazo::Caja {
            r,
            radio: RADIO,
            color: op(&self.paleta.primary, 0.28 * l),
        });
    }

    /// Las de abajo primero: donde se tocan dos glows queda encima el de la de arriba.
    pub fn pintar(&self, s: &mut Scene, escala: f64, bajada: f64) {
        s.reset();
        let base =
            Affine::scale(escala) * Affine::translate((0.0, -bajada.round() - self.scroll.round()));
        for c in self.cartas.iter().rev() {
            trazo::pintar(s, &c.pista, base, escala);
            let centro = c.r.center();
            let tr = base
                * Affine::translate((c.dx, 0.0))
                * Affine::translate(centro.to_vec2())
                * Affine::scale(c.escala)
                * Affine::translate(-centro.to_vec2());
            let capa = c.alfa < 1.0;
            if capa {
                s.push_layer(
                    Fill::NonZero,
                    BlendMode::default(),
                    c.alfa,
                    tr,
                    &c.r.inflate(16.0, 16.0),
                );
            }
            tarjeta(
                s,
                tr,
                &self.paleta,
                self.opacidad,
                self.borde,
                c.r,
                c.anillo,
            );
            trazo::pintar(
                s,
                &c.trazos,
                tr * Affine::translate((c.r.x0, c.r.y0)),
                escala,
            );
            if capa {
                s.pop_layer();
            }
        }
    }

    /// Donde responde la superficie: las tarjetas (con lo que crecen agrandadas), en lógicos.
    pub fn region(&self, bajada: f64) -> Vec<Rect> {
        let dy = -bajada.round() - self.scroll.round();
        self.cartas
            .iter()
            .map(|c| (c.r + Vec2::new(c.dx, dy)).inflate(4.0, 4.0))
            .collect()
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        self.rehacer || self.animos.anda(ahora) || self.deslices.values().any(|d| d.anda(ahora))
    }

    // ── El puntero ──

    fn en_columna(&self, x: f64, y: f64) -> Point {
        Point::new(x, y + self.scroll)
    }

    fn mirar(&mut self, x: f64, y: f64) {
        let p = self.en_columna(x, y);
        self.sobre = self.cartas.iter().find(|c| c.r.contains(p)).map(|c| c.id);
        self.encima = self
            .hoja
            .objetivos
            .iter()
            .rev()
            .find(|(_, r)| r.contains(p))
            .map(|(o, _)| *o);
    }

    /// El toast bajo el puntero, si cambió desde la última vez.
    pub fn encima_cambio(&mut self) -> Option<EventoToast> {
        (self.sobre != self.avisado).then(|| {
            self.avisado = self.sobre;
            EventoToast::Encima(self.sobre)
        })
    }

    pub fn mover(&mut self, x: f64, y: f64, t: u32) -> Vec<EventoToast> {
        let mut eventos = Vec::new();
        self.puntero = Some((x, y));
        if let Some(mut a) = self.arrastre {
            let p = self.en_columna(x, y);
            match a.seguir(p.x, p.y, t) {
                Paso::Nada => {}
                Paso::Corre { dx, recien, lado } => {
                    if recien && let Some(o) = self.apretado.take() {
                        self.animos.apretar(o, false, Instant::now());
                    }
                    self.deslices.insert(a.objetivo(), Desliz::quieto(dx));
                    if let Some(l) = lado {
                        self.de_costado(a.objetivo(), l, &mut eventos);
                    }
                }
            }
            self.arrastre = Some(a);
        }
        if !self.arrastre.is_some_and(|a| a.activo()) {
            self.mirar(x, y);
        }
        eventos
    }

    pub fn salir(&mut self) {
        self.puntero = None;
        self.encima = None;
        if !self.arrastre.is_some_and(|a| a.activo()) {
            self.sobre = None;
        }
    }

    /// La columna se fue: nada queda a medias para la próxima.
    pub fn vaciar(&mut self) {
        self.salir();
        self.sobre = None;
        self.arrastre = None;
        self.apretado = None;
        self.deslices.clear();
        self.scroll = 0.0;
    }

    pub fn apretar(&mut self, x: f64, y: f64, boton: u32, t: u32) {
        if boton != IZQUIERDO {
            return;
        }
        self.mirar(x, y);
        if let Some(o) = self.encima {
            self.apretado = Some(o);
            self.animos.apretar(o, true, Instant::now());
        }
        // Se arrastra desde cualquier lugar de la tarjeta, también desde un botón.
        if let Some(id) = self.sobre {
            let p = self.en_columna(x, y);
            self.arrastre = Some(Arrastre::nuevo(
                Objetivo::Aviso(id),
                p,
                TARJETA,
                (true, true),
                t,
            ));
        }
    }

    pub fn soltar(&mut self, x: f64, y: f64, boton: u32) -> Vec<EventoToast> {
        let mut eventos = Vec::new();
        let ahora = Instant::now();
        self.mirar(x, y);
        if boton != IZQUIERDO {
            // Medio y derecho: cerrarlo.
            if let Some(id) = self.sobre {
                eventos.push(EventoToast::Cerrar(id));
            }
            return eventos;
        }
        let apretado = self.apretado.take();
        if let Some(o) = apretado {
            self.animos.apretar(o, false, ahora);
        }
        if let Some(a) = self.arrastre.take()
            && a.activo()
        {
            let o = a.objetivo();
            let dx = self.deslices.get(&o).map_or(0.0, |d| d.dx(ahora));
            match a.tiron(dx) {
                Some(l) => self.de_costado(o, l, &mut eventos),
                None => self.volver(o),
            }
            return eventos;
        }
        match apretado.filter(|o| self.encima == Some(*o)) {
            Some(Objetivo::Aviso(id)) => eventos.push(EventoToast::Accion(id)),
            Some(Objetivo::AccionAviso(id, i)) => {
                if let Some((clave, _, _)) = self
                    .avisos
                    .iter()
                    .find(|a| a.id == id)
                    .and_then(|a| a.acciones.get(i))
                {
                    eventos.push(EventoToast::Boton(id, clave.clone()));
                }
            }
            _ => {}
        }
        eventos
    }

    pub fn rueda(&mut self, dy: f64, discreto: bool) {
        let paso = if discreto {
            // El paso de la rueda de un `GtkScrolledWindow`: la página a la 2/3.
            f64::from(self.tam.1).powf(2.0 / 3.0)
        } else {
            1.0
        };
        self.scroll = (self.scroll + dy * paso).clamp(0.0, self.max_scroll());
    }

    fn volver(&mut self, o: Objetivo) {
        let ahora = Instant::now();
        let desde = self.deslices.get(&o).map_or(0.0, |d| d.dx(ahora));
        self.deslices.insert(
            o,
            Desliz {
                desde,
                hasta: 0.0,
                t0: ahora,
                al_final: None,
            },
        );
    }

    /// A la derecha se va y, al llegar, se descarta; a la izquierda se copia y vuelve.
    fn de_costado(&mut self, o: Objetivo, lado: Lado, eventos: &mut Vec<EventoToast>) {
        let Objetivo::Aviso(id) = o else { return };
        match lado {
            Lado::Derecha => {
                let ahora = Instant::now();
                let desde = self.deslices.get(&o).map_or(0.0, |d| d.dx(ahora));
                self.deslices.insert(
                    o,
                    Desliz {
                        desde,
                        hasta: TARJETA,
                        t0: ahora,
                        al_final: Some(lado),
                    },
                );
            }
            Lado::Izquierda => {
                eventos.push(EventoToast::Copiar(id));
                self.volver(o);
            }
        }
    }

    /// Lo que termina solo con el tiempo: un toast que terminó de irse.
    pub fn al_cuadro(&mut self, ahora: Instant) -> Vec<EventoToast> {
        self.deslices
            .iter_mut()
            .filter(|(_, d)| !d.anda(ahora))
            .filter_map(|(o, d)| match (o, d.al_final.take()) {
                (Objetivo::Aviso(id), Some(_)) => Some(EventoToast::Cerrar(*id)),
                _ => None,
            })
            .collect()
    }
}

/// Lo que ocupa la imagen a la izquierda del texto (ancho hasta el texto, alto). Un ícono del
/// tema queda con el mismo aire hasta el borde de la tarjeta, arriba, abajo y hasta el texto
/// (10 px; Roberto, 2026-10-24): centrado en los 36 de alto, a 6 del relleno y a 10 del texto.
fn columna(a: &Aviso) -> (f64, f64) {
    match (&a.imagen, a.icono) {
        (None, _) => (0.0, 0.0),
        (Some(_), true) => ((CUADRO + ICONO) / 2.0 + AL_TEXTO, CUADRO),
        (Some(_), false) => (FOTO + 2.0 * MARGEN_FOTO, FOTO + 2.0 * MARGEN_FOTO),
    }
}

/// El ancho de cada botón de una fila de `total`: lo que mide su contenido (`anchos`) con 8
/// a cada lado, y lo que sobra repartido en enteros, el resto a los primeros (un `GtkBox` con
/// todos en `hexpand`).
fn repartir(anchos: &[f64], total: f64) -> Vec<f64> {
    let naturales: Vec<f64> = anchos.iter().map(|w| w + 16.0).collect();
    let n = naturales.len().max(1) as i64;
    let sobra = (total - naturales.iter().sum::<f64>()).max(0.0) as i64;
    naturales
        .iter()
        .enumerate()
        .map(|(i, w)| w + (sobra / n + i64::from((i as i64) < sobra % n)) as f64)
        .collect()
}

/// `img` achicada para entrar en un cuadrado de `lado`, centrada en `caja` (un `GtkImage` con
/// `-gtk-icon-size`).
fn encajar(img: &ImageData, caja: Rect, lado: f64) -> Rect {
    let (w, h) = (f64::from(img.width.max(1)), f64::from(img.height.max(1)));
    let k = lado / w.max(h);
    let (w, h) = ((w * k).round(), (h * k).round());
    let c = caja.center();
    let (x, y) = ((c.x - w / 2.0).round(), (c.y - h / 2.0).round());
    Rect::new(x, y, x + w, y + h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aviso(id: u32, acciones: &[&str]) -> Aviso {
        Aviso {
            id,
            titulo: "Título".into(),
            cuerpo: "Un cuerpo".into(),
            urgencia: Urgencia::Normal,
            llegada: 0,
            imagen: None,
            icono: false,
            chico: None,
            previa: None,
            progreso: None,
            acciones: acciones
                .iter()
                .map(|e| (e.to_string(), e.to_string(), None))
                .collect(),
        }
    }

    fn ui(avisos: Vec<Aviso>) -> ToastsUi {
        let mut ui = ToastsUi::new(Paleta::desde_css(""), 1.0, Borde::Brillo);
        ui.tam = (ANCHO, 900);
        ui.avisos = avisos;
        ui.armar(1.0, Instant::now());
        ui
    }

    #[test]
    fn columna_con_lo_nuevo_arriba_y_12_entre_dos() {
        let ui = ui(vec![aviso(2, &[]), aviso(1, &[])]);
        let (a, b) = (&ui.cartas[0], &ui.cartas[1]);
        assert_eq!((a.id, b.id), (2, 1));
        assert_eq!((a.r.x0, a.r.x1, a.r.y0), (16.0, 292.0, 6.0));
        assert_eq!(b.r.y0, a.r.y1 + 12.0);
        // La región de input son las tarjetas, no la columna entera.
        assert_eq!(ui.region(0.0).len(), 2);
    }

    #[test]
    fn el_click_hace_la_accion_y_el_boton_la_suya() {
        let mut ui = ui(vec![aviso(7, &["Abrir", "Ver"])]);
        let r = ui.cartas[0].r;
        ui.apretar(r.x0 + 20.0, r.y0 + 10.0, IZQUIERDO, 1);
        let e = ui.soltar(r.x0 + 20.0, r.y0 + 10.0, IZQUIERDO);
        assert!(matches!(e[..], [EventoToast::Accion(7)]));
        // Los dos botones entran en fila: el segundo, a la derecha, abajo.
        let (o, b) = ui.hoja.objetivos.last().copied().unwrap();
        assert_eq!(o, Objetivo::AccionAviso(7, 1));
        let c = b.center();
        ui.apretar(c.x, c.y, IZQUIERDO, 2);
        let e = ui.soltar(c.x, c.y, IZQUIERDO);
        assert!(matches!(&e[..], [EventoToast::Boton(7, k)] if k == "Ver"));
        // El derecho cierra.
        let e = ui.soltar(c.x, c.y, 0x111);
        assert!(matches!(e[..], [EventoToast::Cerrar(7)]));
    }

    #[test]
    fn el_icono_tiene_el_mismo_aire_por_los_cuatro_lados() {
        let mut a = aviso(5, &[]);
        a.imagen = Some(ImageData {
            data: morgan_motor::vello::peniko::Blob::new(std::sync::Arc::new(vec![
                0u8;
                4 * 24 * 24
            ])),
            format: morgan_motor::vello::peniko::ImageFormat::Rgba8,
            alpha_type: morgan_motor::vello::peniko::ImageAlphaType::Alpha,
            width: 24,
            height: 24,
        });
        a.icono = true;
        a.cuerpo.clear();
        let ui = ui(vec![a]);
        let c = &ui.cartas[0];
        let Some(Trazo::Imagen { r, .. }) =
            c.trazos.iter().find(|t| matches!(t, Trazo::Imagen { .. }))
        else {
            panic!("sin ícono");
        };
        let Some(Trazo::Texto { x, .. }) =
            c.trazos.iter().find(|t| matches!(t, Trazo::Texto { .. }))
        else {
            panic!("sin título");
        };
        assert_eq!(r.x0, 10.0, "al borde");
        assert_eq!(x - r.x1, 10.0, "al texto");
        assert_eq!(r.y0, 10.0, "arriba");
        assert_eq!(c.r.height() - r.y1, 10.0, "abajo");
    }

    #[test]
    fn la_fila_reparte_lo_que_sobra_en_enteros() {
        // «Abrir» y «Ver» a 14 px: 43 y 26 de texto en 268.
        assert_eq!(repartir(&[43.0, 26.0], 268.0), vec![143.0, 125.0]);
    }

    #[test]
    fn el_puntero_encima_se_avisa_una_vez() {
        let mut ui = ui(vec![aviso(3, &[])]);
        let c = ui.cartas[0].r.center();
        ui.mover(c.x, c.y, 1);
        assert!(matches!(
            ui.encima_cambio(),
            Some(EventoToast::Encima(Some(3)))
        ));
        ui.mover(c.x + 1.0, c.y, 2);
        assert!(ui.encima_cambio().is_none());
        ui.salir();
        assert!(matches!(
            ui.encima_cambio(),
            Some(EventoToast::Encima(None))
        ));
    }

    #[test]
    fn a_la_izquierda_copia_y_a_la_derecha_se_va() {
        let mut ui = ui(vec![aviso(4, &[])]);
        let c = ui.cartas[0].r.center();
        ui.apretar(c.x, c.y, IZQUIERDO, 1);
        ui.mover(c.x - 30.0, c.y, 2);
        let e = ui.mover(c.x - 200.0, c.y, 3);
        assert!(matches!(e[..], [EventoToast::Copiar(4)]));
        ui.soltar(c.x - 200.0, c.y, IZQUIERDO);
        ui.apretar(c.x, c.y, IZQUIERDO, 10);
        ui.mover(c.x + 30.0, c.y, 11);
        assert!(
            ui.mover(c.x + 200.0, c.y, 12).is_empty(),
            "se descarta al llegar"
        );
        let despues = Instant::now() + std::time::Duration::from_secs(1);
        assert!(matches!(
            ui.al_cuadro(despues)[..],
            [EventoToast::Cerrar(4)]
        ));
    }
}
