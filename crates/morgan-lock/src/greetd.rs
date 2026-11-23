//! Cliente de greetd, según `greetd-ipc(7)` (greetd 0.10): por el socket de `GREETD_SOCK`,
//! cada mensaje es un largo de 32 bits en el orden de bytes de la máquina y JSON UTF-8.
//!
//! El intento de entrar es `create_session`, después tantos `auth_message` como pida la
//! pila, cada uno con su `post_auth_message_response`, hasta `success` o `error`. Un
//! `auth_error` (clave mala) se cierra con `cancel_session`, y el próximo intento vuelve a
//! empezar. Con `success`, `start_session` deja la sesión lista y arranca cuando el greeter
//! termina.
//!
//! La clave va en un JSON armado en una reserva que no se realoca y se pisa con ceros
//! después de escribirla, como el campo (`clave.rs`).

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;

use serde::{Deserialize, Serialize};

use crate::clave::{Secreto, borrar};
use crate::verificar::{Resultado, Verificador};

/// Un mensaje de greetd más largo que esto es un error del otro lado, no algo que leer.
const MAXIMO: u32 = 64 * 1024;

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Pedido<'a> {
    CreateSession {
        username: &'a str,
    },
    PostAuthMessageResponse {
        #[serde(skip_serializing_if = "Option::is_none")]
        response: Option<&'a str>,
    },
    StartSession {
        cmd: &'a [String],
        env: &'a [String],
    },
    CancelSession,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Respuesta {
    Success,
    Error {
        error_type: TipoError,
        description: String,
    },
    AuthMessage {
        auth_message_type: TipoMensaje,
        auth_message: String,
    },
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoError {
    AuthError,
    Error,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoMensaje {
    Visible,
    Secret,
    Info,
    Error,
}

/// Escribe un pedido. El JSON se arma en una reserva de tamaño suficiente para el peor caso
/// de escape (6 bytes por byte de texto), así serde no la realoca y no quedan copias de la
/// clave en memoria liberada; después se pisa.
pub fn escribir(w: &mut impl Write, p: &Pedido) -> io::Result<()> {
    let texto = match p {
        Pedido::CreateSession { username } => username.len(),
        Pedido::PostAuthMessageResponse { response } => response.map_or(0, str::len),
        Pedido::StartSession { cmd, env } => cmd.iter().chain(env.iter()).map(String::len).sum(),
        Pedido::CancelSession => 0,
    };
    let mut buf = Vec::with_capacity(128 + 6 * texto + 8 * extra(p));
    let r = (|| {
        serde_json::to_writer(&mut buf, p).map_err(io::Error::other)?;
        let n = u32::try_from(buf.len()).map_err(io::Error::other)?;
        w.write_all(&n.to_ne_bytes())?;
        w.write_all(&buf)?;
        w.flush()
    })();
    borrar(&mut buf);
    r
}

/// Comillas y comas de las listas de `start_session`.
fn extra(p: &Pedido) -> usize {
    match p {
        Pedido::StartSession { cmd, env } => cmd.len() + env.len(),
        _ => 0,
    }
}

pub fn leer(r: &mut impl Read) -> io::Result<Respuesta> {
    let mut largo = [0u8; 4];
    r.read_exact(&mut largo)?;
    let n = u32::from_ne_bytes(largo);
    if n > MAXIMO {
        return Err(io::Error::other(format!("greetd: mensaje de {n} bytes"))); // no-tr
    }
    let mut buf = vec![0u8; n as usize];
    r.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

/// La conexión con greetd y la cuenta que intenta entrar.
pub struct Greetd<S: Read + Write = UnixStream> {
    socket: S,
    usuario: String,
    /// Hubo `success`: la sesión está lista para `iniciar`.
    lista: bool,
}

impl Greetd<UnixStream> {
    /// Por `GREETD_SOCK`, como lo deja greetd en el entorno del greeter.
    pub fn conectar(usuario: &str) -> io::Result<Self> {
        let ruta = std::env::var_os("GREETD_SOCK")
            .ok_or_else(|| io::Error::other("greetd: falta GREETD_SOCK"))?; // no-tr
        Ok(Greetd::sobre(UnixStream::connect(ruta)?, usuario))
    }
}

impl<S: Read + Write> Greetd<S> {
    pub fn sobre(socket: S, usuario: &str) -> Self {
        Greetd {
            socket,
            usuario: usuario.to_owned(),
            lista: false,
        }
    }

    /// Cambia la cuenta. Si había una sesión a medio armar o ya lista, se cancela.
    pub fn usuario(&mut self, usuario: &str) -> io::Result<()> {
        if self.lista {
            self.cancelar()?;
        }
        self.usuario = usuario.to_owned();
        Ok(())
    }

    fn pedir(&mut self, p: &Pedido) -> io::Result<Respuesta> {
        escribir(&mut self.socket, p)?;
        leer(&mut self.socket)
    }

    fn cancelar(&mut self) -> io::Result<()> {
        self.lista = false;
        match self.pedir(&Pedido::CancelSession)? {
            Respuesta::Success => Ok(()),
            otra => Err(io::Error::other(format!(
                "greetd: cancel_session: {otra:?}" // no-tr
            ))),
        }
    }

    fn intento(&mut self, clave: &Secreto) -> io::Result<Resultado> {
        if self.lista {
            // Un intento después de un acierto sin iniciar: se empieza de nuevo.
            self.cancelar()?;
        }
        let usuario = std::mem::take(&mut self.usuario);
        let r = self.pedir(&Pedido::CreateSession { username: &usuario });
        self.usuario = usuario;
        let mut r = r?;
        let mut usada = false;
        let mut mensajes = Vec::new();
        loop {
            r = match r {
                Respuesta::Success => {
                    self.lista = true;
                    return Ok(Resultado::Bien);
                }
                Respuesta::Error {
                    error_type,
                    description,
                } => {
                    // La sesión a medio armar queda en greetd: se cancela para el próximo
                    // intento. Si ya no había nada que cancelar, greetd contesta error y da
                    // igual.
                    let _ = self.pedir(&Pedido::CancelSession);
                    return Ok(match error_type {
                        // La descripción es el código de PAM («pam_authenticate:
                        // AUTH_ERR»), no algo para mostrar: como en el bloqueo, solo los
                        // avisos de la conversación.
                        TipoError::AuthError => Resultado::Fallo(mensajes),
                        TipoError::Error => Resultado::Error(format!("greetd: {description}")), // no-tr
                    });
                }
                Respuesta::AuthMessage {
                    auth_message_type,
                    auth_message,
                } => match auth_message_type {
                    TipoMensaje::Secret if !usada => {
                        usada = true;
                        let Ok(texto) = std::str::from_utf8(clave.bytes()) else {
                            let _ = self.pedir(&Pedido::CancelSession);
                            return Ok(Resultado::Fallo(mensajes));
                        };
                        self.pedir(&Pedido::PostAuthMessageResponse {
                            response: Some(texto),
                        })?
                    }
                    TipoMensaje::Secret | TipoMensaje::Visible => {
                        let _ = self.pedir(&Pedido::CancelSession);
                        return Ok(Resultado::Error(format!(
                            "greetd pide otro dato que esta pantalla no sabe pedir: {}", // no-tr
                            auth_message.trim()
                        )));
                    }
                    TipoMensaje::Info | TipoMensaje::Error => {
                        let m = auth_message.trim();
                        if !m.is_empty() {
                            mensajes.push(m.to_owned());
                        }
                        self.pedir(&Pedido::PostAuthMessageResponse { response: None })?
                    }
                },
            };
        }
    }

    /// Arranca la sesión verificada: greetd la lanza cuando el greeter termina.
    pub fn iniciar(&mut self, cmd: &[String], env: &[String]) -> io::Result<()> {
        if !self.lista {
            return Err(io::Error::other("greetd: no hay sesión verificada")); // no-tr
        }
        match self.pedir(&Pedido::StartSession { cmd, env })? {
            Respuesta::Success => Ok(()),
            Respuesta::Error { description, .. } => Err(io::Error::other(format!(
                "greetd: start_session: {description}" // no-tr
            ))),
            otra => Err(io::Error::other(format!("greetd: start_session: {otra:?}"))), // no-tr
        }
    }
}

impl<S: Read + Write + Send + 'static> Verificador for Greetd<S> {
    fn verificar(&mut self, clave: Secreto) -> Resultado {
        let r = self.intento(&clave);
        drop(clave);
        r.unwrap_or_else(|e| Resultado::Error(format!("greetd: {e}"))) // no-tr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    /// greetd de mentira: del otro lado del par, lee cada pedido como JSON y contesta con lo
    /// que diga el guion. Devuelve los pedidos que recibió.
    fn falso(guion: Vec<&'static str>) -> (UnixStream, thread::JoinHandle<Vec<serde_json::Value>>) {
        let (a, mut b) = UnixStream::pair().unwrap();
        let h = thread::spawn(move || {
            let mut vistos = Vec::new();
            for r in guion {
                let mut largo = [0u8; 4];
                if b.read_exact(&mut largo).is_err() {
                    break;
                }
                let mut buf = vec![0u8; u32::from_ne_bytes(largo) as usize];
                b.read_exact(&mut buf).unwrap();
                vistos.push(serde_json::from_slice(&buf).unwrap());
                b.write_all(&(r.len() as u32).to_ne_bytes()).unwrap();
                b.write_all(r.as_bytes()).unwrap();
            }
            vistos
        });
        (a, h)
    }

    const SECRETO: &str =
        r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#;
    const BIEN: &str = r#"{"type":"success"}"#;
    const MAL: &str =
        r#"{"type":"error","error_type":"auth_error","description":"pam_authenticate: AUTH_ERR"}"#;

    fn tipo(v: &serde_json::Value) -> &str {
        v["type"].as_str().unwrap()
    }

    #[test]
    fn lee_el_ejemplo_del_manual() {
        // El hexdump de greetd-ipc(7), en el orden de bytes de la máquina.
        let json = br#"{"type": "create_session", "username": "me"}"#;
        assert_eq!(json.len(), 0x2c);
        let v: serde_json::Value = serde_json::from_slice(json).unwrap();
        let mut buf = Vec::new();
        escribir(&mut buf, &Pedido::CreateSession { username: "me" }).unwrap();
        assert_eq!(&buf[..4], &((buf.len() - 4) as u32).to_ne_bytes());
        let nuestro: serde_json::Value = serde_json::from_slice(&buf[4..]).unwrap();
        assert_eq!(nuestro, v);
    }

    #[test]
    fn respuesta_informativa_sin_campo_y_cancelar_sin_campos() {
        let mut buf = Vec::new();
        escribir(
            &mut buf,
            &Pedido::PostAuthMessageResponse { response: None },
        )
        .unwrap();
        assert_eq!(&buf[4..], br#"{"type":"post_auth_message_response"}"#);
        buf.clear();
        escribir(&mut buf, &Pedido::CancelSession).unwrap();
        assert_eq!(&buf[4..], br#"{"type":"cancel_session"}"#);
    }

    #[test]
    fn lee_las_tres_respuestas() {
        let leer_de = |s: &str| {
            let mut b = (s.len() as u32).to_ne_bytes().to_vec();
            b.extend_from_slice(s.as_bytes());
            leer(&mut b.as_slice()).unwrap()
        };
        assert_eq!(leer_de(BIEN), Respuesta::Success);
        assert_eq!(
            leer_de(SECRETO),
            Respuesta::AuthMessage {
                auth_message_type: TipoMensaje::Secret,
                auth_message: "Password: ".into()
            }
        );
        assert!(matches!(
            leer_de(MAL),
            Respuesta::Error {
                error_type: TipoError::AuthError,
                ..
            }
        ));
    }

    #[test]
    fn un_largo_absurdo_no_se_lee() {
        let b = (MAXIMO + 1).to_ne_bytes();
        assert!(leer(&mut b.as_slice()).is_err());
    }

    #[test]
    fn clave_correcta_y_despues_iniciar() {
        let (s, h) = falso(vec![SECRETO, BIEN, BIEN]);
        let mut g = Greetd::sobre(s, "anon");
        assert_eq!(g.verificar(Secreto::new("hunter2".into())), Resultado::Bien);
        g.iniciar(
            &["uwsm".into(), "start".into()],
            &["XDG_SESSION_TYPE=wayland".into()],
        )
        .unwrap();
        drop(g);
        let v = h.join().unwrap();
        assert_eq!(tipo(&v[0]), "create_session");
        assert_eq!(v[0]["username"], "anon");
        assert_eq!(tipo(&v[1]), "post_auth_message_response");
        assert_eq!(v[1]["response"], "hunter2");
        assert_eq!(tipo(&v[2]), "start_session");
        assert_eq!(v[2]["cmd"], serde_json::json!(["uwsm", "start"]));
        assert_eq!(v[2]["env"], serde_json::json!(["XDG_SESSION_TYPE=wayland"]));
    }

    #[test]
    fn clave_mala_cancela_y_el_proximo_intento_empieza_de_nuevo() {
        let (s, h) = falso(vec![SECRETO, MAL, BIEN, SECRETO, BIEN]);
        let mut g = Greetd::sobre(s, "anon");
        assert_eq!(
            g.verificar(Secreto::new("mal".into())),
            Resultado::Fallo(Vec::new())
        );
        assert_eq!(g.verificar(Secreto::new("bien".into())), Resultado::Bien);
        drop(g);
        let v = h.join().unwrap();
        let tipos: Vec<_> = v.iter().map(tipo).collect();
        assert_eq!(
            tipos,
            [
                "create_session",
                "post_auth_message_response",
                "cancel_session",
                "create_session",
                "post_auth_message_response"
            ]
        );
    }

    #[test]
    fn los_avisos_se_contestan_vacios_y_se_juntan() {
        let info = r#"{"type":"auth_message","auth_message_type":"info","auth_message":"Quedan 2 intentos"}"#;
        let (s, h) = falso(vec![info, SECRETO, MAL, BIEN]);
        let mut g = Greetd::sobre(s, "anon");
        assert_eq!(
            g.verificar(Secreto::new("x".into())),
            Resultado::Fallo(vec!["Quedan 2 intentos".into()])
        );
        drop(g);
        let v = h.join().unwrap();
        assert_eq!(tipo(&v[1]), "post_auth_message_response");
        assert!(v[1].get("response").is_none());
        assert_eq!(v[2]["response"], "x");
    }

    #[test]
    fn un_segundo_pedido_no_recibe_la_misma_clave() {
        let otro =
            r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Código: "}"#;
        let (s, h) = falso(vec![SECRETO, otro, BIEN]);
        let mut g = Greetd::sobre(s, "anon");
        let r = g.verificar(Secreto::new("hunter2".into()));
        assert!(
            matches!(r, Resultado::Error(ref e) if e.contains("Código")),
            "{r:?}"
        );
        drop(g);
        let v = h.join().unwrap();
        assert_eq!(tipo(&v[2]), "cancel_session");
        assert_eq!(v.iter().filter(|x| x["response"] == "hunter2").count(), 1);
    }

    #[test]
    fn error_general_es_error_y_no_fallo() {
        let roto = r#"{"type":"error","error_type":"error","description":"no such user"}"#;
        let (s, _h) = falso(vec![roto, BIEN]);
        let mut g = Greetd::sobre(s, "nadie");
        assert!(matches!(
            g.verificar(Secreto::new("x".into())),
            Resultado::Error(_)
        ));
        assert!(g.iniciar(&[], &[]).is_err(), "sin acierto no se inicia");
    }

    #[test]
    fn greetd_que_se_cae_es_error() {
        let (s, h) = falso(vec![]);
        h.join().unwrap();
        let mut g = Greetd::sobre(s, "anon");
        assert!(matches!(
            g.verificar(Secreto::new("x".into())),
            Resultado::Error(_)
        ));
    }
}
