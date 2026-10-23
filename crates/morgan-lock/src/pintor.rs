//! El hilo de render. Es dueño de wgpu, de vello y de las superficies de wgpu; recibe del
//! principal las superficies y los `Cambio`s, y anima solo, a ritmo de pantalla: si el
//! principal se traba (PAM, logind, un sleep a propósito), la onda no se entera.
//!
//! Los frame callbacks van a una `EventQueue` propia de este hilo: cada salida pide el
//! siguiente cuadro al presentar y no dibuja otro hasta que el compositor lo suelta. El modo
//! de presentación es Mailbox (si está): no se espera dos veces.

use std::sync::mpsc;
use std::thread::JoinHandle;

use morgan_motor::gpu::{Gpu, Lienzo};
use smithay_client_toolkit::reexports::calloop::channel::{self, Channel, Sender};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopSignal};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use vello::Scene;
use wayland_client::protocol::{wl_callback, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, QueueHandle};

use crate::dibujo::{self, Fondo, Rotulos};
use crate::escena::{Cambio, Estado, ahora};
use crate::texto::Textos;

pub enum Orden {
    /// Una superficie nueva o con otro tamaño (lógico) o escala.
    Superficie {
        id: u32,
        wl: WlSurface,
        ancho: u32,
        alto: u32,
        escala: i32,
    },
    /// Se va una salida: se suelta su superficie de wgpu y se avisa, antes de que el
    /// principal destruya la de Wayland.
    Quitar {
        id: u32,
        listo: mpsc::Sender<()>,
    },
    Cambio(Cambio),
    /// Termina el hilo (después del fundido de salida).
    Fin,
}

pub struct Hilo {
    pub ordenes: Sender<Orden>,
    hilo: Option<JoinHandle<()>>,
}

impl Hilo {
    pub fn mandar(&self, o: Orden) {
        // Si el hilo se cayó no hay a quién avisarle; el principal sigue y puede desbloquear.
        let _ = self.ordenes.send(o);
    }

    pub fn cambio(&self, c: Cambio) {
        self.mandar(Orden::Cambio(c));
    }

    /// Lo deja terminar: no puede quedar presentando sobre superficies destruidas.
    pub fn terminar(&mut self) {
        self.mandar(Orden::Fin);
        if let Some(h) = self.hilo.take() {
            let _ = h.join();
        }
    }
}

/// `fondo` se está cargando en otro hilo: se espera recién con la GPU lista.
pub fn lanzar(conn: Connection, estado: Estado, fondo: JoinHandle<Option<Fondo>>) -> Hilo {
    let (tx, rx) = channel::channel();
    let hilo = std::thread::Builder::new()
        .name("morgan-lock-render".into())
        .spawn(move || correr(conn, estado, fondo, rx))
        .expect("hilo de render");
    Hilo {
        ordenes: tx,
        hilo: Some(hilo),
    }
}

struct Salida {
    id: u32,
    wl: WlSurface,
    lienzo: Lienzo,
    ancho: u32,
    alto: u32,
    escala: i32,
    /// Hay un frame callback pendiente: el compositor todavía no quiere otro cuadro.
    esperando: bool,
    /// Algo cambió aunque no haya animación (un tamaño, el reloj).
    sucio: bool,
    rotulos: Option<Rotulos>,
}

struct Pintor {
    conn: Connection,
    qh: QueueHandle<Pintor>,
    gpu: Gpu,
    estado: Estado,
    fondo: Option<Fondo>,
    textos: Textos,
    salidas: Vec<Salida>,
    escena: Scene,
    fin: LoopSignal,
}

fn correr(conn: Connection, estado: Estado, fondo: JoinHandle<Option<Fondo>>, rx: Channel<Orden>) {
    let gpu = Gpu::con_display("morgan-lock", &conn);
    let fondo = fondo.join().ok().flatten();
    let cola = conn.new_event_queue::<Pintor>();
    let qh = cola.handle();
    let mut bucle: EventLoop<Pintor> = EventLoop::try_new().expect("bucle de render");
    let fin = bucle.get_signal();
    WaylandSource::new(conn.clone(), cola)
        .insert(bucle.handle())
        .expect("cola de render");
    bucle
        .handle()
        .insert_source(rx, |ev, _, p: &mut Pintor| match ev {
            channel::Event::Msg(o) => p.orden(o),
            channel::Event::Closed => p.fin.stop(),
        })
        .expect("canal de render");
    let mut p = Pintor {
        conn,
        qh,
        gpu,
        estado,
        fondo,
        textos: Textos::new(),
        salidas: Vec::new(),
        escena: Scene::new(),
        fin,
    };
    let _ = bucle.run(None, &mut p, |p| p.pintar());
    // Las superficies de wgpu se sueltan antes que nada de Wayland (el principal espera).
    p.salidas.clear();
}

impl Pintor {
    fn orden(&mut self, o: Orden) {
        match o {
            Orden::Superficie {
                id,
                wl,
                ancho,
                alto,
                escala,
            } => {
                if let Some(s) = self.salidas.iter_mut().find(|s| s.id == id) {
                    (s.ancho, s.alto, s.escala, s.sucio) = (ancho, alto, escala, true);
                    s.rotulos = None;
                } else if let Some(s) = self.nueva(id, wl, ancho, alto, escala) {
                    self.salidas.push(s);
                }
            }
            Orden::Quitar { id, listo } => {
                self.salidas.retain(|s| s.id != id);
                let _ = listo.send(());
            }
            Orden::Cambio(c) => {
                let reloj = matches!(c, Cambio::Reloj { .. } | Cambio::Aviso(_));
                self.estado.aplicar(c);
                if reloj {
                    for s in &mut self.salidas {
                        s.sucio = true;
                    }
                }
            }
            Orden::Fin => self.fin.stop(),
        }
    }

    fn nueva(
        &mut self,
        id: u32,
        wl: WlSurface,
        ancho: u32,
        alto: u32,
        escala: i32,
    ) -> Option<Salida> {
        // SAFETY: la wl_surface vive hasta que esta Salida se suelta: el principal la destruye
        // recién después de `Quitar` (que espera la respuesta) o de que este hilo termina.
        let lienzo = unsafe { Lienzo::nuevo(&mut self.gpu, &self.conn, &wl) }?;
        Some(Salida {
            id,
            wl,
            lienzo,
            ancho,
            alto,
            escala,
            esperando: false,
            sucio: true,
            rotulos: None,
        })
    }

    /// Después de cada vuelta del bucle: dibuja las salidas que lo necesitan y pueden.
    fn pintar(&mut self) {
        let t = ahora();
        self.estado.avanzar(t);
        let anima = self.estado.animando(t);
        for i in 0..self.salidas.len() {
            let s = &self.salidas[i];
            if s.esperando || !(s.sucio || anima) || s.ancho == 0 || s.alto == 0 {
                continue;
            }
            if self.estado.t_entrada.is_none() {
                self.estado.t_entrada = Some(t);
            }
            self.cuadro(i, t);
        }
        let _ = self.conn.flush();
    }

    fn cuadro(&mut self, i: usize, t: i64) -> bool {
        let Pintor {
            gpu,
            salidas,
            estado,
            fondo,
            textos,
            escena,
            qh,
            ..
        } = self;
        let s = &mut salidas[i];
        let version = Rotulos::version_de(estado);
        if s.rotulos.as_ref().is_none_or(|r| r.version != version) {
            let mut r = Rotulos::armar(textos, estado, s.escala as f32);
            r.version = version;
            sombras(gpu, &mut r, s.escala as f32);
            s.rotulos = Some(r);
        }
        let Some(rot) = s.rotulos.as_mut() else {
            return false;
        };
        dibujo::pintar(
            escena,
            estado,
            rot,
            textos,
            fondo.as_ref(),
            (s.ancho, s.alto),
            s.escala as f64,
            t,
        );

        // El pedido del próximo cuadro va antes del commit que hace `present`.
        let (wl, id) = (&s.wl, s.id);
        if !s
            .lienzo
            .presentar(gpu, wl, escena, (s.ancho, s.alto), s.escala, || {
                wl.frame(qh, id);
            })
        {
            return false;
        }
        s.esperando = true;
        s.sucio = false;
        true
    }
}

/// Las sombras del texto necesitan la GPU: van acá y no en `Rotulos::armar`.
fn sombras(gpu: &mut Gpu, r: &mut Rotulos, escala: f32) {
    let capas = r.capas();
    let hechas = std::array::from_fn(|i| {
        let Some(l) = capas[i] else {
            return Vec::new();
        };
        dibujo::sombras_de(i)
            .iter()
            .filter_map(|&(sigma, alfa, baja)| {
                let (d, c, v) = (&gpu.device, &gpu.cola, &mut gpu.vello);
                crate::sombra::hacer(d, c, v, l, sigma * escala, alfa).map(|s| (s, baja))
            })
            .collect()
    });
    r.sombras = hechas;
}

impl Dispatch<wl_callback::WlCallback, u32> for Pintor {
    fn event(
        p: &mut Self,
        _: &wl_callback::WlCallback,
        ev: wl_callback::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = ev
            && let Some(s) = p.salidas.iter_mut().find(|s| s.id == *id)
        {
            s.esperando = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::escena::{MS, Paleta};
    use morgan_ipc::bloqueo::Bloqueo;

    /// Una pantalla entera sin compositor, para mirarla: con un juego abierto no se puede
    /// levantar el anidado.
    ///
    ///   MORGAN_LOCK_CAPTURA=/ruta.png cargo test -p morgan-lock -- --ignored captura
    #[test]
    #[ignore = "necesita GPU; escribe en MORGAN_LOCK_CAPTURA"]
    fn captura_sin_compositor() {
        let ruta = std::env::var_os("MORGAN_LOCK_CAPTURA").expect("MORGAN_LOCK_CAPTURA");
        let mut gpu = Gpu::sin_display("morgan-lock");
        let css = crate::config_dir()
            .and_then(|d| std::fs::read_to_string(d.join("morgan/theme.css")).ok())
            .unwrap_or_default();
        let mut e = Estado::new(Paleta::desde_css(&css), Bloqueo::default());
        e.aplicar(Cambio::Reloj {
            hora: "21".into(),
            minutos: "37".into(),
            fecha: "jueves 22 de octubre".into(),
        });
        let t = 10_000 * MS;
        e.t_entrada = Some(0);
        e.avanzar(t);
        let fondo = crate::cargar_fondo();
        let mut textos = Textos::new();
        let mut rot = Rotulos::armar(&mut textos, &e, 1.0);
        sombras(&mut gpu, &mut rot, 1.0);
        let (w, h) = (2560, 1440);
        let mut escena = Scene::new();
        dibujo::pintar(
            &mut escena,
            &e,
            &mut rot,
            &mut textos,
            fondo.as_ref(),
            (w, h),
            1.0,
            t,
        );
        let tex = gpu.textura(w, h, vello::wgpu::TextureUsages::COPY_SRC);
        assert!(gpu.dibujar(
            &escena,
            &tex.create_view(&Default::default()),
            (w, h),
            vello::peniko::Color::BLACK,
        ));
        let px = crate::sombra::leer(&gpu.device, &gpu.cola, &tex, w, h).unwrap();
        let f = std::io::BufWriter::new(std::fs::File::create(ruta).unwrap());
        let mut enc = png::Encoder::new(f, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.write_header().unwrap().write_image_data(&px).unwrap();
    }
}
