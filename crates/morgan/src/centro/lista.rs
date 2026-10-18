
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gtk::prelude::*;

use crate::deslizable::{Accion, Deslizable};
use crate::notificaciones::Notificaciones;
use crate::notificaciones::modelo;
use crate::notificaciones::servidor::Razon;
use crate::pila::Pila;
use crate::tarjeta::{Forma, Tarjeta};

#[derive(Clone)]
pub struct Lista {
    pub raiz: gtk::Box,
    inner: Rc<Inner>,
}

#[derive(Clone)]
struct Fila {
    id: u32,
    varias: bool,
}

struct Inner {
    pila: gtk::Stack,
    lista: gtk::ListBox,
    limpiar: gtk::Button,
    filas: RefCell<Vec<(gtk::ListBoxRow, Fila)>>,
    notificaciones: Notificaciones,
    /// Sobreviven a que el grupo se rearme.
    expandidas: RefCell<HashSet<String>>,
    grupos: RefCell<Vec<Grupo>>,
}

struct Grupo {
    clave: String,
    ids: Vec<u32>,
    /// Si cambia, se rearma el grupo, no sus tarjetas.
    forma: (bool, &'static str, String),
    fila: gtk::ListBoxRow,
    raiz: gtk::Box,
    encabezado: gtk::Revealer,
    pila: Pila,
    tarjetas: HashMap<u32, Rc<Tarjeta>>,
}

impl Lista {
    pub fn new(notificaciones: &Notificaciones) -> Self {
        let raiz = gtk::Box::new(gtk::Orientation::Vertical, 0);
        raiz.add_css_class("widget");
        raiz.add_css_class("widget-notifications");
        raiz.set_overflow(gtk::Overflow::Hidden);

        // El rótulo de la sección, con "Limpiar" a la derecha cuando hay algo que limpiar.
        let cabecera = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        cabecera.add_css_class("cc-seccion");
        let titulo = gtk::Label::new(Some("notificaciones"));
        titulo.set_xalign(0.0);
        titulo.set_hexpand(true);
        cabecera.append(&titulo);
        let limpiar = gtk::Button::with_label("limpiar");
        cabecera.append(&limpiar);
        raiz.append(&cabecera);

        let pila = gtk::Stack::new();
        pila.set_hhomogeneous(true);
        pila.set_vhomogeneous(false);
        pila.set_transition_type(gtk::StackTransitionType::Crossfade);
        pila.set_transition_duration(crate::animaciones::ms(200));
        // Al vaciarse (o llenarse) la altura se desliza, no salta.
        pila.set_interpolate_size(true);
        raiz.append(&pila);

        let lista = gtk::ListBox::new();
        lista.add_css_class("control-center-list");
        lista.set_selection_mode(gtk::SelectionMode::None);
        pila.add_named(&lista, Some("lista"));

        let vacio = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        vacio.add_css_class("control-center-list-placeholder");
        vacio.set_halign(gtk::Align::Center);
        vacio.append(&gtk::Label::new(Some("󰂜")));
        vacio.append(&gtk::Label::new(Some("Nada nuevo")));
        pila.add_named(&vacio, Some("vacio"));

        let l = Lista {
            raiz,
            inner: Rc::new(Inner {
                pila,
                lista,
                limpiar: limpiar.clone(),
                filas: RefCell::default(),
                notificaciones: notificaciones.clone(),
                expandidas: RefCell::default(),
                grupos: RefCell::default(),
            }),
        };
        let yo = l.clone();
        limpiar.connect_clicked(move |_| yo.limpiar());
        l.teclado();
        l
    }

    /// Solo las que había al tocar: una que llega en el medio se queda.
    pub fn limpiar(&self) {
        const SALIDA: u64 = 220; // la transición de `.saliendo` en la hoja
        let inner = &self.inner;
        let ids: Vec<u32> = inner.notificaciones.lista().iter().map(|n| n.id).collect();
        let filas: Vec<gtk::ListBoxRow> = inner
            .filas
            .borrow()
            .iter()
            .map(|(f, _)| f.clone())
            .collect();
        let animar = gtk::Settings::default().is_none_or(|s| s.is_gtk_enable_animations());
        let n = filas.len() as u64;
        if !animar || n == 0 {
            for id in ids {
                inner.notificaciones.cerrar(id, Razon::Usuario);
            }
            return;
        }
        inner.limpiar.set_sensitive(false);
        let paso = (240 / n).min(40);
        for (i, fila) in filas.into_iter().enumerate() {
            let demora = std::time::Duration::from_millis(paso * i as u64);
            gtk::glib::timeout_add_local_once(demora, move || fila.add_css_class("saliendo"));
        }
        let yo = self.clone();
        let fin = std::time::Duration::from_millis(paso * (n - 1) + SALIDA);
        gtk::glib::timeout_add_local_once(fin, move || {
            let inner = &yo.inner;
            for id in ids {
                inner.notificaciones.cerrar(id, Razon::Usuario);
            }
            // Un grupo que sigue (le llegó otra mientras tanto) reusa su fila: vuelve.
            for (f, _) in inner.filas.borrow().iter() {
                f.remove_css_class("saliendo");
            }
            inner.limpiar.set_sensitive(true);
        });
    }

    fn teclado(&self) {
        let teclas = gtk::EventControllerKey::new();
        let yo = self.clone();
        teclas.connect_key_pressed(move |_, tecla, _, _| {
            let Some(fila) = yo.inner.lista.focus_child() else {
                return gtk::glib::Propagation::Proceed;
            };
            let Some((_, cual)) = yo
                .inner
                .filas
                .borrow()
                .iter()
                .find(|(f, _)| f.upcast_ref::<gtk::Widget>() == &fila)
                .cloned()
            else {
                return gtk::glib::Propagation::Proceed;
            };
            let Fila { id, varias } = cual;
            let n = &yo.inner.notificaciones;
            match tecla {
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => {
                    let grupo = n.get(id).map(|x| x.grupo().to_owned());
                    let abierta = grupo
                        .as_ref()
                        .is_some_and(|g| yo.inner.expandidas.borrow().contains(g));
                    if varias && !abierta {
                        if let Some(g) = grupo {
                            yo.inner.expandidas.borrow_mut().insert(g.clone());
                            if let Some(gr) = yo.inner.grupos.borrow().iter().find(|x| x.clave == g)
                            {
                                aplicar(&gr.raiz, &gr.encabezado, &gr.pila, true);
                            }
                        }
                    } else {
                        n.accion(id, "default");
                    }
                }
                gtk::gdk::Key::Delete | gtk::gdk::Key::BackSpace => {
                    n.cerrar(id, Razon::Usuario);
                }
                _ => return gtk::glib::Propagation::Proceed,
            }
            gtk::glib::Propagation::Stop
        });
        self.inner.lista.add_controller(teclas);
    }

    pub fn enfocar(&self) -> bool {
        match self.inner.filas.borrow().first() {
            Some((f, _)) => f.grab_focus(),
            None => false,
        }
    }

    /// Lo demás se reusa: rehacer las tarjetas de un historial largo tarda y traba el centro.
    pub fn actualizar(&self, cambiados: &[u32]) {
        let inner = &self.inner;
        let lista = inner.notificaciones.lista().clone();
        let orden = modelo::grupos(&lista);
        // Lo que se cerró deja de estar abierto.
        let apps: HashSet<String> = lista.iter().map(|n| n.grupo().to_owned()).collect();
        inner.expandidas.borrow_mut().retain(|a| apps.contains(a));

        let mut viejos: HashMap<String, Grupo> = inner
            .grupos
            .take()
            .into_iter()
            .map(|g| (g.clave.clone(), g))
            .collect();
        let mut nuevos = Vec::with_capacity(orden.len());
        for ids in &orden {
            let ns: Vec<_> = ids
                .iter()
                .filter_map(|id| lista.iter().find(|n| n.id == *id))
                .collect();
            let Some(frente) = ns.first() else {
                continue;
            };
            let clave = frente.grupo().to_owned();
            let forma = (
                ns.len() > 1,
                ns.iter()
                    .map(|n| n.urgencia)
                    .max()
                    .unwrap_or(modelo::Urgencia::Normal)
                    .clase(),
                frente.nombre_visible().to_owned(),
            );
            let viejo = viejos.remove(&clave);
            let tocado = ids.iter().any(|id| cambiados.contains(id));
            if let Some(v) = viejo.as_ref()
                && !tocado
                && v.ids == *ids
                && v.forma == forma
            {
                nuevos.push(viejo.expect("recién mirado"));
                continue;
            }
            let mut previas = HashMap::new();
            if let Some(v) = &viejo {
                vaciar(&v.pila);
                previas = v.tarjetas.clone();
            }
            let mut tarjetas = HashMap::new();
            for n in &ns {
                let t = match previas.remove(&n.id) {
                    Some(t) if !cambiados.contains(&n.id) => t,
                    _ => Rc::new(Tarjeta::new(
                        n,
                        Forma::Centro,
                        &inner.notificaciones,
                        Rc::new(|| {}),
                    )),
                };
                tarjetas.insert(n.id, t);
            }
            let g = match viejo {
                // La misma forma: se cambian las tarjetas de la pila y nada más.
                Some(mut v) if v.forma == forma => {
                    llenar(&v.pila, ids, &tarjetas);
                    v.ids = ids.clone();
                    v.tarjetas = tarjetas;
                    v
                }
                otro => {
                    if let Some(v) = otro {
                        inner.lista.remove(&v.fila);
                    }
                    let pila = Pila::default();
                    llenar(&pila, ids, &tarjetas);
                    let (hijo, raiz, encabezado) = self.armar_grupo(&clave, &forma, pila.clone());
                    let fila = gtk::ListBoxRow::new();
                    fila.set_activatable(false);
                    fila.set_child(Some(&hijo));
                    Grupo {
                        clave,
                        ids: ids.clone(),
                        forma,
                        fila,
                        raiz,
                        encabezado,
                        pila,
                        tarjetas,
                    }
                }
            };
            nuevos.push(g);
        }
        for v in viejos.into_values() {
            vaciar(&v.pila);
            inner.lista.remove(&v.fila);
        }
        // Cada fila a su lugar; las que ya están no se tocan.
        for (i, g) in nuevos.iter().enumerate() {
            let i = i as i32;
            if inner.lista.row_at_index(i).as_ref() == Some(&g.fila) {
                continue;
            }
            if g.fila.parent().is_some() {
                inner.lista.remove(&g.fila);
            }
            inner.lista.insert(&g.fila, i);
        }
        inner.filas.replace(
            nuevos
                .iter()
                .map(|g| {
                    (
                        g.fila.clone(),
                        Fila {
                            id: g.ids[0],
                            varias: g.forma.0,
                        },
                    )
                })
                .collect(),
        );
        let nada = nuevos.is_empty();
        inner.grupos.replace(nuevos);
        inner
            .pila
            .set_visible_child_name(if nada { "vacio" } else { "lista" });
        inner.limpiar.set_visible(!nada);
    }

    pub fn refrescar_horas(&self) {
        for g in self.inner.grupos.borrow().iter() {
            for t in g.tarjetas.values() {
                t.actualizar_hora();
            }
        }
    }

    fn armar_grupo(
        &self,
        clave: &str,
        (varias, clase, nombre_visible): &(bool, &'static str, String),
        pila: Pila,
    ) -> (gtk::Widget, gtk::Box, gtk::Revealer) {
        let (varias, clase) = (*varias, *clase);
        let inner = &self.inner;
        // Cerrar todas: las del grupo cuando se toca, no las de cuando se armó.
        let notificaciones = inner.notificaciones.clone();
        let grupo = clave.to_owned();
        let al_cerrar: Rc<dyn Fn()> = Rc::new(move || {
            let ids: Vec<u32> = notificaciones
                .lista()
                .iter()
                .filter(|n| n.grupo() == grupo)
                .map(|n| n.id)
                .collect();
            for id in ids {
                notificaciones.cerrar(id, Razon::Usuario);
            }
        });
        let app = clave.to_owned();
        let raiz = gtk::Box::new(gtk::Orientation::Vertical, 0);
        raiz.add_css_class("notification-group");
        raiz.set_hexpand(true);
        raiz.add_css_class(clase);

        // Encabezado: solo con el grupo abierto.
        let encabezado = gtk::Revealer::new();
        encabezado.set_transition_type(gtk::RevealerTransitionType::SlideUp);
        let controles = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let inicio = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        inicio.add_css_class("notification-group-headers");
        inicio.set_halign(gtk::Align::Start);
        inicio.set_hexpand(true);
        let icono = gtk::Image::new();
        icono.add_css_class("notification-group-icon");
        icono.set_valign(gtk::Align::Center);
        inicio.append(&icono);
        let nombre = gtk::Label::new(Some(nombre_visible));
        nombre.set_xalign(0.0);
        nombre.set_ellipsize(gtk::pango::EllipsizeMode::End);
        nombre.add_css_class("title-1");
        nombre.add_css_class("notification-group-header");
        inicio.append(&nombre);
        controles.append(&inicio);
        let fin = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        fin.add_css_class("notification-group-buttons");
        fin.set_halign(gtk::Align::End);
        let contraer = gtk::Button::from_icon_name("pan-up-symbolic");
        contraer.add_css_class("circular");
        contraer.add_css_class("notification-group-collapse-button");
        contraer.set_valign(gtk::Align::Center);
        fin.append(&contraer);
        let cerrar = gtk::Button::from_icon_name("window-close-symbolic");
        cerrar.add_css_class("circular");
        cerrar.add_css_class("notification-group-close-all-button");
        cerrar.set_valign(gtk::Align::Center);
        fin.append(&cerrar);
        controles.append(&fin);
        encabezado.set_child(Some(&controles));
        raiz.append(&encabezado);

        raiz.append(&pila);

        let abierta = varias && inner.expandidas.borrow().contains(&app);
        aplicar(&raiz, &encabezado, &pila, abierta);

        // Colapsada y con varias, el click abre el grupo y no llega a la tarjeta. Se decide al
        // soltar, no al apretar: apretar y arrastrar es arrastrar el grupo entero.
        if varias {
            let click = gtk::GestureClick::new();
            click.set_propagation_phase(gtk::PropagationPhase::Capture);
            let (r, e, p) = (raiz.clone(), encabezado.clone(), pila.clone());
            let yo = self.clone();
            let a = app.clone();
            click.connect_released(move |g, _, _, _| {
                if p.expandida() {
                    return;
                }
                g.set_state(gtk::EventSequenceState::Claimed);
                yo.inner.expandidas.borrow_mut().insert(a.clone());
                aplicar(&r, &e, &p, true);
            });
            raiz.add_controller(click);
        }
        let (r, e, p) = (raiz.clone(), encabezado.clone(), pila.clone());
        let yo = self.clone();
        let a = app.clone();
        contraer.connect_clicked(move |_| {
            yo.inner.expandidas.borrow_mut().remove(&a);
            aplicar(&r, &e, &p, false);
        });
        let al_cerrar2 = al_cerrar.clone();
        cerrar.connect_clicked(move |_| al_cerrar2());
        if !varias {
            return (raiz.clone().upcast(), raiz, encabezado);
        }
        // Colapsado, el grupo entero se arrastra a la derecha y se cierra todo (abierto, cada
        // tarjeta tiene el suyo).
        let p = pila.clone();
        let hijo = Deslizable::new(
            &raiz,
            Some(Accion {
                glifo: "󰆴",
                texto: "descartar todo",
                se_va: true,
                f: al_cerrar,
            }),
            None,
        )
        .de_grupo(move || !p.expandida())
        .upcast();
        (hijo, raiz, encabezado)
    }
}

fn vaciar(pila: &Pila) {
    while let Some(h) = pila.first_child() {
        h.unparent();
    }
}

fn llenar(pila: &Pila, ids: &[u32], tarjetas: &HashMap<u32, Rc<Tarjeta>>) {
    vaciar(pila);
    for id in ids.iter().rev() {
        if let Some(t) = tarjetas.get(id) {
            if t.raiz.parent().is_some() {
                t.raiz.unparent();
            }
            pila.agregar(&t.raiz);
        }
    }
}

fn aplicar(raiz: &gtk::Box, encabezado: &gtk::Revealer, pila: &Pila, abierta: bool) {
    pila.set_expandida(abierta);
    encabezado.set_reveal_child(abierta);
    if abierta {
        raiz.remove_css_class("collapsed");
    } else {
        raiz.add_css_class("collapsed");
    }
}
