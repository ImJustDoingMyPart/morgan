//! Las notificaciones como las ve el centro: los grupos en el orden de swaync y cada tarjeta con
//! sus imágenes ya en píxeles. Las imágenes se guardan entre aperturas (rehacerlas con decenas
//! de avisos trabaría el hilo de GTK); una vista previa se decodifica ya achicada, fuera del hilo,
//! y cuando llega se vuelve a mandar la lista.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use morgan_motor::vello::peniko::ImageData;

use super::dibujo::avisos::tam_previa_en;
use super::vista::{Aviso, Grupo};
use crate::notificaciones::modelo::{self, Imagen, Notificacion};
use crate::notificaciones::{Notificaciones, captura};
use crate::pixeles;

/// Los tamaños de las imágenes de una tarjeta, en lógicos.
pub struct Medidas {
    /// Una foto, una tapa o los píxeles que mandó la app.
    pub imagen: i32,
    /// Un ícono del tema.
    pub icono: i32,
    /// El de la app, chico, en la esquina de la imagen.
    pub chico: i32,
    /// El ancho de la vista previa.
    pub previa: f64,
}

/// En el centro, todo en la columna de 36.
pub const CENTRO: Medidas = Medidas {
    imagen: 36,
    icono: 36,
    chico: 16,
    previa: 316.0,
};

/// En un toast, un ícono va a 24 en una columna de 36 y una foto a 40 (`.image` de la hoja
/// de GTK hasta 0.1.1).
pub const TOAST: Medidas = Medidas {
    imagen: 40,
    icono: 24,
    chico: 16,
    previa: 268.0,
};

#[derive(Clone, PartialEq, Eq, Hash)]
enum ClaveImagen {
    Nombre(String, i32),
    Archivo(PathBuf, i32),
    Datos(u32, i32),
}

pub struct Lista {
    notificaciones: Notificaciones,
    medidas: Medidas,
    imagenes: RefCell<HashMap<ClaveImagen, Option<ImageData>>>,
    previas: RefCell<HashMap<PathBuf, Option<ImageData>>>,
    pendientes: RefCell<HashSet<PathBuf>>,
    /// El color de los íconos simbólicos: el del texto.
    color: Cell<[f32; 4]>,
    /// Para volver a mandar la lista cuando llega una vista previa.
    al_llegar: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Lista {
    pub fn new(notificaciones: &Notificaciones, color: [f32; 4], medidas: Medidas) -> Rc<Self> {
        Rc::new(Lista {
            notificaciones: notificaciones.clone(),
            medidas,
            imagenes: RefCell::default(),
            previas: RefCell::default(),
            pendientes: RefCell::default(),
            color: Cell::new(color),
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

    /// Esas notificaciones, en ese orden, cada una sola (los toasts).
    pub fn sueltas(self: &Rc<Self>, ids: &[u32]) -> Vec<Aviso> {
        let lista = self.notificaciones.lista().clone();
        let escala = pixeles::escala();
        let mut usadas = HashSet::new();
        let avisos = ids
            .iter()
            .filter_map(|id| lista.iter().find(|n| n.id == *id))
            .map(|n| self.aviso(n, escala, &mut usadas))
            .collect();
        self.imagenes.borrow_mut().retain(|k, _| usadas.contains(k));
        avisos
    }

    fn aviso(
        self: &Rc<Self>,
        n: &Notificacion,
        escala: i32,
        usadas: &mut HashSet<ClaveImagen>,
    ) -> Aviso {
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
        let icono = matches!(columna, Some(Imagen::Nombre(_)));
        let tam = if icono {
            self.medidas.icono
        } else {
            self.medidas.imagen
        };
        let imagen = columna
            .as_ref()
            .and_then(|i| self.imagen(n.id, i, tam, escala, usadas));
        let chico = chico
            .filter(|_| imagen.is_some())
            .and_then(|i| self.imagen(n.id, &i, self.medidas.chico, escala, usadas));
        let previa = previa.and_then(|(ruta, w, h)| {
            let img = self.previa(&ruta, w, h, escala)?;
            Some((img, w, h))
        });
        let es_captura = captura::es_captura(n);
        let acciones = n
            .botones()
            .map(|a| {
                let glifo = es_captura.then(|| captura::glifo(&a.clave)).flatten();
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
            icono,
            chico,
            previa,
            progreso: n.progreso,
            acciones,
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
                crate::icono::de_nombre(n, tam as u32, escala as u32, self.color.get())
            }
            Imagen::Archivo(p) => pixeles::archivo(p, lado),
            Imagen::Datos(px) => de_datos(px, lado),
        };
        self.imagenes.borrow_mut().insert(clave, i.clone());
        i
    }

    /// La vista previa, si ya está; si no, se pide (decodificada ya achicada, fuera del hilo).
    fn previa(
        self: &Rc<Self>,
        ruta: &std::path::Path,
        w: u32,
        h: u32,
        escala: i32,
    ) -> Option<ImageData> {
        if let Some(i) = self.previas.borrow().get(ruta) {
            return i.clone();
        }
        if !self.pendientes.borrow_mut().insert(ruta.to_owned()) {
            return None;
        }
        let (pw, ph) = tam_previa_en(w, h, self.medidas.previa);
        let (pw, ph) = (pw as i32 * escala, ph as i32 * escala);
        let ruta = ruta.to_owned();
        let yo = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let leer = ruta.clone();
            let img = gio::spawn_blocking(move || {
                pixeles::archivo_en(&leer, pw.max(1) as u32, ph.max(1) as u32)
            })
            .await
            .ok()
            .flatten();
            let Some(yo) = yo.upgrade() else { return };
            yo.pendientes.borrow_mut().remove(&ruta);
            yo.previas.borrow_mut().insert(ruta, img);
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
    let (ancho, alto) = pixeles::medidas(ruta)?;
    modelo::es_vista_previa(ancho as i32, alto as i32).then(|| (ruta.clone(), ancho, alto))
}

/// Los píxeles que mandó la app (`image-data`), achicados para entrar en `lado`.
fn de_datos(px: &modelo::Pixeles, lado: i32) -> Option<ImageData> {
    let (w, h) = (u32::try_from(px.ancho).ok()?, u32::try_from(px.alto).ok()?);
    let (fila, canales) = (usize::try_from(px.fila).ok()?, if px.alfa { 4 } else { 3 });
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        let f = px.datos.get(y * fila..)?;
        for x in 0..w as usize {
            let p = f.get(x * canales..x * canales + canales)?;
            rgba.extend_from_slice(&[p[0], p[1], p[2], if px.alfa { p[3] } else { 255 }]);
        }
    }
    let i = image::RgbaImage::from_raw(w, h, rgba)?;
    let lado = lado.max(1) as u32;
    Some(pixeles::achicar(i, lado, lado))
}
