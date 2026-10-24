//! Las notificaciones del centro: el rótulo con «limpiar», los grupos por app (en el orden de
//! swaync) y cada tarjeta. Un grupo cerrado se apila (`pila.rs`: las de atrás asoman 8 px,
//! achicadas, con el fondo más tenue y sin contenido); abierto muestra su encabezado y las
//! tarjetas una debajo de otra, la más nueva arriba.
//!
//! Las medidas, de la hoja de GTK (`.notification-row` y `.control-center …`): 4 px arriba y
//! abajo de cada tarjeta (salvo arriba en el primer grupo y abajo en el último, que ponen el
//! rótulo y el borde del panel), 8 de relleno, la imagen en la columna de 36 y 10 hasta el
//! texto; el título a 14 px y la hora a 12 a su derecha, el cuerpo a 12 en hasta 10 renglones.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use morgan_motor::color::{Color, con_alfa, mezclar};
use morgan_motor::curva::bezier;
use morgan_motor::vello::kurbo::Rect;

use super::armar::{CUADRO, revelar};
use super::trazo::Trazo;
use super::{ANCHO, DESPLIEGUE, ENCABEZADO, ENTRE, Hoja, Objetivo, RADIO, Ui};
use crate::centro::vista::{Aviso, Grupo};
use crate::notificaciones::modelo::Urgencia;
use crate::pila;

const RELLENO: f64 = 8.0;
const AL_TEXTO: f64 = 10.0;
const LINEAS: usize = 10;
/// La vista previa: de ancho fijo, y una vertical no pasa de 180.
const ANCHO_PREVIA: f64 = 316.0;
const ALTO_PREVIA: f64 = 180.0;
/// El ícono chico de la app, en la esquina de la imagen, 6 adentro.
const CHICO: f64 = 16.0;
/// Los botones del encabezado: el ícono de 16 con 2 de relleno, cada 4.
const BOTON: f64 = 20.0;
/// Al limpiar, cada fila sale 56 px a la derecha y se apaga en 220 ms, una tras otra.
const SALIDA: Duration = Duration::from_millis(220);
const CORRIDA: f64 = 56.0;

pub fn clave(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn op(c: &Color, a: f32) -> Color {
    con_alfa(c, c[3] * a)
}

/// El tamaño de la vista previa de una imagen de `w` × `h`.
pub fn tam_previa(w: u32, h: u32) -> (f64, f64) {
    let (w, h) = (i64::from(w.max(1)), i64::from(h.max(1)));
    let caja = ANCHO_PREVIA as i64;
    let (a, b) = if caja * h / w <= ALTO_PREVIA as i64 {
        (caja, caja * h / w)
    } else {
        let tope = ALTO_PREVIA as i64;
        (tope * w / h, tope)
    };
    (a.max(1) as f64, b.max(1) as f64)
}

/// Cuánto salió una fila al limpiar: corrida y opacidad.
fn saliendo(desde: Instant, turno: usize, filas: usize, ahora: Instant) -> (f64, f32) {
    let paso = (240 / filas.max(1) as u64).min(40);
    let t0 = desde + Duration::from_millis(paso * turno as u64);
    let d = crate::animaciones::dur(SALIDA).as_secs_f32().max(1e-3);
    let t = (ahora.saturating_duration_since(t0).as_secs_f32() / d).clamp(0.0, 1.0);
    let corrida = f64::from(bezier(0.55, 0.0, 0.75, 0.2, t)) * CORRIDA;
    // `ease-in`.
    let alfa = 1.0 - bezier(0.42, 0.0, 1.0, 1.0, t);
    (corrida, alfa)
}

/// Lo que dura limpiar con `filas` filas.
pub fn duracion_limpiar(filas: usize) -> Duration {
    let paso = (240 / filas.max(1) as u64).min(40);
    Duration::from_millis(paso * filas.saturating_sub(1) as u64) + crate::animaciones::dur(SALIDA)
}

impl Ui {
    pub(super) fn armar_avisos(&mut self, h: &mut Hoja, y0: f64, ahora: Instant) -> f64 {
        let p = self.paleta;
        let mut y = y0;
        let hay = !self.datos.grupos.is_empty();
        // El rótulo.
        let titulo = self.rotulos.linea("notificaciones", 12.0, 700.0);
        let th = self.alto(&titulo);
        h.trazos.push(Trazo::Texto {
            l: titulo,
            x: 0.0,
            y,
            color: op(&p.on_surface, 0.5),
        });
        if hay {
            let limpiar = self.rotulos.linea("limpiar", 12.0, 400.0);
            let (w, lh) = (self.ancho(&limpiar), self.alto(&limpiar));
            let r = Rect::new(ANCHO - w, y, ANCHO, y + lh);
            let o = Objetivo::Limpiar;
            let l = if self.limpiando.is_some() {
                0.0
            } else {
                self.luz(o, ahora)
            };
            if self.limpiando.is_none() {
                h.objetivo(o, r, true);
            }
            h.trazos
                .push(Trazo::escalar(r, self.animos.escala(o, ahora)));
            h.trazos.push(Trazo::Texto {
                l: limpiar,
                x: r.x0,
                y,
                color: mezclar(&op(&p.on_surface, 0.5), &p.primary, l),
            });
            h.trazos.push(Trazo::Fin);
        }
        y += th + ENTRE;

        if !hay {
            // «Nada nuevo», centrado, con 18 arriba y 14 abajo.
            let glifo = self.rotulos.linea("󰂜", 13.0, 400.0);
            let texto = self.rotulos.linea("Nada nuevo", 13.0, 400.0);
            let (gw, tw) = (self.ancho(&glifo), self.ancho(&texto));
            let alto = self.alto(&texto).max(self.alto(&glifo));
            let x = ((ANCHO - gw - 8.0 - tw) / 2.0).floor();
            let color = op(&p.on_surface, 0.35);
            y += 18.0;
            h.trazos.push(Trazo::Texto {
                l: glifo,
                x,
                y,
                color,
            });
            h.trazos.push(Trazo::Texto {
                l: texto,
                x: x + gw + 8.0,
                y,
                color,
            });
            return y + alto + 14.0;
        }

        let grupos = std::mem::take(&mut self.datos.grupos);
        let n = grupos.len();
        let limpiando = self.limpiando.clone();
        for (i, g) in grupos.iter().enumerate() {
            let arriba = if i == 0 { 0.0 } else { 4.0 };
            let abajo = if i + 1 == n { 0.0 } else { 4.0 };
            let (dx, alfa) = match &limpiando {
                Some((t0, ids)) if g.avisos.iter().any(|a| ids.contains(&a.id)) => {
                    saliendo(*t0, i, n, ahora)
                }
                _ => (0.0, 1.0),
            };
            let mut sub = Hoja::default();
            let alto = self.armar_grupo(&mut sub, g, i == 0, arriba, abajo, ahora);
            if alfa < 1.0 || dx != 0.0 {
                h.trazos.push(Trazo::Grupo {
                    r: Rect::ZERO,
                    escala: 1.0,
                    dx,
                    dy: 0.0,
                    alfa,
                });
                h.incrustar(sub, 0.0, y, Rect::new(-40.0, y, ANCHO + 80.0, y + alto));
                h.trazos.push(Trazo::Fin);
            } else {
                h.incrustar(
                    sub,
                    0.0,
                    y,
                    Rect::new(-40.0, y - 4.0, ANCHO + 40.0, y + alto + 4.0),
                );
            }
            y += alto;
        }
        self.datos.grupos = grupos;
        y
    }

    /// Un grupo desde 0; devuelve su alto.
    fn armar_grupo(
        &mut self,
        h: &mut Hoja,
        g: &Grupo,
        primero: bool,
        arriba: f64,
        abajo: f64,
        ahora: Instant,
    ) -> f64 {
        let k = clave(&g.clave);
        let varias = g.avisos.len() > 1;
        let abierto = varias && self.expandidos.contains(&g.clave);
        let _ = primero;
        let mut y = 0.0;
        // El encabezado, que sube y baja con el grupo.
        let f = self
            .encabezados
            .get(&g.clave)
            .map_or(if abierto { 1.0 } else { 0.0 }, |r| r.valor(ahora));
        if f > 0.0 {
            let mut sub = Hoja::default();
            let alto = self.armar_encabezado(&mut sub, g, k, ahora);
            let v = (alto * f).round();
            h.incrustar(sub, 0.0, 0.0, Rect::new(-20.0, 0.0, ANCHO + 20.0, v));
            y += v;
        }
        if abierto {
            for a in &g.avisos {
                let o = Objetivo::Aviso(a.id);
                let luz = self.luz_tarjeta(o, ahora);
                let escala = 1.0 + 0.02 * f64::from(luz);
                let fondo = self.fondo_aviso(a.id, luz, false);
                let mut sub = Hoja::default();
                let alto = self.armar_aviso(&mut sub, a, arriba, abajo, fondo, true, ahora);
                let r = Rect::new(0.0, y + arriba, ANCHO, y + alto - abajo);
                // La fila del grupo abierto toma el foco en su primera tarjeta.
                h.objetivo(o, r, std::ptr::eq(a, &g.avisos[0]));
                h.deslizables.push((o, r));
                let dx = self.deslices.get(&o).map_or(0.0, |d| d.dx(ahora));
                if dx != 0.0 {
                    self.pista(h, r, dx, Some(("󰆴", "descartar")), Some(("󰆏", "copiar")));
                }
                h.trazos.push(Trazo::Grupo {
                    r,
                    escala,
                    dx,
                    dy: 0.0,
                    alfa: crate::deslizable::opacidad(dx, ANCHO) as f32,
                });
                h.incrustar(
                    sub,
                    0.0,
                    y,
                    Rect::new(-20.0, y - 4.0, ANCHO + 20.0, y + alto + 4.0),
                );
                h.trazos.push(Trazo::Fin);
                y += alto;
            }
            return y;
        }
        // Cerrado: la pila, con la de adelante entera.
        let o = if varias {
            Objetivo::Grupo(k)
        } else {
            Objetivo::Aviso(g.avisos[0].id)
        };
        let luz = self.luz_tarjeta(o, ahora);
        let escala = 1.0 + 0.02 * f64::from(luz);
        let mut frente = Hoja::default();
        let frente_fondo = self.fondo_aviso(g.avisos[0].id, luz, false);
        let alto = self.armar_aviso(
            &mut frente,
            &g.avisos[0],
            arriba,
            abajo,
            frente_fondo,
            true,
            ahora,
        );
        let visibles = g.avisos.len().min(pila::APILADAS);
        let total = alto + f64::from(pila::DESPLAZAMIENTO) * (visibles as f64 - 1.0);
        let r = Rect::new(0.0, y + arriba, ANCHO, y + total - abajo);
        h.objetivo(o, r, true);
        h.deslizables.push((o, r));
        let dx = self.deslices.get(&o).map_or(0.0, |d| d.dx(ahora));
        if dx != 0.0 {
            if varias {
                self.pista(h, r, dx, Some(("󰆴", "descartar todo")), None);
            } else {
                self.pista(h, r, dx, Some(("󰆴", "descartar")), Some(("󰆏", "copiar")));
            }
        }
        h.trazos.push(Trazo::Grupo {
            r: Rect::ZERO,
            escala: 1.0,
            dx,
            dy: 0.0,
            alfa: crate::deslizable::opacidad(dx, ANCHO) as f32,
        });
        // Las de atrás primero, la más lejana primero: solo su fondo.
        for i in (1..visibles).rev() {
            let a = &g.avisos[i];
            let mut sub = Hoja::default();
            let fondo = mezclar(
                &op(&self.paleta.on_surface, 0.03),
                &op(&self.paleta.primary, 0.12),
                luz,
            );
            let propio = self.armar_aviso(&mut sub, a, arriba, abajo, fondo, false, ahora);
            let (px, py, e) = pila::ubicacion(i, ANCHO as i32, alto as i32);
            let aplastado = f64::from(pila::aplastado(alto as i32, propio.max(alto) as i32));
            let ey = f64::from(e) * aplastado;
            h.trazos.push(Trazo::escalar(r, escala));
            // La de atrás, achicada desde su esquina y corrida: `translate(x, y) scale(e)`.
            h.trazos.push(Trazo::Grupo {
                r: Rect::ZERO,
                escala: 1.0,
                dx: f64::from(px),
                dy: y + f64::from(py),
                alfa: 1.0,
            });
            h.trazos.push(Trazo::Escala {
                x: f64::from(e),
                y: ey,
            });
            h.trazos.extend(sub.trazos);
            h.trazos.push(Trazo::Fin);
            h.trazos.push(Trazo::Fin);
            h.trazos.push(Trazo::Fin);
        }
        h.trazos.push(Trazo::escalar(r, escala));
        h.trazos.push(Trazo::mover(0.0, y));
        // Lo que responde de la de adelante (sus acciones) queda en su lugar.
        let mover = morgan_motor::vello::kurbo::Vec2::new(0.0, y);
        for (o, r) in frente.objetivos {
            h.objetivos.push((o, r + mover));
        }
        h.enfocables.extend(frente.enfocables);
        h.presentes.extend(frente.presentes);
        h.trazos.extend(frente.trazos);
        h.trazos.push(Trazo::Fin);
        h.trazos.push(Trazo::Fin);
        h.trazos.push(Trazo::Fin);
        y + total
    }

    /// El puntero o el foco (que en la lista se ve siempre, como el de swaync).
    fn luz_tarjeta(&mut self, o: Objetivo, ahora: Instant) -> f32 {
        let si = self.encima == Some(o) || self.foco == Some(o);
        self.animos.luz(o, si, ahora)
    }

    fn fondo_aviso(&self, id: u32, luz: f32, _atras: bool) -> Color {
        let p = &self.paleta;
        if self.con_acciones.contains(&id) {
            return op(&p.primary, 0.2);
        }
        mezclar(&op(&p.on_surface, 0.06), &op(&p.primary, 0.25), luz)
    }

    /// El encabezado de un grupo abierto: el nombre y los botones de contraer y cerrar todo.
    fn armar_encabezado(&mut self, h: &mut Hoja, g: &Grupo, k: u64, ahora: Instant) -> f64 {
        let p = self.paleta;
        let x1 = ANCHO - 8.0;
        let botones = [
            (Objetivo::CerrarGrupo(k), "window-close", x1 - BOTON),
            (Objetivo::Contraer(k), "pan-up", x1 - BOTON - 4.0 - BOTON),
        ];
        let max = (botones[1].2 - 4.0 - 8.0 - 12.0).max(0.0);
        let nombre = self.rotulos.recortada(&g.nombre.to_lowercase(), 12.0, max);
        let nh = self.alto(&nombre);
        let alto = nh.max(BOTON);
        h.trazos.push(Trazo::Texto {
            l: nombre,
            x: 12.0,
            y: ((alto - nh) / 2.0).floor(),
            color: op(&p.on_surface, 0.5),
        });
        for (o, icono, x) in botones {
            let r = Rect::new(
                x,
                ((alto - BOTON) / 2.0).floor(),
                x + BOTON,
                ((alto - BOTON) / 2.0).floor() + BOTON,
            );
            let l = self.luz(o, ahora);
            let color = mezclar(&op(&p.on_surface, 0.35), &p.primary, l);
            h.objetivo(o, r, false);
            h.trazos
                .push(Trazo::escalar(r, self.animos.escala(o, ahora)));
            let img = match icono {
                "pan-up" => self.datos.iconos.contraer.clone(),
                _ => self.datos.iconos.cerrar.clone(),
            };
            if let Some(img) = img {
                h.trazos.push(Trazo::Mascara {
                    img,
                    r: r.inset(-2.0),
                    color,
                });
            }
            h.trazos.push(Trazo::Fin);
        }
        alto
    }

    /// Una tarjeta desde 0, con sus 4 de afuera arriba y abajo (o lo que diga `arriba` y
    /// `abajo`); con `contenido` falso solo su fondo (las de atrás de una pila). Devuelve el
    /// alto, con los de afuera.
    #[allow(clippy::too_many_arguments)]
    fn armar_aviso(
        &mut self,
        h: &mut Hoja,
        a: &Aviso,
        arriba: f64,
        abajo: f64,
        fondo: Color,
        contenido: bool,
        ahora: Instant,
    ) -> f64 {
        let p = self.paleta;
        let y0 = arriba;
        let x = RELLENO;
        let mut y = y0 + RELLENO;
        let inicio = h.trazos.len();
        let ancho = ANCHO - 2.0 * RELLENO;
        let tiene_imagen = a.imagen.is_some();
        let tx = if tiene_imagen {
            x + CUADRO + AL_TEXTO
        } else {
            x
        };
        let tw = x + ancho - tx;
        // El título y la hora.
        let hora = crate::notificaciones::modelo::relativo(
            crate::notificaciones::servidor::ahora() - a.llegada,
        )
        .to_lowercase();
        let lh = self.rotulos.linea(&hora, 12.0, 400.0);
        let wh = self.ancho(&lh);
        let critica = a.urgencia == Urgencia::Critica;
        let titulo = if critica {
            let max = (tw - wh).max(0.0);
            let l = self.rotulos.linea(&a.titulo, 14.0, 700.0);
            if self.ancho(&l) <= max {
                l
            } else {
                self.rotulos.recortada(&a.titulo, 14.0, max)
            }
        } else {
            self.rotulos.recortada(&a.titulo, 14.0, (tw - wh).max(0.0))
        };
        let th = self.alto(&titulo).max(self.alto(&lh));
        let cuerpo =
            (!a.cuerpo.is_empty()).then(|| self.rotulos.parrafo(&a.cuerpo, 12.0, tw, LINEAS));
        let ch = cuerpo.as_ref().map_or(0.0, |c| {
            (morgan_motor::texto::caja(c).1 / f64::from(self.rotulos.escala())).ceil()
        });
        let alto_textos = th + ch;
        let alto_fila = if tiene_imagen {
            CUADRO.max(alto_textos)
        } else {
            alto_textos
        };
        if contenido {
            if let Some(img) = &a.imagen {
                let iy = y + ((alto_fila - CUADRO) / 2.0).floor();
                let ri = Rect::new(x, iy, x + CUADRO, iy + CUADRO);
                h.trazos.push(Trazo::Imagen {
                    img: img.clone(),
                    r: ri,
                    radio: RADIO,
                    alfa: 1.0,
                });
                if let Some(chico) = &a.chico {
                    // En la esquina de abajo de la superposición, que incluye los 10 de margen
                    // de la imagen: asoma a la derecha del ícono grande.
                    let x1 = ri.x1 + AL_TEXTO - 6.0;
                    let rc = Rect::new(x1 - CHICO, ri.y1 - 6.0 - CHICO, x1, ri.y1 - 6.0);
                    // `-gtk-icon-shadow: 0 1px 4px black`, con la forma de un círculo.
                    h.trazos.push(Trazo::Sombra {
                        r: rc + morgan_motor::vello::kurbo::Vec2::new(0.0, 1.0),
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
            let ty = y + ((alto_fila - alto_textos) / 2.0).floor();
            let color_titulo = match a.urgencia {
                Urgencia::Critica => p.error,
                Urgencia::Baja => op(&p.on_surface, 0.6),
                Urgencia::Normal => p.on_surface,
            };
            h.trazos.push(Trazo::Texto {
                l: titulo,
                x: tx,
                y: ty,
                color: color_titulo,
            });
            h.trazos.push(Trazo::Texto {
                l: lh,
                x: tx + tw - wh,
                y: ty,
                color: op(&p.on_surface, 0.35),
            });
            if let Some(c) = cuerpo {
                h.trazos.push(Trazo::Texto {
                    l: c,
                    x: tx,
                    y: ty + th,
                    color: op(&p.on_surface, 0.5),
                });
            }
        }
        y += alto_fila;
        if let Some((img, w, hh)) = &a.previa {
            let (pw, ph) = tam_previa(*w, *hh);
            y += 8.0;
            let px = x + ((ancho - pw) / 2.0).floor();
            if contenido {
                h.trazos.push(Trazo::Imagen {
                    img: img.clone(),
                    r: Rect::new(px, y, px + pw, y + ph),
                    radio: RADIO,
                    alfa: 1.0,
                });
            }
            y += ph;
        }
        if let Some(v) = a.progreso {
            y += 8.0 + 4.0;
            if contenido {
                let canal = Rect::new(x, y, x + ancho, y + 2.0);
                h.trazos.push(Trazo::Caja {
                    r: canal,
                    radio: 0.0,
                    color: p.outline_variant,
                });
                let w = ancho * (f64::from(v) / 100.0).clamp(0.0, 1.0);
                h.trazos.push(Trazo::Caja {
                    r: Rect::new(x, y, x + w, y + 2.0),
                    radio: 0.0,
                    color: p.primary,
                });
            }
            y += 2.0;
        }
        y += RELLENO;
        // Las acciones, que se despliegan debajo, adentro de la tarjeta.
        if !a.acciones.is_empty() {
            let f = self
                .revelados_acciones
                .get(&a.id)
                .map_or(0.0, |r| r.valor(ahora));
            if f > 0.0 && contenido {
                let mut sub = Hoja::default();
                let mut sy = 0.0;
                for (i, (_, etiqueta, glifo)) in a.acciones.iter().enumerate() {
                    sy = self.armar_accion(
                        &mut sub,
                        a.id,
                        i,
                        glifo,
                        etiqueta,
                        ANCHO - 8.0,
                        sy,
                        ahora,
                    );
                }
                sub.trazos.insert(
                    0,
                    Trazo::Caja {
                        r: Rect::new(0.0, 0.0, ANCHO - 8.0, sy),
                        radio: RADIO,
                        color: op(&p.on_surface, 0.04),
                    },
                );
                let alto = sy + 4.0;
                let v = (alto * f).round();
                h.incrustar(sub, 4.0, y + v - alto, Rect::new(0.0, y, ANCHO, y + v));
                y += v;
            }
        }
        h.trazos.insert(
            inicio,
            Trazo::Caja {
                r: Rect::new(0.0, y0, ANCHO, y),
                radio: RADIO,
                color: fondo,
            },
        );
        y + abajo
    }

    /// Un botón de una notificación, en la forma de las opciones de las listas.
    #[allow(clippy::too_many_arguments)]
    fn armar_accion(
        &mut self,
        h: &mut Hoja,
        id: u32,
        i: usize,
        glifo: &str,
        etiqueta: &str,
        ancho: f64,
        y: f64,
        ahora: Instant,
    ) -> f64 {
        let p = self.paleta;
        let lg = self.rotulos.linea(glifo, 14.0, 400.0);
        let tx = 4.0 + 32.0 + 4.0 + 10.0;
        let lt = self
            .rotulos
            .recortada(etiqueta, 14.0, (ancho - 4.0 - tx).max(0.0));
        let alto = 12.0 + self.alto(&lt).max(self.alto(&lg));
        let r = Rect::new(0.0, y, ancho, y + alto);
        let o = Objetivo::AccionAviso(id, i);
        let l = self.luz(o, ahora);
        h.objetivo(o, r, true);
        h.trazos
            .push(Trazo::escalar(r, self.animos.escala(o, ahora)));
        h.trazos.push(Trazo::Caja {
            r,
            radio: RADIO,
            color: op(&p.primary, 0.14 * l),
        });
        let col = Rect::new(4.0, y + 6.0, 36.0, y + 6.0 + self.alto(&lg));
        h.trazos.push(Trazo::Glifo {
            l: lg,
            c: col.center(),
            color: op(&p.on_surface, 0.7),
        });
        h.trazos.push(Trazo::Texto {
            l: lt,
            x: tx,
            y: y + 6.0,
            color: p.on_surface,
        });
        h.trazos.push(Trazo::Fin);
        y + alto
    }

    /// Abre o cierra un grupo (y su encabezado).
    pub fn expandir(&mut self, clave_grupo: &str, abrir: bool, ahora: Instant) {
        if abrir {
            self.expandidos.insert(clave_grupo.to_owned());
        } else {
            self.expandidos.remove(clave_grupo);
        }
        revelar(
            &mut self.encabezados,
            clave_grupo.to_owned(),
            abrir,
            crate::animaciones::dur(ENCABEZADO),
            ahora,
        );
    }

    /// Muestra o esconde las acciones de una notificación.
    pub fn alternar_acciones(&mut self, id: u32, ahora: Instant) {
        let abrir = !self.con_acciones.contains(&id);
        if abrir {
            self.con_acciones.insert(id);
        } else {
            self.con_acciones.remove(&id);
        }
        revelar(
            &mut self.revelados_acciones,
            id,
            abrir,
            crate::animaciones::dur(DESPLIEGUE),
            ahora,
        );
    }

    /// El grupo de una notificación, por clave de grupo.
    pub fn grupo_de(&self, k: u64) -> Option<&Grupo> {
        self.datos.grupos.iter().find(|g| clave(&g.clave) == k)
    }

    pub fn aviso(&self, id: u32) -> Option<&Aviso> {
        self.datos
            .grupos
            .iter()
            .flat_map(|g| g.avisos.iter())
            .find(|a| a.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previa_de_ancho_fijo_y_vertical_acotada() {
        assert_eq!(tam_previa(1920, 1080), (316.0, 177.0));
        assert_eq!(tam_previa(1080, 1920), (101.0, 180.0));
    }

    #[test]
    fn limpiar_escalona_y_termina() {
        let t0 = Instant::now();
        let (c, a) = saliendo(t0, 0, 3, t0);
        assert!(c.abs() < 1e-3 && (a - 1.0).abs() < 1e-3);
        let (c, a) = saliendo(t0, 0, 3, t0 + Duration::from_secs(1));
        assert!((c - CORRIDA).abs() < 1e-3 && a.abs() < 1e-3);
        // La segunda arranca 40 ms después.
        assert!(saliendo(t0, 1, 3, t0 + Duration::from_millis(40)).0.abs() < 1e-3);
        assert!(duracion_limpiar(3) >= Duration::from_millis(80));
    }
}
