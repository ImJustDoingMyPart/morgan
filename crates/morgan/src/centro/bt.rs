//! La lista de bluetooth: el adaptador, los emparejados (deslizar a la derecha los olvida) y
//! los que están cerca. Elegir uno cercano lo empareja con la caja de `campo.rs` debajo de
//! su renglón, que muestra lo que pida el agente; mientras está abierta, la lista no se
//! rehace (como la de la clave de una Wi-Fi).

use std::rc::Rc;

use gtk::prelude::*;

use zbus::zvariant::OwnedObjectPath;

use super::campo::Campo;
use super::desplegable::Item;
use super::{Centro, resumen};
use crate::bluetooth::{self, Dispositivo, Falla, Pedido};
use crate::deslizable::Accion;

pub struct Emparejo {
    ruta: OwnedObjectPath,
    campo: Campo,
    respuesta: Option<Respuesta>,
    /// Ya terminó mal: la caja solo queda para leer el aviso.
    fallo: bool,
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
    pub(super) fn mostrar_bluetooth(&self) {
        self.pintar_bt(false);
    }

    pub(super) fn pintar_bt(&self, forzar: bool) {
        let i = &self.inner;
        let Some((tarjeta, lista)) = i.bt.borrow().clone() else {
            return;
        };
        let e = i.bluetooth.estado();
        tarjeta.poner(&resumen::bluetooth(&e));
        if !forzar && i.emparejo.borrow().is_some() {
            return;
        }
        let b = i.bluetooth.clone();
        let mut items = vec![Item::new(
            "󰂯",
            tr!("Bluetooth"),
            Some(e.encendido),
            move || b.alternar(),
        )];
        if e.encendido {
            for d in &e.dispositivos {
                let b = i.bluetooth.clone();
                let ruta = d.ruta.clone();
                let yo = self.clone();
                let ruta2 = d.ruta.clone();
                items.push(
                    Item::new(d.glifo(), &d.nombre, Some(d.conectado), move || {
                        b.alternar_dispositivo(&ruta)
                    })
                    .deslizable(Accion {
                        glifo: "󰆴",
                        texto: tr!("olvidar"),
                        se_va: true,
                        f: Rc::new(move || {
                            let yo2 = yo.clone();
                            yo.inner
                                .bluetooth
                                .olvidar(&ruta2, move || yo2.pintar_bt(true));
                        }),
                    }),
                );
            }
            for d in &e.cercanos {
                let yo = self.clone();
                let d2 = d.clone();
                items.push(Item::new(d.glifo(), &d.nombre, None, move || {
                    yo.elegir_bt(&d2)
                }));
            }
        }
        let antes = 1 + if e.encendido { e.dispositivos.len() } else { 0 };
        let botones = lista.llenar(items);
        let Some((ruta, campo)) = i
            .emparejo
            .borrow()
            .as_ref()
            .map(|p| (p.ruta.clone(), p.campo.clone()))
        else {
            return;
        };
        // Recién emparejado ya no está entre los cercanos: la caja espera afuera.
        let Some(n) = e.cercanos.iter().position(|d| d.ruta == ruta) else {
            return;
        };
        let Some(boton) = botones.get(antes + n) else {
            return;
        };
        boton.add_css_class("abierto");
        if let Some(p) = campo.raiz.parent()
            && let Some(b) = p.downcast_ref::<gtk::Box>()
        {
            b.remove(&campo.raiz);
        }
        lista
            .opciones()
            .insert_child_after(&campo.raiz, Some(&lista.fila(boton)));
        campo.enfocar();
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
        let campo = Campo::new("\u{F0306}");
        campo.solo_aviso(tr!("Emparejando…"), false);
        let yo = self.clone();
        campo.al_enviar(move || yo.enviar_codigo());
        let yo = self.clone();
        campo.al_escape(move || yo.cancelar_emparejo());
        i.emparejo.replace(Some(Emparejo {
            ruta: d.ruta.clone(),
            campo,
            respuesta: None,
            fallo: false,
        }));
        self.pintar_bt(true);
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
                    drop(e);
                    yo.pintar_bt(true);
                }
                Err(Falla::Otra) => {
                    p.fallo = true;
                    p.respuesta.take();
                    p.campo.solo_aviso(tr!("No se pudo emparejar"), true);
                }
            }
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
        self.pintar_bt(true);
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
                let p = e.as_mut().expect("es_el");
                let yo = self.clone();
                p.campo
                    .codigo(&codigo, Some(Rc::new(move || yo.confirmar_codigo())));
                p.campo
                    .avisar(tr!("¿Es el mismo código que muestra el dispositivo?"), false);
                p.campo.enfocar();
                p.respuesta = Some(Respuesta::Confirmar(respuesta));
            }
            Pedido::Mostrar {
                dispositivo,
                codigo,
            } if es_el(&dispositivo) => {
                let p = e.as_ref().expect("es_el");
                p.campo.codigo(&codigo, None);
                p.campo
                    .avisar(tr!("Se escribe en el dispositivo, seguido de Enter"), false);
            }
            Pedido::Escribir {
                dispositivo,
                numerico,
                respuesta,
            } if es_el(&dispositivo) => {
                let p = e.as_mut().expect("es_el");
                if numerico {
                    p.campo.pedir(tr!("Código"));
                    p.campo
                        .avisar(tr!("El que muestra el dispositivo"), false);
                } else {
                    p.campo.pedir(tr!("PIN"));
                    p.campo.avisar(tr!("Suele ser 0000 o 1234"), false);
                }
                p.campo.enfocar();
                p.respuesta = Some(Respuesta::Escribir(respuesta));
            }
            Pedido::Cancelado => {}
            otro => bluetooth::rechazar(otro),
        }
    }

    fn confirmar_codigo(&self) {
        let mut e = self.inner.emparejo.borrow_mut();
        let Some(p) = e.as_mut() else { return };
        if let Some(Respuesta::Confirmar(r)) = p.respuesta.take() {
            let _ = r.try_send(true);
            p.campo.ocupar(true);
            p.campo.avisar(tr!("Emparejando…"), false);
        }
    }

    fn enviar_codigo(&self) {
        let mut e = self.inner.emparejo.borrow_mut();
        let Some(p) = e.as_mut() else { return };
        let texto = p.campo.texto.text().trim().to_owned();
        if texto.is_empty() {
            return;
        }
        if let Some(Respuesta::Escribir(r)) = p.respuesta.take() {
            let _ = r.try_send(Some(texto));
            p.campo.ocupar(true);
            p.campo.avisar(tr!("Emparejando…"), false);
        }
    }
}
