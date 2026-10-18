//! Va encima del `gtk::Entry`, que sigue recibiendo el teclado y es invisible: este widget no
//! toma eventos, solo dibuja. Anima con el frame clock y únicamente mientras algo se mueve; con
//! el campo vacío y quieto no hay ni un cuadro.

use std::cell::{Cell, RefCell};
use std::f32::consts::PI;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk, pango};
use morgan_ipc::bloqueo::{Bloqueo, Escritura};

use crate::caracteres::Caracteres;

pub const ANCHO: f32 = 344.0;
pub const ALTO: f32 = 52.0;
pub(crate) const BORDE: f32 = 2.0;
const RADIO: f32 = 4.0;

/// La onda va 4 veces más lenta: a velocidad normal no se lee. El fallo, el latido y el
/// acierto, 2 veces: tienen que caber en lo que duran esos estados.
const LENTO_TECLA: f32 = 4.0;
const LENTO_EVENTO: f32 = 2.0;
const LENTO_FALLO: f32 = 1.4;
const FALLO_ONDA: f32 = 0.7;
const FALLO_TEXTO: f32 = 0.8;
const FALLO_TEXTO_FILA: f32 = 0.25;
const ONDA_X0: f32 = 18.0;
const ONDA_X1: f32 = ANCHO - 18.0;

#[derive(Clone, Copy, Debug)]
pub struct Paleta {
    pub primary: gdk::RGBA,
    pub tertiary: gdk::RGBA,
    pub on_surface: gdk::RGBA,
    pub error: gdk::RGBA,
    pub surface: gdk::RGBA,
    pub surface_container: gdk::RGBA,
}

impl Paleta {
    pub fn desde_css(css: &str) -> Self {
        let rol = |nombre: &str, defecto: &str| {
            css.lines()
                .filter_map(|l| l.trim().strip_prefix("@define-color "))
                .filter_map(|l| l.trim().split_once(char::is_whitespace))
                .find(|(n, _)| *n == nombre)
                .and_then(|(_, v)| gdk::RGBA::parse(v.trim().trim_end_matches(';').trim()).ok())
                .unwrap_or_else(|| gdk::RGBA::parse(defecto).expect("color de fábrica"))
        };
        Paleta {
            primary: rol("primary", "#9fcafd"),
            tertiary: rol("tertiary", "#d6bee5"),
            on_surface: rol("on_surface", "#e1e2e8"),
            error: rol("error", "#ffb4ab"),
            surface: rol("surface", "#101418"),
            surface_container: rol("surface_container", "#1d2024"),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Modo {
    Normal,
    Verificando,
    Fallo(u32),
    Saliendo,
}

#[derive(Clone, Copy, PartialEq)]
enum Tipo {
    Sale,
    Entra,
    Fallo,
    Acierto,
}

impl Tipo {
    fn lento(self) -> f32 {
        match self {
            Tipo::Sale | Tipo::Entra => LENTO_TECLA,
            Tipo::Fallo => LENTO_FALLO,
            Tipo::Acierto => LENTO_EVENTO,
        }
    }
}

struct Impulso {
    t0: i64,
    tipo: Tipo,
    amp: f32,
}

struct Chispa {
    t0: i64,
    dir: f32,
    s0: f32,
    fuerza: f32,
}

/// Una transición lineal entre dos valores, para no depender del dt del cuadro.
#[derive(Clone, Copy)]
struct Tramo<T> {
    de: T,
    a: T,
    t0: i64,
    dura: i64,
}

struct Estado {
    paleta: Paleta,
    estilo: Bloqueo,
    fila: Caracteres,
    t_cuadro: i64,
    modo: Modo,
    t_modo: i64,
    t_fallo: i64,
    t_acierto: i64,
    n: usize,
    mayus: bool,
    impulsos: Vec<Impulso>,
    chispas: Vec<Chispa>,
    lado: f32,
    anillo: Tramo<[gdk::RGBA; 2]>,
    visible: Tramo<f32>,
}

const MS: i64 = 1000;

fn ahora() -> i64 {
    glib::monotonic_time()
}

pub(crate) fn seg(us: i64) -> f32 {
    us as f32 / 1e6
}

pub(crate) fn acotar(x: f32, a: f32, b: f32) -> f32 {
    x.max(a).min(b)
}

pub(crate) fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - acotar(t, 0.0, 1.0)).powi(3)
}

pub(crate) fn mezclar(a: &gdk::RGBA, b: &gdk::RGBA, t: f32) -> gdk::RGBA {
    let m = |x: f32, y: f32| x + (y - x) * t;
    gdk::RGBA::new(
        m(a.red(), b.red()),
        m(a.green(), b.green()),
        m(a.blue(), b.blue()),
        m(a.alpha(), b.alpha()),
    )
}

pub(crate) fn con_alfa(c: &gdk::RGBA, a: f32) -> gdk::RGBA {
    gdk::RGBA::new(c.red(), c.green(), c.blue(), acotar(a, 0.0, 1.0))
}

impl Tramo<f32> {
    fn valor(&self, t: i64) -> f32 {
        let f = acotar((t - self.t0) as f32 / self.dura.max(1) as f32, 0.0, 1.0);
        self.de + (self.a - self.de) * f
    }
}

impl Tramo<[gdk::RGBA; 2]> {
    fn valor(&self, t: i64) -> [gdk::RGBA; 2] {
        let f = acotar((t - self.t0) as f32 / self.dura.max(1) as f32, 0.0, 1.0);
        [
            mezclar(&self.de[0], &self.a[0], f),
            mezclar(&self.de[1], &self.a[1], f),
        ]
    }
}

impl Estado {
    fn new(paleta: Paleta, estilo: Bloqueo) -> Self {
        let par = [paleta.primary, paleta.tertiary];
        Estado {
            paleta,
            estilo,
            fila: Caracteres::default(),
            t_cuadro: ahora(),
            modo: Modo::Normal,
            t_modo: 0,
            t_fallo: i64::MIN / 2,
            t_acierto: i64::MIN / 2,
            n: 0,
            mayus: false,
            impulsos: Vec::new(),
            chispas: Vec::new(),
            lado: 1.0,
            anillo: Tramo {
                de: par,
                a: par,
                t0: 0,
                dura: 300 * MS,
            },
            visible: Tramo {
                de: 0.0,
                a: 0.0,
                t0: 0,
                dura: 1,
            },
        }
    }

    /// El fallo manda sobre Bloq Mayús, y Bloq Mayús sobre la verificación.
    fn objetivo(&self) -> [gdk::RGBA; 2] {
        let p = &self.paleta;
        match self.modo {
            Modo::Fallo(_) => [p.error, p.error],
            _ if self.mayus => [p.on_surface, p.on_surface],
            Modo::Verificando => [p.tertiary, p.primary],
            _ => [p.primary, p.tertiary],
        }
    }

    fn rehacer_anillo(&mut self, t: i64) {
        let a = self.objetivo();
        if a != self.anillo.a {
            self.anillo = Tramo {
                de: self.anillo.valor(t),
                a,
                t0: t,
                dura: 300 * MS,
            };
        }
    }

    fn cambiar(&mut self, m: Modo, t: i64) {
        self.modo = m;
        self.t_modo = t;
        self.rehacer_anillo(t);
    }

    fn onda(&self) -> bool {
        self.estilo.escritura == Escritura::Onda
    }

    fn latido(&self) -> bool {
        self.modo == Modo::Verificando && self.estilo.verificar.late()
    }

    fn quiere_onda(&self, t: i64) -> bool {
        if !self.onda() {
            return false;
        }
        self.n > 0
            || self.latido()
            || self.modo == Modo::Saliendo
            || self
                .impulsos
                .iter()
                .any(|p| p.tipo == Tipo::Fallo && seg(t - p.t0) < FALLO_ONDA)
    }

    fn actualizar_visible(&mut self, t: i64) {
        let quiere = if self.quiere_onda(t) { 1.0 } else { 0.0 };
        if quiere != self.visible.a {
            let dura = if quiere > 0.5 { 150 } else { 300 };
            self.visible = Tramo {
                de: self.visible.valor(t),
                a: quiere,
                t0: t,
                dura: dura * MS,
            };
        }
    }

    fn limpiar(&mut self, t: i64) {
        self.impulsos.retain(|p| {
            let lento = p.tipo.lento();
            seg(t - p.t0) / lento < 2.5
        });
        self.chispas.retain(|c| t - c.t0 < 700 * MS);
        self.fila.limpiar(t);
    }

    fn chispa(&mut self, t: i64, s0: f32, fuerza: f32) {
        if self.estilo.chispa {
            self.chispas.push(Chispa {
                t0: t,
                dir: self.lado,
                s0,
                fuerza,
            });
        }
        self.lado = -self.lado;
    }

    fn animando(&self, t: i64) -> bool {
        (self.n > 0 && self.onda())
            || (self.fila.hay() && self.estilo.escritura != Escritura::Fijo)
            || !self.impulsos.is_empty()
            || !self.chispas.is_empty()
            || matches!(self.modo, Modo::Verificando | Modo::Saliendo)
            || t - self.anillo.t0 < self.anillo.dura
            || t - self.visible.t0 < self.visible.dura
            || t - self.t_fallo < 1500 * MS
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Lienzo {
        pub(super) estado: RefCell<Option<Estado>>,
        pub(super) corriendo: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Lienzo {
        const NAME: &'static str = "MorganLockLienzo";
        type Type = super::Lienzo;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Lienzo {}

    impl WidgetImpl for Lienzo {
        fn measure(&self, o: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let n = if o == gtk::Orientation::Horizontal {
                ANCHO
            } else {
                ALTO
            } as i32;
            (n, n, -1, -1)
        }

        fn snapshot(&self, s: &gtk::Snapshot) {
            let obj = self.obj();
            if let Some(e) = self.estado.borrow_mut().as_mut() {
                dibujar(e, s, obj.upcast_ref());
            }
        }
    }
}

glib::wrapper! {
    pub struct Lienzo(ObjectSubclass<imp::Lienzo>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Lienzo {
    pub fn new(paleta: Paleta, estilo: Bloqueo) -> Self {
        let l: Lienzo = glib::Object::new();
        l.set_can_target(false);
        l.set_can_focus(false);
        *l.imp().estado.borrow_mut() = Some(Estado::new(paleta, estilo));
        l
    }

    fn con(&self, f: impl FnOnce(&mut Estado, i64)) {
        if let Some(e) = self.imp().estado.borrow_mut().as_mut() {
            f(e, ahora());
        }
        self.despertar();
    }

    fn despertar(&self) {
        self.queue_draw();
        if self.imp().corriendo.replace(true) {
            return;
        }
        self.add_tick_callback(|w, _| {
            w.queue_draw();
            let sigue = w
                .imp()
                .estado
                .borrow()
                .as_ref()
                .is_some_and(|e| e.animando(ahora()));
            if sigue {
                glib::ControlFlow::Continue
            } else {
                w.imp().corriendo.set(false);
                glib::ControlFlow::Break
            }
        });
    }

    pub fn agregar(&self, total: usize) {
        self.con(|e, t| {
            e.n = total;
            if e.onda() {
                e.impulsos.push(Impulso {
                    t0: t,
                    tipo: Tipo::Sale,
                    amp: 7.0 + 3.0 * azar(t),
                });
            } else {
                e.fila.contar(total, t);
            }
            e.chispa(t, (ANCHO - 2.0) / 2.0, 1.0);
            if matches!(e.modo, Modo::Fallo(_)) {
                e.cambiar(Modo::Normal, t);
            }
        });
    }

    pub fn quitar(&self, total: usize, todo: bool) {
        self.con(|e, t| {
            e.n = total;
            if e.onda() {
                e.impulsos.push(Impulso {
                    t0: t,
                    tipo: Tipo::Entra,
                    amp: if todo { 10.0 } else { 6.0 },
                });
            } else {
                e.fila.contar(total, t);
            }
            let abajo = (ANCHO - 2.0) + (ALTO - 2.0) + (ANCHO - 2.0) / 2.0;
            e.chispa(t, abajo, 0.6);
        });
    }

    /// Si lo tipeado late mientras verifica, los caracteres se quedan (la onda late sola).
    pub fn vaciar(&self) {
        self.con(|e, t| {
            e.n = 0;
            if !e.estilo.verificar.late() {
                e.fila.vaciar(t);
            }
        });
    }

    pub fn normal(&self) {
        self.con(|e, t| e.cambiar(Modo::Normal, t));
    }

    pub fn verificando(&self) {
        self.con(|e, t| e.cambiar(Modo::Verificando, t));
    }

    pub fn fallo(&self, n: u32) {
        self.con(|e, t| {
            e.t_fallo = t;
            e.cambiar(Modo::Fallo(n), t);
            if !e.estilo.reaccion {
                e.fila.vaciar(t);
            } else if e.onda() {
                e.impulsos.push(Impulso {
                    t0: t,
                    tipo: Tipo::Fallo,
                    amp: 15.0,
                });
            } else {
                e.fila.caer(t);
            }
        });
    }

    pub fn acierto(&self) {
        self.con(|e, t| {
            e.t_acierto = t;
            e.cambiar(Modo::Saliendo, t);
            if e.onda() {
                e.impulsos.push(Impulso {
                    t0: t,
                    tipo: Tipo::Acierto,
                    amp: 6.0,
                });
            }
            e.fila.brillar();
        });
    }

    pub fn mayus(&self, si: bool) {
        self.con(|e, t| {
            e.mayus = si;
            e.rehacer_anillo(t);
        });
    }
}

/// Un número en [0, 1) a partir del tiempo: alcanza para variar el golpe de cada tecla.
pub(crate) fn azar(t: i64) -> f32 {
    let x = (t as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (x >> 40) as f32 / (1u64 << 24) as f32
}


fn rect(x: f32, y: f32, w: f32, h: f32) -> graphene::Rect {
    graphene::Rect::new(x, y, w, h)
}

fn redondo(x: f32, y: f32, w: f32, h: f32, r: f32) -> gsk::RoundedRect {
    gsk::RoundedRect::from_rect(rect(x, y, w, h), r)
}

const BX: f32 = 1.0;
const BY: f32 = 1.0;
const BW: f32 = ANCHO - 2.0;
const BH: f32 = ALTO - 2.0;
const PERIMETRO: f32 = 2.0 * (BW + BH);

fn punto(s: f32) -> (f32, f32) {
    let mut s = s.rem_euclid(PERIMETRO);
    if s < BW {
        return (BX + s, BY);
    }
    s -= BW;
    if s < BH {
        return (BX + BW, BY + s);
    }
    s -= BH;
    if s < BW {
        return (BX + BW - s, BY + BH);
    }
    s -= BW;
    (BX, BY + BH - s)
}

/// La diagonal va suavizada para que la chispa no salte de color.
fn color_en(x: f32, y: f32, par: &[gdk::RGBA; 2]) -> gdk::RGBA {
    let t = acotar((x / ANCHO + y / ALTO) / 2.0, 0.0, 1.0);
    mezclar(&par[0], &par[1], ease_out(t))
}

fn circulo(s: &gtk::Snapshot, x: f32, y: f32, r: f32, c: &gdk::RGBA) {
    let b = gsk::PathBuilder::new();
    b.add_circle(&graphene::Point::new(x, y), r);
    s.append_fill(&b.to_path(), gsk::FillRule::Winding, c);
}

fn cometa(
    s: &gtk::Snapshot,
    cabeza: f32,
    dir: f32,
    largo: f32,
    alfa: f32,
    par: &[gdk::RGBA; 2],
    grosor: f32,
) {
    if alfa <= 0.01 {
        return;
    }
    let blanco = gdk::RGBA::WHITE;
    let pasos = (largo / 2.0).round().max(6.0) as usize;
    for i in (0..=pasos).rev() {
        let f = i as f32 / pasos as f32;
        let (x, y) = punto(cabeza - dir * f * largo);
        let c = mezclar(&color_en(x, y, par), &blanco, 0.45 * (1.0 - f));
        let a = alfa * (1.0 - f).powf(1.6);
        let r = grosor * (1.0 - 0.5 * f);
        circulo(s, x, y, r * 2.6, &con_alfa(&c, a * 0.18));
        circulo(s, x, y, r, &con_alfa(&c, a));
    }
}

fn sacudida(t: f32) -> f32 {
    const K: [(f32, f32); 7] = [
        (0.0, 0.0),
        (0.14, -11.0),
        (0.30, 9.0),
        (0.46, -6.0),
        (0.62, 4.0),
        (0.80, -2.0),
        (1.0, 0.0),
    ];
    if !(0.0..1.0).contains(&t) {
        return 0.0;
    }
    let i = K
        .iter()
        .rposition(|(k, _)| *k <= t)
        .unwrap_or(0)
        .min(K.len() - 2);
    let ((t0, v0), (t1, v1)) = (K[i], K[i + 1]);
    let f = (t - t0) / (t1 - t0);
    v0 + (v1 - v0) * (f * f * (3.0 - 2.0 * f))
}

fn dibujar(e: &mut Estado, s: &gtk::Snapshot, w: &gtk::Widget) {
    let t = ahora();
    let dt = seg(t - e.t_cuadro).min(0.064);
    e.t_cuadro = t;
    e.limpiar(t);
    e.actualizar_visible(t);
    let p = e.paleta;
    let par = e.anillo.valor(t);
    let estilo = e.estilo;

    s.save();
    if estilo.sacudida {
        s.translate(&graphene::Point::new(
            sacudida(seg(t - e.t_fallo) / 0.42),
            0.0,
        ));
    }

    let (alfa, extra) = match e.modo {
        Modo::Fallo(_) if estilo.rojo => {
            let k = (-seg(t - e.t_fallo) / 0.38).exp();
            (0.45 + 0.55 * k, k)
        }
        Modo::Saliendo if estilo.destello => {
            let k = (-(seg(t - e.t_acierto) - 0.2).max(0.0) / 0.3).exp();
            (0.45 + 0.55 * k, 0.6 * k)
        }
        _ => (0.45, 0.0),
    };
    caja(s, &p, &par, alfa, extra);

    let latido = e.latido().then(|| seg(t - e.t_modo));
    match estilo.escritura {
        Escritura::Onda => onda(e, s, t, &par),
        Escritura::Puntos => e.fila.puntos(s, t, dt, latido, &p),
        Escritura::Simbolos => e.fila.simbolos(s, w, t, dt, latido, &p),
        Escritura::Fijo => e.fila.fijos(s, &p),
    }

    // Si lo tipeado late mientras verifica, no hay texto: lo dice eso.
    let espera = match (estilo.reaccion, estilo.escritura) {
        (false, _) => 0.0,
        (true, Escritura::Onda) => FALLO_TEXTO,
        // Las × se apagan en 0,45 s y están donde va el texto.
        (true, Escritura::Simbolos) => 0.45,
        (true, _) => FALLO_TEXTO_FILA,
    };
    let texto = match e.modo {
        Modo::Normal if e.n == 0 => Some(("Contraseña".to_owned(), 1.0 - e.visible.valor(t))),
        Modo::Verificando if !estilo.verificar.late() => Some(("Verificando…".to_owned(), 1.0)),
        Modo::Fallo(n) if e.n == 0 => Some((
            format!("Autenticación fallida ({n})"),
            acotar((seg(t - e.t_fallo) - espera) / 0.2, 0.0, 1.0),
        )),
        _ => None,
    };
    if let Some((texto, a)) = texto
        && a > 0.01
    {
        let l = w.create_pango_layout(Some(&texto));
        l.set_font_description(Some(&pango::FontDescription::from_string(
            "JetBrainsMono Nerd Font 12.5",
        )));
        let (lw, lh) = l.pixel_size();
        s.save();
        s.translate(&graphene::Point::new(
            ((ANCHO - lw as f32) / 2.0).round(),
            ((ALTO - lh as f32) / 2.0).round(),
        ));
        s.append_layout(&l, &con_alfa(&p.on_surface, a));
        s.restore();
    }

    if e.modo == Modo::Verificando && estilo.verificar.gira() {
        let edad = seg(t - e.t_modo);
        let cabeza = PERIMETRO * 0.25 + edad * 520.0;
        let entrada = acotar(edad / 0.15, 0.0, 1.0);
        cometa(s, cabeza, 1.0, 120.0, 0.95 * entrada, &par, 2.2);
        cometa(
            s,
            cabeza + PERIMETRO / 2.0,
            1.0,
            120.0,
            0.95 * entrada,
            &par,
            2.2,
        );
    }
    for c in &e.chispas {
        let f = seg(t - c.t0) / 0.55;
        if f > 1.0 {
            continue;
        }
        let d = 495.0 * ease_out(f);
        cometa(
            s,
            c.s0 + c.dir * d,
            c.dir,
            44.0 * c.fuerza,
            (1.0 - f) * c.fuerza,
            &par,
            2.0,
        );
    }
    if e.modo == Modo::Saliendo && estilo.destello {
        let edad = seg(t - e.t_acierto);
        let d = ease_out(edad / 0.26) * PERIMETRO / 2.0;
        let a = 1.0 - acotar((edad - 0.26) / 0.26, 0.0, 1.0);
        cometa(s, BW / 2.0 + d, 1.0, d.max(8.0), a, &par, 2.4);
        cometa(s, BW / 2.0 - d, -1.0, d.max(8.0), a, &par, 2.4);
    }
    s.restore();
}

fn caja(s: &gtk::Snapshot, p: &Paleta, par: &[gdk::RGBA; 2], alfa: f32, extra: f32) {
    let fuera = redondo(0.0, 0.0, ANCHO, ALTO, RADIO);
    s.append_outset_shadow(
        &fuera,
        &gdk::RGBA::new(0.0, 0.0, 0.0, 0.5),
        0.0,
        0.0,
        4.0,
        10.0,
    );
    s.append_outset_shadow(&fuera, &con_alfa(&p.surface, 0.85), 0.0, 0.0, 1.0, 0.0);
    let blur = 5.0 + 8.0 * extra;
    s.append_outset_shadow(&fuera, &con_alfa(&par[0], alfa), -1.0, -1.0, 2.0, blur);
    s.append_outset_shadow(&fuera, &con_alfa(&par[1], alfa), 1.0, 1.0, 2.0, blur);
    s.push_rounded_clip(&fuera);
    s.append_color(
        &gdk::RGBA::new(0.0, 0.0, 0.0, 0.9),
        &rect(0.0, 0.0, ANCHO, ALTO),
    );
    s.append_color(
        &con_alfa(&p.surface_container, 0.85),
        &rect(0.0, 0.0, ANCHO, ALTO),
    );
    s.pop();
    s.append_border(&fuera, &[BORDE; 4], &[par[0], par[1], par[1], par[0]]);
}

fn onda(e: &Estado, s: &gtk::Snapshot, t: i64, par: &[gdk::RGBA; 2]) {
    let vis = e.visible.valor(t);
    if vis < 0.01 {
        return;
    }
    let p = &e.paleta;
    let (cx, cy) = (ANCHO / 2.0, ALTO / 2.0);
    let mitad = (ONDA_X1 - ONDA_X0) / 2.0;
    let reloj = seg(t) / LENTO_TECLA;
    let respira = 0.5 + 0.5 * (seg(t - e.t_modo) / (1.1 * LENTO_EVENTO) * 2.0 * PI).sin();
    let ondula = e.n > 0 && matches!(e.modo, Modo::Normal | Modo::Fallo(_));

    let b = gsk::PathBuilder::new();
    let mut x = ONDA_X0;
    while x <= ONDA_X1 + 0.01 {
        let d = (x - cx).abs();
        let borde = (PI * (x - ONDA_X0) / (ONDA_X1 - ONDA_X0))
            .sin()
            .max(0.0)
            .powf(0.6);
        let mut y = if ondula {
            0.7 * (x * 0.09 + reloj * 3.1).sin()
        } else {
            0.0
        };
        for i in &e.impulsos {
            let lento = i.tipo.lento();
            let edad = seg(t - i.t0) / lento;
            let (frente, ancho, dec, k) = match i.tipo {
                Tipo::Sale => (430.0 * edad, 13.0, 0.42, 0.55),
                Tipo::Entra => (mitad - 430.0 * edad, 13.0, 0.42, 0.55),
                Tipo::Fallo => (260.0 * edad, 22.0, 0.5, 0.9),
                Tipo::Acierto => (380.0 * edad, 13.0, 0.42, 0.55),
            };
            let u = d - frente;
            y += i.amp * (-edad / dec).exp() * (-(u * u) / (ancho * ancho)).exp() * (u * k).sin();
        }
        if e.latido() {
            y += 4.5 * respira * (x * 0.055 - seg(t) * 4.2 / LENTO_EVENTO).sin();
        }
        let y = acotar(y * borde, -18.0, 18.0);
        if x == ONDA_X0 {
            b.move_to(x, cy + y);
        } else {
            b.line_to(x, cy + y);
        }
        x += 1.5;
    }
    let camino = b.to_path();

    // Con el golpe del fallo la onda es roja entera; se va con la visibilidad.
    // Si se vuelve a tipear antes, la onda nueva ya no sale roja.
    let rojo = e
        .impulsos
        .iter()
        .find(|i| i.tipo == Tipo::Fallo)
        .map(|i| 1.0 - acotar((seg(t - i.t0) - FALLO_ONDA - 0.3) / 0.3, 0.0, 1.0))
        .unwrap_or(0.0);
    let mut c0 = mezclar(&par[0], &p.error, rojo);
    let mut c1 = mezclar(&par[1], &p.error, rojo);
    if e.modo == Modo::Saliendo {
        let blanco = 0.5 * (1.0 - acotar(seg(t - e.t_modo) / 0.3, 0.0, 1.0));
        c0 = mezclar(&c0, &gdk::RGBA::WHITE, blanco);
        c1 = mezclar(&c1, &gdk::RGBA::WHITE, blanco);
    }

    let caja = rect(0.0, 0.0, ANCHO, ALTO);
    let degrade = |a: f32| {
        gsk::LinearGradientNode::new(
            &caja,
            &graphene::Point::new(ONDA_X0, 0.0),
            &graphene::Point::new(ONDA_X1, 0.0),
            &[
                gsk::ColorStop::new(0.0, con_alfa(&c0, a)),
                gsk::ColorStop::new(1.0, con_alfa(&c1, a)),
            ],
        )
    };
    let trazo = |ancho: f32| {
        let st = gsk::Stroke::new(ancho);
        st.set_line_cap(gsk::LineCap::Round);
        st.set_line_join(gsk::LineJoin::Round);
        st
    };
    s.push_blur(4.0);
    s.push_stroke(&camino, &trazo(3.0));
    s.append_node(degrade(0.7 * vis));
    s.pop();
    s.pop();
    s.push_stroke(&camino, &trazo(2.0));
    s.append_node(degrade(vis));
    s.pop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paleta_lee_los_roles_y_cae_en_los_de_fabrica() {
        let p =
            Paleta::desde_css("@define-color primary #ff0000;\n@define-color  error  #00ff00 ;\n");
        assert_eq!(p.primary, gdk::RGBA::parse("#ff0000").unwrap());
        assert_eq!(p.tertiary, gdk::RGBA::parse("#d6bee5").unwrap());
    }

    #[test]
    fn el_recorrido_da_la_vuelta_al_borde() {
        assert_eq!(punto(0.0), (BX, BY));
        assert_eq!(punto(BW), (BX + BW, BY));
        assert_eq!(punto(PERIMETRO), (BX, BY));
        let (x, y) = punto(BW + BH + BW / 2.0);
        assert!((x - (BX + BW / 2.0)).abs() < 1e-3 && (y - (BY + BH)).abs() < 1e-3);
    }

    #[test]
    fn la_sacudida_vuelve_a_cero() {
        assert_eq!(sacudida(0.0), 0.0);
        assert_eq!(sacudida(1.0), 0.0);
        assert!(sacudida(0.14) < -10.0);
    }
}
