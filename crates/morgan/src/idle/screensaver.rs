//! Quien tiene el nombre es el dueño del idle, así que morgan no se lo quita a nadie.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use futures_lite::StreamExt;
use zbus::message::Header;
use zbus::{Connection, MatchRule, MessageStream};

pub const NOMBRE: &str = "org.freedesktop.ScreenSaver";
/// La spec de facto de KDE publica las dos.
const RUTAS: [&str; 2] = ["/org/freedesktop/ScreenSaver", "/ScreenSaver"];

#[derive(Default)]
pub struct Tabla {
    proxima: u32,
    inhibidores: BTreeMap<u32, (String, String)>,
}

impl Tabla {
    pub fn inhibir(&mut self, emisor: &str, app: &str) -> u32 {
        // La cookie 0 no se usa: algunos clientes la toman como "sin inhibidor".
        self.proxima = self.proxima.wrapping_add(1).max(1);
        while self.inhibidores.contains_key(&self.proxima) {
            self.proxima = self.proxima.wrapping_add(1).max(1);
        }
        self.inhibidores
            .insert(self.proxima, (emisor.to_owned(), app.to_owned()));
        self.proxima
    }

    pub fn soltar(&mut self, emisor: &str, cookie: u32) {
        if self
            .inhibidores
            .get(&cookie)
            .is_some_and(|(e, _)| e == emisor)
        {
            self.inhibidores.remove(&cookie);
        }
    }

    pub fn sin_dueno(&mut self, nombre: &str) {
        self.inhibidores.retain(|_, (e, _)| e != nombre);
    }

    pub fn cuantos(&self) -> usize {
        self.inhibidores.len()
    }
}

struct ScreenSaver {
    tabla: Arc<Mutex<Tabla>>,
    aviso: async_channel::Sender<usize>,
}

impl ScreenSaver {
    async fn avisar(&self) {
        let n = self.tabla.lock().expect("tabla").cuantos();
        let _ = self.aviso.send(n).await;
    }
}

fn emisor(h: &Header<'_>) -> String {
    h.sender().map(|s| s.to_string()).unwrap_or_default()
}

#[zbus::interface(name = "org.freedesktop.ScreenSaver")]
impl ScreenSaver {
    async fn inhibit(
        &self,
        application_name: &str,
        _reason_for_inhibit: &str,
        #[zbus(header)] header: Header<'_>,
    ) -> u32 {
        let cookie = self
            .tabla
            .lock()
            .expect("tabla")
            .inhibir(&emisor(&header), application_name);
        self.avisar().await;
        cookie
    }

    async fn un_inhibit(&self, cookie: u32, #[zbus(header)] header: Header<'_>) {
        self.tabla
            .lock()
            .expect("tabla")
            .soltar(&emisor(&header), cookie);
        self.avisar().await;
    }

    /// Acá no hay protector de pantalla: lo que hay es el bloqueo, que tiene su estado en logind.
    fn get_active(&self) -> bool {
        false
    }
}

pub async fn levantar(aviso: async_channel::Sender<usize>) -> zbus::Result<Connection> {
    let tabla = Arc::new(Mutex::new(Tabla::default()));
    let mut b = zbus::connection::Builder::session()?;
    for ruta in RUTAS {
        b = b.serve_at(
            ruta,
            ScreenSaver {
                tabla: tabla.clone(),
                aviso: aviso.clone(),
            },
        )?;
    }
    let conn = b.name(NOMBRE)?.build().await?;
    let c = conn.clone();
    let regla = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")
        .and_then(|b| b.member("NameOwnerChanged"))
        .map(|b| b.build())
        .expect("regla fija");
    let mut senales = MessageStream::for_match_rule(regla, &c, Some(64)).await?;
    gtk::glib::MainContext::default().spawn_local(async move {
        while let Some(Ok(m)) = senales.next().await {
            let Ok((nombre, _, nuevo)) = m.body().deserialize::<(String, String, String)>() else {
                continue;
            };
            if !nuevo.is_empty() {
                continue;
            }
            let n = {
                let mut t = tabla.lock().expect("tabla");
                let antes = t.cuantos();
                t.sin_dueno(&nombre);
                (t.cuantos() != antes).then(|| t.cuantos())
            };
            if let Some(n) = n {
                let _ = aviso.send(n).await;
            }
        }
    });
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_distintas_y_sin_cero() {
        let mut t = Tabla::default();
        let a = t.inhibir(":1.5", "firefox");
        let b = t.inhibir(":1.5", "firefox");
        assert_ne!(a, 0);
        assert_ne!(a, b);
        assert_eq!(t.cuantos(), 2);
    }

    #[test]
    fn solo_el_dueno_suelta() {
        let mut t = Tabla::default();
        let a = t.inhibir(":1.5", "mpv");
        t.soltar(":1.9", a);
        assert_eq!(t.cuantos(), 1);
        t.soltar(":1.5", a);
        assert_eq!(t.cuantos(), 0);
        t.soltar(":1.5", a);
        assert_eq!(t.cuantos(), 0);
    }

    #[test]
    fn el_que_se_va_suelta_todo() {
        let mut t = Tabla::default();
        t.inhibir(":1.5", "firefox");
        t.inhibir(":1.5", "firefox");
        t.inhibir(":1.7", "mpv");
        t.sin_dueno(":1.5");
        assert_eq!(t.cuantos(), 1);
    }

    #[test]
    fn la_cookie_no_repite_una_viva_al_dar_la_vuelta() {
        let mut t = Tabla::default();
        let a = t.inhibir(":1.5", "x");
        t.proxima = u32::MAX;
        let b = t.inhibir(":1.5", "y");
        assert_ne!(a, b);
        assert_ne!(b, 0);
    }
}
