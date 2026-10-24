//! Dónde cae cada cosa del panel arriba de las notificaciones, y con qué color: las tarjetas y
//! sus listas, los niveles, las cuotas y el reproductor. Cada cuadro, de arriba abajo.

use std::time::{Duration, Instant};

use morgan_motor::color::{Color, con_alfa, mezclar};
use morgan_motor::curva::Revelado;
use morgan_motor::parley::Layout;
use morgan_motor::texto;
use morgan_motor::vello::kurbo::{Point, Rect};

use super::trazo::{Degrade, Trazo};
use super::{ANCHO, ENTRE, Hoja, Objetivo, RADIO, Ui};
use crate::centro::marquesina;
use crate::centro::vista::{
    BotonCampo, BotonRepro, Contenido, Cuota, ListaId, Nivel, Opcion, Toque,
};

/// La tarjeta: 8 de relleno, el cuadrito de 36 y 10 hasta el texto.
const RELLENO: f64 = 8.0;
pub const CUADRO: f64 = 36.0;
const AL_TEXTO: f64 = 10.0;
/// Las opciones de una lista: la columna del glifo (32 más 4) y 10 hasta el texto.
const COLUMNA_GLIFO: f64 = 32.0;
const TRAS_GLIFO: f64 = 4.0;
/// El slider: el relleno de la escala (4 arriba, 14 a la derecha, 2 abajo) alrededor de los 10
/// de alto mínimo de una escala de GTK, con el canal de 5 centrado (2 abajo de su borde).
const ESCALA_ALTO: f64 = 16.0;
const CANAL_Y: f64 = 6.0;
const CANAL_ALTO: f64 = 5.0;
const ESCALA_DERECHA: f64 = 14.0;
/// La barra de las cuotas: 6 de alto con 6 arriba, 14 a la derecha y 4 abajo.
const BARRA_ALTO: f64 = 6.0;
/// El reproductor.
const CARATULA: f64 = 72.0;
const BOTON_REPRO: f64 = 32.0;
const ICONO_REPRO: f64 = 16.0;
/// Lo apagado del reproductor (mezclar o repetir sin poner), y un botón deshabilitado (el
/// `-gtk-icon-filter: opacity(0.5)` del tema; el color sigue siendo el de la hoja).
const APAGADO: f32 = 0.5;

fn op(c: &Color, a: f32) -> Color {
    con_alfa(c, c[3] * a)
}

/// Lo que se ve de algo que se despliega: el alto entero por la fracción, como un
/// `GtkRevealer`.
fn visto(alto: f64, f: f64) -> f64 {
    (alto * f).round()
}

impl Ui {
    fn escala_f(&self) -> f64 {
        f64::from(self.rotulos.escala())
    }

    pub(super) fn ancho(&self, l: &Layout<u8>) -> f64 {
        texto::caja(l).0 / self.escala_f()
    }

    /// El alto de la caja de una etiqueta de GTK con ese texto.
    pub(super) fn alto(&self, l: &Layout<u8>) -> f64 {
        texto::alto_de_linea(l) / self.escala_f()
    }

    pub(super) fn armar_todo(&mut self, h: &mut Hoja, ahora: Instant) {
        let mut y = self.armar_tarjetas(h, 0.0, ahora);
        y += ENTRE;
        y = self.armar_niveles(h, y, ahora);
        if self.datos.reproductor.is_some() {
            y += ENTRE;
            y = self.armar_reproductor(h, y, ahora);
        }
        y += ENTRE;
        y = self.armar_avisos(h, y, ahora);
        h.alto = y;
    }

    /// Encendido con el puntero o con el foco del teclado a la vista.
    pub(super) fn luz(&mut self, o: Objetivo, ahora: Instant) -> f32 {
        let si = self.encima == Some(o) || (self.foco_visible && self.foco == Some(o));
        self.animos.luz(o, si, ahora)
    }

    /// El fondo de algo que se toca: el tinte de reposo, el del puntero, y el de lo abierto.
    fn fondo(&mut self, o: Objetivo, reposo: f32, puntero: f32, abierto: bool, ahora: Instant) -> Color {
        let p = &self.paleta;
        let (base, encendido) = (op(&p.on_surface, reposo), op(&p.primary, puntero));
        let l = self.luz(o, ahora);
        if abierto {
            op(&self.paleta.primary, 0.2)
        } else {
            mezclar(&base, &encendido, l)
        }
    }

    // ── Tarjetas ──

    fn armar_tarjetas(&mut self, h: &mut Hoja, y0: f64, ahora: Instant) -> f64 {
        let ancho_col = (ANCHO - ENTRE) / 2.0;
        let mut y = y0;
        let n = self.datos.tarjetas.len();
        for fila in 0..n.div_ceil(2) {
            // El alto de la fila es el de su tarjeta más alta.
            let mut alto_fila: f64 = 0.0;
            let mut armadas = Vec::new();
            for col in 0..2 {
                let i = fila * 2 + col;
                let Some(t) = self.datos.tarjetas.get(i).cloned() else {
                    continue;
                };
                let max = ancho_col - RELLENO - CUADRO - AL_TEXTO - RELLENO;
                let titulo = self.rotulos.recortada(&t.titulo, 14.0, max);
                let estado = self.rotulos.recortada(&t.estado, 12.0, max);
                let (th, eh) = (self.alto(&titulo), self.alto(&estado));
                alto_fila = alto_fila.max(RELLENO * 2.0 + CUADRO.max(th + eh));
                armadas.push((i, col, t, titulo, estado));
            }
            for (i, col, t, titulo, estado) in armadas {
                let x = col as f64 * (ancho_col + ENTRE);
                let r = Rect::new(x, y, x + ancho_col, y + alto_fila);
                let o = Objetivo::Tarjeta(i);
                let abierta = matches!(t.que, Toque::Lista(l) if self.abierta == Some(l));
                let fondo = self.fondo(o, 0.06, 0.14, abierta, ahora);
                h.objetivo(o, r, true);
                h.trazos.push(Trazo::escalar(r, self.animos.escala(o, ahora)));
                h.trazos.push(Trazo::Caja {
                    r,
                    radio: RADIO,
                    color: fondo,
                });
                let cy = y + ((alto_fila - CUADRO) / 2.0).floor();
                let cuadro = Rect::new(x + RELLENO, cy, x + RELLENO + CUADRO, cy + CUADRO);
                let p = self.paleta;
                if t.encendida {
                    h.trazos.push(Trazo::Sombra {
                        r: cuadro,
                        radio: RADIO,
                        color: con_alfa(&p.tertiary, 0.3),
                        spread: 0.0,
                        blur: 6.0,
                    });
                    h.trazos.push(Trazo::Degrade {
                        r: cuadro,
                        radio: RADIO,
                        a: p.primary,
                        b: p.tertiary,
                        forma: Degrade::Diagonal,
                    });
                } else {
                    h.trazos.push(Trazo::Caja {
                        r: cuadro,
                        radio: RADIO,
                        color: op(&p.on_surface, 0.08),
                    });
                }
                let glifo = self.rotulos.linea(&t.glifo, 18.0, 400.0);
                h.trazos.push(Trazo::Glifo {
                    l: glifo,
                    c: cuadro.center(),
                    color: if t.encendida { p.surface } else { p.on_surface },
                });
                let (th, eh) = (self.alto(&titulo), self.alto(&estado));
                let tx = x + RELLENO + CUADRO + AL_TEXTO;
                let ty = y + ((alto_fila - th - eh) / 2.0).floor();
                h.trazos.push(Trazo::Texto {
                    l: titulo,
                    x: tx,
                    y: ty,
                    color: p.on_surface,
                });
                let color_estado = if t.alerta {
                    op(&p.error, 0.5)
                } else if t.encendida {
                    op(&p.tertiary, 0.9)
                } else {
                    op(&p.on_surface, 0.5)
                };
                h.trazos.push(Trazo::Texto {
                    l: estado,
                    x: tx,
                    y: ty + th,
                    color: color_estado,
                });
                h.trazos.push(Trazo::Fin);
            }
            y += alto_fila + if fila + 1 < n.div_ceil(2) { ENTRE } else { 0.0 };
        }
        // Las listas de las tarjetas, debajo de la grilla, en su orden.
        let listas: Vec<ListaId> = self
            .datos
            .tarjetas
            .iter()
            .filter_map(|t| match t.que {
                Toque::Lista(l) => Some(l),
                Toque::Accion(_) => None,
            })
            .collect();
        for l in listas {
            y = self.armar_lista(h, l, 0.0, ANCHO, y, 4.0, ahora);
        }
        y
    }

    // ── Listas ──

    /// La lista `l` desplegada debajo de `y`, del ancho dado; `lado` es su relleno a los
    /// costados (4 en las de las tarjetas, 0 en las de los niveles, que caen en sus columnas).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn armar_lista(
        &mut self,
        h: &mut Hoja,
        l: ListaId,
        x: f64,
        ancho: f64,
        y: f64,
        lado: f64,
        ahora: Instant,
    ) -> f64 {
        let f = self.revelado(l, ahora);
        if f <= 0.0 {
            return y;
        }
        let Some(contenido) = self.datos.listas.get(&l).cloned() else {
            return y;
        };
        let mut sub = Hoja::default();
        let alto = match contenido {
            Contenido::Opciones(ops) => {
                if ops.is_empty() {
                    return y;
                }
                self.armar_opciones(&mut sub, l, &ops, ancho, lado, ahora)
            }
            Contenido::Cuotas(filas) => self.armar_cuotas(&mut sub, &filas, ancho, ahora),
        };
        let v = visto(alto, f);
        // Se despliega hacia abajo: lo de abajo asoma primero.
        h.incrustar(sub, x, y + v - alto, Rect::new(x - 20.0, y, x + ancho + 20.0, y + v));
        y + v
    }

    /// Las opciones en su recuadro (`cc-lista`), con 8 arriba; devuelve el alto.
    fn armar_opciones(
        &mut self,
        h: &mut Hoja,
        l: ListaId,
        ops: &[Opcion],
        ancho: f64,
        lado: f64,
        ahora: Instant,
    ) -> f64 {
        let top = ENTRE;
        let mut y = top + 4.0;
        let inicio = h.trazos.len();
        for (i, o) in ops.iter().enumerate() {
            y = self.armar_opcion(h, l, i, o, lado, ancho - lado * 2.0, y, ahora);
            if let Some(c) = &o.campo {
                y = self.armar_campo(h, l, c, lado, ancho - lado * 2.0, y, ahora);
            }
        }
        y += 4.0;
        h.trazos.insert(
            inicio,
            Trazo::Caja {
                r: Rect::new(0.0, top, ancho, y),
                radio: RADIO,
                color: op(&self.paleta.on_surface, 0.04),
            },
        );
        y
    }

    /// Una opción (`cc-opcion`): 6 arriba y abajo, `lado` a los costados; glifo, texto y el
    /// tilde de lo puesto.
    #[allow(clippy::too_many_arguments)]
    fn armar_opcion(
        &mut self,
        h: &mut Hoja,
        l: ListaId,
        i: usize,
        o: &Opcion,
        x: f64,
        ancho: f64,
        y: f64,
        ahora: Instant,
    ) -> f64 {
        let marcada = o.marcada == Some(true);
        let tilde = marcada.then(|| self.rotulos.linea("󰄬", 14.0, 400.0));
        let ancho_tilde = tilde.as_ref().map_or(0.0, |t| self.ancho(t) + 4.0 + 10.0);
        let tx = x + 4.0 + COLUMNA_GLIFO + TRAS_GLIFO + 10.0;
        let max = (x + ancho - 4.0 - ancho_tilde - tx).max(0.0);
        let texto = self.rotulos.recortada(&o.texto, 14.0, max);
        let glifo = self.rotulos.linea(&o.glifo, 14.0, 400.0);
        let alto = 12.0 + self.alto(&texto).max(self.alto(&glifo));
        let r = Rect::new(x, y, x + ancho, y + alto);
        let obj = Objetivo::Opcion(l, i);
        let p = self.paleta;
        let fondo = self.fondo(obj, 0.0, 0.14, o.abierta, ahora);
        h.objetivo(obj, r, o.habilitada);
        let desliza = o.derecha.is_some() || o.izquierda.is_some();
        if desliza {
            h.deslizables.push((obj, r));
        }
        let dx = self.deslices.get(&obj).map_or(0.0, |d| d.dx(ahora));
        if dx != 0.0 {
            self.pista(h, r, dx, o.derecha.as_ref().map(|d| (d.glifo, d.texto.as_str())), o.izquierda.as_ref().map(|d| (d.glifo, d.texto.as_str())));
        }
        let alfa = if o.habilitada { 1.0 } else { 0.6 } * crate::deslizable::opacidad(dx, ancho) as f32;
        h.trazos.push(Trazo::Grupo {
            r,
            escala: self.animos.escala(obj, ahora),
            dx,
            dy: 0.0,
            alfa,
        });
        h.trazos.push(Trazo::Caja {
            r,
            radio: RADIO,
            color: fondo,
        });
        let gy = y + 6.0;
        let col = Rect::new(x + 4.0, gy, x + 4.0 + COLUMNA_GLIFO, gy + self.alto(&glifo));
        let color_glifo = if marcada {
            p.tertiary
        } else {
            op(&p.on_surface, 0.7)
        };
        h.trazos.push(Trazo::Glifo {
            l: glifo,
            c: col.center(),
            color: color_glifo,
        });
        h.trazos.push(Trazo::Texto {
            l: texto,
            x: tx,
            y: gy,
            color: p.on_surface,
        });
        if let Some(t) = tilde {
            let w = self.ancho(&t);
            let caja = Rect::new(x + ancho - 4.0 - 4.0 - w, gy, x + ancho - 4.0 - 4.0, gy + self.alto(&t));
            h.trazos.push(Trazo::Glifo {
                l: t,
                c: caja.center(),
                color: p.tertiary,
            });
        }
        h.trazos.push(Trazo::Fin);
        y + alto
    }

    /// Lo que asoma debajo de una fila que se arrastra: la acción de ese lado.
    pub(super) fn pista(
        &mut self,
        h: &mut Hoja,
        r: Rect,
        dx: f64,
        derecha: Option<(&str, &str)>,
        izquierda: Option<(&str, &str)>,
    ) {
        let p = self.paleta;
        let asomo = crate::deslizable::asomo(dx, r.width()) as f32;
        let (accion, a_la_izquierda) = if dx > 0.0 {
            (derecha, true)
        } else {
            (izquierda, false)
        };
        h.trazos.push(Trazo::Grupo {
            r,
            escala: 1.0,
            dx: 0.0,
            dy: 0.0,
            alfa: asomo,
        });
        h.trazos.push(Trazo::Caja {
            r,
            radio: RADIO,
            color: op(&p.on_surface, 0.04),
        });
        if let Some((glifo, texto)) = accion {
            let l = self.rotulos.linea(&format!("{glifo}  {texto}"), 12.0, 400.0);
            let (w, al) = (self.ancho(&l), self.alto(&l));
            let x = if a_la_izquierda {
                r.x0 + 16.0
            } else {
                r.x1 - 16.0 - w
            };
            h.trazos.push(Trazo::Texto {
                l,
                x,
                y: r.y0 + ((r.height() - al) / 2.0).floor(),
                color: op(&p.on_surface, 0.7),
            });
        }
        h.trazos.push(Trazo::Fin);
    }

    // ── Niveles ──

    fn armar_niveles(&mut self, h: &mut Hoja, y0: f64, ahora: Instant) -> f64 {
        let niveles = self.datos.niveles.clone();
        if niveles.is_empty() {
            return y0 - ENTRE;
        }
        let inicio = h.trazos.len();
        let mut y = y0 + 4.0;
        for n in &niveles {
            y = self.armar_nivel(h, n, 8.0, ANCHO - 16.0, y, ahora);
        }
        y += 4.0;
        h.trazos.insert(
            inicio,
            Trazo::Caja {
                r: Rect::new(0.0, y0, ANCHO, y),
                radio: RADIO,
                color: op(&self.paleta.on_surface, 0.06),
            },
        );
        y
    }

    /// Un nivel (`cc-nivel`, 4 arriba y abajo): el glifo en su columna, y en la otra el nombre
    /// y el porcentaje encima del slider; debajo, su lista.
    fn armar_nivel(&mut self, h: &mut Hoja, n: &Nivel, x: f64, ancho: f64, y0: f64, ahora: Instant) -> f64 {
        let p = self.paleta;
        let y = y0 + 4.0;
        let valor = match self.arrastrando {
            Some((id, v)) if id == n.id => v,
            _ => n.valor,
        };
        // El glifo.
        let caja_glifo = Rect::new(x, y, x + CUADRO, y + CUADRO);
        let og = Objetivo::Glifo(n.id);
        let glifo = self.rotulos.linea(&n.glifo, 18.0, 400.0);
        let color_glifo = if n.mudo {
            p.error
        } else if n.glifo_accion.is_some() {
            mezclar(&p.on_surface, &p.primary, self.luz(og, ahora))
        } else {
            p.on_surface
        };
        if n.glifo_accion.is_some() {
            h.objetivo(og, caja_glifo, true);
        }
        h.trazos.push(Trazo::escalar(caja_glifo, self.animos.escala(og, ahora)));
        h.trazos.push(Trazo::Glifo {
            l: glifo,
            c: caja_glifo.center(),
            color: color_glifo,
        });
        h.trazos.push(Trazo::Fin);

        // El rótulo y el slider.
        let cx = x + CUADRO + AL_TEXTO;
        let cw = x + ancho - cx;
        let porcentaje = if n.mudo {
            "mudo".to_owned()
        } else {
            format!("{valor:.0}%")
        };
        let lp = self.rotulos.linea(&porcentaje, 12.0, 400.0);
        let wp = self.ancho(&lp);
        let nombre = self.rotulos.recortada(&n.nombre, 12.0, (cw - wp - 6.0).max(0.0));
        let lh = self.alto(&nombre).max(self.alto(&lp));
        let alto_col = lh + 2.0 + ESCALA_ALTO;
        let cy = y + ((CUADRO - alto_col) / 2.0).floor();
        let on = Objetivo::Nombre(n.id);
        let abierta = n.lista.is_some_and(|l| self.abierta == Some(l));
        let r_nombre = Rect::new(cx, cy, cx + self.ancho(&nombre), cy + lh);
        // La etiqueta tiene su color (`.cc-nivel .rotulo label`): con el puntero o la lista
        // abierta solo sube la opacidad.
        let color_nombre = if n.lista.is_some() {
            let l = if abierta { 1.0 } else { self.luz(on, ahora) };
            op(&p.on_surface, 0.5 + 0.5 * l)
        } else {
            op(&p.on_surface, 0.5)
        };
        if n.lista.is_some() {
            h.objetivo(on, r_nombre, true);
        }
        h.trazos.push(Trazo::escalar(r_nombre, self.animos.escala(on, ahora)));
        h.trazos.push(Trazo::Texto {
            l: nombre,
            x: cx,
            y: cy,
            color: color_nombre,
        });
        h.trazos.push(Trazo::Fin);
        h.trazos.push(Trazo::Texto {
            l: lp,
            x: cx + cw - wp,
            y: cy,
            color: if n.mudo { p.error } else { p.tertiary },
        });
        let sy = cy + lh + 2.0;
        let canal = Rect::new(cx, sy + CANAL_Y, cx + cw - ESCALA_DERECHA, sy + CANAL_Y + CANAL_ALTO);
        self.armar_slider(h, n, canal, valor, ahora);
        let mut y = y + CUADRO;
        if let Some(l) = n.lista {
            y = self.armar_lista(h, l, x - 8.0 + 8.0, ancho, y, 0.0, ahora);
        }
        y + 4.0
    }

    /// El canal, lo lleno con el degradé y la perilla, que crece con el foco y se achica
    /// mientras se arrastra.
    fn armar_slider(&mut self, h: &mut Hoja, n: &Nivel, canal: Rect, valor: f64, ahora: Instant) {
        let p = self.paleta;
        let o = Objetivo::Slider(n.id);
        // Donde responde: el canal con su alto de escala.
        let zona = Rect::new(canal.x0 - 5.0, canal.y0 - CANAL_Y, canal.x1 + 5.0, canal.y0 - CANAL_Y + ESCALA_ALTO);
        h.objetivo(o, zona, true);
        h.trazos.push(Trazo::Caja {
            r: canal,
            radio: RADIO,
            color: op(&p.outline_variant, 0.6),
        });
        let cx = canal.x0 + canal.width() * (valor / 100.0).clamp(0.0, 1.0);
        let lleno = Rect::new(canal.x0, canal.y0, cx, canal.y1);
        if lleno.width() > 0.0 {
            if n.mudo {
                h.trazos.push(Trazo::Caja {
                    r: lleno,
                    radio: RADIO,
                    color: op(&p.on_surface, 0.25),
                });
            } else {
                h.trazos.push(Trazo::Degrade {
                    r: lleno,
                    radio: RADIO,
                    a: p.primary,
                    b: p.tertiary,
                    forma: Degrade::Horizontal { de: lleno },
                });
            }
        }
        let encima = self.luz(o, ahora);
        let tam = self.perilla(n.id, o, ahora);
        let c = Point::new(cx, canal.center().y);
        let r = Rect::new(c.x - tam / 2.0, c.y - tam / 2.0, c.x + tam / 2.0, c.y + tam / 2.0);
        if !n.mudo {
            let enfocada = self.foco_visible && self.foco == Some(o);
            let apretada = self.apretado == Some(o);
            // Las sombras de la hoja: en reposo, con el puntero, apretada y con el foco.
            let capas: [(f32, f64, f64); 2] = if enfocada {
                [(0.6, 1.0, 4.0), (0.3, 3.0, 10.0)]
            } else if apretada {
                [(0.5, 1.0, 4.0), (0.25, 2.0, 8.0)]
            } else {
                let k = f64::from(encima);
                [
                    (0.35 + 0.15 * encima, k, 3.0),
                    (0.25 * encima, 2.0 * k, 6.0 * k),
                ]
            };
            for (a, spread, blur) in capas {
                h.trazos.push(Trazo::Sombra {
                    r,
                    radio: tam / 2.0,
                    color: con_alfa(&p.tertiary, a),
                    spread,
                    blur,
                });
            }
        }
        h.trazos.push(Trazo::Caja {
            r,
            radio: tam / 2.0,
            color: if n.mudo { p.on_surface_variant } else { p.tertiary },
        });
    }

    /// El tamaño de la perilla, con su transición de 120 ms: 10, 7 apretada, 14 con el foco.
    fn perilla(&mut self, id: crate::centro::vista::NivelId, o: Objetivo, ahora: Instant) -> f64 {
        let destino = if self.apretado == Some(o) {
            7.0
        } else if self.foco_visible && self.foco == Some(o) {
            14.0
        } else {
            10.0
        };
        let t = self
            .perillas
            .entry(id)
            .or_insert_with(|| morgan_motor::curva::Transicion::new(10.0));
        t.ir(destino, crate::animaciones::dur(Duration::from_millis(120)), ahora);
        f64::from(t.valor(ahora))
    }

    // ── Cuotas ──

    /// La caja de las cuotas (`cc-niveles`, con 8 arriba) con una fila por agente y sus otras
    /// ventanas desplegables; devuelve el alto.
    fn armar_cuotas(&mut self, h: &mut Hoja, filas: &[Cuota], ancho: f64, ahora: Instant) -> f64 {
        let top = ENTRE;
        let inicio = h.trazos.len();
        let mut y = top + 4.0;
        for (i, c) in filas.iter().enumerate() {
            let tocable = !c.otras.is_empty();
            y = self.armar_cuota(h, c, Some(i).filter(|_| tocable), 8.0, ancho - 16.0, y, ahora);
            let f = self
                .revelados_cuotas
                .get(&i)
                .map_or(0.0, |r| r.valor(ahora));
            if f > 0.0 && tocable {
                let mut sub = Hoja::default();
                let mut sy = ENTRE + 4.0;
                for o in &c.otras {
                    sy = self.armar_cuota(&mut sub, o, None, 0.0, ancho - 16.0, sy, ahora);
                }
                sy += 4.0;
                sub.trazos.insert(
                    0,
                    Trazo::Caja {
                        r: Rect::new(0.0, ENTRE, ancho - 16.0, sy),
                        radio: RADIO,
                        color: op(&self.paleta.on_surface, 0.04),
                    },
                );
                let v = visto(sy, f);
                h.incrustar(sub, 8.0, y + v - sy, Rect::new(0.0, y, ancho, y + v));
                y += v;
            }
        }
        y += 4.0;
        h.trazos.insert(
            inicio,
            Trazo::Caja {
                r: Rect::new(0.0, top, ancho, y),
                radio: RADIO,
                color: op(&self.paleta.on_surface, 0.06),
            },
        );
        y
    }

    /// Una fila de cuotas: un nivel con la barra en lugar del slider.
    #[allow(clippy::too_many_arguments)]
    fn armar_cuota(
        &mut self,
        h: &mut Hoja,
        c: &Cuota,
        tocable: Option<usize>,
        x: f64,
        ancho: f64,
        y0: f64,
        ahora: Instant,
    ) -> f64 {
        let p = self.paleta;
        let y = y0 + 4.0;
        let caja_glifo = Rect::new(x, y, x + CUADRO, y + CUADRO);
        let glifo = self.rotulos.linea(&c.glifo, 18.0, 400.0);
        h.trazos.push(Trazo::Glifo {
            l: glifo,
            c: caja_glifo.center(),
            color: if c.alerta { p.error } else { p.on_surface },
        });
        let cx = x + CUADRO + AL_TEXTO;
        let cw = x + ancho - cx;
        let lp = self.rotulos.linea(&c.porcentaje, 12.0, 400.0);
        let wp = self.ancho(&lp);
        let renueva = (!c.renueva.is_empty()).then(|| self.rotulos.linea(&c.renueva, 12.0, 400.0));
        let wr = renueva.as_ref().map_or(0.0, |r| self.ancho(r) + 6.0);
        let nombre = self.rotulos.recortada(&c.nombre, 12.0, (cw - wp - 6.0 - wr).max(0.0));
        let lh = self.alto(&nombre).max(self.alto(&lp));
        let alto_col = lh + 2.0 + 6.0 + BARRA_ALTO + 4.0;
        let cy = y + ((CUADRO - alto_col) / 2.0).floor();
        let color_nombre = match tocable {
            Some(i) => {
                let o = Objetivo::Cuota(i);
                let r = Rect::new(cx, cy, cx + self.ancho(&nombre), cy + lh);
                h.objetivo(o, r, true);
                let l = if self.cuota == Some(i) {
                    1.0
                } else {
                    self.luz(o, ahora)
                };
                op(&p.on_surface, 0.5 + 0.5 * l)
            }
            None => op(&p.on_surface, 0.5),
        };
        h.trazos.push(Trazo::Texto {
            l: nombre,
            x: cx,
            y: cy,
            color: color_nombre,
        });
        if let Some(r) = renueva {
            h.trazos.push(Trazo::Texto {
                l: r,
                x: cx + cw - wp - wr,
                y: cy,
                color: op(&p.on_surface, 0.5),
            });
        }
        h.trazos.push(Trazo::Texto {
            l: lp,
            x: cx + cw - wp,
            y: cy,
            color: if c.alerta { p.error } else { p.tertiary },
        });
        let by = cy + lh + 2.0 + 6.0;
        let canal = Rect::new(cx, by, cx + cw - ESCALA_DERECHA, by + BARRA_ALTO);
        let alfa = if c.apagada { 0.5 } else { 1.0 };
        h.trazos.push(Trazo::Grupo {
            r: canal,
            escala: 1.0,
            dx: 0.0,
            dy: 0.0,
            alfa,
        });
        h.trazos.push(Trazo::Caja {
            r: canal,
            radio: BARRA_ALTO / 2.0,
            color: op(&p.outline_variant, 0.6),
        });
        let lleno = Rect::new(canal.x0, canal.y0, canal.x0 + canal.width() * c.fraccion.clamp(0.0, 1.0), canal.y1);
        if lleno.width() > 0.0 {
            h.trazos.push(Trazo::Sombra {
                r: lleno,
                radio: BARRA_ALTO / 2.0,
                color: con_alfa(&p.tertiary, 0.35),
                spread: 0.0,
                blur: 4.0,
            });
            h.trazos.push(Trazo::Degrade {
                r: lleno,
                radio: BARRA_ALTO / 2.0,
                a: p.primary,
                b: p.tertiary,
                forma: Degrade::Horizontal { de: lleno },
            });
        }
        h.trazos.push(Trazo::Fin);
        y + CUADRO + 4.0
    }

    // ── Reproductor ──

    fn armar_reproductor(&mut self, h: &mut Hoja, y0: f64, ahora: Instant) -> f64 {
        let Some(r) = self.datos.reproductor.clone() else {
            return y0;
        };
        let p = self.paleta;
        let x = 8.0;
        let mut y = y0 + 8.0;
        // Arriba: la carátula y los textos al lado, centrados.
        let tx = if r.caratula.is_some() {
            x + CARATULA + 12.0
        } else {
            x
        };
        let tw = ANCHO - 8.0 - tx;
        let titulo = self.rotulos.linea(&r.titulo, 14.0, 400.0);
        let sub = (!r.subtitulo.is_empty()).then(|| self.rotulos.recortada(&r.subtitulo, 12.0, tw));
        let th = self.alto(&titulo);
        let sh = sub.as_ref().map_or(0.0, |s| self.alto(s) + 4.0);
        let alto_arriba = if r.caratula.is_some() {
            CARATULA.max(th + sh)
        } else {
            th + sh
        };
        let fin = y0 + 8.0 + alto_arriba + BOTON_REPRO + 8.0;
        let caja = Rect::new(0.0, y0, ANCHO, fin);
        h.trazos.push(Trazo::Caja {
            r: caja,
            radio: RADIO,
            color: op(&p.on_surface, 0.06),
        });
        self.armar_ecualizador(h, caja, ahora);
        if let Some(img) = &r.caratula {
            h.trazos.push(Trazo::Imagen {
                img: img.clone(),
                r: Rect::new(x, y + ((alto_arriba - CARATULA) / 2.0).floor(), x + CARATULA, y + ((alto_arriba - CARATULA) / 2.0).floor() + CARATULA),
                radio: RADIO,
                alfa: 1.0,
            });
        }
        let ty = y + ((alto_arriba - th - sh) / 2.0).floor();
        // El título en marquesina si no entra.
        let sobra = (self.ancho(&titulo) - tw).max(0.0);
        self.sobra_titulo = sobra;
        let t = ahora.saturating_duration_since(self.marquesina).as_secs_f64();
        let corrido = marquesina::posicion(t, sobra).round();
        h.trazos.push(Trazo::Recorte {
            r: Rect::new(tx, ty, tx + tw, ty + th),
            radio: 0.0,
        });
        h.trazos.push(Trazo::Texto {
            l: titulo,
            x: tx - corrido,
            y: ty,
            color: p.on_surface,
        });
        h.trazos.push(Trazo::Fin);
        if let Some(s) = sub {
            h.trazos.push(Trazo::Texto {
                l: s,
                x: tx,
                y: ty + th + 4.0,
                color: op(&p.on_surface, 0.5),
            });
        }
        y += alto_arriba;

        // Los botones, centrados, cada 6.
        let mut botones = Vec::new();
        if let Some(m) = r.mezcla {
            botones.push((BotonRepro::Mezcla, self.datos.iconos.mezcla.clone(), true, m));
        }
        botones.push((BotonRepro::Anterior, self.datos.iconos.anterior.clone(), r.puede_anterior, true));
        let play = if r.sonando {
            self.datos.iconos.pausa.clone()
        } else {
            self.datos.iconos.play.clone()
        };
        botones.push((BotonRepro::Play, play, true, true));
        botones.push((BotonRepro::Siguiente, self.datos.iconos.siguiente.clone(), r.puede_siguiente, true));
        if let Some(rep) = &r.repetir {
            let icono = if rep == "Track" {
                self.datos.iconos.repetir_uno.clone()
            } else {
                self.datos.iconos.repetir.clone()
            };
            botones.push((BotonRepro::Repetir, icono, true, rep != "None"));
        }
        let total = botones.len() as f64 * BOTON_REPRO + (botones.len() as f64 - 1.0) * 6.0;
        let mut bx = ((ANCHO - total) / 2.0).floor();
        for (b, icono, habilitado, encendido) in botones {
            let rb = Rect::new(bx, y, bx + BOTON_REPRO, y + BOTON_REPRO);
            let o = Objetivo::Repro(b);
            let color = if !habilitado {
                op(&p.on_surface, APAGADO)
            } else {
                let c = mezclar(&p.on_surface, &p.primary, self.luz(o, ahora));
                op(&c, if encendido { 1.0 } else { APAGADO })
            };
            if habilitado {
                h.objetivo(o, rb, true);
            }
            if let Some(img) = icono {
                let c = rb.center();
                let ri = Rect::new(c.x - ICONO_REPRO / 2.0, c.y - ICONO_REPRO / 2.0, c.x + ICONO_REPRO / 2.0, c.y + ICONO_REPRO / 2.0);
                h.trazos.push(Trazo::escalar(rb, self.animos.escala(o, ahora)));
                h.trazos.push(Trazo::Mascara { img, r: ri, color });
                h.trazos.push(Trazo::Fin);
            }
            bx += BOTON_REPRO + 6.0;
        }
        fin
    }

    /// Barras finas desde abajo hasta el 75 % del alto, en espejo, cada una con su tramo del
    /// degradé de toda la tarjeta; se desvanece en 600 ms.
    fn armar_ecualizador(&mut self, h: &mut Hoja, caja: Rect, ahora: Instant) {
        let alfa = self.ecualizador.0.valor(ahora) * 0.22;
        let niveles = &self.ecualizador.1;
        let n = niveles.len();
        if alfa <= 0.0 || n == 0 {
            return;
        }
        let p = self.paleta;
        const SEPARACION: f64 = 3.0;
        let w = ((caja.width() - SEPARACION * (n as f64 - 1.0)) / n as f64).max(1.0);
        h.trazos.push(Trazo::Recorte {
            r: caja,
            radio: RADIO,
        });
        h.trazos.push(Trazo::Grupo {
            r: caja,
            escala: 1.0,
            dx: 0.0,
            dy: 0.0,
            alfa,
        });
        for (i, v) in niveles.iter().enumerate() {
            let alto = (v * caja.height() * 0.75).max(w);
            let x = caja.x0 + i as f64 * (w + SEPARACION);
            let barra = Rect::new(x, caja.y1 - alto, x + w, caja.y1);
            h.trazos.push(Trazo::Degrade {
                r: barra,
                radio: w / 2.0,
                a: p.primary,
                b: p.tertiary,
                forma: Degrade::Horizontal { de: caja },
            });
        }
        h.trazos.push(Trazo::Fin);
        h.trazos.push(Trazo::Fin);
    }

    /// El campo debajo de una opción (`cc-clave`, 2 arriba y abajo): la caja del launcher
    /// con el glifo, lo escrito y un botón, y el aviso debajo.
    #[allow(clippy::too_many_arguments)]
    fn armar_campo(
        &mut self,
        h: &mut Hoja,
        l: ListaId,
        c: &crate::centro::vista::Campo,
        x: f64,
        ancho: f64,
        y0: f64,
        ahora: Instant,
    ) -> f64 {
        let p = self.paleta;
        let mut y = y0 + 2.0;
        if let Some(caja) = &c.caja {
            let entrada = self.campos.entry(l).or_default();
            entrada.preparar(c.generacion);
            let ver = entrada.ver;
            let alto_texto = {
                let m = self.rotulos.linea("M", 14.0, 400.0);
                self.alto(&m)
            };
            let alto = 6.0 + alto_texto + 6.0;
            let r = Rect::new(x, y, x + ancho, y + alto);
            h.trazos.push(Trazo::Caja {
                r,
                radio: RADIO,
                color: op(&p.on_surface, 0.06),
            });
            let glifo = self.rotulos.linea(caja.glifo, pt_glifo(), 400.0);
            let col = Rect::new(x + 4.0, y + 6.0, x + 4.0 + COLUMNA_GLIFO, y + 6.0 + alto_texto);
            h.trazos.push(Trazo::Glifo {
                l: glifo,
                c: col.center(),
                color: op(&p.on_surface, 0.7),
            });
            // El botón de la derecha, en la columna de los tildes.
            let mut fin_texto = x + ancho - 10.0;
            if let Some(b) = caja.boton {
                let s = match b {
                    BotonCampo::Ojo if ver => "\u{F0209}",
                    BotonCampo::Ojo => "\u{F0208}",
                    BotonCampo::Tilde(_) => "󰄬",
                };
                let lb = self.rotulos.linea(s, pt_glifo(), 400.0);
                let w = self.ancho(&lb);
                let rb = Rect::new(x + ancho - 10.0 - w, y + 6.0, x + ancho - 10.0, y + 6.0 + alto_texto);
                let o = Objetivo::BotonCampo(l);
                let color = if caja.ocupado {
                    op(&p.on_surface, 0.5)
                } else {
                    mezclar(&p.on_surface, &p.primary, self.luz(o, ahora))
                };
                if !caja.ocupado {
                    h.objetivo(o, rb, matches!(b, BotonCampo::Tilde(_)));
                }
                h.trazos.push(Trazo::Glifo {
                    l: lb,
                    c: rb.center(),
                    color,
                });
                fin_texto = rb.x0 - 10.0;
            }
            let tx = col.x1 + TRAS_GLIFO + 10.0;
            let zona = Rect::new(tx, y + 6.0, fin_texto, y + 6.0 + alto_texto);
            let o = Objetivo::Campo(l);
            let editable = caja.fijo.is_none() && !caja.ocupado;
            if editable {
                h.objetivo(o, zona, true);
            }
            let enfocado = self.foco == Some(o);
            self.armar_entrada(h, l, caja, zona, enfocado, ahora);
            y += alto;
        }
        if let Some((aviso, error)) = &c.aviso {
            y += 4.0;
            let ancho_aviso = ancho - 50.0 - 4.0;
            let la = self.rotulos.parrafo(&glib_escape(aviso), 12.0, ancho_aviso, 0);
            let al = texto::caja(&la).1 / self.escala_f();
            h.trazos.push(Trazo::Texto {
                l: la,
                x: x + 50.0,
                y,
                color: if *error { p.error } else { op(&p.on_surface, 0.5) },
            });
            y += al + 2.0;
        }
        y + 2.0
    }
}

/// El tamaño de la letra que no fija la hoja: la de GTK (11 pt) en píxeles.
fn pt_glifo() -> f32 {
    texto::pt(11.0)
}

/// Un texto plano como markup (para los párrafos, que lo leen así).
pub(super) fn glib_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Si el título del reproductor no entra y anda en marquesina.
pub(super) fn titulo_no_entra(ui: &Ui) -> bool {
    ui.sobra_titulo > 0.0
}

/// Que el revelado de `k` vaya hacia `abrir`.
pub(super) fn revelar<K: std::hash::Hash + Eq>(
    m: &mut std::collections::HashMap<K, Revelado>,
    k: K,
    abrir: bool,
    dura: Duration,
    ahora: Instant,
) {
    m.entry(k)
        .or_insert_with(|| Revelado::new(!abrir))
        .ir(abrir, dura, ahora);
}
