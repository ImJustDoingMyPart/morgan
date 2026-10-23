//! La bandeja del lado de GTK: los íconos se buscan y se dibujan acá (el tema, los archivos y
//! los pixmaps que mandan las apps, con el mismo GTK que los mostraba), y le llegan a la barra
//! como píxeles. Los menús siguen siendo de GTK (`menu_bandeja.rs`).
//!
//! El ícono forzado por `Id` se aplica en cada relectura: no se pierde cuando la app cambia de
//! ícono en caliente.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk};
use morgan_motor::color::Paleta;
use morgan_motor::vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use super::dibujo::Icono;
use super::hilo::Orden;
use super::menu_bandeja::MenuBandeja;
use crate::bandeja::{Bandeja, Item};
use crate::tema::Tema;

const TAMANO: i32 = 14;

pub struct Contexto {
    app: gtk::Application,
    bandeja: Bandeja,
    tema: gtk::IconTheme,
    iconos: BTreeMap<String, String>,
    hilo: Sender<Orden>,
    /// El color de los íconos simbólicos: el del texto.
    color: Cell<gdk::RGBA>,
    pintor: RefCell<Option<gsk::Renderer>>,
    menu: RefCell<Option<(String, MenuBandeja)>>,
}

fn color_de(css: &str) -> gdk::RGBA {
    let [r, g, b, a] = Paleta::desde_css(css).on_surface;
    gdk::RGBA::new(r, g, b, a)
}

impl Contexto {
    pub fn new(
        app: &gtk::Application,
        bandeja: Bandeja,
        iconos: BTreeMap<String, String>,
        hilo: Sender<Orden>,
        tema: &Tema,
    ) -> Rc<Self> {
        let ctx = Rc::new(Contexto {
            app: app.clone(),
            bandeja,
            tema: tema_bandeja(),
            iconos,
            hilo,
            color: Cell::new(color_de(&tema.actual().0)),
            pintor: RefCell::default(),
            menu: RefCell::default(),
        });
        let debil = Rc::downgrade(&ctx);
        tema.al_cambiar(move |css, _| {
            if let Some(ctx) = debil.upgrade() {
                ctx.color.set(color_de(css));
                ctx.enviar();
            }
        });
        ctx
    }

    /// Son pocos y cambian poco: se rehacen todos.
    pub fn enviar(&self) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let monitores = display.monitors();
        let escala = (0..monitores.n_items())
            .filter_map(|i| monitores.item(i)?.downcast::<gdk::Monitor>().ok())
            .map(|m| m.scale_factor())
            .max()
            .unwrap_or(1)
            .max(1);
        let iconos = self
            .bandeja
            .items()
            .into_iter()
            .map(|item| Icono {
                imagen: self.imagen(&item, &display, escala),
                clave: item.clave,
                tooltip: item.tooltip,
            })
            .collect();
        let _ = self.hilo.send(Orden::Bandeja(iconos));
    }

    fn imagen(&self, item: &Item, display: &gdk::Display, escala: i32) -> Option<ImageData> {
        let p = paintable(item, self, escala).or_else(|| {
            let p = gtk::IconTheme::for_display(display).lookup_icon(
                "image-missing",
                &[],
                TAMANO,
                escala,
                gtk::TextDirection::Ltr,
                gtk::IconLookupFlags::empty(),
            );
            Some(p.upcast())
        })?;
        let mut pintor = self.pintor.borrow_mut();
        if pintor.is_none() {
            let r = gsk::CairoRenderer::new();
            if let Err(e) = r.realize_for_display(display) {
                eprintln!("morgan: bandeja: {e}");
                return None;
            }
            *pintor = Some(r.upcast());
        }
        pixeles(&p, TAMANO * escala, &self.color.get(), pintor.as_ref()?)
    }

    pub fn click(self: &Rc<Self>, clave: &str, boton: u32, salida: &str, derecha: i32) {
        // Con un menú abierto, cualquier click en un ícono lo cierra; en el suyo, nada más.
        let abierto = self.menu.borrow_mut().take();
        if let Some((c, m)) = abierto {
            m.cerrar();
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

/// El ícono en píxeles RGBA sin premultiplicar, de `tam` × `tam`. Los simbólicos, con `color`.
fn pixeles(
    p: &gdk::Paintable,
    tam: i32,
    color: &gdk::RGBA,
    pintor: &gsk::Renderer,
) -> Option<ImageData> {
    let snap = gtk::Snapshot::new();
    let lado = f64::from(tam);
    match p.dynamic_cast_ref::<gtk::SymbolicPaintable>() {
        Some(s) => s.snapshot_symbolic(&snap, lado, lado, std::slice::from_ref(color)),
        None => p.snapshot(&snap, lado, lado),
    }
    let nodo = snap.to_node()?;
    let caja = graphene::Rect::new(0.0, 0.0, tam as f32, tam as f32);
    let textura = pintor.render_texture(&nodo, Some(&caja));
    let mut bajada = gdk::TextureDownloader::new(&textura);
    bajada.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, paso) = bajada.download_bytes();
    let (w, h) = (textura.width() as usize, textura.height() as usize);
    let mut px = Vec::with_capacity(w * h * 4);
    for fila in bytes.chunks(paso).take(h) {
        px.extend_from_slice(fila.get(..w * 4)?);
    }
    Some(ImageData {
        data: Blob::new(Arc::new(px)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: w as u32,
        height: h as u32,
    })
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
        // Un doble click pudo abrir otro mientras se leía este.
        let anterior = ctx.menu.borrow_mut().take();
        if let Some((_, m)) = anterior {
            m.cerrar();
        }
        let monitor = monitor(&salida);
        let _ = ctx.hilo.send(Orden::Menu(Some(clave.clone())));
        let debil = Rc::downgrade(&ctx);
        let al_cerrar = move || {
            if let Some(ctx) = debil.upgrade() {
                ctx.menu.borrow_mut().take();
                let _ = ctx.hilo.send(Orden::Menu(None));
            }
        };
        let bandeja = ctx.bandeja.clone();
        let c = clave.clone();
        let Some(m) = MenuBandeja::abrir(
            &ctx.app,
            monitor.as_ref(),
            derecha,
            &raiz,
            move |id| bandeja.elegir(&c, id),
            al_cerrar,
        ) else {
            let _ = ctx.hilo.send(Orden::Menu(None));
            return;
        };
        ctx.menu.replace(Some((clave, m)));
    }
}

fn monitor(conector: &str) -> Option<gdk::Monitor> {
    let lista = gdk::Display::default()?.monitors();
    (0..lista.n_items())
        .filter_map(|i| lista.item(i)?.downcast::<gdk::Monitor>().ok())
        .find(|m| m.connector().as_deref() == Some(conector))
}

fn tema_bandeja() -> gtk::IconTheme {
    let datos = glib::user_data_dir();
    if !datos.join("morgan/bandeja/Bandeja/index.theme").is_file()
        && let Some(d) = gtk::gdk::Display::default()
    {
        return gtk::IconTheme::for_display(&d);
    }
    let mut rutas: Vec<PathBuf> = vec![datos.join("morgan/bandeja"), datos.join("icons")];
    rutas.extend(
        glib::system_data_dirs()
            .into_iter()
            .map(|d| d.join("icons")),
    );
    rutas.push(PathBuf::from("/usr/share/pixmaps"));
    let tema = gtk::IconTheme::new();
    let rutas: Vec<&std::path::Path> = rutas.iter().map(PathBuf::as_path).collect();
    tema.set_search_path(&rutas);
    tema.set_theme_name(Some("Bandeja"));
    tema
}

fn paintable(item: &Item, ctx: &Contexto, escala: i32) -> Option<gdk::Paintable> {
    let nombre: &str = if let Some(i) = ctx.iconos.get(&item.id) {
        i.as_str()
    } else if item.atencion && !item.icono_atencion.is_empty() {
        &item.icono_atencion
    } else {
        &item.icono
    };
    if !nombre.is_empty() {
        if nombre.starts_with('/') {
            return archivo(std::path::Path::new(nombre), escala);
        }
        if ctx.tema.has_icon(nombre) {
            let p = ctx.tema.lookup_icon(
                nombre,
                &[],
                TAMANO,
                escala,
                gtk::TextDirection::Ltr,
                gtk::IconLookupFlags::empty(),
            );
            return Some(p.upcast());
        }
        if !item.ruta_tema.is_empty() {
            for ext in ["png", "svg"] {
                let ruta = PathBuf::from(&item.ruta_tema).join(format!("{nombre}.{ext}"));
                if let Some(a) = archivo(&ruta, escala) {
                    return Some(a);
                }
            }
        }
    }
    let (w, h, rgba) = item.pixmap.as_ref()?;
    let bytes = glib::Bytes::from(rgba);
    let textura = gdk::MemoryTexture::new(
        *w,
        *h,
        gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        usize::try_from(*w).ok()? * 4,
    );
    Some(textura.upcast())
}

fn archivo(ruta: &std::path::Path, escala: i32) -> Option<gdk::Paintable> {
    if !ruta.exists() {
        return None;
    }
    let p = gtk::IconPaintable::for_file(&gio::File::for_path(ruta), TAMANO, escala);
    Some(p.upcast())
}
