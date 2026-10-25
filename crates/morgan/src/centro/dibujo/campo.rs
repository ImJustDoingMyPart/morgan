//! Lo escrito en un campo y cómo se ve: la clave tapada con «•» (el `●` ocupa la celda entera
//! de la monoespaciada y se pegan) hasta que se toca el ojo, el placeholder y el cursor, que
//! parpadea como el de GTK (encendido 2/3 de 1,2 s, apagado el resto) y se queda fijo a los
//! 10 s sin tocar nada.

use std::time::{Duration, Instant};

use morgan_motor::color::con_alfa;
use morgan_motor::vello::kurbo::Rect;

use super::trazo::Trazo;
use super::{Hoja, Ui};
use crate::centro::vista::{Caja, ListaId};

const CICLO: Duration = Duration::from_millis(1200);
const DEJA_DE_PARPADEAR: Duration = Duration::from_secs(10);

pub struct Entrada {
    pub texto: String,
    /// En bytes, siempre entre dos caracteres.
    pub cursor: usize,
    /// La clave a la vista (el ojo).
    pub ver: bool,
    generacion: u64,
    /// Desde la última tecla: el cursor arranca encendido.
    tocado: Instant,
    /// Lo que se corre el texto para que el cursor quede a la vista.
    corrido: f64,
}

impl Default for Entrada {
    fn default() -> Self {
        Entrada {
            texto: String::new(),
            cursor: 0,
            ver: false,
            generacion: 0,
            tocado: Instant::now(),
            corrido: 0.0,
        }
    }
}

impl Entrada {
    /// Vacío, con lugar para `n` bytes: una clave que entra no se realoca (ni deja copias).
    pub fn con_reserva(n: usize) -> Self {
        Entrada {
            texto: String::with_capacity(n),
            ..Entrada::default()
        }
    }

    /// Un campo nuevo (otra red, otro pedido) arranca vacío.
    pub fn preparar(&mut self, generacion: u64) {
        if generacion != self.generacion {
            *self = Entrada {
                generacion,
                ..Entrada::default()
            };
        }
    }

    pub fn tocar(&mut self) {
        self.tocado = Instant::now();
    }

    pub fn insertar(&mut self, s: &str) {
        let s: String = s.chars().filter(|c| !c.is_control()).collect();
        if s.is_empty() {
            return;
        }
        self.texto.insert_str(self.cursor, &s);
        self.cursor += s.len();
        self.tocar();
    }

    pub fn borrar_atras(&mut self) {
        if let Some((i, _)) = self.texto[..self.cursor].char_indices().next_back() {
            self.texto.replace_range(i..self.cursor, "");
            self.cursor = i;
        }
        self.tocar();
    }

    pub fn borrar_adelante(&mut self) {
        if let Some(c) = self.texto[self.cursor..].chars().next() {
            self.texto
                .replace_range(self.cursor..self.cursor + c.len_utf8(), "");
        }
        self.tocar();
    }

    /// La palabra de atrás (Ctrl+Retroceso), con los blancos que la siguen.
    pub fn borrar_palabra(&mut self) {
        let antes = &self.texto[..self.cursor];
        let sin_blancos = antes.trim_end();
        let inicio = sin_blancos
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map_or(0, |(i, c)| i + c.len_utf8());
        self.texto.replace_range(inicio..self.cursor, "");
        self.cursor = inicio;
        self.tocar();
    }

    pub fn izquierda(&mut self) {
        if let Some((i, _)) = self.texto[..self.cursor].char_indices().next_back() {
            self.cursor = i;
        }
        self.tocar();
    }

    pub fn derecha(&mut self) {
        if let Some(c) = self.texto[self.cursor..].chars().next() {
            self.cursor += c.len_utf8();
        }
        self.tocar();
    }

    pub fn inicio(&mut self) {
        self.cursor = 0;
        self.tocar();
    }

    pub fn fin(&mut self) {
        self.cursor = self.texto.len();
        self.tocar();
    }

    pub fn encendido(&self, ahora: Instant) -> bool {
        let t = ahora.saturating_duration_since(self.tocado);
        if t >= DEJA_DE_PARPADEAR {
            return true;
        }
        let fase = t.as_millis() % CICLO.as_millis();
        fase < CICLO.as_millis() * 2 / 3
    }

    /// Cuándo cambia el cursor (para despertar a dibujarlo), mientras parpadea.
    pub fn proximo_cambio(&self, ahora: Instant) -> Option<Instant> {
        let t = ahora.saturating_duration_since(self.tocado);
        if t >= DEJA_DE_PARPADEAR {
            return None;
        }
        let ciclo = CICLO.as_millis();
        let fase = t.as_millis() % ciclo;
        let falta = if fase < ciclo * 2 / 3 {
            ciclo * 2 / 3 - fase
        } else {
            ciclo - fase
        };
        Some(ahora + Duration::from_millis(falta as u64 + 1))
    }

    /// Lo que se ve: la clave tapada, o el texto.
    pub(super) fn visible(&self, clave: bool) -> (String, usize) {
        if clave && !self.ver {
            let n = self.texto[..self.cursor].chars().count();
            ("•".repeat(self.texto.chars().count()), n * '•'.len_utf8())
        } else {
            (self.texto.clone(), self.cursor)
        }
    }
}

impl Ui {
    /// Lo escrito (o el código fijo, o el placeholder) en `zona`, con el cursor si tiene el
    /// foco.
    pub(super) fn armar_entrada(
        &mut self,
        h: &mut Hoja,
        l: ListaId,
        caja: &Caja,
        zona: Rect,
        enfocado: bool,
        ahora: Instant,
    ) {
        let p = self.paleta;
        let Some(e) = self.campos.get(&l) else { return };
        let (texto, cursor) = match &caja.fijo {
            Some(f) => (f.clone(), f.len()),
            None => e.visible(caja.clave),
        };
        let encendido = e.encendido(ahora);
        let corrido_antes = e.corrido;
        let alfa = if caja.ocupado { 0.5 } else { 1.0 };
        h.trazos.push(Trazo::Recorte {
            r: zona.inflate(1.0, 0.0),
            radio: 0.0,
        });
        if texto.is_empty() {
            let lp = self.rotulos.linea(&caja.placeholder, 14.0, 400.0);
            h.trazos.push(Trazo::Texto {
                l: lp,
                x: zona.x0,
                y: zona.y0,
                color: con_alfa(&p.on_surface_variant, p.on_surface_variant[3] * alfa),
            });
            if enfocado && encendido && caja.fijo.is_none() {
                h.trazos.push(Trazo::Caja {
                    r: Rect::new(zona.x0, zona.y0, zona.x0 + 1.0, zona.y1),
                    radio: 0.0,
                    color: p.primary,
                });
            }
            h.trazos.push(Trazo::Fin);
            if let Some(e) = self.campos.get_mut(&l) {
                e.corrido = 0.0;
            }
            return;
        }
        let lt = self.rotulos.linea(&texto, 14.0, 400.0);
        let hasta = self.rotulos.linea(&texto[..cursor], 14.0, 400.0);
        let xc = if cursor == 0 {
            0.0
        } else {
            hasta.width() as f64 / f64::from(self.rotulos.escala())
        };
        // Que el cursor quede a la vista.
        let mut corrido = corrido_antes;
        if xc - corrido > zona.width() - 1.0 {
            corrido = xc - zona.width() + 1.0;
        } else if xc < corrido {
            corrido = xc;
        }
        corrido = corrido.max(0.0);
        h.trazos.push(Trazo::Texto {
            l: lt,
            x: zona.x0 - corrido.round(),
            y: zona.y0,
            color: con_alfa(&p.on_surface, p.on_surface[3] * alfa),
        });
        if enfocado && encendido && caja.fijo.is_none() && !caja.ocupado {
            let x = (zona.x0 + xc - corrido).round();
            h.trazos.push(Trazo::Caja {
                r: Rect::new(x, zona.y0, x + 1.0, zona.y1),
                radio: 0.0,
                color: p.primary,
            });
        }
        h.trazos.push(Trazo::Fin);
        if let Some(e) = self.campos.get_mut(&l) {
            e.corrido = corrido;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escribir_mover_y_borrar() {
        let mut e = Entrada::default();
        e.insertar("clave");
        e.izquierda();
        e.izquierda();
        e.insertar("ñ");
        assert_eq!(e.texto, "clañve");
        e.borrar_atras();
        e.borrar_adelante();
        assert_eq!(e.texto, "clae");
        e.fin();
        e.insertar(" dos");
        e.borrar_palabra();
        assert_eq!(e.texto, "clae ");
        e.inicio();
        e.borrar_atras();
        assert_eq!(e.cursor, 0);
        assert_eq!(e.visible(true).0, "•••••");
        e.ver = true;
        assert_eq!(e.visible(true).0, "clae ");
    }

    #[test]
    fn otra_generacion_arranca_vacia() {
        let mut e = Entrada::default();
        e.preparar(1);
        e.insertar("abc");
        e.preparar(1);
        assert_eq!(e.texto, "abc");
        e.preparar(2);
        assert!(e.texto.is_empty());
    }

    #[test]
    fn parpadea_y_se_queda_fijo() {
        let e = Entrada::default();
        let t0 = e.tocado;
        assert!(e.encendido(t0));
        assert!(!e.encendido(t0 + Duration::from_millis(900)));
        assert!(e.encendido(t0 + Duration::from_millis(1250)));
        assert!(e.encendido(t0 + Duration::from_secs(11)));
        assert!(e.proximo_cambio(t0 + Duration::from_secs(11)).is_none());
        assert!(e.proximo_cambio(t0).is_some());
    }
}
