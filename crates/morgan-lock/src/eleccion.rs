//! El greeter elige de a una cosa antes de la clave (diseño G, Roberto, 2026-11-06): primero
//! el usuario, después la sesión, cada uno en una rueda dentro del anillo del campo, con
//! flechas a los costados. Lo elegido sube a un renglón centrado bajo los «:» del reloj
//! (el usuario a la izquierda del «›», la sesión a la derecha) y el anillo se completa desde
//! arriba, como al acertar. Una lista con una sola opción no se pregunta: queda fija en el
//! renglón. El bloqueo no tiene elección.
//!
//! Es solo el modelo, como función del tiempo; lo dibuja `dibujo.rs`. Las duraciones y las
//! curvas son las del boceto G.

use crate::escena::{MS, acotar, bezier, seg};

/// Lo que dura el paso de un paso a otro: lo de antes sube y lo nuevo entra desde abajo.
pub const PASO_MS: i64 = 440;
const PASO_ALFA_MS: i64 = 300;
/// El giro de la rueda, con un rebote.
pub const GIRO_MS: i64 = 460;
/// Lo elegido aparece en el renglón con un salto.
pub const MIGA_MS: i64 = 460;
/// Del destello a que cambia el paso: el anillo ya se completó.
pub const AVANCE_MS: i64 = 380;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Que {
    Usuario,
    Sesion,
}

/// Un paso de la elección o la clave, al final.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Paso {
    Elegir(Que),
    Clave,
}

/// La posición de la rueda, animada: un índice que puede valer 1,3 a mitad de giro.
#[derive(Clone, Copy, Debug)]
struct Giro {
    de: f32,
    a: f32,
    t0: i64,
}

impl Giro {
    fn valor(&self, t: i64) -> f32 {
        let f = acotar(seg(t - self.t0) / seg(GIRO_MS * MS), 0.0, 1.0);
        self.de + (self.a - self.de) * bezier(0.3, 1.3, 0.5, 1.0, f)
    }
}

/// Dónde está cada paso respecto del actual (-1 ya pasó, 0 se ve, 1 viene), animado.
#[derive(Clone, Copy, Debug)]
struct Lugar {
    de: usize,
    t0: i64,
}

#[derive(Debug)]
pub struct Eleccion {
    pub usuarios: Vec<String>,
    pub sesiones: Vec<String>,
    pub u: usize,
    pub s: usize,
    giro_u: Giro,
    giro_s: Giro,
    /// Índice en `pasos()`.
    pub paso: usize,
    lugar: Lugar,
    /// Cuándo apareció en el renglón lo de cada lista (las fijas, sin salto).
    miga_u: Option<i64>,
    miga_s: Option<i64>,
    /// Los destellos de cada avance.
    pub destellos: Vec<i64>,
    /// Lo que tiene el puntero encima, para el color de las flechas y del renglón.
    pub encima: Option<Zona>,
}

/// Lo que se toca con el puntero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zona {
    Antes,
    Despues,
    Centro,
    Miga(Que),
}

impl Eleccion {
    pub fn new(usuarios: Vec<String>, sesiones: Vec<String>, u: usize, s: usize) -> Self {
        let u = u.min(usuarios.len().saturating_sub(1));
        let s = s.min(sesiones.len().saturating_sub(1));
        Eleccion {
            giro_u: Giro {
                de: u as f32,
                a: u as f32,
                t0: 0,
            },
            giro_s: Giro {
                de: s as f32,
                a: s as f32,
                t0: 0,
            },
            usuarios,
            sesiones,
            u,
            s,
            paso: 0,
            lugar: Lugar { de: 0, t0: 0 },
            miga_u: None,
            miga_s: None,
            destellos: Vec::new(),
            encima: None,
        }
    }

    /// Solo se pregunta lo que tiene más de una opción; la clave siempre, al final.
    pub fn pasos(&self) -> Vec<Paso> {
        let mut v = Vec::new();
        if self.usuarios.len() > 1 {
            v.push(Paso::Elegir(Que::Usuario));
        }
        if self.sesiones.len() > 1 {
            v.push(Paso::Elegir(Que::Sesion));
        }
        v.push(Paso::Clave);
        v
    }

    pub fn actual(&self) -> Paso {
        let p = self.pasos();
        p[self.paso.min(p.len() - 1)]
    }

    pub fn lista(&self, q: Que) -> (&[String], usize) {
        match q {
            Que::Usuario => (&self.usuarios, self.u),
            Que::Sesion => (&self.sesiones, self.s),
        }
    }

    /// Mueve la rueda. Fuera de la lista, nada (las flechas de los extremos están apagadas).
    pub fn girar(&mut self, q: Que, i: usize, t: i64) -> bool {
        let n = self.lista(q).0.len();
        if i >= n || i == self.lista(q).1 {
            return false;
        }
        let g = match q {
            Que::Usuario => {
                self.u = i;
                &mut self.giro_u
            }
            Que::Sesion => {
                self.s = i;
                &mut self.giro_s
            }
        };
        *g = Giro {
            de: g.valor(t),
            a: i as f32,
            t0: t,
        };
        true
    }

    pub fn posicion(&self, q: Que, t: i64) -> f32 {
        match q {
            Que::Usuario => self.giro_u.valor(t),
            Que::Sesion => self.giro_s.valor(t),
        }
    }

    /// Pasa al paso `k`. Hacia adelante, con el destello y lo elegido saltando al renglón;
    /// hacia atrás, lo que se vuelve a elegir sale del renglón.
    pub fn ir(&mut self, k: usize, t: i64) {
        let k = k.min(self.pasos().len() - 1);
        if k == self.paso {
            return;
        }
        let pasos = self.pasos();
        if k > self.paso {
            for p in &pasos[self.paso..k] {
                match p {
                    Paso::Elegir(Que::Usuario) => self.miga_u = Some(t),
                    Paso::Elegir(Que::Sesion) => self.miga_s = Some(t),
                    Paso::Clave => {}
                }
            }
        } else {
            for p in &pasos[k..self.paso] {
                match p {
                    Paso::Elegir(Que::Usuario) => self.miga_u = None,
                    Paso::Elegir(Que::Sesion) => self.miga_s = None,
                    Paso::Clave => {}
                }
            }
        }
        self.lugar = Lugar {
            de: self.paso,
            t0: t,
        };
        self.paso = k;
    }

    /// El destello que marca un avance; el paso cambia `AVANCE_MS` después (lo pide el
    /// principal).
    pub fn destello(&mut self, t: i64) {
        self.destellos.push(t);
    }

    /// Cómo se ve el paso `k`: (corrimiento vertical en px, escala, opacidad). El que se va
    /// sube y el que viene entra desde abajo.
    pub fn capa(&self, k: usize, t: i64) -> (f32, f32, f32) {
        let lugar = |paso: usize| (k as f32 - paso as f32).clamp(-1.0, 1.0);
        let (de, a) = (lugar(self.lugar.de), lugar(self.paso));
        let f = acotar(seg(t - self.lugar.t0) / seg(PASO_MS * MS), 0.0, 1.0);
        let p = de + (a - de) * bezier(0.2, 0.9, 0.2, 1.0, f);
        let fa = acotar(seg(t - self.lugar.t0) / seg(PASO_ALFA_MS * MS), 0.0, 1.0);
        let visto = |l: f32| 1.0 - l.abs();
        let alfa = visto(de) + (visto(a) - visto(de)) * (1.0 - (1.0 - fa).powi(2));
        (p * 30.0, 1.0 - 0.06 * p.abs(), acotar(alfa, 0.0, 1.0))
    }

    /// Lo que va en el renglón: (texto, salto) del usuario y de la sesión. Salto: 0 recién
    /// aparece, 1 en su lugar; `None`, no está.
    pub fn migas(&self, t: i64) -> [Option<(&str, f32)>; 2] {
        let salto = |t0: Option<i64>| {
            t0.map(|t0| {
                let f = acotar(seg(t - t0) / seg(MIGA_MS * MS), 0.0, 1.0);
                bezier(0.3, 1.5, 0.5, 1.0, f)
            })
        };
        fn una(lista: &[String], i: usize, f: Option<f32>) -> Option<(&str, f32)> {
            if lista.len() == 1 {
                Some((lista[0].as_str(), 1.0))
            } else {
                f.map(|f| (lista[i].as_str(), f))
            }
        }
        [
            una(&self.usuarios, self.u, salto(self.miga_u)),
            una(&self.sesiones, self.s, salto(self.miga_s)),
        ]
    }

    /// Se puede tocar para volver a ese paso.
    pub fn miga_vuelve(&self, q: Que) -> Option<usize> {
        let k = self.pasos().iter().position(|p| *p == Paso::Elegir(q))?;
        (k < self.paso).then_some(k)
    }

    pub fn animando(&self, t: i64) -> bool {
        let reciente = |t0: i64, ms: i64| t - t0 < (ms + 50) * MS;
        reciente(self.lugar.t0, PASO_MS)
            || reciente(self.giro_u.t0, GIRO_MS)
            || reciente(self.giro_s.t0, GIRO_MS)
            || self.miga_u.is_some_and(|t0| reciente(t0, MIGA_MS))
            || self.miga_s.is_some_and(|t0| reciente(t0, MIGA_MS))
            || self.destellos.iter().any(|&t0| reciente(t0, 700))
    }

    pub fn limpiar(&mut self, t: i64) {
        self.destellos.retain(|&t0| t - t0 < 800 * MS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(u: usize, s: usize) -> Eleccion {
        let v = |n: usize, p: &str| (0..n).map(|i| format!("{p}{i}")).collect();
        Eleccion::new(v(u, "u"), v(s, "s"), 0, 0)
    }

    #[test]
    fn solo_se_pregunta_lo_que_tiene_opciones() {
        use Paso::*;
        assert_eq!(
            e(2, 3).pasos(),
            [Elegir(Que::Usuario), Elegir(Que::Sesion), Clave]
        );
        assert_eq!(e(1, 3).pasos(), [Elegir(Que::Sesion), Clave]);
        assert_eq!(e(1, 1).pasos(), [Clave]);
        assert_eq!(e(1, 1).actual(), Clave);
    }

    #[test]
    fn lo_fijo_esta_en_el_renglon_desde_el_principio() {
        let x = e(1, 3);
        let [u, s] = x.migas(0);
        assert_eq!(u, Some(("u0", 1.0)));
        assert_eq!(s, None);
    }

    #[test]
    fn avanzar_sube_lo_elegido_y_volver_lo_saca() {
        let mut x = e(2, 2);
        assert!(x.girar(Que::Usuario, 1, 0));
        x.ir(1, 100 * MS);
        let [u, s] = x.migas(100 * MS + MIGA_MS * MS);
        assert_eq!(u, Some(("u1", 1.0)));
        assert_eq!(s, None);
        assert_eq!(x.miga_vuelve(Que::Usuario), Some(0));
        assert_eq!(x.miga_vuelve(Que::Sesion), None, "es el paso de ahora");
        x.ir(0, 2000 * MS);
        assert_eq!(x.migas(3000 * MS)[0], None);
    }

    #[test]
    fn la_rueda_no_sale_de_la_lista_y_llega_con_rebote() {
        let mut x = e(3, 1);
        assert!(!x.girar(Que::Usuario, 3, 0));
        assert!(!x.girar(Que::Usuario, 0, 0), "ya está ahí");
        assert!(x.girar(Que::Usuario, 1, 0));
        let pico = (1..GIRO_MS)
            .map(|ms| x.posicion(Que::Usuario, ms * MS))
            .fold(0.0f32, f32::max);
        assert!(pico > 1.0, "se pasa un poco y vuelve");
        assert!((x.posicion(Que::Usuario, GIRO_MS * MS) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn el_paso_que_se_va_sube_y_el_que_viene_entra_de_abajo() {
        let mut x = e(2, 2);
        assert_eq!(x.capa(0, 0), (0.0, 1.0, 1.0));
        assert_eq!(x.capa(1, 0).2, 0.0);
        x.ir(1, 0);
        let fin = PASO_MS * MS;
        let (dy, _, a) = x.capa(0, fin);
        assert!((dy + 30.0).abs() < 1e-3 && a < 1e-3);
        let (dy, esc, a) = x.capa(1, fin);
        assert!(dy.abs() < 1e-3 && (esc - 1.0).abs() < 1e-4 && (a - 1.0).abs() < 1e-4);
        let (dy, _, _) = x.capa(1, fin / 4);
        assert!(dy > 0.0, "a mitad de camino, todavía abajo");
    }

    #[test]
    fn quieto_no_anima() {
        let mut x = e(2, 2);
        assert!(!x.animando(10_000 * MS));
        x.destello(10_000 * MS);
        assert!(x.animando(10_100 * MS));
        x.limpiar(20_000 * MS);
        assert!(x.destellos.is_empty());
    }
}
