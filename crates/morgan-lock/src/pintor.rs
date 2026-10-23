//! El hilo de render. Es dueño de wgpu, de vello y de las superficies de wgpu; recibe del
//! principal las superficies y los `Cambio`s, y anima solo, a ritmo de pantalla: si el
//! principal se traba (PAM, logind, un sleep a propósito), la onda no se entera.
//!
//! Los frame callbacks van a una `EventQueue` propia de este hilo: cada salida pide el
//! siguiente cuadro al presentar y no dibuja otro hasta que el compositor lo suelta. El modo
//! de presentación es Mailbox (si está): no se espera dos veces.
//!
//! vello escribe colores sin premultiplicar en una textura propia; se pasan a la superficie
//! con una mezcla `ALPHA_BLENDING` sobre transparente, que los premultiplica (la superficie es
//! `PreMultiplied`, para que el fundido deje ver el escritorio).

use std::sync::mpsc;
use std::thread::JoinHandle;

use smithay_client_toolkit::reexports::calloop::channel::{self, Channel, Sender};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopSignal};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use vello::wgpu::rwh::{
    DisplayHandle, HandleError, HasDisplayHandle, RawDisplayHandle, RawWindowHandle,
    WaylandDisplayHandle, WaylandWindowHandle,
};
use vello::wgpu::{self, util::TextureBlitter};
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};
use wayland_client::protocol::{wl_callback, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

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

struct Gpu {
    instancia: wgpu::Instance,
    adaptador: wgpu::Adapter,
    device: wgpu::Device,
    cola: wgpu::Queue,
    vello: Renderer,
    /// Por formato de superficie (casi siempre uno solo).
    copiar: Vec<(wgpu::TextureFormat, TextureBlitter)>,
}

struct Salida {
    id: u32,
    wl: WlSurface,
    superficie: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    ancho: u32,
    alto: u32,
    escala: i32,
    escala_puesta: i32,
    /// Hay un frame callback pendiente: el compositor todavía no quiere otro cuadro.
    esperando: bool,
    /// Algo cambió aunque no haya animación (un tamaño, el reloj).
    sucio: bool,
    rotulos: Option<Rotulos>,
    lienzo: Option<(wgpu::Texture, wgpu::TextureView)>,
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

/// El display de libwayland, para wgpu (el backend GL lo necesita al crear la instancia).
#[derive(Debug)]
struct Display(std::ptr::NonNull<std::ffi::c_void>);

// SAFETY: es el puntero del `wl_display` de libwayland, que es seguro entre hilos y vive
// mientras viva la `Connection` que lo creó (la tiene el principal hasta el final).
unsafe impl Send for Display {}
unsafe impl Sync for Display {}

impl Display {
    fn raw(&self) -> RawDisplayHandle {
        RawDisplayHandle::Wayland(WaylandDisplayHandle::new(self.0))
    }
}

impl HasDisplayHandle for Display {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: ver arriba; la conexión dura más que la instancia.
        Ok(unsafe { DisplayHandle::borrow_raw(self.raw()) })
    }
}

fn display(conn: &Connection) -> Display {
    let p = conn.backend().display_ptr().cast();
    Display(std::ptr::NonNull::new(p).expect("display de libwayland"))
}

fn abrir_gpu(conn: &Connection) -> Gpu {
    let desc = wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display(conn)));
    let instancia = wgpu::Instance::new(desc);
    let adaptador = futures_lite::future::block_on(
        instancia.request_adapter(&wgpu::RequestAdapterOptions::default()),
    )
    .expect("no hay GPU para dibujar");
    let (device, cola) =
        futures_lite::future::block_on(adaptador.request_device(&wgpu::DeviceDescriptor {
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .expect("no se pudo abrir la GPU");
    device.on_uncaptured_error(std::sync::Arc::new(|e| eprintln!("morgan-lock: gpu: {e}")));
    let vello = Renderer::new(
        &device,
        RendererOptions {
            antialiasing_support: AaSupport::area_only(),
            ..Default::default()
        },
    )
    .expect("vello");
    Gpu {
        instancia,
        adaptador,
        device,
        cola,
        vello,
        copiar: Vec::new(),
    }
}

fn correr(conn: Connection, estado: Estado, fondo: JoinHandle<Option<Fondo>>, rx: Channel<Orden>) {
    let gpu = abrir_gpu(&conn);
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
        let ptr = std::ptr::NonNull::new(wl.id().as_ptr())?.cast();
        let objetivo = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(display(&self.conn).raw()),
            raw_window_handle: RawWindowHandle::Wayland(WaylandWindowHandle::new(ptr)),
        };
        // SAFETY: la wl_surface vive hasta que esta Salida se suelta: el principal la destruye
        // recién después de `Quitar` (que espera la respuesta) o de que este hilo termina.
        let superficie = match unsafe { self.gpu.instancia.create_surface_unsafe(objetivo) } {
            Ok(s) => s,
            Err(e) => {
                eprintln!("morgan-lock: superficie: {e}");
                return None;
            }
        };
        let cap = superficie.get_capabilities(&self.gpu.adaptador);
        let Some(formato) = cap.formats.iter().copied().find(|f| {
            matches!(
                f,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
            )
        }) else {
            eprintln!("morgan-lock: la superficie no tiene un formato sin sRGB");
            return None;
        };
        let alfa = if cap
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else {
            cap.alpha_modes[0]
        };
        let modo = if cap.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };
        if !self.gpu.copiar.iter().any(|(f, _)| *f == formato) {
            let b = wgpu::util::TextureBlitterBuilder::new(&self.gpu.device, formato)
                .blend_state(wgpu::BlendState::ALPHA_BLENDING)
                .build();
            self.gpu.copiar.push((formato, b));
        }
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: formato,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: 0,
            height: 0,
            present_mode: modo,
            desired_maximum_frame_latency: 2,
            alpha_mode: alfa,
            view_formats: Vec::new(),
        };
        Some(Salida {
            id,
            wl,
            superficie,
            config,
            ancho,
            alto,
            escala,
            escala_puesta: 1,
            esperando: false,
            sucio: true,
            rotulos: None,
            lienzo: None,
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
        let (bw, bh) = (s.ancho * s.escala as u32, s.alto * s.escala as u32);
        if s.config.width != bw || s.config.height != bh || s.escala != s.escala_puesta {
            // La escala va junto con el primer buffer del tamaño nuevo, en el mismo commit.
            s.wl.set_buffer_scale(s.escala);
            s.escala_puesta = s.escala;
            s.config.width = bw;
            s.config.height = bh;
            s.superficie.configure(&gpu.device, &s.config);
            s.lienzo = None;
        }
        let version = Rotulos::version_de(estado);
        if s.rotulos.as_ref().is_none_or(|r| r.version != version) {
            let mut r = Rotulos::armar(textos, estado, s.escala as f32);
            r.version = version;
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

        let (_, vista) = s.lienzo.get_or_insert_with(|| {
            let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("lienzo"),
                size: wgpu::Extent3d {
                    width: bw,
                    height: bh,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let v = tex.create_view(&Default::default());
            (tex, v)
        });
        let params = RenderParams {
            base_color: vello::peniko::Color::TRANSPARENT,
            width: bw,
            height: bh,
            antialiasing_method: AaConfig::Area,
        };
        if let Err(e) = gpu
            .vello
            .render_to_texture(&gpu.device, &gpu.cola, escena, vista, &params)
        {
            eprintln!("morgan-lock: vello: {e}");
            return false;
        }

        let marco = match s.superficie.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                s.superficie.configure(&gpu.device, &s.config);
                return false;
            }
            _ => return false,
        };
        let destino = marco.texture.create_view(&Default::default());
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &destino,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        if let Some((_, b)) = gpu.copiar.iter().find(|(f, _)| *f == s.config.format) {
            b.copy(&gpu.device, &mut enc, vista, &destino);
        }
        gpu.cola.submit([enc.finish()]);
        // El pedido del próximo cuadro va antes del commit que hace `present`.
        s.wl.frame(qh, s.id);
        s.esperando = true;
        s.sucio = false;
        gpu.cola.present(marco);
        true
    }
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
