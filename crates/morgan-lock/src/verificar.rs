//! Quién decide si la clave es buena. El bloqueo pregunta a PAM (`pam.rs` del binario); el
//! greeter, a greetd (`greetd.rs`). La escena no se entera: manda la clave y recibe un
//! `Resultado`.
//!
//! Una sola clave por intento: lo que la pantalla sabe pedir es un campo. Si la pila pide un
//! segundo dato (un código, otra clave), el intento termina en `Error` con el pedido, en vez
//! de contestarle con la misma clave.

use crate::clave::Secreto;

#[derive(Debug, PartialEq)]
pub enum Resultado {
    Bien,
    /// La clave no sirvió. Lo que la pila dijo en el camino (avisos de faillock, etc.), para
    /// mostrar el primero.
    Fallo(Vec<String>),
    /// La pila está rota, no la clave: se informa y no cuenta como intento.
    Error(String),
}

/// Bloquea mientras verifica (pam_unix demora ~2 s al fallar): se llama desde otro hilo. El
/// verificador viaja a ese hilo y vuelve con el resultado, así que no necesita candado.
pub trait Verificador: Send + 'static {
    fn verificar(&mut self, clave: Secreto) -> Resultado;
}
