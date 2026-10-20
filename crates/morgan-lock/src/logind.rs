//! `morgan-lock` corre en una unidad de usuario, fuera de la sesión: `session/auto` es
//! entonces la sesión gráfica del usuario (org.freedesktop.login1(5)). Las señales salen
//! por la ruta real, así que se resuelve con `Id` → `GetSession`.

use futures_lite::StreamExt;
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, Proxy};

const LOGIND: &str = "org.freedesktop.login1";
const RUTA: &str = "/org/freedesktop/login1";
const SESION: &str = "org.freedesktop.login1.Session";

pub struct Logind {
    conn: Connection,
    sesion: OwnedObjectPath,
}

impl Logind {
    pub async fn conectar() -> zbus::Result<Self> {
        let conn = Connection::system().await?;
        let auto = Proxy::new(
            &conn,
            LOGIND,
            "/org/freedesktop/login1/session/auto",
            SESION,
        )
        .await?;
        let id: String = auto.get_property("Id").await?;
        let manager = Proxy::new(&conn, LOGIND, RUTA, "org.freedesktop.login1.Manager").await?;
        let sesion: OwnedObjectPath = manager.call("GetSession", &(id,)).await?;
        Ok(Logind { conn, sesion })
    }

    async fn proxy_sesion(&self) -> zbus::Result<Proxy<'_>> {
        Proxy::new(&self.conn, LOGIND, self.sesion.as_ref(), SESION).await
    }

    pub async fn marcar_bloqueada(&self, si: bool) -> zbus::Result<()> {
        self.proxy_sesion()
            .await?
            .call::<_, _, ()>("SetLockedHint", &(si,))
            .await
    }

    pub async fn bloqueada(&self) -> zbus::Result<bool> {
        self.proxy_sesion().await?.get_property("LockedHint").await
    }

    pub async fn esperar_unlock(&self) -> zbus::Result<()> {
        let p = self.proxy_sesion().await?;
        let mut s = p.receive_signal("Unlock").await?;
        s.next().await;
        Ok(())
    }

    pub async fn al_despertar(&self, f: impl Fn()) -> zbus::Result<()> {
        let p = Proxy::new(&self.conn, LOGIND, RUTA, "org.freedesktop.login1.Manager").await?;
        let mut s = p.receive_signal("PrepareForSleep").await?;
        while let Some(m) = s.next().await {
            if let Ok(false) = m.body().deserialize::<bool>() {
                f();
            }
        }
        Ok(())
    }
}
