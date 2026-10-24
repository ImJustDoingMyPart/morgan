//! Las curvas de las animaciones y un valor que va de un número a otro con su transición, como
//! un `transition` de la hoja de GTK.

use std::time::{Duration, Instant};

pub fn acotar(x: f32, a: f32, b: f32) -> f32 {
    x.max(a).min(b)
}

/// La salida cúbica: arranca rápido y frena.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - acotar(t, 0.0, 1.0)).powi(3)
}

/// `cubic-bezier(x1, y1, x2, y2)` de CSS: se busca el parámetro cuya x es `x` y se da su y.
pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let x = acotar(x, 0.0, 1.0);
    let curva = |a: f32, b: f32, s: f32| {
        let r = 1.0 - s;
        3.0 * r * r * s * a + 3.0 * r * s * s * b + s * s * s
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let m = (lo + hi) / 2.0;
        if curva(x1, x2, m) < x {
            lo = m;
        } else {
            hi = m;
        }
    }
    curva(y1, y2, (lo + hi) / 2.0)
}

/// `ease` de CSS.
pub fn ease(t: f32) -> f32 {
    bezier(0.25, 0.1, 0.25, 1.0, t)
}

/// `ease-in-out` de CSS.
pub fn ease_in_out(t: f32) -> f32 {
    bezier(0.42, 0.0, 0.58, 1.0, t)
}

/// Un número que cambia con una transición `ease`, como en la hoja de GTK: un cambio a mitad
/// de camino arranca desde donde iba.
#[derive(Clone, Copy, Debug)]
pub struct Transicion {
    desde: f32,
    hasta: f32,
    t0: Instant,
    dura: Duration,
}

impl Transicion {
    pub fn new(v: f32) -> Self {
        Transicion {
            desde: v,
            hasta: v,
            t0: Instant::now(),
            dura: Duration::ZERO,
        }
    }

    pub fn valor(&self, ahora: Instant) -> f32 {
        let d = self.dura.as_secs_f32();
        if d <= 0.0 {
            return self.hasta;
        }
        let t = ahora.saturating_duration_since(self.t0).as_secs_f32() / d;
        if t >= 1.0 {
            return self.hasta;
        }
        if t <= 0.0 {
            return self.desde;
        }
        self.desde + (self.hasta - self.desde) * ease(t)
    }

    pub fn destino(&self) -> f32 {
        self.hasta
    }

    /// Hacia `v` en `dura` (cero: ya). Pedir el destino que ya tiene no la reinicia.
    pub fn ir(&mut self, v: f32, dura: Duration, ahora: Instant) {
        if v == self.hasta {
            return;
        }
        self.desde = self.valor(ahora);
        self.hasta = v;
        self.t0 = ahora;
        self.dura = dura;
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        ahora.saturating_duration_since(self.t0) < self.dura
    }
}

/// Lo que se ve de algo que se despliega, de 0 a 1, como un `GtkRevealer`: va de donde está
/// hacia abierto o cerrado en el tiempo entero de la transición, con la salida cúbica en los
/// dos sentidos.
#[derive(Clone, Copy, Debug)]
pub struct Revelado {
    desde: f32,
    abrir: bool,
    t0: Instant,
    dura: Duration,
}

impl Revelado {
    pub fn new(abierto: bool) -> Self {
        Revelado {
            desde: if abierto { 1.0 } else { 0.0 },
            abrir: abierto,
            t0: Instant::now(),
            dura: Duration::ZERO,
        }
    }

    fn hasta(&self) -> f32 {
        if self.abrir { 1.0 } else { 0.0 }
    }

    fn actual(&self, ahora: Instant) -> f32 {
        let d = self.dura.as_secs_f32();
        let t = if d <= 0.0 {
            1.0
        } else {
            ahora.saturating_duration_since(self.t0).as_secs_f32() / d
        };
        self.desde + (self.hasta() - self.desde) * ease_out(t)
    }

    pub fn valor(&self, ahora: Instant) -> f64 {
        f64::from(self.actual(ahora))
    }

    pub fn abierto(&self) -> bool {
        self.abrir
    }

    /// Hacia abierto o cerrado en `dura`, desde donde iba.
    pub fn ir(&mut self, abrir: bool, dura: Duration, ahora: Instant) {
        if abrir != self.abrir {
            self.desde = self.actual(ahora);
            self.t0 = ahora;
            self.abrir = abrir;
            self.dura = dura;
        }
    }

    /// Ya, sin transición.
    pub fn poner(&mut self, abierto: bool) {
        *self = Revelado::new(abierto);
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        ahora.saturating_duration_since(self.t0) < self.dura
    }
}

/// El temblor de un fallo, en píxeles, para `t` de 0 a 1: tres idas y vueltas que se apagan
/// (la de la clave del bloqueo; el campo de una Wi-Fi la usa a la mitad).
pub fn sacudida(t: f32) -> f32 {
    const K: [(f32, f32); 7] = [
        (0.0, 0.0),
        (0.14, -11.0),
        (0.30, 9.0),
        (0.46, -6.0),
        (0.62, 4.0),
        (0.80, -2.0),
        (1.0, 0.0),
    ];
    if !(0.0..1.0).contains(&t) {
        return 0.0;
    }
    let i = K
        .iter()
        .rposition(|(k, _)| *k <= t)
        .unwrap_or(0)
        .min(K.len() - 2);
    let ((t0, v0), (t1, v1)) = (K[i], K[i + 1]);
    let f = (t - t0) / (t1 - t0);
    v0 + (v1 - v0) * (f * f * (3.0 - 2.0 * f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revelado_desanda_desde_donde_iba() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut r = Revelado::new(false);
        assert_eq!(r.valor(t0), 0.0);
        r.ir(true, ms(100), t0);
        assert!(r.anda(t0 + ms(50)));
        assert_eq!(r.valor(t0 + ms(100)), 1.0);
        r.ir(false, ms(100), t0 + ms(150));
        assert!((r.valor(t0 + ms(200)) - f64::from(1.0 - ease_out(0.5))).abs() < 1e-6);
        // Cambiar a mitad de camino sigue desde donde iba.
        r.ir(true, ms(100), t0 + ms(200));
        assert!((r.valor(t0 + ms(200)) - f64::from(1.0 - ease_out(0.5))).abs() < 1e-6);
        assert!(!r.anda(t0 + ms(310)));
        assert!(Revelado::new(true).valor(t0) == 1.0);
    }

    #[test]
    fn bezier_en_las_puntas() {
        assert!(bezier(0.33, 1.0, 0.68, 1.0, 0.0).abs() < 1e-4);
        assert!((bezier(0.33, 1.0, 0.68, 1.0, 1.0) - 1.0).abs() < 1e-4);
        assert!(bezier(0.33, 1.0, 0.68, 1.0, 0.5) > 0.8);
    }

    #[test]
    fn transicion_retoma_desde_donde_iba() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut v = Transicion::new(0.0);
        v.ir(1.0, ms(100), t0);
        assert_eq!(v.valor(t0), 0.0);
        let medio = v.valor(t0 + ms(50));
        assert!(medio > 0.5 && medio < 1.0);
        v.ir(0.0, ms(100), t0 + ms(50));
        assert!((v.valor(t0 + ms(50)) - medio).abs() < 1e-6);
        assert_eq!(v.valor(t0 + ms(200)), 0.0);
        assert!(!v.anda(t0 + ms(200)));
        v.ir(1.0, Duration::ZERO, t0);
        assert_eq!(v.valor(t0), 1.0);
    }
}
