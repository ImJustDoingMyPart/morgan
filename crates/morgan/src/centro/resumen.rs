//! Lo que se escribe sale del estado del sistema, no de lo pedido.

use crate::bluetooth;
use crate::energia;
use crate::red::{self, Tipo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resumen {
    pub glifo: &'static str,
    pub estado: String,
    pub encendida: bool,
}

/// La conexión activa que más pesa (cable antes que Wi-Fi, y las dos antes que una VPN);
/// con una VPN encima, se nombra al final.
pub fn red(e: &red::Estado) -> Resumen {
    let activas: Vec<&red::Conexion> = e.conexiones.iter().filter(|c| c.activa.is_some()).collect();
    let base = activas.iter().find(|c| c.tipo != Tipo::Vpn);
    let vpn = activas.iter().any(|c| c.tipo == Tipo::Vpn);
    match base {
        Some(c) => {
            let mut estado = match c.tipo {
                Tipo::Cable => "Cable".to_owned(),
                _ => c.nombre.clone(),
            };
            if vpn {
                estado.push_str(" · VPN");
            }
            Resumen {
                glifo: c.tipo.glifo(),
                estado,
                encendida: true,
            }
        }
        None if vpn => Resumen {
            glifo: Tipo::Vpn.glifo(),
            estado: "VPN".to_owned(),
            encendida: true,
        },
        None => Resumen {
            glifo: "󰈂",
            estado: "Sin conexión".to_owned(),
            encendida: false,
        },
    }
}

pub fn bluetooth(e: &bluetooth::Estado) -> Resumen {
    if !e.encendido {
        return Resumen {
            glifo: "󰂲",
            estado: "Apagado".to_owned(),
            encendida: false,
        };
    }
    let conectados: Vec<&bluetooth::Dispositivo> =
        e.dispositivos.iter().filter(|d| d.conectado).collect();
    let estado = match conectados.as_slice() {
        [] => "Encendido".to_owned(),
        [d] => d.nombre.clone(),
        v => format!("{} conectados", v.len()),
    };
    Resumen {
        glifo: if conectados.is_empty() {
            "󰂯"
        } else {
            "󰂱"
        },
        estado,
        encendida: true,
    }
}

/// Encendida solo fuera de equilibrado: es lo que se nota.
pub fn energia(activo: Option<&str>) -> Resumen {
    let perfil = activo.unwrap_or("balanced");
    let (glifo, nombre) = energia::etiqueta(perfil);
    Resumen {
        glifo,
        estado: nombre.to_owned(),
        encendida: perfil != "balanced",
    }
}

fn interruptor(glifo: &'static str, encendida: bool, si: &str, no: &str) -> Resumen {
    Resumen {
        glifo,
        estado: if encendida { si } else { no }.to_owned(),
        encendida,
    }
}

pub fn no_molestar(activo: bool) -> Resumen {
    interruptor("󰂛", activo, "Solo críticas", "Apagado")
}

pub fn caffeine(activo: bool) -> Resumen {
    interruptor("󰅶", activo, "Sin apagar pantalla", "Apagado")
}

pub fn luz(activo: bool) -> Resumen {
    interruptor("󰖔", activo, "Encendida", "Apagada")
}

pub fn sesion(segundos: u64) -> Resumen {
    let minutos = segundos / 60;
    let estado = match minutos {
        0 => "Recién encendida".to_owned(),
        m if m < 60 => format!("{m} min encendida"),
        m if m < 24 * 60 => format!("{} h encendida", m / 60),
        m => format!("{} d encendida", m / (24 * 60)),
    };
    Resumen {
        glifo: "󰐥",
        estado,
        encendida: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::OwnedObjectPath;

    fn ruta(s: &str) -> OwnedObjectPath {
        OwnedObjectPath::try_from(s).unwrap()
    }

    fn conexion(nombre: &str, tipo: Tipo, activa: bool) -> red::Conexion {
        red::Conexion {
            ruta: ruta("/c"),
            nombre: nombre.into(),
            tipo,
            activa: activa.then(|| ruta("/a")),
        }
    }

    #[test]
    fn red_por_peso() {
        let e = red::Estado {
            wifi: None,
            conexiones: vec![
                conexion("Cableada 1", Tipo::Cable, true),
                conexion("Casa", Tipo::Wifi, false),
            ],
        };
        assert_eq!(red(&e).estado, "Cable");
        assert!(red(&e).encendida);

        let e = red::Estado {
            wifi: Some(true),
            conexiones: vec![
                conexion("Casa", Tipo::Wifi, true),
                conexion("Trabajo", Tipo::Vpn, true),
            ],
        };
        assert_eq!(red(&e).estado, "Casa · VPN");
        assert_eq!(red(&e).glifo, Tipo::Wifi.glifo());

        let e = red::Estado {
            wifi: None,
            conexiones: vec![conexion("Cableada 1", Tipo::Cable, false)],
        };
        assert_eq!(red(&e).estado, "Sin conexión");
        assert!(!red(&e).encendida);
    }

    #[test]
    fn bluetooth_por_conectados() {
        let d = |nombre: &str, conectado: bool| bluetooth::Dispositivo {
            ruta: ruta("/d"),
            nombre: nombre.into(),
            icono: String::new(),
            conectado,
        };
        let mut e = bluetooth::Estado {
            adaptador: Some(ruta("/hci0")),
            encendido: false,
            dispositivos: vec![d("Auris", true)],
        };
        assert_eq!(bluetooth(&e).estado, "Apagado");
        e.encendido = true;
        assert_eq!(bluetooth(&e).estado, "Auris");
        e.dispositivos.push(d("Joystick", true));
        assert_eq!(bluetooth(&e).estado, "2 conectados");
        e.dispositivos.clear();
        assert_eq!(bluetooth(&e).estado, "Encendido");
    }

    #[test]
    fn sesion_en_la_unidad_que_importa() {
        assert_eq!(sesion(30).estado, "Recién encendida");
        assert_eq!(sesion(12 * 60 + 5).estado, "12 min encendida");
        assert_eq!(sesion(3 * 3600 + 1200).estado, "3 h encendida");
        assert_eq!(sesion(2 * 86400 + 5).estado, "2 d encendida");
    }

    #[test]
    fn energia_encendida_fuera_de_equilibrado() {
        assert!(!energia(None).encendida);
        assert!(!energia(Some("balanced")).encendida);
        assert_eq!(energia(Some("performance")).estado, "Rendimiento");
        assert!(energia(Some("performance")).encendida);
    }
}
