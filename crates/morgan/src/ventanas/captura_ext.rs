//! Las fotos de ventanas por los protocolos estándar (Umbriel, y cualquier compositor que los
//! tenga): `ext-foreign-toplevel-list` da cada ventana con su identificador, que en Umbriel es
//! el mismo id del IPC; `ext-image-copy-capture` la copia a un buffer de memoria compartida.
//! Una sesión por foto: se pide el tamaño, se copia un cuadro y se suelta todo.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::File;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::fs::FileExt;
use std::rc::Rc;
use std::time::Duration;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum, event_created_child};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self as manija, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self as lista, ExtForeignToplevelListV1},
};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::{self as cuadro, ExtImageCopyCaptureFrameV1},
    ext_image_copy_capture_manager_v1::{ExtImageCopyCaptureManagerV1, Options},
    ext_image_copy_capture_session_v1::{self as sesion, ExtImageCopyCaptureSessionV1},
};

use super::captura::{Foto, a_rgba, achicar, archivo_anonimo, memoria};

/// Como en Hyprland: una captura de verdad tarda milisegundos.
const TOPE: Duration = Duration::from_millis(400);

enum Ev {
    Manija(ExtForeignToplevelHandleV1, manija::Event),
    Sesion(u64, sesion::Event),
    Cuadro(u64, cuadro::Event),
}

#[derive(Default)]
struct Estado {
    eventos: Vec<Ev>,
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

impl Dispatch<ExtForeignToplevelListV1, ()> for Estado {
    fn event(
        _: &mut Self,
        _: &ExtForeignToplevelListV1,
        _: lista::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(Estado, ExtForeignToplevelListV1, [
        lista::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for Estado {
    fn event(
        e: &mut Self,
        m: &ExtForeignToplevelHandleV1,
        evento: manija::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        e.eventos.push(Ev::Manija(m.clone(), evento));
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, u64> for Estado {
    fn event(
        e: &mut Self,
        _: &ExtImageCopyCaptureSessionV1,
        evento: sesion::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        e.eventos.push(Ev::Sesion(*id, evento));
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, u64> for Estado {
    fn event(
        e: &mut Self,
        _: &ExtImageCopyCaptureFrameV1,
        evento: cuadro::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        e.eventos.push(Ev::Cuadro(*id, evento));
    }
}

wayland_client::delegate_noop!(Estado: ignore ExtForeignToplevelImageCaptureSourceManagerV1);
wayland_client::delegate_noop!(Estado: ignore ExtImageCaptureSourceV1);
wayland_client::delegate_noop!(Estado: ignore ExtImageCopyCaptureManagerV1);
wayland_client::delegate_noop!(Estado: ignore wl_shm::WlShm);
wayland_client::delegate_noop!(Estado: ignore wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(Estado: ignore wl_buffer::WlBuffer);

struct Pedido {
    fuente: ExtImageCaptureSourceV1,
    sesion: ExtImageCopyCaptureSessionV1,
    tam: Option<(u32, u32)>,
    formato: Option<wl_shm::Format>,
    cuadro: Option<ExtImageCopyCaptureFrameV1>,
    copia: Option<(File, wl_shm_pool::WlShmPool, wl_buffer::WlBuffer, u32)>,
    ancho_max: i32,
    tx: async_channel::Sender<Option<Foto>>,
}

impl Pedido {
    fn soltar(self) {
        if let Some((_, pool, buffer, _)) = self.copia {
            buffer.destroy();
            pool.destroy();
        }
        if let Some(c) = self.cuadro {
            c.destroy();
        }
        self.sesion.destroy();
        self.fuente.destroy();
    }
}

pub struct CapturadorExt {
    conn: Connection,
    cola: RefCell<EventQueue<Estado>>,
    qh: QueueHandle<Estado>,
    fuentes: ExtForeignToplevelImageCaptureSourceManagerV1,
    copias: ExtImageCopyCaptureManagerV1,
    shm: wl_shm::WlShm,
    /// Por identificador, las ventanas que la lista dio y no cerró.
    ventanas: RefCell<HashMap<String, ExtForeignToplevelHandleV1>>,
    pedidos: RefCell<HashMap<u64, Pedido>>,
    siguiente: Cell<u64>,
    escucha: RefCell<Option<glib::SourceId>>,
}

impl CapturadorExt {
    pub fn conectar() -> Result<Rc<Self>, String> {
        let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
        let (globals, cola) = registry_queue_init::<Estado>(&conn).map_err(|e| e.to_string())?;
        let qh = cola.handle();
        let falta = |n: &str, e: wayland_client::globals::BindError| format!("{n}: {e}");
        let _lista: ExtForeignToplevelListV1 = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| falta("ext_foreign_toplevel_list_v1", e))?;
        let fuentes = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| falta("ext_foreign_toplevel_image_capture_source_manager_v1", e))?;
        let copias = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| falta("ext_image_copy_capture_manager_v1", e))?;
        let shm = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| falta("wl_shm", e))?;
        let c = Rc::new(CapturadorExt {
            conn,
            cola: RefCell::new(cola),
            qh,
            fuentes,
            copias,
            shm,
            ventanas: RefCell::default(),
            pedidos: RefCell::default(),
            siguiente: Cell::new(0),
            escucha: RefCell::default(),
        });
        // La lista inicial, antes del primer pedido.
        {
            let mut estado = Estado::default();
            c.cola
                .borrow_mut()
                .roundtrip(&mut estado)
                .map_err(|e| e.to_string())?;
            c.atender(estado);
        }
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
                    c.escucha.take();
                    return glib::ControlFlow::Break;
                }
                c.leer();
                glib::ControlFlow::Continue
            },
        );
        c.escucha.replace(Some(id));
        Ok(c)
    }

    pub async fn capturar(self: &Rc<Self>, ventana: &str, ancho_max: i32) -> Option<Foto> {
        // Una ventana recién abierta puede no haber llegado todavía.
        self.leer();
        let manija = self.ventanas.borrow().get(ventana).cloned()?;
        let id = self.siguiente.get();
        self.siguiente.set(id + 1);
        let (tx, rx) = async_channel::bounded(1);
        let fuente = self.fuentes.create_source(&manija, &self.qh, ());
        let sesion = self
            .copias
            .create_session(&fuente, Options::empty(), &self.qh, id);
        self.pedidos.borrow_mut().insert(
            id,
            Pedido {
                fuente,
                sesion,
                tam: None,
                formato: None,
                cuadro: None,
                copia: None,
                ancho_max,
                tx,
            },
        );
        self.flush();
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
                let _ = g.read();
            }
            if let Err(e) = cola.dispatch_pending(&mut estado) {
                eprintln!("morgan: ventanas: {e}");
            }
        }
        self.atender(estado);
    }

    fn atender(self: &Rc<Self>, estado: Estado) {
        for ev in estado.eventos {
            match ev {
                Ev::Manija(m, manija::Event::Identifier { identifier }) => {
                    self.ventanas.borrow_mut().insert(identifier, m);
                }
                Ev::Manija(m, manija::Event::Closed) => {
                    self.ventanas.borrow_mut().retain(|_, x| *x != m);
                    m.destroy();
                }
                Ev::Manija(..) => {}
                Ev::Sesion(id, e) => self.evento_sesion(id, e),
                Ev::Cuadro(id, e) => self.evento_cuadro(id, e),
            }
        }
    }

    fn evento_sesion(&self, id: u64, e: sesion::Event) {
        match e {
            sesion::Event::BufferSize { width, height } => {
                if let Some(p) = self.pedidos.borrow_mut().get_mut(&id) {
                    p.tam = Some((width, height));
                }
            }
            sesion::Event::ShmFormat {
                format: WEnum::Value(f),
            } => {
                if let Some(p) = self.pedidos.borrow_mut().get_mut(&id)
                    && p.formato.is_none()
                    && memoria(f)
                {
                    p.formato = Some(f);
                }
            }
            sesion::Event::Done => self.copiar(id),
            sesion::Event::Stopped => self.terminar(id, None),
            _ => {}
        }
    }

    fn copiar(&self, id: u64) {
        let creado = (|| {
            let mut pedidos = self.pedidos.borrow_mut();
            let p = pedidos.get_mut(&id)?;
            if p.cuadro.is_some() {
                return Some(());
            }
            let ((w, h), f) = (p.tam?, p.formato?);
            let stride = w.checked_mul(4)?;
            let largo = stride.checked_mul(h)?;
            let archivo = archivo_anonimo(u64::from(largo)).ok()?;
            let pool = self
                .shm
                .create_pool(archivo.as_fd(), largo as i32, &self.qh, ());
            let buffer = pool.create_buffer(0, w as i32, h as i32, stride as i32, f, &self.qh, ());
            let c = p.sesion.create_frame(&self.qh, id);
            c.attach_buffer(&buffer);
            c.damage_buffer(0, 0, w as i32, h as i32);
            c.capture();
            p.cuadro = Some(c);
            p.copia = Some((archivo, pool, buffer, stride));
            Some(())
        })();
        if creado.is_none() {
            self.terminar(id, None);
        } else {
            self.flush();
        }
    }

    fn evento_cuadro(&self, id: u64, e: cuadro::Event) {
        match e {
            cuadro::Event::Ready => self.leer_copia(id),
            cuadro::Event::Failed { .. } => self.terminar(id, None),
            _ => {}
        }
    }

    fn leer_copia(&self, id: u64) {
        let Some(p) = self.pedidos.borrow_mut().remove(&id) else {
            return;
        };
        let (Some((w, h)), Some(f), Some((archivo, _, _, stride))) =
            (p.tam, p.formato, p.copia.as_ref())
        else {
            let _ = p.tx.try_send(None);
            p.soltar();
            return;
        };
        let (w, h, stride) = (w, h, *stride);
        let archivo = archivo.try_clone();
        let (ancho_max, tx) = (p.ancho_max, p.tx.clone());
        p.soltar();
        self.flush();
        let Ok(archivo) = archivo else {
            let _ = tx.try_send(None);
            return;
        };
        let opaco = !matches!(f, wl_shm::Format::Argb8888);
        glib::MainContext::default().spawn_local(async move {
            let achicada = gio::spawn_blocking(move || {
                let mut datos = vec![0u8; (stride * h) as usize];
                archivo.read_exact_at(&mut datos, 0).ok()?;
                Some(achicar(
                    &datos,
                    w as usize,
                    h as usize,
                    stride as usize,
                    false,
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

impl Drop for CapturadorExt {
    fn drop(&mut self) {
        if let Some(id) = self.escucha.take() {
            id.remove();
        }
    }
}
