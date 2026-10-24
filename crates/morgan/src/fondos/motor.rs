//! El punto único por donde pasa un cambio de fondo: la banda, la rotación y `morganctl
//! wallpaper`. Son dos etapas, cada una con su cola de un lugar en la que gana el último pedido:
//! mostrarlo (`awww img`, rápido) y lo que se ve después (la copia para el bloqueo y el tema).
//! Varios Enter seguidos no encolan un tema por cada uno.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};


use super::modelo;
use super::transicion::{self, Receta};
use crate::comando;
use crate::config;

/// Si awww no corre, se lo lanza como unidad y no como hijo: en el cgroup de morgan,
/// reiniciar morgan se llevaría el fondo.
const UNIDAD_AWWW: &str = "morgan-awww";

/// Tope de la espera hasta revisar la rotación: el reloj de glib es monótono y no cuenta una
/// suspensión, así que una espera larga podría pasarse de largo.
const TOPE_ROTACION: Duration = Duration::from_secs(15 * 60);

fn estado() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| glib::home_dir().join(".local/state"))
}

pub fn archivo_actual() -> PathBuf {
    estado().join("wallpaper")
}

fn copia_bloqueo() -> PathBuf {
    estado().join("wallpaper-lock.png")
}

pub fn actual() -> Option<PathBuf> {
    let texto = std::fs::read_to_string(archivo_actual()).ok()?;
    texto.lines().next().filter(|l| !l.is_empty()).map(PathBuf::from)
}

pub fn salida_nueva(linea: &str) -> Option<&str> {
    let datos = linea.strip_prefix("monitoraddedv2>>")?;
    datos.split(',').nth(1).filter(|n| !n.is_empty())
}

/// awww le crea su capa a una pantalla que se conecta pero no le pinta nada, y su caché es
/// por salida. Sin transición (un fundido sobre una pantalla que recién prende parece un
/// parpadeo) y un segundo después: antes, awww todavía no creó la capa.
pub fn pintar_salida(salida: String) {
    glib::timeout_add_local_once(std::time::Duration::from_secs(1), move || {
        let Some(img) = actual().filter(|p| p.is_file()) else { return };
        glib::MainContext::default().spawn_local(async move {
            let img = img.to_string_lossy();
            let args = ["awww", "img", "-o", &salida, &img, "--transition-type", "none"];
            if let Err(e) = comando::salida(&args).await {
                eprintln!("morgan: fondos: {salida}: {e}");
            }
        });
    });
}

#[derive(Clone)]
pub struct Motor {
    inner: Rc<Inner>,
}

struct Inner {
    config: RefCell<config::Fondos>,
    para_mostrar: RefCell<Option<PathBuf>>,
    mostrando: Cell<bool>,
    para_despues: RefCell<Option<PathBuf>>,
    despues: Cell<bool>,
    receta: Cell<Option<Receta>>,
    /// Sube con cada rearmado de la rotación: un timeout de antes se descarta.
    turno: Cell<u64>,
}

impl Motor {
    pub fn new(c: &config::Fondos) -> Self {
        let m = Motor {
            inner: Rc::new(Inner {
                config: RefCell::new(c.clone()),
                para_mostrar: RefCell::default(),
                mostrando: Cell::new(false),
                para_despues: RefCell::default(),
                despues: Cell::new(false),
                receta: Cell::new(None),
                turno: Cell::new(0),
            }),
        };
        m.armar_rotacion();
        m
    }

    pub fn config(&self) -> config::Fondos {
        self.inner.config.borrow().clone()
    }

    pub fn aplicar_config(&self, c: &config::Fondos) {
        if self.inner.config.replace(c.clone()) != *c {
            self.armar_rotacion();
        }
    }

    pub fn aplicar(&self, img: PathBuf) {
        if let Err(e) = anotar(&img) {
            eprintln!("morgan: fondos: {e}");
        }
        // Elegir uno a mano también reinicia la cuenta de la rotación.
        self.armar_rotacion();
        self.inner.para_mostrar.replace(Some(img));
        if !self.inner.mostrando.replace(true) {
            let yo = self.clone();
            glib::MainContext::default().spawn_local(async move { yo.bucle_mostrar().await });
        }
    }

    async fn bucle_mostrar(&self) {
        let i = &self.inner;
        while let Some(img) = i.para_mostrar.take() {
            self.mostrar(&img).await;
            i.para_despues.replace(Some(img));
            if !i.despues.replace(true) {
                let yo = self.clone();
                glib::MainContext::default().spawn_local(async move { yo.bucle_despues().await });
            }
        }
        i.mostrando.set(false);
    }

    async fn mostrar(&self, img: &Path) {
        if !awww_listo().await {
            arrancar_awww().await;
        }
        let fija = self.inner.config.borrow().transicion.clone();
        let transicion = if fija.is_empty() {
            let (receta, args) = transicion::sortear(self.inner.receta.get(), &mut |n| {
                glib::random_int_range(0, n as i32) as u32
            });
            self.inner.receta.set(Some(receta));
            args
        } else {
            transicion::fija(&fija)
        };
        let img = img.to_string_lossy();
        let mut args: Vec<&str> = vec!["awww", "img", &img];
        args.extend(transicion.iter().map(String::as_str));
        if let Err(e) = comando::salida(&args).await {
            eprintln!("morgan: fondos: {e}");
        }
    }

    async fn bucle_despues(&self) {
        let i = &self.inner;
        while let Some(img) = i.para_despues.take() {
            futures_lite::future::zip(copiar_para_bloqueo(&img), tema(img.clone())).await;
        }
        i.despues.set(false);
    }

    pub async fn rotar(&self) -> Result<(), String> {
        let c = self.config();
        let (raiz, excluir) = (c.raiz(), c.excluir.clone());
        let todos = gio::spawn_blocking(move || modelo::listar(&raiz, &excluir))
            .await
            .map_err(|_| "fondos: el hilo se cayó".to_owned())?;
        let azar = glib::random_int_range(0, i32::MAX) as u32;
        let elegido = modelo::otro(&todos, actual().as_deref(), azar)
            .ok_or_else(|| format!("no hay otro fondo en {}", c.raiz().display()))?;
        self.aplicar(elegido.clone());
        Ok(())
    }

    fn armar_rotacion(&self) {
        let i = &self.inner;
        i.turno.set(i.turno.get() + 1);
        let horas = i.config.borrow().rotar;
        if horas == 0 {
            return;
        }
        let plazo = Duration::from_secs(u64::from(horas) * 3600);
        let pasado = std::fs::metadata(archivo_actual())
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            // Sin fondo anotado, el plazo empieza ahora.
            .unwrap_or_default();
        let falta = plazo.saturating_sub(pasado);
        let turno = i.turno.get();
        let yo = self.clone();
        let espera = falta.min(TOPE_ROTACION).max(Duration::from_secs(1));
        glib::timeout_add_local_once(espera, move || {
            if yo.inner.turno.get() != turno {
                return;
            }
            if espera < falta {
                yo.armar_rotacion();
                return;
            }
            let yo2 = yo.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = yo2.rotar().await {
                    eprintln!("morgan: fondos: {e}");
                    // `aplicar` rearma; si no hubo cambio, se vuelve a probar en un plazo.
                    yo2.armar_rotacion();
                }
            });
        });
    }
}

/// Escribe y renombra: quien lee nunca ve el archivo a medias.
fn anotar(img: &Path) -> Result<(), String> {
    let ruta = archivo_actual();
    let tmp = ruta.with_extension("tmp");
    let r = (|| {
        if let Some(d) = ruta.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&tmp, format!("{}\n", img.display()))?;
        std::fs::rename(&tmp, &ruta)
    })();
    r.map_err(|e| format!("{}: {e}", ruta.display()))
}

async fn awww_listo() -> bool {
    comando::salida(&["awww", "query"]).await.is_ok()
}

/// El daemon tarda en tomar la capa: sin la espera, el primer `img` se pierde.
async fn arrancar_awww() {
    let unidad = format!("--unit={UNIDAD_AWWW}");
    let lanzar = comando::salida(&[
        "systemd-run",
        "--user",
        "--quiet",
        "--collect",
        &unidad,
        "-E",
        "WAYLAND_DISPLAY",
        "--",
        "awww-daemon",
    ])
    .await;
    if let Err(e) = lanzar {
        eprintln!("morgan: fondos: {e}");
        return;
    }
    for _ in 0..20 {
        if awww_listo().await {
            return;
        }
        glib::timeout_future(Duration::from_millis(100)).await;
    }
}

/// Ya en PNG y al tamaño de la pantalla: el bloqueo aparece al instante en vez de decodificar
/// un 4K en el momento de la tecla. A un temporal que después se renombra.
async fn copiar_para_bloqueo(img: &Path) {
    let Some((w, h)) = crate::pixeles::salida_grande() else {
        return;
    };
    let destino = copia_bloqueo();
    let tmp = destino.with_extension("tmp.png");
    let salida = format!("{}[strip]", tmp.display());
    let r = comando::salida(&[
        "vipsthumbnail",
        &img.to_string_lossy(),
        "--size",
        &format!("{w}x{h}"),
        // Escala hasta cubrir y recorta centrado.
        "--smartcrop=centre",
        "-o",
        &salida,
    ])
    .await;
    match r.and_then(|_| std::fs::rename(&tmp, &destino).map_err(|e| e.to_string())) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("morgan: fondos: copia para el bloqueo: {e}");
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

async fn tema(img: PathBuf) {
    let r = gio::spawn_blocking(move || {
        morgan_temas::wallpaper(&morgan_temas::Rutas::del_entorno(), Some(&img))
    })
    .await;
    match r {
        Ok(Ok(informe)) => {
            for a in informe.map(|i| i.avisos).unwrap_or_default() {
                eprintln!("morgan: tema: {a}");
            }
        }
        Ok(Err(e)) => eprintln!("morgan: tema: {e}"),
        Err(_) => eprintln!("morgan: tema: el hilo se cayó"),
    }
}

/// En otro hilo: espera a los procesos.
pub fn crear_miniaturas(cache: &Path, imgs: &[PathBuf]) {
    let n = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    for tanda in imgs.chunks(n) {
        let hijos: Vec<_> = tanda
            .iter()
            .filter_map(|img| {
                let m = modelo::miniatura(cache, img);
                std::fs::create_dir_all(m.parent()?).ok()?;
                std::process::Command::new("vipsthumbnail")
                    .arg(img)
                    .args(["--size", LADO_MINIATURA, "--smartcrop=centre", "-o"])
                    .arg(&m)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .ok()
            })
            .collect();
        for mut h in hijos {
            let _ = h.wait();
        }
    }
}

/// Cuadradas por estética: el recorte puede tener cualquier proporción. Si cambia, cambia
/// también la carpeta de la caché (`cache_miniaturas`).
pub const LADO_MINIATURA: &str = "256x256";

pub fn cache_miniaturas() -> PathBuf {
    glib::user_cache_dir().join("morgan/fondos").join(LADO_MINIATURA)
}

#[cfg(test)]
mod tests_salida {
    use super::*;

    #[test]
    fn salida_que_se_conecta() {
        assert_eq!(salida_nueva("monitoraddedv2>>3,HDMI-A-1,LG TV"), Some("HDMI-A-1"));
        assert_eq!(salida_nueva("monitoraddedv2>>3,"), None);
        assert_eq!(salida_nueva("monitorremovedv2>>3,HDMI-A-1,x"), None);
    }
}
