
#[macro_use]
extern crate morgan_idioma;

mod acerca;
mod agenda;
mod apariencia;
mod animaciones;
mod aplicacion;
mod audio;
mod bajada;
mod bandeja;
mod barra;
mod bluetooth;
mod brillo;
pub(crate) mod calculadora;
mod calendario;
mod clima;
mod centro;
mod comando;
mod config;
mod cuotas;
mod deslizable;
mod dispositivos;
mod energia;
mod fondos;
mod hypr_cfg;
mod hyprland;
mod icono;
mod ical;
mod idle;
mod lamina;
mod ipc;
mod launcher;
mod luz;
mod menu;
mod migracion;
mod mezclador;
mod mpris;
mod notificaciones;
mod osd;
mod pixeles;
mod pila;
mod privacidad;
mod red;
mod reproductor;
mod setup;
mod sistema;
mod sonidos;
mod teclas;
mod tema;
mod toasts;
mod ventanas;

use std::cell::OnceCell;
use std::process::ExitCode;
use std::rc::Rc;

use morgan_ipc::{
    AccionSuperficie, Contenido, NivelMenu, PedidoFondo, Request, Response,
};

use crate::audio::Audio;
use crate::centro::Centro;
use crate::hyprland::Hyprland;
use crate::launcher::Launcher;
use crate::menu::Menu;
use crate::notificaciones::Notificaciones;
use crate::osd::{Aviso, Cuerpo, Osd};
use crate::reproductor::Reproductor;
use crate::tema::Tema;

fn main() -> ExitCode {
    // `morgan about` corre en la terminal y sale: no arranca nada del shell.
    match std::env::args().nth(1).as_deref() {
        Some("about") => return acerca::correr(),
        Some("setup") => return setup::correr(),
        _ => {}
    }
    // Evita intentos fallidos de conexión a at-spi en canberra y GTK.
    unsafe {
        std::env::set_var("NO_AT_BRIDGE", "1");
    }
    // Una configuración inválida no arranca nada, ni siquiera toma el socket.
    let config_path = match config::path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("morgan: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(dir) = config_path.parent() {
        for r in migracion::nombres_viejos(dir) {
            eprintln!("morgan: renombrado (nombre de antes de 0.1.2): {r}");
        }
    }
    let config = match config::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("morgan: {e}");
            return ExitCode::FAILURE;
        }
    };
    // La pregunta a uwsm tarda: que se haga ya.
    std::thread::spawn(comando::con_uwsm);
    let theme_path = config_path.with_file_name("theme.css");

    // El socket se toma antes de levantar nada: si ya hay un morgan, este sale sin abrir nada.
    let path = match morgan_ipc::socket_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("morgan: {e}");
            return ExitCode::FAILURE;
        }
    };
    let listener = match morgan_ipc::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("morgan: no se pudo escuchar en {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let calls = ipc::start(listener);

    // La instancia única la garantiza el socket, no el bus de sesión.
    pixeles::leer_salidas();
    let arrancar = move || {
        animaciones::poner(&config.apariencia.velocidad);
        {
            let (c, ruta) = (config.clone(), config_path.clone());
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = hypr_cfg::aplicar(&c, &ruta).await {
                    eprintln!("morgan: hyprland: {e}");
                }
            });
        }
        let tema = Tema::new(
            theme_path.clone(),
            &config.shell,
            morgan_motor::forma::Borde::de(&config.apariencia.sombra),
        );
        let hyprland = Hyprland::new();
        // La escala y el tamaño de las salidas (`pixeles`) siguen a las que se conectan.
        {
            let h = hyprland.clone();
            hyprland.al_evento(move |linea| {
                if ["monitoradded", "monitorremoved", "configreloaded"]
                    .iter()
                    .any(|e| linea.starts_with(e))
                {
                    let h = h.clone();
                    glib::MainContext::default().spawn_local(async move {
                        if let Some(json) = h.consultar("j/monitors").await {
                            let _ = pixeles::poner_salidas(&json);
                        }
                    });
                }
            });
        }
        let notificaciones = Notificaciones::new(&config, hyprland.clone());
        let osd = Osd::new(&tema);
        {
            let (osd, hyprland) = (osd.clone(), hyprland.clone());
            dispositivos::escuchar(move |a| {
                if !hyprland.juego_en_foco() {
                    osd.mostrar(a);
                }
            });
        }
        let agenda = agenda::Agenda::new();
        let idle = idle::Idle::new(&config.idle, hyprland.clone());
        let luz = luz::Luz::new(&config.luz, osd.clone());
        let brillo = brillo::Brillo::new();
        let centro = Centro::new(centro::Servicios {
            notificaciones: notificaciones.clone(),
            hyprland: hyprland.clone(),
            osd: osd.clone(),
            idle: idle.clone(),
            luz: luz.clone(),
            brillo: brillo.clone(),
            tema: tema.clone(),
        });
        centro.aplicar_config(&config.sonido);
        let fondos = fondos::Fondos::new(hyprland.clone(), &config.fondos, &centro);
        let (hilo_barra, eventos_barra) = barra::lanzar(&tema, &config.barra.fijos);
        let iconos_bandeja = config.barra.iconos_bandeja.clone();
        let tele = config.barra.tele.clone();
        let calendario = calendario::Calendario::new(hilo_barra.clone(), &agenda, &config.clima);
        let calculadora = calculadora::Calculadora::new(&centro);
        let audio = Audio::new(&config.sonido);
        let sonidos = sonidos::Sonidos::new(&config.sonido);
        let servicios = Rc::new(Servicios {
            osd,
            calendario: calendario.clone(),
            calculadora,
            idle: idle.clone(),
            luz: luz.clone(),
            brillo,
            centro: centro.clone(),
            launcher: Launcher::new(&centro, &agenda),
            fondos: fondos.clone(),
            ventanas: ventanas::Ventanas::new(hyprland.clone(), &centro),
            menu: Menu::new(
                menu::Servicios {
                    config: config.clone(),
                    ruta_config: config_path.clone(),
                    tema: tema.clone(),
                    notificaciones: notificaciones.clone(),
                    idle: idle.clone(),
                    audio: audio.clone(),
                    sonidos: sonidos.clone(),
                    luz,
                    fondos: fondos.clone(),
                    calendario: calendario.clone(),
                    centro: centro.clone(),
                },
            ),
            audio,
            sonidos,
            reproductor: Reproductor::default(),
            hyprland,
            notificaciones,
            barra: OnceCell::new(),
        });
        let weak = Rc::downgrade(&servicios);
        // Los binds avisan por el socket de eventos (`ipc::de_evento`).
        let w = weak.clone();
        servicios.hyprland.al_evento(move |linea| {
            let Some(pedido) = ipc::de_evento(linea) else {
                return;
            };
            let Some(s) = w.upgrade() else { return };
            match pedido {
                // Ya mismo: el pedido y el soltar del lienzo, en el orden en que llegan.
                Ok(Request::Ventanas { soltar }) => pedir_ventanas(&s, soltar),
                Ok(r) => {
                    let linea = linea.to_owned();
                    glib::MainContext::default().spawn_local(async move {
                        if let Response::Error { message } = dispatch(&r, &s).await {
                            eprintln!("morgan: {linea}: {message}");
                        }
                    });
                }
                Err(e) => eprintln!("morgan: {linea}: {e}"),
            }
        });
        let privacidad = privacidad::Privacidad::new(&servicios.hyprland);
        let sonidos = servicios.sonidos.clone();
        let grababa = std::cell::Cell::new(false);
        privacidad.al_cambiar(move |e| {
            if grababa.replace(e.grabando) && !e.grabando {
                sonidos.tocar(sonidos::Evento::GrabacionGuardada);
            }
        });
        // Lo primero que se lee es el estado de arranque, no un cambio.
        let osd = servicios.osd.clone();
        let salida = tele.clone();
        let estaba = std::cell::Cell::new(None);
        servicios.hyprland.al_cambiar_espacios(move |e| {
            let prendida = e.prendida(&salida);
            if estaba.replace(Some(prendida)).is_some_and(|antes| antes != prendida) {
                let texto = if prendida { "Encendida" } else { "Apagada" };
                osd.mostrar(&Aviso::estado("󰔂", "TV", texto, prendida, 2000));
            }
        });
        let barra = barra::Barra::new(
            hilo_barra,
            eventos_barra,
            barra::Servicios {
                hyprland: servicios.hyprland.clone(),
                notificaciones: servicios.notificaciones.clone(),
                tema: tema.clone(),
                calendario: servicios.calendario.clone(),
                centro: servicios.centro.clone(),
                bandeja: bandeja::Bandeja::new(),
                privacidad,
                iconos_bandeja,
                tele,
                pedir: Rc::new(move |request| {
                    let Some(s) = weak.upgrade() else { return };
                    glib::MainContext::default().spawn_local(async move {
                        if let Response::Error { message } = dispatch(&request, &s).await {
                            eprintln!("morgan: barra: {message}");
                        }
                    });
                }),
            },
        );
        let b = barra.clone();
        servicios
            .centro
            .al_abrir(move |abierto| b.marcar_centro(abierto));
        let _ = servicios.barra.set(barra);
        let f = servicios.fondos.clone();
        servicios.menu.al_fondos(move || f.abrir(true));
        let l = servicios.launcher.clone();
        servicios.menu.al_launcher(move |modo| l.abrir(modo));
        let m = servicios.menu.clone();
        servicios.launcher.al_menu(move |p| match p {
            launcher::ParaElMenu::Ciudad(u) => m.poner_ubicacion(u),
            launcher::ParaElMenu::Calendario => m.abrir(NivelMenu::Clima),
        });
        servicios.menu.precalentar();
        servicios.launcher.precalentar();
        let m = servicios.menu.clone();
        servicios
            .fondos
            .al_volver(move || m.abrir(NivelMenu::Fondos));
        let calls = calls.clone();
        glib::MainContext::default().spawn_local(async move {
            let _tema = tema;
            while let Ok(call) = calls.recv().await {
                // Cada pedido en su tarea: un reproductor que tarda en contestar no traba el volumen. El
                // orden entre pedidos de volumen lo cuida la fila de `Audio`.
                let servicios = servicios.clone();
                glib::MainContext::default().spawn_local(async move {
                    let response = dispatch(&call.request, &servicios).await;
                    call.reply(response);
                });
            }
        });
    };
    arrancar();
    // Sin ventanas visibles morgan sigue vivo: es un servicio, no una app.
    glib::MainLoop::new(None, false).run();
    let _ = std::fs::remove_file(&path);
    ExitCode::SUCCESS
}

struct Servicios {
    osd: Osd,
    idle: idle::Idle,
    luz: luz::Luz,
    brillo: brillo::Brillo,
    centro: Centro,
    calendario: calendario::Calendario,
    calculadora: calculadora::Calculadora,
    launcher: Launcher,
    menu: Menu,
    audio: Audio,
    sonidos: sonidos::Sonidos,
    reproductor: Reproductor,
    hyprland: Hyprland,
    notificaciones: Notificaciones,
    /// Pide por `dispatch`, que necesita a `Servicios`.
    barra: OnceCell<barra::Barra>,
    fondos: fondos::Fondos,
    ventanas: ventanas::Ventanas,
}

impl Servicios {
    fn cerrar_reloj(&self) {
        self.calendario.cerrar();
        self.calculadora.cerrar();
        self.fondos.cerrar();
        self.ventanas.cerrar();
    }
}

async fn dispatch(request: &Request, s: &Servicios) -> Response {
    if let Err(message) = request.validar() {
        return Response::Error { message };
    }
    let aviso = match request {
        Request::Ping => {
            return Response::Pong {
                version: env!("CARGO_PKG_VERSION").into(),
            };
        }
        Request::Osd {
            icono,
            contenido,
            estado,
            encendido,
            titulo,
            ms,
        } => {
            let (titulo, cuerpo) = match contenido {
                Contenido::Mensaje(t) => (
                    t.clone(),
                    Cuerpo::Estado {
                        texto: estado.clone().unwrap_or_default(),
                        encendido: *encendido,
                    },
                ),
                Contenido::Progreso(f) => (
                    titulo.clone().unwrap_or_default(),
                    Cuerpo::Barra {
                        fraccion: *f,
                        porcentaje: f * 100.0,
                        apagada: false,
                    },
                ),
            };
            Aviso {
                icono: icono.clone(),
                titulo,
                cuerpo,
                ms: ms.unwrap_or(morgan_ipc::OSD_MS),
            }
        }
        Request::Volumen {
            accion,
            microfono,
            dispositivo,
        } => match s
            .audio
            .cambiar(*accion, *microfono, dispositivo.clone())
            .await
        {
            Ok(nivel) => {
                if !*microfono && *accion != morgan_ipc::AccionVolumen::Silenciar {
                    s.sonidos.tocar(sonidos::Evento::Volumen);
                }
                audio::aviso(*microfono, nivel, morgan_ipc::OSD_MS)
            }
            Err(message) => return Response::Error { message },
        },
        Request::Reproductor { accion } => {
            match s.reproductor.pedir(*accion, morgan_ipc::OSD_MS).await {
                Ok(aviso) => aviso,
                Err(message) => return Response::Error { message },
            }
        }
        Request::NoMolestar { accion } => {
            let activo = accion.aplicar(s.notificaciones.no_molestar());
            s.notificaciones.set_no_molestar(activo);
            aviso_no_molestar(s.notificaciones.no_molestar())
        }
        Request::Brillo { pedido } => {
            let r = match pedido {
                morgan_ipc::PedidoBrillo::Subir => s.brillo.mover(true),
                morgan_ipc::PedidoBrillo::Bajar => s.brillo.mover(false),
                morgan_ipc::PedidoBrillo::Poner(v) => s.brillo.poner(f64::from(*v)),
            };
            match r {
                Ok(v) => brillo::aviso(v),
                Err(message) => return Response::Error { message },
            }
        }
        Request::Caffeine { accion } => {
            if !s.idle.activo() {
                return Response::Error {
                    message: "el idle no es de morgan (org.freedesktop.ScreenSaver es de otro)"
                        .into(),
                };
            }
            let Some(accion) = accion else {
                let estado = if s.idle.caffeine() { "on" } else { "off" };
                return Response::Texto {
                    texto: format!("{estado}\n"),
                };
            };
            s.idle.set_caffeine(accion.aplicar(s.idle.caffeine()));
            idle::aviso_caffeine(s.idle.caffeine())
        }
        Request::Luz { accion } => {
            let Some(accion) = accion else {
                let estado = if s.luz.prendida() { "on" } else { "off" };
                return Response::Texto {
                    texto: format!("{estado}\n"),
                };
            };
            // `poner` ya da su aviso por el OSD.
            if let Err(message) = s.luz.poner(accion.aplicar(s.luz.prendida())).await {
                return Response::Error { message };
            }
            return Response::Done;
        }
        Request::BloqMayus => {
            let Some(devices) = s.hyprland.consultar("j/devices").await else {
                return Response::Error {
                    message: "Hyprland no contestó j/devices".into(),
                };
            };
            match teclas::bloq_mayus(&devices) {
                Some(prendido) => teclas::aviso(prendido),
                None => {
                    return Response::Error {
                        message: "j/devices sin teclados".into(),
                    };
                }
            }
        }
        Request::Limpiar => {
            s.centro.limpiar_notificaciones();
            return Response::Done;
        }
        Request::Fondo { pedido } => {
            let r = match pedido {
                PedidoFondo::Actual => {
                    return Response::Texto {
                        texto: fondos::actual()
                            .map(|p| format!("{}\n", p.display()))
                            .unwrap_or_default(),
                    };
                }
                PedidoFondo::Rotar => s.fondos.rotar().await,
                PedidoFondo::Poner(ruta) => s.fondos.aplicar(ruta.into()),
            };
            return match r {
                Ok(()) => Response::Done,
                Err(message) => Response::Error { message },
            };
        }
        Request::Captura { ruta } => {
            return match s.notificaciones.captura(std::path::Path::new(&ruta)) {
                Ok(()) => Response::Done,
                Err(message) => Response::Error { message },
            };
        }
        // Centro, launcher y menú toman el teclado: abrir uno cierra los otros.
        Request::Centro { accion } => {
            match accion {
                AccionSuperficie::Alternar => s.centro.alternar(),
                AccionSuperficie::Abrir => s.centro.abrir(),
                AccionSuperficie::Cerrar => s.centro.cerrar(),
            }
            if s.centro.abierto() {
                s.launcher.cerrar();
                s.menu.cerrar();
                s.cerrar_reloj();
            }
            return Response::Done;
        }
        Request::Reloj { accion } => {
            match accion {
                AccionSuperficie::Alternar => s.calendario.alternar(),
                AccionSuperficie::Abrir => s.calendario.abrir(),
                AccionSuperficie::Cerrar => s.calendario.cerrar(),
            }
            if s.calendario.abierto() {
                s.centro.cerrar();
                s.launcher.cerrar();
                s.menu.cerrar();
                s.calculadora.cerrar();
                s.fondos.cerrar();
                s.ventanas.cerrar();
            }
            return Response::Done;
        }
        Request::Calculadora { accion } => {
            match accion {
                AccionSuperficie::Alternar => s.calculadora.alternar(),
                AccionSuperficie::Abrir => s.calculadora.abrir(),
                AccionSuperficie::Cerrar => s.calculadora.cerrar(),
            }
            if s.calculadora.abierto() {
                s.centro.cerrar();
                s.launcher.cerrar();
                s.menu.cerrar();
                s.calendario.cerrar();
                s.fondos.cerrar();
                s.ventanas.cerrar();
            }
            return Response::Done;
        }
        Request::Fondos { accion } => {
            match accion {
                AccionSuperficie::Alternar => s.fondos.alternar(),
                AccionSuperficie::Abrir => s.fondos.abrir(false),
                AccionSuperficie::Cerrar => s.fondos.cerrar(),
            }
            if !matches!(accion, AccionSuperficie::Cerrar) {
                s.centro.cerrar();
                s.launcher.cerrar();
                s.menu.cerrar();
                s.calendario.cerrar();
                s.calculadora.cerrar();
                s.ventanas.cerrar();
            }
            return Response::Done;
        }
        Request::Ventanas { soltar } => {
            pedir_ventanas(s, *soltar);
            return Response::Done;
        }
        // Dormir acá traba el bucle entero, que es lo que se quiere probar.
        Request::Trabar { ms } => {
            if std::env::var_os("MORGAN_DEV").is_none() {
                return Response::Error {
                    message: "solo en desarrollo (MORGAN_DEV)".into(),
                };
            }
            let desde = lamina::ahora_us();
            std::thread::sleep(std::time::Duration::from_millis(u64::from(*ms)));
            eprintln!("morgan: traba {desde} {}", lamina::ahora_us());
            return Response::Done;
        }
        Request::Sesion => {
            if s.centro.abierto() {
                s.centro.cerrar();
            } else {
                s.centro.abrir_sesion();
                s.launcher.cerrar();
                s.menu.cerrar();
                s.cerrar_reloj();
            }
            return Response::Done;
        }
        Request::Launcher { accion, modo } => {
            match accion {
                AccionSuperficie::Alternar => s.launcher.alternar(*modo),
                AccionSuperficie::Abrir => s.launcher.abrir(*modo),
                AccionSuperficie::Cerrar => s.launcher.cerrar(),
            }
            if s.launcher.abierto() {
                s.centro.cerrar();
                s.menu.cerrar();
                s.cerrar_reloj();
            }
            return Response::Done;
        }
        Request::Menu { accion, nivel } => {
            match accion {
                AccionSuperficie::Alternar => s.menu.alternar(*nivel),
                AccionSuperficie::Abrir => s.menu.abrir(*nivel),
                AccionSuperficie::Cerrar => s.menu.cerrar(),
            }
            if s.menu.abierto() {
                s.centro.cerrar();
                s.launcher.cerrar();
                s.cerrar_reloj();
            }
            return Response::Done;
        }
    };
    // Con el centro abierto, su slider ya muestra el nivel.
    let de_nivel = matches!(
        request,
        Request::Volumen { .. }
            | Request::Brillo { .. }
            | Request::Osd {
                contenido: Contenido::Progreso(_),
                ..
            }
    );
    if de_nivel && s.centro.abierto() {
        return Response::Done;
    }
    // Ninguna capa se abre sobre un juego: suelta el puntero enganchado y rompe el direct
    // scanout.
    if !s.hyprland.juego_en_foco() {
        s.osd.mostrar(&aviso);
    }
    Response::Done
}

/// Lo pide `morganctl windows [release]` o el bind, por el socket de eventos.
fn pedir_ventanas(s: &Servicios, soltar: bool) {
    if soltar {
        s.ventanas.soltar();
        return;
    }
    s.ventanas.pedir();
    s.centro.cerrar();
    s.launcher.cerrar();
    s.menu.cerrar();
    s.calendario.cerrar();
    s.calculadora.cerrar();
    s.fondos.cerrar();
}

fn aviso_no_molestar(activo: bool) -> Aviso {
    let (texto, icono) = if activo {
        ("Activado", "󰂛")
    } else {
        ("Desactivado", "󰂛")
    };
    Aviso::estado(icono, "No molestar", texto, activo, 2000)
}
