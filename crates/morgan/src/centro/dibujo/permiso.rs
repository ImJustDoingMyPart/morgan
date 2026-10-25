//! El pedido de clave de polkit con las piezas del launcher: su caja centrada, su buscador
//! como campo (el usuario de prompt y la clave tapada) y, debajo, el mensaje de polkit y lo
//! que se va a hacer. Verificando, el campo late como el de una Wi-Fi; un fallo lo hace
//! temblar y teñirse de `error`, y el aviso aparece después (`armar::fallo_de_campo`). Qué
//! se pide y cómo terminó lo decide `polkit/mod.rs`.

use std::time::Instant;

use morgan_motor::color::{Color, Paleta, con_alfa, mezclar};
use morgan_motor::forma::{Borde, panel};
use morgan_motor::texto;
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Rect};
use smithay_client_toolkit::seat::keyboard::Keysym;

use super::Rotulos;
use super::armar::{fallo_de_campo, glib_escape, latido};
use super::campo::Entrada;
use super::launcher::{
    ANCHO, BAJO_BUSCADOR, BORDE_Y_RELLENO, CONTENIDO, Campo, MARGEN, alto_buscador, armar_buscador,
    letra, letra_chica,
};
use super::trazo::{self, Trazo};
use crate::centro::vista::{EventoPermiso, VistaPermiso};
use crate::polkit::secreto::{Secreto, pisar_texto};

/// El texto, como lo de las filas del launcher.
const TEXTO_X: f64 = 10.0;
const TEXTO_Y: f64 = 6.0;
const ENTRE: f64 = 4.0;
/// Lo que se reserva para la clave: una que entra no se realoca (ni deja copias sueltas).
const RESERVA: usize = 256;

fn op(c: &Color, a: f32) -> Color {
    con_alfa(c, c[3] * a)
}

/// Lo que pide el pedido a su hilo.
pub enum Pedido {
    Gtk(EventoPermiso),
    /// Lo del portapapeles al campo.
    Pegar,
}

pub struct PermisoUi {
    pub paleta: Paleta,
    pub opacidad: f32,
    pub borde: Borde,
    rotulos: Rotulos,
    pub tam: (u32, u32),
    vista: VistaPermiso,
    entrada: Entrada,
    corrido: f64,
    /// El último fallo visto y desde cuándo tiembla.
    fallo: (u64, Option<Instant>),
    latido: Option<Instant>,
    afuera: bool,
    trazos: Vec<Trazo>,
    caja: Rect,
}

impl PermisoUi {
    pub fn new(paleta: Paleta, opacidad: f32, borde: Borde) -> Self {
        PermisoUi {
            paleta,
            opacidad,
            borde,
            rotulos: Rotulos::new(),
            tam: (0, 0),
            vista: VistaPermiso::default(),
            entrada: Entrada::default(),
            corrido: 0.0,
            fallo: (0, None),
            latido: None,
            afuera: false,
            trazos: Vec::new(),
            caja: Rect::ZERO,
        }
    }

    /// Un pedido nuevo, o lo que cambió del abierto.
    pub fn poner(&mut self, v: VistaPermiso) {
        let ahora = Instant::now();
        if v.generacion != self.vista.generacion {
            self.vaciar();
        }
        if v.fallos > self.fallo.0 {
            self.fallo = (v.fallos, Some(ahora));
        } else if v.fallos < self.fallo.0 {
            self.fallo = (v.fallos, None);
        }
        self.latido = if v.ocupado {
            Some(self.latido.unwrap_or(ahora))
        } else {
            None
        };
        self.vista = v;
    }

    /// Lo escrito se pisa: no queda en memoria para el próximo.
    fn vaciar(&mut self) {
        pisar_texto(&mut self.entrada.texto);
        self.entrada = Entrada::con_reserva(RESERVA);
        self.corrido = 0.0;
    }

    pub fn cerrar(&mut self) {
        self.vaciar();
        self.vista = VistaPermiso::default();
        self.fallo = (0, None);
        self.latido = None;
        self.afuera = false;
    }

    pub fn panel(&self) -> Rect {
        self.caja
    }

    fn escala(&self) -> f64 {
        f64::from(self.rotulos.escala())
    }

    /// Desde cuándo tiembla, mientras dura (3 s, lo del campo de una Wi-Fi).
    fn tiembla(&self, ahora: Instant) -> Option<f64> {
        let t = ahora.saturating_duration_since(self.fallo.1?).as_secs_f64();
        let tope = crate::animaciones::dur(std::time::Duration::from_secs(3)).as_secs_f64();
        (t < tope).then_some(t)
    }

    pub fn armar(&mut self, escala: f32, ahora: Instant) {
        self.rotulos.poner_escala(escala);
        let e = self.escala();
        let p = self.paleta;
        let ab = alto_buscador(&mut self.rotulos, &self.vista.usuario);
        let (dx, tinte, aviso_alfa) = fallo_de_campo(self.tiembla(ahora));
        let alfa = self.latido.map_or(1.0, |d| latido(d, ahora));
        let mut t = Vec::new();
        t.push(Trazo::Grupo {
            r: Rect::new(0.0, 0.0, CONTENIDO, ab),
            escala: 1.0,
            dx,
            dy: 0.0,
            alfa,
        });
        let campo = Campo {
            prompt: &self.vista.usuario,
            placeholder: &self.vista.placeholder,
            entrada: &self.entrada,
            tapada: !self.vista.eco,
            cursor: !self.vista.ocupado,
            fondo: mezclar(&op(&p.on_surface, 0.06), &op(&p.error, 0.3), tinte),
        };
        armar_buscador(
            &mut self.rotulos,
            &p,
            &mut t,
            &campo,
            &mut self.corrido,
            ab,
            ahora,
        );
        t.push(Trazo::Fin);
        let ancho = CONTENIDO - 2.0 * TEXTO_X;
        let mut y = ab + BAJO_BUSCADOR + TEXTO_Y;
        let lm = self
            .rotulos
            .parrafo(&glib_escape(&self.vista.mensaje), letra(), ancho, 0);
        let alto = texto::caja(&lm).1 / e;
        t.push(Trazo::Texto {
            l: lm,
            x: TEXTO_X,
            y,
            color: p.on_surface,
        });
        y += alto;
        if let Some(d) = &self.vista.detalle {
            y += ENTRE;
            let ld = self.rotulos.recortada(d, letra_chica(), ancho);
            let alto = texto::alto_de_linea(&ld) / e;
            t.push(Trazo::Texto {
                l: ld,
                x: TEXTO_X,
                y,
                color: p.on_surface_variant,
            });
            y += alto;
        }
        if let Some((aviso, error)) = &self.vista.aviso {
            y += ENTRE;
            let la = self
                .rotulos
                .parrafo(&glib_escape(aviso), letra_chica(), ancho, 0);
            let alto = texto::caja(&la).1 / e;
            let (color, a) = if *error {
                (p.error, aviso_alfa)
            } else {
                (op(&p.on_surface, 0.5), 1.0)
            };
            t.push(Trazo::Texto {
                l: la,
                x: TEXTO_X,
                y,
                color: op(&color, a),
            });
            y += alto;
        }
        y += TEXTO_Y;
        let alto_total = y + 2.0 * BORDE_Y_RELLENO;
        let (w, h) = (f64::from(self.tam.0), f64::from(self.tam.1));
        // Centrada como el launcher: la caja con su margen, en un píxel entero.
        let x0 = ((w - ANCHO - 2.0 * MARGEN) / 2.0).floor() + MARGEN;
        let y0 = ((h - alto_total - 2.0 * MARGEN) / 2.0).floor() + MARGEN;
        self.caja = Rect::new(x0, y0, x0 + ANCHO, y0 + alto_total);
        self.trazos = t;
        self.rotulos.barrer();
    }

    pub fn pintar(&self, s: &mut Scene, escala: f64, bajada: f64) {
        s.reset();
        let tr = Affine::scale(escala) * Affine::translate((0.0, -bajada.round()));
        panel(s, tr, &self.paleta, self.opacidad, self.caja, self.borde);
        let c = self.caja.inset(-BORDE_Y_RELLENO);
        trazo::pintar(
            s,
            &self.trazos,
            tr * Affine::translate((c.x0, c.y0)),
            escala,
        );
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        self.latido.is_some() || self.tiembla(ahora).is_some()
    }

    /// Cuándo cambia el cursor, para despertar a dibujarlo.
    pub fn proximo_parpadeo(&self, ahora: Instant) -> Option<Instant> {
        if self.vista.ocupado {
            return None;
        }
        self.entrada.proximo_cambio(ahora)
    }

    // ── Teclado ──

    /// Verificando no se escribe ni se cancela: la clave ya está en PAM (un par de segundos
    /// si falla).
    pub fn tecla(&mut self, k: Keysym, utf8: Option<&str>, ctrl: bool) -> Vec<Pedido> {
        if self.vista.ocupado {
            return Vec::new();
        }
        let e = &mut self.entrada;
        match k {
            Keysym::Escape => return vec![Pedido::Gtk(EventoPermiso::Cancelado)],
            Keysym::Return | Keysym::KP_Enter | Keysym::ISO_Enter => {
                let s = Secreto::tomar(&mut e.texto);
                e.cursor = 0;
                e.tocar();
                return vec![Pedido::Gtk(EventoPermiso::Enviar(s))];
            }
            Keysym::BackSpace if ctrl => e.borrar_palabra(),
            Keysym::BackSpace => e.borrar_atras(),
            Keysym::Delete => e.borrar_adelante(),
            Keysym::Left => e.izquierda(),
            Keysym::Right => e.derecha(),
            Keysym::Home => e.inicio(),
            Keysym::End => e.fin(),
            Keysym::v | Keysym::V if ctrl => return vec![Pedido::Pegar],
            Keysym::u if ctrl => {
                pisar_texto(&mut e.texto);
                e.inicio();
            }
            _ if ctrl => {}
            _ => {
                if let Some(s) = utf8 {
                    e.insertar(s);
                }
            }
        }
        Vec::new()
    }

    pub fn pegar(&mut self, texto: &str) {
        if !self.vista.ocupado {
            self.entrada.insertar(texto.lines().next().unwrap_or(""));
        }
    }

    // ── Puntero ──

    pub fn apretar(&mut self, x: f64, y: f64) {
        self.afuera = !self.caja.contains((x, y));
    }

    /// Un click afuera de la caja cancela, como cierra el launcher.
    pub fn soltar(&mut self, x: f64, y: f64) -> Vec<EventoPermiso> {
        let afuera = std::mem::take(&mut self.afuera) && !self.caja.contains((x, y));
        if afuera && !self.vista.ocupado {
            return vec![EventoPermiso::Cancelado];
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui() -> PermisoUi {
        PermisoUi::new(Paleta::desde_css(""), 1.0, Borde::Sombra)
    }

    fn vista(generacion: u64) -> VistaPermiso {
        VistaPermiso {
            usuario: "roberto".into(),
            mensaje: "Se requiere autenticación".into(),
            placeholder: "Contraseña".into(),
            generacion,
            ..VistaPermiso::default()
        }
    }

    fn escribir(u: &mut PermisoUi, s: &str) {
        for c in s.chars() {
            u.tecla(Keysym::a, Some(&c.to_string()), false);
        }
    }

    #[test]
    fn enter_se_lleva_la_clave_y_vacia_el_campo() {
        let mut u = ui();
        u.poner(vista(1));
        escribir(&mut u, "clave");
        let p = u.tecla(Keysym::Return, None, false);
        let [Pedido::Gtk(EventoPermiso::Enviar(s))] = p.as_slice() else {
            panic!("tenía que enviar");
        };
        assert_eq!(s.bytes(), b"clave");
        assert!(u.entrada.texto.is_empty());
    }

    #[test]
    fn verificando_no_se_escribe_ni_se_cancela() {
        let mut u = ui();
        u.poner(VistaPermiso {
            ocupado: true,
            ..vista(1)
        });
        escribir(&mut u, "x");
        assert!(u.entrada.texto.is_empty());
        assert!(u.tecla(Keysym::Escape, None, false).is_empty());
        u.apretar(-10.0, -10.0);
        assert!(u.soltar(-10.0, -10.0).is_empty());
    }

    #[test]
    fn otra_generacion_vacia_y_un_fallo_nuevo_tiembla() {
        let mut u = ui();
        u.poner(vista(1));
        escribir(&mut u, "abc");
        u.poner(vista(1));
        assert_eq!(u.entrada.texto, "abc");
        u.poner(VistaPermiso {
            fallos: 1,
            ..vista(2)
        });
        assert!(u.entrada.texto.is_empty());
        assert!(u.entrada.texto.capacity() >= RESERVA);
        assert!(u.anda(Instant::now()));
    }

    #[test]
    fn escape_y_click_afuera_cancelan() {
        let mut u = ui();
        u.poner(vista(1));
        assert!(matches!(
            u.tecla(Keysym::Escape, None, false).as_slice(),
            [Pedido::Gtk(EventoPermiso::Cancelado)]
        ));
        u.apretar(-10.0, -10.0);
        assert!(matches!(
            u.soltar(-10.0, -10.0).as_slice(),
            [EventoPermiso::Cancelado]
        ));
    }
}
