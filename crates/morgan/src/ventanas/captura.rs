//! Una conexión Wayland propia (la de GTK no se comparte). En Hyprland 0.56.2, una ventana de
//! un workspace oculto da su último cuadro, y una que no toca su monitor no contesta nunca, ni
//! `ready` ni `failed`: por eso quien pide filtra antes por el rectángulo, y cada pedido tiene
//! un tope de tiempo.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::File;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::fs::FileExt;
use std::rc::Rc;
use std::time::Duration;

use gtk::{gio, glib};
use morgan_motor::vello::peniko::ImageData;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};

#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    unused_imports,
    clippy::all
)]
mod protocolo {
    use wayland_client;
    use wayland_client::protocol::*;
    use wayland_protocols_wlr::foreign_toplevel::v1::client::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        use wayland_protocols_wlr::foreign_toplevel::v1::client::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "/usr/share/hyprland-protocols/protocols/hyprland-toplevel-export-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!(
        "/usr/share/hyprland-protocols/protocols/hyprland-toplevel-export-v1.xml"
    );
}

use protocolo::hyprland_toplevel_export_frame_v1::{self as marco, HyprlandToplevelExportFrameV1};
use protocolo::hyprland_toplevel_export_manager_v1::HyprlandToplevelExportManagerV1;

/// Una captura de verdad tarda 5–10 ms.
const TOPE: Duration = Duration::from_millis(400);

/// RGBA premultiplicado.
pub struct Foto {
    pub ancho: i32,
    pub alto: i32,
    pub bytes: Vec<u8>,
}

impl Foto {
    pub fn imagen(self) -> ImageData {
        crate::icono::de_pixeles(self.bytes, self.ancho as u32, self.alto as u32)
    }
}

#[derive(Default)]
struct Estado {
    eventos: Vec<(u64, marco::Event)>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Estado {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

macro_rules! sin_eventos {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for Estado {
            fn event(
                _: &mut Self,
                _: &$t,
                _: <$t as wayland_client::Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    )*};
}

sin_eventos!(
    HyprlandToplevelExportManagerV1,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_buffer::WlBuffer
);

impl Dispatch<HyprlandToplevelExportFrameV1, u64> for Estado {
    fn event(
        estado: &mut Self,
        _: &HyprlandToplevelExportFrameV1,
        evento: marco::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        estado.eventos.push((*id, evento));
    }
}

struct Pedido {
    marco: HyprlandToplevelExportFrameV1,
    formato: Option<(wl_shm::Format, u32, u32, u32)>,
    copia: Option<(File, wl_shm_pool::WlShmPool, wl_buffer::WlBuffer)>,
    invertida: bool,
    ancho_max: i32,
    tx: async_channel::Sender<Option<Foto>>,
}

impl Pedido {
    fn soltar(self) {
        if let Some((_, pool, buffer)) = self.copia {
            buffer.destroy();
            pool.destroy();
        }
        self.marco.destroy();
    }
}

pub struct Capturador {
    conn: Connection,
    cola: RefCell<EventQueue<Estado>>,
    qh: QueueHandle<Estado>,
    manager: HyprlandToplevelExportManagerV1,
    shm: wl_shm::WlShm,
    pedidos: RefCell<HashMap<u64, Pedido>>,
    siguiente: Cell<u64>,
    fuente: RefCell<Option<glib::SourceId>>,
}

impl Capturador {
    pub fn conectar() -> Result<Rc<Self>, String> {
        let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
        let (globals, cola) = registry_queue_init::<Estado>(&conn).map_err(|e| e.to_string())?;
        let qh = cola.handle();
        let manager: HyprlandToplevelExportManagerV1 = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| format!("hyprland_toplevel_export_manager_v1: {e}"))?;
        let shm: wl_shm::WlShm = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| format!("wl_shm: {e}"))?;
        let c = Rc::new(Capturador {
            conn,
            cola: RefCell::new(cola),
            qh,
            manager,
            shm,
            pedidos: RefCell::default(),
            siguiente: Cell::new(0),
            fuente: RefCell::default(),
        });
        let fd = c.conn.backend().poll_fd().as_fd().as_raw_fd();
        let debil = Rc::downgrade(&c);
        let id = glib_unix::unix_fd_add_local(
            fd,
            glib::IOCondition::IN | glib::IOCondition::HUP | glib::IOCondition::ERR,
            move |_, cond| {
                let Some(c) = debil.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if cond.intersects(glib::IOCondition::HUP | glib::IOCondition::ERR) {
                    eprintln!("morgan: ventanas: se cortó la conexión con el compositor");
                    c.fuente.take();
                    return glib::ControlFlow::Break;
                }
                c.leer();
                glib::ControlFlow::Continue
            },
        );
        c.fuente.replace(Some(id));
        Ok(c)
    }

    pub async fn capturar(self: &Rc<Self>, direccion: &str, ancho_max: i32) -> Option<Foto> {
        let handle = u64::from_str_radix(direccion.trim_start_matches("0x"), 16).ok()? as u32;
        let id = self.siguiente.get();
        self.siguiente.set(id + 1);
        let (tx, rx) = async_channel::bounded(1);
        let marco = self.manager.capture_toplevel(0, handle, &self.qh, id);
        self.pedidos.borrow_mut().insert(
            id,
            Pedido {
                marco,
                formato: None,
                copia: None,
                invertida: false,
                ancho_max,
                tx,
            },
        );
        self.flush();
        // No se cancela: si vence con el pedido ya resuelto, no lo encuentra y no hace nada.
        let debil = Rc::downgrade(self);
        glib::timeout_add_local_once(TOPE, move || {
            if let Some(c) = debil.upgrade() {
                c.terminar(id, None);
            }
        });
        rx.recv().await.ok().flatten()
    }

    fn flush(&self) {
        if let Err(e) = self.conn.flush() {
            eprintln!("morgan: ventanas: {e}");
        }
    }

    fn terminar(&self, id: u64, foto: Option<Foto>) {
        if let Some(p) = self.pedidos.borrow_mut().remove(&id) {
            let _ = p.tx.try_send(foto);
            p.soltar();
            self.flush();
        }
    }

    fn leer(self: &Rc<Self>) {
        let mut estado = Estado::default();
        {
            let mut cola = self.cola.borrow_mut();
            if let Some(g) = cola.prepare_read() {
                // WouldBlock es posible (otro despertar ya leyó): no es error.
                let _ = g.read();
            }
            if let Err(e) = cola.dispatch_pending(&mut estado) {
                eprintln!("morgan: ventanas: {e}");
            }
        }
        for (id, evento) in estado.eventos {
            self.evento(id, evento);
        }
    }

    fn evento(self: &Rc<Self>, id: u64, evento: marco::Event) {
        match evento {
            marco::Event::Buffer {
                format: WEnum::Value(f),
                width,
                height,
                stride,
            } => {
                if let Some(p) = self.pedidos.borrow_mut().get_mut(&id)
                    && p.formato.is_none()
                    && memoria(f)
                {
                    p.formato = Some((f, width, height, stride));
                }
            }
            marco::Event::Flags {
                flags: WEnum::Value(f),
            } => {
                if let Some(p) = self.pedidos.borrow_mut().get_mut(&id) {
                    p.invertida = f.contains(marco::Flags::YInvert);
                }
            }
            marco::Event::BufferDone => self.copiar(id),
            marco::Event::Ready { .. } => self.leer_copia(id),
            marco::Event::Failed => self.terminar(id, None),
            _ => {}
        }
    }

    fn copiar(&self, id: u64) {
        let creado = (|| {
            let mut pedidos = self.pedidos.borrow_mut();
            let p = pedidos.get_mut(&id)?;
            let (f, w, h, stride) = p.formato?;
            let largo = stride.checked_mul(h)?;
            let archivo = archivo_anonimo(u64::from(largo)).ok()?;
            let pool = self
                .shm
                .create_pool(archivo.as_fd(), largo as i32, &self.qh, ());
            let buffer = pool.create_buffer(0, w as i32, h as i32, stride as i32, f, &self.qh, ());
            p.marco.copy(&buffer, 1);
            p.copia = Some((archivo, pool, buffer));
            Some(())
        })();
        if creado.is_none() {
            self.terminar(id, None);
        } else {
            self.flush();
        }
    }

    fn leer_copia(self: &Rc<Self>, id: u64) {
        let Some(p) = self.pedidos.borrow_mut().remove(&id) else {
            return;
        };
        let (Some((f, w, h, stride)), Some((archivo, _, _))) = (p.formato, p.copia.as_ref()) else {
            let _ = p.tx.try_send(None);
            p.soltar();
            return;
        };
        // El archivo es nuestro: se puede soltar el buffer antes de leerlo. Leer los 7 MB y
        // achicarlos va en otro hilo, para no trabar las teclas del lienzo.
        let archivo = archivo.try_clone();
        let (invertida, ancho_max, tx) = (p.invertida, p.ancho_max, p.tx.clone());
        p.soltar();
        self.flush();
        let Ok(archivo) = archivo else {
            let _ = tx.try_send(None);
            return;
        };
        let opaco = !matches!(f, wl_shm::Format::Argb8888);
        debug_assert!(memoria(f));
        glib::MainContext::default().spawn_local(async move {
            let achicada = gio::spawn_blocking(move || {
                let mut datos = vec![0u8; (stride * h) as usize];
                archivo.read_exact_at(&mut datos, 0).ok()?;
                Some(achicar(
                    &datos,
                    w as usize,
                    h as usize,
                    stride as usize,
                    invertida,
                    ancho_max,
                ))
            })
            .await
            .ok()
            .flatten();
            let foto = achicada.map(|(ancho, alto, mut bytes)| {
                a_rgba(&mut bytes, opaco);
                Foto {
                    ancho: ancho as i32,
                    alto: alto as i32,
                    bytes,
                }
            });
            let _ = tx.try_send(foto);
        });
    }
}

impl Drop for Capturador {
    fn drop(&mut self) {
        if let Some(id) = self.fuente.take() {
            id.remove();
        }
    }
}

/// Los formatos que se saben leer.
fn memoria(f: wl_shm::Format) -> bool {
    matches!(f, wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888)
}

/// De BGRA (lo que da el compositor) a RGBA; con `opaco`, el alfa que sobra pasa a 255.
fn a_rgba(px: &mut [u8], opaco: bool) {
    for p in px.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
        if opaco {
            p[3] = 255;
        }
    }
}

fn archivo_anonimo(largo: u64) -> std::io::Result<File> {
    let dir = glib::user_runtime_dir();
    let ruta = dir.join(format!(
        "morgan-captura-{}-{}",
        std::process::id(),
        glib::monotonic_time()
    ));
    let archivo = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&ruta)?;
    std::fs::remove_file(&ruta)?;
    archivo.set_len(largo)?;
    Ok(archivo)
}

pub fn achicar(
    datos: &[u8],
    w: usize,
    h: usize,
    stride: usize,
    invertida: bool,
    ancho_max: i32,
) -> (usize, usize, Vec<u8>) {
    let tw = w.min(ancho_max.max(1) as usize).max(1);
    let th = (h * tw / w.max(1)).max(1);
    let mut salida = vec![0u8; tw * th * 4];
    for ty in 0..th {
        let y0 = ty * h / th;
        let y1 = ((ty + 1) * h / th).max(y0 + 1);
        for tx in 0..tw {
            let x0 = tx * w / tw;
            let x1 = ((tx + 1) * w / tw).max(x0 + 1);
            let mut suma = [0u32; 4];
            for y in y0..y1 {
                let fila = if invertida { h - 1 - y } else { y };
                for x in x0..x1 {
                    let i = fila * stride + x * 4;
                    for (c, s) in suma.iter_mut().enumerate() {
                        *s += u32::from(datos[i + c]);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let o = (ty * tw + tx) * 4;
            for c in 0..4 {
                salida[o + c] = (suma[c] / n) as u8;
            }
        }
    }
    (tw, th, salida)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn achica_promediando_y_respeta_la_proporcion() {
        // 4×2, dos bloques de 2×2: uno negro, uno blanco; stride con relleno.
        let stride = 20;
        let mut datos = vec![0u8; stride * 2];
        for y in 0..2 {
            for x in 2..4 {
                for c in 0..4 {
                    datos[y * stride + x * 4 + c] = 255;
                }
            }
        }
        let (w, h, b) = achicar(&datos, 4, 2, stride, false, 2);
        assert_eq!((w, h), (2, 1));
        assert_eq!(&b[0..4], &[0, 0, 0, 0]);
        assert_eq!(&b[4..8], &[255, 255, 255, 255]);
    }

    #[test]
    fn bgra_pasa_a_rgba() {
        let mut px = [1u8, 2, 3, 40, 9, 8, 7, 0];
        a_rgba(&mut px, false);
        assert_eq!(px, [3, 2, 1, 40, 7, 8, 9, 0]);
        a_rgba(&mut px, true);
        assert_eq!(px, [1, 2, 3, 255, 9, 8, 7, 255]);
    }

    #[test]
    fn no_agranda_y_endereza() {
        // 1×2: arriba 10, abajo 20; invertida, la primera fila es la de abajo.
        let datos = [10u8, 10, 10, 10, 20, 20, 20, 20];
        let (w, h, b) = achicar(&datos, 1, 2, 4, true, 500);
        assert_eq!((w, h), (1, 2));
        assert_eq!(b, [20, 20, 20, 20, 10, 10, 10, 10]);
    }
}
