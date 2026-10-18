//! En la baja de un USB ya no hay atributos de sysfs, así que se usa el nombre que se guardó en
//! el alta (o al arrancar). Un aparato que se reinicia al conectarse (baja y alta en el mismo
//! segundo) avisa una vez.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::fd::AsRawFd;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures_lite::StreamExt;
use gtk::glib;
use zbus::zvariant::OwnedValue;
use zbus::{MatchRule, MessageStream};

use crate::osd::Aviso;
use crate::sistema;

const MS: u32 = 2000;

const REPETIDO: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tipo {
    Teclado,
    Mouse,
    Mando,
    Audio,
    Almacenamiento,
    Camara,
    Telefono,
    Bluetooth,
    Red,
    Otro,
}

impl Tipo {
    /// Verificados contra el charset de JetBrainsMono.
    pub fn glifo(self) -> &'static str {
        match self {
            Tipo::Teclado => "󰌌",
            Tipo::Mouse => "󰍽",
            Tipo::Mando => "󰊴",
            Tipo::Audio => "󰋋",
            Tipo::Almacenamiento => "󱊞",
            Tipo::Camara => "󰖠",
            Tipo::Telefono => "󰏲",
            Tipo::Bluetooth => "󰂯",
            Tipo::Red => "󰈀",
            Tipo::Otro => "󰕓",
        }
    }
}

fn interfaces(s: &str) -> Vec<(u8, u8, u8)> {
    s.split(':')
        .filter(|i| i.len() == 6)
        .filter_map(|i| {
            let n = |r: std::ops::Range<usize>| u8::from_str_radix(&i[r], 16).ok();
            Some((n(0..2)?, n(2..4)?, n(4..6)?))
        })
        .collect()
}

/// Un HID sin protocolo de arranque (o de clase del fabricante, como los mandos de Xbox) es
/// un mando solo si el nombre lo dice: también son así los controladores RGB.
fn parece_mando(nombre: &str) -> bool {
    let n = nombre.to_lowercase();
    [
        "pad",
        "controller",
        "joystick",
        "xbox",
        "dualsense",
        "dualshock",
        "gamesir",
        "8bitdo",
    ]
    .iter()
    .any(|p| n.contains(p))
}

pub fn tipo_usb(ids: &str, nombre: &str) -> Option<Tipo> {
    let ifs = interfaces(ids);
    if ifs.is_empty() || ifs.iter().all(|i| i.0 == 0x09) {
        return None;
    }
    let clase = |c: u8| ifs.iter().any(|i| i.0 == c);
    let hid = |proto: u8| ifs.contains(&(0x03, 0x01, proto));
    Some(if clase(0x0e) {
        Tipo::Camara
    } else if clase(0x01) {
        // Un auricular con botones también trae HID: manda el audio.
        Tipo::Audio
    } else if clase(0x08) {
        Tipo::Almacenamiento
    } else if clase(0x06) {
        Tipo::Telefono
    } else if clase(0xe0) {
        Tipo::Bluetooth
    } else if clase(0x02) || clase(0x0a) {
        Tipo::Red
    } else if hid(0x02) {
        Tipo::Mouse
    } else if hid(0x01) {
        Tipo::Teclado
    } else if (clase(0x03) || clase(0xff)) && parece_mando(nombre) {
        Tipo::Mando
    } else {
        Tipo::Otro
    })
}

fn limpio(s: Option<&str>) -> Option<String> {
    let s = s?.split_whitespace().collect::<Vec<_>>().join(" ");
    (!s.is_empty()).then_some(s)
}

/// Un `product` genérico («USB Receiver») lleva delante la primera palabra del fabricante.
pub fn nombre_usb(
    product: Option<&str>,
    manufacturer: Option<&str>,
    modelo_db: Option<&str>,
    modelo: Option<&str>,
) -> Option<String> {
    let nombre = limpio(product)
        .or_else(|| limpio(modelo_db))
        .or_else(|| limpio(modelo.map(|m| m.replace('_', " ")).as_deref()))?;
    if !nombre.to_lowercase().starts_with("usb ") {
        return Some(nombre);
    }
    let marca = limpio(manufacturer)
        .and_then(|m| {
            m.split(' ')
                .next()
                .map(|p| p.trim_matches(['.', ',']).to_owned())
        })
        .filter(|m| !m.is_empty() && !["generic", "usb"].contains(&m.to_lowercase().as_str()));
    Some(match marca {
        Some(m) => format!("{m} {nombre}"),
        None => nombre,
    })
}

pub fn tipo_bluetooth(icono: &str) -> Tipo {
    match icono {
        "audio-headset" | "audio-headphones" | "audio-card" => Tipo::Audio,
        "input-mouse" => Tipo::Mouse,
        "input-keyboard" => Tipo::Teclado,
        "input-gaming" => Tipo::Mando,
        "phone" => Tipo::Telefono,
        _ => Tipo::Bluetooth,
    }
}

pub fn aviso(tipo: Tipo, nombre: &str, conectado: bool) -> Aviso {
    let texto = if conectado {
        "Conectado"
    } else {
        "Desconectado"
    };
    Aviso::estado(tipo.glifo(), nombre, texto, conectado, MS)
}

type Mostrar = Rc<dyn Fn(&Aviso)>;

struct Inner {
    mostrar: Mostrar,
    usb: RefCell<HashMap<OsString, (Tipo, String)>>,
    ultimo: RefCell<Option<(String, bool, Instant)>>,
    _monitor: RefCell<Option<glib::SourceId>>,
}

impl Inner {
    fn avisar(&self, tipo: Tipo, nombre: &str, conectado: bool) {
        let ahora = Instant::now();
        if let Some((n, c, t)) = &*self.ultimo.borrow()
            && n == nombre
            && *c == conectado
            && ahora.duration_since(*t) < REPETIDO
        {
            return;
        }
        self.ultimo
            .replace(Some((nombre.to_owned(), conectado, ahora)));
        (self.mostrar)(&aviso(tipo, nombre, conectado));
    }
}

fn texto(v: Option<&std::ffi::OsStr>) -> Option<&str> {
    v.and_then(|v| v.to_str())
}

fn de_udev(d: &udev::Device) -> Option<(Tipo, String)> {
    let s = texto;
    let nombre = nombre_usb(
        s(d.attribute_value("product")),
        s(d.attribute_value("manufacturer")),
        s(d.property_value("ID_MODEL_FROM_DATABASE")),
        s(d.property_value("ID_MODEL")),
    )?;
    let tipo = tipo_usb(s(d.property_value("ID_USB_INTERFACES"))?, &nombre)?;
    Some((tipo, nombre))
}

pub fn escuchar(mostrar: impl Fn(&Aviso) + 'static) {
    let inner = Rc::new(Inner {
        mostrar: Rc::new(mostrar),
        usb: RefCell::default(),
        ultimo: RefCell::default(),
        _monitor: RefCell::default(),
    });
    if let Err(e) = seguir_usb(&inner) {
        eprintln!("morgan: dispositivos: udev: {e}");
    }
    glib::MainContext::default().spawn_local(async move {
        if let Err(e) = seguir_bluetooth(&inner).await {
            eprintln!("morgan: dispositivos: BlueZ: {e}");
        }
    });
}

fn seguir_usb(inner: &Rc<Inner>) -> std::io::Result<()> {
    // Lo que ya está conectado, para tener el nombre si se desconecta.
    let mut lista = udev::Enumerator::new()?;
    lista.match_subsystem("usb")?;
    lista.match_property("DEVTYPE", "usb_device")?;
    for d in lista.scan_devices()? {
        if let Some(t) = de_udev(&d) {
            inner.usb.borrow_mut().insert(d.devpath().to_owned(), t);
        }
    }

    let socket = udev::MonitorBuilder::new()?
        .match_subsystem_devtype("usb", "usb_device")?
        .listen()?;
    let fd = socket.as_raw_fd();
    let debil = Rc::downgrade(inner);
    let id = glib_unix::unix_fd_add_local(
        fd,
        glib::IOCondition::IN | glib::IOCondition::HUP | glib::IOCondition::ERR,
        move |_, cond| {
            let Some(inner) = debil.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if cond.intersects(glib::IOCondition::HUP | glib::IOCondition::ERR) {
                eprintln!("morgan: dispositivos: se cortó el monitor de udev");
                return glib::ControlFlow::Break;
            }
            for e in socket.iter() {
                let d = e.device();
                let ruta = d.devpath().to_owned();
                match e.event_type() {
                    udev::EventType::Add => {
                        if let Some((tipo, nombre)) = de_udev(&d) {
                            inner.avisar(tipo, &nombre, true);
                            inner.usb.borrow_mut().insert(ruta, (tipo, nombre));
                        }
                    }
                    udev::EventType::Remove => {
                        let guardado = inner.usb.borrow_mut().remove(&ruta);
                        if let Some((tipo, nombre)) = guardado.or_else(|| de_udev(&d)) {
                            inner.avisar(tipo, &nombre, false);
                        }
                    }
                    _ => {}
                }
            }
            glib::ControlFlow::Continue
        },
    );
    inner._monitor.replace(Some(id));
    Ok(())
}

fn regla_bluez() -> zbus::Result<MatchRule<'static>> {
    Ok(MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.bluez")?
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .arg(0, "org.bluez.Device1")?
        .build())
}

async fn seguir_bluetooth(inner: &Rc<Inner>) -> zbus::Result<()> {
    let conn = sistema::bus().await?;
    let mut senales = MessageStream::for_match_rule(regla_bluez()?, &conn, Some(16)).await?;
    while let Some(Ok(m)) = senales.next().await {
        let Ok((_, cambios, _)) = m
            .body()
            .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
        else {
            continue;
        };
        let Some(conectado) = cambios
            .get("Connected")
            .and_then(|v| bool::try_from(v).ok())
        else {
            continue;
        };
        let Some(ruta) = m.header().path().map(|p| p.to_owned()) else {
            continue;
        };
        let props = zbus::Proxy::new(&conn, "org.bluez", ruta, "org.bluez.Device1").await?;
        let nombre: String = props
            .get_property("Alias")
            .await
            .unwrap_or_else(|_| "Bluetooth".into());
        let icono: String = props.get_property("Icon").await.unwrap_or_default();
        inner.avisar(tipo_bluetooth(&icono), &nombre, conectado);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osd::Cuerpo;

    #[test]
    fn tipos_de_los_aparatos_de_esta_maquina() {
        assert_eq!(tipo_usb(":090001:090002:", "USB2.1 Hub"), None);
        assert_eq!(tipo_usb(":090000:", "Hub"), None);
        assert_eq!(tipo_usb("", "x"), None);
        assert_eq!(
            tipo_usb(":030101:030001:", "Gaming KB"),
            Some(Tipo::Teclado)
        );
        assert_eq!(
            tipo_usb(":030101:030102:030000:", "Logitech USB Receiver"),
            Some(Tipo::Mouse)
        );
        assert_eq!(tipo_usb(":030000:", "GameSir-Cyclone 2"), Some(Tipo::Mando));
        assert_eq!(tipo_usb(":030000:", "ITE Device"), Some(Tipo::Otro));
        assert_eq!(
            tipo_usb(":010100:010200:030000:", "USB Sound Device"),
            Some(Tipo::Audio)
        );
        assert_eq!(
            tipo_usb(":e00101:", "Bluetooth Radio"),
            Some(Tipo::Bluetooth)
        );
        assert_eq!(tipo_usb(":080650:", "Flash"), Some(Tipo::Almacenamiento));
        assert_eq!(tipo_usb(":0e0100:0e0200:", "Webcam"), Some(Tipo::Camara));
        assert_eq!(tipo_usb(":ff5d01:", "Xbox Controller"), Some(Tipo::Mando));
    }

    #[test]
    fn nombres() {
        assert_eq!(
            nombre_usb(Some("USB Receiver"), Some("Logitech"), None, None).as_deref(),
            Some("Logitech USB Receiver")
        );
        assert_eq!(
            nombre_usb(
                Some("USB Sound Device        "),
                Some("C-Media INC."),
                None,
                None
            )
            .as_deref(),
            Some("C-Media USB Sound Device")
        );
        assert_eq!(
            nombre_usb(Some("USB2.0 Hub"), Some("Generic"), None, None).as_deref(),
            Some("USB2.0 Hub")
        );
        assert_eq!(
            nombre_usb(Some("USB Flash"), Some("Generic"), None, None).as_deref(),
            Some("USB Flash")
        );
        assert_eq!(
            nombre_usb(Some("Gaming KB "), Some("SINOWEALTH"), None, None).as_deref(),
            Some("Gaming KB")
        );
        assert_eq!(
            nombre_usb(
                None,
                None,
                Some("Lightspeed Receiver"),
                Some("USB_Receiver")
            )
            .as_deref(),
            Some("Lightspeed Receiver")
        );
        assert_eq!(
            nombre_usb(None, None, None, Some("Bluetooth_Radio")).as_deref(),
            Some("Bluetooth Radio")
        );
        assert_eq!(nombre_usb(Some("  "), None, None, None), None);
    }

    #[test]
    fn el_aviso() {
        let a = aviso(Tipo::Mando, "GameSir-Cyclone 2", true);
        assert_eq!(a.icono.as_deref(), Some("󰊴"));
        assert_eq!(a.titulo, "GameSir-Cyclone 2");
        assert_eq!(
            a.cuerpo,
            Cuerpo::Estado {
                texto: "Conectado".into(),
                encendido: true
            }
        );
        assert_eq!(
            aviso(Tipo::Audio, "x", false).cuerpo,
            Cuerpo::Estado {
                texto: "Desconectado".into(),
                encendido: false
            }
        );
    }

    #[test]
    fn bluetooth_por_icono() {
        assert_eq!(tipo_bluetooth("audio-headset"), Tipo::Audio);
        assert_eq!(tipo_bluetooth("input-gaming"), Tipo::Mando);
        assert_eq!(tipo_bluetooth("computer"), Tipo::Bluetooth);
    }
}
