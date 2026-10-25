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
        fallo: 0,
    }
}

fn capturar(ui: &mut Ui, nombre: &str) {
    capturar_en(ui, nombre, &[Duration::from_secs(2)]);
}

/// Arma en `t0` y después en cada instante de `despues`; dibuja el último.
fn capturar_en(ui: &mut Ui, nombre: &str, despues: &[Duration]) {
    let Some(dir) = std::env::var_os("MORGAN_CENTRO_CAPTURA") else {
        panic!("MORGAN_CENTRO_CAPTURA");
    };
    let (w, h) = (2560, 1440);
    ui.tam = (w, h);
    let t0 = Instant::now();
    ui.armar(1.0, t0);
    for d in despues {
        ui.armar(1.0, t0 + *d);
    }
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

/// La lista de Red con el campo de «Vecino»: `fallo` y `ocupado` como lleguen.
fn red_con_campo(fallo: u64, ocupado: bool, aviso: (&str, bool)) -> Ui {
    let mut ui = nuevo();
    let mut abierta = Opcion::new("󰤢", "Vecino", None, 10);
    abierta.abierta = true;
    let mut c = campo(
        Caja {
            glifo: "\u{F033E}",
            placeholder: "Contraseña".into(),
            clave: true,
            fijo: None,
            boton: Some(BotonCampo::Ojo),
            ocupado,
            enviar: 11,
            escape: 12,
        },
        aviso,
    );
    c.fallo = fallo;
    abierta.campo = Some(c);
    ui.datos.listas.insert(
        ListaId::Red,
        Contenido::Opciones(vec![Opcion::new("󰤨", "Wi-Fi", Some(true), 7), abierta]),
    );
    ui.desplegar(Some(ListaId::Red), Instant::now());
    if let Some(r) = ui.revelados.get_mut(&ListaId::Red) {
        r.poner(true);
    }
    ui
}

#[test]
#[ignore]
fn captura_del_fallo() {
    // Cuadros del fallo: el campo llega con un fallo nuevo en t0.
    for (i, ms) in [0u64, 60, 140, 300, 500, 700, 1200].into_iter().enumerate() {
        let mut ui = red_con_campo(0, false, ("Conectando…", false));
        let t0 = Instant::now();
        ui.armar(1.0, t0);
        if let Some(Contenido::Opciones(v)) = ui.datos.listas.get_mut(&ListaId::Red)
            && let Some(c) = v[1].campo.as_mut()
        {
            c.fallo = 5;
            c.aviso = Some(("Contraseña incorrecta".into(), true));
        }
        ui.tam = (2560, 1440);
        ui.armar(1.0, t0 + Duration::from_millis(1));
        capturar_en(
            &mut ui,
            &format!("fallo-{i}-{ms}.ppm"),
            &[Duration::from_millis(ms + 1)],
        );
    }
    // El latido, en lo más bajo (medio segundo).
    let mut ui = red_con_campo(0, true, ("Conectando…", false));
    capturar_en(
        &mut ui,
        "latido.ppm",
        &[Duration::ZERO, Duration::from_millis(500)],
    );
}

/// El launcher con lo escrito «fi», lo resaltado y el genérico, y el destello de la elegida.
#[test]
#[ignore]
fn captura_del_launcher() {
    use super::launcher::LauncherUi;
    use crate::centro::vista::{FilaLauncher, IconoFila, ListaLauncher, TipoFila};
    use smithay_client_toolkit::seat::keyboard::Keysym;

    let Some(dir) = std::env::var_os("MORGAN_CENTRO_CAPTURA") else {
        panic!("MORGAN_CENTRO_CAPTURA");
    };
    let css = std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".config/morgan/theme.css"))
        .and_then(|d| std::fs::read_to_string(d).ok())
        .unwrap_or_default();
    let cuadrado = crate::pixeles::rgba([200u8, 120, 60, 255].repeat(24 * 24), 24, 24);
    let fila = |nombre: &str, generico: Option<&str>, resaltes: Vec<(usize, usize)>| FilaLauncher {
        icono: Some(IconoFila::Imagen(cuadrado.clone())),
        nombre: nombre.into(),
        generico: generico.map(Into::into),
        resaltes,
        resaltes_generico: Vec::new(),
        tipo: TipoFila::Cierra,
        carpeta: false,
    };
    let mut filas = vec![
        fila("Archivos", None, vec![]),
        fila("Configure lsfg-vk", None, vec![(4, 6)]),
        fila(
            "Editor de imágenes Pinta",
            Some("Editor de imágenes"),
            vec![],
        ),
        fila("MAKO Renderer Configuration", None, vec![(18, 20)]),
        fila("Obsidian", None, vec![]),
        fila("ONLYOFFICE", Some("Document Editor"), vec![(6, 8)]),
        fila("Firefox", Some("Navegador web"), vec![(0, 2)]),
        fila(
            "Una app con un nombre larguísimo que no entra en la fila del launcher",
            Some("y un genérico"),
            vec![],
        ),
    ];
    filas[1].icono = None;
    for (nombre, destello) in [("launcher.ppm", false), ("launcher-destello.ppm", true)] {
        let mut ui = LauncherUi::new(Paleta::desde_css(&css), 0.85, Borde::Sombra);
        ui.abrir("Apps".into(), false);
        ui.poner_lista(ListaLauncher {
            texto: (1, 0),
            version: 1,
            filas: Vec::new(),
            elegida: 0,
            en_carpeta: false,
            vaciar: Some("Buscar…".into()),
        });
        ui.tecla(Keysym::f, Some("f"), false);
        ui.tecla(Keysym::i, Some("i"), false);
        ui.poner_lista(ListaLauncher {
            texto: (1, 2),
            version: 2,
            filas: filas.clone(),
            elegida: 0,
            en_carpeta: false,
            vaciar: None,
        });
        ui.tecla(Keysym::Down, None, false);
        if destello {
            ui.tecla(Keysym::Return, None, false);
        }
        let (w, h) = (2560u32, 1440u32);
        ui.tam = (w, h);
        let t0 = Instant::now();
        ui.armar(1.0, t0 + Duration::from_secs(2));
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
        let (cx, cy, cw, ch) = (930usize, 510usize, 700usize, 420usize);
        let mut ppm = format!("P6\n{cw} {ch}\n255\n").into_bytes();
        for y in cy..cy + ch {
            for x in cx..cx + cw {
                let i = (y * w as usize + x) * 4;
                ppm.extend_from_slice(&px[i..i + 3]);
            }
        }
        std::fs::write(std::path::Path::new(&dir).join(nombre), ppm).expect("escribir");
    }
}

/// El pedido de clave de polkit: escribiendo, verificando y después de un fallo (con el
/// aviso ya a la vista).
#[test]
#[ignore]
fn captura_del_permiso() {
    use super::permiso::PermisoUi;
    use crate::centro::vista::VistaPermiso;
    use smithay_client_toolkit::seat::keyboard::Keysym;

    let Some(dir) = std::env::var_os("MORGAN_CENTRO_CAPTURA") else {
        panic!("MORGAN_CENTRO_CAPTURA");
    };
    let css = std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".config/morgan/theme.css"))
        .and_then(|d| std::fs::read_to_string(d).ok())
        .unwrap_or_default();
    let base = VistaPermiso {
        usuario: "roberto".into(),
        mensaje: "Se requiere autenticación para ejecutar «/usr/bin/btrfs-assistant-bin» como el superusuario".into(),
        detalle: Some("/usr/bin/btrfs-assistant-bin".into()),
        placeholder: "Contraseña".into(),
        generacion: 1,
        ..VistaPermiso::default()
    };
    let casos = [
        ("permiso.ppm", base.clone(), 3, 2.0),
        (
            "permiso-verificando.ppm",
            VistaPermiso {
                ocupado: true,
                ..base.clone()
            },
            0,
            0.5,
        ),
        (
            "permiso-fallo.ppm",
            VistaPermiso {
                generacion: 2,
                fallos: 1,
                aviso: Some(("Contraseña incorrecta".into(), true)),
                ..base.clone()
            },
            0,
            1.5,
        ),
        ("permiso-vacio.ppm", base.clone(), 0, 0.0),
    ];
    for (nombre, vista, letras, despues) in casos {
        let mut ui = PermisoUi::new(Paleta::desde_css(&css), 0.85, Borde::Sombra);
        let t0 = Instant::now();
        ui.poner(vista);
        for _ in 0..letras {
            ui.tecla(Keysym::a, Some("a"), false);
        }
        let (w, h) = (2560u32, 1440u32);
        ui.tam = (w, h);
        ui.armar(1.0, t0 + Duration::from_secs_f64(despues));
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
        let (cx, cy, cw, ch) = (930usize, 580usize, 700usize, 280usize);
        let mut ppm = format!("P6\n{cw} {ch}\n255\n").into_bytes();
        for y in cy..cy + ch {
            for x in cx..cx + cw {
                let i = (y * w as usize + x) * 4;
                ppm.extend_from_slice(&px[i..i + 3]);
            }
        }
        std::fs::write(std::path::Path::new(&dir).join(nombre), ppm).expect("escribir");
    }
}
