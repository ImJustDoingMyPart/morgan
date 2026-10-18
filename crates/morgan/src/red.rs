//! Lo marcado sale de NetworkManager, así que dos conexiones excluyentes nunca quedan
//! marcadas a la vez.
//!
//! Además de las guardadas, las Wi-Fi a la vista que no lo están: cada SSID una vez, con la
//! antena más fuerte. Conectarse a una crea la conexión (`AddAndActivateConnection`); si no
//! llega a conectarse, se borra, para que una clave mal escrita no quede guardada.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use futures_lite::StreamExt;
use gtk::glib;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::sistema;

const NM: &str = "org.freedesktop.NetworkManager";
const RUTA: &str = "/org/freedesktop/NetworkManager";
const AJUSTES: &str = "/org/freedesktop/NetworkManager/Settings";
/// `NM_DEVICE_TYPE_WIFI` y `NM_DEVICE_STATE_UNAVAILABLE`.
const DISPOSITIVO_WIFI: u32 = 2;
const NO_DISPONIBLE: u32 = 20;
const DISPOSITIVOS: &str = "/org/freedesktop/NetworkManager/Devices";
/// `NM_802_11_AP_FLAGS_PRIVACY` y los `NM_802_11_AP_SEC_KEY_MGMT_*`.
const PRIVADA: u32 = 0x1;
const PSK: u32 = 0x100;
const EMPRESA: u32 = 0x200 | 0x2000;
const SAE: u32 = 0x400;
const OWE: u32 = 0x800;
/// `NM_ACTIVE_CONNECTION_STATE_*` y los motivos que dicen «la clave no sirvió».
const ACTIVADA: u32 = 2;
const DESACTIVADA: u32 = 4;
const SIN_CLAVE: u32 = 9;
const LOGIN_FALLIDO: u32 = 10;
/// `NM_DEVICE_STATE_FAILED` y `NM_DEVICE_STATE_REASON_NO_SECRETS`.
const FALLIDO: u32 = 120;
const DISPOSITIVO_SIN_CLAVE: u32 = 7;
/// Lo que puede tardar una conexión nueva: el saludo de WPA y el DHCP.
const ESPERA: Duration = Duration::from_secs(45);

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

/// Sin `Empresa` ni WEP: 802.1X pide más que una clave y WEP ya no se usa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seguridad {
    Abierta,
    /// Cifrada sin clave (Enhanced Open).
    Owe,
    Psk,
    Sae,
}

impl Seguridad {
    fn de(privada: bool, wpa: u32, rsn: u32) -> Option<Seguridad> {
        let s = wpa | rsn;
        if s & EMPRESA != 0 {
            None
        } else if s & PSK != 0 {
            Some(Seguridad::Psk)
        } else if s & SAE != 0 {
            Some(Seguridad::Sae)
        } else if s & OWE != 0 {
            Some(Seguridad::Owe)
        } else if privada {
            None
        } else {
            Some(Seguridad::Abierta)
        }
    }

    pub fn pide_clave(self) -> bool {
        matches!(self, Seguridad::Psk | Seguridad::Sae)
    }

    fn key_mgmt(self) -> Option<&'static str> {
        match self {
            Seguridad::Abierta => None,
            Seguridad::Owe => Some("owe"),
            Seguridad::Psk => Some("wpa-psk"),
            Seguridad::Sae => Some("sae"),
        }
    }
}

/// Una antena tal como la lista NetworkManager.
#[derive(Debug, Clone)]
pub struct Antena {
    pub ruta: OwnedObjectPath,
    pub ssid: Vec<u8>,
    pub fuerza: u8,
    pub seguridad: Option<Seguridad>,
}

/// Una Wi-Fi a la vista que no está guardada.
#[derive(Debug, Clone, PartialEq)]
pub struct Visible {
    pub ssid: Vec<u8>,
    pub nombre: String,
    /// De 1 a 4: con la fuerza exacta la lista se reordenaría a cada escaneo.
    pub barras: u8,
    pub seguridad: Seguridad,
    pub antena: OwnedObjectPath,
    pub dispositivo: OwnedObjectPath,
}

impl Visible {
    pub fn glifo(&self) -> &'static str {
        const ABIERTA: [&str; 4] = ["\u{F091F}", "\u{F0922}", "\u{F0925}", "\u{F0928}"];
        const CON_CLAVE: [&str; 4] = ["\u{F0921}", "\u{F0924}", "\u{F0927}", "\u{F092A}"];
        let i = usize::from(self.barras.clamp(1, 4) - 1);
        if self.seguridad.pide_clave() {
            CON_CLAVE[i]
        } else {
            ABIERTA[i]
        }
    }
}

pub fn barras(fuerza: u8) -> u8 {
    match fuerza {
        0..=24 => 1,
        25..=49 => 2,
        50..=74 => 3,
        _ => 4,
    }
}

/// Cada SSID una vez, con su antena más fuerte; sin las ocultas, las guardadas ni las que no
/// se pueden usar con una clave. Las de más señal primero.
pub fn visibles(
    antenas: Vec<Antena>,
    guardadas: &HashSet<Vec<u8>>,
    dispositivo: &OwnedObjectPath,
) -> Vec<Visible> {
    let mut mejor: HashMap<Vec<u8>, Antena> = HashMap::new();
    for a in antenas {
        if a.ssid.is_empty() || a.seguridad.is_none() || guardadas.contains(&a.ssid) {
            continue;
        }
        match mejor.get(&a.ssid) {
            Some(m) if m.fuerza >= a.fuerza => {}
            _ => {
                mejor.insert(a.ssid.clone(), a);
            }
        }
    }
    let mut v: Vec<Visible> = mejor
        .into_values()
        .filter_map(|a| {
            Some(Visible {
                nombre: String::from_utf8_lossy(&a.ssid).into_owned(),
                barras: barras(a.fuerza),
                seguridad: a.seguridad?,
                ssid: a.ssid,
                antena: a.ruta,
                dispositivo: dispositivo.clone(),
            })
        })
        .collect();
    v.sort_by(|a, b| {
        b.barras
            .cmp(&a.barras)
            .then_with(|| a.nombre.cmp(&b.nombre))
    });
    v
}

/// Cómo terminó un intento con una Wi-Fi nueva.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Falla {
    Clave,
    Otra,
}

/// Lo que acepta WPA como clave: de 8 a 63 caracteres ASCII, o 64 hexadecimales.
pub fn clave_valida(s: Seguridad, clave: &str) -> bool {
    match s {
        Seguridad::Psk => {
            let n = clave.len();
            (clave.is_ascii() && (8..=63).contains(&n))
                || (n == 64 && clave.chars().all(|c| c.is_ascii_hexdigit()))
        }
        Seguridad::Sae => !clave.is_empty(),
        Seguridad::Abierta | Seguridad::Owe => true,
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Estado {
    pub wifi: Option<bool>,
    pub conexiones: Vec<Conexion>,
    pub visibles: Vec<Visible>,
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
    vigias: RefCell<Vec<glib::JoinHandle<()>>>,
}

impl Red {
    pub fn estado(&self) -> Estado {
        self.inner.estado.borrow().clone()
    }

    pub fn al_cambiar(&self, f: impl Fn() + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    /// Mira NetworkManager y los dispositivos (la lista de antenas cambia en el de Wi-Fi), y
    /// pide un escaneo: lo que se ve al abrir es de ahora.
    pub fn abrir(&self) {
        self.cerrar();
        let reglas = [
            sistema::propiedades(NM, RUTA),
            sistema::propiedades_bajo(NM, DISPOSITIVOS),
        ];
        let mut vigias = self.inner.vigias.borrow_mut();
        for regla in reglas {
            let yo = self.clone();
            vigias.push(sistema::vigilar(
                regla,
                Box::new(move || {
                    let yo = yo.clone();
                    Box::pin(async move { yo.releer().await })
                }),
            ));
        }
        glib::MainContext::default().spawn_local(async {
            // Con un escaneo reciente NetworkManager lo rechaza: no importa.
            let _ = escanear().await;
        });
    }

    pub fn cerrar(&self) {
        for h in self.inner.vigias.take() {
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

impl Red {
    /// Borra la conexión guardada (y con ella la clave); si estaba activa, se cae.
    pub fn olvidar(&self, ruta: &OwnedObjectPath, listo: impl FnOnce() + 'static) {
        let ruta = ruta.clone();
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let r = async {
                sistema::proxy(NM, ruta, "org.freedesktop.NetworkManager.Settings.Connection")
                    .await?
                    .call::<_, _, ()>("Delete", &())
                    .await
            };
            if let Err(e) = r.await {
                eprintln!("morgan: red: olvidar: {e}");
            }
            yo.releer().await;
            listo();
        });
    }

    /// La clave guardada de una Wi-Fi. NetworkManager la da solo si polkit deja (en una
    /// sesión local activa, sí; si no, la pide).
    pub fn clave(&self, ruta: &OwnedObjectPath, listo: impl FnOnce(Option<String>) + 'static) {
        let ruta = ruta.clone();
        glib::MainContext::default().spawn_local(async move {
            let r = async {
                let s: HashMap<String, HashMap<String, OwnedValue>> = sistema::proxy(
                    NM,
                    ruta,
                    "org.freedesktop.NetworkManager.Settings.Connection",
                )
                .await?
                .call("GetSecrets", &("802-11-wireless-security",))
                .await?;
                Ok::<_, zbus::Error>(
                    s.get("802-11-wireless-security")
                        .and_then(|w| w.get("psk"))
                        .and_then(|v| <&str>::try_from(&**v).ok().map(str::to_owned)),
                )
            };
            listo(r.await.unwrap_or_else(|e| {
                eprintln!("morgan: red: clave: {e}");
                None
            }));
        });
    }

    /// Con la clave, si la pide. `listo` llega una vez, cuando conectó o cuando ya no va a
    /// conectar (y la conexión creada se borró).
    pub fn conectar_nueva(
        &self,
        v: &Visible,
        clave: Option<String>,
        listo: impl FnOnce(Result<(), Falla>) + 'static,
    ) {
        let v = v.clone();
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let r = nueva(&v, clave.as_deref()).await;
            if let Err(f) = &r {
                eprintln!("morgan: red: {}: no conectó ({f:?})", v.nombre);
            }
            yo.releer().await;
            listo(r);
        });
    }
}

async fn escanear() -> zbus::Result<()> {
    if let Some(d) = dispositivo_wifi().await? {
        let p = sistema::proxy(NM, d, "org.freedesktop.NetworkManager.Device.Wireless").await?;
        p.call::<_, _, ()>("RequestScan", &(HashMap::<&str, Value>::new(),))
            .await?;
    }
    Ok(())
}

async fn dispositivo_wifi() -> zbus::Result<Option<OwnedObjectPath>> {
    let dispositivos: Vec<OwnedObjectPath> = nm().await?.call("GetDevices", &()).await?;
    for d in dispositivos {
        let p = sistema::proxy(NM, d.clone(), "org.freedesktop.NetworkManager.Device").await?;
        if p.get_property::<u32>("DeviceType").await? == DISPOSITIVO_WIFI {
            return Ok(Some(d));
        }
    }
    Ok(None)
}

async fn antenas(d: &OwnedObjectPath) -> zbus::Result<Vec<Antena>> {
    let p = sistema::proxy(
        NM,
        d.clone(),
        "org.freedesktop.NetworkManager.Device.Wireless",
    )
    .await?;
    let rutas: Vec<OwnedObjectPath> = p.call("GetAllAccessPoints", &()).await?;
    let mut v = Vec::new();
    for ruta in rutas {
        let a = sistema::proxy(
            NM,
            ruta.clone(),
            "org.freedesktop.NetworkManager.AccessPoint",
        )
        .await?;
        // Una antena puede desaparecer entre la lista y la lectura.
        let leida = async {
            Ok::<_, zbus::Error>(Antena {
                ssid: a.get_property("Ssid").await?,
                fuerza: a.get_property("Strength").await?,
                seguridad: Seguridad::de(
                    a.get_property::<u32>("Flags").await? & PRIVADA != 0,
                    a.get_property("WpaFlags").await?,
                    a.get_property("RsnFlags").await?,
                ),
                ruta,
            })
        };
        if let Ok(antena) = leida.await {
            v.push(antena);
        }
    }
    Ok(v)
}

async fn nueva(v: &Visible, clave: Option<&str>) -> Result<(), Falla> {
    let mut ajustes: HashMap<&str, HashMap<&str, Value>> = HashMap::new();
    ajustes.insert(
        "connection",
        HashMap::from([("type", Value::from("802-11-wireless"))]),
    );
    ajustes.insert(
        "802-11-wireless",
        HashMap::from([("ssid", Value::from(v.ssid.clone()))]),
    );
    if let Some(km) = v.seguridad.key_mgmt() {
        let mut s = HashMap::from([("key-mgmt", Value::from(km))]);
        if let Some(c) = clave {
            s.insert("psk", Value::from(c.to_owned()));
        }
        ajustes.insert("802-11-wireless-security", s);
    }
    let nm = nm().await.map_err(|_| Falla::Otra)?;
    let (guardada, activa): (OwnedObjectPath, OwnedObjectPath) = nm
        .call(
            "AddAndActivateConnection",
            &(ajustes, &v.dispositivo, &v.antena),
        )
        .await
        .map_err(|e| {
            eprintln!("morgan: red: {}: {e}", v.nombre);
            Falla::Otra
        })?;
    let r = esperar(&activa, &v.dispositivo).await;
    if r.is_err()
        && let Ok(p) = sistema::proxy(
            NM,
            guardada,
            "org.freedesktop.NetworkManager.Settings.Connection",
        )
        .await
    {
        let _ = p.call::<_, _, ()>("Delete", &()).await;
    }
    r
}

/// Hasta que la activa conecta, se cae o se acaba la espera. Una clave mala la dice el
/// dispositivo (falla con `NO_SECRETS`, sin agente que pida otra), no la activa, que solo
/// dice que el dispositivo se desconectó.
async fn esperar(activa: &OwnedObjectPath, dispositivo: &OwnedObjectPath) -> Result<(), Falla> {
    let a = sistema::proxy(
        NM,
        activa.clone(),
        "org.freedesktop.NetworkManager.Connection.Active",
    )
    .await
    .map_err(|_| Falla::Otra)?;
    let d = sistema::proxy(
        NM,
        dispositivo.clone(),
        "org.freedesktop.NetworkManager.Device",
    )
    .await
    .map_err(|_| Falla::Otra)?;
    // Suscripción antes de leer: un cambio entre las dos queda en la fila.
    let de_activa = a
        .receive_signal("StateChanged")
        .await
        .map_err(|_| Falla::Otra)?
        .map(|m| {
            m.body()
                .deserialize::<(u32, u32)>()
                .map(|(estado, motivo)| Cambio::Activa(estado, motivo))
        });
    let de_dispositivo = d
        .receive_signal("StateChanged")
        .await
        .map_err(|_| Falla::Otra)?
        .map(|m| {
            m.body()
                .deserialize::<(u32, u32, u32)>()
                .map(|(nuevo, _, motivo)| Cambio::Dispositivo(nuevo, motivo))
        });
    match a.get_property::<u32>("State").await {
        Ok(ACTIVADA) => return Ok(()),
        Ok(DESACTIVADA) | Err(_) => return Err(Falla::Otra),
        Ok(_) => {}
    }
    let mut cambios = de_activa.or(de_dispositivo);
    let senales = async {
        while let Some(c) = cambios.next().await {
            match c {
                Ok(Cambio::Activa(ACTIVADA, _)) => return Ok(()),
                Ok(Cambio::Activa(DESACTIVADA, SIN_CLAVE | LOGIN_FALLIDO))
                | Ok(Cambio::Dispositivo(FALLIDO, DISPOSITIVO_SIN_CLAVE)) => {
                    return Err(Falla::Clave);
                }
                Ok(Cambio::Activa(DESACTIVADA, _)) | Ok(Cambio::Dispositivo(FALLIDO, _)) => {
                    return Err(Falla::Otra);
                }
                _ => {}
            }
        }
        Err(Falla::Otra)
    };
    futures_lite::future::or(senales, async {
        glib::timeout_future(ESPERA).await;
        Err(Falla::Otra)
    })
    .await
}

enum Cambio {
    Activa(u32, u32),
    /// El estado nuevo y el motivo.
    Dispositivo(u32, u32),
}

async fn nm() -> zbus::Result<zbus::Proxy<'static>> {
    sistema::proxy(NM, RUTA, NM).await
}

async fn leer() -> zbus::Result<Estado> {
    let nm = nm().await?;
    let dispositivo = dispositivo_wifi().await?;
    let wifi = match dispositivo {
        Some(_) => Some(nm.get_property::<bool>("WirelessEnabled").await?),
        None => None,
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
    let mut ssids = HashSet::new();
    for ruta in guardadas {
        let p = sistema::proxy(
            NM,
            ruta.clone(),
            "org.freedesktop.NetworkManager.Settings.Connection",
        )
        .await?;
        // Una conexión puede borrarse entre la lista y la lectura (la de un intento fallido).
        let Ok(s) = p
            .call::<_, _, HashMap<String, HashMap<String, OwnedValue>>>("GetSettings", &())
            .await
        else {
            continue;
        };
        if let Some(ssid) = s
            .get("802-11-wireless")
            .and_then(|w| w.get("ssid"))
            .and_then(|v| Vec::<u8>::try_from(v.try_clone().ok()?).ok())
        {
            ssids.insert(ssid);
        }
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
    let visibles = match &dispositivo {
        Some(d) if wifi == Some(true) => visibles(antenas(d).await?, &ssids, d),
        _ => Vec::new(),
    };
    Ok(Estado {
        wifi,
        conexiones,
        visibles,
    })
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
    let Some(d) = dispositivo_wifi().await? else {
        return Ok(false);
    };
    let p = sistema::proxy(NM, d, "org.freedesktop.NetworkManager.Device").await?;
    Ok(p.get_property::<u32>("State").await? > NO_DISPONIBLE)
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

    fn a(ssid: &str, fuerza: u8, seguridad: Option<Seguridad>) -> Antena {
        Antena {
            ruta: OwnedObjectPath::try_from(format!("/a/{fuerza}")).unwrap(),
            ssid: ssid.as_bytes().to_vec(),
            fuerza,
            seguridad,
        }
    }

    #[test]
    fn visibles_una_por_ssid_sin_guardadas_ni_ocultas() {
        let d = OwnedObjectPath::try_from("/d").unwrap();
        let guardadas = HashSet::from([b"casa".to_vec()]);
        let v = visibles(
            vec![
                a("bar", 30, Some(Seguridad::Psk)),
                a("bar", 80, Some(Seguridad::Psk)),
                a("casa", 90, Some(Seguridad::Psk)),
                a("", 90, Some(Seguridad::Abierta)),
                a("eduroam", 90, None),
                a("abierta", 10, Some(Seguridad::Abierta)),
                a("aaa", 76, Some(Seguridad::Sae)),
            ],
            &guardadas,
            &d,
        );
        let n: Vec<_> = v.iter().map(|v| (v.nombre.as_str(), v.barras)).collect();
        assert_eq!(n, [("aaa", 4), ("bar", 4), ("abierta", 1)]);
        assert_eq!(v[1].antena.as_str(), "/a/80");
    }

    #[test]
    fn seguridad_de_las_banderas() {
        assert_eq!(Seguridad::de(false, 0, 0), Some(Seguridad::Abierta));
        // WEP: privada sin WPA.
        assert_eq!(Seguridad::de(true, 0, 0), None);
        assert_eq!(Seguridad::de(true, 0, PSK), Some(Seguridad::Psk));
        // WPA3 en transición: con la clave de WPA2 alcanza.
        assert_eq!(Seguridad::de(true, 0, PSK | SAE), Some(Seguridad::Psk));
        assert_eq!(Seguridad::de(true, 0, SAE), Some(Seguridad::Sae));
        assert_eq!(Seguridad::de(true, 0, 0x200), None);
        assert_eq!(Seguridad::de(false, 0, OWE), Some(Seguridad::Owe));
    }

    #[test]
    fn claves() {
        assert!(!clave_valida(Seguridad::Psk, "corta"));
        assert!(clave_valida(Seguridad::Psk, "ocho1234"));
        assert!(!clave_valida(Seguridad::Psk, &"x".repeat(64)));
        assert!(clave_valida(Seguridad::Psk, &"ab".repeat(32)));
        assert!(!clave_valida(Seguridad::Psk, "contraseñaa"));
        assert!(clave_valida(Seguridad::Sae, "x"));
        assert!(!clave_valida(Seguridad::Sae, ""));
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
