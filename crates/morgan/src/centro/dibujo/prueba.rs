//! Capturas del centro sin compositor, para ver lo que en el anidado no se puede provocar sin
//! tocar la máquina de verdad (la clave de una Wi-Fi nueva, un emparejamiento). Ignoradas: se
//! corren a mano y dejan imágenes PPM en `MORGAN_CENTRO_CAPTURA` (una carpeta):
//!
//!   MORGAN_CENTRO_CAPTURA=/tmp/x cargo test -p morgan captura_sin_compositor -- --ignored

use std::time::{Duration, Instant};

use morgan_motor::color::Paleta;
use morgan_motor::forma::Borde;
use morgan_motor::gpu::{Gpu, leer};
use morgan_motor::vello::{Scene, peniko, wgpu};

use super::{Objetivo, Ui};
use crate::centro::vista::{
    BotonCampo, Caja, Campo, Contenido, ListaId, Nivel, NivelId, Opcion, Tarjeta, Toque,
};

fn tarjetas() -> Vec<Tarjeta> {
    let t = |glifo: &str, titulo: &str, estado: &str, encendida: bool, que| Tarjeta {
        glifo: glifo.into(),
        titulo: titulo.into(),
        estado: estado.into(),
        encendida,
        alerta: false,
        que,
    };
    vec![
        t("󰤨", "Red", "Casa", true, Toque::Lista(ListaId::Red)),
        t(
            "󰂯",
            "Bluetooth",
            "Encendido",
            true,
            Toque::Lista(ListaId::Bluetooth),
        ),
        t("󰂛", "No molestar", "Apagado", false, Toque::Accion(1)),
        t("󰅶", "Caffeine", "Apagado", false, Toque::Accion(2)),
        t("󰖔", "Luz nocturna", "Apagada", false, Toque::Accion(3)),
        t(
            "󰾅",
            "Energía",
            "Equilibrado",
            false,
            Toque::Lista(ListaId::Energia),
        ),
        t(
            "󰊚",
            "Cuotas de IA",
            "Claude: 65%",
            false,
            Toque::Lista(ListaId::Cuotas),
        ),
        t(
            "󰐥",
            "Sesión",
            "3 h encendida",
            false,
            Toque::Lista(ListaId::Sesion),
        ),
    ]
}

fn niveles() -> Vec<Nivel> {
    vec![Nivel {
        id: NivelId::Salida,
        glifo: "󰋋".into(),
        nombre: "Auriculares".into(),
        valor: 64.0,
        mudo: false,
        glifo_accion: Some(4),
        lista: Some(ListaId::Salida),
        mover: 5,
    }]
}

fn campo(caja: Caja, aviso: (&str, bool)) -> Campo {
    Campo {
        generacion: 1,
        caja: Some(caja),
        aviso: Some((aviso.0.into(), aviso.1)),
    }
}

fn capturar(ui: &mut Ui, nombre: &str) {
    let Some(dir) = std::env::var_os("MORGAN_CENTRO_CAPTURA") else {
        panic!("MORGAN_CENTRO_CAPTURA");
    };
    let (w, h) = (2560, 1440);
    ui.tam = (w, h);
    let t0 = Instant::now();
    ui.armar(1.0, t0);
    // Que terminen las transiciones.
    let t = t0 + Duration::from_secs(2);
    ui.armar(1.0, t);
    let mut s = Scene::new();
    ui.pintar(&mut s, 1.0, 0.0);
    let mut gpu = Gpu::sin_display("morgan");
    let tex = gpu.textura(w, h, wgpu::TextureUsages::COPY_SRC);
    assert!(gpu.dibujar(
        &s,
        &tex.create_view(&Default::default()),
        (w, h),
        peniko::Color::from_rgb8(30, 34, 38),
    ));
    let px = leer(&gpu.device, &gpu.cola, &tex, w, h).expect("leer");
    let x0 = (w - 440) as usize;
    let mut ppm = b"P6\n440 600\n255\n".to_vec();
    for y in 0..600usize {
        for x in x0..w as usize {
            let i = (y * w as usize + x) * 4;
            ppm.extend_from_slice(&px[i..i + 3]);
        }
    }
    std::fs::write(std::path::Path::new(&dir).join(nombre), ppm).expect("escribir");
}

fn nuevo() -> Ui {
    let css = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .and_then(|d| std::fs::read_to_string(d.join("morgan/theme.css")).ok())
        .unwrap_or_default();
    let mut ui = Ui::new(Paleta::desde_css(&css), 0.85, Borde::Sombra);
    ui.datos.tarjetas = tarjetas();
    ui.datos.niveles = niveles();
    ui
}

#[test]
#[ignore]
fn captura_sin_compositor() {
    // La clave de una Wi-Fi nueva, con lo escrito tapado y una clave mala.
    let mut ui = nuevo();
    let mut abierta = Opcion::new("󰤢", "Vecino", None, 10);
    abierta.abierta = true;
    abierta.campo = Some(campo(
        Caja {
            glifo: "\u{F033E}",
            placeholder: "Contraseña".into(),
            clave: true,
            fijo: None,
            boton: Some(BotonCampo::Ojo),
            ocupado: false,
            enviar: 11,
            escape: 12,
        },
        ("Contraseña incorrecta", true),
    ));
    ui.datos.listas.insert(
        ListaId::Red,
        Contenido::Opciones(vec![
            Opcion::new("󰈀", "Conexión cableada 1", Some(false), 6),
            Opcion::new("󰤨", "Wi-Fi", Some(true), 7),
            Opcion::new("󰤨", "Casa", Some(true), 8),
            abierta,
            Opcion::new("󰤟", "Otra red", None, 9),
        ]),
    );
    ui.desplegar(Some(ListaId::Red), Instant::now());
    ui.enfocar_campo(ListaId::Red, 1);
    if let Some(e) = ui.campos.get_mut(&ListaId::Red) {
        e.insertar("secreto1");
    }
    capturar(&mut ui, "clave.ppm");

    // Un código para confirmar en un emparejamiento, con el foco en el tilde.
    let mut ui = nuevo();
    let mut cerca = Opcion::new("󰋋", "Auriculares de alguien", None, 20);
    cerca.abierta = true;
    cerca.campo = Some(campo(
        Caja {
            glifo: "\u{F0306}",
            placeholder: String::new(),
            clave: false,
            fijo: Some("482915".into()),
            boton: Some(BotonCampo::Tilde(21)),
            ocupado: false,
            enviar: 22,
            escape: 23,
        },
        ("¿Es el mismo código que muestra el dispositivo?", false),
    ));
    ui.datos.listas.insert(
        ListaId::Bluetooth,
        Contenido::Opciones(vec![
            Opcion::new("󰂯", "Bluetooth", Some(true), 13),
            Opcion::new("󰋋", "Mis auriculares", Some(true), 14),
            cerca,
        ]),
    );
    ui.desplegar(Some(ListaId::Bluetooth), Instant::now());
    ui.enfocar_campo(ListaId::Bluetooth, 1);
    ui.foco_visible = true;
    assert_eq!(ui.foco, Some(Objetivo::BotonCampo(ListaId::Bluetooth)));
    capturar(&mut ui, "codigo.ppm");
}
