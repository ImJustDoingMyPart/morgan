//! Un cambio se pide y el estado se relee cuando el servidor avisa (suscripción de pulse), no
//! al soltar el slider. La conexión vive solo mientras el centro está abierto.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::subscribe::InterestMaskSet;
use pulse::volume::{ChannelVolumes, Volume};

use crate::audio::Conexion;

#[derive(Debug, Clone, PartialEq)]
pub struct Salida {
    pub indice: u32,
    pub nombre: String,
    pub descripcion: String,
    pub puerto: Option<String>,
    pub volumen: f64,
    pub mudo: bool,
    canales: u8,
}

impl Salida {
    pub fn glifo(&self) -> &'static str {
        let p = self.puerto.as_deref().unwrap_or_default();
        if p.contains("hdmi") || self.nombre.contains("hdmi") {
            "󰟴"
        } else if p.contains("headphones") || p.contains("headset") {
            "󰋋"
        } else {
            "󰓃"
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entrada {
    pub indice: u32,
    pub nombre: String,
    pub descripcion: String,
    pub volumen: f64,
    pub mudo: bool,
    canales: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub indice: u32,
    pub nombre: String,
    pub icono: Option<String>,
    pub salida: u32,
    pub volumen: f64,
    pub mudo: bool,
    canales: u8,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Estado {
    pub salidas: Vec<Salida>,
    pub predeterminada: Option<String>,
    pub entradas: Vec<Entrada>,
    pub entrada_predeterminada: Option<String>,
    pub apps: Vec<App>,
}

impl Estado {
    pub fn salida_predeterminada(&self) -> Option<&Salida> {
        let p = self.predeterminada.as_deref()?;
        self.salidas.iter().find(|s| s.nombre == p)
    }

    pub fn entrada_predeterminada(&self) -> Option<&Entrada> {
        let p = self.entrada_predeterminada.as_deref()?;
        self.entradas.iter().find(|e| e.nombre == p)
    }
}

pub fn volumen(pct: f64) -> Volume {
    Volume((f64::from(Volume::NORMAL.0) * pct.max(0.0) / 100.0).round() as u32)
}

pub fn porcentaje(v: Volume) -> f64 {
    (f64::from(v.0) * 100.0 / f64::from(Volume::NORMAL.0)).round()
}

#[derive(Clone)]
pub struct Mezclador {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    conexion: RefCell<Option<Conexion>>,
    estado: RefCell<Estado>,
    oyentes: RefCell<Vec<Rc<dyn Fn()>>>,
    pendiente: Cell<bool>,
    /// Sube en cada apertura: una relectura que termina tarde no pisa la de otra conexión.
    generacion: Cell<u64>,
}

impl Mezclador {
    pub fn new() -> Self {
        Mezclador {
            inner: Rc::default(),
        }
    }

    pub fn al_cambiar(&self, f: impl Fn() + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    pub fn estado(&self) -> Estado {
        self.inner.estado.borrow().clone()
    }

    pub fn abrir(&self) {
        let vuelta = self.inner.generacion.get() + 1;
        self.inner.generacion.set(vuelta);
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let c = match Conexion::abrir().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("morgan: mezclador: {e}");
                    return;
                }
            };
            if yo.inner.generacion.get() != vuelta {
                return;
            }
            let weak = Rc::downgrade(&yo.inner);
            {
                let mut ctx = c.context.borrow_mut();
                ctx.set_subscribe_callback(Some(Box::new(move |_, _, _| {
                    if let Some(inner) = weak.upgrade() {
                        Mezclador { inner }.programar();
                    }
                })));
                ctx.subscribe(
                    InterestMaskSet::SINK
                        | InterestMaskSet::SINK_INPUT
                        | InterestMaskSet::SOURCE
                        | InterestMaskSet::SERVER,
                    |_| {},
                );
            }
            yo.inner.conexion.replace(Some(c));
            yo.programar();
        });
    }

    pub fn cerrar(&self) {
        self.inner.generacion.set(self.inner.generacion.get() + 1);
        if let Some(c) = self.inner.conexion.take() {
            c.context.borrow_mut().set_subscribe_callback(None);
            c.context.borrow_mut().disconnect();
        }
    }

    pub fn volumen_salida(&self, nombre: &str, pct: f64) {
        let Some(s) = self
            .inner
            .estado
            .borrow()
            .salidas
            .iter()
            .find(|s| s.nombre == nombre)
            .cloned()
        else {
            return;
        };
        self.con_contexto(|ctx| {
            let mut cv = ChannelVolumes::default();
            cv.set(s.canales.max(1), volumen(pct));
            ctx.introspect()
                .set_sink_volume_by_name(&s.nombre, &cv, None);
        });
    }

    pub fn silenciar_salida(&self, nombre: &str, mudo: bool) {
        let nombre = nombre.to_owned();
        self.con_contexto(|ctx| {
            ctx.introspect().set_sink_mute_by_name(&nombre, mudo, None);
        });
    }

    pub fn volumen_entrada(&self, nombre: &str, pct: f64) {
        let Some(e) = self
            .inner
            .estado
            .borrow()
            .entradas
            .iter()
            .find(|e| e.nombre == nombre)
            .cloned()
        else {
            return;
        };
        self.con_contexto(|ctx| {
            let mut cv = ChannelVolumes::default();
            cv.set(e.canales.max(1), volumen(pct));
            ctx.introspect()
                .set_source_volume_by_name(&e.nombre, &cv, None);
        });
    }

    pub fn silenciar_entrada(&self, nombre: &str, mudo: bool) {
        let nombre = nombre.to_owned();
        self.con_contexto(|ctx| {
            ctx.introspect()
                .set_source_mute_by_name(&nombre, mudo, None);
        });
    }

    pub fn predeterminar_entrada(&self, nombre: &str) {
        let nombre = nombre.to_owned();
        self.con_contexto(|ctx| {
            ctx.set_default_source(&nombre, |_| {});
        });
    }

    pub fn predeterminar(&self, nombre: &str) {
        let nombre = nombre.to_owned();
        self.con_contexto(|ctx| {
            ctx.set_default_sink(&nombre, |_| {});
        });
    }

    pub fn volumen_app(&self, indice: u32, pct: f64) {
        let Some(canales) = self
            .inner
            .estado
            .borrow()
            .apps
            .iter()
            .find(|a| a.indice == indice)
            .map(|a| a.canales)
        else {
            return;
        };
        self.con_contexto(|ctx| {
            let mut cv = ChannelVolumes::default();
            cv.set(canales.max(1), volumen(pct));
            ctx.introspect().set_sink_input_volume(indice, &cv, None);
        });
    }

    pub fn silenciar_app(&self, indice: u32, mudo: bool) {
        self.con_contexto(|ctx| {
            ctx.introspect().set_sink_input_mute(indice, mudo, None);
        });
    }

    pub fn mover_app(&self, indice: u32, salida: &str) {
        let salida = salida.to_owned();
        self.con_contexto(|ctx| {
            ctx.introspect()
                .move_sink_input_by_name(indice, &salida, None);
        });
    }

    fn con_contexto(&self, f: impl FnOnce(&mut pulse::context::Context)) {
        if let Some(c) = self.inner.conexion.borrow().as_ref() {
            f(&mut c.context.borrow_mut());
        }
    }

    /// Arrastrar un slider son decenas de eventos: una sola relectura.
    fn programar(&self) {
        if self.inner.pendiente.replace(true) {
            return;
        }
        let yo = self.clone();
        glib::idle_add_local_once(move || {
            yo.inner.pendiente.set(false);
            glib::MainContext::default().spawn_local(async move { yo.releer().await });
        });
    }

    async fn releer(&self) {
        let vuelta = self.inner.generacion.get();
        let introspect = match self.inner.conexion.borrow().as_ref() {
            Some(c) => c.context.borrow().introspect(),
            None => return,
        };

        let (tx, rx) = async_channel::bounded(1);
        introspect.get_server_info(move |i| {
            let _ = tx.try_send((
                i.default_sink_name.as_ref().map(|n| n.to_string()),
                i.default_source_name.as_ref().map(|n| n.to_string()),
            ));
        });

        let (tx_s, rx_s) = async_channel::unbounded();
        introspect.get_sink_info_list(move |r| match r {
            ListResult::Item(i) => {
                let _ = tx_s.try_send(Some(Salida {
                    indice: i.index,
                    nombre: i.name.as_deref().unwrap_or_default().to_owned(),
                    descripcion: i
                        .description
                        .as_deref()
                        .or(i.name.as_deref())
                        .unwrap_or_default()
                        .to_owned(),
                    puerto: i
                        .active_port
                        .as_ref()
                        .and_then(|p| p.name.as_deref().map(str::to_owned)),
                    volumen: porcentaje(i.volume.avg()),
                    mudo: i.mute,
                    canales: i.volume.len(),
                }));
            }
            ListResult::End | ListResult::Error => {
                let _ = tx_s.try_send(None);
            }
        });

        // Los monitores de las salidas también son fuentes: no son un micrófono.
        let (tx_e, rx_e) = async_channel::unbounded();
        introspect.get_source_info_list(move |r| match r {
            ListResult::Item(i) => {
                if i.monitor_of_sink.is_some() {
                    return;
                }
                let _ = tx_e.try_send(Some(Entrada {
                    indice: i.index,
                    nombre: i.name.as_deref().unwrap_or_default().to_owned(),
                    descripcion: i
                        .description
                        .as_deref()
                        .or(i.name.as_deref())
                        .unwrap_or_default()
                        .to_owned(),
                    volumen: porcentaje(i.volume.avg()),
                    mudo: i.mute,
                    canales: i.volume.len(),
                }));
            }
            ListResult::End | ListResult::Error => {
                let _ = tx_e.try_send(None);
            }
        });

        let (tx_a, rx_a) = async_channel::unbounded();
        introspect.get_sink_input_info_list(move |r| match r {
            ListResult::Item(i) => {
                if !i.has_volume {
                    return;
                }
                let prop = |k: &str| i.proplist.get_str(k).filter(|v| !v.is_empty());
                let _ = tx_a.try_send(Some(App {
                    indice: i.index,
                    nombre: prop("application.name")
                        .or_else(|| i.name.as_deref().map(str::to_owned))
                        .unwrap_or_default(),
                    icono: prop("application.icon_name"),
                    salida: i.sink,
                    volumen: porcentaje(i.volume.avg()),
                    mudo: i.mute,
                    canales: i.volume.len(),
                }));
            }
            ListResult::End | ListResult::Error => {
                let _ = tx_a.try_send(None);
            }
        });

        let (predeterminada, entrada_predeterminada) = rx.recv().await.unwrap_or_default();
        let mut salidas = Vec::new();
        while let Ok(Some(s)) = rx_s.recv().await {
            salidas.push(s);
        }
        let mut entradas = Vec::new();
        while let Ok(Some(e)) = rx_e.recv().await {
            entradas.push(e);
        }
        let mut apps = Vec::new();
        while let Ok(Some(a)) = rx_a.recv().await {
            apps.push(a);
        }
        if self.inner.generacion.get() != vuelta {
            return;
        }
        salidas.sort_by(|a, b| a.descripcion.cmp(&b.descripcion));
        entradas.sort_by(|a, b| a.descripcion.cmp(&b.descripcion));
        apps.sort_by_key(|a| a.indice);
        let nuevo = Estado {
            salidas,
            predeterminada,
            entradas,
            entrada_predeterminada,
            apps,
        };
        if *self.inner.estado.borrow() == nuevo {
            return;
        }
        self.inner.estado.replace(nuevo);
        let oyentes = self.inner.oyentes.borrow().clone();
        for f in oyentes {
            f();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ida_y_vuelta_de_porcentajes() {
        for p in [0.0, 1.0, 30.0, 50.0, 99.0, 100.0, 150.0] {
            assert_eq!(porcentaje(volumen(p)), p);
        }
        assert_eq!(volumen(100.0), Volume::NORMAL);
        assert_eq!(volumen(-5.0), Volume(0));
    }

    #[test]
    fn predeterminada() {
        let s = |n: &str| Salida {
            indice: 0,
            nombre: n.into(),
            descripcion: n.into(),
            puerto: None,
            volumen: 50.0,
            mudo: false,
            canales: 2,
        };
        let e = Estado {
            salidas: vec![s("a"), s("b")],
            predeterminada: Some("b".into()),
            ..Default::default()
        };
        assert_eq!(e.salida_predeterminada().unwrap().nombre, "b");
        assert!(Estado::default().salida_predeterminada().is_none());
    }

    #[test]
    fn glifo_por_puerto() {
        let s = |nombre: &str, puerto: &str| Salida {
            indice: 0,
            nombre: nombre.into(),
            descripcion: String::new(),
            puerto: Some(puerto.into()),
            volumen: 0.0,
            mudo: false,
            canales: 2,
        };
        assert_eq!(
            s("alsa_output.pci.hdmi-stereo", "hdmi-output-4").glifo(),
            "󰟴"
        );
        assert_eq!(
            s("alsa_output.pci.analog", "analog-output-headphones").glifo(),
            "󰋋"
        );
        assert_eq!(s("alsa_output.usb.analog", "analog-output").glifo(), "󰓃");
    }
}
