//! Lo que hacen el puntero y el teclado en el centro. Como en la versión con GTK: el click pide
//! (lo que se ve encendido lo dice el sistema), el slider salta adonde se aprieta y sigue a la
//! mano, una fila con acción de costado se arrastra (`deslizable.rs`), Tab y las flechas
//! recorren lo que se toca (las flechas sobre un slider cambian su nivel), Enter abre o hace,
//! Supr cierra una notificación y Escape cierra el centro (o solo el campo con el foco).

use std::time::{Duration, Instant};

use morgan_motor::vello::kurbo::{Point, Rect};
use smithay_client_toolkit::seat::keyboard::Keysym;

use super::avisos::{clave, duracion_limpiar};
use super::{Desliz, Objetivo, Ui};
use crate::centro::vista::{BotonCampo, Contenido, Evento, ListaId, Opcion, Toque};
pub use crate::deslizable::Lado;
use crate::deslizable::{ARRANQUE, decidir, tiron};

/// Lo que el centro le pide a su hilo.
#[derive(Debug)]
pub enum Pedido {
    Gtk(Evento),
    /// Cerrarse: Escape o un click afuera.
    Cerrar,
    /// Pegar en el campo de esa lista lo que haya en el portapapeles.
    Pegar(ListaId),
}

/// Un arrastre de costado en curso (o por empezar: arranca con 20 px más horizontales que
/// verticales).
#[derive(Clone, Copy, Debug)]
pub struct Arrastre {
    objetivo: Objetivo,
    desde: Point,
    ancho: f64,
    derecha: bool,
    izquierda: bool,
    activo: bool,
    hecho: bool,
    /// El último `dx` y su hora en ms, para la velocidad del tirón.
    ultimo: (f64, u32),
    velocidad: f64,
}

/// Los botones de Linux.
pub const IZQUIERDO: u32 = 0x110;

impl Ui {
    fn bajo(&self, x: f64, y: f64) -> Option<Objetivo> {
        let v = self.ventana();
        if !v.contains(Point::new(x, y)) {
            return None;
        }
        let (cx, cy) = self.al_contenido(x, y);
        let p = Point::new(cx, cy);
        self.hoja
            .objetivos
            .iter()
            .rev()
            .find(|(_, r)| r.contains(p))
            .map(|(o, _)| *o)
    }

    pub fn dentro(&self, x: f64, y: f64) -> bool {
        self.panel().contains(Point::new(x, y))
    }

    /// Si hay un campo bajo el puntero (para el cursor de texto).
    pub fn sobre_campo(&self) -> bool {
        matches!(self.encima, Some(Objetivo::Campo(_)))
    }

    fn opcion(&self, l: ListaId, i: usize) -> Option<&Opcion> {
        match self.datos.listas.get(&l)? {
            Contenido::Opciones(v) => v.get(i),
            Contenido::Cuotas(_) => None,
        }
    }

    /// La opción que lleva el campo de la lista `l`.
    fn con_campo(&self, l: ListaId) -> Option<&Opcion> {
        match self.datos.listas.get(&l)? {
            Contenido::Opciones(v) => v.iter().find(|o| o.campo.is_some()),
            Contenido::Cuotas(_) => None,
        }
    }

    pub fn mover_puntero(&mut self, x: f64, y: f64, t: u32) -> Vec<Pedido> {
        let mut pedidos = Vec::new();
        if let Some(a) = self.arrastre {
            let (cx, cy) = self.al_contenido(x, y);
            self.arrastrar(a, cx, cy, t, &mut pedidos);
        }
        if let Some(Objetivo::Slider(id)) = self.apretado {
            self.slider_a(id, x, &mut pedidos);
        }
        let b = if self.arrastre.is_some_and(|a| a.activo) {
            None
        } else {
            self.bajo(x, y)
        };
        self.encima = b;
        pedidos
    }

    pub fn salir_puntero(&mut self) {
        self.encima = None;
    }

    pub fn apretar(&mut self, x: f64, y: f64, boton: u32, t: u32) -> Vec<Pedido> {
        let mut pedidos = Vec::new();
        let ahora = Instant::now();
        if boton != IZQUIERDO {
            return pedidos;
        }
        self.foco_visible = false;
        let Some(o) = self.bajo(x, y) else {
            return pedidos;
        };
        self.apretado = Some(o);
        self.animos.apretar(o, true, ahora);
        if self.hoja.enfocables.contains(&o) || matches!(o, Objetivo::Campo(_)) {
            self.foco = Some(o);
        }
        if let Some((_, r)) = self.hoja.deslizables.iter().find(|(d, _)| *d == o) {
            let (derecha, izquierda) = self.lados(o);
            let (cx, cy) = self.al_contenido(x, y);
            self.arrastre = Some(Arrastre {
                objetivo: o,
                desde: Point::new(cx, cy),
                ancho: r.width(),
                derecha,
                izquierda,
                activo: false,
                hecho: false,
                ultimo: (0.0, t),
                velocidad: 0.0,
            });
        }
        if let Objetivo::Slider(id) = o {
            self.slider_a(id, x, &mut pedidos);
        }
        pedidos
    }

    pub fn soltar(&mut self, x: f64, y: f64, boton: u32) -> Vec<Pedido> {
        let mut pedidos = Vec::new();
        let ahora = Instant::now();
        let bajo = self.bajo(x, y);
        // Cualquier botón afuera del panel lo cierra, como el click de GTK.
        if self.apretado.is_none() && self.arrastre.is_none() && !self.dentro(x, y) {
            pedidos.push(Pedido::Cerrar);
            return pedidos;
        }
        if boton != IZQUIERDO {
            // Medio y derecho sobre una notificación: cerrarla.
            match bajo {
                Some(Objetivo::Aviso(id)) => pedidos.push(Pedido::Gtk(Evento::Cerrar(id))),
                Some(Objetivo::Grupo(k)) => {
                    if let Some(g) = self.grupo_de(k) {
                        pedidos.push(Pedido::Gtk(Evento::Cerrar(g.avisos[0].id)));
                    }
                }
                _ => {}
            }
            return pedidos;
        }
        let apretado = self.apretado.take();
        if let Some(o) = apretado {
            self.animos.apretar(o, false, ahora);
        }
        if let Some(Objetivo::Slider(_)) = apretado {
            self.arrastrando = None;
            return pedidos;
        }
        if let Some(a) = self.arrastre.take()
            && a.activo
        {
            let dx = self.deslices.get(&a.objetivo).map_or(0.0, |d| d.dx(ahora));
            match tiron(dx, a.velocidad).filter(|l| self.lado_vale(&a, *l)) {
                Some(l) => self.soltar_de_costado(a.objetivo, l, a.ancho, &mut pedidos),
                None => self.volver(a.objetivo, 0.0),
            }
            return pedidos;
        }
        if let Some(o) = apretado
            && bajo == Some(o)
        {
            self.activar(o, &mut pedidos);
        }
        pedidos
    }

    pub fn rueda(&mut self, dy: f64, discreto: bool) {
        let paso = if discreto {
            // El paso de la rueda de un `GtkScrolledWindow`: la página a la 2/3.
            self.ventana().height().powf(2.0 / 3.0)
        } else {
            1.0
        };
        self.desplazar(dy * paso);
    }

    // ── Lo que hace cada cosa ──

    fn activar(&mut self, o: Objetivo, pedidos: &mut Vec<Pedido>) {
        let ahora = Instant::now();
        match o {
            Objetivo::Tarjeta(i) => match self.datos.tarjetas.get(i).map(|t| t.que) {
                Some(Toque::Accion(id)) => pedidos.push(Pedido::Gtk(Evento::Tocar(id))),
                Some(Toque::Lista(l)) => self.alternar_lista(l, ahora),
                None => {}
            },
            Objetivo::Opcion(l, i) => {
                if let Some(op) = self.opcion(l, i).filter(|o| o.habilitada) {
                    pedidos.push(Pedido::Gtk(Evento::Tocar(op.accion)));
                }
            }
            Objetivo::BotonCampo(l) => {
                let boton = self
                    .con_campo(l)
                    .and_then(|o| o.campo.as_ref()?.caja.as_ref()?.boton);
                match boton {
                    Some(BotonCampo::Ojo) => {
                        if let Some(e) = self.campos.get_mut(&l) {
                            e.ver = !e.ver;
                        }
                        // El foco sigue en lo escrito.
                        self.foco = Some(Objetivo::Campo(l));
                    }
                    Some(BotonCampo::Tilde(id)) => pedidos.push(Pedido::Gtk(Evento::Tocar(id))),
                    None => {}
                }
            }
            Objetivo::Campo(l) => {
                self.foco = Some(Objetivo::Campo(l));
                if let Some(e) = self.campos.get_mut(&l) {
                    e.tocar();
                }
            }
            Objetivo::Glifo(n) => {
                if let Some(id) = self
                    .datos
                    .niveles
                    .iter()
                    .find(|x| x.id == n)
                    .and_then(|x| x.glifo_accion)
                {
                    pedidos.push(Pedido::Gtk(Evento::Tocar(id)));
                }
            }
            Objetivo::Nombre(n) => {
                if let Some(l) = self.datos.niveles.iter().find(|x| x.id == n).and_then(|x| x.lista) {
                    self.alternar_lista(l, ahora);
                }
            }
            Objetivo::Slider(_) => {}
            Objetivo::Cuota(i) => {
                let abrir = self.cuota != Some(i);
                let d = crate::animaciones::dur(super::DESPLIEGUE);
                if let Some(vieja) = self.cuota.take() {
                    super::armar::revelar(&mut self.revelados_cuotas, vieja, false, d, ahora);
                }
                if abrir {
                    self.cuota = Some(i);
                    super::armar::revelar(&mut self.revelados_cuotas, i, true, d, ahora);
                }
            }
            Objetivo::Repro(b) => pedidos.push(Pedido::Gtk(Evento::Repro(b))),
            Objetivo::Limpiar => {
                let ids: Vec<u32> = self
                    .datos
                    .grupos
                    .iter()
                    .flat_map(|g| g.avisos.iter().map(|a| a.id))
                    .collect();
                if !ids.is_empty() && self.limpiando.is_none() {
                    self.limpiando = Some((ahora, ids));
                }
            }
            Objetivo::Aviso(id) => {
                let con_acciones = self.aviso(id).is_some_and(|a| !a.acciones.is_empty());
                if con_acciones {
                    self.alternar_acciones(id, ahora);
                } else {
                    pedidos.push(Pedido::Gtk(Evento::Accion(id)));
                }
            }
            Objetivo::Grupo(k) => {
                if let Some(c) = self.grupo_de(k).map(|g| g.clave.clone()) {
                    self.expandir(&c, true, ahora);
                }
            }
            Objetivo::Contraer(k) => {
                if let Some(c) = self.grupo_de(k).map(|g| g.clave.clone()) {
                    self.expandir(&c, false, ahora);
                }
            }
            Objetivo::CerrarGrupo(k) => {
                if let Some(c) = self.grupo_de(k).map(|g| g.clave.clone()) {
                    pedidos.push(Pedido::Gtk(Evento::CerrarGrupo(c)));
                }
            }
            Objetivo::AccionAviso(id, i) => {
                if let Some(c) = self.aviso(id).and_then(|a| a.acciones.get(i)).map(|a| a.0.clone()) {
                    pedidos.push(Pedido::Gtk(Evento::Boton(id, c)));
                }
            }
        }
    }

    // ── Sliders ──

    fn slider_a(&mut self, id: crate::centro::vista::NivelId, x: f64, pedidos: &mut Vec<Pedido>) {
        let Some(r) = self.hoja.caja(Objetivo::Slider(id)) else {
            return;
        };
        // La zona tiene 5 a cada lado del canal (media perilla).
        let canal = Rect::new(r.x0 + 5.0, r.y0, r.x1 - 5.0, r.y1);
        let (cx, _) = self.al_contenido(x, 0.0);
        let v = (((cx - canal.x0) / canal.width().max(1.0)) * 100.0)
            .clamp(0.0, 100.0)
            .round();
        self.poner_nivel(id, v, pedidos);
    }

    fn poner_nivel(&mut self, id: crate::centro::vista::NivelId, v: f64, pedidos: &mut Vec<Pedido>) {
        let Some(n) = self.datos.niveles.iter().find(|n| n.id == id) else {
            return;
        };
        let antes = match self.arrastrando {
            Some((x, v)) if x == id => v,
            _ => n.valor.round(),
        };
        let mover = n.mover;
        // Mientras dura el arrastre (o la tecla), lo que llega no mueve la perilla.
        if self.apretado == Some(Objetivo::Slider(id)) {
            self.arrastrando = Some((id, v));
        } else {
            self.arrastrando = None;
        }
        if v != antes {
            pedidos.push(Pedido::Gtk(Evento::Nivel(mover, v)));
            if let Some(n) = self.datos.niveles.iter_mut().find(|n| n.id == id) {
                n.valor = v;
            }
        }
    }

    // ── De costado ──

    fn lados(&self, o: Objetivo) -> (bool, bool) {
        match o {
            Objetivo::Opcion(l, i) => self
                .opcion(l, i)
                .map_or((false, false), |op| (op.derecha.is_some(), op.izquierda.is_some())),
            Objetivo::Aviso(_) => (true, true),
            Objetivo::Grupo(_) => (true, false),
            _ => (false, false),
        }
    }

    fn lado_vale(&self, a: &Arrastre, l: Lado) -> bool {
        match l {
            Lado::Derecha => a.derecha,
            Lado::Izquierda => a.izquierda,
        }
    }

    fn arrastrar(&mut self, mut a: Arrastre, cx: f64, cy: f64, t: u32, pedidos: &mut Vec<Pedido>) {
        if a.hecho {
            return;
        }
        let (dx, dy) = (cx - a.desde.x, cy - a.desde.y);
        let (dx0, t0) = std::mem::replace(&mut a.ultimo, (dx, t));
        if t0 != 0 && t > t0 {
            a.velocidad = (dx - dx0) / f64::from(t - t0);
        }
        if !a.activo {
            if dx.abs() < ARRANQUE || dx.abs() < dy.abs() {
                self.arrastre = Some(a);
                return;
            }
            let lado = if dx > 0.0 { Lado::Derecha } else { Lado::Izquierda };
            if !self.lado_vale(&a, lado) {
                self.arrastre = Some(a);
                return;
            }
            // Le gana al click.
            a.activo = true;
            if let Some(o) = self.apretado.take() {
                self.animos.apretar(o, false, Instant::now());
            }
        }
        // Hacia un lado sin acción no se mueve.
        let dx = match (dx > 0.0, a.derecha, a.izquierda) {
            (true, false, _) | (false, _, false) => 0.0,
            _ => dx,
        };
        self.deslices.insert(a.objetivo, Desliz::quieto(dx));
        self.arrastre = Some(a);
        if let Some(l) = decidir(dx, a.ancho) {
            a.hecho = true;
            a.activo = false;
            self.arrastre = None;
            self.soltar_de_costado(a.objetivo, l, a.ancho, pedidos);
        }
    }

    fn volver(&mut self, o: Objetivo, hasta: f64) {
        let ahora = Instant::now();
        let desde = self.deslices.get(&o).map_or(0.0, |d| d.dx(ahora));
        self.deslices.insert(
            o,
            Desliz {
                desde,
                hasta,
                t0: ahora,
                al_final: None,
            },
        );
    }

    /// La acción de ese lado: la fila que se va sale y la acción se hace al llegar; la que
    /// vuelve la hace ya.
    fn soltar_de_costado(&mut self, o: Objetivo, lado: Lado, ancho: f64, pedidos: &mut Vec<Pedido>) {
        let se_va = match (o, lado) {
            (Objetivo::Opcion(l, i), Lado::Derecha) => {
                self.opcion(l, i).and_then(|x| x.derecha.as_ref()).is_some_and(|d| d.se_va)
            }
            (Objetivo::Opcion(l, i), Lado::Izquierda) => {
                self.opcion(l, i).and_then(|x| x.izquierda.as_ref()).is_some_and(|d| d.se_va)
            }
            (_, Lado::Derecha) => true,
            (_, Lado::Izquierda) => false,
        };
        if se_va {
            let ahora = Instant::now();
            let desde = self.deslices.get(&o).map_or(0.0, |d| d.dx(ahora));
            let hasta = if lado == Lado::Derecha { ancho } else { -ancho };
            self.deslices.insert(
                o,
                Desliz {
                    desde,
                    hasta,
                    t0: ahora,
                    al_final: Some(lado),
                },
            );
        } else {
            self.hacer_de_costado(o, lado, pedidos);
            self.volver(o, 0.0);
        }
    }

    fn hacer_de_costado(&mut self, o: Objetivo, lado: Lado, pedidos: &mut Vec<Pedido>) {
        let e = match (o, lado) {
            (Objetivo::Opcion(l, i), Lado::Derecha) => self
                .opcion(l, i)
                .and_then(|x| x.derecha.as_ref())
                .map(|d| Evento::Tocar(d.accion)),
            (Objetivo::Opcion(l, i), Lado::Izquierda) => self
                .opcion(l, i)
                .and_then(|x| x.izquierda.as_ref())
                .map(|d| Evento::Tocar(d.accion)),
            (Objetivo::Aviso(id), Lado::Derecha) => Some(Evento::Cerrar(id)),
            (Objetivo::Aviso(id), Lado::Izquierda) => Some(Evento::Copiar(id)),
            (Objetivo::Grupo(k), Lado::Derecha) => {
                self.grupo_de(k).map(|g| Evento::CerrarGrupo(g.clave.clone()))
            }
            _ => None,
        };
        if let Some(e) = e {
            pedidos.push(Pedido::Gtk(e));
        }
    }

    /// Lo que termina solo con el tiempo: una fila que terminó de irse, limpiar.
    pub fn al_cuadro(&mut self, ahora: Instant) -> Vec<Pedido> {
        let mut pedidos = Vec::new();
        let llegaron: Vec<(Objetivo, Lado)> = self
            .deslices
            .iter_mut()
            .filter(|(_, d)| !d.anda(ahora))
            .filter_map(|(o, d)| d.al_final.take().map(|l| (*o, l)))
            .collect();
        for (o, l) in llegaron {
            self.hacer_de_costado(o, l, &mut pedidos);
        }
        let filas = self.datos.grupos.len();
        if let Some((t0, ids)) = &self.limpiando
            && ahora.saturating_duration_since(*t0) >= duracion_limpiar(filas)
        {
            pedidos.push(Pedido::Gtk(Evento::Limpiar(ids.clone())));
            self.limpiando = None;
        }
        pedidos
    }

    // ── Teclado ──

    /// `ctrl` y `shift`: los modificadores sostenidos.
    pub fn tecla(&mut self, k: Keysym, texto: Option<&str>, ctrl: bool, shift: bool) -> Vec<Pedido> {
        let mut pedidos = Vec::new();
        if let Some(Objetivo::Campo(l)) = self.foco
            && self.tecla_campo(l, k, texto, ctrl, &mut pedidos)
        {
            return pedidos;
        }
        match k {
            Keysym::Escape => pedidos.push(Pedido::Cerrar),
            Keysym::Tab => self.recorrer(!shift),
            Keysym::ISO_Left_Tab => self.recorrer(false),
            Keysym::Down => self.recorrer(true),
            Keysym::Up => self.recorrer(false),
            Keysym::Left | Keysym::Right | Keysym::Page_Up | Keysym::Page_Down
                if matches!(self.foco, Some(Objetivo::Slider(_))) =>
            {
                let Some(Objetivo::Slider(id)) = self.foco else {
                    return pedidos;
                };
                let paso = match k {
                    Keysym::Left => -1.0,
                    Keysym::Right => 1.0,
                    Keysym::Page_Up => 10.0,
                    _ => -10.0,
                };
                let actual = self
                    .datos
                    .niveles
                    .iter()
                    .find(|n| n.id == id)
                    .map_or(0.0, |n| n.valor.round());
                self.foco_visible = true;
                self.poner_nivel(id, (actual + paso).clamp(0.0, 100.0), &mut pedidos);
            }
            Keysym::Right => self.recorrer(true),
            Keysym::Left => self.recorrer(false),
            Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                let Some(o) = self.foco else { return pedidos };
                match o {
                    // En la lista, Enter abre el grupo o hace la acción por defecto.
                    Objetivo::Grupo(_) if k != Keysym::space => self.activar(o, &mut pedidos),
                    Objetivo::Aviso(id) if k != Keysym::space => {
                        pedidos.push(Pedido::Gtk(Evento::Accion(id)));
                    }
                    Objetivo::Aviso(_) | Objetivo::Grupo(_) => {}
                    _ => self.activar(o, &mut pedidos),
                }
            }
            Keysym::Delete | Keysym::BackSpace => match self.foco {
                Some(Objetivo::Aviso(id)) => pedidos.push(Pedido::Gtk(Evento::Cerrar(id))),
                Some(Objetivo::Grupo(k)) => {
                    if let Some(g) = self.grupo_de(k) {
                        pedidos.push(Pedido::Gtk(Evento::Cerrar(g.avisos[0].id)));
                    }
                }
                _ => {}
            },
            _ => {}
        }
        pedidos
    }

    /// Las teclas del campo con el foco; falso si no son suyas (Tab, por ejemplo).
    fn tecla_campo(
        &mut self,
        l: ListaId,
        k: Keysym,
        texto: Option<&str>,
        ctrl: bool,
        pedidos: &mut Vec<Pedido>,
    ) -> bool {
        let Some(caja) = self
            .con_campo(l)
            .and_then(|o| o.campo.as_ref()?.caja.clone())
        else {
            return false;
        };
        let Some(e) = self.campos.get_mut(&l) else {
            return false;
        };
        match k {
            Keysym::Escape => pedidos.push(Pedido::Gtk(Evento::Tocar(caja.escape))),
            Keysym::Return | Keysym::KP_Enter => {
                if !caja.ocupado {
                    pedidos.push(Pedido::Gtk(Evento::Enviar(caja.enviar, e.texto.clone())));
                }
            }
            Keysym::Tab | Keysym::ISO_Left_Tab | Keysym::Up | Keysym::Down => return false,
            _ if caja.ocupado || caja.fijo.is_some() => {}
            Keysym::BackSpace if ctrl => e.borrar_palabra(),
            Keysym::BackSpace => e.borrar_atras(),
            Keysym::Delete => e.borrar_adelante(),
            Keysym::Left => e.izquierda(),
            Keysym::Right => e.derecha(),
            Keysym::Home => e.inicio(),
            Keysym::End => e.fin(),
            Keysym::v | Keysym::V if ctrl => pedidos.push(Pedido::Pegar(l)),
            Keysym::u if ctrl => {
                e.inicio();
                e.texto.clear();
            }
            _ if ctrl => {}
            _ => {
                if let Some(s) = texto {
                    e.insertar(s);
                }
            }
        }
        true
    }

    /// El foco al siguiente (o al anterior) de lo que se toca, a la vista.
    fn recorrer(&mut self, adelante: bool) {
        self.foco_visible = true;
        let v = &self.hoja.enfocables;
        if v.is_empty() {
            return;
        }
        let i = self.foco.and_then(|f| v.iter().position(|x| *x == f));
        let n = v.len();
        let j = match (i, adelante) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
        };
        let o = v[j];
        self.foco = Some(o);
        if let Objetivo::Campo(l) = o
            && let Some(e) = self.campos.get_mut(&l)
        {
            e.tocar();
        }
        self.mostrar(o);
    }

    /// Lo pegado del portapapeles, en el campo de `l`.
    pub fn pegar(&mut self, l: ListaId, texto: &str) {
        let linea = texto.lines().next().unwrap_or("");
        if let Some(e) = self.campos.get_mut(&l) {
            e.insertar(linea);
        }
    }

    /// El foco en la primera notificación, como al abrir (si no hay, en nada).
    pub fn enfocar_lista(&mut self) {
        self.foco = self
            .hoja
            .enfocables
            .iter()
            .find(|o| matches!(o, Objetivo::Aviso(_) | Objetivo::Grupo(_)))
            .copied();
    }

    /// El foco en una opción, a la vista (la sesión: Bloquear, o «Volver»).
    pub fn enfocar_opcion(&mut self, l: ListaId, i: usize) {
        self.foco = Some(Objetivo::Opcion(l, i));
        self.foco_visible = true;
    }

    /// Cuándo hay que despertar a dibujar el cursor de un campo.
    pub fn proximo_parpadeo(&self, ahora: Instant) -> Option<Instant> {
        let Some(Objetivo::Campo(l)) = self.foco else {
            return None;
        };
        self.campos.get(&l)?.proximo_cambio(ahora)
    }

    /// El campo de la lista con el foco, si un campo nuevo lo pide (otra generación).
    pub fn enfocar_campo(&mut self, l: ListaId, generacion: u64) {
        let fijo_con_tilde = self
            .con_campo(l)
            .and_then(|o| o.campo.as_ref())
            .and_then(|c| c.caja.as_ref())
            .is_some_and(|c| c.fijo.is_some() && matches!(c.boton, Some(BotonCampo::Tilde(_))));
        // Un código para confirmar: el foco en el tilde (Enter lo acepta).
        self.foco = Some(if fijo_con_tilde {
            Objetivo::BotonCampo(l)
        } else {
            Objetivo::Campo(l)
        });
        let e = self.campos.entry(l).or_default();
        e.preparar(generacion);
        e.tocar();
    }

    /// Un grupo que ya no está deja de estar abierto.
    pub fn olvidar_cerrados(&mut self) {
        let vivos: std::collections::HashSet<String> =
            self.datos.grupos.iter().map(|g| g.clave.clone()).collect();
        self.expandidos.retain(|g| vivos.contains(g));
        self.encabezados.retain(|g, _| vivos.contains(g));
        let ids: std::collections::HashSet<u32> = self
            .datos
            .grupos
            .iter()
            .flat_map(|g| g.avisos.iter().map(|a| a.id))
            .collect();
        self.con_acciones.retain(|i| ids.contains(i));
        self.revelados_acciones.retain(|i, _| ids.contains(i));
        let _ = clave;
        let _ = Duration::ZERO;
    }
}
