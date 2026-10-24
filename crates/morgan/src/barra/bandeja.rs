//! La bandeja del lado de los servicios: los íconos se buscan y se pasan a píxeles acá (el tema,
//! los archivos y los pixmaps que mandan las apps, con `icono.rs`), y le llegan a la barra.
//!
//! El ícono forzado por `Id` se aplica en cada relectura: no se pierde cuando la app cambia de
//! ícono en caliente.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib;
use morgan_motor::color::{Color, Paleta};
use morgan_motor::vello::peniko::ImageData;
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use super::dibujo::Icono;
use super::hilo::Orden;
use crate::bandeja::menu::{Nodo, secciones};
use crate::bandeja::{Bandeja, Item};
use crate::centro::Centro;
use crate::centro::vista::{Menu as VistaMenu, NodoMenu};
use crate::tema::Tema;

const TAMANO: u32 = 14;

pub struct Contexto {
    centro: Centro,
    bandeja: Bandeja,
    /// El tema propio de la bandeja, si está; si no, el del sistema.
    tema: Option<crate::icono::TemaPropio>,
    iconos: BTreeMap<String, String>,
    hilo: Sender<Orden>,
    /// El color de los íconos simbólicos: el del texto.
    color: Cell<Color>,
    /// La clave del ícono con su menú abierto.
    menu: RefCell<Option<String>>,
}

fn color_de(css: &str) -> Color {
    Paleta::desde_css(css).on_surface
}

impl Contexto {
    pub fn new(
        centro: &Centro,
        bandeja: Bandeja,
        iconos: BTreeMap<String, String>,
        hilo: Sender<Orden>,
        tema: &Tema,
    ) -> Rc<Self> {
        let ctx = Rc::new(Contexto {
            centro: centro.clone(),
            bandeja,
            tema: tema_bandeja(),
            iconos,
            hilo,
            color: Cell::new(color_de(&tema.actual().0)),
            menu: RefCell::default(),
        });
        let debil = Rc::downgrade(&ctx);
        tema.al_cambiar(move |css, _, _| {
            if let Some(ctx) = debil.upgrade() {
                ctx.color.set(color_de(css));
                ctx.enviar();
            }
        });
        ctx
    }

    /// Son pocos y cambian poco: se rehacen todos.
    pub fn enviar(&self) {
        let escala = crate::pixeles::escala().max(1) as u32;
        let iconos = self
            .bandeja
            .items()
            .into_iter()
            .map(|item| Icono {
                imagen: self.imagen(&item, escala),
                clave: item.clave,
                tooltip: item.tooltip,
            })
            .collect();
        let _ = self.hilo.send(Orden::Bandeja(iconos));
    }

    fn imagen(&self, item: &Item, escala: u32) -> Option<ImageData> {
        imagen(item, self, escala)
            .or_else(|| crate::icono::de_nombre("image-missing", TAMANO, escala, self.color.get()))
    }

    pub fn click(self: &Rc<Self>, clave: &str, boton: u32, salida: &str, derecha: i32) {
        // Con un menú abierto, cualquier click en un ícono lo cierra; en el suyo, nada más.
        let abierto = self.menu.borrow().clone();
        if let Some(c) = abierto {
            self.centro.cerrar_menu();
            if c == clave {
                return;
            }
        }
        let menu = Menu {
            ctx: self.clone(),
            clave: clave.to_owned(),
            salida: salida.to_owned(),
            derecha,
        };
        match boton {
            1 => {
                glib::MainContext::default().spawn_local(async move {
                    if !menu.ctx.bandeja.activar(&menu.clave).await {
                        menu.abrir().await;
                    }
                });
            }
            2 => self.bandeja.secundario(clave),
            3 => {
                glib::MainContext::default().spawn_local(async move { menu.abrir().await });
            }
            _ => {}
        }
    }
}

struct Menu {
    ctx: Rc<Contexto>,
    clave: String,
    salida: String,
    derecha: i32,
}

impl Menu {
    async fn abrir(self) {
        let Some(raiz) = self.ctx.bandeja.menu(&self.clave).await else {
            return;
        };
        let Menu {
            ctx,
            clave,
            salida,
            derecha,
        } = self;
        // El ícono pudo desaparecer mientras se leía el menú.
        if !ctx.bandeja.items().iter().any(|i| i.clave == clave) {
            return;
        }
        let debil = Rc::downgrade(&ctx);
        let al_cerrar = move || {
            if let Some(ctx) = debil.upgrade() {
                ctx.menu.borrow_mut().take();
                let _ = ctx.hilo.send(Orden::Menu(None));
            }
        };
        let bandeja = ctx.bandeja.clone();
        let c = clave.clone();
        let m = VistaMenu {
            salida,
            derecha,
            secciones: nodos(&raiz.hijos),
        };
        // Un doble click pudo abrir otro mientras se leía este: `abrir_menu` lo cierra antes.
        ctx.centro
            .abrir_menu(m, move |id| bandeja.elegir(&c, id), al_cerrar);
        let _ = ctx.hilo.send(Orden::Menu(Some(clave.clone())));
        ctx.menu.replace(Some(clave));
    }
}

/// Las secciones de un menú (los separadores parten), con sus submenús.
fn nodos(hijos: &[Nodo]) -> Vec<Vec<NodoMenu>> {
    secciones(hijos)
        .into_iter()
        .map(|s| {
            s.into_iter()
                .map(|n| NodoMenu {
                    id: n.id,
                    etiqueta: n.etiqueta.clone(),
                    habilitado: n.habilitado,
                    marca: n.marca,
                    hijos: nodos(&n.hijos),
                })
                .collect()
        })
        .collect()
}

/// El tema «Bandeja» de `~/.local/share/morgan/bandeja`, si alguien lo armó: pisa los íconos
/// de algunas apps (y hereda del sistema para el resto).
fn tema_bandeja() -> Option<crate::icono::TemaPropio> {
    let datos = glib::user_data_dir();
    let propia = datos.join("morgan/bandeja");
    propia
        .join("Bandeja/index.theme")
        .is_file()
        .then(|| crate::icono::TemaPropio::nuevo("Bandeja", &[propia]))
}

fn imagen(item: &Item, ctx: &Contexto, escala: u32) -> Option<ImageData> {
    let lado = TAMANO * escala;
    let color = ctx.color.get();
    let dibujar = |ruta: &std::path::Path| {
        let tinte = crate::icono::es_simbolico(ruta).then_some(color);
        crate::icono::dibujar(ruta, lado, tinte)
    };
    let nombre: &str = if let Some(i) = ctx.iconos.get(&item.id) {
        i.as_str()
    } else if item.atencion && !item.icono_atencion.is_empty() {
        &item.icono_atencion
    } else {
        &item.icono
    };
    if !nombre.is_empty() {
        if nombre.starts_with('/') {
            return dibujar(std::path::Path::new(nombre));
        }
        let ruta = match &ctx.tema {
            Some(t) => t.buscar(nombre, TAMANO, escala),
            None => crate::icono::buscar(nombre, TAMANO, escala),
        };
        if let Some(r) = ruta
            && let Some(i) = dibujar(&r)
        {
            return Some(i);
        }
        if !item.ruta_tema.is_empty() {
            for ext in ["png", "svg"] {
                let ruta = PathBuf::from(&item.ruta_tema).join(format!("{nombre}.{ext}"));
                if ruta.is_file()
                    && let Some(i) = dibujar(&ruta)
                {
                    return Some(i);
                }
            }
        }
    }
    let (w, h, rgba) = item.pixmap.as_ref()?;
    let i = image::RgbaImage::from_raw(
        u32::try_from(*w).ok()?,
        u32::try_from(*h).ok()?,
        rgba.clone(),
    )?;
    Some(crate::icono::de_rgba(&i, lado))
}
