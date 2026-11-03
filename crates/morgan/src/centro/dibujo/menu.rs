//! El menú de una app de la bandeja, con las piezas del centro y sin anillo: un marco de línea
//! neutra debajo del ícono (su borde derecho alineado con el del ícono), las filas en recuadros
//! de lista y cada submenú como un botón suelto, con el tinte de las tarjetas, cuya lista cae
//! debajo en su propio recuadro. Un submenú parte su sección: lo de antes, el botón y lo de
//! después. Abrir uno cierra los de su nivel. Lo marcado es lo que mandó la app al abrir; elegir
//! una fila solo pide y cierra.
//!
//! Las medidas: filas de 6 y 8 de relleno, 2 entre la fila y su recuadro, 4 entre los bloques y
//! del marco al contenido; la etiqueta con «_» de tecla (se saca) y hasta 40 caracteres, la
//! flecha de un submenú, la casilla de 14 px o el tilde; el panel mide al menos 150 de borde a
//! borde. El marco y las filas caen en píxeles enteros: una línea entre dos píxeles se ve más
//! gruesa de un lado que del otro.

use std::collections::HashMap;
use std::time::Instant;

use morgan_motor::color::{Color, Paleta, con_alfa, mezclar};
use morgan_motor::curva::Revelado;
use morgan_motor::forma::marco;
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Point, Rect};
use smithay_client_toolkit::seat::keyboard::Keysym;

use super::trazo::{self, Degrade, Trazo};
use super::{Animos, DESPLIEGUE, Hoja, MARGEN, Objetivo, RADIO, Rotulos};
use crate::bandeja::menu::Marca;
use crate::centro::vista::{Menu, NodoMenu};

const MIN: f64 = 150.0;
/// La línea del marco (1) y el relleno hasta el contenido (4).
const BORDE_Y_RELLENO: f64 = 5.0;
const LISTA: f64 = 2.0;
const FILA_X: f64 = 8.0;
const FILA_Y: f64 = 6.0;
/// Del borde del botón de un submenú a su texto: el mismo que el de las filas en su recuadro.
const BOTON_X: f64 = LISTA + FILA_X;
const RADIO_FILA: f64 = 3.0;
const ENTRE: f64 = 10.0;
const ENTRE_BLOQUES: f64 = 4.0;
/// `max-width-chars` de la etiqueta, en celdas de la monoespaciada a 14 px.
const ETIQUETA_MAX: f64 = 40.0 * 8.4;
const CASILLA: f64 = 14.0;
const FLECHA: &str = "\u{F0140}";

/// Lo que pide el menú a su hilo.
pub enum Salida {
    Elegido(i32),
    Cerrar,
}

/// La etiqueta sin la marca de tecla de dbusmenu (`_`; `__` es uno de verdad).
pub fn sin_tecla(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut c = s.chars().peekable();
    while let Some(x) = c.next() {
        if x == '_' {
            if c.peek() == Some(&'_') {
                c.next();
                out.push('_');
            }
            continue;
        }
        out.push(x);
    }
    out
}

pub struct MenuUi {
    pub menu: Menu,
    pub paleta: Paleta,
    pub opacidad: f32,
    pub rotulos: Rotulos,
    animos: Animos,
    /// El submenú abierto de cada nivel (por el id de su padre; la raíz es -1).
    abiertos: HashMap<i32, i32>,
    revelados: HashMap<i32, Revelado>,
    pub foco: Option<i32>,
    foco_visible: bool,
    encima: Option<i32>,
    apretado: Option<i32>,
    puntero: Option<(f64, f64)>,
    hoja: Hoja,
    /// La caja del panel, de borde a borde.
    caja: Rect,
    pub tam: (u32, u32),
}

fn op(c: &Color, a: f32) -> Color {
    con_alfa(c, c[3] * a)
}

impl MenuUi {
    pub fn new(menu: Menu, paleta: Paleta, opacidad: f32) -> Self {
        MenuUi {
            menu,
            paleta,
            opacidad,
            rotulos: Rotulos::new(),
            animos: Animos::default(),
            abiertos: HashMap::new(),
            revelados: HashMap::new(),
            foco: None,
            foco_visible: false,
            encima: None,
            apretado: None,
            puntero: None,
            hoja: Hoja::default(),
            caja: Rect::ZERO,
            tam: (0, 0),
        }
    }

    pub fn panel(&self) -> Rect {
        self.caja
    }

    fn escala(&self) -> f64 {
        f64::from(self.rotulos.escala())
    }

    fn ancho(&self, l: &morgan_motor::parley::Layout<u8>) -> f64 {
        morgan_motor::texto::caja(l).0 / self.escala()
    }

    fn alto(&self, l: &morgan_motor::parley::Layout<u8>) -> f64 {
        morgan_motor::texto::alto_de_linea(l) / self.escala()
    }

    /// Lo que mide el contenido de una fila sin recortar la etiqueta.
    fn natural(&mut self, n: &NodoMenu) -> f64 {
        let l = self.rotulos.linea(&sin_tecla(&n.etiqueta), 14.0, 400.0);
        let mut w = self.ancho(&l).min(ETIQUETA_MAX);
        if !n.hijos.is_empty() {
            let f = self.rotulos.linea(FLECHA, 16.0, 400.0);
            w += ENTRE + self.ancho(&f);
        } else {
            match n.marca {
                Some(Marca::Casilla(_)) => w += ENTRE + CASILLA,
                Some(Marca::Radio(true)) => {
                    let t = self.rotulos.linea("󰄬", 14.0, 400.0);
                    w += ENTRE + self.ancho(&t);
                }
                _ => {}
            }
        }
        // Lo de adentro de un submenú, aunque esté cerrado (como un `GtkRevealer`).
        let hijos: Vec<NodoMenu> = n.hijos.iter().flatten().cloned().collect();
        for h in &hijos {
            w = w.max(self.natural(h));
        }
        w
    }

    pub fn armar(&mut self, escala: f32, ahora: Instant) {
        self.rotulos.poner_escala(escala);
        let secciones = self.menu.secciones.clone();
        let mut contenido: f64 = 0.0;
        for n in secciones.iter().flatten() {
            contenido = contenido.max(self.natural(n));
        }
        let ancho = (contenido + 2.0 * (BORDE_Y_RELLENO + BOTON_X))
            .max(MIN)
            .ceil();
        let w = f64::from(self.tam.0);
        let x1 = (w - f64::from(self.menu.derecha))
            .min(w - MARGEN)
            .max(ancho + MARGEN);
        let mut h = Hoja::default();
        let interior = ancho - 2.0 * BORDE_Y_RELLENO;
        let mut y = 0.0;
        for (i, s) in secciones.iter().enumerate() {
            if i > 0 {
                y += ENTRE_BLOQUES;
            }
            y = self.armar_seccion(&mut h, s, interior, y, ahora);
        }
        h.alto = y;
        let x0 = x1 - ancho;
        self.caja = Rect::new(x0, MARGEN, x1, MARGEN + y + 2.0 * BORDE_Y_RELLENO);
        let vivos: std::collections::HashSet<Objetivo> =
            h.objetivos.iter().map(|(o, _)| *o).collect();
        self.hoja = h;
        let _ = vivos;
        if let Some((x, y)) = self.puntero {
            self.encima = self.bajo(x, y);
        }
    }

    /// Una sección: las filas seguidas en un recuadro y cada submenú con su botón y su lista.
    /// Devuelve dónde termina.
    fn armar_seccion(
        &mut self,
        h: &mut Hoja,
        filas: &[NodoMenu],
        ancho: f64,
        y0: f64,
        ahora: Instant,
    ) -> f64 {
        let mut y = y0;
        let mut resto = filas;
        let mut primero = true;
        while let Some(n) = resto.first() {
            if !primero {
                y += ENTRE_BLOQUES;
            }
            primero = false;
            if n.hijos.is_empty() {
                let cuantas = resto
                    .iter()
                    .position(|n| !n.hijos.is_empty())
                    .unwrap_or(resto.len());
                let (seguidas, despues) = resto.split_at(cuantas);
                y = self.armar_recuadro(h, seguidas, ancho, y, ahora);
                resto = despues;
            } else {
                y = self.armar_boton(h, n, ancho, y, ahora);
                y = self.armar_submenu(h, n, ancho, y, ahora);
                resto = &resto[1..];
            }
        }
        y
    }

    /// Filas seguidas en su recuadro.
    fn armar_recuadro(
        &mut self,
        h: &mut Hoja,
        filas: &[NodoMenu],
        ancho: f64,
        y0: f64,
        ahora: Instant,
    ) -> f64 {
        let inicio = h.trazos.len();
        let mut y = y0 + LISTA;
        for n in filas {
            y = self.armar_fila(h, n, LISTA, ancho - 2.0 * LISTA, y, ahora);
        }
        y += LISTA;
        h.trazos.insert(
            inicio,
            Trazo::Caja {
                r: Rect::new(0.0, y0, ancho, y),
                radio: RADIO,
                color: op(&self.paleta.on_surface, 0.04),
            },
        );
        y
    }

    /// Una fila que se elige.
    fn armar_fila(
        &mut self,
        h: &mut Hoja,
        n: &NodoMenu,
        x: f64,
        ancho: f64,
        y: f64,
        ahora: Instant,
    ) -> f64 {
        let p = self.paleta;
        let o = Objetivo::Fila(n.id);
        // Lo de la derecha.
        let tilde =
            (n.marca == Some(Marca::Radio(true))).then(|| self.rotulos.linea("󰄬", 14.0, 400.0));
        let casilla = match n.marca {
            Some(Marca::Casilla(puesta)) => Some(puesta),
            _ => None,
        };
        let w_der = tilde
            .as_ref()
            .map(|l| self.ancho(l))
            .or(casilla.map(|_| CASILLA));
        let tx = x + FILA_X;
        let max = (x + ancho - FILA_X - w_der.map_or(0.0, |w| w + ENTRE) - tx).max(0.0);
        let etiqueta = self
            .rotulos
            .recortada(&sin_tecla(&n.etiqueta), 14.0, max.min(ETIQUETA_MAX));
        let alto_contenido = self
            .alto(&etiqueta)
            .max(tilde.as_ref().map_or(0.0, |t| self.alto(t)))
            .max(casilla.map_or(0.0, |_| CASILLA))
            .ceil();
        let alto = FILA_Y * 2.0 + alto_contenido;
        let r = Rect::new(x, y, x + ancho, y + alto);
        let si = self.encima == Some(n.id) || (self.foco_visible && self.foco == Some(n.id));
        let luz = self.animos.luz(o, si && n.habilitado, ahora);
        h.objetivo(o, r, n.habilitado);
        h.trazos.push(Trazo::Grupo {
            r,
            escala: self.animos.escala(o, ahora),
            dx: 0.0,
            dy: 0.0,
            alfa: if n.habilitado { 1.0 } else { 0.6 },
        });
        h.trazos.push(Trazo::Caja {
            r,
            radio: RADIO_FILA,
            color: op(&p.primary, 0.14 * luz),
        });
        let ty = y + FILA_Y + ((alto_contenido - self.alto(&etiqueta)) / 2.0).floor();
        h.trazos.push(Trazo::Texto {
            l: etiqueta,
            x: tx,
            y: ty,
            color: p.on_surface,
        });
        let x_der = x + ancho - FILA_X;
        if let Some(t) = tilde {
            let w = self.ancho(&t);
            let caja = Rect::new(x_der - w, y + FILA_Y, x_der, y + FILA_Y + alto_contenido);
            h.trazos.push(Trazo::Glifo {
                l: t,
                c: caja.center(),
                color: p.tertiary,
            });
        }
        if let Some(puesta) = casilla {
            let cy = y + FILA_Y + ((alto_contenido - CASILLA) / 2.0).floor();
            let c = Rect::new(x_der - CASILLA, cy, x_der, cy + CASILLA);
            if puesta {
                h.trazos.push(Trazo::Sombra {
                    r: c,
                    radio: 3.0,
                    color: con_alfa(&p.tertiary, 0.3),
                    spread: 0.0,
                    blur: 6.0,
                });
                h.trazos.push(Trazo::Degrade {
                    r: c,
                    radio: 3.0,
                    a: p.primary,
                    b: p.tertiary,
                    forma: Degrade::Diagonal,
                });
            } else {
                h.trazos.push(Trazo::Borde {
                    r: c,
                    radio: 3.0,
                    ancho: 1.0,
                    color: op(&p.on_surface, 0.35),
                });
            }
        }
        h.trazos.push(Trazo::Fin);
        y + alto
    }

    /// El botón de un submenú: de lado a lado, con el tinte de las tarjetas del centro y la
    /// flecha, que da media vuelta al abrirse.
    fn armar_boton(
        &mut self,
        h: &mut Hoja,
        n: &NodoMenu,
        ancho: f64,
        y: f64,
        ahora: Instant,
    ) -> f64 {
        let p = self.paleta;
        let o = Objetivo::Fila(n.id);
        let flecha = self.rotulos.linea(FLECHA, 16.0, 400.0);
        let w_flecha = self.ancho(&flecha);
        let max = (ancho - 2.0 * BOTON_X - w_flecha - ENTRE).max(0.0);
        let etiqueta = self
            .rotulos
            .recortada(&sin_tecla(&n.etiqueta), 14.0, max.min(ETIQUETA_MAX));
        let alto_contenido = self.alto(&etiqueta).max(self.alto(&flecha)).ceil();
        let r = Rect::new(0.0, y, ancho, y + FILA_Y * 2.0 + alto_contenido);
        let revelado = self.revelados.get(&n.id);
        let abierto = revelado.is_some_and(|r| r.abierto());
        let giro = revelado.map_or(0.0, |r| r.valor(ahora));
        let si = self.encima == Some(n.id) || (self.foco_visible && self.foco == Some(n.id));
        let luz = self.animos.luz(o, si && n.habilitado, ahora);
        let fondo = if abierto {
            op(&p.primary, 0.2)
        } else {
            mezclar(&op(&p.on_surface, 0.06), &op(&p.primary, 0.14), luz)
        };
        h.objetivo(o, r, n.habilitado);
        h.trazos.push(Trazo::Grupo {
            r,
            escala: self.animos.escala(o, ahora),
            dx: 0.0,
            dy: 0.0,
            alfa: if n.habilitado { 1.0 } else { 0.6 },
        });
        h.trazos.push(Trazo::Caja {
            r,
            radio: RADIO,
            color: fondo,
        });
        let ty = y + FILA_Y + ((alto_contenido - self.alto(&etiqueta)) / 2.0).floor();
        h.trazos.push(Trazo::Texto {
            l: etiqueta,
            x: BOTON_X,
            y: ty,
            color: p.on_surface,
        });
        let x_der = ancho - BOTON_X;
        let c = Rect::new(
            x_der - w_flecha,
            y + FILA_Y,
            x_der,
            y + FILA_Y + alto_contenido,
        )
        .center();
        h.trazos.push(Trazo::Giro {
            c,
            angulo: std::f64::consts::PI * giro,
        });
        h.trazos.push(Trazo::Glifo {
            l: flecha,
            c,
            color: p.on_surface_variant,
        });
        h.trazos.push(Trazo::Fin);
        h.trazos.push(Trazo::Fin);
        r.y1
    }

    /// La lista de un submenú, debajo de su botón, mientras se ve. El espacio de arriba es
    /// parte de lo que se despliega.
    fn armar_submenu(
        &mut self,
        h: &mut Hoja,
        n: &NodoMenu,
        ancho: f64,
        y: f64,
        ahora: Instant,
    ) -> f64 {
        let f = self.revelados.get(&n.id).map_or(0.0, |r| r.valor(ahora));
        if f <= 0.0 {
            return y;
        }
        let mut sub = Hoja::default();
        let mut sy = 0.0;
        for s in n.hijos.iter().filter(|s| !s.is_empty()) {
            sy = self.armar_seccion(&mut sub, s, ancho, sy + ENTRE_BLOQUES, ahora);
        }
        let v = (sy * f).round();
        h.incrustar(
            sub,
            0.0,
            y + v - sy,
            Rect::new(-10.0, y, ancho + 10.0, y + v),
        );
        y + v
    }

    pub fn pintar(&self, s: &mut Scene, escala: f64, bajada: f64) {
        s.reset();
        let tr = Affine::scale(escala) * Affine::translate((0.0, -bajada.round()));
        marco(s, tr, escala, &self.paleta, self.opacidad, self.caja);
        let base = tr
            * Affine::translate((
                self.caja.x0 + BORDE_Y_RELLENO,
                self.caja.y0 + BORDE_Y_RELLENO,
            ));
        trazo::pintar(s, &self.hoja.trazos, base, escala);
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        self.animos.anda(ahora) || self.revelados.values().any(|r| r.anda(ahora))
    }

    fn bajo(&self, x: f64, y: f64) -> Option<i32> {
        let p = Point::new(
            x - self.caja.x0 - BORDE_Y_RELLENO,
            y - self.caja.y0 - BORDE_Y_RELLENO,
        );
        self.hoja
            .objetivos
            .iter()
            .rev()
            .find(|(_, r)| r.contains(p))
            .and_then(|(o, _)| match o {
                Objetivo::Fila(id) => Some(*id),
                _ => None,
            })
    }

    fn nodo(&self, id: i32) -> Option<(&NodoMenu, i32)> {
        fn buscar(v: &[Vec<NodoMenu>], id: i32, padre: i32) -> Option<(&NodoMenu, i32)> {
            for n in v.iter().flatten() {
                if n.id == id {
                    return Some((n, padre));
                }
                if let Some(x) = buscar(&n.hijos, id, n.id) {
                    return Some(x);
                }
            }
            None
        }
        buscar(&self.menu.secciones, id, -1)
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
        self.foco_visible = false;
        if let Some(id) = self.bajo(x, y) {
            self.apretado = Some(id);
            self.animos
                .apretar(Objetivo::Fila(id), true, Instant::now());
        }
    }

    /// Cualquier botón afuera del panel lo cierra (con el teclado tomado, Hyprland le manda a
    /// esta capa también los clicks sobre la barra).
    pub fn soltar(&mut self, x: f64, y: f64, izquierdo: bool) -> Option<Salida> {
        let ahora = Instant::now();
        let apretado = self.apretado.take();
        if let Some(id) = apretado {
            self.animos.apretar(Objetivo::Fila(id), false, ahora);
        }
        if apretado.is_none() && !self.caja.contains(Point::new(x, y)) {
            return Some(Salida::Cerrar);
        }
        let bajo = self.bajo(x, y);
        match apretado {
            Some(id) if izquierdo && bajo == Some(id) => self.activar(id),
            _ => None,
        }
    }

    fn activar(&mut self, id: i32) -> Option<Salida> {
        let ahora = Instant::now();
        let (n, padre) = self.nodo(id)?;
        if !n.habilitado {
            return None;
        }
        if n.hijos.is_empty() {
            return Some(Salida::Elegido(id));
        }
        // Un nivel adentro: abrir un submenú cierra los de su nivel, no al de arriba.
        let d = crate::animaciones::dur(DESPLIEGUE);
        let abrir = self.abiertos.get(&padre) != Some(&id);
        if let Some(viejo) = self.abiertos.remove(&padre) {
            super::armar::revelar(&mut self.revelados, viejo, false, d, ahora);
        }
        if abrir {
            self.abiertos.insert(padre, id);
            super::armar::revelar(&mut self.revelados, id, true, d, ahora);
        }
        None
    }

    pub fn tecla(&mut self, k: Keysym, shift: bool) -> Option<Salida> {
        match k {
            Keysym::Escape => Some(Salida::Cerrar),
            Keysym::Tab | Keysym::Down | Keysym::Right if !shift || k != Keysym::Tab => {
                self.recorrer(true);
                None
            }
            Keysym::Tab | Keysym::ISO_Left_Tab | Keysym::Up | Keysym::Left => {
                self.recorrer(false);
                None
            }
            Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                let id = self.foco?;
                self.activar(id)
            }
            _ => None,
        }
    }

    fn recorrer(&mut self, adelante: bool) {
        self.foco_visible = true;
        let v: Vec<i32> = self
            .hoja
            .enfocables
            .iter()
            .filter_map(|o| match o {
                Objetivo::Fila(id) => Some(*id),
                _ => None,
            })
            .collect();
        if v.is_empty() {
            return;
        }
        let i = self.foco.and_then(|f| v.iter().position(|x| *x == f));
        let n = v.len();
        let j = match (i, adelante) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
        };
        self.foco = Some(v[j]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_tecla_de_dbusmenu_no_se_ve() {
        assert_eq!(sin_tecla("_Ejecutar"), "Ejecutar");
        assert_eq!(sin_tecla("Guardar__como"), "Guardar_como");
        assert_eq!(sin_tecla("Sin tecla"), "Sin tecla");
    }

    fn nodo(id: i32, etiqueta: &str, hijos: Vec<Vec<NodoMenu>>) -> NodoMenu {
        NodoMenu {
            id,
            etiqueta: etiqueta.to_owned(),
            habilitado: true,
            marca: None,
            hijos,
        }
    }

    fn armado(secciones: Vec<Vec<NodoMenu>>) -> MenuUi {
        let menu = Menu {
            salida: String::new(),
            derecha: 100,
            secciones,
        };
        let mut ui = MenuUi::new(menu, Paleta::desde_css(""), 1.0);
        ui.tam = (1920, 1080);
        ui.armar(1.0, Instant::now());
        ui
    }

    /// Una línea que cae entre dos píxeles se ve más gruesa de un lado que del otro.
    #[test]
    fn el_marco_cae_en_pixeles_enteros() {
        let ui = armado(vec![vec![
            nodo(1, "Abrir", vec![]),
            nodo(2, "Próxima comprobación en 23h", vec![]),
        ]]);
        let c = ui.panel();
        for v in [c.x0, c.y0, c.x1, c.y1] {
            assert_eq!(v, v.round(), "{c:?}");
        }
    }

    #[test]
    fn un_menu_corto_mide_el_minimo() {
        let ui = armado(vec![vec![nodo(1, "Salir", vec![])]]);
        assert_eq!(ui.panel().width(), MIN);
    }

    /// Un submenú parte su sección: su botón va de lado a lado, afuera de los recuadros.
    #[test]
    fn un_submenu_parte_su_seccion() {
        let ui = armado(vec![vec![
            nodo(1, "Antes", vec![]),
            nodo(2, "Colores", vec![vec![nodo(3, "Rojo", vec![])]]),
            nodo(4, "Después", vec![]),
        ]]);
        let rect = |id| {
            ui.hoja
                .objetivos
                .iter()
                .find(|(o, _)| *o == Objetivo::Fila(id))
                .map(|(_, r)| *r)
                .unwrap()
        };
        let interior = ui.panel().width() - 2.0 * BORDE_Y_RELLENO;
        assert_eq!(rect(1).x0, LISTA);
        assert_eq!(rect(1).width(), interior - 2.0 * LISTA);
        assert_eq!(rect(2).x0, 0.0);
        assert_eq!(rect(2).width(), interior);
        assert_eq!(rect(4).x0, LISTA);
        assert!(rect(2).y0 >= rect(1).y1 + LISTA + ENTRE_BLOQUES);
        assert!(rect(4).y0 >= rect(2).y1 + LISTA + ENTRE_BLOQUES);
    }
}
