//! La batería de la notebook, para el centro: el dispositivo de resumen de UPower
//! (`DisplayDevice`), que junta todas las baterías de la máquina. En una PC de escritorio no
//! está presente y el centro no la muestra.
//!
//! Lo bajo y lo crítico los decide UPower (`WarningLevel`, con los umbrales de su
//! `UPower.conf`), igual que lo que hace al quedar sin carga: acá solo se muestra.
//!
//! Sin sondeo: se relee con cada `PropertiesChanged` del dispositivo.

use std::cell::RefCell;
use std::rc::Rc;

use crate::sistema;

const UPOWER: &str = "org.freedesktop.UPower";
const RUTA: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const DEVICE: &str = "org.freedesktop.UPower.Device";

/// `Type` de UPower para una batería de la máquina (no la de un mouse o un UPS).
const TIPO_BATERIA: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Carga {
    #[default]
    Descargando,
    Cargando,
    /// Llena y enchufada.
    Llena,
    /// Enchufada sin cargar (un tope de carga, por ejemplo al 80 %).
    Enchufada,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Estado {
    /// De 0 a 100.
    pub porcentaje: f64,
    pub carga: Carga,
    /// Lo que falta para vaciarse (descargando) o llenarse (cargando); 0 si UPower no lo sabe.
    pub segundos: i64,
    /// UPower la da por baja o crítica.
    pub baja: bool,
}

/// Lo que dice UPower, tal cual, para pasarlo a `Estado` sin el bus.
#[derive(Debug, Clone, Default)]
pub struct Crudo {
    pub presente: bool,
    pub tipo: u32,
    pub estado: u32,
    pub porcentaje: f64,
    pub vaciar: i64,
    pub llenar: i64,
    pub aviso: u32,
}

impl Crudo {
    /// `None`: no hay batería.
    pub fn estado(&self) -> Option<Estado> {
        if !self.presente || self.tipo != TIPO_BATERIA {
            return None;
        }
        // `State`: 1 cargando, 2 descargando, 3 vacía, 4 llena, 5 sin cargar, 6 sin descargar.
        let carga = match self.estado {
            1 => Carga::Cargando,
            4 => Carga::Llena,
            5 => Carga::Enchufada,
            _ => Carga::Descargando,
        };
        let segundos = match carga {
            Carga::Cargando => self.llenar,
            Carga::Descargando => self.vaciar,
            _ => 0,
        };
        Some(Estado {
            porcentaje: self.porcentaje.clamp(0.0, 100.0),
            carga,
            segundos: segundos.max(0),
            // `WarningLevel`: 3 baja, 4 crítica, 5 a punto de actuar (2 es la de un UPS).
            baja: carga == Carga::Descargando && self.aviso >= 3,
        })
    }
}

/// Las de Material Design Icons (en las Nerd Fonts), de 10 en 10.
const DESCARGANDO: [&str; 11] = ["󰂎", "󰁺", "󰁻", "󰁼", "󰁽", "󰁾", "󰁿", "󰂀", "󰂁", "󰂂", "󰁹"];
const CARGANDO: [&str; 11] = ["󰢟", "󰢜", "󰂆", "󰂇", "󰂈", "󰢝", "󰂉", "󰢞", "󰂊", "󰂋", "󰂅"];

impl Estado {
    pub fn glifo(&self) -> &'static str {
        let i = ((self.porcentaje / 10.0).round() as usize).min(10);
        match self.carga {
            Carga::Cargando | Carga::Llena => CARGANDO[i],
            Carga::Descargando | Carga::Enchufada => DESCARGANDO[i],
        }
    }

    pub fn por_ciento(&self) -> String {
        format!("{}%", self.porcentaje.round() as i64)
    }

    /// El nombre de la fila del centro: dice si carga.
    pub fn nombre(&self) -> &'static str {
        match self.carga {
            Carga::Descargando => tr!("Batería"),
            Carga::Cargando => tr!("Cargando"),
            Carga::Llena => tr!("Llena"),
            Carga::Enchufada => tr!("Enchufada"),
        }
    }

    /// Lo que falta para vaciarse o llenarse, o nada si UPower no lo sabe.
    pub fn detalle(&self) -> String {
        if self.segundos > 0 {
            tiempo(self.segundos)
        } else {
            String::new()
        }
    }
}

/// «2 h 10 min», «40 min», «1 min».
pub fn tiempo(segundos: i64) -> String {
    let min = ((segundos + 30) / 60).max(1);
    let (h, m) = (min / 60, min % 60);
    match (h, m) {
        (0, m) => trf!("{} min", m),
        (h, 0) => trf!("{} h", h),
        (h, m) => trf!("{} h {} min", h, m),
    }
}

/// Estados de mentira, uno de cada, para verla sin batería (`MORGAN_MUESTRA_BATERIA`).
pub fn muestras() -> Vec<Estado> {
    use Carga::*;
    let e = |porcentaje, carga, segundos| Estado {
        porcentaje,
        carga,
        segundos,
        baja: carga == Descargando && porcentaje < 20.0,
    };
    vec![
        e(100.0, Descargando, 6 * 3600 + 20 * 60),
        e(73.0, Descargando, 2 * 3600 + 10 * 60),
        e(45.0, Descargando, 3600 + 25 * 60),
        e(18.0, Descargando, 32 * 60),
        e(6.0, Descargando, 7 * 60),
        e(35.0, Cargando, 3600 + 5 * 60),
        e(88.0, Cargando, 14 * 60),
        e(80.0, Enchufada, 0),
        e(100.0, Llena, 0),
    ]
}

#[derive(Clone, Default)]
pub struct Bateria {
    inner: Rc<Inner>,
}

type Oyente = Rc<dyn Fn(Option<&Estado>)>;

#[derive(Default)]
struct Inner {
    estado: RefCell<Option<Estado>>,
    oyentes: RefCell<Vec<Oyente>>,
    vigia: RefCell<Option<glib::JoinHandle<()>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(h) = self.vigia.take() {
            h.abort();
        }
    }
}

impl Bateria {
    /// Empieza a seguir a UPower.
    pub fn new() -> Self {
        let b = Bateria::default();
        let weak = Rc::downgrade(&b.inner);
        let h = sistema::vigilar(
            sistema::propiedades(UPOWER, RUTA),
            Box::new(move || {
                let weak = weak.clone();
                Box::pin(async move {
                    let e = match leer().await {
                        Ok(c) => c.estado(),
                        Err(e) => {
                            // Sin UPower no hay batería que mostrar.
                            eprintln!("morgan: batería: {e}");
                            None
                        }
                    };
                    if let Some(inner) = weak.upgrade() {
                        Bateria { inner }.poner(e);
                    }
                })
            }),
        );
        b.inner.vigia.replace(Some(h));
        b
    }

    pub fn al_cambiar(&self, f: impl Fn(Option<&Estado>) + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    fn poner(&self, e: Option<Estado>) {
        if *self.inner.estado.borrow() == e {
            return;
        }
        self.inner.estado.replace(e.clone());
        let oyentes = self.inner.oyentes.borrow().clone();
        for f in oyentes {
            f(e.as_ref());
        }
    }
}

async fn leer_ruta(ruta: &str) -> zbus::Result<Crudo> {
    let p = sistema::proxy(UPOWER, ruta.to_owned(), DEVICE).await?;
    Ok(Crudo {
        presente: p.get_property("IsPresent").await?,
        tipo: p.get_property("Type").await?,
        estado: p.get_property("State").await?,
        porcentaje: p.get_property("Percentage").await?,
        vaciar: p.get_property("TimeToEmpty").await?,
        llenar: p.get_property("TimeToFull").await?,
        aviso: p.get_property("WarningLevel").await?,
    })
}

async fn leer() -> zbus::Result<Crudo> {
    leer_ruta(RUTA).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crudo(estado: u32, porcentaje: f64, aviso: u32) -> Crudo {
        Crudo {
            presente: true,
            tipo: TIPO_BATERIA,
            estado,
            porcentaje,
            vaciar: 7800,
            llenar: 2400,
            aviso,
        }
    }

    #[test]
    fn sin_bateria_no_hay_nada() {
        // El `DisplayDevice` de una PC de escritorio.
        let pc = Crudo::default();
        assert_eq!(pc.estado(), None);
        let ups = Crudo {
            tipo: 3,
            ..crudo(2, 50.0, 1)
        };
        assert_eq!(ups.estado(), None);
    }

    #[test]
    fn el_tiempo_es_el_que_corresponde() {
        let e = crudo(2, 73.0, 1).estado().unwrap();
        assert_eq!((e.carga, e.segundos), (Carga::Descargando, 7800));
        let e = crudo(1, 73.0, 1).estado().unwrap();
        assert_eq!((e.carga, e.segundos), (Carga::Cargando, 2400));
        let e = crudo(4, 100.0, 1).estado().unwrap();
        assert_eq!((e.carga, e.segundos), (Carga::Llena, 0));
    }

    #[test]
    fn glifos() {
        let g = |porcentaje, carga| {
            Estado {
                porcentaje,
                carga,
                ..Estado::default()
            }
            .glifo()
        };
        assert_eq!(g(100.0, Carga::Descargando), "󰁹");
        assert_eq!(g(73.0, Carga::Descargando), "󰂀");
        assert_eq!(g(2.0, Carga::Descargando), "󰂎");
        assert_eq!(g(35.0, Carga::Cargando), "󰂈", "35 redondea a 40");
        assert_eq!(g(100.0, Carga::Llena), "󰂅");
        assert_eq!(g(80.0, Carga::Enchufada), "󰂁");
        // Los once de cada lado son distintos.
        for t in [DESCARGANDO, CARGANDO] {
            let mut v = t.to_vec();
            v.sort();
            v.dedup();
            assert_eq!(v.len(), 11);
        }
    }

    /// Contra el UPower de la máquina: `MORGAN_UPOWER_RUTA=/org/freedesktop/UPower/devices/…
    /// cargo test -p morgan bateria::tests::upower_real -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn upower_real() {
        let ruta = std::env::var("MORGAN_UPOWER_RUTA").unwrap_or(RUTA.into());
        let c = futures_lite::future::block_on(leer_ruta(&ruta)).expect("UPower contesta");
        println!("{ruta}: {c:?} → {:?}", c.estado());
        assert!((0.0..=100.0).contains(&c.porcentaje));
    }

    #[test]
    fn tiempos() {
        assert_eq!(tiempo(7800), "2 h 10 min");
        assert_eq!(tiempo(2400), "40 min");
        assert_eq!(tiempo(7200), "2 h");
        assert_eq!(tiempo(10), "1 min");
    }

    #[test]
    fn baja_la_decide_upower_y_solo_descargando() {
        assert!(crudo(2, 40.0, 3).estado().unwrap().baja, "UPower manda");
        assert!(!crudo(2, 15.0, 1).estado().unwrap().baja);
        assert!(
            !crudo(1, 4.0, 4).estado().unwrap().baja,
            "cargando no alarma"
        );
    }

    #[test]
    fn la_fila() {
        let e = crudo(2, 72.6, 1).estado().unwrap();
        assert_eq!(
            (e.nombre(), e.por_ciento(), e.detalle()),
            ("Batería", "73%".into(), "2 h 10 min".into())
        );
        let e = crudo(1, 40.0, 1).estado().unwrap();
        assert_eq!((e.nombre(), e.detalle()), ("Cargando", "40 min".into()));
        let e = Crudo {
            vaciar: 0,
            ..crudo(2, 50.0, 1)
        }
        .estado()
        .unwrap();
        assert_eq!(e.detalle(), "");
        let e = crudo(4, 100.0, 1).estado().unwrap();
        assert_eq!((e.nombre(), e.detalle()), ("Llena", String::new()));
    }
}
