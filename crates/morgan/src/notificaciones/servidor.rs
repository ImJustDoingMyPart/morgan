//! Los métodos corren en el ejecutor de zbus, fuera del hilo de GTK: acá solo se traduce lo
//! que llega y se le pasa al hilo de GTK, que tiene el estado y decide (y contesta el id).
//! Las señales salen del hilo de GTK con [`Emisor`].

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};

use super::markup;
use super::modelo::{Accion, Imagen, Notificacion, PedidoSonido, Pixeles, Urgencia};

pub const NOMBRE: &str = "org.freedesktop.Notifications";
pub const RUTA: &str = "/org/freedesktop/Notifications";

pub enum Pedido {
    /// `id` viene en 0 salvo que sea un reemplazo (`replaces_id`); el hilo de GTK contesta
    /// el id que quedó.
    Notificar {
        n: Box<Notificacion>,
        id: async_channel::Sender<u32>,
    },
    Cerrar(u32),
}

pub struct Servidor {
    pub pedidos: async_channel::Sender<Pedido>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Servidor {
    fn get_capabilities(&self) -> Vec<&'static str> {
        vec![
            "actions",
            "body",
            "body-hyperlinks",
            "body-markup",
            "icon-static",
            "persistence",
        ]
    }

    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> zbus::fdo::Result<u32> {
        let n = traducir(
            app_name,
            replaces_id,
            &app_icon,
            summary,
            &body,
            &actions,
            &hints,
            expire_timeout,
        );
        let (tx, rx) = async_channel::bounded(1);
        self.pedidos
            .send(Pedido::Notificar {
                n: Box::new(n),
                id: tx,
            })
            .await
            .map_err(|_| zbus::fdo::Error::Failed("morgan se está cerrando".into()))?;
        rx.recv()
            .await
            .map_err(|_| zbus::fdo::Error::Failed("morgan se está cerrando".into()))
    }

    async fn close_notification(&self, id: u32) {
        let _ = self.pedidos.send(Pedido::Cerrar(id)).await;
    }

    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        ("morgan", "morgan", env!("CARGO_PKG_VERSION"), "1.2")
    }

    #[zbus(signal)]
    async fn notification_closed(e: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(e: &SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Razon {
    Vencio = 1,
    Usuario = 2,
    Pedido = 3,
}

#[derive(Clone)]
pub struct Emisor {
    conn: zbus::Connection,
}

impl Emisor {
    pub fn new(conn: zbus::Connection) -> Self {
        Emisor { conn }
    }

    pub async fn cerrada(&self, id: u32, razon: Razon) {
        if let Ok(e) = SignalEmitter::new(&self.conn, RUTA) {
            let _ = Servidor::notification_closed(&e, id, razon as u32).await;
        }
    }

    pub async fn accion(&self, id: u32, clave: &str) {
        if let Ok(e) = SignalEmitter::new(&self.conn, RUTA) {
            let _ = Servidor::action_invoked(&e, id, clave).await;
        }
    }
}

/// Si otro servidor ya tiene el nombre, falla: morgan no se lo quita.
pub async fn levantar(pedidos: async_channel::Sender<Pedido>) -> zbus::Result<zbus::Connection> {
    zbus::connection::Builder::session()?
        .serve_at(RUTA, Servidor { pedidos })?
        .name(NOMBRE)?
        .build()
        .await
}

#[allow(clippy::too_many_arguments)]
pub fn traducir(
    app: String,
    replaces_id: u32,
    app_icon: &str,
    resumen: String,
    cuerpo: &str,
    acciones: &[String],
    hints: &HashMap<String, OwnedValue>,
    vence_ms: i32,
) -> Notificacion {
    let texto = |k: &str| hints.get(k).and_then(|v| <&str>::try_from(&**v).ok());
    let booleano = |k: &str| {
        hints
            .get(k)
            .and_then(|v| bool::try_from(&**v).ok())
            .unwrap_or(false)
    };
    let urgencia = hints
        .get("urgency")
        .and_then(entero)
        .map(|u| Urgencia::de(u.clamp(0, 255) as u8))
        .unwrap_or(Urgencia::Normal);
    // Precedencia de la spec: image-data, image-path, app_icon, icon_data.
    let datos = ["image-data", "image_data"]
        .iter()
        .find_map(|k| hints.get(*k).and_then(pixeles));
    let ruta = ["image-path", "image_path"]
        .iter()
        .find_map(|k| texto(k))
        .and_then(Imagen::de_texto);
    let icono_app = Imagen::de_texto(app_icon);
    let imagen = datos
        .map(Imagen::Datos)
        .or(ruta)
        .or_else(|| icono_app.clone())
        .or_else(|| hints.get("icon_data").and_then(pixeles).map(Imagen::Datos));
    // El ícono chico de la app solo tiene sentido sobre otra imagen.
    let icono_app = icono_app.filter(|i| Some(i) != imagen.as_ref());
    Notificacion {
        id: replaces_id,
        app,
        entrada: texto("desktop-entry").map(str::to_owned),
        nombre_app: None,
        icono_app,
        resumen,
        cuerpo: markup::seguro(cuerpo),
        acciones: acciones
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[clave, etiqueta]| Accion {
                clave: clave.clone(),
                etiqueta: etiqueta.clone(),
            })
            .collect(),
        urgencia,
        categoria: texto("category").map(str::to_owned),
        imagen,
        residente: booleano("resident"),
        transitoria: booleano("transient"),
        progreso: hints
            .get("value")
            .and_then(entero)
            .map(|v| v.clamp(0, 100) as i32),
        vence_ms,
        llegada: ahora(),
        sonido: if booleano("suppress-sound") {
            PedidoSonido::Callar
        } else if let Some(f) = texto("sound-file") {
            PedidoSonido::Archivo(f.to_owned())
        } else if let Some(n) = texto("sound-name") {
            PedidoSonido::Nombre(n.to_owned())
        } else {
            PedidoSonido::Normal
        },
    }
}

pub fn ahora() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Un hint numérico. La spec dice `byte` para `urgency` e `int` para `value`, pero hay
/// emisores que mandan otro entero.
fn entero(v: &OwnedValue) -> Option<i64> {
    match &**v {
        Value::U8(x) => Some((*x).into()),
        Value::I16(x) => Some((*x).into()),
        Value::U16(x) => Some((*x).into()),
        Value::I32(x) => Some((*x).into()),
        Value::U32(x) => Some((*x).into()),
        Value::I64(x) => Some(*x),
        Value::U64(x) => i64::try_from(*x).ok(),
        _ => None,
    }
}

/// `(iiibiiay)`: ancho, alto, bytes por fila, alfa, bits por canal, canales, datos. Solo se
/// aceptan 8 bits y 3 o 4 canales que coincidan con el alfa, con los datos completos.
fn pixeles(v: &OwnedValue) -> Option<Pixeles> {
    let Value::Structure(s) = &**v else {
        return None;
    };
    let f = s.fields();
    if f.len() != 7 {
        return None;
    }
    let i = |n: usize| match &f[n] {
        Value::I32(x) => Some(*x),
        _ => None,
    };
    let (ancho, alto, fila, bits, canales) = (i(0)?, i(1)?, i(2)?, i(4)?, i(5)?);
    let Value::Bool(alfa) = f[3] else {
        return None;
    };
    let datos: Vec<u8> = match &f[6] {
        Value::Array(a) => a
            .iter()
            .map(|b| match b {
                Value::U8(x) => Some(*x),
                _ => None,
            })
            .collect::<Option<_>>()?,
        _ => return None,
    };
    let validos = ancho > 0
        && alto > 0
        && bits == 8
        && canales == if alfa { 4 } else { 3 }
        && fila >= ancho * canales
        && datos.len() as i64 >= i64::from(fila) * i64::from(alto - 1) + i64::from(ancho * canales);
    validos.then_some(Pixeles {
        ancho,
        alto,
        fila,
        alfa,
        canales,
        datos,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{Structure, StructureBuilder};

    fn v(x: impl Into<Value<'static>>) -> OwnedValue {
        x.into().try_into().unwrap()
    }

    fn hints(pares: Vec<(&str, OwnedValue)>) -> HashMap<String, OwnedValue> {
        pares.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
    }

    fn t(app_icon: &str, h: HashMap<String, OwnedValue>) -> Notificacion {
        traducir("app".into(), 0, app_icon, "r".into(), "c", &[], &h, -1)
    }

    fn rgba(ancho: i32, alto: i32) -> OwnedValue {
        let s: Structure = StructureBuilder::new()
            .add_field(ancho)
            .add_field(alto)
            .add_field(ancho * 4)
            .add_field(true)
            .add_field(8i32)
            .add_field(4i32)
            .add_field(vec![0u8; (ancho * alto * 4) as usize])
            .build()
            .unwrap();
        v(s)
    }

    #[test]
    fn urgencia_y_flags() {
        let n = t(
            "",
            hints(vec![
                ("urgency", v(2u8)),
                ("resident", v(true)),
                ("transient", v(true)),
                ("category", v("email.arrived")),
                ("value", v(140i32)),
            ]),
        );
        assert_eq!(n.urgencia, Urgencia::Critica);
        assert!(n.residente && n.transitoria);
        assert_eq!(n.categoria.as_deref(), Some("email.arrived"));
        assert_eq!(n.progreso, Some(100));
        // Un entero que no es byte también vale.
        assert_eq!(
            t("", hints(vec![("urgency", v(0i32))])).urgencia,
            Urgencia::Baja
        );
        assert_eq!(t("", hints(vec![])).urgencia, Urgencia::Normal);
    }

    #[test]
    fn acciones_de_a_pares() {
        let a = ["default", "Abrir", "x", "Otra", "impar"].map(String::from);
        let n = traducir("a".into(), 0, "", "r".into(), "", &a, &HashMap::new(), -1);
        assert_eq!(n.acciones.len(), 2);
        assert_eq!(n.acciones[1].etiqueta, "Otra");
    }

    #[test]
    fn precedencia_de_imagenes() {
        // Solo app_icon: es la imagen, y no se repite como ícono chico.
        let n = t("firefox", hints(vec![]));
        assert_eq!(n.imagen, Some(Imagen::Nombre("firefox".into())));
        assert_eq!(n.icono_app, None);
        // image-path gana a app_icon, que queda como ícono chico.
        let n = t("firefox", hints(vec![("image-path", v("/a.png"))]));
        assert_eq!(n.imagen, Some(Imagen::Archivo("/a.png".into())));
        assert_eq!(n.icono_app, Some(Imagen::Nombre("firefox".into())));
        // image-data gana a todo.
        let n = t(
            "firefox",
            hints(vec![
                ("image-path", v("/a.png")),
                ("image-data", rgba(2, 2)),
            ]),
        );
        assert!(matches!(
            n.imagen,
            Some(Imagen::Datos(Pixeles {
                ancho: 2,
                alto: 2,
                ..
            }))
        ));
    }

    #[test]
    fn pixeles_invalidos_se_ignoran() {
        let s: Structure = StructureBuilder::new()
            .add_field(10i32)
            .add_field(10i32)
            .add_field(40i32)
            .add_field(true)
            .add_field(8i32)
            .add_field(4i32)
            .add_field(vec![0u8; 12])
            .build()
            .unwrap();
        let n = t("", hints(vec![("image-data", v(s))]));
        assert_eq!(n.imagen, None);
    }

    #[test]
    fn el_cuerpo_sale_saneado() {
        let n = traducir(
            "a".into(),
            0,
            "",
            "r".into(),
            "<b>x",
            &[],
            &HashMap::new(),
            -1,
        );
        assert_eq!(n.cuerpo, "<b>x</b>");
    }
}
