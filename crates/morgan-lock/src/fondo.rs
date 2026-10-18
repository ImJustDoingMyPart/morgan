//! El blur va acá y no en el `filter` de la hoja: el de GTK mezcla con transparente más allá
//! del borde y oscurece los costados. Debajo de la imagen va una copia agrandada en `MARGEN`
//! por lado, que le da al blur algo parecido para mezclar, y se recorta a la salida.

use std::cell::OnceCell;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene};

const RADIO: f64 = 34.0;
/// Lo que se agranda la imagen por lado, para que el borde del blur quede afuera.
const MARGEN: f32 = 2.0 * RADIO as f32;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Fondo {
        pub textura: OnceCell<gdk::Texture>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Fondo {
        const NAME: &'static str = "MorganLockFondo";
        type Type = super::Fondo;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Fondo {}

    impl WidgetImpl for Fondo {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let Some(t) = self.textura.get() else {
                return;
            };
            let obj = self.obj();
            let (w, h) = (obj.width() as f32, obj.height() as f32);
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            snapshot.push_clip(&graphene::Rect::new(0.0, 0.0, w, h));
            snapshot.push_blur(RADIO);
            snapshot.append_texture(t, &cubrir(t, w, h, MARGEN));
            snapshot.append_texture(t, &cubrir(t, w, h, 0.0));
            snapshot.pop();
            snapshot.pop();
        }
    }
}

fn cubrir(t: &gdk::Texture, w: f32, h: f32, margen: f32) -> graphene::Rect {
    let (ancho, alto) = (w + 2.0 * margen, h + 2.0 * margen);
    let (tw, th) = (t.width() as f32, t.height() as f32);
    let escala = (ancho / tw).max(alto / th);
    let (iw, ih) = (tw * escala, th * escala);
    graphene::Rect::new((w - iw) / 2.0, (h - ih) / 2.0, iw, ih)
}

glib::wrapper! {
    pub struct Fondo(ObjectSubclass<imp::Fondo>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Fondo {
    pub fn new(textura: gdk::Texture) -> Self {
        let f: Fondo = glib::Object::new();
        let _ = f.imp().textura.set(textura);
        f.add_css_class("fondo");
        f.set_hexpand(true);
        f.set_vexpand(true);
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(t: (u32, u32), w: f32, h: f32, m: f32) -> (f32, f32, f32, f32) {
        let bytes = glib::Bytes::from_owned(vec![0u8; (t.0 * t.1 * 4) as usize]);
        let tex = gdk::MemoryTexture::new(
            t.0 as i32,
            t.1 as i32,
            gdk::MemoryFormat::R8g8b8a8,
            &bytes,
            t.0 as usize * 4,
        );
        let r = cubrir(tex.upcast_ref(), w, h, m);
        (r.x(), r.y(), r.width(), r.height())
    }

    #[test]
    fn misma_proporcion_cubre_justo() {
        assert_eq!(
            rect((1920, 1080), 1920.0, 1080.0, 0.0),
            (0.0, 0.0, 1920.0, 1080.0)
        );
    }

    #[test]
    fn cubre_sin_deformar_y_centrado() {
        // 16:9 en una salida más alta: escala por el alto y recorta los costados.
        let (x, y, w, h) = rect((1920, 1080), 1252.0, 1376.0, 0.0);
        assert_eq!(h, 1376.0);
        assert!((w / h - 1920.0 / 1080.0).abs() < 1e-4);
        assert!((x + w / 2.0 - 626.0).abs() < 1e-3);
        assert_eq!(y, 0.0);
    }

    #[test]
    fn el_margen_sobra_por_los_cuatro_lados() {
        let (x, y, w, h) = rect((1920, 1080), 1920.0, 1080.0, MARGEN);
        assert!(x <= -MARGEN && y <= -MARGEN);
        assert!(x + w >= 1920.0 + MARGEN && y + h >= 1080.0 + MARGEN);
    }
}
