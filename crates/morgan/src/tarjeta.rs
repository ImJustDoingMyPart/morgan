
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, pango};

use crate::centro::desplegable::{Item, opcion};
use crate::deslizable::{Accion, Deslizable};
use crate::notificaciones::modelo::{self, Imagen, Notificacion};
use crate::notificaciones::servidor::{self, Razon};
use crate::notificaciones::{Notificaciones, captura};

pub const LINEAS_TOAST: i32 = 5;
pub const LINEAS_CENTRO: i32 = 10;

/// Flotando no lleva hora.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forma {
    Toast,
    Centro,
}

pub struct Tarjeta {
    pub raiz: gtk::Box,
    pub fondo: gtk::Box,
    hora: gtk::Label,
    llegada: i64,
}

impl Tarjeta {
    pub fn new(
        n: &Notificacion,
        forma: Forma,
        notificaciones: &Notificaciones,
        al_tocar: Rc<dyn Fn()>,
    ) -> Tarjeta {
        let raiz = gtk::Box::new(gtk::Orientation::Vertical, 0);
        raiz.add_css_class("notification-row");
        raiz.set_hexpand(true);
        let fondo_externo = gtk::Box::new(gtk::Orientation::Vertical, 0);
        fondo_externo.add_css_class("notification-background");
        raiz.append(&fondo_externo);

        let fondo = gtk::Box::new(gtk::Orientation::Vertical, 0);
        fondo.add_css_class("notification");
        fondo.add_css_class(n.urgencia.clase());
        fondo.set_overflow(gtk::Overflow::Hidden);
        // A la izquierda se copia el texto (título y cuerpo, sin markup) y la tarjeta vuelve.
        let descartar = {
            let (notificaciones, al_tocar, id) = (notificaciones.clone(), al_tocar.clone(), n.id);
            Rc::new(move || {
                notificaciones.cerrar(id, Razon::Usuario);
                al_tocar();
            })
        };
        let texto = texto_plano(n);
        let copiar = Rc::new(move || crate::comando::lanzar(&["wl-copy", "--", &texto]));
        fondo_externo.append(&Deslizable::new(
            &fondo,
            Some(Accion {
                glifo: "󰆴",
                texto: "descartar",
                se_va: true,
                f: descartar,
            }),
            Some(Accion {
                glifo: "󰆏",
                texto: "copiar",
                se_va: false,
                f: copiar,
            }),
        ));

        let accion = gtk::Box::new(gtk::Orientation::Vertical, 0);
        accion.add_css_class("notification-default-action");
        fondo.append(&accion);
        let contenido = gtk::Box::new(gtk::Orientation::Vertical, 8);
        contenido.add_css_class("notification-content");
        accion.append(&contenido);

        let fila = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        contenido.append(&fila);
        // Una captura o una foto va abajo, a lo ancho; en su columna queda el ícono de la app.
        let previa = vista_previa(n.imagen.as_ref());
        let columna = match &previa {
            Some(_) => Some(
                n.icono_app
                    .clone()
                    .unwrap_or_else(|| Imagen::Nombre("image-x-generic".into())),
            ),
            None => n.imagen.clone(),
        };
        let pequeno = if previa.is_some() {
            None
        } else {
            n.icono_app.as_ref()
        };
        if let Some(img) = &columna {
            let capas = gtk::Overlay::new();
            capas.set_halign(gtk::Align::Center);
            capas.set_valign(gtk::Align::Center);
            let imagen = imagen_de(img);
            imagen.add_css_class("image");
            // Un ícono del tema va en el cuadrito del OSD; una foto o una tapa, no.
            if matches!(img, Imagen::Nombre(_)) {
                imagen.add_css_class("icono");
            }
            imagen.set_valign(gtk::Align::Center);
            imagen.set_overflow(gtk::Overflow::Hidden);
            capas.set_child(Some(&imagen));
            if let Some(app) = pequeno {
                let chico = imagen_de(app);
                chico.add_css_class("app-icon");
                chico.set_halign(gtk::Align::End);
                chico.set_valign(gtk::Align::End);
                capas.add_overlay(&chico);
            }
            fila.append(&capas);
        }

        let textos = gtk::Box::new(gtk::Orientation::Vertical, 0);
        textos.add_css_class("text-box");
        textos.set_hexpand(true);
        textos.set_vexpand(true);
        textos.set_valign(gtk::Align::Center);
        fila.append(&textos);
        let arriba = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        textos.append(&arriba);
        let resumen = gtk::Label::new(Some(n.titulo()));
        resumen.add_css_class("summary");
        resumen.set_ellipsize(pango::EllipsizeMode::End);
        resumen.set_xalign(0.0);
        resumen.set_hexpand(true);
        arriba.append(&resumen);
        let hora = gtk::Label::new(None);
        hora.add_css_class("time");
        hora.set_valign(gtk::Align::Start);
        hora.set_visible(forma == Forma::Centro);
        arriba.append(&hora);
        let lineas = match forma {
            Forma::Toast => LINEAS_TOAST,
            Forma::Centro => LINEAS_CENTRO,
        };
        if !n.cuerpo.is_empty() {
            let cuerpo = gtk::Label::new(None);
            cuerpo.set_markup(&n.cuerpo);
            cuerpo.add_css_class("body");
            cuerpo.set_ellipsize(pango::EllipsizeMode::End);
            cuerpo.set_wrap(true);
            cuerpo.set_wrap_mode(pango::WrapMode::WordChar);
            cuerpo.set_natural_wrap_mode(gtk::NaturalWrapMode::Word);
            cuerpo.set_lines(lineas);
            cuerpo.set_xalign(0.0);
            cuerpo.set_yalign(0.0);
            cuerpo.set_hexpand(true);
            textos.append(&cuerpo);
        }
        if let Some((ruta, ancho, alto)) = previa {
            contenido.append(&imagen_previa(&ruta, ancho, alto, forma));
        }
        if let Some(v) = n.progreso {
            let barra = gtk::ProgressBar::new();
            barra.set_fraction(f64::from(v) / 100.0);
            contenido.append(&barra);
        }

        let botones: Vec<_> = n.botones().cloned().collect();
        // En el centro, las acciones no se ven hasta tocar la tarjeta: entonces se despliegan
        // debajo con la forma de las listas del panel. Flotando siguen a la vista (el toast
        // dura unos segundos).
        let mut desplegar: Option<gtk::Revealer> = None;
        if !botones.is_empty() && forma == Forma::Centro {
            let es_captura = captura::es_captura(n);
            let items = botones
                .into_iter()
                .map(|a| {
                    let notificaciones = notificaciones.clone();
                    let al_tocar = al_tocar.clone();
                    let id = n.id;
                    let glifo = es_captura
                        .then(|| captura::glifo(&a.clave))
                        .flatten()
                        .unwrap_or("󰅂");
                    Item::new(glifo, a.etiqueta.clone(), None, move || {
                        notificaciones.accion(id, &a.clave);
                        al_tocar();
                    })
                })
                .collect::<Vec<_>>();
            let lista = gtk::Box::new(gtk::Orientation::Vertical, 0);
            lista.add_css_class("cc-lista");
            lista.add_css_class("acciones");
            for it in items {
                lista.append(&opcion(it));
            }
            let r = gtk::Revealer::new();
            r.set_transition_type(gtk::RevealerTransitionType::SlideDown);
            r.set_transition_duration(crate::animaciones::ms(200));
            r.set_child(Some(&lista));
            fondo.append(&r);
            desplegar = Some(r);
        } else if !botones.is_empty() {
            // Flotando, la misma lista del centro: en fila si entran todas enteras, si no en
            // columna. Sin la flecha del centro: solo el glifo propio de una acción.
            let es_captura = captura::es_captura(n);
            let glifos: Vec<Option<&str>> = botones
                .iter()
                .map(|a| es_captura.then(|| captura::glifo(&a.clave)).flatten())
                .collect();
            let en_fila = modelo::acciones_en_fila(
                glifos
                    .iter()
                    .zip(&botones)
                    .map(|(g, a)| (g.is_some(), a.etiqueta.as_str())),
            );
            let lista = gtk::Box::new(
                if en_fila {
                    gtk::Orientation::Horizontal
                } else {
                    gtk::Orientation::Vertical
                },
                0,
            );
            lista.add_css_class("cc-lista");
            lista.add_css_class("acciones");
            if en_fila {
                lista.add_css_class("en-fila");
            }
            for (a, glifo) in botones.into_iter().zip(glifos) {
                let b = if en_fila {
                    opcion_en_fila(glifo, &a.etiqueta, true)
                } else if columna.is_some() {
                    // Con ícono, la columna del glifo queda aunque no haya glifo: el texto
                    // arranca con el del título, como en el centro.
                    opcion(Item::new(
                        glifo.unwrap_or(""),
                        a.etiqueta.clone(),
                        None,
                        || {},
                    ))
                } else {
                    // Sin ícono, el título arranca en el borde: el texto también.
                    opcion_en_fila(glifo, &a.etiqueta, false)
                };
                let notificaciones = notificaciones.clone();
                let al_tocar = al_tocar.clone();
                let id = n.id;
                b.connect_clicked(move |_| {
                    notificaciones.accion(id, &a.clave);
                    al_tocar();
                });
                lista.append(&b);
            }
            fondo.append(&lista);
        }

        // Izquierdo: la acción por defecto (o cerrar). Medio y derecho: cerrar.
        let click = gtk::GestureClick::new();
        click.set_button(0);
        let notificaciones = notificaciones.clone();
        let id = n.id;
        let f = fondo.clone();
        click.connect_released(move |g, _, _, _| {
            match (g.current_button(), &desplegar) {
                // Con acciones (en el centro), el click las muestra o las esconde.
                (1, Some(r)) => {
                    let abrir = !r.reveals_child();
                    r.set_reveal_child(abrir);
                    if abrir {
                        f.add_css_class("abierta");
                    } else {
                        f.remove_css_class("abierta");
                    }
                    return;
                }
                (1, None) => notificaciones.accion(id, "default"),
                _ => notificaciones.cerrar(id, Razon::Usuario),
            }
            al_tocar();
        });
        accion.add_controller(click);

        let t = Tarjeta {
            raiz,
            fondo,
            hora,
            llegada: n.llegada,
        };
        t.actualizar_hora();
        t
    }

    pub fn actualizar_hora(&self) {
        self.hora
            .set_text(&modelo::relativo(servidor::ahora() - self.llegada));
    }
}

fn opcion_en_fila(glifo: Option<&str>, texto: &str, centrada: bool) -> gtk::Button {
    let fila = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    fila.set_halign(if centrada {
        gtk::Align::Center
    } else {
        gtk::Align::Start
    });
    if let Some(glifo) = glifo {
        let g = gtk::Label::new(Some(glifo));
        g.add_css_class("glifo");
        fila.append(&g);
    }
    let t = gtk::Label::new(Some(texto));
    t.set_ellipsize(pango::EllipsizeMode::End);
    fila.append(&t);
    let b = gtk::Button::new();
    b.set_child(Some(&fila));
    b.add_css_class("cc-opcion");
    b.set_hexpand(true);
    b
}

fn texto_plano(n: &Notificacion) -> String {
    let cuerpo = pango::parse_markup(&n.cuerpo, '\0')
        .map(|(_, t, _)| t.to_string())
        .unwrap_or_else(|_| n.cuerpo.clone());
    [n.titulo(), cuerpo.as_str()]
        .into_iter()
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Fijo, para que el alto salga de la proporción sin medir nada.
fn ancho_previa(forma: Forma) -> i32 {
    match forma {
        Forma::Toast => 268,
        Forma::Centro => 316,
    }
}

/// Una captura vertical no se come la pantalla.
const ALTO_PREVIA: i32 = 180;

fn vista_previa(img: Option<&Imagen>) -> Option<(std::path::PathBuf, i32, i32)> {
    let Some(Imagen::Archivo(ruta)) = img else {
        return None;
    };
    let (_, ancho, alto) = gtk::gdk_pixbuf::Pixbuf::file_info(ruta)?;
    modelo::es_vista_previa(ancho, alto).then(|| (ruta.clone(), ancho, alto))
}

/// La imagen se decodifica ya escalada, fuera del hilo de GTK: abrir el centro con varias
/// capturas en el historial no traba nada. Va dentro de un `Fixed`, que le da su tamaño
/// natural: los toasts se miden ancho-para-alto y una imagen vertical pediría todo el ancho.
fn imagen_previa(ruta: &std::path::Path, ancho: i32, alto: i32, forma: Forma) -> gtk::Fixed {
    let (ancho, alto) = (i64::from(ancho.max(1)), i64::from(alto.max(1)));
    let caja = i64::from(ancho_previa(forma));
    let (w, h) = if caja * alto / ancho <= i64::from(ALTO_PREVIA) {
        (caja, caja * alto / ancho)
    } else {
        let tope = i64::from(ALTO_PREVIA);
        (tope * ancho / alto, tope)
    };
    let (w, h) = (w.max(1) as i32, h.max(1) as i32);
    let p = gtk::Picture::new();
    p.add_css_class("preview");
    p.set_content_fit(gtk::ContentFit::Fill);
    p.set_can_shrink(true);
    p.set_overflow(gtk::Overflow::Hidden);
    p.set_size_request(w, h);
    let marco = gtk::Fixed::new();
    marco.set_halign(gtk::Align::Center);
    marco.put(&p, 0.0, 0.0);
    let archivo = gio::File::for_path(ruta);
    let debil = p.downgrade();
    gtk::glib::MainContext::default().spawn_local(async move {
        let flujo = match archivo.read_future(gtk::glib::Priority::DEFAULT).await {
            Ok(f) => f,
            Err(e) => {
                eprintln!("morgan: vista previa: {e}");
                return;
            }
        };
        match gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale_future(&flujo, w, h, true).await {
            Ok(pb) => {
                if let Some(p) = debil.upgrade() {
                    let formato = if pb.has_alpha() {
                        gtk::gdk::MemoryFormat::R8g8b8a8
                    } else {
                        gtk::gdk::MemoryFormat::R8g8b8
                    };
                    let textura = gtk::gdk::MemoryTexture::new(
                        pb.width(),
                        pb.height(),
                        formato,
                        &pb.read_pixel_bytes(),
                        pb.rowstride() as usize,
                    );
                    p.set_paintable(Some(&textura));
                }
            }
            Err(e) => eprintln!("morgan: vista previa: {e}"),
        }
    });
    marco
}

fn imagen_de(img: &Imagen) -> gtk::Image {
    match img {
        Imagen::Nombre(n) => gtk::Image::from_gicon(&gio::ThemedIcon::new(n)),
        Imagen::Archivo(p) => gtk::Image::from_file(p),
        // El servidor ya la pasó a PNG; si no pudo, no hay qué mostrar.
        Imagen::Datos(_) => gtk::Image::new(),
    }
}
