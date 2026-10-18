//! Los hijos van de la más vieja a la más nueva: la de adelante es la última, y así la hoja la
//! reconoce con `:last-child`.

use std::cell::Cell;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, graphene, gsk};

pub const APILADAS: usize = 3;
pub const DESPLAZAMIENTO: i32 = 8;
const ESCALA: f32 = 0.95;

pub fn alto_colapsada(alto_frente: i32, n: usize) -> i32 {
    alto_frente + DESPLAZAMIENTO * (n.clamp(1, APILADAS) as i32 - 1)
}

pub fn ubicacion(i: usize, ancho: i32, alto: i32) -> (f32, f32, f32) {
    let escala = ESCALA.powi(i as i32);
    let x = (ancho as f32 - ancho as f32 * escala) * 0.5;
    let y = alto as f32 * (1.0 - escala) + (DESPLAZAMIENTO * i as i32) as f32;
    (x, y, escala)
}

/// Una más alta (una captura grande detrás de una chica) no se puede asignar más corta: su
/// contenido no achica y se derramaría debajo de la pila. De atrás solo se ve el fondo, así que
/// aplastarla no se nota.
pub fn aplastado(alto: i32, alto_propio: i32) -> f32 {
    if alto_propio <= alto {
        1.0
    } else {
        alto as f32 / alto_propio as f32
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Pila {
        pub expandida: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Pila {
        const NAME: &'static str = "MorganPila";
        type Type = super::Pila;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Pila {
        fn dispose(&self) {
            while let Some(h) = self.obj().first_child() {
                h.unparent();
            }
        }
    }

    impl WidgetImpl for Pila {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, o: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let obj = self.obj();
            let hijos = hijos(&*obj);
            if o == gtk::Orientation::Horizontal {
                // Colapsada, solo las que se ven: con decenas de avisos de una app, medir el texto de todas
                // cuesta en cada apertura del centro.
                let desde = if self.expandida.get() {
                    0
                } else {
                    hijos.len().saturating_sub(APILADAS)
                };
                let (mut min, mut nat) = (0, 0);
                for h in &hijos[desde..] {
                    let (m, n, _, _) = h.measure(o, for_size);
                    min = min.max(m);
                    nat = nat.max(n);
                }
                return (min, nat, -1, -1);
            }
            if self.expandida.get() {
                let (mut min, mut nat) = (0, 0);
                for h in &hijos {
                    let (m, n, _, _) = h.measure(o, for_size);
                    min += m;
                    nat += n;
                }
                return (min, nat, -1, -1);
            }
            match hijos.last() {
                Some(frente) => {
                    let (_, nat, _, _) = frente.measure(o, for_size);
                    let alto = alto_colapsada(nat, hijos.len());
                    (alto, alto, -1, -1)
                }
                None => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, ancho: i32, _alto: i32, baseline: i32) {
            let obj = self.obj();
            let hijos = hijos(&*obj);
            if self.expandida.get() {
                let mut y = 0;
                for h in hijos.iter().rev() {
                    h.set_child_visible(true);
                    let (_, nat, _, _) = h.measure(gtk::Orientation::Vertical, ancho);
                    h.size_allocate(&gtk::Allocation::new(0, y, ancho, nat), baseline);
                    y += nat;
                }
                return;
            }
            let Some(frente) = hijos.last() else {
                return;
            };
            let (_, alto, _, _) = frente.measure(gtk::Orientation::Vertical, ancho);
            for (i, h) in hijos.iter().rev().enumerate() {
                if i >= APILADAS {
                    h.set_child_visible(false);
                    continue;
                }
                h.set_child_visible(true);
                let (x, y, escala) = ubicacion(i, ancho, alto);
                let propio = if i == 0 {
                    alto
                } else {
                    h.measure(gtk::Orientation::Vertical, ancho).0.max(alto)
                };
                let t = gsk::Transform::new()
                    .translate(&graphene::Point::new(x, y))
                    .scale(escala, escala * aplastado(alto, propio));
                h.allocate(ancho, propio, baseline, Some(t));
            }
        }
    }
}

glib::wrapper! {
    pub struct Pila(ObjectSubclass<imp::Pila>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Pila {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl Pila {
    pub fn agregar(&self, w: &impl IsA<gtk::Widget>) {
        w.set_parent(self);
    }

    pub fn expandida(&self) -> bool {
        self.imp().expandida.get()
    }

    pub fn set_expandida(&self, v: bool) {
        self.imp().expandida.set(v);
        self.queue_resize();
    }
}

fn hijos(w: &impl IsA<gtk::Widget>) -> Vec<gtk::Widget> {
    let mut v = Vec::new();
    let mut h = w.first_child();
    while let Some(x) = h {
        h = x.next_sibling();
        v.push(x);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asoman_ocho_pixeles_cada_una() {
        assert_eq!(alto_colapsada(60, 1), 60);
        assert_eq!(alto_colapsada(60, 2), 68);
        assert_eq!(alto_colapsada(60, 3), 76);
        // Más de tres no agrega: no se dibujan.
        assert_eq!(alto_colapsada(60, 9), 76);
    }

    #[test]
    fn ancladas_abajo_y_centradas() {
        assert_eq!(ubicacion(0, 300, 60), (0.0, 0.0, 1.0));
        let (x, y, e) = ubicacion(1, 300, 60);
        assert!((e - 0.95).abs() < 1e-6);
        assert!((x - 7.5).abs() < 1e-4);
        // El borde de abajo de la de atrás queda 8 px más abajo que el de la de adelante.
        assert!((y + 60.0 * e - 68.0).abs() < 1e-3);
        let (_, y, e) = ubicacion(2, 300, 60);
        assert!((y + 60.0 * e - 76.0).abs() < 1e-3);
    }

    #[test]
    fn una_de_atras_mas_alta_no_se_derrama() {
        assert_eq!(aplastado(60, 60), 1.0);
        assert_eq!(aplastado(60, 40), 1.0);
        // Una captura de 180 detrás de una de 120: se ve de 120, no se sale por abajo.
        let (_, y, e) = ubicacion(1, 300, 120);
        let visto = 180.0 * e * aplastado(120, 180);
        assert!((y + visto - 128.0).abs() < 1e-3);
    }
}
