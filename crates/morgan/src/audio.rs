//! Cada pedido lee el dispositivo, le pide el cambio y vuelve a leerlo: lo que se muestra es
//! lo que el sistema responde.
//!
//! Los pedidos van en fila, de a uno: con la tecla sostenida llegan varios seguidos, y dos que
//! leen el mismo nivel antes de escribir perderían un paso.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use libpulse_binding as pulse;
use libpulse_glib_binding::Mainloop;
use morgan_ipc::AccionVolumen;
use pulse::callbacks::ListResult;
use pulse::context::{Context, FlagSet, State};
use pulse::volume::{ChannelVolumes, Volume};

use crate::osd::{Aviso, Cuerpo};

#[derive(Debug, Clone, PartialEq)]
pub struct Nivel {
    pub nombre: String,
    /// Puede pasar de 100 si otro programa lo subió.
    pub volumen: f64,
    pub silenciado: bool,
}

pub fn glifo(microfono: bool, nivel: &Nivel) -> &'static str {
    let mudo = nivel.silenciado || nivel.volumen <= 0.0;
    match (microfono, nivel.volumen) {
        (true, _) if mudo => "󰍭",
        (true, _) => "󰍬",
        (false, _) if mudo => "󰖁",
        (false, v) if v <= 33.0 => "󰕿",
        (false, v) if v <= 66.0 => "󰖀",
        _ => "󰕾",
    }
}

pub fn aviso(microfono: bool, nivel: Nivel, ms: u32) -> Aviso {
    Aviso {
        icono: Some(glifo(microfono, &nivel).into()),
        titulo: nivel.nombre,
        cuerpo: Cuerpo::Barra {
            fraccion: (nivel.volumen / 100.0).clamp(0.0, 1.0),
            porcentaje: nivel.volumen,
            apagada: nivel.silenciado,
        },
        ms,
    }
}

#[derive(Clone)]
pub struct Audio {
    fila: async_channel::Sender<Pedido>,
    ajustes: Rc<Cell<(f64, f64)>>,
}

struct Pedido {
    accion: AccionVolumen,
    paso: f64,
    tope: f64,
    microfono: bool,
    dispositivo: Option<String>,
    respuesta: async_channel::Sender<Result<Nivel, String>>,
}

impl Audio {
    pub fn new(c: &crate::config::Sonido) -> Self {
        let (fila, pedidos) = async_channel::unbounded();
        glib::MainContext::default().spawn_local(atender(pedidos));
        let audio = Audio {
            fila,
            ajustes: Rc::default(),
        };
        audio.aplicar_config(c);
        audio
    }

    pub fn aplicar_config(&self, c: &crate::config::Sonido) {
        self.ajustes.set((f64::from(c.paso), c.tope()));
    }

    pub async fn cambiar(
        &self,
        accion: AccionVolumen,
        microfono: bool,
        dispositivo: Option<String>,
    ) -> Result<Nivel, String> {
        let (respuesta, r) = async_channel::bounded(1);
        let (paso, tope) = self.ajustes.get();
        let pedido = Pedido {
            accion,
            paso,
            tope,
            microfono,
            dispositivo,
            respuesta,
        };
        self.fila
            .send(pedido)
            .await
            .map_err(|_| "audio: la fila está cerrada".to_string())?;
        r.recv()
            .await
            .map_err(|_| "audio: el pedido quedó sin respuesta".to_string())?
    }
}

async fn atender(pedidos: async_channel::Receiver<Pedido>) {
    let mut conexion: Option<Conexion> = None;
    while let Ok(p) = pedidos.recv().await {
        let viva = conexion
            .as_ref()
            .is_some_and(|c| c.context.borrow().get_state() == State::Ready);
        if !viva {
            conexion = match Conexion::abrir().await {
                Ok(c) => Some(c),
                Err(e) => {
                    let _ = p.respuesta.send(Err(e)).await;
                    continue;
                }
            };
        }
        let c = conexion.as_ref().expect("recién abierta");
        let r = c
            .aplicar(
                p.accion,
                p.paso,
                p.tope,
                p.microfono,
                p.dispositivo.as_deref(),
            )
            .await;
        let _ = p.respuesta.send(r).await;
    }
}

pub(crate) struct Conexion {
    // El contexto se suelta antes que el main loop que usa (orden de los campos).
    pub(crate) context: Rc<RefCell<Context>>,
    _mainloop: Mainloop,
}

impl Conexion {
    pub(crate) async fn abrir() -> Result<Self, String> {
        let mainloop =
            Mainloop::new(None).ok_or("audio: no se pudo crear el main loop de pulse")?;
        let mut props = pulse::proplist::Proplist::new().ok_or("audio: proplist")?;
        let _ = props.set_str(pulse::proplist::properties::APPLICATION_NAME, "morgan");
        let context = Context::new_with_proplist(&mainloop, "morgan", &props)
            .ok_or("audio: no se pudo crear el contexto de pulse")?;
        let context = Rc::new(RefCell::new(context));

        let (aviso, cambios) = async_channel::unbounded();
        context
            .borrow_mut()
            .set_state_callback(Some(Box::new(move || {
                let _ = aviso.try_send(());
            })));
        context
            .borrow_mut()
            .connect(None, FlagSet::NOAUTOSPAWN, None)
            .map_err(|e| format!("audio: no se pudo conectar a pulse: {e}"))?;
        loop {
            let estado = context.borrow().get_state();
            match estado {
                State::Ready => break,
                State::Failed | State::Terminated => {
                    return Err("audio: el servidor de pulse rechazó la conexión".into());
                }
                _ => {
                    cambios
                        .recv()
                        .await
                        .map_err(|_| "audio: se perdió el contexto de pulse".to_string())?;
                }
            }
        }
        context.borrow_mut().set_state_callback(None);
        Ok(Conexion {
            context,
            _mainloop: mainloop,
        })
    }

    async fn aplicar(
        &self,
        accion: AccionVolumen,
        paso: f64,
        tope: f64,
        microfono: bool,
        dispositivo: Option<&str>,
    ) -> Result<Nivel, String> {
        let nombre = dispositivo.unwrap_or(if microfono {
            "@DEFAULT_SOURCE@"
        } else {
            "@DEFAULT_SINK@"
        });
        let (volumen, mudo, _) = self.leer(microfono, nombre).await?;
        let paso = Volume((f64::from(Volume::NORMAL.0) * paso / 100.0) as u32);
        let tope = Volume((f64::from(Volume::NORMAL.0) * tope / 100.0) as u32);
        match accion {
            AccionVolumen::Subir => {
                // inc_clamp baja al tope lo que ya estaba por encima; eso no es subir.
                if volumen.max() < tope {
                    let mut v = volumen;
                    v.inc_clamp(paso, tope);
                    self.escribir_volumen(microfono, nombre, &v).await?;
                }
            }
            AccionVolumen::Bajar => {
                let mut v = volumen;
                v.decrease(paso);
                self.escribir_volumen(microfono, nombre, &v).await?;
            }
            AccionVolumen::Silenciar => self.escribir_mudo(microfono, nombre, !mudo).await?,
        }
        let (volumen, mudo, descripcion) = self.leer(microfono, nombre).await?;
        Ok(Nivel {
            nombre: descripcion,
            volumen: (f64::from(volumen.avg().0) * 100.0 / f64::from(Volume::NORMAL.0)).round(),
            silenciado: mudo,
        })
    }

    async fn leer(
        &self,
        microfono: bool,
        nombre: &str,
    ) -> Result<(ChannelVolumes, bool, String), String> {
        let (tx, rx) = async_channel::bounded(1);
        let introspect = self.context.borrow().introspect();
        let error = tx.clone();
        let fallo = move || {
            let _ = error.try_send(None);
        };
        if microfono {
            introspect.get_source_info_by_name(nombre, move |r| match r {
                ListResult::Item(i) => {
                    let d = i.description.as_deref().unwrap_or_default().to_owned();
                    let _ = tx.try_send(Some((i.volume, i.mute, d)));
                }
                ListResult::Error => fallo(),
                ListResult::End => {}
            });
        } else {
            introspect.get_sink_info_by_name(nombre, move |r| match r {
                ListResult::Item(i) => {
                    let d = i.description.as_deref().unwrap_or_default().to_owned();
                    let _ = tx.try_send(Some((i.volume, i.mute, d)));
                }
                ListResult::Error => fallo(),
                ListResult::End => {}
            });
        }
        rx.recv()
            .await
            .ok()
            .flatten()
            .ok_or_else(|| format!("audio: no existe el dispositivo «{nombre}»"))
    }

    async fn escribir_volumen(
        &self,
        microfono: bool,
        nombre: &str,
        v: &ChannelVolumes,
    ) -> Result<(), String> {
        let (tx, rx) = async_channel::bounded(1);
        let cb = Box::new(move |ok| {
            let _ = tx.try_send(ok);
        });
        let mut introspect = self.context.borrow().introspect();
        if microfono {
            introspect.set_source_volume_by_name(nombre, v, Some(cb));
        } else {
            introspect.set_sink_volume_by_name(nombre, v, Some(cb));
        }
        confirmar(rx, nombre).await
    }

    async fn escribir_mudo(&self, microfono: bool, nombre: &str, mudo: bool) -> Result<(), String> {
        let (tx, rx) = async_channel::bounded(1);
        let cb = Box::new(move |ok| {
            let _ = tx.try_send(ok);
        });
        let mut introspect = self.context.borrow().introspect();
        if microfono {
            introspect.set_source_mute_by_name(nombre, mudo, Some(cb));
        } else {
            introspect.set_sink_mute_by_name(nombre, mudo, Some(cb));
        }
        confirmar(rx, nombre).await
    }
}

async fn confirmar(rx: async_channel::Receiver<bool>, nombre: &str) -> Result<(), String> {
    match rx.recv().await {
        Ok(true) => Ok(()),
        _ => Err(format!(
            "audio: el servidor no aplicó el cambio en «{nombre}»"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nivel(volumen: f64, silenciado: bool) -> Nivel {
        Nivel {
            nombre: "Auriculares".into(),
            volumen,
            silenciado,
        }
    }

    #[test]
    fn glifos_por_tramo() {
        let g = |m: bool, v: f64, s: bool| glifo(m, &nivel(v, s));
        assert_eq!(g(false, 0.0, false), "󰖁");
        assert_eq!(g(false, 1.0, false), "󰕿");
        assert_eq!(g(false, 33.0, false), "󰕿");
        assert_eq!(g(false, 34.0, false), "󰖀");
        assert_eq!(g(false, 66.0, false), "󰖀");
        assert_eq!(g(false, 67.0, false), "󰕾");
        assert_eq!(g(false, 150.0, false), "󰕾");
        assert_eq!(g(false, 80.0, true), "󰖁");
        assert_eq!(g(true, 50.0, false), "󰍬");
        assert_eq!(g(true, 50.0, true), "󰍭");
    }

    #[test]
    fn silenciado_muestra_el_nivel_atenuado() {
        let a = aviso(false, nivel(45.0, true), 1000);
        assert_eq!(a.icono.as_deref(), Some("󰖁"));
        assert_eq!(a.titulo, "Auriculares");
        assert_eq!(
            a.cuerpo,
            Cuerpo::Barra {
                fraccion: 0.45,
                porcentaje: 45.0,
                apagada: true
            }
        );
    }

    #[test]
    fn la_barra_no_pasa_del_tope() {
        // La barra se llena; el número dice cuánto es de verdad.
        let a = aviso(false, nivel(130.0, false), 1000);
        assert!(matches!(
            a.cuerpo,
            Cuerpo::Barra { fraccion, porcentaje, .. } if fraccion == 1.0 && porcentaje == 130.0
        ));
    }
}
