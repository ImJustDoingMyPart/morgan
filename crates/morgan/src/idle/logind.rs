//! morgan corre en una unidad de usuario: `session/auto` es la sesión gráfica del usuario, y
//! las señales salen por la ruta real (`Id` → `GetSession`), como en `morgan-lock`.

use std::os::fd::OwnedFd;

use futures_lite::StreamExt;
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, Proxy};

const LOGIND: &str = "org.freedesktop.login1";
const RUTA: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const SESION: &str = "org.freedesktop.login1.Session";

pub struct Logind {
    conn: Connection,
    sesion: OwnedObjectPath,
}

impl Logind {
    pub async fn conectar() -> zbus::Result<Self> {
        let conn = crate::sistema::bus().await?;
        let auto = Proxy::new(
            &conn,
            LOGIND,
            "/org/freedesktop/login1/session/auto",
            SESION,
        )
        .await?;
        let id: String = auto.get_property("Id").await?;
        let sesion: OwnedObjectPath = manager(&conn).await?.call("GetSession", &(id,)).await?;
        Ok(Logind { conn, sesion })
    }

    async fn proxy_sesion(&self) -> zbus::Result<Proxy<'_>> {
        Proxy::new(&self.conn, LOGIND, self.sesion.as_ref(), SESION).await
    }

    pub async fn bloquear(&self) -> zbus::Result<()> {
        self.proxy_sesion()
            .await?
            .call::<_, _, ()>("Lock", &())
            .await
    }

    pub async fn suspender(&self) -> zbus::Result<()> {
        manager(&self.conn)
            .await?
            .call::<_, _, ()>("Suspend", &(false,))
            .await
    }

    pub async fn bloqueada(&self) -> zbus::Result<bool> {
        self.proxy_sesion().await?.get_property("LockedHint").await
    }

    pub async fn inhibir_suspension(&self) -> zbus::Result<OwnedFd> {
        let fd: zbus::zvariant::OwnedFd = manager(&self.conn)
            .await?
            .call(
                "Inhibit",
                &(
                    "sleep",
                    "morgan",
                    "Bloquear la sesión antes de suspender",
                    "delay",
                ),
            )
            .await?;
        Ok(fd.into())
    }

    pub async fn idle_inhibido(&self) -> zbus::Result<bool> {
        let v: String = manager(&self.conn)
            .await?
            .get_property("BlockInhibited")
            .await?;
        Ok(inhibe_idle(&v))
    }

    pub async fn al_lock(&self, f: impl Fn()) -> zbus::Result<()> {
        let p = self.proxy_sesion().await?;
        let mut s = p.receive_signal("Lock").await?;
        while s.next().await.is_some() {
            f();
        }
        Ok(())
    }

    pub async fn al_suspender(&self, f: impl Fn(bool)) -> zbus::Result<()> {
        let p = manager(&self.conn).await?;
        let mut s = p.receive_signal("PrepareForSleep").await?;
        while let Some(m) = s.next().await {
            if let Ok(antes) = m.body().deserialize::<bool>() {
                f(antes);
            }
        }
        Ok(())
    }

    pub async fn al_cambiar_bloqueo(&self, f: impl Fn()) -> zbus::Result<()> {
        let p = self.proxy_sesion().await?;
        let mut s = p.receive_property_changed::<bool>("LockedHint").await;
        while s.next().await.is_some() {
            f();
        }
        Ok(())
    }

    pub async fn al_cambiar_inhibidores(&self, f: impl Fn()) -> zbus::Result<()> {
        let p = manager(&self.conn).await?;
        let mut s = p.receive_property_changed::<String>("BlockInhibited").await;
        while s.next().await.is_some() {
            f();
        }
        Ok(())
    }
}

async fn manager(conn: &Connection) -> zbus::Result<Proxy<'static>> {
    Proxy::new(conn, LOGIND, RUTA, MANAGER).await
}

pub fn inhibe_idle(block: &str) -> bool {
    block.split(':').any(|w| w == "idle")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_inhibited() {
        assert!(!inhibe_idle(""));
        assert!(inhibe_idle("idle"));
        assert!(inhibe_idle("sleep:idle:handle-lid-switch"));
        assert!(!inhibe_idle("sleep:handle-power-key"));
        assert!(!inhibe_idle("idlex"));
    }
}
