//! `ActiveProfile` incluye lo que sostenga otro proceso con un hold.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::glib;
use zbus::zvariant::OwnedValue;

use crate::sistema;

const PPD: &str = "org.freedesktop.UPower.PowerProfiles";
const RUTA: &str = "/org/freedesktop/UPower/PowerProfiles";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Estado {
    pub activo: Option<String>,
    pub perfiles: Vec<String>,
}

pub fn etiqueta(perfil: &str) -> (&'static str, &str) {
    match perfil {
        "power-saver" => ("󰾆", "Ahorro"),
        "balanced" => ("󰾅", "Equilibrado"),
        "performance" => ("󰓅", "Rendimiento"),
        otro => ("󰾅", otro),
    }
}

#[derive(Clone, Default)]
pub struct Energia {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    estado: RefCell<Estado>,
    oyentes: RefCell<Vec<Rc<dyn Fn()>>>,
    vigia: RefCell<Option<glib::JoinHandle<()>>>,
}

impl Energia {
    pub fn estado(&self) -> Estado {
        self.inner.estado.borrow().clone()
    }

    pub fn al_cambiar(&self, f: impl Fn() + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    pub fn abrir(&self) {
        let yo = self.clone();
        let h = sistema::vigilar(
            sistema::propiedades(PPD, RUTA),
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
        if let Some(h) = self.inner.vigia.take() {
            h.abort();
        }
    }

    async fn releer(&self) {
        let e = match leer().await {
            Ok(e) => e,
            Err(e) => {
                eprintln!("morgan: energía: {e}");
                Estado::default()
            }
        };
        if *self.inner.estado.borrow() != e {
            self.inner.estado.replace(e);
            let oyentes = self.inner.oyentes.borrow().clone();
            for f in oyentes {
                f();
            }
        }
    }

    pub fn elegir(&self, perfil: &str) {
        let perfil = perfil.to_owned();
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let r = match sistema::proxy(PPD, RUTA, PPD).await {
                Ok(p) => p
                    .set_property("ActiveProfile", perfil.as_str())
                    .await
                    .map_err(zbus::Error::from),
                Err(e) => Err(e),
            };
            if let Err(e) = r {
                eprintln!("morgan: energía: {perfil}: {e}");
            }
            yo.releer().await;
        });
    }
}

async fn leer() -> zbus::Result<Estado> {
    let p = sistema::proxy(PPD, RUTA, PPD).await?;
    let activo = p.get_property::<String>("ActiveProfile").await?;
    let perfiles = p
        .get_property::<Vec<HashMap<String, OwnedValue>>>("Profiles")
        .await?
        .iter()
        .filter_map(|m| {
            m.get("Profile")
                .and_then(|v| <&str>::try_from(&**v).ok())
                .map(str::to_owned)
        })
        .collect();
    Ok(Estado {
        activo: Some(activo),
        perfiles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etiquetas() {
        assert_eq!(etiqueta("performance"), ("󰓅", "Rendimiento"));
        assert_eq!(etiqueta("power-saver").1, "Ahorro");
        assert_eq!(etiqueta("raro"), ("󰾅", "raro"));
    }
}
