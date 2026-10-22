//! Al pasar el umbral la acción se hace sin esperar a soltar: llevar la tarjeta hasta el
//! borde cuesta. Un tirón corto y rápido antes del umbral también cuenta.
//!
//! El arrastre arranca recién cuando el movimiento es más horizontal que vertical y pasa
//! unos píxeles: así el scroll del panel y el click siguen andando.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, graphene, gsk};

const ARRANQUE: f64 = 20.0;
pub const UMBRAL: f64 = 0.5;
const TIRON_MIN: f64 = 100.0;
const TIRON_VELOCIDAD: f64 = 2.0;
const ANIMACION: Duration = Duration::from_millis(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lado {
    Derecha,
    Izquierda,
}

fn lado(dx: f64) -> Lado {
    if dx > 0.0 {
        Lado::Derecha
    } else {
        Lado::Izquierda
    }
}

pub fn tiron(dx: f64, velocidad: f64) -> Option<Lado> {
    let mismo_lado = dx * velocidad > 0.0;
    (mismo_lado && dx.abs() >= TIRON_MIN && velocidad.abs() >= TIRON_VELOCIDAD).then(|| lado(dx))
}

pub fn decidir(dx: f64, ancho: f64) -> Option<Lado> {
    if ancho <= 0.0 || dx.abs() < ancho * UMBRAL {
        return None;
    }
    Some(if dx > 0.0 {
        Lado::Derecha
    } else {
        Lado::Izquierda
    })
}

pub fn asomo(dx: f64, ancho: f64) -> f64 {
    if ancho <= 0.0 {
        return 0.0;
    }
    (dx.abs() / (ancho * UMBRAL)).min(1.0)
}

pub fn opacidad(dx: f64, ancho: f64) -> f64 {
    if ancho <= 0.0 {
        return 1.0;
    }
    let u = ancho * UMBRAL;
    let pasado = (dx.abs() - u).max(0.0) / (ancho - u).max(1.0);
    (1.0 - pasado).clamp(0.0, 1.0)
}

pub fn avance(micros: i64) -> f64 {
    (micros as f64 / crate::animaciones::dur(ANIMACION).as_micros() as f64).clamp(0.0, 1.0)
}

pub fn suavizar(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

pub struct Accion {
    pub glifo: &'static str,
    pub texto: &'static str,
    pub se_va: bool,
    pub f: Rc<dyn Fn()>,
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Deslizable {
        pub dx: Cell<f64>,
        pub arrastrando: Cell<bool>,
        pub hecho: Cell<bool>,
        pub ultimo: Cell<(f64, u32)>,
        pub velocidad: Cell<f64>,
        pub animando: RefCell<Option<gtk::TickCallbackId>>,
        pub hijo: RefCell<Option<gtk::Widget>>,
        pub fondo: RefCell<Option<gtk::Box>>,
        pub pistas: RefCell<[Option<gtk::Label>; 2]>,
        pub acciones: RefCell<[Option<Accion>; 2]>,
        pub puede: RefCell<Option<Box<dyn Fn() -> bool>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Deslizable {
        const NAME: &'static str = "MorganDeslizable";
        type Type = super::Deslizable;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Deslizable {
        fn dispose(&self) {
            if let Some(id) = self.animando.take() {
                id.remove();
            }
            while let Some(h) = self.obj().first_child() {
                h.unparent();
            }
        }
    }

    impl WidgetImpl for Deslizable {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, o: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            match self.hijo.borrow().as_ref() {
                Some(h) => h.measure(o, for_size),
                None => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, ancho: i32, alto: i32, baseline: i32) {
            if let Some(f) = self.fondo.borrow().as_ref() {
                f.allocate(ancho, alto, baseline, None);
            }
            if let Some(h) = self.hijo.borrow().as_ref() {
                let t = gsk::Transform::new()
                    .translate(&graphene::Point::new(self.dx.get() as f32, 0.0));
                h.allocate(ancho, alto, baseline, Some(t));
            }
        }

        fn snapshot(&self, s: &gtk::Snapshot) {
            let obj = self.obj();
            let ancho = f64::from(obj.width());
            let dx = self.dx.get();
            if dx != 0.0
                && let Some(f) = self.fondo.borrow().as_ref()
            {
                s.push_opacity(asomo(dx, ancho));
                obj.snapshot_child(f, s);
                s.pop();
            }
            // GTK 4.24 arma el nodo de opacidad aun en 1, y ese nodo aísla el fondo: la región
            // de `backdrop-filter` que se le declara a Hyprland queda vacía y la tarjeta, sin blur.
            if let Some(h) = self.hijo.borrow().as_ref() {
                let o = opacidad(dx, ancho);
                if o < 1.0 {
                    s.push_opacity(o);
                    obj.snapshot_child(h, s);
                    s.pop();
                } else {
                    obj.snapshot_child(h, s);
                }
            }
        }
    }
}

glib::wrapper! {
    pub struct Deslizable(ObjectSubclass<imp::Deslizable>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Deslizable {
    pub fn new(
        hijo: &impl IsA<gtk::Widget>,
        derecha: Option<Accion>,
        izquierda: Option<Accion>,
    ) -> Self {
        let d: Deslizable = glib::Object::new();
        d.add_css_class("deslizable");
        let imp = d.imp();

        let fondo = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        fondo.add_css_class("deslizable-fondo");
        let pista = |a: &Option<Accion>, xalign: f32| {
            let l = gtk::Label::new(
                a.as_ref()
                    .map(|a| format!("{}  {}", a.glifo, a.texto))
                    .as_deref(),
            );
            l.set_hexpand(true);
            l.set_xalign(xalign);
            l.set_visible(false);
            fondo.append(&l);
            l
        };
        let p_der = pista(&derecha, 0.0);
        let p_izq = pista(&izquierda, 1.0);
        fondo.set_parent(&d);
        hijo.set_parent(&d);
        imp.fondo.replace(Some(fondo));
        imp.hijo.replace(Some(hijo.clone().upcast()));
        imp.pistas.replace([Some(p_der), Some(p_izq)]);
        imp.acciones.replace([derecha, izquierda]);
        d.gesto();
        d
    }

    /// Escucha en la fase de captura, así le gana a las tarjetas de adentro, que tienen el suyo
    /// para cuando el grupo está abierto.
    pub fn de_grupo(self, puede: impl Fn() -> bool + 'static) -> Self {
        self.imp().puede.replace(Some(Box::new(puede)));
        if let Some(g) = self
            .observe_controllers()
            .into_iter()
            .filter_map(|c| c.ok()?.downcast::<gtk::GestureDrag>().ok())
            .next()
        {
            g.set_propagation_phase(gtk::PropagationPhase::Capture);
        }
        self
    }

    fn accion(&self, lado: Lado) -> bool {
        self.imp().acciones.borrow()[lado as usize].is_some()
    }

    fn gesto(&self) {
        let g = gtk::GestureDrag::new();
        g.set_button(gtk::gdk::BUTTON_PRIMARY);
        let yo = self.clone();
        g.connect_drag_begin(move |_, _, _| {
            let imp = yo.imp();
            imp.hecho.set(false);
            imp.velocidad.set(0.0);
            imp.ultimo.set((0.0, 0));
        });
        let yo = self.clone();
        g.connect_drag_update(move |g, dx, dy| {
            let imp = yo.imp();
            if imp.hecho.get() {
                return;
            }
            let t = g.current_event_time();
            let (dx0, t0) = imp.ultimo.replace((dx, t));
            if t0 != 0 && t > t0 {
                imp.velocidad.set((dx - dx0) / f64::from(t - t0));
            }
            if !imp.arrastrando.get() {
                if dx.abs() < ARRANQUE || dx.abs() < dy.abs() {
                    return;
                }
                if !yo.accion(lado(dx)) {
                    return;
                }
                if imp.puede.borrow().as_ref().is_some_and(|f| !f()) {
                    return;
                }
                imp.arrastrando.set(true);
                g.set_state(gtk::EventSequenceState::Claimed);
                if let Some(id) = imp.animando.take() {
                    id.remove();
                }
            }
            // Hacia un lado sin acción no se mueve.
            let dx = match (
                dx > 0.0,
                yo.accion(Lado::Derecha),
                yo.accion(Lado::Izquierda),
            ) {
                (true, false, _) | (false, _, false) => 0.0,
                _ => dx,
            };
            yo.poner(dx);
            // Pasado el umbral se hace ya, sin esperar a soltar.
            if let Some(l) = decidir(dx, f64::from(yo.width())) {
                imp.hecho.set(true);
                imp.arrastrando.set(false);
                yo.soltar(l);
            }
        });
        let yo = self.clone();
        g.connect_drag_end(move |_, _, _| {
            let imp = yo.imp();
            if !imp.arrastrando.replace(false) {
                return;
            }
            let dx = imp.dx.get();
            match tiron(dx, imp.velocidad.get()).filter(|l| yo.accion(*l)) {
                Some(l) => yo.soltar(l),
                None => yo.animar(0.0, None),
            }
        });
        // Si otro control se queda con la secuencia, no llega `drag-end`: sin esto la
        // tarjeta quedaría corrida.
        let yo = self.clone();
        g.connect_cancel(move |_, _| {
            if yo.imp().arrastrando.replace(false) {
                yo.animar(0.0, None);
            }
        });
        // Escondida a mitad de camino (se cerró el centro): la próxima vez, en su lugar.
        self.connect_unmap(|w| {
            let imp = w.imp();
            imp.arrastrando.set(false);
            if let Some(id) = imp.animando.take() {
                id.remove();
            }
            if imp.dx.get() != 0.0 {
                w.poner(0.0);
            }
        });
        self.add_controller(g);
    }

    fn poner(&self, dx: f64) {
        let imp = self.imp();
        imp.dx.set(dx);
        let pistas = imp.pistas.borrow();
        if let [Some(der), Some(izq)] = &*pistas {
            der.set_visible(dx > 0.0);
            izq.set_visible(dx < 0.0);
        }
        self.queue_allocate();
        self.queue_draw();
    }

    fn soltar(&self, lado: Lado) {
        let (se_va, f) = {
            let acciones = self.imp().acciones.borrow();
            let Some(a) = &acciones[lado as usize] else {
                return;
            };
            (a.se_va, a.f.clone())
        };
        if se_va {
            let ancho = f64::from(self.width());
            let destino = if lado == Lado::Derecha { ancho } else { -ancho };
            self.animar(destino, Some(f));
        } else {
            f();
            self.animar(0.0, None);
        }
    }

    fn animar(&self, destino: f64, al_final: Option<Rc<dyn Fn()>>) {
        let imp = self.imp();
        if let Some(id) = imp.animando.take() {
            id.remove();
        }
        let desde = imp.dx.get();
        let inicio: Cell<Option<i64>> = Cell::new(None);
        let id = self.add_tick_callback(move |w, reloj| {
            let ahora = reloj.frame_time();
            // El primer cuadro marca el inicio (`Cell::get` copia: hay que guardarlo).
            let t0 = match inicio.get() {
                Some(t0) => t0,
                None => {
                    inicio.set(Some(ahora));
                    ahora
                }
            };
            let t = avance(ahora - t0);
            w.poner(desde + (destino - desde) * suavizar(t));
            if t < 1.0 {
                return glib::ControlFlow::Continue;
            }
            w.imp().animando.take();
            if let Some(f) = &al_final {
                f();
            }
            glib::ControlFlow::Break
        });
        imp.animando.replace(Some(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_pasado_el_umbral() {
        assert_eq!(decidir(0.0, 300.0), None);
        assert_eq!(decidir(140.0, 300.0), None);
        assert_eq!(decidir(150.0, 300.0), Some(Lado::Derecha));
        assert_eq!(decidir(-150.0, 300.0), Some(Lado::Izquierda));
        assert_eq!(decidir(50.0, 0.0), None);
    }

    #[test]
    fn asoma_y_se_apaga() {
        assert_eq!(asomo(0.0, 300.0), 0.0);
        assert!((asomo(75.0, 300.0) - 0.5).abs() < 1e-9);
        assert_eq!(asomo(-200.0, 300.0), 1.0);
        assert_eq!(opacidad(140.0, 300.0), 1.0);
        assert_eq!(opacidad(300.0, 300.0), 0.0);
        assert!(opacidad(-200.0, 300.0) < 1.0);
    }

    #[test]
    fn un_tiron_corto_y_rapido_cuenta() {
        assert_eq!(tiron(110.0, 2.5), Some(Lado::Derecha));
        assert_eq!(tiron(-110.0, -2.1), Some(Lado::Izquierda));
        // Lento, corto o volviendo: no.
        assert_eq!(tiron(110.0, 1.5), None);
        assert_eq!(tiron(80.0, 4.0), None);
        assert_eq!(tiron(110.0, -2.5), None);
    }

    #[test]
    fn la_animacion_avanza_y_termina() {
        assert_eq!(avance(0), 0.0);
        assert!((avance(90_000) - 0.5).abs() < 1e-9);
        assert_eq!(avance(180_000), 1.0);
        assert_eq!(avance(500_000), 1.0);
        assert_eq!(suavizar(0.0), 0.0);
        assert_eq!(suavizar(1.0), 1.0);
        assert!(suavizar(0.5) > 0.5);
    }
}
