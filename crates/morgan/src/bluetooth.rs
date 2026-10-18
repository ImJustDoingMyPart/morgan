//! Leer no arranca `bluetoothd`: si `org.bluez` no tiene dueño, el bluetooth está apagado.
//! Prenderlo sí lo arranca (por activación de D-Bus) y espera al adaptador.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::sistema;

const BLUEZ: &str = "org.bluez";
const ADAPTADOR: &str = "org.bluez.Adapter1";
const DISPOSITIVO: &str = "org.bluez.Device1";

type Objetos = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;

#[derive(Debug, Clone, PartialEq)]
pub struct Dispositivo {
    pub ruta: OwnedObjectPath,
    pub nombre: String,
    pub icono: String,
    pub conectado: bool,
}

impl Dispositivo {
    pub fn glifo(&self) -> &'static str {
        crate::dispositivos::tipo_bluetooth(&self.icono).glifo()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Estado {
    pub adaptador: Option<OwnedObjectPath>,
    pub encendido: bool,
    pub dispositivos: Vec<Dispositivo>,
}

pub fn interpretar(objetos: &Objetos) -> Estado {
    let mut rutas: Vec<_> = objetos.keys().collect();
    rutas.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    let booleano = |props: &HashMap<String, OwnedValue>, k: &str| {
        props
            .get(k)
            .and_then(|v| bool::try_from(&**v).ok())
            .unwrap_or(false)
    };
    let texto = |props: &HashMap<String, OwnedValue>, k: &str| {
        props
            .get(k)
            .and_then(|v| <&str>::try_from(&**v).ok())
            .map(str::to_owned)
    };
    let adaptador = rutas
        .iter()
        .find(|r| objetos[**r].contains_key(ADAPTADOR))
        .map(|r| (*r).clone());
    let encendido = adaptador
        .as_ref()
        .is_some_and(|a| booleano(&objetos[a][ADAPTADOR], "Powered"));
    let mut dispositivos: Vec<Dispositivo> = rutas
        .iter()
        .filter_map(|r| {
            let d = objetos[*r].get(DISPOSITIVO)?;
            booleano(d, "Paired").then(|| Dispositivo {
                ruta: (*r).clone(),
                nombre: texto(d, "Alias")
                    .or_else(|| texto(d, "Name"))
                    .unwrap_or_else(|| r.to_string()),
                icono: texto(d, "Icon").unwrap_or_default(),
                conectado: booleano(d, "Connected"),
            })
        })
        .collect();
    dispositivos.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    Estado {
        adaptador,
        encendido,
        dispositivos,
    }
}

#[derive(Clone, Default)]
pub struct Bluetooth {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    estado: RefCell<Estado>,
    oyentes: RefCell<Vec<Rc<dyn Fn()>>>,
    vigia: RefCell<Option<glib::JoinHandle<()>>>,
}

impl Bluetooth {
    pub fn estado(&self) -> Estado {
        self.inner.estado.borrow().clone()
    }

    pub fn al_cambiar(&self, f: impl Fn() + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    pub fn abrir(&self) {
        let yo = self.clone();
        let h = sistema::vigilar(
            sistema::de(BLUEZ),
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
        let e = match leer(false).await {
            Ok(e) => e,
            Err(e) => {
                eprintln!("morgan: bluetooth: {e}");
                Estado::default()
            }
        };
        if *self.inner.estado.borrow() != e {
            self.inner.estado.replace(e);
            let oyentes = self.inner.oyentes.borrow().clone();
            for f in oyentes {
                f();
            }
        }
    }

    pub fn alternar(&self) {
        let encendido = self.inner.estado.borrow().encendido;
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = encender(!encendido).await {
                eprintln!("morgan: bluetooth: {e}");
            }
            yo.releer().await;
        });
    }

    pub fn alternar_dispositivo(&self, ruta: &OwnedObjectPath) {
        let Some(d) = self
            .inner
            .estado
            .borrow()
            .dispositivos
            .iter()
            .find(|d| &d.ruta == ruta)
            .cloned()
        else {
            return;
        };
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let metodo = if d.conectado { "Disconnect" } else { "Connect" };
            let r = match sistema::proxy(BLUEZ, d.ruta.clone(), DISPOSITIVO).await {
                Ok(p) => p.call::<_, _, ()>(metodo, &()).await,
                Err(e) => Err(e),
            };
            if let Err(e) = r {
                eprintln!("morgan: bluetooth: {}: {e}", d.nombre);
            }
            yo.releer().await;
        });
    }
}

async fn leer(activar: bool) -> zbus::Result<Estado> {
    let conn = sistema::bus().await?;
    if !activar {
        let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
        let nombre = zbus::names::BusName::try_from(BLUEZ).expect("nombre fijo");
        if !dbus.name_has_owner(nombre).await? {
            return Ok(Estado::default());
        }
    }
    let om = sistema::proxy(BLUEZ, "/", "org.freedesktop.DBus.ObjectManager").await?;
    let objetos: Objetos = om.call("GetManagedObjects", &()).await?;
    Ok(interpretar(&objetos))
}

async fn encender(v: bool) -> zbus::Result<()> {
    let mut adaptador = leer(v).await?.adaptador;
    // Recién arrancado, `bluetoothd` tarda en publicar el adaptador.
    for _ in 0..20 {
        if adaptador.is_some() || !v {
            break;
        }
        glib::timeout_future(Duration::from_millis(250)).await;
        adaptador = leer(true).await?.adaptador;
    }
    let Some(a) = adaptador else {
        return Err(zbus::Error::Failure("no hay adaptador".into()));
    };
    sistema::proxy(BLUEZ, a, ADAPTADOR)
        .await?
        .set_property("Powered", v)
        .await
        .map_err(zbus::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    fn props(pares: &[(&str, Value<'static>)]) -> HashMap<String, OwnedValue> {
        pares
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.try_clone().unwrap().try_into().unwrap()))
            .collect()
    }

    fn ruta(s: &str) -> OwnedObjectPath {
        OwnedObjectPath::try_from(s).unwrap()
    }

    #[test]
    fn adaptador_y_emparejados() {
        let mut o: Objetos = HashMap::new();
        o.insert(
            ruta("/org/bluez/hci0"),
            HashMap::from([(
                ADAPTADOR.to_owned(),
                props(&[("Powered", Value::from(true))]),
            )]),
        );
        o.insert(
            ruta("/org/bluez/hci0/dev_B"),
            HashMap::from([(
                DISPOSITIVO.to_owned(),
                props(&[
                    ("Alias", Value::from("Auriculares")),
                    ("Paired", Value::from(true)),
                    ("Connected", Value::from(true)),
                    ("Icon", Value::from("audio-headset")),
                ]),
            )]),
        );
        o.insert(
            ruta("/org/bluez/hci0/dev_C"),
            HashMap::from([(
                DISPOSITIVO.to_owned(),
                props(&[
                    ("Alias", Value::from("Ajeno")),
                    ("Paired", Value::from(false)),
                ]),
            )]),
        );
        let e = interpretar(&o);
        assert_eq!(e.adaptador, Some(ruta("/org/bluez/hci0")));
        assert!(e.encendido);
        assert_eq!(e.dispositivos.len(), 1);
        assert_eq!(e.dispositivos[0].nombre, "Auriculares");
        assert!(e.dispositivos[0].conectado);
        assert_eq!(e.dispositivos[0].glifo(), "󰋋");
    }

    #[test]
    fn sin_bluez_es_apagado() {
        let e = interpretar(&HashMap::new());
        assert_eq!(e, Estado::default());
        assert!(!e.encendido);
    }
}
