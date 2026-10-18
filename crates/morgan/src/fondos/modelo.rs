
use std::path::{Path, PathBuf};

use crate::launcher::busqueda::plegar;

pub const ALTO_BANDA: f64 = 0.30;
const SEPARACION_MIN: i32 = 20;
pub const VECINAS: usize = 3;
const CORTE: f64 = 0.5;
pub const MARGEN_GLOW: i32 = 22;
pub const ANILLO: i32 = 2;
const CABECERA: i32 = 42;

const EXTENSIONES: [&str; 5] = ["jpg", "jpeg", "png", "webp", "gif"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometria {
    pub alto: i32,
    pub lado: i32,
    pub celda: i32,
    pub separacion: i32,
    pub paso: i32,
    pub costado: i32,
}

/// El paso no es fijo: sale del ancho para que la celda número `VECINAS` a cada lado quede
/// cortada por el borde y se vea que la tira sigue.
pub fn geometria(ancho: i32, alto_salida: i32) -> Geometria {
    let alto = (f64::from(alto_salida) * ALTO_BANDA).round() as i32;
    let lado = (alto - CABECERA - 2 * MARGEN_GLOW).max(120);
    let celda = lado + 2 * ANILLO;
    let paso = (f64::from(ancho) / 2.0 + f64::from(celda) / 2.0 - CORTE * f64::from(celda))
        / VECINAS as f64;
    let separacion = (paso.round() as i32 - celda).max(SEPARACION_MIN);
    Geometria {
        alto,
        lado,
        celda,
        separacion,
        paso: celda + separacion,
        costado: ancho / 2 - celda / 2,
    }
}

/// Sin carpetas ocultas (los repos de fondos son clones de git). La banda, las miniaturas y la
/// rotación usan esta misma lista: si difirieran, la rotación podría poner un fondo que la
/// banda no muestra.
pub fn listar(raiz: &Path, excluir: &[String]) -> Vec<PathBuf> {
    let mut v = Vec::new();
    recorrer(raiz, excluir, &mut v);
    v.sort_by_cached_key(|p| plegar(&p.to_string_lossy()));
    v
}

fn recorrer(dir: &Path, excluir: &[String], v: &mut Vec<PathBuf>) {
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entradas.flatten() {
        let nombre = e.file_name();
        let nombre = nombre.to_string_lossy();
        let ruta = e.path();
        let Ok(tipo) = e.file_type() else { continue };
        if tipo.is_dir() {
            if !nombre.starts_with('.') && !excluir.iter().any(|x| *x == nombre) {
                recorrer(&ruta, excluir, v);
            }
        } else if es_imagen(&ruta) {
            v.push(ruta);
        }
    }
}

fn es_imagen(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONES.contains(&e.to_lowercase().as_str()))
}

pub fn nombre_de(p: &Path) -> String {
    let base = p
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    let mut s = String::new();
    let mut separando = false;
    for c in base.chars() {
        if c == '_' || c == '-' {
            if !separando {
                s.push(' ');
            }
            separando = true;
        } else {
            s.push(c);
            separando = false;
        }
    }
    s
}

pub fn filtrar(todos: &[PathBuf], filtro: &str) -> Vec<usize> {
    let ascii = |s: &str| plegar(s).replace('ñ', "n");
    let clave = ascii(filtro);
    (0..todos.len())
        .filter(|i| ascii(&nombre_de(&todos[*i])).contains(&clave))
        .collect()
}

pub fn orden_de_carga(n: usize, sel: usize) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    v.sort_by_key(|i| i.abs_diff(sel));
    v
}

/// Espeja su ruta absoluta bajo la caché, así no hay que hashear nada para encontrarla.
pub fn miniatura(cache: &Path, imagen: &Path) -> PathBuf {
    let mut s = cache.as_os_str().to_owned();
    s.push(imagen.as_os_str());
    s.push(".png");
    PathBuf::from(s)
}

pub fn sin_miniatura(cache: &Path, todos: &[PathBuf]) -> Vec<PathBuf> {
    let cambio = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    todos
        .iter()
        .filter(|img| match (cambio(img), cambio(&miniatura(cache, img))) {
            (_, None) => true,
            (Some(i), Some(m)) => i > m,
            (None, Some(_)) => false,
        })
        .cloned()
        .collect()
}

/// Con uno solo no hay nada que rotar: volver a ponerlo regeneraría el tema y la copia del
/// bloqueo para nada.
pub fn otro<'a>(todos: &'a [PathBuf], actual: Option<&Path>, azar: u32) -> Option<&'a PathBuf> {
    if todos.len() < 2 {
        return None;
    }
    let candidatos: Vec<&PathBuf> = todos.iter().filter(|p| Some(p.as_path()) != actual).collect();
    (!candidatos.is_empty()).then(|| candidatos[azar as usize % candidatos.len()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometria_de_dp3() {
        let g = geometria(2560, 1440);
        assert_eq!(g.alto, 432);
        assert_eq!(g.lado, 432 - 42 - 44);
        assert_eq!(g.celda, g.lado + 4);
        assert_eq!(g.paso, g.celda + g.separacion);
        // La celda número 3 a la derecha empieza a media celda del borde.
        let borde_izq_de_la_tercera = 2560 / 2 - g.celda / 2 + 3 * g.paso;
        assert!((borde_izq_de_la_tercera + g.celda / 2 - 2560).abs() <= 2);
        assert_eq!(g.costado, 1280 - g.celda / 2);
    }

    #[test]
    fn la_separacion_tiene_piso() {
        // Una salida angosta: el paso ideal no deja aire; queda el mínimo.
        let g = geometria(800, 1440);
        assert_eq!(g.separacion, SEPARACION_MIN);
        // Una muy baja: la foto no baja de 120.
        assert_eq!(geometria(1920, 300).lado, 120);
    }

    #[test]
    fn listado_de_la_carpeta() {
        let raiz = std::env::temp_dir().join(format!("morgan-fondos-{}", std::process::id()));
        let crear = |p: &str| {
            let r = raiz.join(p);
            std::fs::create_dir_all(r.parent().unwrap()).unwrap();
            std::fs::write(r, b"").unwrap();
        };
        crear("Árbol.jpg");
        crear("sub/b.PNG");
        crear("sub/nota.txt");
        crear(".ocultos/x.jpg");
        crear("Screenshots/c.png");
        crear("Fastfetch/d.png");
        crear("sub/.oculto/e.webp");
        let excluir = ["Screenshots".to_owned(), "Fastfetch".to_owned()];
        let nombres: Vec<String> = listar(&raiz, &excluir)
            .iter()
            .map(|p| {
                p.strip_prefix(&raiz)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(nombres, ["Árbol.jpg", "sub/b.PNG"]);
        std::fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn filtro_por_nombre() {
        let todos: Vec<PathBuf> = ["/f/Puesta_de--Sol.jpg", "/f/montaña.png", "/f/mar.webp"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(nombre_de(&todos[0]), "Puesta de Sol");
        assert_eq!(filtrar(&todos, "de sol"), [0]);
        assert_eq!(filtrar(&todos, "MONTANA"), [1]);
        assert_eq!(filtrar(&todos, "m"), [1, 2]);
        assert_eq!(filtrar(&todos, ""), [0, 1, 2]);
    }

    #[test]
    fn carga_por_cercania() {
        assert_eq!(orden_de_carga(5, 2), [2, 1, 3, 0, 4]);
        assert_eq!(orden_de_carga(3, 0), [0, 1, 2]);
        assert!(orden_de_carga(0, 0).is_empty());
    }

    #[test]
    fn la_rotacion_no_repite_el_actual() {
        let todos: Vec<PathBuf> = ["/a.jpg", "/b.jpg", "/c.jpg"].iter().map(PathBuf::from).collect();
        let actual = Some(Path::new("/b.jpg"));
        for azar in 0..10 {
            assert_ne!(otro(&todos, actual, azar).map(PathBuf::as_path), actual);
        }
        assert_eq!(otro(&todos[..1], None, 3), None);
        assert_eq!(otro(&[], None, 3), None);
        // Un actual que no está en la carpeta: cualquiera sirve.
        assert!(otro(&todos, Some(Path::new("/z.jpg")), 7).is_some());
    }

    #[test]
    fn miniaturas_que_faltan() {
        let raiz = std::env::temp_dir().join(format!("morgan-minis-{}", std::process::id()));
        let cache = raiz.join("cache");
        let (a, b) = (raiz.join("a.jpg"), raiz.join("b.jpg"));
        std::fs::create_dir_all(&raiz).unwrap();
        std::fs::write(&a, b"").unwrap();
        std::fs::write(&b, b"").unwrap();
        let mb = miniatura(&cache, &b);
        std::fs::create_dir_all(mb.parent().unwrap()).unwrap();
        std::fs::write(&mb, b"").unwrap();
        assert_eq!(sin_miniatura(&cache, &[a.clone(), b.clone()]), [a]);
        std::fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn ruta_de_la_miniatura() {
        assert_eq!(
            miniatura(Path::new("/c/256x256"), Path::new("/home/a/f.jpg")),
            PathBuf::from("/c/256x256/home/a/f.jpg.png")
        );
    }
}
