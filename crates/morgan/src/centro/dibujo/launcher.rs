//! El launcher con las piezas del centro: la caja de los paneles centrada en la salida, el
//! buscador con el campo del centro (`campo.rs`) y la lista con el recuadro de la elegida, que
//! se desliza, y el riel bicolor. Lo que se busca y lo que hace cada fila lo decide el hilo de
//! GTK (`launcher/mod.rs`); acá vive lo escrito, la elegida, el scroll y el puntero.
//!
//! Las medidas, de la versión con GTK (`.launcher` en `estilo.css` hasta 0.1.1): 600 px de
//! borde a borde, 14 de relleno y 2 de anillo; el buscador con 8 y 12 de relleno y 8 debajo;
//! seis filas de 36 (el ícono de 24 con 6 arriba y abajo, 10 a los costados) con 4 entre
//! ellas, y el riel de 8 a 4 de la lista. La letra, a 10 pt.

use std::time::{Duration, Instant};

use morgan_motor::color::{Paleta, con_alfa, mezclar};
use morgan_motor::forma::{Borde, panel};
use morgan_motor::texto;
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Rect};
use smithay_client_toolkit::seat::keyboard::Keysym;

use super::Rotulos;
use super::campo::Entrada;
use super::trazo::{self, Degrade, Trazo};
use crate::centro::vista::{EventoLauncher, IconoFila, ListaLauncher, TipoFila, Turno};

/// De borde a borde del anillo.
const ANCHO: f64 = 600.0;
/// El lugar del brillo, alrededor de la caja.
const MARGEN: f64 = 8.0;
const BORDE_Y_RELLENO: f64 = 16.0;
const CONTENIDO: f64 = ANCHO - 2.0 * BORDE_Y_RELLENO;
const RADIO: f64 = 4.0;

const BUSCADOR_X: f64 = 12.0;
const BUSCADOR_Y: f64 = 8.0;
/// Entre el prompt, el separador y lo escrito.
const SEPARACION: f64 = 6.0;
const BAJO_BUSCADOR: f64 = 8.0;

pub const FILAS: usize = 6;
const ALTO_FILA: f64 = 36.0;
const ESPACIO: f64 = 4.0;
const PASO_FILA: f64 = ALTO_FILA + ESPACIO;
const ALTO_LISTA: f64 = FILAS as f64 * ALTO_FILA + (FILAS - 1) as f64 * ESPACIO;
const FILA_X: f64 = 10.0;
const FILA_Y: f64 = 6.0;
const ICONO: f64 = 24.0;
const ICONO_TEXTO: f64 = 8.0;

const RIEL: f64 = 8.0;
const RIEL_SEPARACION: f64 = 4.0;
const ANCHO_LISTA: f64 = CONTENIDO - RIEL - RIEL_SEPARACION;
/// Lo que se ve del degradé en el riel (el `30` hex de rofi).
const TINTE: f32 = 0x30 as f32 / 255.0;
const ANCHO_RIEL: f64 = 4.0;
const PERILLA_MIN: f64 = 8.0;

/// El recuadro de la elegida y el scroll del teclado.
const RECUADRO: Duration = Duration::from_millis(100);
const SCROLL: Duration = Duration::from_millis(120);

/// Roles de los tramos de una fila: el nombre, lo resaltado del nombre, el genérico y lo
/// resaltado del genérico.
const NOMBRE: u8 = 0;
const NOMBRE_RESALTADO: u8 = 1;
const GENERICO: u8 = 2;
const GENERICO_RESALTADO: u8 = 3;

fn letra() -> f32 {
    texto::pt(10.0)
}

/// El `size="small"` de Pango.
fn letra_chica() -> f32 {
    letra() / 1.2
}

pub(super) fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

pub(super) fn avance(desde: Instant, dura: Duration, ahora: Instant) -> f64 {
    let d = crate::animaciones::dur(dura).as_secs_f64();
    if d <= 0.0 {
        return 1.0;
    }
    ahora.saturating_duration_since(desde).as_secs_f64() / d
}

/// Cómo llega el recuadro a la elegida.
#[derive(Clone, Copy, PartialEq)]
enum Paso {
    /// Directo (al filtrar).
    Saltar,
    /// Desde donde estaba (las flechas, el mouse).
    Deslizar,
    /// De un extremo al otro: no cruza la lista, aparece allá.
    Aparecer,
}

/// Lo que pide el launcher a su hilo.
pub enum Pedido {
    Gtk(EventoLauncher),
    /// Lo del portapapeles al campo.
    Pegar,
}

/// Un valor que va hacia otro con la salida cúbica: cada paso sigue al destino del anterior.
pub(super) struct Recorrido {
    desde: f64,
    pub(super) hasta: f64,
    t0: Instant,
    dura: Duration,
}

impl Recorrido {
    pub(super) fn new(dura: Duration) -> Self {
        Recorrido {
            desde: 0.0,
            hasta: 0.0,
            t0: Instant::now(),
            dura,
        }
    }

    pub(super) fn valor(&self, ahora: Instant) -> f64 {
        self.desde + (self.hasta - self.desde) * ease_out(avance(self.t0, self.dura, ahora))
    }

    pub(super) fn ir(&mut self, v: f64, ahora: Instant) {
        self.desde = self.valor(ahora);
        self.hasta = v;
        self.t0 = ahora;
    }

    pub(super) fn poner(&mut self, v: f64) {
        self.desde = v;
        self.hasta = v;
    }

    pub(super) fn anda(&self, ahora: Instant) -> bool {
        self.desde != self.hasta && avance(self.t0, self.dura, ahora) < 1.0
    }
}

/// Dónde se apretó el botón.
#[derive(Clone, Copy, PartialEq)]
enum Apretado {
    Fila(usize),
    Riel,
    Adentro,
    Afuera,
}

pub struct LauncherUi {
    pub paleta: Paleta,
    pub opacidad: f32,
    pub borde: Borde,
    rotulos: Rotulos,
    pub tam: (u32, u32),
    prompt: String,
    placeholder: String,
    simbolos: bool,
    entrada: Entrada,
    /// Lo que se corre lo escrito para que el cursor quede a la vista.
    corrido: f64,
    turno: Turno,
    lista: ListaLauncher,
    elegida: usize,
    paso: Paso,
    /// Dónde estaba el recuadro al empezar a moverse, y desde cuándo.
    recuadro: (f64, Instant),
    dibujado: f64,
    scroll: Recorrido,
    /// Enter antes de que llegue la lista de lo escrito: se elige con ella.
    pendiente: bool,
    /// La elegida con su destello (y la versión de su lista): se pinta un cuadro y, con el
    /// siguiente, se hace y se cierra.
    destello: Option<(usize, u64, bool)>,
    puntero: Option<(f64, f64)>,
    apretado: Option<Apretado>,
    trazos: Vec<Trazo>,
    caja: Rect,
}

impl LauncherUi {
    pub fn new(paleta: Paleta, opacidad: f32, borde: Borde) -> Self {
        LauncherUi {
            paleta,
            opacidad,
            borde,
            rotulos: Rotulos::new(),
            tam: (0, 0),
            prompt: String::new(),
            placeholder: String::new(),
            simbolos: false,
            entrada: Entrada::default(),
            corrido: 0.0,
            turno: (0, 0),
            lista: ListaLauncher {
                texto: (0, 0),
                version: 0,
                filas: Vec::new(),
                elegida: 0,
                en_carpeta: false,
                vaciar: None,
            },
            elegida: 0,
            paso: Paso::Saltar,
            recuadro: (0.0, Instant::now()),
            dibujado: 0.0,
            scroll: Recorrido::new(SCROLL),
            pendiente: false,
            destello: None,
            puntero: None,
            apretado: None,
            trazos: Vec::new(),
            caja: Rect::ZERO,
        }
    }

    /// Un modo nuevo (o el mismo, al abrir): el prompt; lo escrito lo vacía la lista que sigue.
    pub fn abrir(&mut self, prompt: String, simbolos: bool) {
        self.prompt = prompt;
        self.simbolos = simbolos;
        self.destello = None;
        self.pendiente = false;
        self.apretado = None;
    }

    /// Al cerrar: lo que es de una apertura no queda para la próxima.
    pub fn cerrar(&mut self) {
        self.destello = None;
        self.pendiente = false;
        self.apretado = None;
        self.puntero = None;
        self.entrada = Entrada::default();
        self.lista.filas.clear();
    }

    pub fn poner_lista(&mut self, l: ListaLauncher) -> Vec<Pedido> {
        // Con el destello, lo elegido ya está decidido.
        if self.destello.is_some() {
            return Vec::new();
        }
        if let Some(p) = &l.vaciar {
            self.placeholder = p.clone();
            self.entrada = Entrada::default();
            self.corrido = 0.0;
            self.turno = l.texto;
        }
        self.elegida = l.elegida.min(l.filas.len().saturating_sub(1));
        self.lista = l;
        self.paso = Paso::Saltar;
        self.scroll.poner(0.0);
        self.mostrar_fila(self.elegida, false);
        let mut pedidos = Vec::new();
        if self.pendiente && self.lista.texto == self.turno {
            self.pendiente = false;
            self.elegir(self.elegida, &mut pedidos);
        }
        pedidos
    }

    pub fn panel(&self) -> Rect {
        self.caja
    }

    fn escala(&self) -> f64 {
        f64::from(self.rotulos.escala())
    }

    fn alto_total(&self) -> f64 {
        let n = self.lista.filas.len() as f64;
        (n * PASO_FILA - ESPACIO).max(0.0)
    }

    fn max_scroll(&self) -> f64 {
        (self.alto_total() - ALTO_LISTA).max(0.0)
    }

    /// Lo de adentro del relleno, en la superficie.
    fn contenido(&self) -> Rect {
        self.caja.inset(-BORDE_Y_RELLENO)
    }

    fn alto_buscador(&mut self) -> f64 {
        let l = self.rotulos.linea(&self.prompt, letra(), 400.0);
        2.0 * BUSCADOR_Y + texto::alto_de_linea(&l) / self.escala()
    }

    /// La lista, en coordenadas del contenido.
    fn ventana_lista(&self, alto_buscador: f64) -> Rect {
        let y = alto_buscador + BAJO_BUSCADOR;
        Rect::new(0.0, y, ANCHO_LISTA, y + ALTO_LISTA)
    }

    fn ventana_riel(&self, alto_buscador: f64) -> Rect {
        let y = alto_buscador + BAJO_BUSCADOR;
        Rect::new(CONTENIDO - RIEL, y, CONTENIDO, y + ALTO_LISTA)
    }

    pub fn armar(&mut self, escala: f32, ahora: Instant) {
        self.rotulos.poner_escala(escala);
        let ab = self.alto_buscador();
        let alto = ab + BAJO_BUSCADOR + ALTO_LISTA + 2.0 * BORDE_Y_RELLENO;
        let (w, h) = (f64::from(self.tam.0), f64::from(self.tam.1));
        // Centrada como un widget de GTK: la caja con su margen, en un píxel entero.
        let x0 = ((w - ANCHO - 2.0 * MARGEN) / 2.0).floor() + MARGEN;
        let y0 = ((h - alto - 2.0 * MARGEN) / 2.0).floor() + MARGEN;
        self.caja = Rect::new(x0, y0, x0 + ANCHO, y0 + alto);
        let mut t = Vec::new();
        self.armar_buscador(&mut t, ab, ahora);
        self.armar_lista(&mut t, ab, ahora);
        self.armar_riel(&mut t, ab, ahora);
        self.trazos = t;
        self.rotulos.barrer();
    }

    fn armar_buscador(&mut self, t: &mut Vec<Trazo>, alto: f64, ahora: Instant) {
        let p = self.paleta;
        let e = self.escala();
        t.push(Trazo::Caja {
            r: Rect::new(0.0, 0.0, CONTENIDO, alto),
            radio: RADIO,
            color: con_alfa(&p.on_surface, p.on_surface[3] * 0.06),
        });
        let prompt = self.rotulos.linea(&self.prompt, letra(), 400.0);
        let mut x = BUSCADOR_X;
        let wp = texto::caja(&prompt).0 / e;
        t.push(Trazo::Texto {
            l: prompt,
            x,
            y: BUSCADOR_Y,
            color: p.primary,
        });
        x += wp + SEPARACION;
        let sep = self.rotulos.linea("›", letra(), 400.0);
        let ws = texto::caja(&sep).0 / e;
        t.push(Trazo::Texto {
            l: sep,
            x,
            y: BUSCADOR_Y,
            color: p.on_surface_variant,
        });
        x += ws + SEPARACION;
        let zona = Rect::new(x, BUSCADOR_Y, CONTENIDO - BUSCADOR_X, alto - BUSCADOR_Y);
        t.push(Trazo::Recorte {
            r: zona.inflate(1.0, 0.0),
            radio: 0.0,
        });
        let encendido = self.entrada.encendido(ahora);
        let cursor = |x: f64| Trazo::Caja {
            r: Rect::new(x, zona.y0, x + 1.0, zona.y1),
            radio: 0.0,
            color: p.primary,
        };
        if self.entrada.texto.is_empty() {
            let l = self.rotulos.linea(&self.placeholder, letra(), 400.0);
            t.push(Trazo::Texto {
                l,
                x: zona.x0,
                y: zona.y0,
                color: p.on_surface_variant,
            });
            if encendido {
                t.push(cursor(zona.x0));
            }
            self.corrido = 0.0;
        } else {
            let texto = self.entrada.texto.clone();
            let c = self.entrada.cursor;
            let lt = self.rotulos.linea(&texto, letra(), 400.0);
            let xc = if c == 0 {
                0.0
            } else {
                f64::from(self.rotulos.linea(&texto[..c], letra(), 400.0).width()) / e
            };
            // Que el cursor quede a la vista.
            if xc - self.corrido > zona.width() - 1.0 {
                self.corrido = xc - zona.width() + 1.0;
            } else if xc < self.corrido {
                self.corrido = xc;
            }
            self.corrido = self.corrido.max(0.0);
            t.push(Trazo::Texto {
                l: lt,
                x: zona.x0 - self.corrido.round(),
                y: zona.y0,
                color: p.on_surface,
            });
            if encendido {
                t.push(cursor((zona.x0 + xc - self.corrido).round()));
            }
        }
        t.push(Trazo::Fin);
    }

    fn armar_lista(&mut self, t: &mut Vec<Trazo>, ab: f64, ahora: Instant) {
        let p = self.paleta;
        let v = self.ventana_lista(ab);
        let scroll = self.scroll.valor(ahora).round();
        t.push(Trazo::Recorte { r: v, radio: 0.0 });
        t.push(Trazo::mover(v.x0, v.y0 - scroll));
        // El recuadro de la elegida, debajo de las filas.
        if !self.lista.filas.is_empty() {
            let destino = self.elegida as f64 * PASO_FILA;
            let f = ease_out(avance(self.recuadro.1, RECUADRO, ahora));
            let (y, alfa) = match self.paso {
                Paso::Deslizar => (self.recuadro.0 + (destino - self.recuadro.0) * f, 1.0),
                Paso::Aparecer => (destino, f as f32),
                Paso::Saltar => (destino, 1.0),
            };
            self.dibujado = y;
            t.push(Trazo::Caja {
                r: Rect::new(0.0, y, ANCHO_LISTA, y + ALTO_FILA),
                radio: RADIO,
                color: con_alfa(&p.primary, p.primary[3] * 0.14 * alfa),
            });
        }
        let primera = (scroll / PASO_FILA).floor().max(0.0) as usize;
        let ultima =
            (((scroll + ALTO_LISTA) / PASO_FILA).ceil() as usize).min(self.lista.filas.len());
        for i in primera..ultima {
            self.armar_fila(t, i);
        }
        t.push(Trazo::Fin);
        t.push(Trazo::Fin);
    }

    fn armar_fila(&mut self, t: &mut Vec<Trazo>, i: usize) {
        let p = self.paleta;
        let y = i as f64 * PASO_FILA;
        let r = Rect::new(0.0, y, ANCHO_LISTA, y + ALTO_FILA);
        if self.destello.is_some_and(|(d, _, _)| d == i) {
            for (color, d) in [(p.tertiary, (1.0, 1.0)), (p.primary, (-1.0, -1.0))] {
                t.push(Trazo::Brillo {
                    r,
                    radio: RADIO,
                    color: con_alfa(&color, color[3] * 0.45),
                    d,
                    blur: 4.0,
                });
            }
            t.push(Trazo::Caja {
                r,
                radio: RADIO,
                color: con_alfa(&p.primary, p.primary[3] * 0.24),
            });
            t.push(Trazo::Anillo {
                r,
                radio: RADIO,
                ancho: 2.0,
                a: p.primary,
                b: p.tertiary,
            });
        }
        let Some(f) = self.lista.filas.get(i) else {
            return;
        };
        let mut x = FILA_X;
        match &f.icono {
            Some(IconoFila::Imagen(img)) => t.push(Trazo::Imagen {
                img: img.clone(),
                r: Rect::new(x, y + FILA_Y, x + ICONO, y + FILA_Y + ICONO),
                radio: 0.0,
                alfa: 1.0,
            }),
            Some(IconoFila::Simbolico(img)) => t.push(Trazo::Mascara {
                img: img.clone(),
                r: Rect::new(x, y + FILA_Y, x + ICONO, y + FILA_Y + ICONO),
                color: p.on_surface,
            }),
            None => {}
        }
        if f.icono.is_some() {
            x += ICONO + ICONO_TEXTO;
        }
        let mut partes = tramos(&f.nombre, &f.resaltes, NOMBRE, NOMBRE_RESALTADO, letra());
        if let Some(g) = &f.generico {
            partes.push(("  ·  ".to_owned(), GENERICO, letra_chica()));
            partes.extend(tramos(
                g,
                &f.resaltes_generico,
                GENERICO,
                GENERICO_RESALTADO,
                letra_chica(),
            ));
        }
        let partes: Vec<(&str, u8, f32)> = partes
            .iter()
            .map(|(s, r, px)| (s.as_str(), *r, *px))
            .collect();
        let l = self.rotulos.tramos(&partes, ANCHO_LISTA - FILA_X - x);
        // Por la línea de base de la letra de la etiqueta: un emoji (otra fuente, más alta)
        // no corre el renglón, como en GTK.
        let e = self.escala();
        let muestra = self.rotulos.linea("M", letra(), 400.0);
        let (alto, base) = (
            texto::alto_de_linea(&muestra) / e,
            texto::linea_de_base(&muestra) / e,
        );
        let corrida = base - texto::linea_de_base(&l) / e;
        let atenuado = |c: &morgan_motor::color::Color| con_alfa(c, c[3] * 0.55);
        t.push(Trazo::Tramos {
            l,
            x,
            y: y + FILA_Y + ((ICONO - alto) / 2.0).floor() + corrida,
            colores: vec![
                p.on_surface,
                p.tertiary,
                atenuado(&p.on_surface),
                atenuado(&p.tertiary),
            ],
        });
    }

    /// La franja de rofi: el degradé recorre todo el alto, el riel lo muestra tenue y la
    /// perilla, su tramo, con el brillo del slider del centro.
    fn armar_riel(&mut self, t: &mut Vec<Trazo>, ab: f64, ahora: Instant) {
        let p = self.paleta;
        let v = self.ventana_riel(ab);
        let x = v.x0 + (RIEL - ANCHO_RIEL) / 2.0;
        let riel = Rect::new(x, v.y0, x + ANCHO_RIEL, v.y1);
        let tenue = |c: &morgan_motor::color::Color| con_alfa(c, c[3] * TINTE);
        t.push(Trazo::Degrade {
            r: riel,
            radio: ANCHO_RIEL / 2.0,
            a: tenue(&p.primary),
            b: tenue(&p.tertiary),
            forma: Degrade::Vertical { de: v },
        });
        let (y, h) = self.perilla(ahora);
        let perilla = Rect::new(v.x0, v.y0 + y, v.x1, v.y0 + y + h);
        let medio = mezclar(&p.primary, &p.tertiary, ((y + h / 2.0) / ALTO_LISTA) as f32);
        t.push(Trazo::Sombra {
            r: perilla,
            radio: RIEL / 2.0,
            color: con_alfa(&medio, 0.3),
            spread: 0.0,
            blur: 2.0,
        });
        t.push(Trazo::Degrade {
            r: perilla,
            radio: RIEL / 2.0,
            a: p.primary,
            b: p.tertiary,
            forma: Degrade::Vertical { de: v },
        });
    }

    /// Dónde está la perilla y cuánto mide, en el riel.
    fn perilla(&self, ahora: Instant) -> (f64, f64) {
        let total = self.alto_total().max(ALTO_LISTA);
        let h = (ALTO_LISTA / total * ALTO_LISTA).max(PERILLA_MIN);
        let recorrido = total - ALTO_LISTA;
        let y = if recorrido > 0.0 {
            (self.scroll.valor(ahora) / recorrido).clamp(0.0, 1.0) * (ALTO_LISTA - h)
        } else {
            0.0
        };
        (y, h)
    }

    pub fn pintar(&mut self, s: &mut Scene, escala: f64, bajada: f64) {
        s.reset();
        let tr = Affine::scale(escala) * Affine::translate((0.0, -bajada.round()));
        panel(s, tr, &self.paleta, self.opacidad, self.caja, self.borde);
        let c = self.contenido();
        let base = tr * Affine::translate((c.x0, c.y0));
        trazo::pintar(s, &self.trazos, base, escala);
        if let Some((_, _, pintado)) = &mut self.destello {
            *pintado = true;
        }
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        self.scroll.anda(ahora)
            || (self.paso != Paso::Saltar && avance(self.recuadro.1, RECUADRO, ahora) < 1.0)
            || self.destello.is_some()
    }

    /// Cuándo cambia el cursor, para despertar a dibujarlo.
    pub fn proximo_parpadeo(&self, ahora: Instant) -> Option<Instant> {
        self.entrada.proximo_cambio(ahora)
    }

    /// Antes de cada cuadro: el destello ya se vio, así que se hace y se cierra.
    pub fn al_cuadro(&mut self) -> Vec<EventoLauncher> {
        match self.destello {
            Some((indice, version, true)) => {
                self.destello = None;
                vec![
                    EventoLauncher::Elegido { version, indice },
                    EventoLauncher::Cerrado { escape: false },
                ]
            }
            _ => Vec::new(),
        }
    }

    // ── La elegida y el scroll ──

    fn mover_a(&mut self, i: usize, paso: Paso) {
        let ahora = Instant::now();
        if i == self.elegida && paso != Paso::Saltar {
            return;
        }
        self.recuadro = (self.dibujado, ahora);
        self.paso = paso;
        self.elegida = i;
        self.mostrar_fila(i, true);
    }

    /// Con vuelta al otro extremo solo cuando se avanza de a una.
    fn mover(&mut self, paso: i64) {
        let n = self.lista.filas.len() as i64;
        if n == 0 {
            return;
        }
        let actual = self.elegida as i64;
        let nueva = if paso.abs() == 1 {
            (actual + paso).rem_euclid(n)
        } else {
            (actual + paso).clamp(0, n - 1)
        };
        let vuelta = (nueva - actual).abs() > paso.abs();
        self.mover_a(
            nueva as usize,
            if vuelta {
                Paso::Aparecer
            } else {
                Paso::Deslizar
            },
        );
    }

    /// Contra adónde va el scroll, no donde está: con las flechas apretadas, cada paso sigue
    /// al anterior aunque el recorrido no haya terminado.
    fn mostrar_fila(&mut self, i: usize, animado: bool) {
        let arriba = i as f64 * PASO_FILA;
        let abajo = arriba + ALTO_FILA;
        let valor = self.scroll.hasta;
        let destino = if arriba < valor {
            arriba
        } else if abajo > valor + ALTO_LISTA {
            abajo - ALTO_LISTA
        } else {
            return;
        };
        let destino = destino.clamp(0.0, self.max_scroll());
        if animado {
            self.scroll.ir(destino, Instant::now());
        } else {
            self.scroll.poner(destino);
        }
    }

    fn desplazar(&mut self, dy: f64) {
        let v = (self.scroll.hasta + dy).clamp(0.0, self.max_scroll());
        self.scroll.poner(v);
    }

    fn elegir(&mut self, i: usize, pedidos: &mut Vec<Pedido>) {
        let Some(f) = self.lista.filas.get(i) else {
            return;
        };
        match f.tipo {
            TipoFila::Nada => {}
            TipoFila::Queda => pedidos.push(Pedido::Gtk(EventoLauncher::Elegido {
                version: self.lista.version,
                indice: i,
            })),
            // Un cuadro con el destello y, con el siguiente, se hace y se cierra.
            TipoFila::Cierra => self.destello = Some((i, self.lista.version, false)),
        }
    }

    // ── Teclado ──

    pub fn tecla(&mut self, k: Keysym, utf8: Option<&str>, ctrl: bool) -> Vec<Pedido> {
        let mut pedidos = Vec::new();
        // Un segundo Enter en el cuadro del destello no repite la acción.
        if self.destello.is_some() {
            return pedidos;
        }
        let vacio = self.entrada.texto.is_empty();
        let carpeta = self.lista.en_carpeta;
        let gtk = |e| Pedido::Gtk(e);
        match k {
            Keysym::Escape if carpeta => pedidos.push(gtk(EventoLauncher::SalirCarpeta)),
            Keysym::Escape => pedidos.push(gtk(EventoLauncher::Cerrado { escape: true })),
            Keysym::Left | Keysym::BackSpace if self.simbolos && vacio && carpeta => {
                pedidos.push(gtk(EventoLauncher::SalirCarpeta));
            }
            Keysym::Right if self.simbolos && vacio => {
                if self
                    .lista
                    .filas
                    .get(self.elegida)
                    .is_some_and(|f| f.carpeta)
                {
                    pedidos.push(gtk(EventoLauncher::Elegido {
                        version: self.lista.version,
                        indice: self.elegida,
                    }));
                }
            }
            Keysym::Return | Keysym::KP_Enter | Keysym::ISO_Enter => {
                // Enter sin coincidencias no hace nada; antes de la lista de lo escrito, espera.
                if self.lista.texto != self.turno {
                    self.pendiente = true;
                } else {
                    self.elegir(self.elegida, &mut pedidos);
                }
            }
            Keysym::Down | Keysym::Tab => self.mover(1),
            Keysym::Up | Keysym::ISO_Left_Tab => self.mover(-1),
            Keysym::Page_Down => self.mover(FILAS as i64),
            Keysym::Page_Up => self.mover(-(FILAS as i64)),
            _ => {
                let antes = self.entrada.texto.clone();
                let e = &mut self.entrada;
                match k {
                    Keysym::BackSpace if ctrl => e.borrar_palabra(),
                    Keysym::BackSpace => e.borrar_atras(),
                    Keysym::Delete => e.borrar_adelante(),
                    Keysym::Left => e.izquierda(),
                    Keysym::Right => e.derecha(),
                    Keysym::Home => e.inicio(),
                    Keysym::End => e.fin(),
                    Keysym::v | Keysym::V if ctrl => pedidos.push(Pedido::Pegar),
                    Keysym::u if ctrl => {
                        e.inicio();
                        e.texto.clear();
                    }
                    _ if ctrl => {}
                    _ => {
                        if let Some(s) = utf8 {
                            e.insertar(s);
                        }
                    }
                }
                if self.entrada.texto != antes {
                    pedidos.push(self.cambio());
                }
            }
        }
        pedidos
    }

    pub fn pegar(&mut self, texto: &str) -> Vec<Pedido> {
        let linea = texto.lines().next().unwrap_or("");
        let antes = self.entrada.texto.clone();
        self.entrada.insertar(linea);
        if self.entrada.texto != antes {
            return vec![self.cambio()];
        }
        Vec::new()
    }

    /// Lo escrito cambió: otro turno, y un Enter que esperaba la lista de antes se olvida.
    fn cambio(&mut self) -> Pedido {
        self.turno.1 += 1;
        self.pendiente = false;
        Pedido::Gtk(EventoLauncher::Texto(
            self.turno,
            self.entrada.texto.clone(),
        ))
    }

    // ── Puntero ──

    /// La fila bajo el puntero (no en el espacio entre dos).
    fn fila_en(&self, x: f64, y: f64) -> Option<usize> {
        let c = self.contenido();
        let ab = self.caja.height() - 2.0 * BORDE_Y_RELLENO - BAJO_BUSCADOR - ALTO_LISTA;
        let v = self.ventana_lista(ab);
        let (x, y) = (x - c.x0, y - c.y0);
        if !v.contains((x, y)) {
            return None;
        }
        let ly = y - v.y0 + self.scroll.valor(Instant::now()).round();
        let i = (ly / PASO_FILA).floor();
        if i < 0.0 || ly - i * PASO_FILA >= ALTO_FILA {
            return None;
        }
        let i = i as usize;
        (i < self.lista.filas.len()).then_some(i)
    }

    fn en_riel(&self, x: f64, y: f64) -> Option<f64> {
        let c = self.contenido();
        let ab = self.caja.height() - 2.0 * BORDE_Y_RELLENO - BAJO_BUSCADOR - ALTO_LISTA;
        let v = self.ventana_riel(ab);
        let (x, y) = (x - c.x0, y - c.y0);
        v.contains((x, y)).then_some(y - v.y0)
    }

    /// Arrastrar (o un click) lleva el medio de la perilla al puntero.
    fn llevar(&mut self, y: f64) {
        let total = self.alto_total().max(ALTO_LISTA);
        let v = y / ALTO_LISTA * total - ALTO_LISTA / 2.0;
        self.scroll.poner(v.clamp(0.0, self.max_scroll()));
    }

    /// `real`: el puntero se movió (no es el `enter` de una capa que aparece debajo de él).
    pub fn mover_puntero(&mut self, x: f64, y: f64, real: bool) {
        self.puntero = Some((x, y));
        if self.destello.is_some() {
            return;
        }
        if self.apretado == Some(Apretado::Riel) {
            let c = self.contenido();
            let ab = self.caja.height() - 2.0 * BORDE_Y_RELLENO - BAJO_BUSCADOR - ALTO_LISTA;
            let v = self.ventana_riel(ab);
            self.llevar(y - c.y0 - v.y0);
            return;
        }
        // Pasar el mouse elige (el `hover-select` de rofi).
        if real && let Some(i) = self.fila_en(x, y) {
            self.mover_a(i, Paso::Deslizar);
        }
    }

    pub fn salir_puntero(&mut self) {
        self.puntero = None;
    }

    pub fn apretar(&mut self, x: f64, y: f64) {
        self.apretado = Some(if let Some(i) = self.fila_en(x, y) {
            Apretado::Fila(i)
        } else if let Some(ry) = self.en_riel(x, y) {
            self.llevar(ry);
            Apretado::Riel
        } else if self.caja.contains((x, y)) {
            Apretado::Adentro
        } else {
            Apretado::Afuera
        });
    }

    /// Un click en una fila la elige; uno afuera de la caja cierra.
    pub fn soltar(&mut self, x: f64, y: f64, izquierdo: bool) -> Vec<Pedido> {
        let mut pedidos = Vec::new();
        let apretado = self.apretado.take();
        if self.destello.is_some() {
            return pedidos;
        }
        match apretado {
            Some(Apretado::Afuera) if !self.caja.contains((x, y)) => {
                pedidos.push(Pedido::Gtk(EventoLauncher::Cerrado { escape: false }));
            }
            Some(Apretado::Fila(i)) if izquierdo && self.fila_en(x, y) == Some(i) => {
                self.mover_a(i, Paso::Deslizar);
                self.elegir(i, &mut pedidos);
            }
            _ => {}
        }
        pedidos
    }

    pub fn rueda(&mut self, dy: f64, discreto: bool) {
        // El paso de la rueda de un `GtkScrolledWindow`: la página a la 2/3.
        let paso = if discreto {
            ALTO_LISTA.powf(2.0 / 3.0)
        } else {
            1.0
        };
        self.desplazar(dy * paso);
    }
}

/// `texto` partido en lo que coincide (`resaltes`, en caracteres) y lo que no.
pub(super) fn tramos(
    texto: &str,
    resaltes: &[(usize, usize)],
    rol: u8,
    resaltado: u8,
    px: f32,
) -> Vec<(String, u8, f32)> {
    let mut bytes: Vec<usize> = texto.char_indices().map(|(b, _)| b).collect();
    bytes.push(texto.len());
    let n = bytes.len() - 1;
    let mut v = Vec::new();
    let mut desde = 0;
    for &(a, b) in resaltes {
        let (a, b) = (a.min(n), b.min(n));
        if a < desde || a >= b {
            continue;
        }
        if a > desde {
            v.push((texto[bytes[desde]..bytes[a]].to_owned(), rol, px));
        }
        v.push((texto[bytes[a]..bytes[b]].to_owned(), resaltado, px));
        desde = b;
    }
    if desde < n {
        v.push((texto[bytes[desde]..].to_owned(), rol, px));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tramos_con_lo_resaltado() {
        let v = tramos("Música", &[(0, 2)], 0, 1, 10.0);
        assert_eq!(
            v.iter()
                .map(|(s, r, _)| (s.as_str(), *r))
                .collect::<Vec<_>>(),
            [("Mú", 1), ("sica", 0)]
        );
        let v = tramos("abc", &[], 0, 1, 10.0);
        assert_eq!(v.len(), 1);
        assert_eq!(tramos("", &[], 0, 1, 10.0).len(), 0);
    }

    #[test]
    fn el_recorrido_sigue_al_destino() {
        let mut r = Recorrido::new(Duration::from_millis(100));
        let t0 = Instant::now();
        r.ir(100.0, t0);
        assert_eq!(r.valor(t0), 0.0);
        assert!(r.anda(t0));
        r.ir(200.0, t0);
        assert_eq!(r.hasta, 200.0);
        assert_eq!(r.valor(t0 + Duration::from_secs(1)), 200.0);
    }
}
