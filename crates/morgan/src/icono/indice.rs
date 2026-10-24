//! `icon-theme.cache`, el índice que arma `gtk-update-icon-cache` en cada tema (lo mantienen al
//! día los ganchos de pacman): qué carpetas del tema tienen un ícono, sin mirar el disco. Se
//! mapea en memoria, así que solo ocupa lo que se lee. No se usa si es más viejo que la carpeta
//! del tema o que alguna de las suyas: GTK mira solo la del tema, y un ícono agregado después en
//! una subcarpeta (el de lsfg-vk en `~/.local/share/icons/hicolor`) le quedaba invisible.
//!
//! El formato (de `gtk/gtkiconcache.c`), todo en big-endian: la cabecera (versión 1.0, dónde
//! empiezan la tabla y la lista de carpetas), la tabla de nombres (cubetas con cadenas) y, por
//! nombre, la lista de imágenes: la carpeta y qué extensiones hay.

use std::path::Path;

const SVG: u16 = 2;
const PNG: u16 = 4;
const FIN: u32 = 0xFFFF_FFFF;

/// Un archivo mapeado en memoria, de solo lectura.
struct Mapa {
    ptr: *mut libc::c_void,
    largo: usize,
}

impl Mapa {
    fn abrir(ruta: &Path) -> Option<Mapa> {
        use std::os::fd::AsRawFd;
        let f = std::fs::File::open(ruta).ok()?;
        let largo = usize::try_from(f.metadata().ok()?.len()).ok()?;
        if largo == 0 {
            return None;
        }
        // SAFETY: un mapeo de solo lectura y privado de un archivo abierto; se suelta en Drop.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                largo,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                f.as_raw_fd(),
                0,
            )
        };
        (ptr != libc::MAP_FAILED).then_some(Mapa { ptr, largo })
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: `ptr` es un mapeo vigente de `largo` bytes legibles.
        unsafe { std::slice::from_raw_parts(self.ptr as *const u8, self.largo) }
    }
}

impl Drop for Mapa {
    fn drop(&mut self) {
        // SAFETY: el mapeo es de `abrir` y no se usa después.
        unsafe {
            libc::munmap(self.ptr, self.largo);
        }
    }
}

/// El índice de una carpeta de tema.
pub struct Indice {
    mapa: Mapa,
    /// Las carpetas que nombra, en su orden.
    carpetas: Vec<String>,
}

fn u16_en(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn u32_en(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn cadena(b: &[u8], o: usize) -> Option<&[u8]> {
    let resto = b.get(o..)?;
    let fin = resto.iter().position(|c| *c == 0)?;
    Some(&resto[..fin])
}

/// El hash de GTK (`icon_name_hash`), con los bytes como `signed char`.
fn hash(nombre: &[u8]) -> u32 {
    let mut it = nombre.iter().map(|b| *b as i8 as i32 as u32);
    let Some(primero) = it.next() else { return 0 };
    it.fold(primero, |h, c| (h << 5).wrapping_sub(h).wrapping_add(c))
}

impl Indice {
    /// El de `raiz`, si está y no es más viejo que ella ni que `carpetas` (las del tema).
    pub fn abrir<'a>(raiz: &Path, carpetas: impl Iterator<Item = &'a str>) -> Option<Indice> {
        let ruta = raiz.join("icon-theme.cache");
        let del_indice = std::fs::metadata(&ruta).ok()?.modified().ok()?;
        let viejo = |p: &Path| {
            std::fs::metadata(p)
                .and_then(|m| m.modified())
                .is_ok_and(|t| t > del_indice)
        };
        if viejo(raiz) || carpetas.into_iter().any(|c| viejo(&raiz.join(c))) {
            return None;
        }
        Indice::de_mapa(Mapa::abrir(&ruta)?)
    }

    fn de_mapa(mapa: Mapa) -> Option<Indice> {
        let b = mapa.bytes();
        if u16_en(b, 0)? != 1 {
            return None;
        }
        let lista = u32_en(b, 8)? as usize;
        let n = u32_en(b, lista)? as usize;
        let carpetas = (0..n)
            .map(|i| {
                let o = u32_en(b, lista + 4 + 4 * i)? as usize;
                Some(String::from_utf8_lossy(cadena(b, o)?).into_owned())
            })
            .collect::<Option<Vec<String>>>()?;
        Some(Indice { mapa, carpetas })
    }

    /// Las carpetas (por su nombre en el tema) que tienen `nombre`, con su extensión: PNG
    /// antes que SVG, como en la especificación.
    pub fn buscar(&self, nombre: &str) -> Vec<(&str, &'static str)> {
        self.buscar_bytes(nombre).unwrap_or_default()
    }

    fn buscar_bytes(&self, nombre: &str) -> Option<Vec<(&str, &'static str)>> {
        let b = self.mapa.bytes();
        let tabla = u32_en(b, 4)? as usize;
        let cubetas = u32_en(b, tabla)?;
        if cubetas == 0 {
            return None;
        }
        let mut icono = u32_en(
            b,
            tabla + 4 + 4 * (hash(nombre.as_bytes()) % cubetas) as usize,
        )?;
        while icono != FIN {
            let o = icono as usize;
            if cadena(b, u32_en(b, o + 4)? as usize)? == nombre.as_bytes() {
                let lista = u32_en(b, o + 8)? as usize;
                let n = u32_en(b, lista)? as usize;
                let mut v = Vec::new();
                for i in 0..n {
                    let im = lista + 4 + 8 * i;
                    let carpeta = self.carpetas.get(usize::from(u16_en(b, im)?))?;
                    let marcas = u16_en(b, im + 2)?;
                    if marcas & PNG != 0 {
                        v.push((carpeta.as_str(), "png"));
                    }
                    if marcas & SVG != 0 {
                        v.push((carpeta.as_str(), "svg"));
                    }
                }
                return Some(v);
            }
            icono = u32_en(b, o)?;
        }
        Some(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_hash_de_gtk() {
        // h = h * 31 + c, desde el primero.
        assert_eq!(hash(b"a"), 97);
        assert_eq!(hash(b"ab"), 97 * 31 + 98);
        assert_eq!(hash(b""), 0);
    }

    /// Contra el índice de hicolor de esta máquina, si está: lo que dice que hay, existe.
    #[test]
    fn lo_que_dice_el_indice_existe() {
        let raiz = Path::new("/usr/share/icons/hicolor");
        let Some(i) = Indice::abrir(raiz, std::iter::empty()) else {
            return;
        };
        let mut probados = 0;
        for carpeta in i.carpetas.iter().take(40) {
            let Ok(dir) = std::fs::read_dir(raiz.join(carpeta)) else {
                continue;
            };
            for e in dir.flatten().take(3) {
                let nombre = e.file_name().to_string_lossy().into_owned();
                let Some((n, ext)) = nombre.rsplit_once('.') else {
                    continue;
                };
                if ext != "png" && ext != "svg" {
                    continue;
                }
                let hay = i.buscar(n);
                assert!(
                    hay.iter().any(|(c, x)| c == carpeta && *x == ext),
                    "{carpeta}/{nombre}: {hay:?}"
                );
                probados += 1;
            }
        }
        assert!(probados > 0 || i.carpetas.is_empty());
    }
}
