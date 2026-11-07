//! La escena de `morgan-lock`, aparte del binario para que el greeter (`morgan-greet`) dibuje
//! la misma: lo que se ve (`escena`, `dibujo`, `pintor` y lo suyo), cómo se tipea la clave
//! (`clave`) y lo que se toma de la cuenta (`perfil`). `pantalla` corre todo: superficies,
//! teclado y verificación; cada binario le pasa lo suyo (`pantalla::Ajustes`). PAM queda en el
//! binario del bloqueo.

pub mod caracteres;
pub mod clave;
pub mod dibujo;
pub mod eleccion;
pub mod escena;
pub mod fondo;
mod foco;
pub mod greetd;
pub mod greeter;
pub mod hundido;
pub mod instalar;
mod logind;
pub mod pantalla;
pub mod perfil;
pub mod pintor;
pub mod sombra;
pub mod verificar;
mod xray;

pub(crate) use morgan_motor::texto;
