//! El lienzo de ventanas con las piezas del centro: una banda de borde a borde, centrada en la
//! salida, con el pie (qué ventana es la elegida), los números de workspace y, por cada uno,
//! una franja con su cinta de scrolling entera y la miniatura de cada ventana. Qué ventanas hay,
//! las fotos y enfocar lo lleva el hilo de GTK (`ventanas/`); acá viven la elegida, el
//! desplazamiento, el anillo que se desliza y el puntero.
//!
//! Las medidas, de la versión con GTK (`.ventanas` en `estilo.css` hasta 0.1.1): 14 de relleno
//! arriba y abajo, 8 entre el pie y las franjas, el pie con 24 a los costados, la celda con 3 de
//! relleno y 6 de radio (4 la foto), el anillo de 3 con el glow de 8 y spread 1.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use morgan_motor::color::{Paleta, con_alfa};
use morgan_motor::curva;
use morgan_motor::forma::Borde;
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Rect};
use morgan_motor::vello::peniko::ImageData;
use smithay_client_toolkit::seat::keyboard::Keysym;

use super::trazo::{self, Trazo};
use super::{Animos, Objetivo, Rotulos};
use crate::centro::vista::{EventoVentanas, VistaVentanas};
use crate::ventanas::modelo::{self, Dir, Lienzo};

const SEPARACION: i32 = 8;
const MARGEN_GLOW: f64 = 14.0;
const COLUMNA: f64 = 36.0;
const COSTADO: f64 = 24.0;
const RELLENO: f64 = 14.0;
const PIE_X: f64 = 24.0;
const ENTRE_PIE: f64 = 10.0;
const PX: f32 = 14.0;
const PX_DETALLE: f32 = 12.0;
const ICONO_PIE: f64 = 20.0;
const ICONO_GRANDE: f64 = 48.0;
const RADIO_CELDA: f64 = 6.0;
const RADIO_FOTO: f64 = 4.0;
const RELLENO_CELDA: f64 = 3.0;
const ANILLO: f64 = 3.0;
const SCROLL: Duration = Duration::from_millis(220);
const DESLIZAR: Duration = Duration::from_millis(160);
const FUNDIDO: Duration = Duration::from_millis(180);
/// La banda sube estos px al entrar, mientras Hyprland funde la capa.
const SUBIDA: f64 = 10.0;
const ENTRADA: Duration = Duration::from_millis(180);

/// La foto de una ventana: la que se ve y la anterior, de la que se funde.
#[derive(Default)]
struct Pagina {
    antes: Option<ImageData>,
    ahora: Option<ImageData>,
    desde: Option<Instant>,
}

struct Estado {
    lienzo: Lienzo,
    iconos: Vec<ImageData>,
    paginas: Vec<Pagina>,
    fondo: Option<ImageData>,
    sel: usize,
    /// Lo que se dibujó en el último cuadro: de ahí parten el desplazamiento y el anillo al
    /// cambiar la elegida.
    scroll_visto: Option<f64>,
    scroll_desde: Option<(f64, Instant)>,
    anillo_visto: Option<[f64; 4]>,
    anillo_desde: Option<([f64; 4], Instant)>,
    inicio: Option<Instant>,
    hover: Option<usize>,
    apretado: Option<usize>,
    afuera: bool,
    puntero: Option<(f64, f64)>,
    /// Lo de este cuadro, para responder al puntero.
    celdas: Vec<(usize, Rect)>,
    tarjeta: Rect,
    trazos: Vec<Trazo>,
}

pub struct VentanasUi {
    pub paleta: Paleta,
    pub opacidad: f32,
    pub borde: Borde,
    pub tam: (u32, u32),
    rotulos: Rotulos,
    animos: Animos,
    abierta: Option<Estado>,
}

fn suave(desde: f64, hasta: f64, t0: Instant, dur: Duration, ahora: Instant) -> f64 {
    let p = (ahora.saturating_duration_since(t0).as_secs_f64()
        / crate::animaciones::dur(dur).as_secs_f64().max(1e-6))
    .clamp(0.0, 1.0);
    desde + (hasta - desde) * (1.0 - (1.0 - p).powi(3))
}

fn hacia(desde: [f64; 4], hasta: [f64; 4], t0: Instant, ahora: Instant) -> [f64; 4] {
    std::array::from_fn(|i| suave(desde[i], hasta[i], t0, DESLIZAR, ahora))
}

/// El rectángulo con el que una imagen cubre `r` sin deformarse (`ContentFit::Cover`).
fn cubrir(img: &ImageData, r: Rect) -> Rect {
    let (iw, ih) = (f64::from(img.width.max(1)), f64::from(img.height.max(1)));
    let k = (r.width() / iw).max(r.height() / ih);
    let c = r.center();
    Rect::new(
        c.x - iw * k / 2.0,
        c.y - ih * k / 2.0,
        c.x + iw * k / 2.0,
        c.y + ih * k / 2.0,
    )
}

fn centrado(c: morgan_motor::vello::kurbo::Point, lado: f64) -> Rect {
    Rect::new(c.x - lado / 2.0, c.y - lado / 2.0, c.x + lado / 2.0, c.y + lado / 2.0)
}

impl VentanasUi {
    pub fn new(paleta: Paleta, opacidad: f32, borde: Borde) -> Self {
        VentanasUi {
            paleta,
            opacidad,
            borde,
            tam: (0, 0),
            rotulos: Rotulos::new(),
            animos: Animos::default(),
            abierta: None,
        }
    }

    pub fn abrir(&mut self, v: VistaVentanas) {
        self.cerrar();
        let paginas = v
            .fotos
            .into_iter()
            .map(|f| Pagina {
                antes: None,
                ahora: f,
                desde: None,
            })
            .collect();
        self.abierta = Some(Estado {
            lienzo: v.lienzo,
            iconos: v.iconos,
            paginas,
            fondo: v.fondo,
            sel: v.sel,
            scroll_visto: None,
            scroll_desde: None,
            anillo_visto: None,
            anillo_desde: None,
            inicio: None,
            hover: None,
            apretado: None,
            afuera: false,
            puntero: None,
            celdas: Vec::new(),
            tarjeta: Rect::ZERO,
            trazos: Vec::new(),
        });
    }

    pub fn cerrar(&mut self) {
        if self.abierta.take().is_some() {
            self.animos = Animos::default();
        }
    }

    pub fn abierta(&self) -> bool {
        self.abierta.is_some()
    }

    /// Llegó la foto de una ventana: entra fundiéndose.
    pub fn foto(&mut self, direccion: &str, img: ImageData) {
        let Some(e) = &mut self.abierta else { return };
        let Some(i) = e.lienzo.ventanas.iter().position(|v| v.direccion == direccion) else {
            return;
        };
        let p = &mut e.paginas[i];
        p.antes = p.ahora.take();
        p.ahora = Some(img);
        p.desde = Some(Instant::now());
    }

    pub fn fondo(&mut self, img: ImageData) {
        if let Some(e) = &mut self.abierta {
            e.fondo = Some(img);
        }
    }

    // ── Elegida ──

    fn seleccionar(&mut self, i: usize, animar: bool) {
        let Some(e) = &mut self.abierta else { return };
        e.sel = i;
        let ahora = Instant::now();
        e.scroll_desde = animar.then_some(e.scroll_visto.map(|s| (s, ahora))).flatten();
        e.anillo_desde = animar.then_some(e.anillo_visto.map(|a| (a, ahora))).flatten();
        if !animar {
            e.scroll_visto = None;
            e.anillo_visto = None;
        }
    }

    pub fn reciente(&mut self) {
        let Some(e) = &self.abierta else { return };
        let i = e.lienzo.reciente(e.sel);
        self.seleccionar(i, true);
    }

    fn ir(&mut self, dir: Dir) {
        let Some(e) = &self.abierta else { return };
        let i = e.lienzo.vecina(e.sel, dir);
        self.seleccionar(i, true);
    }

    /// Enfocar la elegida; `submap` si Hyprland sigue en el submap del lienzo.
    pub fn confirmar(&self, submap: bool) -> Vec<EventoVentanas> {
        let Some(e) = &self.abierta else {
            return Vec::new();
        };
        vec![EventoVentanas::Enfocar {
            direccion: e.lienzo.ventanas[e.sel].direccion.clone(),
            submap,
        }]
    }

    // ── Cuadro ──

    fn escala(&self) -> f64 {
        f64::from(self.rotulos.escala())
    }

    fn ancho(&self, l: &morgan_motor::parley::Layout<u8>) -> f64 {
        morgan_motor::texto::caja(l).0 / self.escala()
    }

    fn alto_linea(&self, l: &morgan_motor::parley::Layout<u8>) -> f64 {
        morgan_motor::texto::alto_de_linea(l) / self.escala()
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        let Some(e) = &self.abierta else { return false };
        let en = |t0: Instant, d: Duration| {
            ahora.saturating_duration_since(t0) < crate::animaciones::dur(d)
        };
        self.animos.anda(ahora)
            || e.inicio.is_none_or(|t| en(t, ENTRADA))
            || e.scroll_desde.is_some_and(|(_, t)| en(t, SCROLL))
            || e.anillo_desde.is_some_and(|(_, t)| en(t, DESLIZAR))
            || e.paginas
                .iter()
                .any(|p| p.desde.is_some_and(|t| en(t, FUNDIDO)))
    }

    pub fn armar(&mut self, escala: f32, ahora: Instant) {
        self.rotulos.poner_escala(escala);
        let Some(mut e) = self.abierta.take() else { return };
        let p = self.paleta;
        let (w, h) = (f64::from(self.tam.0), f64::from(self.tam.1));
        let inicio = *e.inicio.get_or_insert(ahora);
        let l = &e.lienzo;

        // Tope de pantalla: el 85 % del alto, descontando el pie, los rellenos de la banda, la
        // separación y el aire del glow.
        let alto_pantalla = h as i32;
        let alto_cuerpo = alto_pantalla * 85 / 100 - 30 - 28 - SEPARACION - 2 * MARGEN_GLOW as i32;
        let m = modelo::medidas(
            l.franjas.len(),
            l.alto_monitor(),
            alto_cuerpo,
            SEPARACION,
            alto_pantalla / 5,
        );
        let (min, max) = l.extension();
        let ancho_lienzo = (f64::from(max - min) * m.escala).ceil() + 2.0 * MARGEN_GLOW;
        let n = l.franjas.len() as f64;
        let alto_franja = f64::from(m.alto_franja);
        let sep = f64::from(SEPARACION);
        let alto_lienzo = n * (alto_franja + sep) - sep + 2.0 * MARGEN_GLOW;
        let ancho_visible = ancho_lienzo.min(w - 2.0 * COSTADO - COLUMNA - sep);

        // El pie: el prompt, el ícono, el título (lo que sobra) y el detalle a la derecha.
        let v = &l.ventanas[e.sel];
        let prompt = self.rotulos.linea("󰖯  Ventanas", PX, 400.0);
        let detalle = self
            .rotulos
            .linea(&format!("{} · {}", v.clase, v.ws), PX_DETALLE, 400.0);
        let alto_pie = self
            .alto_linea(&prompt)
            .max(self.alto_linea(&detalle))
            .max(ICONO_PIE);
        let alto_tarjeta = RELLENO + alto_pie + sep + alto_lienzo + RELLENO;
        let y0 = ((h - alto_tarjeta) / 2.0).floor();
        let tarjeta = Rect::new(0.0, y0, w, y0 + alto_tarjeta);
        let subida = SUBIDA
            * (1.0
                - f64::from(curva::bezier(
                    0.05,
                    0.7,
                    0.1,
                    1.0,
                    (ahora.saturating_duration_since(inicio).as_secs_f64()
                        / crate::animaciones::dur(ENTRADA).as_secs_f64().max(1e-6))
                    .min(1.0) as f32,
                )));

        let mut t = vec![Trazo::mover(0.0, subida)];
        t.push(Trazo::Caja {
            r: tarjeta,
            radio: 0.0,
            color: con_alfa(&p.surface, p.surface[3] * self.opacidad),
        });
        let centro_pie = y0 + RELLENO + alto_pie / 2.0;
        let (wp, wd) = (self.ancho(&prompt), self.ancho(&detalle));
        let (ap, ad) = (self.alto_linea(&prompt), self.alto_linea(&detalle));
        t.push(Trazo::Texto {
            l: prompt,
            x: PIE_X,
            y: centro_pie - ap / 2.0,
            color: p.primary,
        });
        let x_icono = PIE_X + wp + ENTRE_PIE;
        t.push(Trazo::Imagen {
            img: e.iconos[e.sel].clone(),
            r: Rect::new(
                x_icono,
                centro_pie - ICONO_PIE / 2.0,
                x_icono + ICONO_PIE,
                centro_pie + ICONO_PIE / 2.0,
            ),
            radio: 0.0,
            alfa: 1.0,
        });
        let x_detalle = w - PIE_X - wd;
        t.push(Trazo::Texto {
            l: detalle,
            x: x_detalle,
            y: centro_pie - ad / 2.0,
            color: p.on_surface_variant,
        });
        let x_titulo = x_icono + ICONO_PIE + ENTRE_PIE;
        let titulo = self.rotulos.recortada(
            &v.titulo,
            PX,
            (x_detalle - ENTRE_PIE - x_titulo).max(0.0),
        );
        let at = self.alto_linea(&titulo);
        t.push(Trazo::Texto {
            l: titulo,
            x: x_titulo,
            y: centro_pie - at / 2.0,
            color: p.on_surface,
        });

        // El cuerpo: los números de workspace y, al lado, lo que se desplaza.
        let ancho_cuerpo = COLUMNA + sep + ancho_visible;
        let x_cuerpo = ((w - ancho_cuerpo) / 2.0).floor();
        let y_cuerpo = y0 + RELLENO + alto_pie + sep;
        let viewport = Rect::new(
            x_cuerpo + COLUMNA + sep,
            y_cuerpo,
            x_cuerpo + COLUMNA + sep + ancho_visible,
            y_cuerpo + alto_lienzo,
        );
        for (i, f) in l.franjas.iter().enumerate() {
            let y = y_cuerpo + MARGEN_GLOW + i as f64 * (alto_franja + sep);
            let r = self.rotulos.linea(&f.ws.to_string(), PX, 400.0);
            let wr = self.ancho(&r);
            t.push(Trazo::Texto {
                l: r,
                x: x_cuerpo + (COLUMNA - wr) / 2.0,
                y,
                color: if Some(f.ws) == l.ws_actual {
                    p.primary
                } else {
                    p.on_surface_variant
                },
            });
        }

        // Lo que se desplaza: la elegida, centrada en lo que se ve (si el lienzo no entra).
        let a_px = |x: i32| MARGEN_GLOW + f64::from(x - min) * m.escala;
        let centro = |i: usize| {
            let r = l.ventanas[i].rect;
            MARGEN_GLOW + f64::from(r.x - min + r.w / 2) * m.escala
        };
        let tope = (ancho_lienzo - ancho_visible).max(0.0);
        let objetivo = (centro(e.sel) - ancho_visible / 2.0).clamp(0.0, tope);
        let scroll = match e.scroll_desde {
            Some((desde, t0)) => suave(desde.clamp(0.0, tope), objetivo, t0, SCROLL, ahora),
            None => objetivo,
        };
        e.scroll_visto = Some(scroll);

        let franja_y = |i: usize| MARGEN_GLOW + i as f64 * (alto_franja + sep);
        let mut cuerpo = Vec::new();
        let mut celdas = Vec::new();
        let mut vivos = HashSet::new();
        let mut rect_elegida = [0.0; 4];
        for (n, f) in l.franjas.iter().enumerate() {
            let y = franja_y(n);
            let pantalla = l.ventanas[f.ventanas[0]].monitor;
            let marco = Rect::new(
                a_px(pantalla.x),
                y,
                a_px(pantalla.x) + (f64::from(pantalla.w) * m.escala).round(),
                y + alto_franja,
            );
            cuerpo.push(Trazo::Caja {
                r: marco,
                radio: 0.0,
                color: con_alfa(&p.on_surface, p.on_surface[3] * 0.06),
            });
            if let Some(fondo) = &e.fondo {
                cuerpo.push(Trazo::Recorte {
                    r: marco,
                    radio: 0.0,
                });
                cuerpo.push(Trazo::Imagen {
                    img: fondo.clone(),
                    r: cubrir(fondo, marco),
                    radio: 0.0,
                    alfa: 0.55,
                });
                cuerpo.push(Trazo::Fin);
            }
            for &i in &f.ventanas {
                let v = &l.ventanas[i];
                let x = a_px(v.rect.x);
                let yv = y + f64::from(v.rect.y) * m.escala;
                let ancho = (f64::from(v.rect.w) * m.escala).round().max(8.0);
                let alto = (f64::from(v.rect.h) * m.escala).round().max(8.0);
                let r = Rect::new(x, yv, x + ancho, yv + alto);
                if i == e.sel {
                    rect_elegida = [r.x0, r.y0, r.width(), r.height()];
                }
                let o = Objetivo::Ventana(i);
                vivos.insert(o);
                let luz = self.animos.luz(o, e.hover == Some(i), ahora);
                celdas.push((i, r));
                // Las que no se ven en su monitor, tenues; la elegida y la de abajo del
                // puntero, enteras.
                let alfa = if v.capturable() || i == e.sel {
                    1.0
                } else {
                    0.5 + 0.5 * luz
                };
                cuerpo.push(Trazo::Grupo {
                    r,
                    escala: 1.0,
                    dx: 0.0,
                    dy: 0.0,
                    alfa,
                });
                if luz > 0.0 {
                    cuerpo.push(Trazo::Caja {
                        r,
                        radio: RADIO_CELDA,
                        color: con_alfa(&p.primary, p.primary[3] * 0.5 * luz),
                    });
                }
                let dentro = r.inset(-RELLENO_CELDA);
                cuerpo.push(Trazo::Caja {
                    r: dentro,
                    radio: RADIO_FOTO,
                    color: con_alfa(&p.on_surface, p.on_surface[3] * 0.08),
                });
                cuerpo.push(Trazo::Recorte {
                    r: dentro,
                    radio: RADIO_FOTO,
                });
                let pag = &e.paginas[i];
                let t_foto = pag.desde.map_or(1.0, |t0| {
                    (ahora.saturating_duration_since(t0).as_secs_f32()
                        / crate::animaciones::dur(FUNDIDO).as_secs_f32().max(1e-6))
                    .min(1.0)
                });
                let icono = Trazo::Imagen {
                    img: e.iconos[i].clone(),
                    r: centrado(dentro.center(), ICONO_GRANDE),
                    radio: 0.0,
                    alfa: 0.8,
                };
                match (&pag.antes, &pag.ahora) {
                    (_, None) => cuerpo.push(icono),
                    (antes, Some(foto)) => {
                        // Lo de antes (la foto vieja o el ícono) se ve hasta que la nueva entra.
                        match antes {
                            Some(a) => cuerpo.push(Trazo::Imagen {
                                img: a.clone(),
                                r: cubrir(a, dentro),
                                radio: 0.0,
                                alfa: 1.0,
                            }),
                            None if t_foto < 1.0 => cuerpo.push(icono),
                            None => {}
                        }
                        cuerpo.push(Trazo::Imagen {
                            img: foto.clone(),
                            r: cubrir(foto, dentro),
                            radio: 0.0,
                            alfa: t_foto,
                        });
                    }
                }
                cuerpo.push(Trazo::Fin);
                // La insignia: el ícono de la app, abajo a la izquierda.
                let (xi, yi) = (dentro.x0 + 8.0, dentro.y1 - 8.0 - ICONO_PIE);
                cuerpo.push(Trazo::Imagen {
                    img: e.iconos[i].clone(),
                    r: Rect::new(xi, yi, xi + ICONO_PIE, yi + ICONO_PIE),
                    radio: 0.0,
                    alfa: 1.0,
                });
                cuerpo.push(Trazo::Fin);
            }
        }
        // Encima de todo, el anillo de la elegida, que se desliza de una a otra.
        let anillo = match e.anillo_desde {
            Some((desde, t0)) => hacia(desde, rect_elegida, t0, ahora),
            None => rect_elegida,
        };
        e.anillo_visto = Some(anillo);
        let ra = Rect::new(anillo[0], anillo[1], anillo[0] + anillo[2], anillo[1] + anillo[3]);
        cuerpo.push(Trazo::Brillo {
            r: ra.inflate(1.0, 1.0),
            radio: RADIO_CELDA + 1.0,
            color: con_alfa(&p.primary, p.primary[3] * 0.45),
            d: (0.0, 0.0),
            blur: 8.0,
        });
        cuerpo.push(Trazo::Marco {
            r: ra,
            radio: RADIO_CELDA,
            ancho: ANILLO,
            a: p.primary,
            b: p.tertiary,
        });
        self.animos.retener(&vivos);

        t.push(Trazo::Recorte {
            r: viewport,
            radio: 0.0,
        });
        t.push(Trazo::mover(viewport.x0 - scroll.round(), viewport.y0));
        t.extend(cuerpo);
        t.push(Trazo::Fin);
        t.push(Trazo::Fin);
        t.push(Trazo::Fin);

        // Para el puntero: las celdas donde se ven, sin el corrimiento de la entrada.
        e.celdas = celdas
            .into_iter()
            .filter_map(|(i, r)| {
                let r = (r + morgan_motor::vello::kurbo::Vec2::new(
                    viewport.x0 - scroll.round(),
                    viewport.y0,
                ))
                .intersect(viewport);
                (r.width() > 0.0 && r.height() > 0.0).then_some((i, r))
            })
            .collect();
        e.tarjeta = tarjeta;
        e.trazos = t;
        if let Some((x, y)) = e.puntero {
            e.hover = e.celdas.iter().find(|(_, r)| r.contains((x, y))).map(|(i, _)| *i);
        }
        self.abierta = Some(e);
        self.rotulos.barrer();
    }

    pub fn pintar(&self, s: &mut Scene, escala: f64) {
        s.reset();
        if let Some(e) = &self.abierta {
            trazo::pintar(s, &e.trazos, Affine::scale(escala), escala);
        }
    }

    // ── Teclado ──

    pub fn tecla(&mut self, k: Keysym) -> Vec<EventoVentanas> {
        if self.abierta.is_none() {
            return Vec::new();
        }
        match k {
            Keysym::Escape => return vec![EventoVentanas::Cerrada],
            Keysym::Return | Keysym::KP_Enter | Keysym::space => return self.confirmar(true),
            Keysym::Tab => self.reciente(),
            Keysym::Left | Keysym::KP_Left | Keysym::h => self.ir(Dir::Izquierda),
            Keysym::Right | Keysym::KP_Right | Keysym::l => self.ir(Dir::Derecha),
            Keysym::Up | Keysym::KP_Up | Keysym::k => self.ir(Dir::Arriba),
            Keysym::Down | Keysym::KP_Down | Keysym::j => self.ir(Dir::Abajo),
            _ => {}
        }
        Vec::new()
    }

    /// Se soltó `Mod`: se enfoca la elegida.
    pub fn soltada(&self, k: Keysym) -> Vec<EventoVentanas> {
        if matches!(k, Keysym::Super_L | Keysym::Super_R) {
            self.confirmar(true)
        } else {
            Vec::new()
        }
    }

    // ── Puntero ──

    pub fn mover(&mut self, x: f64, y: f64) {
        if let Some(e) = &mut self.abierta {
            e.puntero = Some((x, y));
            e.hover = e.celdas.iter().find(|(_, r)| r.contains((x, y))).map(|(i, _)| *i);
        }
    }

    pub fn salir(&mut self) {
        if let Some(e) = &mut self.abierta {
            e.puntero = None;
            e.hover = None;
        }
    }

    pub fn apretar(&mut self, x: f64, y: f64) {
        self.mover(x, y);
        if let Some(e) = &mut self.abierta {
            e.apretado = e.hover;
            e.afuera = !e.tarjeta.contains((x, y));
        }
    }

    /// Un click en una ventana la enfoca; uno afuera de la banda cierra.
    pub fn soltar(&mut self, x: f64, y: f64) -> Vec<EventoVentanas> {
        self.mover(x, y);
        let Some(e) = &mut self.abierta else {
            return Vec::new();
        };
        let apretado = e.apretado.take();
        if apretado.is_none() && e.afuera && !e.tarjeta.contains((x, y)) {
            return vec![EventoVentanas::Cerrada];
        }
        match apretado {
            Some(i) if e.hover == Some(i) => {
                self.seleccionar(i, false);
                self.confirmar(true)
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ventanas::modelo::{Franja, Rect as R, Ventana};

    fn ventana(dir: &str, x: i32) -> Ventana {
        Ventana {
            direccion: dir.into(),
            clase: "kitty".into(),
            titulo: dir.into(),
            ws: 1,
            rect: R { x, y: 0, w: 100, h: 100 },
            monitor: R { x: 0, y: 0, w: 1000, h: 1000 },
            historia: 0,
        }
    }

    fn abierta() -> VentanasUi {
        let mut ui = VentanasUi::new(Paleta::desde_css(""), 1.0, Borde::Brillo);
        let lienzo = Lienzo {
            ventanas: vec![ventana("0xa", 0), ventana("0xb", 200), ventana("0xc", 400)],
            franjas: vec![Franja {
                ws: 1,
                ventanas: vec![0, 1, 2],
            }],
            recientes: vec![0, 1, 2],
            ws_actual: Some(1),
        };
        let icono = crate::icono::de_pixeles(vec![0; 4], 1, 1);
        ui.abrir(VistaVentanas {
            salida: None,
            lienzo,
            sel: 1,
            iconos: vec![icono; 3],
            fotos: vec![None, None, None],
            fondo: None,
        });
        ui
    }

    fn sel(ui: &VentanasUi) -> usize {
        ui.abierta.as_ref().unwrap().sel
    }

    #[test]
    fn tab_va_por_uso_reciente() {
        let mut ui = abierta();
        ui.tecla(Keysym::Tab);
        assert_eq!(sel(&ui), 2);
        ui.tecla(Keysym::Tab);
        assert_eq!(sel(&ui), 0);
    }

    #[test]
    fn las_flechas_van_por_el_espacio() {
        let mut ui = abierta();
        ui.tecla(Keysym::Right);
        assert_eq!(sel(&ui), 2);
        ui.tecla(Keysym::h);
        assert_eq!(sel(&ui), 1);
        ui.tecla(Keysym::Left);
        ui.tecla(Keysym::Left);
        assert_eq!(sel(&ui), 0);
    }

    #[test]
    fn enter_y_soltar_el_mod_enfocan_la_elegida() {
        let mut ui = abierta();
        let v = ui.tecla(Keysym::Return);
        assert!(matches!(
            &v[..],
            [EventoVentanas::Enfocar { direccion, submap: true }] if direccion == "0xb"
        ));
        assert!(ui.soltada(Keysym::a).is_empty());
        assert_eq!(ui.soltada(Keysym::Super_L).len(), 1);
        assert!(matches!(
            &ui.tecla(Keysym::Escape)[..],
            [EventoVentanas::Cerrada]
        ));
    }

    #[test]
    fn la_foto_nueva_llega_por_direccion() {
        let mut ui = abierta();
        let img = crate::icono::de_pixeles(vec![0; 4], 1, 1);
        ui.foto("0xc", img.clone());
        ui.foto("0xzz", img);
        let e = ui.abierta.as_ref().unwrap();
        assert!(e.paginas[2].ahora.is_some());
        assert!(e.paginas[0].ahora.is_none());
    }
}
