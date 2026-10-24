//! El centro en una `Scene` de vello: lo que se ve (lo manda el hilo de GTK, `vista.rs`) y lo
//! que es solo de la vista (qué lista está abierta, el foco, lo escrito, un arrastre). Cada
//! cuadro se rearma la hoja entera como una lista de trazos (`armar.rs`, `avisos.rs`) con los
//! textos guardados (`rotulos.rs`); responder al puntero y al teclado está en `toque.rs`.
//!
//! Las medidas son las de la versión con GTK (`.control-center` y lo que cuelga de él en
//! `estilo.css` hasta 0.1.1), medidas en el anidado:
//!
//! - el panel mide 412 px de borde a borde (380 de contenido, 14 de relleno y 2 de borde), a 8
//!   del borde derecho y de la barra;
//! - 8 px entre dos cajas y 8 adentro de cada una; los íconos en una columna de 36 y los textos
//!   a 54 del borde de su caja; títulos de 14 px y lo secundario a 12, atenuado.

mod armar;
pub mod avisos;
pub mod campo;
pub mod menu;
#[cfg(test)]
mod prueba;
mod rotulos;
pub mod toasts;
pub mod toque;
pub mod trazo;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use morgan_motor::color::Paleta;
use morgan_motor::curva::{Revelado, Transicion};
use morgan_motor::forma::{Borde, panel};
use morgan_motor::vello::Scene;
use morgan_motor::vello::kurbo::{Affine, Rect};

use super::vista::{
    BotonRepro, Contenido, Grupo, Iconos, ListaId, Nivel, NivelId, Reproductor, Tarjeta,
};
use campo::Entrada;
pub use rotulos::Rotulos;
use trazo::Trazo;

/// El contenido del panel.
pub const ANCHO: f64 = 380.0;
/// Del borde del panel al contenido: 2 de borde y 14 de relleno.
pub const BORDE_Y_RELLENO: f64 = 16.0;
pub const MARGEN: f64 = 8.0;
pub const ENTRE: f64 = 8.0;
pub const RADIO: f64 = 4.0;
/// Las listas que se despliegan (`GtkRevealer` de 200 ms).
pub const DESPLIEGUE: Duration = Duration::from_millis(200);
/// El encabezado de un grupo abierto (el `GtkRevealer` por defecto).
pub const ENCABEZADO: Duration = Duration::from_millis(250);

/// Lo que responde al puntero o al teclado.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Objetivo {
    Tarjeta(usize),
    Opcion(ListaId, usize),
    /// El botón del campo de una lista (el ojo o el tilde).
    BotonCampo(ListaId),
    /// Donde se escribe.
    Campo(ListaId),
    Glifo(NivelId),
    Nombre(NivelId),
    Slider(NivelId),
    /// El nombre de la fila de un agente, que despliega sus otras ventanas.
    Cuota(usize),
    Repro(BotonRepro),
    Limpiar,
    /// Una notificación: sola, o una de un grupo abierto.
    Aviso(u32),
    /// Un grupo cerrado con varias: el click lo abre. Por su clave.
    Grupo(u64),
    Contraer(u64),
    CerrarGrupo(u64),
    AccionAviso(u32, usize),
    /// Una fila del menú de la bandeja, por su id de dbusmenu.
    Fila(i32),
}

/// Cómo responde cada clase de objetivo, como en la hoja de GTK: lo que tarda en encenderse con
/// el puntero (o el foco), y cuánto se achica apretado y lo que tarda en volver.
fn tiempos(o: Objetivo) -> (Duration, f64, Duration) {
    let ms = Duration::from_millis;
    match o {
        Objetivo::Tarjeta(_) => (ms(150), 0.97, ms(100)),
        Objetivo::Opcion(..) | Objetivo::AccionAviso(..) | Objetivo::Fila(_) => {
            (ms(100), 0.97, ms(100))
        }
        Objetivo::Aviso(_) | Objetivo::Grupo(_) => (ms(100), 1.0, ms(100)),
        Objetivo::Campo(_) => (ms(100), 1.0, ms(100)),
        Objetivo::Slider(_) => (ms(200), 1.0, ms(120)),
        _ => (ms(100), 0.9, ms(150)),
    }
}

/// El puntero encima (o el foco) y lo apretado de cada objetivo, con sus transiciones.
#[derive(Default)]
pub struct Animos(HashMap<Objetivo, (Transicion, Transicion)>);

impl Animos {
    fn entrada(&mut self, o: Objetivo) -> &mut (Transicion, Transicion) {
        self.0
            .entry(o)
            .or_insert_with(|| (Transicion::new(0.0), Transicion::new(1.0)))
    }

    /// Hacia encendido o apagado; devuelve cuánto está encendido ahora.
    pub fn luz(&mut self, o: Objetivo, si: bool, ahora: Instant) -> f32 {
        let d = crate::animaciones::dur(tiempos(o).0);
        let e = self.entrada(o);
        e.0.ir(if si { 1.0 } else { 0.0 }, d, ahora);
        e.0.valor(ahora)
    }

    pub fn apretar(&mut self, o: Objetivo, si: bool, ahora: Instant) {
        let (_, k, d) = tiempos(o);
        let e = self.entrada(o);
        if si {
            e.1.ir(k as f32, Duration::ZERO, ahora);
        } else {
            e.1.ir(1.0, crate::animaciones::dur(d), ahora);
        }
    }

    pub fn escala(&self, o: Objetivo, ahora: Instant) -> f64 {
        self.0
            .get(&o)
            .map_or(1.0, |(_, e)| f64::from(e.valor(ahora)))
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        self.0.values().any(|(h, e)| h.anda(ahora) || e.anda(ahora))
    }

    /// Lo que ya no está en la hoja se olvida.
    fn retener(&mut self, vivos: &HashSet<Objetivo>) {
        self.0.retain(|o, _| vivos.contains(o));
    }
}

/// Una fila que se arrastra de costado (una opción con `Deslizar`, una notificación, un grupo
/// cerrado): dónde está y hacia dónde vuelve o se va.
#[derive(Clone, Copy, Debug)]
pub struct Desliz {
    pub desde: f64,
    pub hasta: f64,
    pub t0: Instant,
    /// Al llegar se hace (una que se va: olvidar, descartar).
    pub al_final: Option<toque::Lado>,
}

impl Desliz {
    pub fn quieto(dx: f64) -> Self {
        Desliz {
            desde: dx,
            hasta: dx,
            t0: Instant::now(),
            al_final: None,
        }
    }

    pub fn dx(&self, ahora: Instant) -> f64 {
        let micros = ahora.saturating_duration_since(self.t0).as_micros() as i64;
        let t = crate::deslizable::avance(micros);
        self.desde + (self.hasta - self.desde) * crate::deslizable::suavizar(t)
    }

    pub fn anda(&self, ahora: Instant) -> bool {
        self.desde != self.hasta
            && crate::deslizable::avance(ahora.saturating_duration_since(self.t0).as_micros() as i64)
                < 1.0
    }
}

/// La hoja de un cuadro: los trazos en coordenadas del contenido (0 arriba del todo, sin el
/// scroll) y dónde responde cada objetivo.
#[derive(Default)]
pub struct Hoja {
    pub trazos: Vec<Trazo>,
    pub objetivos: Vec<(Objetivo, Rect)>,
    /// En el orden del teclado.
    pub enfocables: Vec<Objetivo>,
    /// Las filas que se arrastran de costado, con su ancho.
    pub deslizables: Vec<(Objetivo, Rect)>,
    /// Todo lo que está en la hoja, aunque lo recorte algo que se despliega: conserva el foco
    /// y sus animaciones.
    pub presentes: Vec<Objetivo>,
    pub alto: f64,
}

impl Hoja {
    pub fn objetivo(&mut self, o: Objetivo, r: Rect, enfocable: bool) {
        self.objetivos.push((o, r));
        self.presentes.push(o);
        if enfocable {
            self.enfocables.push(o);
        }
    }

    /// `otra` (armada desde 0) corrida a `(dx, dy)` y recortada a `recorte`: lo que se
    /// despliega. Lo que queda fuera del recorte no responde.
    pub fn incrustar(&mut self, otra: Hoja, dx: f64, dy: f64, recorte: Rect) {
        // Lo que todavía no asoma (una lista que empieza a abrirse) igual está.
        if recorte.height() <= 0.0 {
            self.presentes.extend(otra.presentes);
            return;
        }
        self.trazos.push(Trazo::Recorte {
            r: recorte,
            radio: 0.0,
        });
        self.trazos.push(Trazo::mover(dx, dy));
        self.trazos.extend(otra.trazos);
        self.trazos.push(Trazo::Fin);
        self.trazos.push(Trazo::Fin);
        let mover = morgan_motor::vello::kurbo::Vec2::new(dx, dy);
        for (o, r) in otra.objetivos {
            let r = (r + mover).intersect(recorte);
            if r.width() > 0.0 && r.height() > 0.0 {
                self.objetivos.push((o, r));
            }
        }
        self.enfocables.extend(otra.enfocables);
        self.presentes.extend(otra.presentes);
        for (o, r) in otra.deslizables {
            self.deslizables.push((o, r + mover));
        }
    }

    pub fn caja(&self, o: Objetivo) -> Option<Rect> {
        self.objetivos
            .iter()
            .rev()
            .find(|(x, _)| *x == o)
            .map(|(_, r)| *r)
    }
}

/// Lo que se ve, tal como llegó del hilo de GTK.
#[derive(Default)]
pub struct Datos {
    pub tarjetas: Vec<Tarjeta>,
    pub listas: HashMap<ListaId, Contenido>,
    pub niveles: Vec<Nivel>,
    pub reproductor: Option<Reproductor>,
    pub iconos: Iconos,
    pub grupos: Vec<Grupo>,
}

pub struct Ui {
    pub paleta: Paleta,
    pub opacidad: f32,
    pub borde: Borde,
    pub datos: Datos,
    pub rotulos: Rotulos,
    pub animos: Animos,
    /// La lista abierta del grupo del panel, y cómo se despliega cada una.
    pub abierta: Option<ListaId>,
    pub revelados: HashMap<ListaId, Revelado>,
    /// La fila de cuotas desplegada (su propio grupo).
    pub cuota: Option<usize>,
    pub revelados_cuotas: HashMap<usize, Revelado>,
    /// Los grupos de notificaciones abiertos, por clave, y su encabezado.
    pub expandidos: HashSet<String>,
    pub encabezados: HashMap<String, Revelado>,
    /// Las notificaciones con sus acciones a la vista.
    pub con_acciones: HashSet<u32>,
    pub revelados_acciones: HashMap<u32, Revelado>,
    /// Lo escrito en el campo de cada lista.
    pub campos: HashMap<ListaId, Entrada>,
    /// El último fallo visto de cada campo y cuándo llegó (tiembla y se tiñe desde ahí), y
    /// desde cuándo late uno ocupado.
    pub fallos: HashMap<ListaId, (u64, Instant)>,
    pub latidos: HashMap<ListaId, Instant>,
    /// Un campo tiembla, se destiñe o late: hace falta otro cuadro.
    pub campo_anda: bool,
    pub foco: Option<Objetivo>,
    /// El foco se ve (se llegó con el teclado), como el `focus-visible` de GTK.
    pub foco_visible: bool,
    pub encima: Option<Objetivo>,
    /// Dónde está el puntero (en la superficie): cuando la hoja cambia debajo de él, lo que
    /// queda encima es otra cosa.
    pub puntero: Option<(f64, f64)>,
    pub apretado: Option<Objetivo>,
    /// El nivel de cada slider mientras la mano lo arrastra: lo que llega no lo mueve.
    pub arrastrando: Option<(NivelId, f64)>,
    pub deslices: HashMap<Objetivo, Desliz>,
    pub arrastre: Option<toque::Arrastre>,
    /// Lo que se ve del ecualizador (el fundido de 600 ms) y sus niveles.
    pub ecualizador: (Transicion, Vec<f64>),
    /// Desde cuándo anda la marquesina del título del reproductor.
    pub marquesina: Instant,
    /// Al limpiar, cada notificación sale a su turno; las de antes de tocar.
    pub limpiando: Option<(Instant, Vec<u32>)>,
    pub scroll: f64,
    /// El tamaño de cada perilla (crece con el foco, se achica apretada).
    pub perillas: HashMap<crate::centro::vista::NivelId, Transicion>,
    /// Lo que no entra del título del reproductor (anda en marquesina si es más de 0).
    pub sobra_titulo: f64,
    /// La hoja del último cuadro: con ella se responde al puntero.
    pub hoja: Hoja,
    pub tam: (u32, u32),
}

impl Ui {
    pub fn new(paleta: Paleta, opacidad: f32, borde: Borde) -> Self {
        Ui {
            paleta,
            opacidad,
            borde,
            datos: Datos::default(),
            rotulos: Rotulos::new(),
            animos: Animos::default(),
            abierta: None,
            revelados: HashMap::new(),
            cuota: None,
            revelados_cuotas: HashMap::new(),
            expandidos: HashSet::new(),
            encabezados: HashMap::new(),
            con_acciones: HashSet::new(),
            revelados_acciones: HashMap::new(),
            campos: HashMap::new(),
            fallos: HashMap::new(),
            latidos: HashMap::new(),
            campo_anda: false,
            foco: None,
            foco_visible: false,
            encima: None,
            puntero: None,
            apretado: None,
            arrastrando: None,
            deslices: HashMap::new(),
            arrastre: None,
            ecualizador: (Transicion::new(0.0), Vec::new()),
            marquesina: Instant::now(),
            limpiando: None,
            scroll: 0.0,
            perillas: HashMap::new(),
            sobra_titulo: 0.0,
            hoja: Hoja::default(),
            tam: (0, 0),
        }
    }

    /// Lo que se ve del panel: su caja de borde a borde, en la superficie, sin el scroll.
    pub fn panel(&self) -> Rect {
        let (w, h) = (f64::from(self.tam.0), f64::from(self.tam.1));
        let ancho = ANCHO + 2.0 * BORDE_Y_RELLENO;
        let x1 = w - MARGEN;
        let alto = (self.hoja.alto + 2.0 * BORDE_Y_RELLENO).min((h - 2.0 * MARGEN).max(0.0));
        Rect::new(x1 - ancho, MARGEN, x1, MARGEN + alto)
    }

    /// Donde se ve el contenido (dentro del relleno), en la superficie.
    pub fn ventana(&self) -> Rect {
        self.panel().inset(-BORDE_Y_RELLENO)
    }

    /// De la superficie al contenido.
    pub fn al_contenido(&self, x: f64, y: f64) -> (f64, f64) {
        let v = self.ventana();
        (x - v.x0, y - v.y0 + self.scroll)
    }

    pub fn max_scroll(&self) -> f64 {
        (self.hoja.alto - self.ventana().height()).max(0.0)
    }

    pub fn desplazar(&mut self, dy: f64) {
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll());
    }

    /// Que `o` quede a la vista.
    pub fn mostrar(&mut self, o: Objetivo) {
        let Some(r) = self.hoja.caja(o) else { return };
        let alto = self.ventana().height();
        if r.y0 < self.scroll {
            self.scroll = r.y0.max(0.0);
        } else if r.y1 > self.scroll + alto {
            self.scroll = (r.y1 - alto).min(self.max_scroll());
        }
    }

    /// Rearma la hoja de este cuadro.
    pub fn armar(&mut self, escala: f32, ahora: Instant) {
        self.rotulos.poner_escala(escala);
        let mut h = Hoja::default();
        self.campo_anda = false;
        self.armar_todo(&mut h, ahora);
        let vivos: HashSet<Objetivo> = h.presentes.iter().copied().collect();
        self.animos.retener(&vivos);
        self.deslices
            .retain(|o, d| vivos.contains(o) && (d.dx(ahora) != 0.0 || d.anda(ahora)));
        if self.foco.is_some_and(|f| !vivos.contains(&f)) {
            self.foco = None;
        }
        self.hoja = h;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        if let Some((x, y)) = self.puntero
            && !self.arrastre.is_some_and(|a| a.activo())
        {
            self.encima = self.bajo(x, y);
        }
        self.rotulos.barrer();
    }

    /// Si algo se mueve solo y pide otro cuadro.
    pub fn anda(&self, ahora: Instant) -> bool {
        self.animos.anda(ahora)
            || self.revelados.values().any(|r| r.anda(ahora))
            || self.revelados_cuotas.values().any(|r| r.anda(ahora))
            || self.encabezados.values().any(|r| r.anda(ahora))
            || self.revelados_acciones.values().any(|r| r.anda(ahora))
            || self.deslices.values().any(|d| d.anda(ahora))
            || self.ecualizador.0.anda(ahora)
            || self.limpiando.is_some()
            || self.campo_anda
            || self.marquesina_anda()
    }

    /// Pinta la hoja: el panel, y el contenido recortado a su ventana y corrido por el scroll
    /// y la bajada de la entrada.
    pub fn pintar(&self, s: &mut Scene, escala: f64, bajada: f64) {
        s.reset();
        let b = bajada.round();
        let tr = Affine::scale(escala) * Affine::translate((0.0, -b));
        panel(s, tr, &self.paleta, self.opacidad, self.panel(), self.borde);
        let v = self.ventana();
        s.push_clip_layer(morgan_motor::vello::peniko::Fill::NonZero, tr, &v);
        let base = tr * Affine::translate((v.x0, v.y0 - self.scroll.round()));
        trazo::pintar(s, &self.hoja.trazos, base, escala);
        s.pop_layer();
    }

    /// El centro se cierra: lo que es de una apertura no queda para la próxima.
    pub fn cerrar(&mut self) {
        self.abierta = None;
        self.revelados.clear();
        self.cuota = None;
        self.revelados_cuotas.clear();
        self.con_acciones.clear();
        self.revelados_acciones.clear();
        self.foco = None;
        self.foco_visible = false;
        self.encima = None;
        self.apretado = None;
        self.arrastrando = None;
        self.arrastre = None;
        self.deslices.clear();
        self.limpiando = None;
        self.scroll = 0.0;
        self.campos.clear();
        self.latidos.clear();
    }

    /// Abrir una lista cierra la que estaba abierta; tocar la abierta la cierra.
    pub fn alternar_lista(&mut self, l: ListaId, ahora: Instant) {
        let abrir = self.abierta != Some(l);
        self.desplegar(abrir.then_some(l), ahora);
    }

    pub fn desplegar(&mut self, l: Option<ListaId>, ahora: Instant) {
        let d = crate::animaciones::dur(DESPLIEGUE);
        if let Some(vieja) = self.abierta
            && Some(vieja) != l
        {
            self.revelados
                .entry(vieja)
                .or_insert_with(|| Revelado::new(true))
                .ir(false, d, ahora);
        }
        if let Some(n) = l {
            self.revelados
                .entry(n)
                .or_insert_with(|| Revelado::new(false))
                .ir(true, d, ahora);
        }
        self.abierta = l;
    }

    pub fn revelado(&self, l: ListaId, ahora: Instant) -> f64 {
        self.revelados.get(&l).map_or(0.0, |r| r.valor(ahora))
    }

    fn marquesina_anda(&self) -> bool {
        self.datos.reproductor.is_some() && armar::titulo_no_entra(self)
    }
}
