//! El campo de la contraseña. Reserva fija: nunca se realoca, así que no quedan copias viejas
//! en memoria liberada; al vaciarse se pisa con ceros. No hay selección ni portapapeles: lo
//! único que entra es lo que se tipea.

use std::ptr;
use std::sync::atomic::{Ordering, compiler_fence};

use crate::pam::Secreto;

/// Bytes; una contraseña más larga no entra (GTK tampoco tiene límite útil más allá).
const RESERVA: usize = 1024;

pub struct Clave {
    texto: String,
    /// En caracteres, lo que muestra la onda.
    n: usize,
}

impl Clave {
    pub fn new() -> Self {
        Clave {
            texto: String::with_capacity(RESERVA),
            n: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Agrega lo que salió de una tecla (ya compuesto: una tilde, una ñ). Lo que no es
    /// imprimible no entra. Devuelve si cambió.
    pub fn agregar(&mut self, s: &str) -> bool {
        if s.is_empty() || s.chars().any(char::is_control) {
            return false;
        }
        if self.texto.len() + s.len() > self.texto.capacity() {
            return false;
        }
        self.texto.push_str(s);
        self.n += s.chars().count();
        true
    }

    pub fn borrar_uno(&mut self) -> bool {
        let Some(c) = self.texto.pop() else {
            return false;
        };
        // El byte que quedó fuera sigue en la reserva: se pisa.
        let fin = self.texto.len();
        // SAFETY: se escribe en la reserva del String, más allá de su largo; sigue siendo UTF-8
        // válido porque esos bytes no son parte del texto.
        unsafe {
            let p = self.texto.as_mut_vec().as_mut_ptr();
            for i in fin..fin + c.len_utf8() {
                ptr::write_volatile(p.add(i), 0);
            }
        }
        compiler_fence(Ordering::SeqCst);
        self.n -= 1;
        true
    }

    pub fn vaciar(&mut self) {
        // SAFETY: igual que arriba; se pisa toda la reserva y queda vacío.
        unsafe {
            let v = self.texto.as_mut_vec();
            let p = v.as_mut_ptr();
            for i in 0..v.capacity() {
                ptr::write_volatile(p.add(i), 0);
            }
            v.set_len(0);
        }
        compiler_fence(Ordering::SeqCst);
        self.n = 0;
    }

    /// Se la lleva para PAM, sin copiarla: el `Secreto` se pisa al soltarse. El campo queda
    /// con una reserva nueva.
    pub fn tomar(&mut self) -> Secreto {
        self.n = 0;
        Secreto::new(std::mem::replace(
            &mut self.texto,
            String::with_capacity(RESERVA),
        ))
    }
}

impl Drop for Clave {
    fn drop(&mut self) {
        self.vaciar();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuenta_caracteres_no_bytes_y_rechaza_controles() {
        let mut c = Clave::new();
        assert!(c.agregar("ñ"));
        assert!(c.agregar("á"));
        assert!(!c.agregar("\r"));
        assert!(!c.agregar("\u{1b}"));
        assert_eq!(c.len(), 2);
        assert!(c.borrar_uno());
        assert_eq!(c.len(), 1);
        assert_eq!(c.texto, "ñ");
    }

    #[test]
    fn no_pasa_de_la_reserva() {
        let mut c = Clave::new();
        let cap = c.texto.capacity();
        while c.agregar("x") {}
        assert_eq!(c.texto.len(), cap);
        assert_eq!(c.texto.capacity(), cap, "no se realocó");
    }

    #[test]
    fn vaciar_pisa_la_reserva() {
        let mut c = Clave::new();
        c.agregar("secreto");
        c.vaciar();
        let p = c.texto.as_ptr();
        // SAFETY: lectura dentro de la reserva del String.
        let bytes = unsafe { std::slice::from_raw_parts(p, 7) };
        assert_eq!(bytes, [0; 7]);
        assert!(c.is_empty());
    }
}
