//! El estado vive acá, en el hilo de GTK: los toasts y el centro de control son vistas.

pub mod captura;
pub mod markup;
pub mod modelo;
pub mod servidor;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;


use crate::config;
use crate::hyprland::Hyprland;
use modelo::{Destino, Guardado, Imagen, Notificacion, Pixeles};
use servidor::{Emisor, Pedido, Razon};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cambio {
    Llego {
        id: u32,
        flota: bool,
    },
    Cerrada(u32),
    NoMolestar(bool),
}

type Oyente = Rc<dyn Fn(Cambio)>;

#[derive(Clone)]
pub struct Notificaciones {
    inner: Rc<Inner>,
}

struct Inner {
    lista: RefCell<Vec<Notificacion>>,
    no_molestar: Cell<bool>,
    ultimo_id: Cell<u32>,
    en_juego: RefCell<Vec<String>>,
    config: RefCell<config::Notificaciones>,
    resumen_juego: Cell<bool>,
    hyprland: Hyprland,
    dir: Option<PathBuf>,
    emisor: RefCell<Option<Emisor>>,
    oyentes: RefCell<Vec<Oyente>>,
    guardar_pendiente: Cell<bool>,
}

impl Notificaciones {
    pub fn new(config: &config::Config, hyprland: Hyprland) -> Self {
        let dir = dir_from(|k| std::env::var_os(k));
        let guardado = dir.as_deref().map(cargar).unwrap_or_default();
        let inner = Rc::new(Inner {
            lista: RefCell::new(guardado.lista),
            no_molestar: Cell::new(guardado.no_molestar),
            ultimo_id: Cell::new(guardado.ultimo_id),
            en_juego: RefCell::default(),
            config: RefCell::new(config.notificaciones.clone()),
            resumen_juego: Cell::new(config.juego.resumen),
            hyprland: hyprland.clone(),
            dir,
            emisor: RefCell::default(),
            oyentes: RefCell::default(),
            guardar_pendiente: Cell::new(false),
        });
        let n = Notificaciones { inner };
        n.podar();

        let weak = Rc::downgrade(&n.inner);
        hyprland.al_cambiar(move |juego| {
            if !juego && let Some(inner) = weak.upgrade() {
                Notificaciones { inner }.salir_del_juego();
            }
        });

        let (tx, rx) = async_channel::unbounded();
        let yo = n.clone();
        glib::MainContext::default().spawn_local(async move {
            match servidor::levantar(tx).await {
                Ok(conn) => {
                    yo.inner.emisor.replace(Some(Emisor::new(conn)));
                }
                Err(e) => {
                    eprintln!(
                        "morgan: notificaciones: no se tomó {}: {e}",
                        servidor::NOMBRE
                    );
                    return;
                }
            }
            while let Ok(p) = rx.recv().await {
                match p {
                    Pedido::Notificar { n, id } => {
                        let _ = id.send(yo.recibir(*n)).await;
                    }
                    Pedido::Cerrar(id) => yo.cerrar(id, Razon::Pedido),
                }
            }
        });
        n
    }

    pub fn al_cambiar(&self, f: impl Fn(Cambio) + 'static) {
        self.inner.oyentes.borrow_mut().push(Rc::new(f));
    }

    pub fn lista(&self) -> std::cell::Ref<'_, Vec<Notificacion>> {
        self.inner.lista.borrow()
    }

    pub fn get(&self, id: u32) -> Option<Notificacion> {
        self.inner
            .lista
            .borrow()
            .iter()
            .find(|n| n.id == id)
            .cloned()
    }

    pub fn no_molestar(&self) -> bool {
        self.inner.no_molestar.get()
    }

    pub fn set_no_molestar(&self, v: bool) {
        if self.inner.no_molestar.replace(v) != v {
            self.avisar(Cambio::NoMolestar(v));
            self.guardar();
        }
    }

    /// Pasa por el mismo camino que las de D-Bus.
    pub fn interna(&self, n: Notificacion) -> u32 {
        self.recibir(n)
    }

    pub fn captura(&self, ruta: &Path) -> Result<(), String> {
        if !ruta.is_absolute() || !ruta.is_file() {
            return Err(format!("no hay una captura en {}", ruta.display()));
        }
        self.interna(captura::notificacion(ruta, servidor::ahora()));
        Ok(())
    }

    fn recibir(&self, mut n: Notificacion) -> u32 {
        let reemplaza = n.id != 0 && self.inner.lista.borrow().iter().any(|x| x.id == n.id);
        if !reemplaza {
            n.id = self.proximo_id();
        }
        if let Some(Imagen::Datos(p)) = &n.imagen {
            n.imagen = self.guardar_png(n.id, p).map(Imagen::Archivo);
        }
        escritorio(&mut n);
        if self.inner.config.borrow().una_sola(&n.app) {
            let viejas = modelo::desplazadas(&self.inner.lista.borrow(), &n);
            for v in viejas {
                self.cerrar(v, Razon::Vencio);
            }
        }
        let silenciada = self.inner.config.borrow().silenciada(&n.app);
        let destino = modelo::destino(
            &n,
            self.no_molestar(),
            self.inner.hyprland.juego_en_foco(),
            silenciada,
        );
        if destino == Destino::Juego {
            self.inner.en_juego.borrow_mut().push(n.app.clone());
        }
        if let Some((archivo, s)) = modelo::sonido(&n, destino, reemplaza, &self.inner.config.borrow()) {
            if archivo {
                crate::comando::lanzar(&["canberra-gtk-play", "-f", &s, "-d", "morgan"]);
            } else {
                crate::sonidos::sonar(&s, None);
            }
        }
        let id = n.id;
        {
            let mut lista = self.inner.lista.borrow_mut();
            // Un reemplazo sube al final: es lo más nuevo.
            lista.retain(|x| x.id != id);
            lista.push(n);
        }
        // Una transitoria que no flota no tiene dónde vivir.
        let transitoria = self.get(id).is_some_and(|n| n.transitoria);
        if transitoria && destino != Destino::Flota {
            self.cerrar(id, Razon::Vencio);
            return id;
        }
        self.avisar(Cambio::Llego {
            id,
            flota: destino == Destino::Flota,
        });
        self.guardar();
        id
    }

    fn proximo_id(&self) -> u32 {
        // 0 no es un id válido (la spec lo usa para "ninguna").
        let id = self.inner.ultimo_id.get().checked_add(1).unwrap_or(1);
        self.inner.ultimo_id.set(id);
        id
    }

    pub fn cerrar(&self, id: u32, razon: Razon) {
        let antes = self.inner.lista.borrow().len();
        self.inner.lista.borrow_mut().retain(|n| n.id != id);
        if self.inner.lista.borrow().len() == antes {
            return;
        }
        self.senal_cerrada(id, razon);
        self.avisar(Cambio::Cerrada(id));
        self.guardar();
    }

    pub fn vencio(&self, id: u32) {
        if self.get(id).is_some_and(|n| n.transitoria) {
            self.cerrar(id, Razon::Vencio);
        }
    }

    pub fn accion(&self, id: u32, clave: &str) {
        let Some(n) = self.get(id) else {
            return;
        };
        if n.acciones.iter().any(|a| a.clave == clave) {
            if captura::es_captura(&n) {
                captura::accion(&n, clave);
            } else if let Some(e) = self.inner.emisor.borrow().clone() {
                let clave = clave.to_owned();
                glib::MainContext::default().spawn_local(async move {
                    e.accion(id, &clave).await;
                });
            }
            if n.residente {
                return;
            }
        }
        self.cerrar(id, Razon::Usuario);
    }

    pub fn limpiar(&self) {
        let ids: Vec<u32> = self.inner.lista.borrow().iter().map(|n| n.id).collect();
        for id in ids {
            self.cerrar(id, Razon::Usuario);
        }
    }

    fn salir_del_juego(&self) {
        let apps = std::mem::take(&mut *self.inner.en_juego.borrow_mut());
        if !self.inner.resumen_juego.get() || self.no_molestar() {
            return;
        }
        if let Some((titulo, cuerpo)) = modelo::resumen_juego(&apps) {
            self.interna(Notificacion {
                id: 0,
                app: "morgan".into(),
                entrada: None,
                nombre_app: None,
                icono_app: None,
                resumen: titulo,
                cuerpo: glib::markup_escape_text(&cuerpo).to_string(),
                acciones: Vec::new(),
                urgencia: modelo::Urgencia::Normal,
                categoria: None,
                imagen: Some(Imagen::Nombre("input-gaming-symbolic".into())),
                residente: false,
                transitoria: true,
                progreso: None,
                vence_ms: -1,
                llegada: servidor::ahora(),
                sonido: Default::default(),
            });
        }
    }

    fn senal_cerrada(&self, id: u32, razon: Razon) {
        if let Some(e) = self.inner.emisor.borrow().clone() {
            glib::MainContext::default().spawn_local(async move {
                e.cerrada(id, razon).await;
            });
        }
    }

    fn avisar(&self, c: Cambio) {
        // Copia: un oyente puede suscribir a otro mientras tanto.
        let oyentes = self.inner.oyentes.borrow().clone();
        for f in oyentes {
            f(c);
        }
    }

    pub fn aplicar_config(&self, c: &config::Config) {
        self.inner.config.replace(c.notificaciones.clone());
        self.inner.resumen_juego.set(c.juego.resumen);
        self.podar();
    }

    pub fn apps(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .inner
            .lista
            .borrow()
            .iter()
            .map(|n| n.app.clone())
            .filter(|a| !a.is_empty())
            .collect();
        v.sort_by_key(|a| a.to_lowercase());
        v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        v
    }

    fn podar(&self) {
        let vencidas = modelo::vencidas(
            &self.inner.lista.borrow(),
            servidor::ahora(),
            self.inner.config.borrow().retencion_dias,
        );
        if !vencidas.is_empty() {
            self.inner
                .lista
                .borrow_mut()
                .retain(|n| !vencidas.contains(&n.id));
            self.guardar();
        }
    }

    fn guardar(&self) {
        if self.inner.guardar_pendiente.replace(true) {
            return;
        }
        let weak = Rc::downgrade(&self.inner);
        glib::idle_add_local_once(move || {
            if let Some(inner) = weak.upgrade() {
                inner.guardar_pendiente.set(false);
                Notificaciones { inner }.escribir();
            }
        });
    }

    fn escribir(&self) {
        let Some(dir) = &self.inner.dir else {
            return;
        };
        let guardado = Guardado {
            no_molestar: self.no_molestar(),
            ultimo_id: self.inner.ultimo_id.get(),
            lista: self
                .inner
                .lista
                .borrow()
                .iter()
                .filter(|n| !n.transitoria)
                .cloned()
                .collect(),
        };
        if let Err(e) = escribir(dir, &guardado) {
            eprintln!("morgan: notificaciones: no se pudo guardar el historial: {e}");
        }
        // Las imágenes que ya no usa nadie (incluidas las de transitorias que se fueron).
        let usadas: HashSet<PathBuf> = self
            .inner
            .lista
            .borrow()
            .iter()
            .filter_map(|n| match &n.imagen {
                Some(Imagen::Archivo(p)) => Some(p.clone()),
                _ => None,
            })
            .collect();
        if let Ok(archivos) = std::fs::read_dir(dir.join("imagenes")) {
            for a in archivos.flatten() {
                if !usadas.contains(&a.path()) {
                    let _ = std::fs::remove_file(a.path());
                }
            }
        }
    }

    fn guardar_png(&self, id: u32, p: &Pixeles) -> Option<PathBuf> {
        let dir = self.inner.dir.as_ref()?.join("imagenes");
        std::fs::create_dir_all(&dir).ok()?;
        let ruta = dir.join(format!("{id}.png"));
        let (ancho, alto, fila) = (p.ancho as usize, p.alto as usize, p.fila as usize);
        let canales = if p.alfa { 4 } else { 3 };
        // Sin el relleno del final de cada fila.
        let mut compacto = Vec::with_capacity(ancho * alto * canales);
        for y in 0..alto {
            let Some(f) = p.datos.get(y * fila..y * fila + ancho * canales) else {
                eprintln!("morgan: notificaciones: imagen incompleta");
                return None;
            };
            compacto.extend_from_slice(f);
        }
        let color = if p.alfa {
            image::ExtendedColorType::Rgba8
        } else {
            image::ExtendedColorType::Rgb8
        };
        match image::save_buffer(&ruta, &compacto, p.ancho as u32, p.alto as u32, color) {
            Ok(()) => Some(ruta),
            Err(e) => {
                eprintln!("morgan: notificaciones: no se pudo guardar la imagen: {e}");
                None
            }
        }
    }
}

fn escritorio(n: &mut Notificacion) {
    use gio::prelude::{AppInfoExt, IconExt};
    let Some(info) = n.entradas().iter().find_map(|e| {
        let id = format!("{e}.desktop");
        gio_unix::DesktopAppInfo::new(&id)
    }) else {
        return;
    };
    n.nombre_app = Some(info.display_name().to_string()).filter(|s| !s.is_empty());
    let icono = info
        .icon()
        .and_then(|i| IconExt::to_string(&i))
        .and_then(|s| Imagen::de_texto(s.as_str()));
    if n.imagen.is_none() {
        n.imagen = icono;
    } else if n.icono_app.is_none() {
        n.icono_app = icono;
    }
}

pub fn dir_from(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    env("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            env("HOME")
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(".local").join("state"))
        })
        .map(|p| p.join("morgan"))
}

fn archivo(dir: &Path) -> PathBuf {
    dir.join("notificaciones.json")
}

/// Un historial ilegible no impide arrancar: se empieza vacío y se guarda aparte para no
/// pisarlo.
fn cargar(dir: &Path) -> Guardado {
    let ruta = archivo(dir);
    let texto = match std::fs::read_to_string(&ruta) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Guardado::default(),
        Err(e) => {
            eprintln!("morgan: notificaciones: {}: {e}", ruta.display());
            return Guardado::default();
        }
    };
    match serde_json::from_str(&texto) {
        Ok(g) => g,
        Err(e) => {
            eprintln!(
                "morgan: notificaciones: {} ilegible ({e}); queda en .roto",
                ruta.display()
            );
            let _ = std::fs::rename(&ruta, ruta.with_extension("json.roto"));
            Guardado::default()
        }
    }
}

/// A un temporal y rename: un corte no deja el historial a medias.
fn escribir(dir: &Path, g: &Guardado) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let ruta = archivo(dir);
    let tmp = ruta.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(g)?)?;
    std::fs::rename(tmp, ruta)
}

/// Título y cuerpo sin markup, para copiar.
pub fn texto_plano(n: &Notificacion) -> String {
    let cuerpo = markup::plano(&n.cuerpo);
    [n.titulo(), cuerpo.as_str()]
        .into_iter()
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_de_estado() {
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |k: &str| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.into())
        };
        assert_eq!(
            dir_from(env(&[("XDG_STATE_HOME", "/s"), ("HOME", "/h")])),
            Some(PathBuf::from("/s/morgan"))
        );
        assert_eq!(
            dir_from(env(&[("XDG_STATE_HOME", "rel"), ("HOME", "/h")])),
            Some(PathBuf::from("/h/.local/state/morgan"))
        );
        assert_eq!(dir_from(env(&[])), None);
    }

    #[test]
    fn guardar_y_cargar() {
        let dir = std::env::temp_dir().join(format!("morgan-test-{}", std::process::id()));
        let g = Guardado {
            no_molestar: true,
            ultimo_id: 41,
            lista: vec![modelo::prueba(41, "a")],
        };
        escribir(&dir, &g).unwrap();
        let g2 = cargar(&dir);
        assert!(g2.no_molestar);
        assert_eq!(g2.ultimo_id, 41);
        assert_eq!(g2.lista, g.lista);
        // Uno roto se aparta y se arranca vacío.
        std::fs::write(archivo(&dir), "{roto").unwrap();
        assert!(cargar(&dir).lista.is_empty());
        assert!(dir.join("notificaciones.json.roto").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
