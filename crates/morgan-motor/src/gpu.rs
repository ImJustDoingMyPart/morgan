//! La GPU y lo que cada superficie de Wayland necesita para presentar con ella.
//!
//! vello escribe colores sin premultiplicar en una textura propia (el lienzo); se pasan a la
//! superficie con una mezcla `ALPHA_BLENDING` sobre transparente, que los premultiplica (la
//! superficie es `PreMultiplied`, para que lo transparente deje ver lo de atrás).

use vello::wgpu::rwh::{
    DisplayHandle, HandleError, HasDisplayHandle, RawDisplayHandle, RawWindowHandle,
    WaylandDisplayHandle, WaylandWindowHandle,
};
use vello::wgpu::{self, util::TextureBlitter};
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Proxy};

pub struct Gpu {
    /// Quién avisa en los mensajes de error (`morgan`, `morgan-lock`).
    pub quien: &'static str,
    pub instancia: wgpu::Instance,
    pub adaptador: wgpu::Adapter,
    pub device: wgpu::Device,
    pub cola: wgpu::Queue,
    pub vello: Renderer,
    /// Por formato de superficie (casi siempre uno solo).
    copiar: Vec<(wgpu::TextureFormat, TextureBlitter)>,
}

/// El display de libwayland, para wgpu (el backend GL lo necesita al crear la instancia).
#[derive(Debug)]
struct Display(std::ptr::NonNull<std::ffi::c_void>);

// SAFETY: es el puntero del `wl_display` de libwayland, que es seguro entre hilos y vive
// mientras viva la `Connection` que lo creó (quien abre la GPU la conserva hasta el final).
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

impl Gpu {
    /// Para dibujar en las superficies de `conn`.
    pub fn con_display(quien: &'static str, conn: &Connection) -> Gpu {
        Gpu::abrir(
            quien,
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display(conn))),
        )
    }

    /// Sin compositor: para las pruebas que dibujan a una textura.
    pub fn sin_display(quien: &'static str) -> Gpu {
        Gpu::abrir(
            quien,
            wgpu::InstanceDescriptor::new_without_display_handle(),
        )
    }

    fn abrir(quien: &'static str, desc: wgpu::InstanceDescriptor) -> Gpu {
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
        device.on_uncaptured_error(std::sync::Arc::new(move |e| eprintln!("{quien}: gpu: {e}")));
        let vello = Renderer::new(
            &device,
            RendererOptions {
                antialiasing_support: AaSupport::area_only(),
                ..Default::default()
            },
        )
        .expect("vello");
        Gpu {
            quien,
            instancia,
            adaptador,
            device,
            cola,
            vello,
            copiar: Vec::new(),
        }
    }

    /// Una textura donde vello puede escribir y desde donde se puede copiar o leer.
    pub fn textura(&self, w: u32, h: u32, uso: wgpu::TextureUsages) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lienzo"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | uso,
            view_formats: &[],
        })
    }

    /// `escena` en `vista`, sobre `fondo`.
    pub fn dibujar(
        &mut self,
        escena: &Scene,
        vista: &wgpu::TextureView,
        (w, h): (u32, u32),
        fondo: vello::peniko::Color,
    ) -> bool {
        let params = RenderParams {
            base_color: fondo,
            width: w,
            height: h,
            antialiasing_method: AaConfig::Area,
        };
        match self
            .vello
            .render_to_texture(&self.device, &self.cola, escena, vista, &params)
        {
            Ok(()) => true,
            Err(e) => {
                eprintln!("{}: vello: {e}", self.quien);
                false
            }
        }
    }
}

/// Lo de wgpu de una `wl_surface`: se suelta antes de que se destruya la superficie.
pub struct Lienzo {
    superficie: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    escala: i32,
    textura: Option<(wgpu::Texture, wgpu::TextureView)>,
}

impl Lienzo {
    /// # Safety
    ///
    /// `wl` tiene que vivir más que el `Lienzo`.
    pub unsafe fn nuevo(gpu: &mut Gpu, conn: &Connection, wl: &WlSurface) -> Option<Lienzo> {
        let quien = gpu.quien;
        let ptr = std::ptr::NonNull::new(wl.id().as_ptr())?.cast();
        let objetivo = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(display(conn).raw()),
            raw_window_handle: RawWindowHandle::Wayland(WaylandWindowHandle::new(ptr)),
        };
        // SAFETY: lo garantiza quien llama.
        let superficie = match unsafe { gpu.instancia.create_surface_unsafe(objetivo) } {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{quien}: superficie: {e}");
                return None;
            }
        };
        let cap = superficie.get_capabilities(&gpu.adaptador);
        let Some(formato) = cap.formats.iter().copied().find(|f| {
            matches!(
                f,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
            )
        }) else {
            eprintln!("{quien}: la superficie no tiene un formato sin sRGB");
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
        if !gpu.copiar.iter().any(|(f, _)| *f == formato) {
            let b = wgpu::util::TextureBlitterBuilder::new(&gpu.device, formato)
                .blend_state(wgpu::BlendState::ALPHA_BLENDING)
                .build();
            gpu.copiar.push((formato, b));
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
        Some(Lienzo {
            superficie,
            config,
            escala: 1,
            textura: None,
        })
    }

    /// Dibuja `escena` (en píxeles del buffer) y la presenta en `wl`, de `ancho` × `alto`
    /// lógicos a `escala`. `antes` corre justo antes del commit que hace `present`: ahí va el
    /// pedido del próximo frame callback. Falso si no se presentó.
    pub fn presentar(
        &mut self,
        gpu: &mut Gpu,
        wl: &WlSurface,
        escena: &Scene,
        (ancho, alto): (u32, u32),
        escala: i32,
        antes: impl FnOnce(),
    ) -> bool {
        let (bw, bh) = (ancho * escala as u32, alto * escala as u32);
        if bw == 0 || bh == 0 {
            return false;
        }
        if self.config.width != bw || self.config.height != bh || self.escala != escala {
            // La escala va junto con el primer buffer del tamaño nuevo, en el mismo commit.
            wl.set_buffer_scale(escala);
            self.escala = escala;
            self.config.width = bw;
            self.config.height = bh;
            self.superficie.configure(&gpu.device, &self.config);
            self.textura = None;
        }
        let (_, vista) = self.textura.get_or_insert_with(|| {
            let tex = gpu.textura(bw, bh, wgpu::TextureUsages::TEXTURE_BINDING);
            let v = tex.create_view(&Default::default());
            (tex, v)
        });
        if !gpu.dibujar(escena, vista, (bw, bh), vello::peniko::Color::TRANSPARENT) {
            return false;
        }

        let marco = match self.superficie.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.superficie.configure(&gpu.device, &self.config);
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
        if let Some((_, b)) = gpu.copiar.iter().find(|(f, _)| *f == self.config.format) {
            b.copy(&gpu.device, &mut enc, vista, &destino);
        }
        gpu.cola.submit([enc.finish()]);
        antes();
        gpu.cola.present(marco);
        true
    }
}
