//! La escena de `morgan-lock`, aparte del binario para que el greeter (`morgan-greet`) dibuje
//! la misma: lo que se ve (`escena`, `dibujo`, `pintor` y lo suyo), cómo se tipea la clave
//! (`clave`) y lo que se toma de la cuenta (`perfil`). Lo que es solo del bloqueo (la
//! superficie de `ext-session-lock`, PAM, logind, el xray de Hyprland) queda en `main.rs`.

pub mod caracteres;
pub mod clave;
pub mod dibujo;
pub mod escena;
pub mod fondo;
pub mod greetd;
pub mod hundido;
pub mod perfil;
pub mod pintor;
pub mod sombra;
pub mod verificar;

pub(crate) use morgan_motor::texto;
