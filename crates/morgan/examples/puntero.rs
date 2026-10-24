//! Un puntero de mentira para probar morgan en el Hyprland anidado (`dev/anidado`), que solo
//! tenía teclado (`wtype`). Usa `zwlr_virtual_pointer_v1` contra el `WAYLAND_DISPLAY` que le
//! toque: corrido con `dev/anidado ejecutar`, mueve el puntero del anidado y nada más.
//!
//!   puntero mover X Y
//!   puntero click X Y [derecho|medio]
//!   puntero arrastrar X0 Y0 X1 Y1 [MS]
//!   puntero rueda X Y PASOS
//!
//! Las coordenadas son de la salida (la única del anidado); el tamaño sale de `wl_output`.

use std::time::{Duration, Instant};

use wayland_client::protocol::{wl_output, wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

#[derive(Default)]
struct Estado {
    manager: Option<ZwlrVirtualPointerManagerV1>,
    seat: Option<wl_seat::WlSeat>,
    salida: Option<wl_output::WlOutput>,
    tam: (u32, u32),
}

impl Dispatch<wl_registry::WlRegistry, ()> for Estado {
    fn event(
        e: &mut Self,
        r: &wl_registry::WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name, interface, ..
        } = ev
        {
            match interface.as_str() {
                "zwlr_virtual_pointer_manager_v1" => {
                    e.manager = Some(r.bind(name, 1, qh, ()));
                }
                "wl_seat" if e.seat.is_none() => e.seat = Some(r.bind(name, 1, qh, ())),
                "wl_output" if e.salida.is_none() => e.salida = Some(r.bind(name, 2, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for Estado {
    fn event(
        e: &mut Self,
        _: &wl_output::WlOutput,
        ev: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Mode { width, height, .. } = ev {
            e.tam = (width as u32, height as u32);
        }
    }
}

macro_rules! sin_eventos {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for Estado {
            fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
sin_eventos!(wl_seat::WlSeat, ZwlrVirtualPointerManagerV1, ZwlrVirtualPointerV1);

const IZQUIERDO: u32 = 0x110;
const DERECHO: u32 = 0x111;
const MEDIO: u32 = 0x112;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let num = |i: usize| -> f64 {
        args.get(i)
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("falta el número {i}"))
    };
    let conn = Connection::connect_to_env().expect("sin Wayland");
    let mut cola = conn.new_event_queue();
    let qh = cola.handle();
    let _registro = conn.display().get_registry(&qh, ());
    let mut e = Estado::default();
    cola.roundtrip(&mut e).expect("roundtrip");
    cola.roundtrip(&mut e).expect("roundtrip");
    let manager = e.manager.clone().expect("el compositor no tiene virtual pointer");
    let p = manager.create_virtual_pointer(e.seat.as_ref(), &qh, ());
    let (w, h) = e.tam;
    let t0 = Instant::now();
    let ms = || t0.elapsed().as_millis() as u32;
    let mover = |x: f64, y: f64| {
        p.motion_absolute(ms(), x.round() as u32, y.round() as u32, w, h);
        p.frame();
    };
    let boton = |b: u32, apretado: bool| {
        let estado = if apretado {
            wl_pointer::ButtonState::Pressed
        } else {
            wl_pointer::ButtonState::Released
        };
        p.button(ms(), b, estado);
        p.frame();
    };
    let esperar = |d: u64| {
        let _ = conn.flush();
        std::thread::sleep(Duration::from_millis(d));
    };
    match args.first().map(String::as_str) {
        Some("mover") => mover(num(1), num(2)),
        Some("click") => {
            let b = match args.get(3).map(String::as_str) {
                Some("derecho") => DERECHO,
                Some("medio") => MEDIO,
                _ => IZQUIERDO,
            };
            mover(num(1), num(2));
            esperar(30);
            boton(b, true);
            esperar(40);
            boton(b, false);
        }
        Some("arrastrar") => {
            let (x0, y0, x1, y1) = (num(1), num(2), num(3), num(4));
            let dura: f64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(300.0);
            mover(x0, y0);
            esperar(30);
            boton(IZQUIERDO, true);
            let pasos = (dura / 8.0).max(2.0) as usize;
            for i in 1..=pasos {
                let t = i as f64 / pasos as f64;
                esperar(8);
                mover(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
            }
            esperar(30);
            boton(IZQUIERDO, false);
        }
        Some("rueda") => {
            mover(num(1), num(2));
            esperar(30);
            let pasos = num(3) as i32;
            for _ in 0..pasos.abs() {
                p.axis_source(wl_pointer::AxisSource::Wheel);
                p.axis_discrete(ms(), wl_pointer::Axis::VerticalScroll, 15.0 * f64::from(pasos.signum()), pasos.signum());
                p.frame();
                esperar(20);
            }
        }
        _ => {
            eprintln!("uso: puntero mover|click|arrastrar|rueda …");
            std::process::exit(2);
        }
    }
    esperar(20);
    p.destroy();
    let _ = conn.roundtrip();
}
