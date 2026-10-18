//! Un hilo propio queda bloqueado en `accept` (cero consumo en reposo) y hace toda la E/S del
//! socket: así un cliente que no escribe nunca no congela la interfaz.

use std::os::unix::net::UnixListener;
use std::thread;

use async_channel::{Receiver, Sender};
use morgan_ipc::{Request, Response};

pub struct Call {
    pub request: Request,
    reply: Sender<Response>,
}

impl Call {
    pub fn reply(self, response: Response) {
        // Si el cliente ya se fue, no hay a quién avisarle.
        let _ = self.reply.send_blocking(response);
    }
}

pub fn start(listener: UnixListener) -> Receiver<Call> {
    let (tx, rx) = async_channel::unbounded();
    thread::Builder::new()
        .name("morgan-ipc".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("morgan: accept falló: {e}");
                        continue;
                    }
                };
                let result = morgan_ipc::serve(&mut stream, |request| {
                    let (reply, answer) = async_channel::bounded(1);
                    if tx.send_blocking(Call { request, reply }).is_err() {
                        return Response::Error {
                            message: "morgan se está cerrando".into(),
                        };
                    }
                    answer.recv_blocking().unwrap_or(Response::Error {
                        message: "la petición quedó sin respuesta".into(),
                    })
                });
                if let Err(e) = result {
                    eprintln!("morgan: conexión de morganctl: {e}");
                }
            }
        })
        .expect("no se pudo crear el hilo del socket");
    rx
}
