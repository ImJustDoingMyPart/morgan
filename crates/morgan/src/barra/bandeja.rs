//! El ícono forzado por `Id` se aplica en cada relectura: no se pierde cuando la app cambia de
//! ícono en caliente.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use super::menu_bandeja::MenuBandeja;
use crate::bandeja::{Bandeja, Item};

const TAMANO: i32 = 14;
const TRANSICION: u32 = 300;

pub struct Contexto {
    pub bandeja: Bandeja,
    pub tema: gtk::IconTheme,
    pub iconos: BTreeMap<String, String>,
    menu: RefCell<Option<(String, MenuBandeja)>>,
}

impl Contexto {
    pub fn new(bandeja: Bandeja, iconos: BTreeMap<String, String>) -> Self {
        Contexto {
            bandeja,
            tema: tema_bandeja(),
            iconos,
            menu: RefCell::default(),
        }
    }
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

pub struct Cajon {
    pub raiz: gtk::Box,
    items: gtk::Box,
    revelador: gtk::Revealer,
    movimiento: gtk::EventControllerMotion,
    menu_abierto: Rc<Cell<bool>>,
}

impl Cajon {
    pub fn new() -> Self {
        let flecha = gtk::Label::new(Some("󰅁"));
        flecha.add_css_class("flecha");
        let items = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        items.add_css_class("bandeja");
        let revelador = gtk::Revealer::new();
        revelador.set_transition_type(gtk::RevealerTransitionType::SlideLeft);
        revelador.set_transition_duration(crate::animaciones::ms(TRANSICION));
        revelador.set_child(Some(&items));
        let raiz = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        raiz.add_css_class("cajon");
        raiz.append(&flecha);
        raiz.append(&revelador);

        let menu_abierto = Rc::new(Cell::new(false));
        let movimiento = gtk::EventControllerMotion::new();
        let r = revelador.clone();
        movimiento.connect_enter(move |_, _, _| r.set_reveal_child(true));
        let r = revelador.clone();
        let abierto = menu_abierto.clone();
        // Con un menú abierto el puntero está en el popover: el cajón no se cierra debajo.
        movimiento.connect_leave(move |_| {
            if !abierto.get() {
                r.set_reveal_child(false);
            }
        });
        raiz.add_controller(movimiento.clone());
        Cajon {
            raiz,
            items,
            revelador,
            movimiento,
            menu_abierto,
        }
    }

    /// Son pocos y cambian poco: no vale la pena reusarlos.
    pub fn pintar(&self, ctx: &Rc<Contexto>, ventana: &gtk::ApplicationWindow) {
        while let Some(h) = self.items.first_child() {
            self.items.remove(&h);
        }
        let escala = ventana.scale_factor();
        for item in ctx.bandeja.items() {
            let imagen = match paintable(&item, ctx, escala) {
                Some(p) => gtk::Image::from_paintable(Some(&p)),
                None => gtk::Image::from_icon_name("image-missing"),
            };
            imagen.set_pixel_size(TAMANO);
            imagen.add_css_class("boton");
            if !item.tooltip.is_empty() {
                imagen.set_tooltip_text(Some(&item.tooltip));
            }
            self.clicks(&imagen, &item.clave, ctx, ventana);
            self.items.append(&imagen);
        }
    }

    fn clicks(
        &self,
        imagen: &gtk::Image,
        clave: &str,
        ctx: &Rc<Contexto>,
        ventana: &gtk::ApplicationWindow,
    ) {
        let click = gtk::GestureClick::new();
        click.set_button(0);
        super::apretable(&click);
        let (c, ctx2, img, win) = (
            clave.to_owned(),
            ctx.clone(),
            imagen.clone(),
            ventana.clone(),
        );
        let abierto = self.menu_abierto.clone();
        let (rev, mov) = (self.revelador.clone(), self.movimiento.clone());
        click.connect_released(move |g, _, _, _| {
            let boton = g.current_button();
            // Con un menú abierto, cualquier click en un ícono lo cierra; en el suyo, nada más.
            let abierto_antes = ctx2.menu.borrow_mut().take();
            if let Some((clave, m)) = abierto_antes {
                m.cerrar();
                if clave == c {
                    return;
                }
            }
            let menu = Menu {
                ctx: ctx2.clone(),
                clave: c.clone(),
                imagen: img.clone(),
                ventana: win.clone(),
                abierto: abierto.clone(),
                revelador: rev.clone(),
                movimiento: mov.clone(),
            };
            match boton {
                1 => {
                    glib::MainContext::default().spawn_local(async move {
                        if !menu.ctx.bandeja.activar(&menu.clave).await {
                            menu.abrir().await;
                        }
                    });
                }
                2 => ctx2.bandeja.secundario(&c),
                3 => {
                    glib::MainContext::default().spawn_local(async move { menu.abrir().await });
                }
                _ => {}
            }
        });
        imagen.add_controller(click);

        let rueda = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let (c, ctx2) = (clave.to_owned(), ctx.clone());
        rueda.connect_scroll(move |_, _, dy| {
            let delta = dy.round() as i32;
            if delta != 0 {
                ctx2.bandeja.desplazar(&c, delta);
            }
            glib::Propagation::Stop
        });
        imagen.add_controller(rueda);
    }
}

struct Menu {
    ctx: Rc<Contexto>,
    clave: String,
    imagen: gtk::Image,
    ventana: gtk::ApplicationWindow,
    abierto: Rc<Cell<bool>>,
    revelador: gtk::Revealer,
    movimiento: gtk::EventControllerMotion,
}

impl Menu {
    async fn abrir(self) {
        let Some(raiz) = self.ctx.bandeja.menu(&self.clave).await else {
            return;
        };
        // El ícono pudo desaparecer mientras se leía el menú.
        if self.imagen.parent().is_none() {
            return;
        }
        let bandeja = self.ctx.bandeja.clone();
        let clave = self.clave.clone();
        let Menu {
            ctx,
            clave: c,
            imagen,
            ventana,
            abierto,
            revelador,
            movimiento,
        } = self;
        // Un doble click pudo abrir otro mientras se leía este.
        let anterior = ctx.menu.borrow_mut().take();
        if let Some((_, m)) = anterior {
            m.cerrar();
        }
        abierto.set(true);
        imagen.add_css_class("abierto");
        let (a, i) = (abierto.clone(), imagen.clone());
        let img = imagen.clone();
        let debil = Rc::downgrade(&ctx);
        let al_cerrar = move || {
            if let Some(ctx) = debil.upgrade() {
                ctx.menu.borrow_mut().take();
            }
            abierto.set(false);
            img.remove_css_class("abierto");
            if !movimiento.contains_pointer() {
                revelador.set_reveal_child(false);
            }
        };
        let Some(m) = MenuBandeja::abrir(
            &ventana,
            &imagen,
            &raiz,
            move |id| bandeja.elegir(&clave, id),
            al_cerrar,
        ) else {
            a.set(false);
            i.remove_css_class("abierto");
            return;
        };
        ctx.menu.replace(Some((c, m)));
    }
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
