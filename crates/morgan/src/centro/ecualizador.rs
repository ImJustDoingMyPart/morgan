//! El motor es cava (no se reimplementa un analizador). Corre solo con el centro abierto y el
//! reproductor sonando. cava escucha el monitor de la salida, no el micrófono: el indicador de
//! privacidad no lo cuenta.
//!
//! El widget no pide tamaño (la tarjeta la mide su contenido) y cada cuadro solo lo vuelve a
//! dibujar: el panel no se vuelve a medir.

use std::cell::{Cell, RefCell};

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gio, graphene, gsk};

pub const BARRAS: usize = 48;
const SEPARACION: f32 = 3.0;
const RANGO: f64 = 100.0;
const ALTURA: f32 = 0.75;

/// Solo claves del `example_files/config` de cava 1.0.0.
pub fn config() -> String {
    format!(
        "[general]\n\
         framerate = 30\n\
         bars = {BARRAS}\n\
         autosens = 1\n\
         lower_cutoff_freq = 50\n\
         higher_cutoff_freq = 10000\n\
         \n\
         [input]\n\
         method = pipewire\n\
         source = auto\n\
         \n\
         [output]\n\
         method = raw\n\
         raw_target = /dev/stdout\n\
         data_format = ascii\n\
         ascii_max_range = {RANGO}\n\
         bar_delimiter = 59\n\
         frame_delimiter = 10\n\
         channels = stereo\n\
         \n\
         [smoothing]\n\
         noise_reduction = 77\n"
    )
}

pub fn interpretar(linea: &str) -> Option<Vec<f64>> {
    let niveles = linea
        .trim()
        .split(';')
        .filter(|v| !v.is_empty())
        .map(|v| {
            v.trim()
                .parse::<f64>()
                .ok()
                .map(|v| (v / RANGO).clamp(0.0, 1.0))
        })
        .collect::<Option<Vec<f64>>>()?;
    (niveles.len() == BARRAS).then_some(niveles)
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Ecualizador {
        pub niveles: RefCell<Vec<f64>>,
        pub fin: RefCell<Option<gtk::Box>>,
        pub cava: RefCell<Option<gio::Subprocess>>,
        /// Sube con cada arranque y cada parada: una lectura vieja no pisa la nueva.
        pub vuelta: Cell<u64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Ecualizador {
        const NAME: &'static str = "MorganEcualizador";
        type Type = super::Ecualizador;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Ecualizador {
        fn dispose(&self) {
            if let Some(p) = self.cava.take() {
                p.force_exit();
            }
            if let Some(f) = self.fin.take() {
                f.unparent();
            }
        }
    }

    impl WidgetImpl for Ecualizador {
        fn measure(&self, _o: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }

        fn size_allocate(&self, _ancho: i32, _alto: i32, _baseline: i32) {
            if let Some(f) = self.fin.borrow().as_ref() {
                f.allocate(0, 0, -1, None);
            }
        }

        fn snapshot(&self, s: &gtk::Snapshot) {
            let obj = self.obj();
            let (ancho, alto) = (obj.width() as f32, obj.height() as f32);
            let niveles = self.niveles.borrow();
            let n = niveles.len();
            if n == 0 || ancho <= 0.0 || alto <= 0.0 {
                return;
            }
            let principio = obj.color();
            let fin = self.fin.borrow().as_ref().map_or(principio, |f| f.color());
            let todo = graphene::Rect::new(0.0, 0.0, ancho, alto);
            let paradas = [
                gsk::ColorStop::new(0.0, principio),
                gsk::ColorStop::new(1.0, fin),
            ];
            let w = ((ancho - SEPARACION * (n as f32 - 1.0)) / n as f32).max(1.0);
            for (i, v) in niveles.iter().enumerate() {
                let h = (*v as f32 * alto * ALTURA).max(w);
                let x = i as f32 * (w + SEPARACION);
                let barra = graphene::Rect::new(x, alto - h, w, h);
                let radio = graphene::Size::new(w / 2.0, w / 2.0);
                s.push_rounded_clip(&gsk::RoundedRect::new(barra, radio, radio, radio, radio));
                // El degradé es el de toda la tarjeta: cada barra muestra su tramo.
                s.append_linear_gradient(
                    &todo,
                    &graphene::Point::new(0.0, 0.0),
                    &graphene::Point::new(ancho, 0.0),
                    &paradas,
                );
                s.pop();
            }
        }
    }
}

glib::wrapper! {
    pub struct Ecualizador(ObjectSubclass<imp::Ecualizador>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Ecualizador {
    pub fn new() -> Self {
        let e: Ecualizador = glib::Object::new();
        e.add_css_class("ecualizador");
        e.add_css_class("quieto");
        e.set_hexpand(true);
        e.set_vexpand(true);
        e.set_can_target(false);
        let fin = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        fin.add_css_class("fin");
        fin.set_parent(&e);
        e.imp().fin.replace(Some(fin));
        e
    }

    fn poner(&self, niveles: Vec<f64>) {
        self.imp().niveles.replace(niveles);
        self.queue_draw();
    }

    pub fn encender(&self) {
        let imp = self.imp();
        if imp.cava.borrow().is_some() {
            return;
        }
        let ruta = glib::user_runtime_dir().join("morgan").join("cava.conf");
        if let Err(e) = std::fs::create_dir_all(ruta.parent().expect("tiene carpeta"))
            .and_then(|_| std::fs::write(&ruta, config()))
        {
            eprintln!("morgan: ecualizador: {e}");
            return;
        }
        let proceso = match gio::Subprocess::newv(
            &["cava".as_ref(), "-p".as_ref(), ruta.as_os_str()],
            gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE,
        ) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("morgan: ecualizador: cava: {e}");
                return;
            }
        };
        let Some(salida) = proceso.stdout_pipe() else {
            return;
        };
        imp.cava.replace(Some(proceso));
        let vuelta = imp.vuelta.get() + 1;
        imp.vuelta.set(vuelta);
        self.remove_css_class("quieto");
        let lector = gio::DataInputStream::new(&salida);
        let yo = self.downgrade();
        glib::MainContext::default().spawn_local(async move {
            while let Ok(Some(linea)) = lector.read_line_utf8_future(glib::Priority::DEFAULT).await
            {
                let Some(e) = yo.upgrade() else { return };
                if e.imp().vuelta.get() != vuelta {
                    return;
                }
                if let Some(n) = interpretar(&linea) {
                    e.poner(n);
                }
            }
        });
    }

    pub fn apagar(&self) {
        let imp = self.imp();
        imp.vuelta.set(imp.vuelta.get() + 1);
        if let Some(p) = imp.cava.take() {
            p.force_exit();
        }
        self.add_css_class("quieto");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_renglon_de_cava() {
        let renglon = |v: &[u32]| v.iter().map(|x| format!("{x};")).collect::<String>();
        let mut v = vec![0u32; BARRAS];
        v[0] = 50;
        v[1] = 250;
        let n = interpretar(&(renglon(&v) + "\n")).unwrap();
        assert_eq!(n.len(), BARRAS);
        assert_eq!(n[0], 0.5);
        // Lo que se pasa del rango se recorta.
        assert_eq!(n[1], 1.0);
        // Sin el ; final también.
        assert!(interpretar(renglon(&v).trim_end_matches(';')).is_some());
        // Cantidad equivocada o basura: nada.
        assert!(interpretar("1;2;3;").is_none());
        assert!(interpretar(&renglon(&[1; BARRAS + 1])).is_none());
        assert!(interpretar(&"a;".repeat(BARRAS)).is_none());
        assert!(interpretar("").is_none());
    }

    #[test]
    fn la_config_pide_lo_que_se_lee() {
        let c = config();
        assert!(c.contains(&format!("bars = {BARRAS}")));
        assert!(c.contains("method = raw"));
        assert!(c.contains("data_format = ascii"));
        assert!(c.contains("ascii_max_range = 100"));
        assert!(c.contains("channels = stereo"));
    }
}
