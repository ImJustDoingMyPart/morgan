//! Lo que se dibuja, ya ubicado y con su color: la hoja se rearma en cada cuadro como una lista
//! de trazos, y pintarla es recorrerla. Las medidas van en lógicos; el texto se ubica en
//! píxeles enteros del buffer, como una etiqueta de GTK.

use std::rc::Rc;

use morgan_motor::color::{Color, pincel};
use morgan_motor::forma::{self, degrade_135, degrade_horizontal, sombra};
use morgan_motor::parley::Layout;
use morgan_motor::texto;
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Point, Rect, RoundedRect};
use morgan_motor::vello::peniko::{BlendMode, Fill, ImageData};

pub enum Degrade {
    /// `linear-gradient(135deg, …)`: los cuadritos encendidos.
    Diagonal,
    /// `linear-gradient(to right, …)` sobre `de`: los sliders y las barras, que muestran su
    /// tramo de un degradé más largo.
    Horizontal { de: Rect },
}

pub enum Trazo {
    Caja {
        r: Rect,
        radio: f64,
        color: Color,
    },
    Degrade {
        r: Rect,
        radio: f64,
        a: Color,
        b: Color,
        forma: Degrade,
    },
    /// Un borde de `ancho` adentro de `r` (`border: 1px solid`).
    Borde {
        r: Rect,
        radio: f64,
        ancho: f64,
        color: Color,
    },
    /// `box-shadow` sin desplazamiento.
    Sombra {
        r: Rect,
        radio: f64,
        color: Color,
        spread: f64,
        blur: f64,
    },
    /// Arriba a la izquierda en `(x, y)`.
    Texto {
        l: Rc<Layout<u8>>,
        x: f64,
        y: f64,
        color: Color,
    },
    /// Centrado por su tinta en `c`.
    Glifo {
        l: Rc<Layout<u8>>,
        c: Point,
        color: Color,
    },
    Imagen {
        img: ImageData,
        r: Rect,
        radio: f64,
        alfa: f32,
    },
    /// Un ícono simbólico de `color`.
    Mascara {
        img: ImageData,
        r: Rect,
        color: Color,
    },
    /// Lo que sigue, hasta `Fin`, recortado a `r`.
    Recorte { r: Rect, radio: f64 },
    /// Lo que sigue, hasta `Fin`, achicado (o agrandado) hacia el centro de `r`, corrido y con
    /// opacidad.
    Grupo {
        r: Rect,
        escala: f64,
        dx: f64,
        dy: f64,
        alfa: f32,
    },
    /// Lo que sigue, hasta `Fin`, escalado desde la esquina de lo de arriba.
    Escala { x: f64, y: f64 },
    Fin,
}

impl Trazo {
    /// Corre lo que sigue, hasta `Fin`.
    pub fn mover(dx: f64, dy: f64) -> Trazo {
        Trazo::Grupo {
            r: Rect::ZERO,
            escala: 1.0,
            dx,
            dy,
            alfa: 1.0,
        }
    }

    /// Achica lo que sigue, hasta `Fin`, hacia el centro de `r`.
    pub fn escalar(r: Rect, escala: f64) -> Trazo {
        Trazo::Grupo {
            r,
            escala,
            dx: 0.0,
            dy: 0.0,
            alfa: 1.0,
        }
    }
}

/// Cómo se apila lo abierto al pintar: una transformación y si abrió una capa de vello.
struct Nivel {
    tr: Affine,
    capa: bool,
}

/// Pinta `trazos` con `base` (lógicos a píxeles del buffer, con el desplazamiento del panel).
pub fn pintar(s: &mut Scene, trazos: &[Trazo], base: Affine, escala: f64) {
    let mut pila = vec![Nivel {
        tr: base,
        capa: false,
    }];
    let tr = |pila: &Vec<Nivel>| pila.last().map_or(base, |n| n.tr);
    for t in trazos {
        let actual = tr(&pila);
        match t {
            Trazo::Caja { r, radio, color } => {
                if color[3] > 0.0 {
                    s.fill(
                        Fill::NonZero,
                        actual,
                        pincel(color),
                        None,
                        &RoundedRect::from_rect(*r, *radio),
                    );
                }
            }
            Trazo::Degrade {
                r,
                radio,
                a,
                b,
                forma,
            } => {
                let g = match forma {
                    Degrade::Diagonal => degrade_135(*r, *a, *b),
                    Degrade::Horizontal { de } => degrade_horizontal(*de, *a, *b),
                };
                s.fill(
                    Fill::NonZero,
                    actual,
                    &g,
                    None,
                    &RoundedRect::from_rect(*r, *radio),
                );
            }
            Trazo::Borde {
                r,
                radio,
                ancho,
                color,
            } => {
                let m = ancho / 2.0;
                s.stroke(
                    &morgan_motor::vello::kurbo::Stroke::new(*ancho),
                    actual,
                    pincel(color),
                    None,
                    &RoundedRect::from_rect(r.inset(-m), (radio - m).max(0.0)),
                );
            }
            Trazo::Sombra {
                r,
                radio,
                color,
                spread,
                blur,
            } => {
                if color[3] > 0.0 {
                    sombra(s, actual, *r, *radio, *color, (0.0, 0.0), *spread, *blur);
                }
            }
            Trazo::Texto { l, x, y, color } => {
                let p = actual * Point::new(*x, *y);
                let pos = Affine::translate((p.x.round(), p.y.round()));
                // Lo que queda de la transformación (una escala por apretar) se aplica al texto
                // alrededor de su esquina.
                let lineal = sin_traslacion(actual, escala);
                texto::dibujar_en(s, l, pos * lineal, |_| *color, 1.0);
            }
            Trazo::Glifo { l, c, color } => {
                let p = actual * *c;
                let lineal = sin_traslacion(actual, escala);
                let (x, y) = texto::esquina_por_tinta(l, 0.0, 0.0);
                let pos = Affine::translate((p.x.round(), p.y.round()));
                texto::dibujar_en(
                    s,
                    l,
                    pos * lineal * Affine::translate((x, y)),
                    |_| *color,
                    1.0,
                );
            }
            Trazo::Imagen {
                img,
                r,
                radio,
                alfa,
            } => {
                if *radio > 0.0 {
                    s.push_clip_layer(Fill::NonZero, actual, &RoundedRect::from_rect(*r, *radio));
                    forma::imagen(s, actual, img, *r, *alfa);
                    s.pop_layer();
                } else {
                    forma::imagen(s, actual, img, *r, *alfa);
                }
            }
            Trazo::Mascara { img, r, color } => forma::mascara(s, actual, img, *r, *color),
            Trazo::Recorte { r, radio } => {
                s.push_clip_layer(Fill::NonZero, actual, &RoundedRect::from_rect(*r, *radio));
                pila.push(Nivel {
                    tr: actual,
                    capa: true,
                });
            }
            Trazo::Grupo {
                r,
                escala: k,
                dx,
                dy,
                alfa,
            } => {
                let c = r.center();
                let t = actual
                    * Affine::translate((c.x + dx, c.y + dy))
                    * Affine::scale(*k)
                    * Affine::translate((-c.x, -c.y));
                let capa = *alfa < 1.0;
                if capa {
                    // Una capa con la opacidad, del tamaño de lo que se ve de la hoja.
                    let todo = Rect::new(-1e5, -1e5, 1e5, 1e5);
                    s.push_layer(Fill::NonZero, BlendMode::default(), *alfa, Affine::IDENTITY, &todo);
                }
                pila.push(Nivel { tr: t, capa });
            }
            Trazo::Escala { x, y } => {
                pila.push(Nivel {
                    tr: actual * Affine::scale_non_uniform(*x, *y),
                    capa: false,
                });
            }
            Trazo::Fin => {
                if pila.len() > 1
                    && let Some(n) = pila.pop()
                    && n.capa
                {
                    s.pop_layer();
                }
            }
        }
    }
    while pila.len() > 1 {
        if pila.pop().is_some_and(|n| n.capa) {
            s.pop_layer();
        }
    }
}

/// La parte lineal de `tr` dividida la escala de la salida: lo que se le aplica al texto, que
/// ya está armado en píxeles del buffer.
fn sin_traslacion(tr: Affine, escala: f64) -> Affine {
    let [a, b, c, d, _, _] = tr.as_coeffs();
    Affine::new([a / escala, b / escala, c / escala, d / escala, 0.0, 0.0])
}
