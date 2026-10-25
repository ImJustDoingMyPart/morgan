//! La conversación con `polkit-agent-helper-1`, el que corre PAM como root. Desde polkit 126
//! no es setuid: se habla con él por `/run/polkit/agent-helper.socket` (lo levanta systemd), y
//! él saca el usuario que pide de las credenciales del socket. Lo que hace
//! `polkitagentsession.c` (polkit 128): se escribe el usuario a autenticar y la cookie, una
//! línea cada uno; él contesta de a líneas (`PAM_PROMPT_ECHO_OFF Contraseña: `, `SUCCESS`…)
//! escapadas con `g_strescape`, y cada pedido se contesta con una línea. Si autentica, él
//! mismo le avisa a polkitd: el agente solo se entera de cómo terminó.
//!
//! Un intento por conexión. El ayudante se abre recién con la clave escrita: uno abierto y
//! cerrado sin contestar cuenta como un fallo para `pam_faillock`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;

use super::secreto::Secreto;

pub const SOCKET: &str = "/run/polkit/agent-helper.socket";

/// Una línea del ayudante.
#[derive(Debug, PartialEq, Eq)]
pub enum Linea {
    /// PAM pide algo; `eco` es si lo escrito se puede ver (un usuario, no una clave).
    Pedir {
        texto: String,
        eco: bool,
    },
    Error(String),
    Info(String),
    Exito,
    Fallo,
    /// Algo que no es del protocolo: se toma como fallo, igual que polkit.
    Otra,
}

pub fn leer(linea: &str) -> Linea {
    let s = descomprimir(linea.strip_suffix('\n').unwrap_or(linea));
    let resto = |p: &str| s.strip_prefix(p).map(str::to_owned);
    if let Some(t) = resto("PAM_PROMPT_ECHO_OFF ") {
        // no-tr
        Linea::Pedir {
            texto: t,
            eco: false,
        }
    } else if let Some(t) = resto("PAM_PROMPT_ECHO_ON ") {
        // no-tr
        Linea::Pedir {
            texto: t,
            eco: true,
        }
    } else if let Some(t) = resto("PAM_ERROR_MSG ") {
        // no-tr
        Linea::Error(t)
    } else if let Some(t) = resto("PAM_TEXT_INFO ") {
        // no-tr
        Linea::Info(t)
    } else if s.starts_with("SUCCESS") {
        Linea::Exito
    } else if s.starts_with("FAILURE") {
        Linea::Fallo
    } else {
        Linea::Otra
    }
}

/// Lo inverso de `g_strescape` (`g_strcompress`): los escapes de C y los octales, que es como
/// llega todo lo que no es ASCII («Contrase\303\261a»).
fn descomprimir(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 == b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        match b[i] {
            d @ b'0'..=b'7' => {
                let mut v = u32::from(d - b'0');
                let mut n = 1;
                while n < 3 && i + n < b.len() && (b'0'..=b'7').contains(&b[i + n]) {
                    v = v * 8 + u32::from(b[i + n] - b'0');
                    n += 1;
                }
                out.push(v as u8);
                i += n;
                continue;
            }
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Lo que cuenta un intento mientras corre.
#[derive(Debug)]
pub enum DelAyudante {
    /// Un pedido que no se contesta con la clave ya escrita (un segundo factor).
    Pedir {
        texto: String,
        eco: bool,
    },
    Aviso {
        texto: String,
        error: bool,
    },
    Fin(bool),
}

/// Un intento, en un hilo: `clave` contesta el primer pedido de clave; los demás pedidos se
/// avisan y esperan su respuesta en `respuestas` (`None` corta). Devuelve el socket, para
/// cortarlo desde afuera con `shutdown`.
pub fn intentar(
    usuario: String,
    cookie: String,
    clave: Secreto,
    respuestas: mpsc::Receiver<Option<Secreto>>,
    avisar: impl Fn(DelAyudante) + Send + 'static,
) -> std::io::Result<UnixStream> {
    let socket = UnixStream::connect(SOCKET)?;
    let copia = socket.try_clone()?;
    std::thread::Builder::new()
        .name("morgan-polkit".into())
        .spawn(move || {
            let fin = conversar(socket, &usuario, &cookie, clave, &respuestas, &avisar);
            avisar(DelAyudante::Fin(fin));
        })?;
    Ok(copia)
}

fn conversar(
    mut socket: UnixStream,
    usuario: &str,
    cookie: &str,
    clave: Secreto,
    respuestas: &mpsc::Receiver<Option<Secreto>>,
    avisar: &impl Fn(DelAyudante),
) -> bool {
    let inicio = format!("{usuario}\n{cookie}\n");
    if socket.write_all(inicio.as_bytes()).is_err() {
        return false;
    }
    let Ok(lector) = socket.try_clone() else {
        return false;
    };
    let mut clave = Some(clave);
    for linea in BufReader::new(lector).lines() {
        let Ok(linea) = linea else { return false };
        match leer(&linea) {
            Linea::Pedir { eco: false, .. } if clave.is_some() => {
                let c = clave.take().expect("recién visto");
                if contestar(&mut socket, &c).is_err() {
                    return false;
                }
            }
            Linea::Pedir { texto, eco } => {
                avisar(DelAyudante::Pedir { texto, eco });
                let Ok(Some(r)) = respuestas.recv() else {
                    return false;
                };
                if contestar(&mut socket, &r).is_err() {
                    return false;
                }
            }
            Linea::Error(texto) => avisar(DelAyudante::Aviso { texto, error: true }),
            Linea::Info(texto) => avisar(DelAyudante::Aviso {
                texto,
                error: false,
            }),
            Linea::Exito => return true,
            Linea::Fallo | Linea::Otra => return false,
        }
    }
    false
}

fn contestar(socket: &mut UnixStream, s: &Secreto) -> std::io::Result<()> {
    socket.write_all(s.bytes())?;
    socket.write_all(b"\n")?;
    socket.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_las_lineas_del_protocolo() {
        assert_eq!(
            leer("PAM_PROMPT_ECHO_OFF Contrase\\303\\261a: \n"),
            Linea::Pedir {
                texto: "Contraseña: ".into(),
                eco: false
            }
        );
        assert_eq!(
            leer("PAM_PROMPT_ECHO_ON login:"),
            Linea::Pedir {
                texto: "login:".into(),
                eco: true
            }
        );
        assert_eq!(
            leer("PAM_ERROR_MSG La cuenta est\\303\\241 bloqueada.\\n"),
            Linea::Error("La cuenta está bloqueada.\n".into())
        );
        assert_eq!(leer("PAM_TEXT_INFO hola"), Linea::Info("hola".into()));
        assert_eq!(leer("SUCCESS\n"), Linea::Exito);
        assert_eq!(leer("FAILURE"), Linea::Fallo);
        assert_eq!(leer("cualquier cosa"), Linea::Otra);
    }

    #[test]
    fn descomprime_como_g_strcompress() {
        assert_eq!(descomprimir(r#"a\tb\\c\"d"#), "a\tb\\c\"d");
        assert_eq!(descomprimir(r"\101\0"), "A\0");
        assert_eq!(descomprimir("fin\\"), "fin\\");
    }

    /// Un ayudante de mentira del otro lado de un par de sockets: la conversación entera.
    #[test]
    fn contesta_la_clave_y_pregunta_lo_demas() {
        let (agente, ayudante) = UnixStream::pair().unwrap();
        let (tx, rx) = mpsc::channel();
        let avisos = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let a = avisos.clone();
        let hilo = std::thread::spawn(move || {
            let mut t = String::from("clave");
            conversar(
                agente,
                "roberto",
                "cookie-1",
                Secreto::tomar(&mut t),
                &rx,
                &|d| a.lock().unwrap().push(format!("{d:?}")),
            )
        });
        let mut lector = BufReader::new(ayudante.try_clone().unwrap());
        let mut escritor = ayudante;
        let mut linea = String::new();
        let mut siguiente = |l: &mut BufReader<UnixStream>| {
            linea.clear();
            l.read_line(&mut linea).unwrap();
            linea.clone()
        };
        assert_eq!(siguiente(&mut lector), "roberto\n");
        assert_eq!(siguiente(&mut lector), "cookie-1\n");
        escritor
            .write_all(b"PAM_PROMPT_ECHO_OFF Password: \n")
            .unwrap();
        assert_eq!(siguiente(&mut lector), "clave\n");
        escritor
            .write_all(b"PAM_PROMPT_ECHO_ON C\\303\\263digo: \n")
            .unwrap();
        let mut r = String::from("123456");
        tx.send(Some(Secreto::tomar(&mut r))).unwrap();
        assert_eq!(siguiente(&mut lector), "123456\n");
        escritor
            .write_all(b"PAM_TEXT_INFO listo\nSUCCESS\n")
            .unwrap();
        assert!(hilo.join().unwrap());
        let avisos = avisos.lock().unwrap();
        assert_eq!(avisos.len(), 2);
        assert!(avisos[0].contains("Código: "));
        assert!(avisos[1].contains("listo"));
    }

    #[test]
    fn cortar_un_pedido_es_un_fallo() {
        let (agente, ayudante) = UnixStream::pair().unwrap();
        let (tx, rx) = mpsc::channel();
        let hilo = std::thread::spawn(move || {
            let mut t = String::from("clave");
            conversar(agente, "r", "c", Secreto::tomar(&mut t), &rx, &|_| {})
        });
        let mut escritor = ayudante;
        escritor
            .write_all(b"PAM_PROMPT_ECHO_OFF a\nPAM_PROMPT_ECHO_OFF b\n")
            .unwrap();
        tx.send(None).unwrap();
        assert!(!hilo.join().unwrap());
    }
}
