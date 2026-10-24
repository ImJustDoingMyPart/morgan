//! El watcher (`org.kde.StatusNotifierWatcher`) se toma solo si está libre: con otra barra
//! corriendo, el watcher es el suyo y morgan solo lo lee. Cada ítem se relee entero después
//! de cada señal suya: la señal avisa, el dato sale de `GetAll`.
//!
//! Todo lo del host corre en el hilo de GTK; la interfaz del watcher, en el ejecutor de zbus.

pub mod menu;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use futures_lite::StreamExt;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const RUTA_WATCHER: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const DBUSMENU: &str = "com.canonical.dbusmenu";
/// Un cambio de ícono suele llegar con `NewIcon` y `NewToolTip` juntos.
const RAFAGA: Duration = Duration::from_millis(30);

type Tooltip = (String, Vec<(i32, i32, Vec<u8>)>, String, String);

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Item {
    pub clave: String,
    pub id: String,
    pub titulo: String,
    pub tooltip: String,
    pub pasivo: bool,
    pub atencion: bool,
    pub icono: String,
    pub icono_atencion: String,
    pub ruta_tema: String,
    pub pixmap: Option<(i32, i32, Vec<u8>)>,
    pub menu: Option<String>,
    pub es_menu: bool,
}

#[derive(Debug, Clone)]
enum Registro {
    Alta(String),
    Baja(String),
}

struct Watcher {
    items: Vec<String>,
    host: async_channel::Sender<Registro>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &mut self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] e: SignalEmitter<'_>,
    ) {
        let emisor = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let clave = clave_item(service, &emisor);
        if self.items.contains(&clave) {
            return;
        }
        self.items.push(clave.clone());
        let _ = self.host.send(Registro::Alta(clave.clone())).await;
        let _ = Self::status_notifier_item_registered(&e, &clave).await;
        let _ = self.registered_status_notifier_items_changed(&e).await;
    }

    async fn register_status_notifier_host(
        &self,
        _service: &str,
        #[zbus(signal_emitter)] e: SignalEmitter<'_>,
    ) {
        let _ = Self::status_notifier_host_registered(&e).await;
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.clone()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(e: &SignalEmitter<'_>, item: &str)
    -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        e: &SignalEmitter<'_>,
        item: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(e: &SignalEmitter<'_>) -> zbus::Result<()>;
}

impl Watcher {
    fn sin_dueno(&mut self, nombre: &str) -> Vec<String> {
        let (fuera, quedan) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|c| bus_de(c) == nombre);
        self.items = quedan;
        fuera
    }
}

/// Una ruta (libappindicator, Electron: el bus es el del emisor) o un nombre de bus (la ruta
/// es la de la spec).
pub fn clave_item(servicio: &str, emisor: &str) -> String {
    if servicio.starts_with('/') {
        format!("{emisor}{servicio}")
    } else if servicio.contains('/') {
        servicio.to_owned()
    } else {
        format!("{servicio}/StatusNotifierItem")
    }
}

pub fn bus_de(clave: &str) -> &str {
    clave.split_once('/').map_or(clave, |(b, _)| b)
}

pub fn ruta_de(clave: &str) -> String {
    clave.split_once('/').map_or_else(
        || "/StatusNotifierItem".to_owned(),
        |(_, r)| format!("/{r}"),
    )
}

/// Las apps publican subconjuntos distintos (las de libappindicator no tienen `ToolTip` ni
/// `ItemIsMenu`): lo que falta queda vacío.
pub fn item_de(clave: &str, p: &HashMap<String, OwnedValue>) -> Item {
    let texto = |k: &str| {
        p.get(k)
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default()
    };
    let estado = texto("Status");
    let tooltip = p
        .get("ToolTip")
        .and_then(|v| Tooltip::try_from(v.try_clone().ok()?).ok())
        .map(
            |(_, _, titulo, texto)| match (titulo.is_empty(), texto.is_empty()) {
                (false, false) => format!("{titulo}\n{texto}"),
                (false, true) => titulo,
                _ => texto,
            },
        )
        .unwrap_or_default();
    let pixmap = p
        .get("IconPixmap")
        .and_then(|v| Vec::<(i32, i32, Vec<u8>)>::try_from(v.try_clone().ok()?).ok())
        .and_then(|v| v.into_iter().max_by_key(|(w, h, _)| w * h))
        .and_then(|(w, h, argb)| Some((w, h, rgba(w, h, &argb)?)));
    let menu = p
        .get("Menu")
        .and_then(|v| OwnedObjectPath::try_from(v.try_clone().ok()?).ok())
        .map(|o| o.to_string())
        .filter(|o| o != "/");
    let item = Item {
        clave: clave.to_owned(),
        id: texto("Id"),
        titulo: texto("Title"),
        tooltip,
        pasivo: estado == "Passive",
        atencion: estado == "NeedsAttention",
        icono: texto("IconName"),
        icono_atencion: texto("AttentionIconName"),
        ruta_tema: texto("IconThemePath"),
        pixmap,
        menu,
        es_menu: p
            .get("ItemIsMenu")
            .and_then(|v| bool::try_from(v.try_clone().ok()?).ok())
            .unwrap_or(false),
    };
    Item {
        tooltip: if item.tooltip.is_empty() {
            item.titulo.clone()
        } else {
            item.tooltip.clone()
        },
        ..item
    }
}

pub fn rgba(w: i32, h: i32, argb: &[u8]) -> Option<Vec<u8>> {
    let n = usize::try_from(w).ok()? * usize::try_from(h).ok()? * 4;
    if n == 0 || argb.len() != n {
        return None;
    }
    Some(
        argb.as_chunks::<4>()
            .0
            .iter()
            .flat_map(|[a, r, g, b]| [*r, *g, *b, *a])
            .collect(),
    )
}

type Oyentes = RefCell<Vec<Rc<dyn Fn()>>>;

#[derive(Clone)]
pub struct Bandeja {
    inner: Rc<Inner>,
}

struct Inner {
    conn: RefCell<Option<Connection>>,
    items: RefCell<Vec<Item>>,
    tareas: RefCell<HashMap<String, glib::JoinHandle<()>>>,
    oyentes: Oyentes,
}

impl Bandeja {
    pub fn new() -> Self {
        let b = Bandeja {
            inner: Rc::new(Inner {
                conn: RefCell::default(),
                items: RefCell::default(),
                tareas: RefCell::default(),
                oyentes: RefCell::default(),
            }),
        };
        let yo = b.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = yo.arrancar().await {
                eprintln!("morgan: bandeja: {e}");
            }
        });
        b
    }

    pub fn items(&self) -> Vec<Item> {
        self.inner
            .items
            .borrow()
            .iter()
            .filter(|i| !i.pasivo)
            .cloned()
            .collect()
    }

    pub fn al_cambiar(&self, f: impl Fn() + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    fn avisar(&self) {
        let oyentes = self.inner.oyentes.borrow().clone();
        for f in oyentes {
            f();
        }
    }

    fn conn(&self) -> Option<Connection> {
        self.inner.conn.borrow().clone()
    }

    async fn arrancar(&self) -> zbus::Result<()> {
        let (tx, rx) = async_channel::unbounded();
        // El watcher, si el nombre está libre. `name` no le quita el nombre a nadie.
        let propio = zbus::connection::Builder::session()?
            .serve_at(
                RUTA_WATCHER,
                Watcher {
                    items: Vec::new(),
                    host: tx,
                },
            )?
            .name(WATCHER)?
            .build()
            .await;
        match propio {
            Ok(conn) => {
                self.inner.conn.replace(Some(conn.clone()));
                let yo = self.clone();
                glib::MainContext::default().spawn_local(async move {
                    yo.podar_sin_dueno(conn).await;
                });
                while let Ok(r) = rx.recv().await {
                    self.registro(r);
                }
                Ok(())
            }
            Err(e) => {
                eprintln!("morgan: bandeja: el watcher es de otro ({e}); morgan solo lo lee");
                let conn = Connection::session().await?;
                self.inner.conn.replace(Some(conn.clone()));
                self.leer_watcher_ajeno(&conn).await
            }
        }
    }

    /// Lo normal es que la app se cierre sin avisar: un ítem cuyo bus se va sale del registro.
    async fn podar_sin_dueno(&self, conn: Connection) {
        let regla = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .and_then(|b| b.member("NameOwnerChanged"))
            .map(|b| b.build())
            .expect("regla fija");
        let Ok(mut senales) = MessageStream::for_match_rule(regla, &conn, Some(64)).await else {
            return;
        };
        while let Some(Ok(m)) = senales.next().await {
            let Ok((nombre, _, nuevo)) = m.body().deserialize::<(String, String, String)>() else {
                continue;
            };
            if !nuevo.is_empty() {
                continue;
            }
            let Ok(iface) = conn
                .object_server()
                .interface::<_, Watcher>(RUTA_WATCHER)
                .await
            else {
                continue;
            };
            let fuera = iface.get_mut().await.sin_dueno(&nombre);
            if fuera.is_empty() {
                continue;
            }
            let e = iface.signal_emitter();
            for clave in fuera {
                let _ = Watcher::status_notifier_item_unregistered(e, &clave).await;
                self.registro(Registro::Baja(clave));
            }
            let _ = iface
                .get()
                .await
                .registered_status_notifier_items_changed(e)
                .await;
        }
    }

    /// Sus señales primero y su lista después, así no se pierde un alta entre las dos.
    async fn leer_watcher_ajeno(&self, conn: &Connection) -> zbus::Result<()> {
        let regla = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(WATCHER)?
            .interface(WATCHER)?
            .build();
        let mut senales = MessageStream::for_match_rule(regla, conn, Some(64)).await?;
        let lista = conn
            .call_method(
                Some(WATCHER),
                RUTA_WATCHER,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(WATCHER, "RegisteredStatusNotifierItems"),
            )
            .await?;
        let v: OwnedValue = lista.body().deserialize()?;
        for clave in Vec::<String>::try_from(v).unwrap_or_default() {
            self.registro(Registro::Alta(clave));
        }
        let nombre = conn
            .unique_name()
            .map(|n| n.to_string())
            .unwrap_or_default();
        conn.call_method(
            Some(WATCHER),
            RUTA_WATCHER,
            Some(WATCHER),
            "RegisterStatusNotifierHost",
            &(nombre.as_str(),),
        )
        .await?;
        while let Some(Ok(m)) = senales.next().await {
            let miembro = m.header().member().map(|n| n.to_string());
            let Ok(clave) = m.body().deserialize::<String>() else {
                continue;
            };
            match miembro.as_deref() {
                Some("StatusNotifierItemRegistered") => self.registro(Registro::Alta(clave)),
                Some("StatusNotifierItemUnregistered") => self.registro(Registro::Baja(clave)),
                _ => {}
            }
        }
        Ok(())
    }

    fn registro(&self, r: Registro) {
        match r {
            Registro::Alta(clave) => {
                if self.inner.tareas.borrow().contains_key(&clave) {
                    return;
                }
                let yo = self.clone();
                let c = clave.clone();
                let tarea = glib::MainContext::default().spawn_local(async move {
                    yo.seguir_item(&c).await;
                });
                self.inner.tareas.borrow_mut().insert(clave, tarea);
            }
            Registro::Baja(clave) => {
                if let Some(t) = self.inner.tareas.borrow_mut().remove(&clave) {
                    t.abort();
                }
                let antes = self.inner.items.borrow().len();
                self.inner.items.borrow_mut().retain(|i| i.clave != clave);
                if self.inner.items.borrow().len() != antes {
                    self.avisar();
                }
            }
        }
    }

    async fn seguir_item(&self, clave: &str) {
        let Some(conn) = self.conn() else { return };
        let bus = bus_de(clave).to_owned();
        let ruta = ruta_de(clave);
        let regla = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(bus.as_str())
            .and_then(|b| b.path(ruta.as_str()))
            .and_then(|b| b.interface(ITEM))
            .map(|b| b.build());
        let mut senales = match regla {
            Ok(r) => MessageStream::for_match_rule(r, &conn, Some(64)).await.ok(),
            Err(_) => None,
        };
        self.leer_item(&conn, clave).await;
        let Some(senales) = senales.as_mut() else {
            return;
        };
        while senales.next().await.is_some() {
            glib::timeout_future(RAFAGA).await;
            while let Some(Some(_)) = futures_lite::future::poll_once(senales.next()).await {}
            self.leer_item(&conn, clave).await;
        }
    }

    async fn leer_item(&self, conn: &Connection, clave: &str) {
        let r = conn
            .call_method(
                Some(bus_de(clave)),
                ruta_de(clave).as_str(),
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &(ITEM,),
            )
            .await;
        let props: HashMap<String, OwnedValue> = match r.and_then(|m| m.body().deserialize()) {
            Ok(p) => p,
            Err(e) => return eprintln!("morgan: bandeja: {clave}: {e}"),
        };
        let item = item_de(clave, &props);
        let cambio = {
            let mut items = self.inner.items.borrow_mut();
            match items.iter_mut().find(|i| i.clave == clave) {
                Some(i) if *i == item => false,
                Some(i) => {
                    *i = item;
                    true
                }
                None => {
                    items.push(item);
                    true
                }
            }
        };
        if cambio {
            self.avisar();
        }
    }

    fn item(&self, clave: &str) -> Option<Item> {
        self.inner
            .items
            .borrow()
            .iter()
            .find(|i| i.clave == clave)
            .cloned()
    }

    async fn llamar_item(
        &self,
        clave: &str,
        metodo: &str,
        cuerpo: &(impl serde::Serialize + zbus::zvariant::DynamicType),
    ) -> zbus::Result<()> {
        let conn = self.conn().ok_or(zbus::Error::Failure("sin bus".into()))?;
        conn.call_method(
            Some(bus_de(clave)),
            ruta_de(clave).as_str(),
            Some(ITEM),
            metodo,
            cuerpo,
        )
        .await?;
        Ok(())
    }

    /// `false` si en vez de activar hay que abrir el menú: el ítem dice que es solo menú, o no
    /// tiene `Activate` (los de libappindicator, como Steam).
    pub async fn activar(&self, clave: &str) -> bool {
        if self.item(clave).is_some_and(|i| i.es_menu) {
            return false;
        }
        match self.llamar_item(clave, "Activate", &(0i32, 0i32)).await {
            Ok(()) => true,
            Err(e) => {
                eprintln!("morgan: bandeja: {clave}: Activate: {e}");
                false
            }
        }
    }

    pub fn secundario(&self, clave: &str) {
        let (yo, clave) = (self.clone(), clave.to_owned());
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = yo
                .llamar_item(&clave, "SecondaryActivate", &(0i32, 0i32))
                .await
            {
                eprintln!("morgan: bandeja: {clave}: SecondaryActivate: {e}");
            }
        });
    }

    pub fn desplazar(&self, clave: &str, delta: i32) {
        let (yo, clave) = (self.clone(), clave.to_owned());
        glib::MainContext::default().spawn_local(async move {
            let _ = yo.llamar_item(&clave, "Scroll", &(delta, "vertical")).await;
        });
    }

    pub async fn menu(&self, clave: &str) -> Option<menu::Nodo> {
        let ruta = self.item(clave)?.menu?;
        let conn = self.conn()?;
        let bus = bus_de(clave);
        // Algunas apps arman el menú recién acá; si no lo implementan, no importa.
        let _ = conn
            .call_method(
                Some(bus),
                ruta.as_str(),
                Some(DBUSMENU),
                "AboutToShow",
                &(0i32,),
            )
            .await;
        let r = conn
            .call_method(
                Some(bus),
                ruta.as_str(),
                Some(DBUSMENU),
                "GetLayout",
                &(0i32, -1i32, Vec::<&str>::new()),
            )
            .await
            .inspect_err(|e| eprintln!("morgan: bandeja: {clave}: GetLayout: {e}"))
            .ok()?;
        type Raiz = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);
        let (_, (id, props, hijos)): (u32, Raiz) = r
            .body()
            .deserialize()
            .inspect_err(|e| eprintln!("morgan: bandeja: {clave}: menú ilegible: {e}"))
            .ok()?;
        let hijos: Vec<Value<'_>> = hijos
            .iter()
            .filter_map(|h| h.try_clone().ok().map(Value::from))
            .collect();
        menu::armar_nodo(id, &props, &hijos)
    }

    pub fn elegir(&self, clave: &str, id: i32) {
        let Some(ruta) = self.item(clave).and_then(|i| i.menu) else {
            return;
        };
        let (yo, clave) = (self.clone(), clave.to_owned());
        glib::MainContext::default().spawn_local(async move {
            let Some(conn) = yo.conn() else { return };
            let ahora = u32::try_from(glib::real_time() / 1_000_000).unwrap_or(0);
            let r = conn
                .call_method(
                    Some(bus_de(&clave)),
                    ruta.as_str(),
                    Some(DBUSMENU),
                    "Event",
                    &(id, "clicked", Value::from(0i32), ahora),
                )
                .await;
            if let Err(e) = r {
                eprintln!("morgan: bandeja: {clave}: Event: {e}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claves_del_registro() {
        // libappindicator (Steam) y Electron mandan una ruta.
        assert_eq!(
            clave_item("/org/ayatana/NotificationItem/steam", ":1.58"),
            ":1.58/org/ayatana/NotificationItem/steam"
        );
        // KDE y Qt mandan un nombre de bus.
        assert_eq!(
            clave_item("org.kde.StatusNotifierItem-1504-1", ":1.70"),
            "org.kde.StatusNotifierItem-1504-1/StatusNotifierItem"
        );
        assert_eq!(
            clave_item(":1.61/StatusNotifierItem", ":1.61"),
            ":1.61/StatusNotifierItem"
        );
        let c = ":1.58/org/ayatana/NotificationItem/steam";
        assert_eq!(bus_de(c), ":1.58");
        assert_eq!(ruta_de(c), "/org/ayatana/NotificationItem/steam");
        assert_eq!(ruta_de(":1.9"), "/StatusNotifierItem");
    }

    #[test]
    fn el_watcher_poda_lo_que_se_fue() {
        let (tx, _rx) = async_channel::unbounded();
        let mut w = Watcher {
            items: vec![":1.5/A".into(), ":1.6/B".into(), ":1.5/C".into()],
            host: tx,
        };
        assert_eq!(w.sin_dueno(":1.5"), [":1.5/A", ":1.5/C"]);
        assert_eq!(w.items, [":1.6/B"]);
        assert!(w.sin_dueno(":1.9").is_empty());
    }

    #[test]
    fn pixmap_argb_a_rgba() {
        assert_eq!(rgba(1, 1, &[0x80, 1, 2, 3]), Some(vec![1, 2, 3, 0x80]));
        assert_eq!(rgba(2, 1, &[0; 4]), None);
        assert_eq!(rgba(0, 0, &[]), None);
    }

    fn v(x: impl Into<Value<'static>>) -> OwnedValue {
        x.into().try_into_owned().unwrap()
    }

    #[test]
    fn propiedades_de_un_item() {
        let mut p = HashMap::new();
        p.insert("Id".into(), v("Cachy-Update"));
        p.insert("Title".into(), v("Cachy-Update"));
        p.insert("Status".into(), v("Active"));
        p.insert("IconName".into(), v("cachy-update-blue"));
        p.insert(
            "Menu".into(),
            v(OwnedObjectPath::try_from("/MenuBar").unwrap()),
        );
        let tt: Tooltip = (
            String::new(),
            vec![],
            "Cachy-Update".into(),
            "El sistema está actualizado".into(),
        );
        p.insert("ToolTip".into(), v(Value::from(tt)));
        let i = item_de("k/StatusNotifierItem", &p);
        assert_eq!(i.id, "Cachy-Update");
        assert_eq!(i.icono, "cachy-update-blue");
        assert_eq!(i.tooltip, "Cachy-Update\nEl sistema está actualizado");
        assert_eq!(i.menu.as_deref(), Some("/MenuBar"));
        assert!(!i.pasivo && !i.atencion && !i.es_menu);

        // Lo mínimo de libappindicator: sin ToolTip, el tooltip es el título.
        let mut p = HashMap::new();
        p.insert("Id".into(), v("steam"));
        p.insert("Title".into(), v("Steam"));
        p.insert("Status".into(), v("Passive"));
        p.insert(
            "IconPixmap".into(),
            v(Value::from(vec![
                (1i32, 1i32, vec![255u8, 9, 9, 9]),
                (2, 2, vec![0u8; 16]),
            ])),
        );
        let i = item_de("k", &p);
        assert_eq!(i.tooltip, "Steam");
        assert!(i.pasivo);
        assert_eq!(i.pixmap.as_ref().map(|(w, h, _)| (*w, *h)), Some((2, 2)));
        assert_eq!(i.menu, None);
    }
}
