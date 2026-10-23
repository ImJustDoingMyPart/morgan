//! Lo que se ve, como función del tiempo. El hilo principal declara cada cambio con su
//! instante (`Cambio`) y el de render camina las animaciones solo, a ritmo de pantalla: si el
//! principal se traba, la onda sigue. Acá no hay Wayland ni GPU, solo el modelo; lo dibuja
//! `dibujo.rs`.
//!
//! Los tiempos y las constantes son los de la versión con GTK (hasta 0.1.1), que tenía el
//! reloj, el aviso y los fundidos en la hoja de estilo.

use std::sync::OnceLock;
use std::time::Instant;

use morgan_ipc::bloqueo::{Bloqueo, Escritura};

use crate::caracteres::Caracteres;

pub const ANCHO: f32 = 344.0;
pub const ALTO: f32 = 52.0;

/// Los fundidos de entrada y de salida, y el destello del acierto.
pub const ENTRADA_MS: i64 = 200;
pub const SALIDA_MS: i64 = 200;
pub const DESTELLO_MS: i64 = 380;
pub const FALLO_MS: i64 = 2000;

pub(crate) const LENTO_TECLA: f32 = 4.0;
pub(crate) const LENTO_EVENTO: f32 = 2.0;
const LENTO_FALLO: f32 = 1.4;
pub(crate) const FALLO_ONDA: f32 = 0.7;
pub(crate) const FALLO_TEXTO: f32 = 0.8;

pub const MS: i64 = 1000;

/// Microsegundos desde el arranque, el mismo reloj en los dos hilos.
pub fn ahora() -> i64 {
    static CERO: OnceLock<Instant> = OnceLock::new();
    CERO.get_or_init(Instant::now).elapsed().as_micros() as i64
}

pub(crate) fn seg(us: i64) -> f32 {
    us as f32 / 1e6
}

pub(crate) use morgan_motor::curva::{acotar, bezier, ease_out};
pub use morgan_motor::forma::Borde;

pub use morgan_motor::color::{BLANCO, Color, Paleta, con_alfa, mezclar};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Modo {
    Normal,
    Verificando,
    Fallo(u32),
    Saliendo,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tipo {
    Sale,
    Entra,
    Fallo,
    Acierto,
}

impl Tipo {
    pub(crate) fn lento(self) -> f32 {
        match self {
            Tipo::Sale | Tipo::Entra => LENTO_TECLA,
            Tipo::Fallo => LENTO_FALLO,
            Tipo::Acierto => LENTO_EVENTO,
        }
    }
}

pub struct Impulso {
    pub t0: i64,
    pub tipo: Tipo,
    pub amp: f32,
}

pub struct Chispa {
    pub t0: i64,
    pub dir: f32,
    pub s0: f32,
    pub fuerza: f32,
}

/// Una transición lineal entre dos valores, para no depender del dt del cuadro.
#[derive(Clone, Copy)]
pub struct Tramo<T> {
    pub de: T,
    pub a: T,
    pub t0: i64,
    pub dura: i64,
}

impl Tramo<f32> {
    pub fn valor(&self, t: i64) -> f32 {
        let f = acotar((t - self.t0) as f32 / self.dura.max(1) as f32, 0.0, 1.0);
        self.de + (self.a - self.de) * f
    }
}

impl Tramo<[Color; 2]> {
    pub fn valor(&self, t: i64) -> [Color; 2] {
        let f = acotar((t - self.t0) as f32 / self.dura.max(1) as f32, 0.0, 1.0);
        [
            mezclar(&self.de[0], &self.a[0], f),
            mezclar(&self.de[1], &self.a[1], f),
        ]
    }
}

/// Lo que el hilo principal le declara al de render. Cada uno trae su instante.
#[derive(Debug, Clone, PartialEq)]
pub enum Cambio {
    Agregar {
        n: usize,
        t: i64,
    },
    Quitar {
        n: usize,
        todo: bool,
        t: i64,
    },
    /// El campo se vació por el envío, no por un borrado: sin golpe de onda.
    Vaciar {
        t: i64,
    },
    Normal {
        t: i64,
    },
    Verificando {
        t: i64,
    },
    Fallo {
        n: u32,
        t: i64,
    },
    /// El destello, y después el fundido de salida: los dos quedan declarados acá.
    Acierto {
        t: i64,
    },
    Mayus {
        si: bool,
        t: i64,
    },
    Reloj {
        hora: String,
        minutos: String,
        fecha: String,
    },
    Aviso(Option<String>),
}

pub struct Estado {
    pub paleta: Paleta,
    pub estilo: Bloqueo,
    /// Lo de afuera del anillo de la caja: sigue a la sombra de las ventanas.
    pub borde: Borde,
    pub modo: Modo,
    pub t_modo: i64,
    pub t_fallo: i64,
    pub t_acierto: i64,
    pub n: usize,
    pub mayus: bool,
    pub impulsos: Vec<Impulso>,
    pub chispas: Vec<Chispa>,
    lado: f32,
    pub anillo: Tramo<[Color; 2]>,
    pub visible: Tramo<f32>,
    pub hora: String,
    pub minutos: String,
    pub fecha: String,
    pub aviso: Option<String>,
    /// Desde el primer cuadro: la entrada empieza cuando se ve algo, no al pedir el bloqueo.
    pub t_entrada: Option<i64>,
    /// Sube con cada cambio de texto: quien arma los textos sabe cuándo rehacerlos.
    pub version_textos: u64,
    /// Las escrituras que no son la onda.
    pub fila: Caracteres,
    t_cuadro: i64,
}

impl Estado {
    pub fn new(paleta: Paleta, estilo: Bloqueo) -> Self {
        let par = [paleta.primary, paleta.tertiary];
        Estado {
            paleta,
            estilo,
            borde: Borde::Sombra,
            modo: Modo::Normal,
            t_modo: 0,
            t_fallo: i64::MIN / 2,
            t_acierto: i64::MIN / 2,
            n: 0,
            mayus: false,
            impulsos: Vec::new(),
            chispas: Vec::new(),
            lado: 1.0,
            anillo: Tramo {
                de: par,
                a: par,
                t0: 0,
                dura: 300 * MS,
            },
            visible: Tramo {
                de: 0.0,
                a: 0.0,
                t0: 0,
                dura: 1,
            },
            hora: String::new(),
            minutos: String::new(),
            fecha: String::new(),
            aviso: None,
            t_entrada: None,
            version_textos: 0,
            fila: Caracteres::default(),
            t_cuadro: 0,
        }
    }

    pub fn aplicar(&mut self, c: Cambio) {
        match c {
            Cambio::Agregar { n, t } => {
                self.n = n;
                if self.onda() {
                    self.impulsos.push(Impulso {
                        t0: t,
                        tipo: Tipo::Sale,
                        amp: 7.0 + 3.0 * azar(t),
                    });
                } else {
                    self.fila.contar(n, t);
                }
                self.chispa(t, (ANCHO - 2.0) / 2.0, 1.0);
                if matches!(self.modo, Modo::Fallo(_)) {
                    self.cambiar(Modo::Normal, t);
                }
            }
            Cambio::Quitar { n, todo, t } => {
                self.n = n;
                if self.onda() {
                    self.impulsos.push(Impulso {
                        t0: t,
                        tipo: Tipo::Entra,
                        amp: if todo { 10.0 } else { 6.0 },
                    });
                } else {
                    self.fila.contar(n, t);
                }
                let abajo = (ANCHO - 2.0) + (ALTO - 2.0) + (ANCHO - 2.0) / 2.0;
                self.chispa(t, abajo, 0.6);
            }
            // Si lo tipeado late mientras verifica, los caracteres se quedan.
            Cambio::Vaciar { t } => {
                self.n = 0;
                if !self.estilo.verificar.late() {
                    self.fila.vaciar(t);
                }
            }
            Cambio::Normal { t } => self.cambiar(Modo::Normal, t),
            Cambio::Verificando { t } => self.cambiar(Modo::Verificando, t),
            Cambio::Fallo { n, t } => {
                self.t_fallo = t;
                self.cambiar(Modo::Fallo(n), t);
                if !self.estilo.reaccion {
                    self.fila.vaciar(t);
                } else if self.onda() {
                    self.impulsos.push(Impulso {
                        t0: t,
                        tipo: Tipo::Fallo,
                        amp: 15.0,
                    });
                } else {
                    self.fila.caer(t);
                }
            }
            Cambio::Acierto { t } => {
                self.t_acierto = t;
                self.cambiar(Modo::Saliendo, t);
                if self.onda() {
                    self.impulsos.push(Impulso {
                        t0: t,
                        tipo: Tipo::Acierto,
                        amp: 6.0,
                    });
                }
                self.fila.brillar();
            }
            Cambio::Mayus { si, t } => {
                self.mayus = si;
                self.rehacer_anillo(t);
            }
            Cambio::Reloj {
                hora,
                minutos,
                fecha,
            } => {
                (self.hora, self.minutos, self.fecha) = (hora, minutos, fecha);
                self.version_textos += 1;
            }
            Cambio::Aviso(a) => {
                self.aviso = a;
                self.version_textos += 1;
            }
        }
    }

    /// Cuándo empieza el fundido de salida: después del destello, si lo hay.
    pub fn t_salida(&self) -> Option<i64> {
        (self.modo == Modo::Saliendo).then(|| {
            let destello = if self.estilo.destello { DESTELLO_MS } else { 0 };
            self.t_acierto + destello * MS
        })
    }

    /// La opacidad de todo: los dos fundidos de la hoja de `morgan-lock`.
    pub fn opacidad(&self, t: i64) -> f32 {
        let entra = match self.t_entrada {
            Some(t0) => bezier(0.5, 1.0, 0.89, 1.0, seg(t - t0) / seg(ENTRADA_MS * MS)),
            None => 0.0,
        };
        let sale = match self.t_salida() {
            Some(t0) if t >= t0 => {
                1.0 - bezier(0.33, 1.0, 0.68, 1.0, seg(t - t0) / seg(SALIDA_MS * MS))
            }
            _ => 1.0,
        };
        entra * sale
    }

    /// El fallo manda sobre Bloq Mayús, y Bloq Mayús sobre la verificación.
    fn objetivo(&self) -> [Color; 2] {
        let p = &self.paleta;
        match self.modo {
            Modo::Fallo(_) => [p.error, p.error],
            _ if self.mayus => [p.on_surface, p.on_surface],
            Modo::Verificando => [p.tertiary, p.primary],
            _ => [p.primary, p.tertiary],
        }
    }

    fn rehacer_anillo(&mut self, t: i64) {
        let a = self.objetivo();
        if a != self.anillo.a {
            self.anillo = Tramo {
                de: self.anillo.valor(t),
                a,
                t0: t,
                dura: 300 * MS,
            };
        }
    }

    fn cambiar(&mut self, m: Modo, t: i64) {
        self.modo = m;
        self.t_modo = t;
        self.rehacer_anillo(t);
    }

    pub fn onda(&self) -> bool {
        self.estilo.escritura == Escritura::Onda
    }

    pub fn latido(&self) -> bool {
        self.modo == Modo::Verificando && self.estilo.verificar.late()
    }

    fn quiere_onda(&self, t: i64) -> bool {
        if !self.onda() {
            return false;
        }
        self.n > 0
            || self.latido()
            || self.modo == Modo::Saliendo
            || self
                .impulsos
                .iter()
                .any(|p| p.tipo == Tipo::Fallo && seg(t - p.t0) < FALLO_ONDA)
    }

    /// Lo que corre al empezar cada cuadro: limpia lo que terminó y fija la visibilidad.
    pub fn avanzar(&mut self, t: i64) {
        let dt = seg(t - self.t_cuadro).clamp(0.0, 0.064);
        self.t_cuadro = t;
        let latido = self.latido();
        let juntar = latido && self.estilo.escritura == Escritura::Puntos;
        self.fila.avanzar(t, dt, juntar, latido);
        self.impulsos
            .retain(|p| seg(t - p.t0) / p.tipo.lento() < 2.5);
        self.chispas.retain(|c| t - c.t0 < 700 * MS);
        let quiere = if self.quiere_onda(t) { 1.0 } else { 0.0 };
        if quiere != self.visible.a {
            let dura = if quiere > 0.5 { 150 } else { 300 };
            self.visible = Tramo {
                de: self.visible.valor(t),
                a: quiere,
                t0: t,
                dura: dura * MS,
            };
        }
    }

    fn chispa(&mut self, t: i64, s0: f32, fuerza: f32) {
        if self.estilo.chispa {
            self.chispas.push(Chispa {
                t0: t,
                dir: self.lado,
                s0,
                fuerza,
            });
        }
        self.lado = -self.lado;
    }

    /// Hace falta otro cuadro. Con el campo vacío y quieto, no.
    pub fn animando(&self, t: i64) -> bool {
        let fundido = match (self.t_entrada, self.t_salida()) {
            (None, _) => true,
            (_, Some(t0)) => t - t0 < (SALIDA_MS + 50) * MS,
            (Some(t0), None) => t - t0 < (ENTRADA_MS + 50) * MS,
        };
        fundido
            || (self.n > 0 && self.onda())
            || (self.fila.hay() && self.estilo.escritura != Escritura::Fijo)
            || !self.impulsos.is_empty()
            || !self.chispas.is_empty()
            || matches!(self.modo, Modo::Verificando | Modo::Saliendo)
            || t - self.anillo.t0 < self.anillo.dura
            || t - self.visible.t0 < self.visible.dura
            || t - self.t_fallo < 1500 * MS
    }
}

/// Un número en [0, 1) a partir del tiempo: alcanza para variar el golpe de cada tecla.
pub(crate) fn azar(t: i64) -> f32 {
    let x = (t as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (x >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estado() -> Estado {
        Estado::new(Paleta::desde_css(""), Bloqueo::default())
    }

    #[test]
    fn bezier_va_de_cero_a_uno_y_sale_rapido() {
        assert!(bezier(0.33, 1.0, 0.68, 1.0, 0.0).abs() < 1e-4);
        assert!((bezier(0.33, 1.0, 0.68, 1.0, 1.0) - 1.0).abs() < 1e-4);
        assert!(bezier(0.33, 1.0, 0.68, 1.0, 0.5) > 0.8);
    }

    #[test]
    fn quieto_y_vacio_no_pide_cuadros() {
        let mut e = estado();
        assert!(
            e.animando(0),
            "sin primer cuadro, la entrada está pendiente"
        );
        e.t_entrada = Some(0);
        let t = 10_000 * MS;
        e.avanzar(t);
        assert!(!e.animando(t));
        e.aplicar(Cambio::Agregar { n: 1, t });
        assert!(
            e.animando(t + 5_000 * MS),
            "con algo tipeado la onda ondula"
        );
    }

    #[test]
    fn la_salida_queda_declarada_desde_el_acierto() {
        let mut e = estado();
        e.t_entrada = Some(0);
        let t = 5_000 * MS;
        e.aplicar(Cambio::Acierto { t });
        assert_eq!(e.t_salida(), Some(t + DESTELLO_MS * MS));
        assert!(e.opacidad(t + DESTELLO_MS * MS) > 0.999);
        assert!(e.opacidad(t + (DESTELLO_MS + SALIDA_MS) * MS) < 1e-3);
        assert!(
            e.animando(t + (DESTELLO_MS + SALIDA_MS) * MS),
            "el último cuadro, en cero"
        );
    }

    #[test]
    fn el_fallo_tiñe_el_anillo_y_tipear_lo_saca() {
        let mut e = estado();
        e.aplicar(Cambio::Fallo { n: 1, t: 0 });
        assert_eq!(e.anillo.a, [e.paleta.error; 2]);
        e.aplicar(Cambio::Agregar { n: 1, t: 10 });
        assert_eq!(e.modo, Modo::Normal);
    }
}
