//! `morgan-lock.service`, en el bus de sesión. `LockedHint` lo pone y lo saca `morgan-lock`, y
//! logind no lo limpia cuando el proceso que lo puso muere: un `morgan-lock` que termina sin
//! desbloquear (un crash sin relanzar, uno de prueba dentro de un compositor anidado) deja la
//! sesión marcada como bloqueada, y el idle apaga las pantallas a los 30 s sin entrada. Con la
//! unidad parada, la marca no es de nadie y se saca.

use futures_lite::StreamExt;
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, Proxy};

const SYSTEMD: &str = "org.freedesktop.systemd1";
const UNIDAD: &str = "morgan-lock.service";

pub struct Unidad {
    conn: Connection,
    ruta: OwnedObjectPath,
}

impl Unidad {
    /// systemd solo emite `PropertiesChanged` de las unidades si algún cliente llamó a
    /// `Subscribe` (org.freedesktop.systemd1(5)); la suscripción dura lo que la conexión.
    pub async fn conectar() -> zbus::Result<Self> {
        let conn = Connection::session().await?;
        let m = Proxy::new(
            &conn,
            SYSTEMD,
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
        )
        .await?;
        m.call::<_, _, ()>("Subscribe", &()).await?; // no-tr
        let ruta: OwnedObjectPath = m.call("LoadUnit", &(UNIDAD,)).await?;
        Ok(Unidad { conn, ruta })
    }

    async fn proxy(&self) -> zbus::Result<Proxy<'_>> {
        Proxy::new(
            &self.conn,
            SYSTEMD,
            self.ruta.as_ref(),
            "org.freedesktop.systemd1.Unit",
        )
        .await
    }

    /// `None` si la unidad no está instalada: entonces el bloqueo es de otro y la marca
    /// también.
    pub async fn parada(&self) -> zbus::Result<Option<bool>> {
        let p = self.proxy().await?;
        let carga: String = p.get_property("LoadState").await?;
        if carga != "loaded" {
            return Ok(None);
        }
        let estado: String = p.get_property("ActiveState").await?;
        Ok(Some(parada(&estado)))
    }

    pub async fn al_cambiar(&self, f: impl Fn()) -> zbus::Result<()> {
        let p = self.proxy().await?;
        let mut s = p.receive_property_changed::<String>("ActiveState").await;
        while s.next().await.is_some() {
            f();
        }
        Ok(())
    }
}

/// Mientras systemd la relanza tras un crash (`Restart=on-abnormal`) está en `activating`:
/// el bloqueo sigue y la marca también.
pub fn parada(estado: &str) -> bool {
    matches!(estado, "inactive" | "failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estados() {
        assert!(parada("inactive"));
        assert!(parada("failed"));
        assert!(!parada("active"));
        assert!(!parada("activating"));
        assert!(!parada("deactivating"));
        assert!(!parada("reloading"));
    }
}
