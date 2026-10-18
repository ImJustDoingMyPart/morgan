//! No abre ninguna superficie ni habla con el morgan que corre: es un programa de terminal
//! dentro del mismo binario. El sonido lo toca `pw-play` desde un memfd: nada queda en disco.

mod clip;
mod guion;

use std::fmt::Write as _;
use std::fs::File;
use std::io::{Seek, Write};
use std::os::fd::FromRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use clip::Clip;
use guion::Texto;

const CUADROS: &[u8] = include_bytes!("cuadros.bin");
const SONIDO: &[u8] = include_bytes!("sonido.ogg");
const MEMORIA: &str = "In memory of Morgan";

pub fn correr() -> ExitCode {
    let clip = match clip::leer(CUADROS) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("morgan about: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ficha = ficha();
    // Sin terminal (una tubería) o en una que no alcanza: solo el texto.
    let tty = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    if !tty || lienzo(tamano(), clip.lado).is_none() {
        for linea in &ficha {
            println!("{linea}");
        }
        println!("\n{MEMORIA}");
        return ExitCode::SUCCESS;
    }

    let Some(terminal) = Terminal::abrir() else {
        eprintln!("morgan about: no se pudo poner la terminal en modo crudo");
        return ExitCode::FAILURE;
    };
    let fondo = fondo_terminal().unwrap_or([0, 0, 0]);
    let salir = Arc::new(AtomicBool::new(false));
    escuchar_teclas(salir.clone());

    let sonido = Sonido::tocar();
    let completo = reproducir(&clip, &ficha, fondo, &salir);
    drop(sonido);
    drop(terminal);
    if completo {
        println!("{MEMORIA}");
    }
    ExitCode::SUCCESS
}

fn ficha() -> Vec<String> {
    let version = env!("CARGO_PKG_VERSION");
    let morgan = match env!("MORGAN_COMMIT") {
        "" => format!("morgan {version}"),
        commit => format!("morgan {version} · {commit}"),
    };
    let rust = match env!("MORGAN_RUSTC") {
        "" => "Rust".to_owned(),
        v => format!("Rust {v}"),
    };
    // La biblioteca contesta sin inicializarse.
    let gtk = format!("GTK {}.{}.{}", gtk::major_version(), gtk::minor_version(), gtk::micro_version());
    vec![
        morgan,
        "a desktop shell for Hyprland".to_owned(),
        format!("{rust} · {gtk} · gtk4-layer-shell"),
        "zbus · PipeWire · matugen".to_owned(),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Tamano {
    columnas: usize,
    filas: usize,
    proporcion: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Lienzo {
    columnas: usize,
    filas: usize,
    ancho: usize,
    alto: usize,
    x0: usize,
    y0: usize,
}

/// A lo ancho, sin deformar el pixelado: múltiplos enteros del clip, o su mitad, tercio o
/// cuarto. A lo alto, lo que lo deja cuadrado en pantalla: media celda casi nunca es cuadrada,
/// así que ahí se muestrea.
fn lienzo(t: Tamano, lado_clip: usize) -> Option<Lienzo> {
    let alto = |ancho: usize| (ancho as f32 * t.proporcion).round() as usize;
    let entra = |ancho: usize| ancho <= t.columnas && alto(ancho) <= t.filas * 2;
    let ancho = (1..=t.columnas / lado_clip.max(1))
        .rev()
        .map(|k| lado_clip * k)
        .chain([2, 3, 4].into_iter().map(|d| lado_clip / d))
        .find(|&a| a > 0 && entra(a))?;
    let alto = alto(ancho);
    Some(Lienzo {
        columnas: t.columnas,
        filas: t.filas,
        ancho,
        alto,
        x0: (t.columnas - ancho) / 2,
        y0: (t.filas - alto.div_ceil(2)) / 2,
    })
}

fn reproducir(clip: &Clip, ficha: &[String], fondo: [u8; 3], salir: &AtomicBool) -> bool {
    let mut salida = std::io::stdout().lock();
    let mut buf = String::with_capacity(1 << 18);
    let claro = clip.claro();
    let inicio = Instant::now();
    let mut previo: Option<Lienzo> = None;
    let mut brillo_previo = -1.0;
    let mut texto_previo: Option<Texto> = None;
    let mut tic = 0u32;
    loop {
        if salir.load(Ordering::Relaxed) {
            return false;
        }
        let t = inicio.elapsed().as_secs_f32();
        if t >= guion::FIN {
            return true;
        }
        buf.clear();
        buf.push_str("\x1b[?2026h"); // salida sincronizada: el cuadro entero de una vez
        let lienzo = lienzo(tamano(), clip.lado);
        let texto = guion::texto(t);
        // Lo que no se repinta solo (un texto que se fue, otro tamaño) se borra entero.
        let limpiar = lienzo != previo || (texto_previo.is_some() && texto.map(|(x, _)| x) != texto_previo);
        if limpiar {
            buf.push_str("\x1b[0m\x1b[2J");
        }
        if let Some(l) = &lienzo {
            let brillo = guion::brillo(t);
            // Oscuro y ya pintado oscuro: el video no cambia nada.
            if limpiar || brillo > 0.0 || brillo_previo != 0.0 {
                let n = ((t * clip.fps) as usize).min(clip.cuadros.len() - 1);
                dibujar(&mut buf, clip, n, l, brillo, fondo);
            }
            brillo_previo = brillo;
            if let Some((cual, alfa)) = texto {
                let lineas: &[String] = match cual {
                    Texto::Ficha => ficha,
                    Texto::Memoria => &[MEMORIA.to_owned()],
                };
                escribir(&mut buf, lineas, l, mezcla(fondo, claro, alfa));
            }
        }
        buf.push_str("\x1b[?2026l");
        if salida.write_all(buf.as_bytes()).and_then(|_| salida.flush()).is_err() {
            return false;
        }
        previo = lienzo;
        texto_previo = texto.map(|(x, _)| x);

        // Al ritmo del clip; si un cuadro tardó de más, se salta al que toca.
        tic += 1;
        let siguiente = Duration::from_secs_f32(tic as f32 / clip.fps);
        match siguiente.checked_sub(inicio.elapsed()) {
            Some(falta) => std::thread::sleep(falta),
            None => tic = (inicio.elapsed().as_secs_f32() * clip.fps) as u32,
        }
    }
}

fn mezcla(a: [u8; 3], b: [u8; 3], f: f32) -> [u8; 3] {
    std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * f).round() as u8)
}

#[derive(Default)]
struct Pincel {
    frente: Option<Option<[u8; 3]>>,
    fondo: Option<Option<[u8; 3]>>,
}

impl Pincel {
    fn frente(&mut self, buf: &mut String, c: Option<[u8; 3]>) {
        if self.frente != Some(c) {
            match c {
                Some([r, g, b]) => _ = write!(buf, "\x1b[38;2;{r};{g};{b}m"),
                None => buf.push_str("\x1b[39m"),
            }
            self.frente = Some(c);
        }
    }

    fn fondo(&mut self, buf: &mut String, c: Option<[u8; 3]>) {
        if self.fondo != Some(c) {
            match c {
                Some([r, g, b]) => _ = write!(buf, "\x1b[48;2;{r};{g};{b}m"),
                None => buf.push_str("\x1b[49m"),
            }
            self.fondo = Some(c);
        }
    }
}

fn dibujar(buf: &mut String, clip: &Clip, n: usize, l: &Lienzo, brillo: f32, fondo: [u8; 3]) {
    let cuadro = &clip.cuadros[n];
    let lado = clip.lado;
    // Apagado del todo es el fondo de la terminal tal cual, no su color: se ve igual que antes.
    let color = |x: usize, y: usize| -> Option<[u8; 3]> {
        if brillo <= 0.0 || y >= l.alto {
            return None;
        }
        let i = cuadro[(y * lado / l.alto) * lado + x * lado / l.ancho];
        Some(mezcla(fondo, clip.paleta[i as usize], brillo))
    };
    let mut pincel = Pincel::default();
    for fila in 0..l.alto.div_ceil(2) {
        _ = write!(buf, "\x1b[{};{}H", l.y0 + fila + 1, l.x0 + 1);
        for x in 0..l.ancho {
            let (arriba, abajo) = (color(x, 2 * fila), color(x, 2 * fila + 1));
            if arriba == abajo {
                pincel.fondo(buf, arriba);
                buf.push(' ');
            } else if arriba.is_none() {
                pincel.frente(buf, abajo);
                pincel.fondo(buf, None);
                buf.push('▄');
            } else {
                pincel.frente(buf, arriba);
                pincel.fondo(buf, abajo);
                buf.push('▀');
            }
        }
    }
}

fn escribir(buf: &mut String, lineas: &[String], l: &Lienzo, color: [u8; 3]) {
    let [r, g, b] = color;
    _ = write!(buf, "\x1b[49m\x1b[38;2;{r};{g};{b}m");
    let y0 = l.filas.saturating_sub(lineas.len()) / 2;
    for (i, linea) in lineas.iter().enumerate() {
        let ancho = linea.chars().count().min(l.columnas);
        _ = write!(buf, "\x1b[{};{}H", y0 + i + 1, (l.columnas - ancho) / 2 + 1);
        buf.extend(linea.chars().take(ancho));
    }
}

fn tamano() -> Tamano {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) } != 0 {
        return Tamano { columnas: 0, filas: 0, proporcion: 1.0 };
    }
    let (columnas, filas) = (ws.ws_col as usize, ws.ws_row as usize);
    let proporcion = if ws.ws_xpixel > 0 && ws.ws_ypixel > 0 && columnas > 0 && filas > 0 {
        // ancho de celda / (alto de celda / 2)
        2.0 * (ws.ws_xpixel as f32 / columnas as f32) / (ws.ws_ypixel as f32 / filas as f32)
    } else {
        1.0
    };
    Tamano { columnas, filas, proporcion }
}

/// Al soltarla, todo vuelve como estaba (también si algo entra en pánico). Ctrl-C llega como
/// tecla, no como señal: así siempre se pasa por acá.
struct Terminal {
    original: libc::termios,
}

impl Terminal {
    fn abrir() -> Option<Terminal> {
        let mut original: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(0, &mut original) } != 0 {
            return None;
        }
        let mut crudo = original;
        crudo.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG | libc::IEXTEN);
        crudo.c_iflag &= !(libc::IXON | libc::ICRNL);
        crudo.c_cc[libc::VMIN] = 1;
        crudo.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(0, libc::TCSANOW, &crudo) } != 0 {
            return None;
        }
        let mut salida = std::io::stdout();
        _ = salida.write_all(b"\x1b[?1049h\x1b[?25l\x1b[0m\x1b[2J");
        _ = salida.flush();
        Some(Terminal { original })
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let mut salida = std::io::stdout();
        _ = salida.write_all(b"\x1b[?2026l\x1b[0m\x1b[?25h\x1b[?1049l");
        _ = salida.flush();
        // TCSAFLUSH: las teclas que quedaron sin leer no le llegan al shell.
        unsafe { libc::tcsetattr(0, libc::TCSAFLUSH, &self.original) };
    }
}

/// Para fundir hacia él y no hacia negro. Una terminal que no contesta en 150 ms se queda con
/// negro.
fn fondo_terminal() -> Option<[u8; 3]> {
    let mut salida = std::io::stdout();
    salida.write_all(b"\x1b]11;?\x1b\\").ok()?;
    salida.flush().ok()?;
    let limite = Instant::now() + Duration::from_millis(150);
    let mut leido = Vec::new();
    loop {
        let falta = limite.checked_duration_since(Instant::now())?.as_millis() as i32;
        let mut pfd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
        if unsafe { libc::poll(&mut pfd, 1, falta) } <= 0 {
            return None;
        }
        let mut b = [0u8; 64];
        let n = unsafe { libc::read(0, b.as_mut_ptr().cast(), b.len()) };
        if n <= 0 {
            return None;
        }
        leido.extend_from_slice(&b[..n as usize]);
        if leido.ends_with(b"\x1b\\") || leido.ends_with(b"\x07") {
            return color_osc(&leido);
        }
    }
}

fn color_osc(respuesta: &[u8]) -> Option<[u8; 3]> {
    let texto = std::str::from_utf8(respuesta).ok()?;
    let rgb = &texto[texto.find("rgb:")? + 4..];
    let mut canales = rgb.split('/').map(|c| {
        let cifras: String = c.chars().take_while(char::is_ascii_hexdigit).collect();
        let v = u32::from_str_radix(&cifras, 16).ok()?;
        let tope = (1u32 << (4 * cifras.len() as u32)) - 1;
        Some((v * 255 / tope) as u8)
    });
    Some([canales.next()??, canales.next()??, canales.next()??])
}

/// El hilo queda bloqueado en `read` hasta que el proceso sale: no hay nada que soltar.
fn escuchar_teclas(salir: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut b = [0u8; 16];
        loop {
            let n = unsafe { libc::read(0, b.as_mut_ptr().cast(), b.len()) };
            if n <= 0 || b[..n as usize].iter().any(|c| matches!(c, b'q' | b'Q' | 0x1b | 0x03 | 0x04)) {
                salir.store(true, Ordering::Relaxed);
                return;
            }
        }
    });
}

struct Sonido(Option<Child>);

impl Sonido {
    fn tocar() -> Sonido {
        let fd = unsafe { libc::memfd_create(c"morgan-about".as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return Sonido(None);
        }
        let mut ogg = unsafe { File::from_raw_fd(fd) };
        if ogg.write_all(SONIDO).and_then(|_| ogg.rewind()).is_err() {
            return Sonido(None);
        }
        let mut cmd = Command::new("pw-play");
        cmd.arg("-").stdin(Stdio::from(ogg)).stdout(Stdio::null()).stderr(Stdio::null());
        // Si morgan muere sin pasar por Drop (se cierra la terminal), pw-play se va con él.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        Sonido(cmd.spawn().ok())
    }
}

impl Drop for Sonido {
    fn drop(&mut self) {
        if let Some(hijo) = &mut self.0 {
            _ = hijo.kill();
            _ = hijo.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(columnas: usize, filas: usize, proporcion: f32) -> Tamano {
        Tamano { columnas, filas, proporcion }
    }

    #[test]
    fn el_lienzo_no_deforma_el_pixelado() {
        let ancho = |c, f| lienzo(t(c, f, 1.0), 108).map(|l| l.ancho);
        assert_eq!(ancho(120, 60), Some(108));
        assert_eq!(ancho(240, 120), Some(216));
        assert_eq!(ancho(80, 24), Some(36));
        assert_eq!(ancho(100, 40), Some(54));
        assert_eq!(ancho(20, 10), None);
    }

    #[test]
    fn el_lienzo_queda_cuadrado_en_pantalla() {
        // kitty: celdas de 9 × 20 px, media celda de 9 × 10.
        let l = lienzo(t(284, 72, 0.9), 108).unwrap();
        assert_eq!((l.ancho, l.alto), (108, 97));
        // Más alta que ancha: a lo alto entran más medias filas.
        let l = lienzo(t(284, 72, 1.2), 108).unwrap();
        assert_eq!((l.ancho, l.alto), (108, 130));
        // Si el alto no entra, baja el ancho al escalón siguiente.
        assert_eq!(lienzo(t(284, 60, 1.2), 108).map(|l| l.ancho), Some(54));
    }

    #[test]
    fn el_lienzo_va_centrado() {
        let l = lienzo(t(120, 60, 1.0), 108).unwrap();
        assert_eq!((l.x0, l.y0), (6, 3));
    }

    #[test]
    fn lee_el_fondo_de_la_terminal() {
        assert_eq!(color_osc(b"\x1b]11;rgb:2828/2828/2828\x1b\\"), Some([0x28; 3]));
        assert_eq!(color_osc(b"\x1b]11;rgb:f/0/8\x07"), Some([255, 0, 136]));
        assert_eq!(color_osc(b"basura"), None);
    }
}
