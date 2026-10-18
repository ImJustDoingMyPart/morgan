//! Por DDC se va con `--bus`: con `--display`, ddcutil enumera todo en cada llamada (12 veces
//! más lento). Una escritura tarda ~100 ms y dos ddcutil solapados pueden colgar el bus
//! minutos: por eso un solo hilo escribe, y lo que se pide mientras escribe se pisa (siempre
//! queda el último).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};

use gtk::glib;

use crate::osd::{Aviso, Cuerpo};

pub const PASO: f64 = 5.0;
const VCP_BRILLO: &str = "10";
pub const GLIFO: &str = "󰖙";

pub fn aviso(porcentaje: f64) -> Aviso {
    Aviso {
        icono: Some(GLIFO.into()),
        titulo: tr!("Brillo").into(),
        cuerpo: Cuerpo::Barra {
            fraccion: (porcentaje / 100.0).clamp(0.0, 1.0),
            porcentaje,
            apagada: false,
        },
        ms: morgan_ipc::OSD_MS,
    }
}

pub fn buses(detect: &str) -> Vec<u32> {
    let mut v = Vec::new();
    let mut valido = false;
    for l in detect.lines() {
        if !l.starts_with(' ') {
            valido = l.starts_with("Display ");
        } else if valido
            && let Some(n) = l.trim().strip_prefix("I2C bus:").and_then(|r| r.trim().strip_prefix("/dev/i2c-"))
            && let Ok(n) = n.parse()
        {
            v.push(n);
        }
    }
    v
}

pub fn vcp(salida: &str) -> Option<(u32, u32)> {
    let l = salida.lines().find(|l| l.starts_with("VCP "))?;
    let c: Vec<&str> = l.split_whitespace().collect();
    match c.as_slice() {
        [_, _, "C", actual, max] => Some((actual.parse().ok()?, max.parse().ok()?)).filter(|(_, m)| *m > 0),
        _ => None,
    }
}

fn backlight(raiz: &Path) -> Option<(String, u32, u32)> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(raiz).ok()?.flatten().map(|e| e.path()).collect();
    v.sort();
    v.into_iter().find_map(|d| {
        let leer = |n: &str| std::fs::read_to_string(d.join(n)).ok()?.trim().parse::<u32>().ok();
        let max = leer("max_brightness").filter(|m| *m > 0)?;
        Some((d.file_name()?.to_str()?.to_owned(), leer("brightness")?, max))
    })
}

fn ddcutil(args: &[&str]) -> Option<String> {
    let out = Command::new("ddcutil")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

enum Via {
    /// Todavía detectando: ddcutil tarda más de un segundo.
    Buscando,
    Ninguna,
    Backlight { nombre: String, max: u32 },
    Ddc(Arc<(Mutex<Option<f64>>, Condvar)>),
}

#[derive(Clone)]
pub struct Brillo {
    inner: Rc<Inner>,
}

struct Inner {
    via: RefCell<Via>,
    valor: Cell<f64>,
    oyentes: RefCell<Vec<Rc<dyn Fn()>>>,
}

impl Brillo {
    pub fn new() -> Brillo {
        let b = Brillo {
            inner: Rc::new(Inner {
                via: RefCell::new(Via::Buscando),
                valor: Cell::new(0.0),
                oyentes: RefCell::default(),
            }),
        };
        if let Some((nombre, actual, max)) = backlight(Path::new("/sys/class/backlight")) {
            b.inner.via.replace(Via::Backlight { nombre, max });
            b.inner.valor.set(f64::from(actual) * 100.0 / f64::from(max));
        } else {
            b.arrancar_ddc();
        }
        b
    }

    fn arrancar_ddc(&self) {
        let (tx, rx) = async_channel::unbounded::<Option<f64>>();
        let pedido = Arc::new((Mutex::new(None::<f64>), Condvar::new()));
        let p = pedido.clone();
        let hilo = std::thread::Builder::new().name("morgan-brillo".into()).spawn(move || {
            let monitores: Vec<(u32, u32)> = ddcutil(&["detect", "--brief"])
                .map(|d| buses(&d))
                .unwrap_or_default()
                .into_iter()
                .filter_map(|bus| {
                    let s = ddcutil(&["getvcp", VCP_BRILLO, "--bus", &bus.to_string(), "--brief"])?;
                    let (actual, max) = vcp(&s)?;
                    let _ = tx.send_blocking(Some(f64::from(actual) * 100.0 / f64::from(max)));
                    Some((bus, max))
                })
                .collect();
            if monitores.is_empty() {
                let _ = tx.send_blocking(None);
                return;
            }
            drop(tx);
            let (lugar, cv) = &*p;
            loop {
                let pct = {
                    let mut g = lugar.lock().unwrap_or_else(|e| e.into_inner());
                    while g.is_none() {
                        g = cv.wait(g).unwrap_or_else(|e| e.into_inner());
                    }
                    g.take().unwrap_or_default()
                };
                for (bus, max) in &monitores {
                    let v = (pct * f64::from(*max) / 100.0).round() as u32;
                    // --noverify: sin la lectura de confirmación, que es otra vuelta por el bus.
                    let _ = ddcutil(&["setvcp", VCP_BRILLO, &v.to_string(), "--bus", &bus.to_string(), "--noverify"]);
                }
            }
        });
        if let Err(e) = hilo {
            eprintln!("morgan: brillo: {e}");
            self.inner.via.replace(Via::Ninguna);
            return;
        }
        let yo = self.clone();
        glib::MainContext::default().spawn_local(async move {
            // El primer valor dice que hay DDC; uno vacío, que no.
            let mut primero = true;
            while let Ok(v) = rx.recv().await {
                match v {
                    Some(v) if primero => {
                        primero = false;
                        yo.inner.valor.set(v);
                        yo.inner.via.replace(Via::Ddc(pedido.clone()));
                        yo.avisar();
                    }
                    Some(_) => {}
                    None => {
                        yo.inner.via.replace(Via::Ninguna);
                        yo.avisar();
                    }
                }
            }
        });
    }

    pub fn disponible(&self) -> bool {
        matches!(*self.inner.via.borrow(), Via::Backlight { .. } | Via::Ddc(_))
    }

    pub fn valor(&self) -> f64 {
        self.inner.valor.get()
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

    pub fn poner(&self, pct: f64) -> Result<f64, String> {
        let pct = pct.clamp(0.0, 100.0).round();
        match &*self.inner.via.borrow() {
            Via::Buscando => return Err("todavía buscando los monitores".into()),
            Via::Ninguna => return Err("no hay backlight ni monitor con DDC/CI (ddcutil)".into()),
            Via::Backlight { nombre, max } => {
                let (nombre, valor) = (nombre.clone(), (pct * f64::from(*max) / 100.0).round() as u32);
                glib::MainContext::default().spawn_local(async move {
                    if let Err(e) = escribir_backlight(&nombre, valor).await {
                        eprintln!("morgan: brillo: {e}");
                    }
                });
            }
            Via::Ddc(p) => {
                let (lugar, cv) = &**p;
                *lugar.lock().unwrap_or_else(|e| e.into_inner()) = Some(pct);
                cv.notify_one();
            }
        }
        self.inner.valor.set(pct);
        self.avisar();
        Ok(pct)
    }

    pub fn mover(&self, subir: bool) -> Result<f64, String> {
        self.poner(self.valor() + if subir { PASO } else { -PASO })
    }
}

/// logind escribe el sysfs por el usuario de la sesión, sin root.
async fn escribir_backlight(nombre: &str, valor: u32) -> zbus::Result<()> {
    if std::env::var_os("MORGAN_SIN_LOGIND").is_some() {
        return Ok(());
    }
    let conn = crate::sistema::bus().await?;
    let sesion = zbus::Proxy::new(
        &conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1/session/auto",
        "org.freedesktop.login1.Session",
    )
    .await?;
    sesion.call::<_, _, ()>("SetBrightness", &("backlight", nombre, valor)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect() {
        // Una salida real: la TV no habla DDC.
        let s = "Invalid display\n   I2C bus:          /dev/i2c-7\n   DRM connector:    card1-HDMI-A-1\n\n\
                 Display 1\n   I2C bus:          /dev/i2c-11\n   DRM connector:    card1-DP-3\n\n\
                 Display 2\n   I2C bus:          /dev/i2c-3\n";
        assert_eq!(buses(s), [11, 3]);
        assert!(buses("").is_empty());
    }

    #[test]
    fn getvcp() {
        assert_eq!(vcp("VCP 10 C 100 100\n"), Some((100, 100)));
        assert_eq!(vcp("VCP 10 C 37 255"), Some((37, 255)));
        assert_eq!(vcp("VCP 10 ERR"), None);
        assert_eq!(vcp("VCP 10 C 0 0"), None);
    }

    #[test]
    fn sysfs() {
        let d = std::env::temp_dir().join(format!("morgan-backlight-{}", std::process::id()));
        let b = d.join("intel_backlight");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("brightness"), "480\n").unwrap();
        std::fs::write(b.join("max_brightness"), "960\n").unwrap();
        assert_eq!(backlight(&d), Some(("intel_backlight".into(), 480, 960)));
        std::fs::remove_dir_all(&d).unwrap();
        assert_eq!(backlight(&d), None);
    }
}
