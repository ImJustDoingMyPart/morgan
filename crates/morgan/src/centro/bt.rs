//! La lista de bluetooth: el adaptador, los emparejados (deslizar a la derecha los olvida) y
//! los que están cerca. Elegir uno cercano lo empareja con el campo debajo de su renglón, que
//! muestra lo que pida el agente.

use zbus::zvariant::OwnedObjectPath;

use super::acciones::Parte;
use super::hilo::Orden;
use super::vista::{BotonCampo, Caja, Campo, Contenido, Deslizar, ListaId, Opcion};
use super::{BT, Centro, resumen};
use crate::bluetooth::{self, Dispositivo, Falla, Pedido};

pub struct Emparejo {
    ruta: OwnedObjectPath,
    generacion: u64,
    caja: Option<CajaBt>,
    aviso: Option<(String, bool)>,
    ocupado: bool,
    respuesta: Option<Respuesta>,
    /// Ya terminó mal: el campo solo queda para leer el aviso.
    fallo: bool,
}

/// Lo que muestra la caja: un código para leer (y quizás confirmar) o para escribir.
#[derive(Clone)]
enum CajaBt {
    Codigo { codigo: String, confirmar: bool },
    Pedir { placeholder: &'static str },
}

enum Respuesta {
    Confirmar(async_channel::Sender<bool>),
    Escribir(async_channel::Sender<Option<String>>),
}

impl Respuesta {
    fn rechazar(self) {
        match self {
            Respuesta::Confirmar(r) => {
                let _ = r.try_send(false);
            }
            Respuesta::Escribir(r) => {
                let _ = r.try_send(None);
            }
        }
    }
}

impl Centro {
    pub(super) fn pintar_bt(&self) {
        let i = &self.inner;
        let e = i.bluetooth.estado();
        self.poner_tarjeta(BT, &resumen::bluetooth(&e), false);
        let a = &i.acciones;
        let p = Parte::Lista(ListaId::Bluetooth);
        a.soltar(p);
        let b = i.bluetooth.clone();
        let mut ops = vec![Opcion::new(
            "󰂯",
            tr!("Bluetooth"),
            Some(e.encendido),
            a.tocar(p, move || b.alternar()),
        )];
        if e.encendido {
            for d in &e.dispositivos {
                let b = i.bluetooth.clone();
                let ruta = d.ruta.clone();
                let yo = self.clone();
                let ruta2 = d.ruta.clone();
                let mut o = Opcion::new(
                    d.glifo(),
                    &d.nombre,
                    Some(d.conectado),
                    a.tocar(p, move || b.alternar_dispositivo(&ruta)),
                );
                o.derecha = Some(Deslizar {
                    glifo: "󰆴",
                    texto: tr!("olvidar").into(),
                    se_va: true,
                    accion: a.tocar(p, move || {
                        let yo2 = yo.clone();
                        yo.inner.bluetooth.olvidar(&ruta2, move || yo2.pintar_bt());
                    }),
                });
                ops.push(o);
            }
            let emparejando = i.emparejo.borrow().as_ref().map(|x| x.ruta.clone());
            for d in &e.cercanos {
                let yo = self.clone();
                let d2 = d.clone();
                let mut o = Opcion::new(
                    d.glifo(),
                    &d.nombre,
                    None,
                    a.tocar(p, move || yo.elegir_bt(&d2)),
                );
                // Recién emparejado ya no está entre los cercanos: el campo espera afuera.
                if emparejando.as_ref() == Some(&d.ruta) {
                    o.abierta = true;
                    o.campo = self.campo_bt();
                }
                ops.push(o);
            }
        }
        self.mandar(Orden::Lista(ListaId::Bluetooth, Contenido::Opciones(ops)));
    }

    fn campo_bt(&self) -> Option<Campo> {
        let i = &self.inner;
        let e = i.emparejo.borrow();
        let x = e.as_ref()?;
        let a = &i.acciones;
        let p = Parte::Campo(ListaId::Bluetooth);
        a.soltar(p);
        let yo = self.clone();
        let enviar = a.enviar(p, move |s| yo.enviar_codigo(s));
        let yo = self.clone();
        let escape = a.tocar(p, move || yo.cancelar_emparejo());
        let caja = x.caja.clone().map(|c| match c {
            CajaBt::Codigo { codigo, confirmar } => {
                let boton = confirmar.then(|| {
                    let yo = self.clone();
                    BotonCampo::Tilde(a.tocar(p, move || yo.confirmar_codigo()))
                });
                Caja {
                    glifo: "\u{F0306}",
                    placeholder: String::new(),
                    clave: false,
                    fijo: Some(codigo),
                    boton,
                    ocupado: x.ocupado,
                    enviar,
                    escape,
                }
            }
            CajaBt::Pedir { placeholder } => Caja {
                glifo: "\u{F0306}",
                placeholder: placeholder.into(),
                clave: false,
                fijo: None,
                boton: None,
                ocupado: x.ocupado,
                enviar,
                escape,
            },
        });
        Some(Campo {
            generacion: x.generacion,
            caja,
            aviso: x.aviso.clone(),
        })
    }

    /// Otro click en el que se está emparejando lo cancela.
    fn elegir_bt(&self, d: &Dispositivo) {
        let i = &self.inner;
        let mismo = i
            .emparejo
            .borrow()
            .as_ref()
            .is_some_and(|p| p.ruta == d.ruta);
        self.cancelar_emparejo();
        if mismo {
            return;
        }
        i.emparejo.replace(Some(Emparejo {
            ruta: d.ruta.clone(),
            generacion: self.generacion(),
            caja: None,
            aviso: Some((tr!("Emparejando…").into(), false)),
            ocupado: false,
            respuesta: None,
            fallo: false,
        }));
        self.pintar_bt();
        let yo = self.clone();
        let ruta = d.ruta.clone();
        i.bluetooth.emparejar(&d.ruta, move |r| {
            let mut e = yo.inner.emparejo.borrow_mut();
            let Some(p) = e.as_mut().filter(|p| p.ruta == ruta) else {
                return;
            };
            match r {
                Ok(()) | Err(Falla::Cancelado) => {
                    e.take();
                }
                Err(Falla::Otra) => {
                    p.fallo = true;
                    p.respuesta.take();
                    p.caja = None;
                    p.aviso = Some((tr!("No se pudo emparejar").into(), true));
                }
            }
            drop(e);
            yo.pintar_bt();
        });
    }

    /// Escape, otro click o el centro que se cierra.
    pub(super) fn cancelar_emparejo(&self) {
        let Some(mut p) = self.inner.emparejo.take() else {
            return;
        };
        if let Some(r) = p.respuesta.take() {
            r.rechazar();
        }
        if !p.fallo {
            self.inner.bluetooth.cancelar_emparejar(&p.ruta);
        }
        self.pintar_bt();
    }

    pub(super) fn atender_pedido(&self, pedido: Pedido) {
        let mut e = self.inner.emparejo.borrow_mut();
        let es_el = |d: &OwnedObjectPath| e.as_ref().is_some_and(|p| &p.ruta == d && !p.fallo);
        match pedido {
            Pedido::Confirmar {
                dispositivo,
                codigo,
                respuesta,
            } if es_el(&dispositivo) => {
                let g = self.generacion();
                let p = e.as_mut().expect("es_el");
                p.generacion = g;
                p.caja = Some(CajaBt::Codigo {
                    codigo,
                    confirmar: true,
                });
                p.aviso = Some((
                    tr!("¿Es el mismo código que muestra el dispositivo?").into(),
                    false,
                ));
                p.respuesta = Some(Respuesta::Confirmar(respuesta));
            }
            Pedido::Mostrar {
                dispositivo,
                codigo,
            } if es_el(&dispositivo) => {
                let g = self.generacion();
                let p = e.as_mut().expect("es_el");
                p.generacion = g;
                p.caja = Some(CajaBt::Codigo {
                    codigo,
                    confirmar: false,
                });
                p.aviso = Some((
                    tr!("Se escribe en el dispositivo, seguido de Enter").into(),
                    false,
                ));
            }
            Pedido::Escribir {
                dispositivo,
                numerico,
                respuesta,
            } if es_el(&dispositivo) => {
                let g = self.generacion();
                let p = e.as_mut().expect("es_el");
                p.generacion = g;
                let (placeholder, aviso) = if numerico {
                    (tr!("Código"), tr!("El que muestra el dispositivo"))
                } else {
                    (tr!("PIN"), tr!("Suele ser 0000 o 1234"))
                };
                p.caja = Some(CajaBt::Pedir { placeholder });
                p.aviso = Some((aviso.into(), false));
                p.respuesta = Some(Respuesta::Escribir(respuesta));
            }
            Pedido::Cancelado => return,
            otro => {
                bluetooth::rechazar(otro);
                return;
            }
        }
        drop(e);
        self.pintar_bt();
    }

    fn confirmar_codigo(&self) {
        let mut e = self.inner.emparejo.borrow_mut();
        let Some(p) = e.as_mut() else { return };
        if let Some(Respuesta::Confirmar(r)) = p.respuesta.take() {
            let _ = r.try_send(true);
            p.ocupado = true;
            p.aviso = Some((tr!("Emparejando…").into(), false));
            drop(e);
            self.pintar_bt();
        }
    }

    fn enviar_codigo(&self, texto: String) {
        let texto = texto.trim().to_owned();
        if texto.is_empty() {
            return;
        }
        let mut e = self.inner.emparejo.borrow_mut();
        let Some(p) = e.as_mut() else { return };
        if let Some(Respuesta::Escribir(r)) = p.respuesta.take() {
            let _ = r.try_send(Some(texto));
            p.ocupado = true;
            p.aviso = Some((tr!("Emparejando…").into(), false));
            drop(e);
            self.pintar_bt();
        }
    }
}
