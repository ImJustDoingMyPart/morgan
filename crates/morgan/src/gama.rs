//! La luz nocturna fuera de Hyprland: `morgan nightlight TEMPERATURA` toma la gama de cada
//! salida por `wlr-gamma-control` y la deja en esa temperatura, con un fundido desde neutro.
//! Con SIGUSR1 funde de vuelta a neutro y sale; al salir (o caerse), el compositor repone la
//! gama de antes por su cuenta.
//!
//! Corre como unidad de systemd aparte (`luz.rs`), igual que `hyprsunset` en Hyprland.

use std::os::fd::AsFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use wayland_client::protocol::{wl_output, wl_registry};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::gamma_control::v1::client::{
    zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1,
    zwlr_gamma_control_v1::{self, ZwlrGammaControlV1},
};

/// Como el fundido de `hyprsunset`.
const FUNDIDO: Duration = Duration::from_millis(500);
const NEUTRO: f64 = 6500.0;

static APAGAR: AtomicBool = AtomicBool::new(false);

extern "C" fn al_apagar(_: libc::c_int) {
    APAGAR.store(true, Ordering::Relaxed);
}

/// Los multiplicadores de rojo, verde y azul de un cuerpo negro a `kelvin` (la aproximación de
/// Tanner Helland), normalizados para que 6500 K sea neutro.
pub fn color(kelvin: f64) -> [f64; 3] {
    fn crudo(k: f64) -> [f64; 3] {
        let t = k / 100.0;
        let r = if t <= 66.0 {
            255.0
        } else {
            329.698_727_446 * (t - 60.0).powf(-0.133_204_759_2)
        };
        let g = if t <= 66.0 {
            99.470_802_586_1 * t.ln() - 161.119_568_166_1
        } else {
            288.122_169_528_3 * (t - 60.0).powf(-0.075_514_849_2)
        };
        let b = if t >= 66.0 {
            255.0
        } else if t <= 19.0 {
            0.0
        } else {
            138.517_731_223_1 * (t - 10.0).ln() - 305.044_792_730_7
        };
        [r, g, b].map(|c| (c / 255.0).clamp(0.0, 1.0))
    }
    let n = crudo(NEUTRO);
    let c = crudo(kelvin.clamp(1000.0, 20000.0));
    [0, 1, 2].map(|i| (c[i] / n[i]).clamp(0.0, 1.0))
}

/// La tabla que pide el protocolo: rojo, verde y azul seguidos, `tam` valores de 16 bits cada
/// uno, en el orden de la máquina.
pub fn tabla(tam: usize, kelvin: f64) -> Vec<u8> {
    let m = color(kelvin);
    let mut v = Vec::with_capacity(tam * 3 * 2);
    for canal in m {
        for i in 0..tam {
            let x = if tam > 1 {
                i as f64 / (tam - 1) as f64
            } else {
                0.0
            };
            let valor = (x * canal * 65535.0).round().clamp(0.0, 65535.0) as u16;
            v.extend_from_slice(&valor.to_ne_bytes());
        }
    }
    v
}

struct Salida {
    control: ZwlrGammaControlV1,
    tam: Option<usize>,
    fallo: bool,
}

#[derive(Default)]
struct Estado {
    manager: Option<ZwlrGammaControlManagerV1>,
    /// Las que llegaron antes que el manager esperan acá.
    sin_control: Vec<wl_output::WlOutput>,
    controles: Vec<Salida>,
}

impl Estado {
    fn controlar(&mut self, s: &wl_output::WlOutput, qh: &QueueHandle<Self>) {
        match &self.manager {
            Some(m) => self.controles.push(Salida {
                control: m.get_gamma_control(s, qh, ()),
                tam: None,
                fallo: false,
            }),
            None => self.sin_control.push(s.clone()),
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for Estado {
    fn event(
        e: &mut Self,
        registro: &wl_registry::WlRegistry,
        evento: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = evento
        {
            match interface.as_str() {
                "zwlr_gamma_control_manager_v1" => {
                    e.manager = Some(registro.bind(name, 1, qh, ()));
                    for s in std::mem::take(&mut e.sin_control) {
                        e.controlar(&s, qh);
                    }
                }
                "wl_output" => {
                    let s: wl_output::WlOutput = registro.bind(name, version.min(4), qh, ());
                    e.controlar(&s, qh);
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<ZwlrGammaControlV1, ()> for Estado {
    fn event(
        e: &mut Self,
        control: &ZwlrGammaControlV1,
        evento: zwlr_gamma_control_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(s) = e.controles.iter_mut().find(|s| s.control == *control) else {
            return;
        };
        match evento {
            zwlr_gamma_control_v1::Event::GammaSize { size } => s.tam = Some(size as usize),
            zwlr_gamma_control_v1::Event::Failed => {
                eprintln!(
                    "morgan nightlight: una salida no deja cambiar su gama (¿otro programa la tiene?)"
                );
                s.fallo = true;
            }
            _ => {}
        }
    }
}

wayland_client::delegate_noop!(Estado: ignore ZwlrGammaControlManagerV1);
wayland_client::delegate_noop!(Estado: ignore wl_output::WlOutput);

fn poner(e: &Estado, kelvin: f64) {
    for s in e.controles.iter().filter(|s| !s.fallo) {
        let Some(tam) = s.tam else { continue };
        let datos = tabla(tam, kelvin);
        match memfd(&datos) {
            Ok(fd) => s.control.set_gamma(fd.as_fd()),
            Err(err) => eprintln!("morgan nightlight: {err}"),
        }
    }
}

fn memfd(datos: &[u8]) -> std::io::Result<std::os::fd::OwnedFd> {
    use std::io::{Seek, Write};
    use std::os::fd::FromRawFd;
    let fd = unsafe { libc::memfd_create(c"morgan-gama".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
    f.write_all(datos)?;
    f.rewind()?;
    Ok(f.into())
}

/// Funde de `desde` a `hasta`, atendiendo los eventos (una salida que aparece) en el medio.
fn fundir(
    con: &Connection,
    cola: &mut wayland_client::EventQueue<Estado>,
    e: &mut Estado,
    desde: f64,
    hasta: f64,
) -> Result<(), String> {
    let t0 = Instant::now();
    loop {
        cola.dispatch_pending(e).map_err(|x| x.to_string())?;
        let k = (t0.elapsed().as_secs_f64() / FUNDIDO.as_secs_f64()).min(1.0);
        // En mired (1/K) el fundido se ve parejo.
        let mired = 1.0 / desde + (1.0 / hasta - 1.0 / desde) * k;
        poner(e, 1.0 / mired);
        con.flush().map_err(|x| x.to_string())?;
        if k >= 1.0 {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}

pub fn correr(kelvin: f64) -> std::process::ExitCode {
    match correr_(kelvin) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("morgan nightlight: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn correr_(kelvin: f64) -> Result<(), String> {
    unsafe {
        libc::signal(libc::SIGUSR1, al_apagar as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, al_apagar as *const () as libc::sighandler_t);
    }
    let con = Connection::connect_to_env().map_err(|e| e.to_string())?;
    let mut cola = con.new_event_queue();
    let qh = cola.handle();
    let mut e = Estado::default();
    let _registro = con.display().get_registry(&qh, ());
    // La primera vuelta trae los globales; la segunda, el tamaño de cada gama.
    cola.roundtrip(&mut e).map_err(|x| x.to_string())?;
    if e.manager.is_none() {
        return Err("el compositor no tiene wlr-gamma-control".into());
    }
    cola.roundtrip(&mut e).map_err(|x| x.to_string())?;
    fundir(&con, &mut cola, &mut e, NEUTRO, kelvin)?;
    // Atento a las salidas que se conectan, hasta que pidan apagar.
    while !APAGAR.load(Ordering::Relaxed) {
        let antes: usize = e.controles.iter().filter(|s| s.tam.is_some()).count();
        if let Some(guardia) = cola.prepare_read() {
            let fd = con.as_fd();
            let mut pfd = libc::pollfd {
                fd: std::os::fd::AsRawFd::as_raw_fd(&fd),
                events: libc::POLLIN,
                revents: 0,
            };
            let r = unsafe { libc::poll(&mut pfd, 1, 200) };
            if r > 0 {
                guardia.read().map_err(|x| x.to_string())?;
            }
        }
        cola.dispatch_pending(&mut e).map_err(|x| x.to_string())?;
        if e.controles.iter().filter(|s| s.tam.is_some()).count() != antes {
            poner(&e, kelvin);
            con.flush().map_err(|x| x.to_string())?;
        }
    }
    fundir(&con, &mut cola, &mut e, kelvin, NEUTRO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutro_a_6500_y_calido_abajo() {
        let n = color(6500.0);
        assert!(n.iter().all(|c| (c - 1.0).abs() < 1e-9));
        let c = color(4000.0);
        assert!((c[0] - 1.0).abs() < 1e-9, "el rojo queda");
        assert!(c[2] < c[1] && c[1] < 1.0, "baja más el azul que el verde");
    }

    #[test]
    fn la_tabla_es_rojo_verde_azul() {
        let t = tabla(4, 6500.0);
        assert_eq!(t.len(), 4 * 3 * 2);
        let v: Vec<u16> = t
            .chunks(2)
            .map(|b| u16::from_ne_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(&v[..4], &[0, 21845, 43690, 65535]);
        assert_eq!(&v[4..8], &v[..4]);
        let calida: Vec<u16> = tabla(2, 3000.0)
            .chunks(2)
            .map(|b| u16::from_ne_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(calida[1], 65535);
        assert!(calida[5] < calida[3]);
    }
}
