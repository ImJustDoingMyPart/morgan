//! Lo marcado sale de NetworkManager, así que dos conexiones excluyentes nunca quedan
//! marcadas a la vez.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::sistema;

const NM: &str = "org.freedesktop.NetworkManager";
const RUTA: &str = "/org/freedesktop/NetworkManager";
const AJUSTES: &str = "/org/freedesktop/NetworkManager/Settings";
/// `NM_DEVICE_TYPE_WIFI` y `NM_DEVICE_STATE_UNAVAILABLE`.
const DISPOSITIVO_WIFI: u32 = 2;
const NO_DISPONIBLE: u32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tipo {
    Cable,
    Wifi,
    Vpn,
}

impl Tipo {
    fn de(t: &str) -> Option<Tipo> {
        match t {
            "802-3-ethernet" => Some(Tipo::Cable),
            "802-11-wireless" => Some(Tipo::Wifi),
            "vpn" | "wireguard" => Some(Tipo::Vpn),
            _ => None,
        }
    }

    pub fn glifo(self) -> &'static str {
        match self {
            Tipo::Cable => "󰈁",
            Tipo::Wifi => "󰤨",
            Tipo::Vpn => "󰖂",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conexion {
    pub ruta: OwnedObjectPath,
    pub nombre: String,
    pub tipo: Tipo,
    pub activa: Option<OwnedObjectPath>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Estado {
    pub wifi: Option<bool>,
    pub conexiones: Vec<Conexion>,
}

pub fn ordenar(c: &mut [Conexion]) {
    c.sort_by(|a, b| a.tipo.cmp(&b.tipo).then_with(|| a.nombre.cmp(&b.nombre)));
}

#[derive(Clone, Default)]
pub struct Red {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    estado: RefCell<Estado>,
    oyentes: RefCell<Vec<Rc<dyn Fn()>>>,
    vigia: RefCell<Option<glib::JoinHandle<()>>>,
}

impl Red {
    pub fn estado(&self) -> Estado {
        self.inner.estado.borrow().clone()
    }

    pub fn al_cambiar(&self, f: impl Fn() + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    pub fn abrir(&self) {
        let yo = self.clone();
        let h = sistema::vigilar(
            sistema::propiedades(NM, RUTA),
            Box::new(move || {
                let yo = yo.clone();
                Box::pin(async move { yo.releer().await })
            }),
        );
        if let Some(viejo) = self.inner.vigia.replace(Some(h)) {
            viejo.abort();
        }
    }

    pub fn cerrar(&self) {
        if let Some(h) = self.inner.vigia.take() {
            h.abort();
        }
    }

    async fn releer(&self) {
        match leer().await {
            Ok(e) => {
                if *self.inner.estado.borrow() != e {
                    self.inner.estado.replace(e);
                    let oyentes = self.inner.oyentes.borrow().clone();
                    for f in oyentes {
                        f();
                    }
                }
            }
            Err(e) => eprintln!("morgan: red: {e}"),
        }
    }

    pub fn alternar_wifi(&self) {
        let Some(actual) = self.inner.estado.borrow().wifi else {
            return;
        };
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = wifi(!actual).await {
                eprintln!("morgan: red: Wi-Fi: {e}");
            }
            yo.releer().await;
        });
    }

    pub fn alternar(&self, ruta: &OwnedObjectPath) {
        let Some(c) = self
            .inner
            .estado
            .borrow()
            .conexiones
            .iter()
            .find(|c| &c.ruta == ruta)
            .cloned()
        else {
            return;
        };
        let wifi_apagada = self.inner.estado.borrow().wifi == Some(false);
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let r = match &c.activa {
                Some(a) => bajar(a).await,
                None => subir(&c, wifi_apagada).await,
            };
            if let Err(e) = r {
                eprintln!("morgan: red: {}: {e}", c.nombre);
            }
            yo.releer().await;
        });
    }
}

async fn nm() -> zbus::Result<zbus::Proxy<'static>> {
    sistema::proxy(NM, RUTA, NM).await
}

async fn leer() -> zbus::Result<Estado> {
    let nm = nm().await?;
    let dispositivos: Vec<OwnedObjectPath> = nm.call("GetDevices", &()).await?;
    let mut hay_wifi = false;
    for d in dispositivos {
        let p = sistema::proxy(NM, d, "org.freedesktop.NetworkManager.Device").await?;
        if p.get_property::<u32>("DeviceType").await? == DISPOSITIVO_WIFI {
            hay_wifi = true;
            break;
        }
    }
    let wifi = if hay_wifi {
        Some(nm.get_property::<bool>("WirelessEnabled").await?)
    } else {
        None
    };

    let mut activas = HashMap::new();
    for a in nm
        .get_property::<Vec<OwnedObjectPath>>("ActiveConnections")
        .await?
    {
        let p = sistema::proxy(
            NM,
            a.clone(),
            "org.freedesktop.NetworkManager.Connection.Active",
        )
        .await?;
        // Una activa puede desaparecer entre la lista y la lectura.
        if let Ok(c) = p.get_property::<OwnedObjectPath>("Connection").await {
            activas.insert(c, a);
        }
    }

    let ajustes = sistema::proxy(NM, AJUSTES, "org.freedesktop.NetworkManager.Settings").await?;
    let guardadas: Vec<OwnedObjectPath> = ajustes.call("ListConnections", &()).await?;
    let mut conexiones = Vec::new();
    for ruta in guardadas {
        let p = sistema::proxy(
            NM,
            ruta.clone(),
            "org.freedesktop.NetworkManager.Settings.Connection",
        )
        .await?;
        let s: HashMap<String, HashMap<String, OwnedValue>> = p.call("GetSettings", &()).await?;
        let Some(c) = s.get("connection") else {
            continue;
        };
        let texto = |k: &str| {
            c.get(k)
                .and_then(|v| <&str>::try_from(&**v).ok().map(str::to_owned))
        };
        let (Some(nombre), Some(tipo)) = (texto("id"), texto("type").as_deref().and_then(Tipo::de))
        else {
            continue;
        };
        conexiones.push(Conexion {
            activa: activas.get(&ruta).cloned(),
            ruta,
            nombre,
            tipo,
        });
    }
    ordenar(&mut conexiones);
    Ok(Estado { wifi, conexiones })
}

async fn wifi(encendida: bool) -> zbus::Result<()> {
    nm().await?
        .set_property("WirelessEnabled", encendida)
        .await
        .map_err(zbus::Error::from)
}

async fn bajar(activa: &OwnedObjectPath) -> zbus::Result<()> {
    nm().await?
        .call::<_, _, ()>("DeactivateConnection", &(activa,))
        .await
}

/// Una Wi-Fi con la radio apagada necesita prenderla y esperar a que la placa esté lista.
async fn subir(c: &Conexion, wifi_apagada: bool) -> zbus::Result<()> {
    if c.tipo == Tipo::Wifi && wifi_apagada {
        wifi(true).await?;
        for _ in 0..20 {
            if wifi_lista().await.unwrap_or(false) {
                break;
            }
            glib::timeout_future(Duration::from_millis(250)).await;
        }
    }
    let raiz = OwnedObjectPath::try_from("/").expect("ruta fija");
    nm().await?
        .call::<_, _, OwnedObjectPath>("ActivateConnection", &(&c.ruta, &raiz, &raiz))
        .await
        .map(|_| ())
}

async fn wifi_lista() -> zbus::Result<bool> {
    let dispositivos: Vec<OwnedObjectPath> = nm().await?.call("GetDevices", &()).await?;
    for d in dispositivos {
        let p = sistema::proxy(NM, d, "org.freedesktop.NetworkManager.Device").await?;
        if p.get_property::<u32>("DeviceType").await? == DISPOSITIVO_WIFI {
            return Ok(p.get_property::<u32>("State").await? > NO_DISPONIBLE);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(nombre: &str, tipo: Tipo) -> Conexion {
        Conexion {
            ruta: OwnedObjectPath::try_from("/x").unwrap(),
            nombre: nombre.into(),
            tipo,
            activa: None,
        }
    }

    #[test]
    fn orden_cable_wifi_vpn() {
        let mut v = vec![
            c("zeta", Tipo::Wifi),
            c("vpn", Tipo::Vpn),
            c("alfa", Tipo::Wifi),
            c("Conexión cableada 1", Tipo::Cable),
        ];
        ordenar(&mut v);
        let n: Vec<_> = v.iter().map(|c| c.nombre.as_str()).collect();
        assert_eq!(n, ["Conexión cableada 1", "alfa", "zeta", "vpn"]);
    }

    #[test]
    fn tipos() {
        assert_eq!(Tipo::de("802-3-ethernet"), Some(Tipo::Cable));
        assert_eq!(Tipo::de("802-11-wireless"), Some(Tipo::Wifi));
        assert_eq!(Tipo::de("wireguard"), Some(Tipo::Vpn));
        assert_eq!(Tipo::de("loopback"), None);
        assert_eq!(Tipo::de("bridge"), None);
    }
}
