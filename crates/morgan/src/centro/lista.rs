//! Las notificaciones como las ve el centro: los grupos en el orden de swaync y cada tarjeta con
//! sus imágenes ya en píxeles. Las imágenes se guardan entre aperturas (rehacerlas con decenas
//! de avisos trabaría el hilo de GTK); una vista previa se decodifica ya achicada, fuera del hilo,
//! y cuando llega se vuelve a mandar la lista.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use gtk::{gdk_pixbuf, gio, glib};
use gtk::prelude::*;
use morgan_motor::vello::peniko::ImageData;

use super::dibujo::avisos::tam_previa;
use super::vista::{Aviso, Grupo};
use crate::notificaciones::modelo::{self, Imagen, Notificacion};
use crate::notificaciones::{Notificaciones, captura};
use crate::pixeles::{self, Pintor};

/// La columna de la imagen y el ícono chico de la app.
const IMAGEN: i32 = 36;
const CHICO: i32 = 16;

#[derive(Clone, PartialEq, Eq, Hash)]
enum ClaveImagen {
    Nombre(String, i32),
    Archivo(PathBuf, i32),
    Datos(u32, i32),
}

pub struct Lista {
    notificaciones: Notificaciones,
    pintor: RefCell<Pintor>,
    imagenes: RefCell<HashMap<ClaveImagen, Option<ImageData>>>,
    previas: RefCell<HashMap<PathBuf, Option<ImageData>>>,
    pendientes: RefCell<HashSet<PathBuf>>,
    /// El color de los íconos simbólicos: el del texto.
    color: Cell<[f32; 4]>,
    /// Sube cuando llega una imagen tarde: las tarjetas se rearman.
    version: Cell<u64>,
    /// Para volver a mandar la lista cuando llega una vista previa.
    al_llegar: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Lista {
    pub fn new(notificaciones: &Notificaciones, color: [f32; 4]) -> Rc<Self> {
        Rc::new(Lista {
            notificaciones: notificaciones.clone(),
            pintor: RefCell::default(),
            imagenes: RefCell::default(),
            previas: RefCell::default(),
            pendientes: RefCell::default(),
            color: Cell::new(color),
            version: Cell::new(0),
            al_llegar: RefCell::default(),
        })
    }

    pub fn al_llegar(&self, f: impl Fn() + 'static) {
        self.al_llegar.replace(Some(Rc::new(f)));
    }

    /// Otro tema: los íconos simbólicos cambian de color.
    pub fn poner_color(&self, c: [f32; 4]) {
        if self.color.replace(c) != c {
            self.imagenes.borrow_mut().clear();
            self.version.set(self.version.get() + 1);
        }
    }

    pub fn grupos(self: &Rc<Self>) -> Vec<Grupo> {
        let lista = self.notificaciones.lista().clone();
        let escala = pixeles::escala();
        let mut usadas = HashSet::new();
        let grupos = modelo::grupos(&lista)
            .into_iter()
            .filter_map(|ids| {
                let ns: Vec<&Notificacion> = ids
                    .iter()
                    .filter_map(|id| lista.iter().find(|n| n.id == *id))
                    .collect();
                let frente = ns.first()?;
                Some(Grupo {
                    clave: frente.grupo().to_owned(),
                    nombre: frente.nombre_visible().to_owned(),
                    avisos: ns
                        .iter()
                        .map(|n| self.aviso(n, escala, &mut usadas))
                        .collect(),
                })
            })
            .collect();
        self.imagenes.borrow_mut().retain(|k, _| usadas.contains(k));
        grupos
    }

    fn aviso(self: &Rc<Self>, n: &Notificacion, escala: i32, usadas: &mut HashSet<ClaveImagen>) -> Aviso {
        // Una captura o una foto va abajo, a lo ancho; en su columna queda el ícono de la app.
        let previa = vista_previa(n.imagen.as_ref());
        let (columna, chico) = match &previa {
            Some(_) => (
                Some(
                    n.icono_app
                        .clone()
                        .unwrap_or_else(|| Imagen::Nombre("image-x-generic".into())),
                ),
                None,
            ),
            None => (n.imagen.clone(), n.icono_app.clone()),
        };
        let imagen = columna
            .as_ref()
            .and_then(|i| self.imagen(n.id, i, IMAGEN, escala, usadas));
        let chico = chico
            .filter(|_| imagen.is_some())
            .and_then(|i| self.imagen(n.id, &i, CHICO, escala, usadas));
        let previa = previa.and_then(|(ruta, w, h)| {
            let img = self.previa(&ruta, w, h, escala)?;
            Some((img, w, h))
        });
        let es_captura = captura::es_captura(n);
        let acciones = n
            .botones()
            .map(|a| {
                let glifo = es_captura
                    .then(|| captura::glifo(&a.clave))
                    .flatten()
                    .unwrap_or("󰅂");
                (a.clave.clone(), a.etiqueta.clone(), glifo)
            })
            .collect();
        Aviso {
            id: n.id,
            titulo: n.titulo().to_owned(),
            cuerpo: n.cuerpo.clone(),
            urgencia: n.urgencia,
            llegada: n.llegada,
            imagen,
            chico,
            previa,
            progreso: n.progreso,
            acciones,
            version: self.version.get(),
        }
    }

    fn imagen(
        &self,
        id: u32,
        img: &Imagen,
        tam: i32,
        escala: i32,
        usadas: &mut HashSet<ClaveImagen>,
    ) -> Option<ImageData> {
        let lado = tam * escala;
        let clave = match img {
            Imagen::Nombre(n) => ClaveImagen::Nombre(n.clone(), lado),
            Imagen::Archivo(p) => ClaveImagen::Archivo(p.clone(), lado),
            Imagen::Datos(_) => ClaveImagen::Datos(id, lado),
        };
        usadas.insert(clave.clone());
        if let Some(i) = self.imagenes.borrow().get(&clave) {
            return i.clone();
        }
        let i = match img {
            Imagen::Nombre(n) => {
                let c = pixeles::color(self.color.get());
                self.pintor.borrow_mut().icono(n, tam, escala, &c)
            }
            Imagen::Archivo(p) => pixeles::archivo(p, lado),
            Imagen::Datos(px) => de_datos(px, lado),
        };
        self.imagenes.borrow_mut().insert(clave, i.clone());
        i
    }

    /// La vista previa, si ya está; si no, se pide (decodificada ya achicada, fuera del hilo).
    fn previa(self: &Rc<Self>, ruta: &std::path::Path, w: u32, h: u32, escala: i32) -> Option<ImageData> {
        if let Some(i) = self.previas.borrow().get(ruta) {
            return i.clone();
        }
        if !self.pendientes.borrow_mut().insert(ruta.to_owned()) {
            return None;
        }
        let (pw, ph) = tam_previa(w, h);
        let (pw, ph) = (pw as i32 * escala, ph as i32 * escala);
        let archivo = gio::File::for_path(ruta);
        let ruta = ruta.to_owned();
        let yo = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let img = match archivo.read_future(glib::Priority::DEFAULT).await {
                Ok(flujo) => {
                    gdk_pixbuf::Pixbuf::from_stream_at_scale_future(&flujo, pw, ph, true)
                        .await
                        .map_err(|e| eprintln!("morgan: vista previa: {e}"))
                        .ok()
                        .map(|pb| pixeles::de_pixbuf(&pb))
                }
                Err(e) => {
                    eprintln!("morgan: vista previa: {e}");
                    None
                }
            };
            let Some(yo) = yo.upgrade() else { return };
            yo.pendientes.borrow_mut().remove(&ruta);
            yo.previas.borrow_mut().insert(ruta, img);
            yo.version.set(yo.version.get() + 1);
            let f = yo.al_llegar.borrow().clone();
            if let Some(f) = f {
                f();
            }
        });
        None
    }

    /// Las vistas previas de lo que ya no está se sueltan.
    pub fn barrer_previas(&self) {
        let lista = self.notificaciones.lista();
        let vivas: HashSet<&PathBuf> = lista
            .iter()
            .filter_map(|n| match &n.imagen {
                Some(Imagen::Archivo(p)) => Some(p),
                _ => None,
            })
            .collect();
        self.previas.borrow_mut().retain(|p, _| vivas.contains(p));
    }
}

/// Una captura o una foto: la imagen va abajo, a lo ancho.
fn vista_previa(img: Option<&Imagen>) -> Option<(PathBuf, u32, u32)> {
    let Some(Imagen::Archivo(ruta)) = img else {
        return None;
    };
    let (_, ancho, alto) = gdk_pixbuf::Pixbuf::file_info(ruta)?;
    modelo::es_vista_previa(ancho, alto).then(|| (ruta.clone(), ancho as u32, alto as u32))
}

/// Los píxeles que mandó la app (`image-data`), achicados para entrar en `lado`.
fn de_datos(px: &modelo::Pixeles, lado: i32) -> Option<ImageData> {
    let bytes = glib::Bytes::from(&px.datos);
    let pb = gdk_pixbuf::Pixbuf::from_bytes(
        &bytes,
        gdk_pixbuf::Colorspace::Rgb,
        px.alfa,
        8,
        px.ancho,
        px.alto,
        px.fila,
    );
    let (w, h) = (pb.width().max(1), pb.height().max(1));
    let k = f64::from(lado) / f64::from(w.max(h));
    let pb = if k < 1.0 {
        pb.scale_simple(
            ((f64::from(w) * k).round() as i32).max(1),
            ((f64::from(h) * k).round() as i32).max(1),
            gdk_pixbuf::InterpType::Bilinear,
        )?
    } else {
        pb
    };
    Some(pixeles::de_pixbuf(&pb))
}
