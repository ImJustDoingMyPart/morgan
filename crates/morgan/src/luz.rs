//! Lo que se toca a mano vale hasta el próximo cambio de horario.
//!
//! Hyprland funde cada cambio de la CTM y no deja preguntar si está en identidad, así que
//! prendida es que `hyprsunset` corre: al prender se lanza con la temperatura (funde de neutro
//! a cálido) y al apagar se le pide `identity` y se lo termina cuando acabó el fundido.
//!
//! Corre como unidad propia y no como hijo: en el cgroup de morgan, reiniciar morgan la
//! apagaría de golpe. Así sobrevive al reinicio, y el estado del arranque sale de preguntarle a
//! systemd.
//!
//! Un solo timeout hasta el próximo cambio de horario, con tope de 15 minutos para que una
//! suspensión o un cambio de hora no lo desfasen (el reloj de glib es monótono).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;


use crate::comando;
use crate::config;
use crate::osd::{Aviso, Osd};

const UNIDAD: &str = "morgan-nightlight";
/// El nombre hasta 0.1.1: una luz prendida con él antes de actualizar también se apaga.
const UNIDAD_VIEJA: &str = "morgan-luz";

/// La velocidad 5 son 500 ms, con un poco de margen.
const FUNDIDO: Duration = Duration::from_millis(600);

/// La ventana puede cruzar la medianoche. `prender == apagar` es nunca.
pub fn plan(ahora: u32, prender: u32, apagar: u32) -> (bool, u32) {
    const DIA: u32 = 24 * 60;
    if prender == apagar {
        return (false, DIA);
    }
    let dentro = if prender < apagar {
        (prender..apagar).contains(&ahora)
    } else {
        ahora >= prender || ahora < apagar
    };
    let proximo = if dentro { apagar } else { prender };
    (dentro, (proximo + DIA - ahora) % DIA)
}

pub fn aviso(prendida: bool, temperatura: u32) -> Aviso {
    let texto = if prendida {
        format!("Activada · {temperatura} K")
    } else {
        "Desactivada".to_owned()
    };
    Aviso::estado("󰖔", "Luz nocturna", &texto, prendida, 2000)
}

#[derive(Clone)]
pub struct Luz {
    inner: Rc<Inner>,
}

struct Inner {
    config: RefCell<config::Luz>,
    osd: Osd,
    turno: Cell<u64>,
    estado: Cell<Option<bool>>,
    prendida: Cell<bool>,
    /// Sube con cada prender o apagar: un apagado que espera el fundido no termina el proceso si
    /// en el medio se volvió a prender.
    orden: Cell<u64>,
    oyentes: RefCell<Vec<Rc<dyn Fn()>>>,
}

impl Luz {
    pub fn new(c: &config::Luz, osd: Osd) -> Self {
        let luz = Luz {
            inner: Rc::new(Inner {
                config: RefCell::new(c.clone()),
                osd,
                turno: Cell::new(0),
                estado: Cell::new(None),
                prendida: Cell::new(false),
                orden: Cell::new(0),
                oyentes: RefCell::default(),
            }),
        };
        let yo = luz.clone();
        glib::MainContext::default().spawn_local(async move {
            if unidad_activa().await {
                yo.inner.prendida.set(true);
                yo.avisar_oyentes();
            }
            yo.armar();
        });
        luz
    }

    pub fn prendida(&self) -> bool {
        self.inner.prendida.get()
    }

    pub fn al_cambiar(&self, f: impl Fn() + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    fn avisar_oyentes(&self) {
        let oyentes: Vec<_> = self.inner.oyentes.borrow().clone();
        for f in oyentes {
            f();
        }
    }

    pub async fn poner(&self, si: bool) -> Result<(), String> {
        let i = &self.inner;
        let orden = i.orden.get() + 1;
        i.orden.set(orden);
        let temperatura = i.config.borrow().temperatura;
        if si {
            prender(temperatura).await?;
        } else if unidad_activa().await {
            // Si ya no corre, no hay fundido que esperar.
            let _ = comando::salida(&["hyprctl", "hyprsunset", "identity"]).await;
            glib::timeout_future(FUNDIDO).await;
            if i.orden.get() == orden {
                comando::salida(&["systemctl", "--user", "stop", UNIDAD]).await?;
                let _ = comando::salida(&["systemctl", "--user", "stop", UNIDAD_VIEJA]).await;
            }
        }
        // Un pedido posterior ya puso (o va a poner) su estado.
        if i.orden.get() != orden {
            return Ok(());
        }
        let cambio = i.prendida.replace(si) != si;
        if cambio {
            self.avisar_oyentes();
        }
        if si || cambio {
            i.osd.mostrar(&aviso(si, temperatura));
        }
        Ok(())
    }

    pub fn lanzar(&self, si: bool) {
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = yo.poner(si).await {
                eprintln!("morgan: luz nocturna: {e}");
            }
        });
    }

    pub fn alternar(&self) {
        self.lanzar(!self.prendida());
    }

    pub fn aplicar_config(&self, c: &config::Luz) {
        // El menú lo llama con cualquier cambio de la config: si `[nightlight]` no cambió, nada.
        let antes = self.inner.config.replace(c.clone());
        if antes == *c {
            return;
        }
        if antes.temperatura != c.temperatura && self.prendida() {
            self.lanzar(true);
        }
        if (antes.horario, &antes.prender, &antes.apagar) != (c.horario, &c.prender, &c.apagar) {
            self.inner.estado.set(None);
            self.armar();
        }
    }

    fn armar(&self) {
        let i = &self.inner;
        i.turno.set(i.turno.get() + 1);
        let c = i.config.borrow().clone();
        if !c.horario {
            return;
        }
        let Ok(ahora) = glib::DateTime::now_local() else {
            return;
        };
        let minutos = ahora.hour() as u32 * 60 + ahora.minute() as u32;
        let (dentro, faltan) = plan(
            minutos,
            config::minutos_del_dia(&c.prender),
            config::minutos_del_dia(&c.apagar),
        );
        // Solo al cambiar: lo prendido o apagado a mano no se pisa a cada vuelta.
        if i.estado.replace(Some(dentro)) != Some(dentro) {
            self.lanzar(dentro);
        }
        let segundos = (faltan.min(15) * 60)
            .saturating_sub(ahora.second() as u32)
            .max(1);
        let turno = i.turno.get();
        let yo = self.clone();
        glib::timeout_add_local_once(Duration::from_secs(u64::from(segundos)), move || {
            if yo.inner.turno.get() == turno {
                yo.armar();
            }
        });
    }
}

async fn unidad_activa() -> bool {
    for u in [UNIDAD, UNIDAD_VIEJA] {
        if comando::salida(&["systemctl", "--user", "is-active", "--quiet", u]).await.is_ok() {
            return true;
        }
    }
    false
}

/// Con el entorno de la sesión gráfica, que el gestor de systemd no tiene por qué tener.
async fn prender(temperatura: u32) -> Result<(), String> {
    let t = temperatura.to_string();
    if unidad_activa().await {
        return comando::salida(&["hyprctl", "hyprsunset", "temperature", &t])
            .await
            .map(drop);
    }
    let unidad = format!("--unit={UNIDAD}");
    comando::salida(&[
        "systemd-run",
        "--user",
        "--quiet",
        "--collect",
        &unidad,
        "-E",
        "WAYLAND_DISPLAY",
        "-E",
        "HYPRLAND_INSTANCE_SIGNATURE",
        "--",
        "hyprsunset",
        "-t",
        &t,
    ])
    .await
    .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ventana_que_cruza_la_medianoche() {
        let (p, a) = (20 * 60, 7 * 60);
        assert_eq!(plan(19 * 60, p, a), (false, 60));
        assert_eq!(plan(20 * 60, p, a), (true, 11 * 60));
        assert_eq!(plan(23 * 60 + 30, p, a), (true, 7 * 60 + 30));
        assert_eq!(plan(3 * 60, p, a), (true, 4 * 60));
        assert_eq!(plan(7 * 60, p, a), (false, 13 * 60));
    }

    #[test]
    fn ventana_del_mismo_dia_y_nunca() {
        assert_eq!(plan(10 * 60, 9 * 60, 17 * 60), (true, 7 * 60));
        assert_eq!(plan(18 * 60, 9 * 60, 17 * 60), (false, 15 * 60));
        assert_eq!(plan(5, 600, 600), (false, 1440));
    }
}
