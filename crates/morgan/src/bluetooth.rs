//! Leer no arranca `bluetoothd`: si `org.bluez` no tiene dueño, el bluetooth está apagado.
//! Prenderlo sí lo arranca (por activación de D-Bus) y espera al adaptador.
//!
//! Mientras el centro está abierto con el bluetooth prendido se buscan dispositivos
//! (`StartDiscovery`, que BlueZ ata a esta conexión). Para emparejar, morgan registra su
//! propio agente (`org.bluez.Agent1`) solo mientras dura el `Pair`: lo que el agente pide
//! (confirmar un código, mostrarlo, escribirlo) llega al centro como un `Pedido`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};

use crate::sistema;

const BLUEZ: &str = "org.bluez";
const ADAPTADOR: &str = "org.bluez.Adapter1";
const DISPOSITIVO: &str = "org.bluez.Device1";
const AGENTE: &str = "/org/morgan/agente";

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
    /// Sin emparejar, con nombre y vistos en esta búsqueda (con `RSSI`): los que BlueZ
    /// recuerda de antes y ya no están, no.
    pub cercanos: Vec<Dispositivo>,
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
    let mut dispositivos = Vec::new();
    let mut cercanos = Vec::new();
    for r in &rutas {
        let Some(d) = objetos[*r].get(DISPOSITIVO) else {
            continue;
        };
        let emparejado = booleano(d, "Paired");
        // Sin `Name`, el alias es la dirección: nadie sabe qué es.
        if !emparejado && (!d.contains_key("Name") || !d.contains_key("RSSI")) {
            continue;
        }
        let disp = Dispositivo {
            ruta: (*r).clone(),
            nombre: texto(d, "Alias")
                .or_else(|| texto(d, "Name"))
                .unwrap_or_else(|| r.to_string()),
            icono: texto(d, "Icon").unwrap_or_default(),
            conectado: booleano(d, "Connected"),
        };
        if emparejado {
            dispositivos.push(disp);
        } else {
            cercanos.push(disp);
        }
    }
    dispositivos.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    cercanos.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    Estado {
        adaptador,
        encendido,
        dispositivos,
        cercanos,
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
    abierto: Cell<bool>,
    /// El adaptador en el que se pidió `StartDiscovery`, para pararlo al cerrar.
    buscando: RefCell<Option<OwnedObjectPath>>,
    al_pedido: RefCell<Option<AlPedido>>,
    agente: Cell<bool>,
}

type AlPedido = Rc<dyn Fn(Pedido)>;

/// Lo que pide el agente de BlueZ durante un emparejamiento.
pub enum Pedido {
    /// ¿Es el mismo código que muestra el dispositivo?
    Confirmar {
        dispositivo: OwnedObjectPath,
        codigo: String,
        respuesta: async_channel::Sender<bool>,
    },
    /// Un código para escribir en el dispositivo (un teclado).
    Mostrar {
        dispositivo: OwnedObjectPath,
        codigo: String,
    },
    /// Un código para escribir acá: el PIN de un dispositivo viejo o el que muestra el otro.
    Escribir {
        dispositivo: OwnedObjectPath,
        numerico: bool,
        respuesta: async_channel::Sender<Option<String>>,
    },
    /// BlueZ dejó de esperar (el dispositivo canceló o se acabó el tiempo).
    Cancelado,
}

/// Cómo terminó un intento de emparejar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Falla {
    /// Lo canceló quien emparejaba: no hay nada que avisar.
    Cancelado,
    Otra,
}

pub fn codigo(passkey: u32) -> String {
    format!("{passkey:06}")
}

impl Bluetooth {
    pub fn estado(&self) -> Estado {
        self.inner.estado.borrow().clone()
    }

    pub fn al_cambiar(&self, f: impl Fn() + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    pub fn al_pedido(&self, f: impl Fn(Pedido) + 'static) {
        self.inner.al_pedido.replace(Some(Rc::new(f)));
    }

    pub fn abrir(&self) {
        self.inner.abierto.set(true);
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
        self.inner.abierto.set(false);
        if let Some(h) = self.inner.vigia.take() {
            h.abort();
        }
        if let Some(a) = self.inner.buscando.take() {
            glib::MainContext::default().spawn_local(async move {
                let _ = buscar(&a, false).await;
            });
        }
    }

    /// Busca mientras el centro está abierto con el bluetooth prendido.
    fn seguir_buscando(&self, e: &Estado) {
        let quiere = self.inner.abierto.get() && e.encendido;
        let actual = self.inner.buscando.borrow().clone();
        match (quiere, e.adaptador.clone(), actual) {
            (true, Some(a), None) => {
                self.inner.buscando.replace(Some(a.clone()));
                glib::MainContext::default().spawn_local(async move {
                    if let Err(e) = buscar(&a, true).await {
                        eprintln!("morgan: bluetooth: buscar: {e}");
                    }
                });
            }
            // Se apagó: BlueZ ya paró la búsqueda.
            (false, _, Some(_)) => {
                self.inner.buscando.replace(None);
            }
            _ => {}
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
        self.seguir_buscando(&e);
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

impl Bluetooth {
    /// Empareja, confía y conecta. `listo` llega una vez, al final.
    pub fn emparejar(
        &self,
        ruta: &OwnedObjectPath,
        listo: impl FnOnce(Result<(), Falla>) + 'static,
    ) {
        let ruta = ruta.clone();
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let r = yo.emparejar_(&ruta).await;
            yo.releer().await;
            listo(r);
        });
    }

    async fn emparejar_(&self, ruta: &OwnedObjectPath) -> Result<(), Falla> {
        let otra = |e: zbus::Error| {
            eprintln!("morgan: bluetooth: emparejar: {e}");
            Falla::Otra
        };
        self.servir_agente().await.map_err(otra)?;
        let gestor = sistema::proxy(BLUEZ, "/org/bluez", "org.bluez.AgentManager1")
            .await
            .map_err(otra)?;
        let agente = ObjectPath::from_static_str_unchecked(AGENTE);
        gestor
            .call::<_, _, ()>("RegisterAgent", &(&agente, "KeyboardDisplay"))
            .await
            .map_err(otra)?;
        let d = sistema::proxy(BLUEZ, ruta.clone(), DISPOSITIVO)
            .await
            .map_err(otra)?;
        let r = match d.call::<_, _, ()>("Pair", &()).await {
            Ok(()) => Ok(()),
            Err(zbus::Error::MethodError(nombre, ..))
                if nombre.as_str() == "org.bluez.Error.AuthenticationCanceled" =>
            {
                Err(Falla::Cancelado)
            }
            Err(e) => Err(otra(e)),
        };
        let _ = gestor.call::<_, _, ()>("UnregisterAgent", &(&agente,)).await;
        r?;
        // Confiado, se vuelve a conectar solo; si no conecta ahora, queda emparejado igual.
        if let Err(e) = d.set_property("Trusted", true).await {
            eprintln!("morgan: bluetooth: Trusted: {e}");
        }
        if let Err(e) = d.call::<_, _, ()>("Connect", &()).await {
            eprintln!("morgan: bluetooth: conectar: {e}");
        }
        Ok(())
    }

    pub fn cancelar_emparejar(&self, ruta: &OwnedObjectPath) {
        let ruta = ruta.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Ok(d) = sistema::proxy(BLUEZ, ruta, DISPOSITIVO).await {
                let _ = d.call::<_, _, ()>("CancelPairing", &()).await;
            }
        });
    }

    /// Lo saca de los emparejados (y lo desconecta).
    pub fn olvidar(&self, ruta: &OwnedObjectPath, listo: impl FnOnce() + 'static) {
        let ruta = ruta.clone();
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let adaptador = yo.inner.estado.borrow().adaptador.clone();
            let r = async {
                let a = adaptador.ok_or_else(|| zbus::Error::Failure("no hay adaptador".into()))?;
                sistema::proxy(BLUEZ, a, ADAPTADOR)
                    .await?
                    .call::<_, _, ()>("RemoveDevice", &(&ruta,))
                    .await
            };
            if let Err(e) = r.await {
                eprintln!("morgan: bluetooth: olvidar: {e}");
            }
            yo.releer().await;
            listo();
        });
    }

    /// El objeto del agente se publica una vez; los pedidos pasan al hilo de GTK por un canal.
    async fn servir_agente(&self) -> zbus::Result<()> {
        if self.inner.agente.get() {
            return Ok(());
        }
        let (tx, rx) = async_channel::unbounded();
        sistema::bus()
            .await?
            .object_server()
            .at(AGENTE, Agente { pedidos: tx })
            .await?;
        self.inner.agente.set(true);
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            while let Ok(p) = rx.recv().await {
                let f = yo.inner.al_pedido.borrow().clone();
                match f {
                    Some(f) => f(p),
                    None => rechazar(p),
                }
            }
        });
        Ok(())
    }
}

/// Sin nadie que conteste, se rechaza.
pub fn rechazar(p: Pedido) {
    match p {
        Pedido::Confirmar { respuesta, .. } => {
            let _ = respuesta.try_send(false);
        }
        Pedido::Escribir { respuesta, .. } => {
            let _ = respuesta.try_send(None);
        }
        Pedido::Mostrar { .. } | Pedido::Cancelado => {}
    }
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum ErrorAgente {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected,
    Canceled,
}

struct Agente {
    pedidos: async_channel::Sender<Pedido>,
}

impl Agente {
    fn mandar(&self, p: Pedido) -> Result<(), ErrorAgente> {
        self.pedidos.try_send(p).map_err(|_| ErrorAgente::Canceled)
    }

    async fn escribir(&self, d: ObjectPath<'_>, numerico: bool) -> Result<String, ErrorAgente> {
        let (tx, rx) = async_channel::bounded(1);
        self.mandar(Pedido::Escribir {
            dispositivo: d.into(),
            numerico,
            respuesta: tx,
        })?;
        rx.recv()
            .await
            .ok()
            .flatten()
            .ok_or(ErrorAgente::Canceled)
    }
}

/// Solo está registrado mientras morgan empareja, así que lo que no necesita respuesta del
/// usuario (autorizar un servicio de lo que se está emparejando) se acepta.
#[zbus::interface(name = "org.bluez.Agent1")]
impl Agente {
    fn release(&self) {}

    async fn request_pin_code(&self, device: ObjectPath<'_>) -> Result<String, ErrorAgente> {
        self.escribir(device, false).await
    }

    fn display_pin_code(&self, device: ObjectPath<'_>, pincode: &str) -> Result<(), ErrorAgente> {
        self.mandar(Pedido::Mostrar {
            dispositivo: device.into(),
            codigo: pincode.to_owned(),
        })
    }

    async fn request_passkey(&self, device: ObjectPath<'_>) -> Result<u32, ErrorAgente> {
        let s = self.escribir(device, true).await?;
        s.trim().parse().map_err(|_| ErrorAgente::Rejected)
    }

    fn display_passkey(
        &self,
        device: ObjectPath<'_>,
        passkey: u32,
        _entered: u16,
    ) -> Result<(), ErrorAgente> {
        self.mandar(Pedido::Mostrar {
            dispositivo: device.into(),
            codigo: codigo(passkey),
        })
    }

    async fn request_confirmation(
        &self,
        device: ObjectPath<'_>,
        passkey: u32,
    ) -> Result<(), ErrorAgente> {
        let (tx, rx) = async_channel::bounded(1);
        self.mandar(Pedido::Confirmar {
            dispositivo: device.into(),
            codigo: codigo(passkey),
            respuesta: tx,
        })?;
        match rx.recv().await {
            Ok(true) => Ok(()),
            _ => Err(ErrorAgente::Rejected),
        }
    }

    fn request_authorization(&self, _device: ObjectPath<'_>) {}

    fn authorize_service(&self, _device: ObjectPath<'_>, _uuid: &str) {}

    fn cancel(&self) {
        let _ = self.pedidos.try_send(Pedido::Cancelado);
    }
}

async fn buscar(adaptador: &OwnedObjectPath, si: bool) -> zbus::Result<()> {
    let metodo = if si { "StartDiscovery" } else { "StopDiscovery" };
    sistema::proxy(BLUEZ, adaptador.clone(), ADAPTADOR)
        .await?
        .call::<_, _, ()>(metodo, &())
        .await
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
    fn cercanos_con_nombre_y_vistos_ahora() {
        let mut o: Objetos = HashMap::new();
        let dispositivo = |o: &mut Objetos, r: &str, p: &[(&str, Value<'static>)]| {
            o.insert(
                ruta(r),
                HashMap::from([(DISPOSITIVO.to_owned(), props(p))]),
            );
        };
        dispositivo(
            &mut o,
            "/org/bluez/hci0/dev_A",
            &[
                ("Name", Value::from("Teclado")),
                ("Alias", Value::from("Teclado")),
                ("Paired", Value::from(false)),
                ("RSSI", Value::from(-60i16)),
            ],
        );
        // Sin nombre: el alias sería la dirección.
        dispositivo(
            &mut o,
            "/org/bluez/hci0/dev_B",
            &[
                ("Alias", Value::from("AA-BB-CC-DD-EE-FF")),
                ("Paired", Value::from(false)),
                ("RSSI", Value::from(-50i16)),
            ],
        );
        // Recordado de antes, no está ahora.
        dispositivo(
            &mut o,
            "/org/bluez/hci0/dev_C",
            &[
                ("Name", Value::from("Viejo")),
                ("Paired", Value::from(false)),
            ],
        );
        let e = interpretar(&o);
        assert!(e.dispositivos.is_empty());
        let n: Vec<_> = e.cercanos.iter().map(|d| d.nombre.as_str()).collect();
        assert_eq!(n, ["Teclado"]);
    }

    #[test]
    fn codigo_con_ceros() {
        assert_eq!(codigo(42), "000042");
        assert_eq!(codigo(123456), "123456");
    }

    #[test]
    fn sin_bluez_es_apagado() {
        let e = interpretar(&HashMap::new());
        assert_eq!(e, Estado::default());
        assert!(!e.encendido);
    }
}
