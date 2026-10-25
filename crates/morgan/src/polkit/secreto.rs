//! Una clave en camino al ayudante de polkit: se pisa con ceros al soltarse (escritura volátil,
//! como `clave.rs` de `morgan-lock`) y no se imprime nunca, ni con `{:?}`.

use std::fmt;

pub struct Secreto(Vec<u8>);

impl Secreto {
    /// Se lleva lo escrito y deja el texto pisado y vacío.
    pub fn tomar(texto: &mut String) -> Secreto {
        let s = Secreto(texto.as_bytes().to_vec());
        pisar_texto(texto);
        s
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for Secreto {
    fn drop(&mut self) {
        pisar(&mut self.0);
    }
}

impl fmt::Debug for Secreto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secreto(…)")
    }
}

/// Pisa con ceros lo escrito en un campo de clave y lo vacía.
pub fn pisar_texto(texto: &mut String) {
    // SAFETY: los ceros son UTF-8 válido, y el texto se vacía enseguida.
    pisar(unsafe { texto.as_mut_vec() });
    texto.clear();
}

fn pisar(b: &mut [u8]) {
    for x in b.iter_mut() {
        // SAFETY: `x` es una referencia válida; volátil para que no se quite por optimizar.
        unsafe { std::ptr::write_volatile(x, 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_lleva_lo_escrito_y_no_se_imprime() {
        let mut t = String::from("clave");
        let s = Secreto::tomar(&mut t);
        assert!(t.is_empty());
        assert_eq!(s.bytes(), b"clave");
        assert_eq!(format!("{s:?}"), "Secreto(…)");
    }

    #[test]
    fn pisar_deja_ceros_en_la_reserva() {
        let mut t = String::with_capacity(16);
        t.push_str("abc");
        let p = t.as_ptr();
        pisar_texto(&mut t);
        assert!(t.is_empty());
        // SAFETY: la reserva sigue siendo de `t` (vaciar no la suelta).
        let quedo = unsafe { std::slice::from_raw_parts(p, 3) };
        assert_eq!(quedo, [0, 0, 0]);
    }
}
