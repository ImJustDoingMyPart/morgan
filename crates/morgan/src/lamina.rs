//! Lo de wgpu y el ritmo de cuadros de una superficie dibujada con el motor propio: la comparten
//! los hilos de la barra y del centro.

use morgan_motor::gpu::{Gpu, Lienzo};
use morgan_motor::vello::Scene;
use smithay_client_toolkit::compositor::FrameCallbackData;
use wayland_client::protocol::{wl_callback, wl_surface};
use wayland_client::{Dispatch, QueueHandle};

/// Con `MORGAN_CUADROS`, cada cuadro presentado se anota con su hilo y su instante
/// (`dev/traba` los cuenta).
static CUADROS: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| std::env::var_os("MORGAN_CUADROS").is_some());

/// Microsegundos desde la época: el mismo reloj en todos los hilos y en los registros.
pub fn ahora_us() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_micros())
}

pub struct Lamina {
    /// Antes que la superficie: lo de wgpu se suelta antes de que se destruya.
    pub lienzo: Option<Lienzo>,
    pub tam: (u32, u32),
    pub escala: i32,
    /// Hay un frame callback pendiente: el compositor todavía no quiere otro cuadro.
    pub esperando: bool,
    pub sucio: bool,
}

impl Lamina {
    pub fn new(tam: (u32, u32)) -> Self {
        Lamina {
            lienzo: None,
            tam,
            escala: 1,
            esperando: false,
            sucio: false,
        }
    }

    /// Dibuja `escena` en `wl` si hay lienzo y el compositor quiere otro cuadro.
    pub fn presentar<D>(
        &mut self,
        gpu: &mut Gpu,
        escena: &Scene,
        wl: &wl_surface::WlSurface,
        qh: &QueueHandle<D>,
    ) where
        D: Dispatch<wl_callback::WlCallback, FrameCallbackData> + 'static,
    {
        let Some(lienzo) = self.lienzo.as_mut() else {
            return;
        };
        // El pedido del próximo cuadro va antes del commit que hace `present`.
        if lienzo.presentar(gpu, wl, escena, self.tam, self.escala, || {
            wl.frame(qh, FrameCallbackData(wl.clone()));
        }) {
            self.esperando = true;
            self.sucio = false;
            if *CUADROS {
                let hilo = std::thread::current();
                eprintln!("morgan: cuadro {} {}", hilo.name().unwrap_or("?"), ahora_us());
            }
        }
    }

    pub fn lista(&self) -> bool {
        self.lienzo.is_some() && !self.esperando
    }
}
