//! Lista los globales Wayland del `WAYLAND_DISPLAY` que le toque (como `wayland-info`, que no
//! está instalado): sirve para ver qué protocolos da cada compositor.

use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle};

struct Estado(Vec<(String, u32)>);

impl Dispatch<wl_registry::WlRegistry, ()> for Estado {
    fn event(
        estado: &mut Self,
        _: &wl_registry::WlRegistry,
        evento: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            interface, version, ..
        } = evento
        {
            estado.0.push((interface, version));
        }
    }
}

fn main() {
    let con = Connection::connect_to_env().expect("sin compositor");
    let mut cola = con.new_event_queue();
    let _registro = con.display().get_registry(&cola.handle(), ());
    let mut estado = Estado(Vec::new());
    cola.roundtrip(&mut estado).expect("roundtrip");
    estado.0.sort();
    estado.0.dedup();
    for (i, v) in estado.0 {
        println!("{i} {v}");
    }
}
