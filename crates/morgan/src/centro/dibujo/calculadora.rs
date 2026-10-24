//! La calculadora con las piezas del centro: la caja de los paneles centrada en la salida, la
//! pantalla en el recuadro con el tinte de las cajas (la cinta de las cuentas hechas arriba y
//! la de ahora abajo) y la botonera de las celdas del calendario. Qué hay escrito y qué dio
//! lo lleva el hilo de GTK (`calculadora/mod.rs`, con `qalc`); acá viven las teclas que se
//! iluminan, el scroll de la cinta y el puntero.
//!
//! Las medidas, de la versión con GTK (`.calculadora` en `estilo.css` hasta 0.1.1): 320 de
//! ancho con el margen del brillo, 8 entre la pantalla y la botonera; la pantalla con 4 de
//! relleno, la cinta de 104 de alto con filas de 4 y 10, y la cuenta de ahora con 8 y 10; la
//! botonera con 8 de relleno, 4 entre teclas y cada tecla con 10 arriba y abajo.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use morgan_motor::color::{Color, Paleta, con_alfa};
use morgan_motor::forma::{Borde, panel};
use morgan_motor::texto;
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Point, Rect};
use smithay_client_toolkit::seat::keyboard::Keysym;

use super::ajustes::parejas;
use super::trazo::{self, Degrade, Trazo};
use super::{Animos, Hoja, Objetivo, RADIO, Rotulos};
use crate::calculadora::modelo::{self, Accion, BOTONERA, Clase};
use crate::centro::vista::{EventoCalculadora, VistaCalculadora};

/// De borde a borde del anillo (los 320 de GTK traían el margen del brillo).
const ANCHO: f64 = 320.0 - 2.0 * MARGEN;
const MARGEN: f64 = 8.0;
const BORDE_Y_RELLENO: f64 = 16.0;
const ENTRE: f64 = 8.0;
const PANTALLA: f64 = 4.0;
const ALTO_CINTA: f64 = 104.0;
const FILA_X: f64 = 10.0;
const FILA_Y: f64 = 4.0;
const ENTRE_FILA: f64 = 6.0;
/// `max-width-chars` del resultado de una fila, en celdas de la monoespaciada a 13 px.
const RESULTADO_MAX: f64 = 16.0 * 7.8;
const VISOR_X: f64 = 10.0;
const VISOR_Y: f64 = 8.0;
const BOTONERA_RELLENO: f64 = 8.0;
const ENTRE_TECLAS: f64 = 4.0;
const TECLA_Y: f64 = 10.0;

/// Lo que una tecla del teclado ilumina la suya.
const ILUMINADA: Duration = Duration::from_millis(120);

fn op(c: &Color, a: f32) -> Color {
    con_alfa(c, c[3] * a)
}

pub struct CalculadoraUi {
    pub paleta: Paleta,
    pub opacidad: f32,
    pub borde: Borde,
    rotulos: Rotulos,
    animos: Animos,
    pub tam: (u32, u32),
    vista: VistaCalculadora,
    /// Desde arriba de la cinta; con una cuenta nueva, al fondo.
    scroll: f64,
    al_fondo: bool,
    iluminadas: HashMap<(usize, usize), Instant>,
    puntero: Option<(f64, f64)>,
    encima: Option<Objetivo>,
    apretado: Option<Objetivo>,
    afuera: bool,
    hoja: Hoja,
    /// La cinta va aparte: se recorta a su lugar y se corre por su scroll.
    cinta: Hoja,
    vista_cinta: Rect,
    caja: Rect,
}

impl CalculadoraUi {
    pub fn new(paleta: Paleta, opacidad: f32, borde: Borde) -> Self {
        CalculadoraUi {
            paleta,
            opacidad,
            borde,
            rotulos: Rotulos::new(),
            animos: Animos::default(),
            tam: (0, 0),
            vista: VistaCalculadora::default(),
            scroll: 0.0,
            al_fondo: true,
            iluminadas: HashMap::new(),
            puntero: None,
            encima: None,
            apretado: None,
            afuera: false,
            hoja: Hoja::default(),
            cinta: Hoja::default(),
            vista_cinta: Rect::ZERO,
            caja: Rect::ZERO,
        }
    }

    /// Abre con la última cuenta a la vista, como una de bolsillo.
    pub fn abrir(&mut self) {
        self.al_fondo = true;
        self.puntero = None;
        self.encima = None;
        self.apretado = None;
    }

    pub fn poner(&mut self, v: VistaCalculadora) {
        if v.cinta.len() != self.vista.cinta.len() || v.cinta.last() != self.vista.cinta.last() {
            self.al_fondo = true;
        }
        self.vista = v;
    }

    pub fn panel(&self) -> Rect {
        self.caja
    }

    fn escala(&self) -> f64 {
        f64::from(self.rotulos.escala())
    }

    fn alto(&self, l: &morgan_motor::parley::Layout<u8>) -> f64 {
        texto::alto_de_linea(l) / self.escala()
    }

    fn ancho(&self, l: &morgan_motor::parley::Layout<u8>) -> f64 {
        texto::caja(l).0 / self.escala()
    }

    pub fn armar(&mut self, escala: f32, ahora: Instant) {
        self.rotulos.poner_escala(escala);
        let p = self.paleta;
        // Lo iluminado por el teclado se suelta a su tiempo.
        let fin = crate::animaciones::dur(ILUMINADA);
        let sueltas: Vec<(usize, usize)> = self
            .iluminadas
            .iter()
            .filter(|(_, t)| ahora.saturating_duration_since(**t) >= fin)
            .map(|(k, _)| *k)
            .collect();
        for k in sueltas {
            self.iluminadas.remove(&k);
            if self.apretado != Some(Objetivo::Tecla(k.0, k.1)) {
                self.animos.apretar(Objetivo::Tecla(k.0, k.1), false, ahora);
            }
        }

        let ancho = ANCHO - 2.0 * BORDE_Y_RELLENO;
        let mut h = Hoja::default();
        // La pantalla: la cinta y, debajo, la cuenta de ahora.
        let grande_muestra = self.rotulos.linea("0", 26.0, 400.0);
        let chica_muestra = self.rotulos.linea("0", 13.0, 400.0);
        let alto_visor =
            2.0 * VISOR_Y + self.alto(&grande_muestra) + 2.0 + self.alto(&chica_muestra);
        let alto_pantalla = 2.0 * PANTALLA + ALTO_CINTA + alto_visor;
        h.trazos.push(Trazo::Caja {
            r: Rect::new(0.0, 0.0, ancho, alto_pantalla),
            radio: RADIO,
            color: op(&p.on_surface, 0.06),
        });
        let interior = ancho - 2.0 * PANTALLA;
        self.armar_cinta(interior, ahora);
        let visor = Rect::new(
            PANTALLA,
            PANTALLA + ALTO_CINTA,
            PANTALLA + interior,
            PANTALLA + ALTO_CINTA + alto_visor,
        );
        let luz = self.luz(Objetivo::Visor, ahora);
        h.objetivo(Objetivo::Visor, visor, false);
        h.trazos.push(Trazo::Caja {
            r: visor,
            radio: RADIO,
            color: op(&p.on_surface, 0.03 * luz),
        });
        let max = visor.width() - 2.0 * VISOR_X;
        let texto_grande = if self.vista.vacia {
            "0"
        } else {
            &self.vista.grande.clone()
        };
        let g = self.rotulos.recortada_al_inicio(texto_grande, 26.0, max);
        let (wg, ag) = (self.ancho(&g), self.alto(&g));
        h.trazos.push(Trazo::Texto {
            l: g,
            x: visor.x1 - VISOR_X - wg,
            y: visor.y0 + VISOR_Y,
            color: if self.vista.vacia {
                op(&p.on_surface, 0.5)
            } else {
                p.on_surface
            },
        });
        if !self.vista.chica.is_empty() {
            let c = self
                .rotulos
                .recortada_al_inicio(&self.vista.chica.clone(), 13.0, max);
            let wc = self.ancho(&c);
            h.trazos.push(Trazo::Texto {
                l: c,
                x: visor.x1 - VISOR_X - wc,
                y: visor.y0 + VISOR_Y + ag + 2.0,
                color: if self.vista.copiado {
                    p.tertiary
                } else {
                    op(&p.on_surface, 0.5)
                },
            });
        }

        // La botonera.
        let y0 = alto_pantalla + ENTRE;
        let muestra = self.rotulos.linea("0", 16.0, 400.0);
        let alto_tecla = 2.0 * TECLA_Y + self.alto(&muestra);
        let filas = BOTONERA.len() as f64;
        let alto_botonera =
            2.0 * BOTONERA_RELLENO + filas * alto_tecla + (filas - 1.0) * ENTRE_TECLAS;
        h.trazos.push(Trazo::Caja {
            r: Rect::new(0.0, y0, ancho, y0 + alto_botonera),
            radio: RADIO,
            color: op(&p.on_surface, 0.06),
        });
        let columnas = parejas(ancho - 2.0 * BOTONERA_RELLENO, 4, ENTRE_TECLAS);
        for (f, fila) in BOTONERA.iter().enumerate() {
            for (c, t) in fila.iter().enumerate() {
                let (x, w) = columnas[c];
                let x = BOTONERA_RELLENO + x;
                let y = y0 + BOTONERA_RELLENO + f as f64 * (alto_tecla + ENTRE_TECLAS);
                let r = Rect::new(x, y, x + w, y + alto_tecla);
                let o = Objetivo::Tecla(f, c);
                h.objetivo(o, r, false);
                let luz = self.luz(o, ahora);
                let hundida = self.apretado == Some(o) || self.iluminadas.contains_key(&(f, c));
                h.trazos
                    .push(Trazo::escalar(r, self.animos.escala(o, ahora)));
                let fondo = if hundida {
                    op(&p.primary, 0.2)
                } else {
                    op(&p.primary, 0.14 * luz)
                };
                if t.clase == Clase::Igual {
                    h.trazos.push(Trazo::Sombra {
                        r,
                        radio: RADIO,
                        color: op(&p.tertiary, 0.3),
                        spread: 0.0,
                        blur: 6.0,
                    });
                    h.trazos.push(Trazo::Degrade {
                        r,
                        radio: RADIO,
                        a: p.primary,
                        b: p.tertiary,
                        forma: Degrade::Diagonal,
                    });
                } else {
                    h.trazos.push(Trazo::Caja {
                        r,
                        radio: RADIO,
                        color: fondo,
                    });
                }
                let color = match t.clase {
                    Clase::Numero => p.on_surface,
                    Clase::Operador => p.tertiary,
                    Clase::Funcion => p.on_surface_variant,
                    Clase::Igual => p.surface,
                };
                let l = self.rotulos.linea(t.etiqueta, 16.0, 400.0);
                // El ícono de borrar (de la Nerd Font) se centra por su tinta; los números y
                // los signos, por su caja, como las etiquetas de GTK (la coma, abajo).
                if t.etiqueta
                    .chars()
                    .any(|c| ('\u{E000}'..='\u{F8FF}').contains(&c) || c >= '\u{F0000}')
                {
                    h.trazos.push(Trazo::Glifo {
                        l,
                        c: r.center(),
                        color,
                    });
                } else {
                    let (w, a) = (self.ancho(&l), self.alto(&l));
                    let c = r.center();
                    h.trazos.push(Trazo::Texto {
                        l,
                        x: (c.x - w / 2.0).floor(),
                        y: (c.y - a / 2.0).floor(),
                        color,
                    });
                }
                h.trazos.push(Trazo::Fin);
            }
        }
        h.alto = y0 + alto_botonera;
        self.hoja = h;

        let alto = self.hoja.alto + 2.0 * BORDE_Y_RELLENO;
        let (w, hh) = (f64::from(self.tam.0), f64::from(self.tam.1));
        // Centrada como un widget de GTK: la caja con su margen, en un píxel entero.
        let x0 = ((w - ANCHO - 2.0 * MARGEN) / 2.0).floor() + MARGEN;
        let y0 = ((hh - alto - 2.0 * MARGEN) / 2.0).floor() + MARGEN;
        self.caja = Rect::new(x0, y0, x0 + ANCHO, y0 + alto);
        let c = self.contenido();
        self.vista_cinta = Rect::new(
            c.x0 + PANTALLA,
            c.y0 + PANTALLA,
            c.x0 + PANTALLA + interior,
            c.y0 + PANTALLA + ALTO_CINTA,
        );
        if let Some((x, y)) = self.puntero {
            self.encima = self.bajo(x, y);
        }
        self.rotulos.barrer();
    }

    /// Las cuentas hechas, la última abajo; pocas quedan pegadas a la de ahora.
    fn armar_cinta(&mut self, ancho: f64, ahora: Instant) {
        let p = self.paleta;
        let mut h = Hoja::default();
        let muestra = self.rotulos.linea("0", 13.0, 400.0);
        let alto_fila = 2.0 * FILA_Y + self.alto(&muestra);
        let n = self.vista.cinta.len() as f64;
        let total = n * alto_fila;
        let mut y = (ALTO_CINTA - total).max(0.0);
        let cinta = self.vista.cinta.clone();
        for (i, (cuenta, resultado)) in cinta.iter().enumerate() {
            let r = Rect::new(0.0, y, ancho, y + alto_fila);
            let o = Objetivo::Cuenta(i);
            h.objetivo(o, r, false);
            let luz = self.luz(o, ahora);
            let fondo = if self.apretado == Some(o) {
                op(&p.primary, 0.2)
            } else {
                op(&p.primary, 0.14 * luz)
            };
            h.trazos.push(Trazo::Caja {
                r,
                radio: RADIO,
                color: fondo,
            });
            let lr = self
                .rotulos
                .recortada(&format!("= {resultado}"), 13.0, RESULTADO_MAX);
            let wr = self.ancho(&lr);
            let xr = r.x1 - FILA_X - wr;
            let max = (xr - ENTRE_FILA - FILA_X).max(0.0);
            let lc = self.rotulos.recortada_al_inicio(cuenta, 13.0, max);
            let wc = self.ancho(&lc);
            h.trazos.push(Trazo::Texto {
                l: lc,
                x: xr - ENTRE_FILA - wc,
                y: y + FILA_Y,
                color: op(&p.on_surface, 0.5),
            });
            h.trazos.push(Trazo::Texto {
                l: lr,
                x: xr,
                y: y + FILA_Y,
                color: p.on_surface,
            });
            y += alto_fila;
        }
        h.alto = y;
        let max = (h.alto - ALTO_CINTA).max(0.0);
        if self.al_fondo {
            self.scroll = max;
            self.al_fondo = false;
        }
        self.scroll = self.scroll.clamp(0.0, max);
        self.cinta = h;
    }

    fn luz(&mut self, o: Objetivo, ahora: Instant) -> f32 {
        self.animos.luz(o, self.encima == Some(o), ahora)
    }

    fn contenido(&self) -> Rect {
        self.caja.inset(-BORDE_Y_RELLENO)
    }

    pub fn pintar(&self, s: &mut Scene, escala: f64, bajada: f64) {
        s.reset();
        let tr = Affine::scale(escala) * Affine::translate((0.0, -bajada.round()));
        panel(s, tr, &self.paleta, self.opacidad, self.caja, self.borde);
        let c = self.contenido();
        trazo::pintar(
            s,
            &self.hoja.trazos,
            tr * Affine::translate((c.x0, c.y0)),
            escala,
        );
        let fill = morgan_motor::vello::peniko::Fill::NonZero;
        s.push_clip_layer(fill, tr, &self.vista_cinta);
        let base = tr
            * Affine::translate((
                self.vista_cinta.x0,
                self.vista_cinta.y0 - self.scroll.round(),
            ));
        trazo::pintar(s, &self.cinta.trazos, base, escala);
        s.pop_layer();
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        self.animos.anda(ahora) || !self.iluminadas.is_empty()
    }

    // ── Teclado ──

    pub fn tecla(
        &mut self,
        k: Keysym,
        utf8: Option<&str>,
        ctrl: bool,
        alt: bool,
    ) -> Vec<EventoCalculadora> {
        let mut v = Vec::new();
        let mut accion = |yo: &mut Self, a: Accion, etiqueta: &str| {
            let pos = BOTONERA.iter().enumerate().find_map(|(f, fila)| {
                fila.iter()
                    .position(|t| t.etiqueta == etiqueta)
                    .map(|c| (f, c))
            });
            if let Some(p) = pos {
                yo.iluminar(p);
            }
            v.push(EventoCalculadora::Accion(a));
        };
        match k {
            Keysym::Escape => return vec![EventoCalculadora::Cerrada],
            Keysym::Return | Keysym::KP_Enter => accion(self, Accion::Igual, "="),
            Keysym::BackSpace => accion(self, Accion::Borrar, "󰭜"),
            Keysym::Delete => accion(self, Accion::Limpiar, "C"),
            Keysym::c | Keysym::C if ctrl => return vec![EventoCalculadora::Copiar],
            // Con Ctrl o Alt no es texto: lo demás no hace nada.
            _ if ctrl || alt => {}
            Keysym::equal | Keysym::KP_Equal => accion(self, Accion::Igual, "="),
            Keysym::KP_Decimal | Keysym::KP_Separator => {
                accion(self, Accion::Escribir(","), ",");
            }
            _ => {
                let Some(s) = utf8
                    .and_then(|u| u.chars().next())
                    .and_then(modelo::del_teclado)
                else {
                    return v;
                };
                // `(` y `)` del teclado se escriben tal cual; la tecla es la de los dos.
                if s == "(" || s == ")" {
                    self.iluminar((0, 1));
                } else if let Some(p) = modelo::tecla_de(&s) {
                    self.iluminar(p);
                }
                v.push(EventoCalculadora::Escribir(s));
            }
        }
        v
    }

    fn iluminar(&mut self, (f, c): (usize, usize)) {
        let ahora = Instant::now();
        self.iluminadas.insert((f, c), ahora);
        self.animos.apretar(Objetivo::Tecla(f, c), true, ahora);
    }

    // ── Puntero ──

    fn bajo(&self, x: f64, y: f64) -> Option<Objetivo> {
        if self.vista_cinta.contains((x, y)) {
            let p = Point::new(
                x - self.vista_cinta.x0,
                y - self.vista_cinta.y0 + self.scroll.round(),
            );
            return self
                .cinta
                .objetivos
                .iter()
                .find(|(_, r)| r.contains(p))
                .map(|(o, _)| *o);
        }
        let c = self.contenido();
        let p = Point::new(x - c.x0, y - c.y0);
        self.hoja
            .objetivos
            .iter()
            .rev()
            .find(|(_, r)| r.contains(p))
            .map(|(o, _)| *o)
    }

    pub fn mover(&mut self, x: f64, y: f64) {
        self.puntero = Some((x, y));
        self.encima = self.bajo(x, y);
    }

    pub fn salir(&mut self) {
        self.puntero = None;
        self.encima = None;
    }

    pub fn apretar(&mut self, x: f64, y: f64) {
        self.afuera = !self.caja.contains((x, y));
        self.apretado = self.bajo(x, y);
        if let Some(o) = self.apretado {
            self.animos.apretar(o, true, Instant::now());
        }
    }

    /// Un click en una tecla la hace; en la pantalla copia el resultado; en una fila de la
    /// cinta escribe su resultado. Uno afuera de la caja cierra.
    pub fn soltar(&mut self, x: f64, y: f64, izquierdo: bool) -> Vec<EventoCalculadora> {
        let apretado = self.apretado.take();
        if let Some(o) = apretado {
            self.animos.apretar(o, false, Instant::now());
        }
        if apretado.is_none() && self.afuera && !self.caja.contains((x, y)) {
            return vec![EventoCalculadora::Cerrada];
        }
        let Some(o) = apretado.filter(|o| izquierdo && self.bajo(x, y) == Some(*o)) else {
            return Vec::new();
        };
        match o {
            Objetivo::Tecla(f, c) => vec![EventoCalculadora::Accion(BOTONERA[f][c].accion)],
            Objetivo::Visor => vec![EventoCalculadora::Copiar],
            Objetivo::Cuenta(i) => self
                .vista
                .cinta
                .get(i)
                .map(|(_, r)| vec![EventoCalculadora::Escribir(r.clone())])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    pub fn rueda(&mut self, dy: f64, discreto: bool) {
        let paso = if discreto {
            ALTO_CINTA.powf(2.0 / 3.0)
        } else {
            1.0
        };
        let max = (self.cinta.alto - ALTO_CINTA).max(0.0);
        self.scroll = (self.scroll + dy * paso).clamp(0.0, max);
    }
}
