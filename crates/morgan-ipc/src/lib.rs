//! Sin GTK, para que `morganctl` arranque rápido desde los binds.

pub mod bloqueo;
pub mod hyprland;

use std::ffi::OsString;
use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Tope de una línea del protocolo. Una petición real ocupa decenas de bytes.
const MAX_LINE: u64 = 64 * 1024;

pub const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    Ping,
    Osd {
        #[serde(default)]
        icono: Option<String>,
        contenido: Contenido,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        estado: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        encendido: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        titulo: Option<String>,
        #[serde(default)]
        ms: Option<u32>,
    },
    Volumen {
        accion: AccionVolumen,
        #[serde(default)]
        microfono: bool,
        #[serde(default)]
        dispositivo: Option<String>,
    },
    Reproductor {
        accion: AccionReproductor,
    },
    NoMolestar {
        accion: AccionInterruptor,
    },
    Brillo {
        pedido: PedidoBrillo,
    },
    Caffeine {
        #[serde(default)]
        accion: Option<AccionInterruptor>,
    },
    Luz {
        #[serde(default)]
        accion: Option<AccionInterruptor>,
    },
    BloqMayus,
    Limpiar,
    Centro {
        accion: AccionSuperficie,
    },
    Reloj {
        accion: AccionSuperficie,
    },
    Calculadora {
        accion: AccionSuperficie,
    },
    Fondos {
        accion: AccionSuperficie,
    },
    Sesion,
    Menu {
        accion: AccionSuperficie,
        #[serde(default)]
        nivel: NivelMenu,
    },
    /// Alternar con el launcher abierto en otro modo cambia de modo en vez de cerrar.
    Launcher {
        accion: AccionSuperficie,
        #[serde(default)]
        modo: ModoLauncher,
    },
    Ventanas {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        soltar: bool,
    },
    /// Desarrollo: traba el hilo principal `ms` milisegundos, para probar que lo que se dibuja
    /// no lo espera (`dev/traba`). morgan lo rechaza si no corre con `MORGAN_DEV`.
    Trabar {
        ms: u32,
    },
    Captura {
        ruta: String,
    },
    Fondo {
        pedido: PedidoFondo,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PedidoFondo {
    Actual,
    Rotar,
    Poner(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum Contenido {
    Mensaje(String),
    Progreso(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccionVolumen {
    Subir,
    Bajar,
    Silenciar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PedidoBrillo {
    Subir,
    Bajar,
    Poner(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccionReproductor {
    PlayPausa,
    Siguiente,
    Anterior,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccionInterruptor {
    Alternar,
    Activar,
    Desactivar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccionSuperficie {
    Alternar,
    Abrir,
    Cerrar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModoLauncher {
    #[default]
    Apps,
    Simbolos,
    Portapapeles,
    Atajos,
    Ciudad,
    Calendario,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NivelMenu {
    #[default]
    Raiz,
    Fondos,
    Clima,
}

impl ModoLauncher {
    pub fn prompt(self) -> &'static str {
        match self {
            ModoLauncher::Apps => "Apps",
            ModoLauncher::Simbolos => "Símbolos",
            ModoLauncher::Portapapeles => "Portapapeles",
            ModoLauncher::Atajos => "Atajos",
            ModoLauncher::Ciudad => "Ciudad",
            ModoLauncher::Calendario => "Calendario",
        }
    }
}

impl AccionInterruptor {
    pub fn aplicar(self, actual: bool) -> bool {
        match self {
            AccionInterruptor::Alternar => !actual,
            AccionInterruptor::Activar => true,
            AccionInterruptor::Desactivar => false,
        }
    }
}

pub const OSD_MS: u32 = 1000;

pub const OSD_MS_RANGO: std::ops::RangeInclusive<u32> = 200..=60_000;

/// En inglés: es la API pública.
trait Nombres: Sized + Copy + 'static {
    const TODOS: &[(&str, Self)];
    const QUE: &str;
    fn parse(s: &str) -> Result<Self, String> {
        Self::TODOS
            .iter()
            .find(|(n, _)| *n == s)
            .map(|(_, v)| *v)
            .ok_or_else(|| {
                let hay: Vec<_> = Self::TODOS.iter().map(|(n, _)| *n).collect();
                format!("unknown {} «{s}» (one of: {})", Self::QUE, hay.join(", "))
            })
    }
}

impl Nombres for AccionVolumen {
    const TODOS: &[(&str, Self)] = &[
        ("up", Self::Subir),
        ("down", Self::Bajar),
        ("mute", Self::Silenciar),
    ];
    const QUE: &str = "volume action";
}

impl Nombres for AccionInterruptor {
    const TODOS: &[(&str, Self)] = &[
        ("toggle", Self::Alternar),
        ("on", Self::Activar),
        ("off", Self::Desactivar),
    ];
    const QUE: &str = "action";
}

impl Nombres for AccionSuperficie {
    const TODOS: &[(&str, Self)] = &[
        ("toggle", Self::Alternar),
        ("open", Self::Abrir),
        ("close", Self::Cerrar),
    ];
    const QUE: &str = "surface action";
}

impl Nombres for ModoLauncher {
    const TODOS: &[(&str, Self)] = &[
        ("apps", Self::Apps),
        ("symbols", Self::Simbolos),
        ("clipboard", Self::Portapapeles),
        ("keybinds", Self::Atajos),
        ("city", Self::Ciudad),
        ("calendar", Self::Calendario),
    ];
    const QUE: &str = "launcher mode";
}

impl Nombres for NivelMenu {
    const TODOS: &[(&str, Self)] = &[
        ("root", Self::Raiz),
        ("wallpapers", Self::Fondos),
        ("weather", Self::Clima),
    ];
    const QUE: &str = "menu branch";
}

impl Nombres for AccionReproductor {
    const TODOS: &[(&str, Self)] = &[
        ("play-pausa", Self::PlayPausa),
        ("next", Self::Siguiente),
        ("previous", Self::Anterior),
    ];
    const QUE: &str = "player action";
}

/// Lo más que se deja trabar el hilo principal a propósito.
pub const TRABA_MAX: u32 = 5000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Response {
    Pong {
        version: String,
    },
    Done,
    Texto {
        texto: String,
    },
    Error {
        message: String,
    },
}

impl Request {
    pub fn from_args<S: AsRef<str>>(args: &[S]) -> Result<Request, String> {
        let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
        let request = match args.as_slice() {
            ["ping"] => Request::Ping,
            ["osd", "message", texto, resto @ ..] => {
                let o = Opciones::parse(resto, &["--icon", "--ms", "--state"], &["--on"])?;
                Request::Osd {
                    icono: o.valor("--icon"),
                    contenido: Contenido::Mensaje((*texto).into()),
                    estado: o.valor("--state"),
                    encendido: o.bandera("--on"),
                    titulo: None,
                    ms: o.numero("--ms")?,
                }
            }
            ["osd", "progress", valor, resto @ ..] => {
                let o = Opciones::parse(resto, &["--icon", "--ms", "--title"], &[])?;
                let valor = valor
                    .parse()
                    .map_err(|_| format!("progress is not a number: «{valor}»"))?;
                Request::Osd {
                    icono: o.valor("--icon"),
                    contenido: Contenido::Progreso(valor),
                    estado: None,
                    encendido: false,
                    titulo: o.valor("--title"),
                    ms: o.numero("--ms")?,
                }
            }
            ["osd", ..] => {
                return Err("usage: morganctl osd message <title> | progress <0-1>".into());
            }
            ["volume", accion, resto @ ..] => {
                let o = Opciones::parse(resto, &["--device"], &["--mic"])?;
                Request::Volumen {
                    accion: AccionVolumen::parse(accion)?,
                    microfono: o.bandera("--mic"),
                    dispositivo: o.valor("--device"),
                }
            }
            ["player", accion] => Request::Reproductor {
                accion: AccionReproductor::parse(accion)?,
            },
            ["dnd", accion] => Request::NoMolestar {
                accion: AccionInterruptor::parse(accion)?,
            },
            ["wallpaper", "current"] => Request::Fondo {
                pedido: PedidoFondo::Actual,
            },
            ["wallpaper", "random"] => Request::Fondo {
                pedido: PedidoFondo::Rotar,
            },
            ["wallpaper", ruta] => {
                if !ruta.starts_with('/') {
                    return Err(format!("the wallpaper path is not absolute: «{ruta}»"));
                }
                Request::Fondo {
                    pedido: PedidoFondo::Poner((*ruta).into()),
                }
            }
            ["nightlight", "status"] => Request::Luz { accion: None },
            ["nightlight", accion] => Request::Luz {
                accion: Some(AccionInterruptor::parse(accion)?),
            },
            ["brightness", "up"] => Request::Brillo { pedido: PedidoBrillo::Subir },
            ["brightness", "down"] => Request::Brillo { pedido: PedidoBrillo::Bajar },
            ["brightness", n] => match n.parse() {
                Ok(v @ 0..=100) => Request::Brillo { pedido: PedidoBrillo::Poner(v) },
                _ => return Err(format!("brightness: up, down or 0 to 100, not «{n}»")),
            },
            ["caffeine", "status"] => Request::Caffeine { accion: None },
            ["caffeine", accion] => Request::Caffeine {
                accion: Some(AccionInterruptor::parse(accion)?),
            },
            ["capslock"] => Request::BloqMayus,
            ["notifications", "clear"] => Request::Limpiar,
            ["screenshot", ruta] => {
                if !ruta.starts_with('/') {
                    return Err(format!("the screenshot path is not absolute: «{ruta}»"));
                }
                Request::Captura {
                    ruta: (*ruta).into(),
                }
            }
            ["center", accion] => Request::Centro {
                accion: AccionSuperficie::parse(accion)?,
            },
            ["clock", accion] => Request::Reloj {
                accion: AccionSuperficie::parse(accion)?,
            },
            ["calculator", accion] => Request::Calculadora {
                accion: AccionSuperficie::parse(accion)?,
            },
            ["wallpapers", accion] => Request::Fondos {
                accion: AccionSuperficie::parse(accion)?,
            },
            ["session"] => Request::Sesion,
            ["windows"] => Request::Ventanas { soltar: false },
            ["windows", "release"] => Request::Ventanas { soltar: true },
            ["dev", "block", ms] => Request::Trabar {
                ms: ms.parse().map_err(|_| format!("not a number of ms: {ms}"))?,
            },
            ["menu", accion] => Request::Menu {
                accion: AccionSuperficie::parse(accion)?,
                nivel: NivelMenu::Raiz,
            },
            ["menu", accion, nivel] => Request::Menu {
                accion: AccionSuperficie::parse(accion)?,
                nivel: NivelMenu::parse(nivel)?,
            },
            ["launcher", accion] => Request::Launcher {
                accion: AccionSuperficie::parse(accion)?,
                modo: ModoLauncher::Apps,
            },
            ["launcher", accion, modo] => Request::Launcher {
                accion: AccionSuperficie::parse(accion)?,
                modo: ModoLauncher::parse(modo)?,
            },
            [] => return Err("missing command".into()),
            [cmd, ..] => return Err(format!("unknown or malformed command: «{cmd}»")),
        };
        request.validar()?;
        Ok(request)
    }

    /// Lo corren los dos extremos: el socket acepta cualquier cliente, no solo `morganctl`.
    pub fn validar(&self) -> Result<(), String> {
        if let Request::Trabar { ms } = self
            && *ms > TRABA_MAX
        {
            return Err(format!("block too long: {ms} ms (at most {TRABA_MAX})"));
        }
        if let Request::Osd { contenido, ms, .. } = self {
            if let Some(ms) = ms
                && !OSD_MS_RANGO.contains(ms)
            {
                return Err(format!(
                    "duration out of range: {ms} ms (from {} to {})",
                    OSD_MS_RANGO.start(),
                    OSD_MS_RANGO.end()
                ));
            }
            if let Contenido::Progreso(v) = contenido
                && !(0.0..=1.0).contains(v)
            {
                return Err(format!("progress out of range: {v} (from 0 to 1)"));
            }
        }
        Ok(())
    }
}

struct Opciones<'a> {
    valores: Vec<(&'a str, &'a str)>,
    banderas: Vec<&'a str>,
}

impl<'a> Opciones<'a> {
    fn parse(args: &[&'a str], con_valor: &[&str], banderas: &[&str]) -> Result<Self, String> {
        let mut o = Opciones {
            valores: Vec::new(),
            banderas: Vec::new(),
        };
        let mut args = args.iter();
        while let Some(&a) = args.next() {
            let repetida = o.valores.iter().any(|(n, _)| *n == a) || o.banderas.contains(&a);
            if repetida {
                return Err(format!("repeated option: {a}"));
            }
            if con_valor.contains(&a) {
                let v = args
                    .next()
                    .ok_or_else(|| format!("missing value for {a}"))?;
                o.valores.push((a, v));
            } else if banderas.contains(&a) {
                o.banderas.push(a);
            } else {
                return Err(format!("unknown option: «{a}»"));
            }
        }
        Ok(o)
    }

    fn valor(&self, nombre: &str) -> Option<String> {
        self.valores
            .iter()
            .find(|(n, _)| *n == nombre)
            .map(|(_, v)| (*v).into())
    }

    fn numero(&self, nombre: &str) -> Result<Option<u32>, String> {
        self.valor(nombre)
            .map(|v| {
                v.parse()
                    .map_err(|_| format!("{nombre} is not an integer: «{v}»"))
            })
            .transpose()
    }

    fn bandera(&self, nombre: &str) -> bool {
        self.banderas.contains(&nombre)
    }
}

#[derive(Debug)]
pub enum PathError {
    NoRuntimeDir,
    NoWaylandDisplay,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::NoRuntimeDir => f.write_str("XDG_RUNTIME_DIR no está definido"),
            PathError::NoWaylandDisplay => f.write_str("WAYLAND_DISPLAY no está definido"),
        }
    }
}

impl std::error::Error for PathError {}

/// Un morgan por sesión gráfica: un Hyprland anidado no choca con la sesión de afuera.
pub fn socket_path() -> Result<PathBuf, PathError> {
    socket_path_from(|k| std::env::var_os(k))
}

pub fn socket_path_from(env: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf, PathError> {
    let runtime = env("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .ok_or(PathError::NoRuntimeDir)?;
    let display = env("WAYLAND_DISPLAY")
        .filter(|v| !v.is_empty())
        .ok_or(PathError::NoWaylandDisplay)?;
    // Un WAYLAND_DISPLAY absoluto (`/run/…/wayland-1`) se reduce a su nombre.
    let display = Path::new(&display)
        .file_name()
        .ok_or(PathError::NoWaylandDisplay)?;
    let mut name = OsString::from("morgan-");
    name.push(display);
    name.push(".sock");
    Ok(PathBuf::from(runtime).join(name))
}

pub fn send(path: &Path, request: &Request) -> io::Result<Response> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    write_line(&mut stream, request)?;
    read_line(&mut stream)
}

/// Escucha en `path`. Si hay un socket viejo de un morgan que murió, lo reemplaza; si hay un
/// morgan vivo atendiendo, falla con `AddrInUse`.
pub fn bind(path: &Path) -> io::Result<UnixListener> {
    match UnixListener::bind(path) {
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            if UnixStream::connect(path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("ya hay un morgan atendiendo en {}", path.display()),
                ));
            }
            std::fs::remove_file(path)?;
            UnixListener::bind(path)
        }
        other => other,
    }
}

pub fn serve(stream: &mut UnixStream, handle: impl FnOnce(Request) -> Response) -> io::Result<()> {
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let response = match read_line::<Request>(stream) {
        Ok(request) => handle(request),
        Err(e) if e.kind() == io::ErrorKind::InvalidData => Response::Error {
            message: format!("petición inválida: {e}"),
        },
        Err(e) => return Err(e),
    };
    write_line(stream, &response)
}

fn write_line<T: Serialize>(stream: &mut UnixStream, value: &T) -> io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(io::Error::other)?;
    line.push(b'\n');
    stream.write_all(&line)?;
    stream.flush()
}

fn read_line<T: for<'de> Deserialize<'de>>(stream: &mut UnixStream) -> io::Result<T> {
    let mut line = String::new();
    BufReader::new(stream.take(MAX_LINE)).read_line(&mut line)?;
    if !line.ends_with('\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "línea cortada o demasiado larga",
        ));
    }
    serde_json::from_str(&line).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("morgan-ipc-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.into())
    }

    #[test]
    fn ruta_por_sesion_grafica() {
        let p = socket_path_from(env(&[
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("WAYLAND_DISPLAY", "wayland-1"),
        ]));
        assert_eq!(
            p.unwrap(),
            PathBuf::from("/run/user/1000/morgan-wayland-1.sock")
        );
    }

    #[test]
    fn ruta_con_display_absoluto() {
        let p = socket_path_from(env(&[
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("WAYLAND_DISPLAY", "/run/user/1000/wayland-2"),
        ]));
        assert_eq!(
            p.unwrap(),
            PathBuf::from("/run/user/1000/morgan-wayland-2.sock")
        );
    }

    #[test]
    fn ruta_sin_variables() {
        assert!(matches!(
            socket_path_from(env(&[("WAYLAND_DISPLAY", "wayland-1")])),
            Err(PathError::NoRuntimeDir)
        ));
        assert!(matches!(
            socket_path_from(env(&[
                ("XDG_RUNTIME_DIR", "/run/user/1000"),
                ("WAYLAND_DISPLAY", "")
            ])),
            Err(PathError::NoWaylandDisplay)
        ));
    }

    #[test]
    fn argumentos() {
        assert_eq!(Request::from_args(&["ping"]), Ok(Request::Ping));
        assert!(Request::from_args(&["ping", "de-mas"]).is_err());
        assert!(Request::from_args::<&str>(&[]).is_err());
        assert!(Request::from_args(&["alternar", "prueba"]).is_err());
    }

    #[test]
    fn argumentos_osd() {
        assert_eq!(
            Request::from_args(&["osd", "message", "TV encendida", "--icon", "tv-symbolic"]),
            Ok(Request::Osd {
                icono: Some("tv-symbolic".into()),
                contenido: Contenido::Mensaje("TV encendida".into()),
                estado: None,
                encendido: false,
                titulo: None,
                ms: None,
            })
        );
        assert_eq!(
            Request::from_args(&[
                "osd",
                "message",
                "Luz nocturna",
                "--state",
                "Activada · 4000 K",
                "--on"
            ]),
            Ok(Request::Osd {
                icono: None,
                contenido: Contenido::Mensaje("Luz nocturna".into()),
                estado: Some("Activada · 4000 K".into()),
                encendido: true,
                titulo: None,
                ms: None,
            })
        );
        assert_eq!(
            Request::from_args(&[
                "osd", "progress", "0.42", "--ms", "2000", "--title", "Brillo"
            ]),
            Ok(Request::Osd {
                icono: None,
                contenido: Contenido::Progreso(0.42),
                estado: None,
                encendido: false,
                titulo: Some("Brillo".into()),
                ms: Some(2000),
            })
        );
        // --state y --on son de un mensaje.
        assert!(Request::from_args(&["osd", "progress", "0.4", "--on"]).is_err());
        // Un texto que empieza con guiones es texto, no una opción.
        assert!(matches!(
            Request::from_args(&["osd", "message", "--ms"]),
            Ok(Request::Osd { contenido: Contenido::Mensaje(t), .. }) if t == "--ms"
        ));
        assert!(Request::from_args(&["osd", "message"]).is_err());
        assert!(Request::from_args(&["osd", "message", "x", "--icon"]).is_err());
        assert!(Request::from_args(&["osd", "message", "x", "--ms", "2s"]).is_err());
        assert!(Request::from_args(&["osd", "message", "x", "--ms", "1", "--ms", "2"]).is_err());
        assert!(Request::from_args(&["osd", "message", "x", "--color", "rojo"]).is_err());
        assert!(Request::from_args(&["osd", "progress", "medio"]).is_err());
    }

    #[test]
    fn rangos_del_osd() {
        assert!(Request::from_args(&["osd", "progress", "1"]).is_ok());
        assert!(Request::from_args(&["osd", "progress", "0"]).is_ok());
        assert!(Request::from_args(&["osd", "progress", "1.01"]).is_err());
        assert!(Request::from_args(&["osd", "progress", "-0.1"]).is_err());
        assert!(Request::from_args(&["osd", "progress", "NaN"]).is_err());
        assert!(Request::from_args(&["osd", "message", "x", "--ms", "200"]).is_ok());
        assert!(Request::from_args(&["osd", "message", "x", "--ms", "60000"]).is_ok());
        assert!(Request::from_args(&["osd", "message", "x", "--ms", "199"]).is_err());
        assert!(Request::from_args(&["osd", "message", "x", "--ms", "60001"]).is_err());
        // Lo mismo si la petición llega armada por otro cliente.
        let r: Request =
            serde_json::from_str(r#"{"cmd":"osd","contenido":{"progreso":7.0}}"#).unwrap();
        assert!(r.validar().is_err());
    }

    #[test]
    fn argumentos_notificaciones() {
        assert_eq!(
            Request::from_args(&["dnd", "toggle"]),
            Ok(Request::NoMolestar {
                accion: AccionInterruptor::Alternar
            })
        );
        assert!(Request::from_args(&["dnd"]).is_err());
        assert!(Request::from_args(&["dnd", "alternar"]).is_err());
        assert_eq!(
            Request::from_args(&["notifications", "clear"]),
            Ok(Request::Limpiar)
        );
        assert!(Request::from_args(&["notifications", "clear", "ya"]).is_err());
        assert_eq!(
            Request::from_args(&["screenshot", "/tmp/a b.png"]),
            Ok(Request::Captura {
                ruta: "/tmp/a b.png".into()
            })
        );
        assert!(Request::from_args(&["screenshot", "a.png"]).is_err());
        assert!(Request::from_args(&["screenshot"]).is_err());
        assert_eq!(
            Request::from_args(&["caffeine", "on"]),
            Ok(Request::Caffeine {
                accion: Some(AccionInterruptor::Activar)
            })
        );
        assert_eq!(
            Request::from_args(&["caffeine", "status"]),
            Ok(Request::Caffeine { accion: None })
        );
        assert!(Request::from_args(&["caffeine"]).is_err());
        assert_eq!(
            Request::from_args(&["nightlight", "toggle"]),
            Ok(Request::Luz {
                accion: Some(AccionInterruptor::Alternar)
            })
        );
        assert_eq!(
            Request::from_args(&["nightlight", "status"]),
            Ok(Request::Luz { accion: None })
        );
        assert!(Request::from_args(&["nightlight", "alternar"]).is_err());
        assert_eq!(
            Request::from_args(&["brightness", "up"]),
            Ok(Request::Brillo { pedido: PedidoBrillo::Subir })
        );
        assert_eq!(
            Request::from_args(&["brightness", "40"]),
            Ok(Request::Brillo { pedido: PedidoBrillo::Poner(40) })
        );
        assert!(Request::from_args(&["brightness", "101"]).is_err());
        assert!(Request::from_args(&["brightness", "raise"]).is_err());
        assert_eq!(
            Request::from_args(&["wallpaper", "/f/a b.jpg"]),
            Ok(Request::Fondo {
                pedido: PedidoFondo::Poner("/f/a b.jpg".into())
            })
        );
        assert_eq!(
            Request::from_args(&["wallpaper", "random"]),
            Ok(Request::Fondo {
                pedido: PedidoFondo::Rotar
            })
        );
        assert!(Request::from_args(&["wallpaper", "a.jpg"]).is_err());
        assert!(Request::from_args(&["wallpaper"]).is_err());
        assert_eq!(Request::from_args(&["capslock"]), Ok(Request::BloqMayus));
        assert!(Request::from_args(&["capslock", "x"]).is_err());
        assert!(Request::from_args(&["caffeine", "alternar"]).is_err());
        assert_eq!(
            Request::from_args(&["center", "open"]),
            Ok(Request::Centro {
                accion: AccionSuperficie::Abrir
            })
        );
        assert!(Request::from_args(&["center", "alternar"]).is_err());
        assert_eq!(
            Request::from_args(&["clock", "toggle"]),
            Ok(Request::Reloj {
                accion: AccionSuperficie::Alternar,
            })
        );
        assert!(Request::from_args(&["clock"]).is_err());
        assert_eq!(
            Request::from_args(&["calculator", "toggle"]),
            Ok(Request::Calculadora {
                accion: AccionSuperficie::Alternar,
            })
        );
        assert!(Request::from_args(&["launcher", "open", "calc"]).is_err());
        assert!(Request::from_args(&["notes", "toggle"]).is_err());
        assert_eq!(
            Request::from_args(&["wallpapers", "toggle"]),
            Ok(Request::Fondos {
                accion: AccionSuperficie::Alternar,
            })
        );
        assert_eq!(
            Request::from_args(&["launcher", "toggle"]),
            Ok(Request::Launcher {
                accion: AccionSuperficie::Alternar,
                modo: ModoLauncher::Apps,
            })
        );
        assert_eq!(
            Request::from_args(&["launcher", "open", "clipboard"]),
            Ok(Request::Launcher {
                accion: AccionSuperficie::Abrir,
                modo: ModoLauncher::Portapapeles,
            })
        );
        assert!(Request::from_args(&["launcher", "open", "windows"]).is_err());
        // Sin modo, apps.
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"cmd":"launcher","accion":"cerrar"}"#).unwrap(),
            Request::Launcher {
                accion: AccionSuperficie::Cerrar,
                modo: ModoLauncher::Apps,
            }
        );
        assert!(Request::from_args(&["launcher"]).is_err());
        assert_eq!(Request::from_args(&["session"]), Ok(Request::Sesion));
        assert_eq!(
            Request::from_args(&["menu", "toggle", "wallpapers"]),
            Ok(Request::Menu {
                accion: AccionSuperficie::Alternar,
                nivel: NivelMenu::Fondos,
            })
        );
        assert!(Request::from_args(&["menu", "open", "temas"]).is_err());
        assert!(Request::from_args(&["session", "open"]).is_err());
        assert!(AccionInterruptor::Alternar.aplicar(false));
        assert!(!AccionInterruptor::Alternar.aplicar(true));
        assert!(AccionInterruptor::Activar.aplicar(true));
        assert!(!AccionInterruptor::Desactivar.aplicar(true));
    }

    #[test]
    fn argumentos_volumen_y_reproductor() {
        assert_eq!(
            Request::from_args(&["volume", "up"]),
            Ok(Request::Volumen {
                accion: AccionVolumen::Subir,
                microfono: false,
                dispositivo: None,
            })
        );
        assert_eq!(
            Request::from_args(&["volume", "mute", "--mic", "--device", "x"]),
            Ok(Request::Volumen {
                accion: AccionVolumen::Silenciar,
                microfono: true,
                dispositivo: Some("x".into()),
            })
        );
        assert!(Request::from_args(&["volume", "up", "--mic", "--mic"]).is_err());
        assert!(Request::from_args(&["volume", "gritar"]).is_err());
        assert!(Request::from_args(&["volume"]).is_err());
        assert_eq!(
            Request::from_args(&["player", "play-pausa"]),
            Ok(Request::Reproductor {
                accion: AccionReproductor::PlayPausa
            })
        );
        assert!(Request::from_args(&["player", "stop"]).is_err());
        assert!(Request::from_args(&["player", "next", "x"]).is_err());
    }

    #[test]
    fn formato_en_el_cable() {
        let r = Request::Osd {
            icono: Some("tv-symbolic".into()),
            contenido: Contenido::Mensaje("TV".into()),
            estado: None,
            encendido: false,
            titulo: None,
            ms: Some(2000),
        };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"cmd":"osd","icono":"tv-symbolic","contenido":{"mensaje":"TV"},"ms":2000}"#
        );
        // Lo opcional puede faltar; lo desconocido no pasa.
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"cmd":"volumen","accion":"bajar"}"#).unwrap(),
            Request::Volumen {
                accion: AccionVolumen::Bajar,
                microfono: false,
                dispositivo: None,
            }
        );
        assert!(
            serde_json::from_str::<Request>(r#"{"cmd":"volumen","accion":"bajar","x":1}"#).is_err()
        );
        assert!(serde_json::from_str::<Request>(r#"{"cmd":"toggle","surface":"prueba"}"#).is_err());
    }

    #[test]
    fn ida_y_vuelta() {
        let dir = tmp_dir("ida");
        let path = dir.join("s.sock");
        let listener = bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            serve(&mut s, |req| {
                assert_eq!(req, Request::Ping);
                Response::Pong {
                    version: "x".into(),
                }
            })
            .unwrap();
        });
        assert_eq!(
            send(&path, &Request::Ping).unwrap(),
            Response::Pong {
                version: "x".into()
            }
        );
        server.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn peticion_ilegible_se_contesta_con_error() {
        let dir = tmp_dir("ilegible");
        let path = dir.join("s.sock");
        let listener = bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            serve(&mut s, |_| panic!("no debería llegar al handler")).unwrap();
        });
        let mut c = UnixStream::connect(&path).unwrap();
        c.write_all(b"{\"cmd\":\"volar\"}\n").unwrap();
        let resp: Response = read_line(&mut c).unwrap();
        assert!(matches!(resp, Response::Error { .. }));
        server.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn socket_viejo_se_reemplaza() {
        let dir = tmp_dir("viejo");
        let path = dir.join("s.sock");
        drop(bind(&path).unwrap()); // queda el archivo, sin nadie escuchando
        assert!(path.exists());
        let _l = bind(&path).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn segundo_morgan_no_arranca() {
        let dir = tmp_dir("segundo");
        let path = dir.join("s.sock");
        let _vivo = bind(&path).unwrap();
        let err = bind(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AddrInUse);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn trabar_es_de_desarrollo_y_con_tope() {
        let r = Request::from_args(&["dev", "block", "800"]).unwrap();
        assert_eq!(r, Request::Trabar { ms: 800 });
        assert!(r.validar().is_ok());
        assert!(Request::Trabar { ms: TRABA_MAX + 1 }.validar().is_err());
        assert!(Request::from_args(&["dev", "block", "mucho"]).is_err());
    }
}
