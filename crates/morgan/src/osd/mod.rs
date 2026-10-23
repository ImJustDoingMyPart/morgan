//! El aviso que aparece abajo, en todas las salidas, y se va solo. Sin GTK: lo dibuja su
//! propio hilo con el motor de `morgan-motor` (`hilo.rs`, `dibujo.rs`).
//!
//! La forma es la del centro de control, para que pasar de un aviso a otro cambie el contenido
//! y no la caja. Todos miden lo mismo: un ancho fijo, acotado a la salida, y dos renglones.

mod dibujo;
mod hilo;

use morgan_motor::color::Paleta;
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::tema::Tema;

const MARGEN_INFERIOR: f64 = 0.15;

/// El tamaño de la ventana, con el lugar del brillo del borde: la tarjeta mide 12 px menos.
pub const ANCHO: i32 = 320;
pub const ALTO: i32 = 74;

#[derive(Debug, Clone, PartialEq)]
pub struct Aviso {
    pub icono: Option<String>,
    pub titulo: String,
    pub cuerpo: Cuerpo,
    pub ms: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cuerpo {
    Estado {
        texto: String,
        encendido: bool,
    },
    /// `apagada`: el nivel que había antes de silenciar, atenuado.
    Barra {
        fraccion: f64,
        porcentaje: f64,
        apagada: bool,
    },
}

impl Aviso {
    pub fn estado(icono: &str, titulo: &str, texto: &str, encendido: bool, ms: u32) -> Aviso {
        Aviso {
            icono: Some(icono.into()),
            titulo: titulo.into(),
            cuerpo: Cuerpo::Estado {
                texto: texto.into(),
                encendido,
            },
            ms,
        }
    }
}

/// Un par de caracteres que no son ASCII.
pub fn es_glifo(icono: &str) -> bool {
    !icono.is_ascii() && icono.chars().count() <= 2
}

pub fn porcentaje(p: f64, apagada: bool) -> String {
    if apagada {
        "mudo".into()
    } else {
        format!("{p:.0}%")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tope {
    pub ancho: i32,
    pub alto: i32,
    pub margen_inferior: i32,
}

pub fn tope(ancho: i32, alto: i32) -> Tope {
    let margen = (f64::from(alto) * MARGEN_INFERIOR).round() as i32;
    Tope {
        ancho: (ancho - 2 * margen).max(1),
        alto: (alto - 2 * margen).max(1),
        margen_inferior: margen,
    }
}

#[derive(Clone)]
pub struct Osd {
    ordenes: Sender<hilo::Orden>,
}

impl Osd {
    /// Lanza el hilo del OSD, que sigue los colores de `tema`.
    pub fn new(tema: &Tema) -> Self {
        let (css, opacidad, borde) = tema.actual();
        let ordenes = hilo::lanzar(Paleta::desde_css(&css), opacidad as f32, borde);
        let tx = ordenes.clone();
        tema.al_cambiar(move |css, opacidad, borde| {
            let _ = tx.send(hilo::Orden::Tema(
                Paleta::desde_css(css),
                opacidad as f32,
                borde,
            ));
        });
        Osd { ordenes }
    }

    pub fn mostrar(&self, aviso: &Aviso) {
        // Si el hilo terminó (sin Wayland o sin GPU) ya lo dijo al terminar.
        let _ = self.ordenes.send(hilo::Orden::Mostrar(aviso.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tope_de_la_pantalla_principal() {
        // 2560×1440.
        assert_eq!(
            tope(2560, 1440),
            Tope {
                ancho: 2560 - 432,
                alto: 1440 - 432,
                margen_inferior: 216,
            }
        );
    }

    #[test]
    fn tope_del_anidado() {
        // 1252×1376: 206 px abajo.
        assert_eq!(tope(1252, 1376).margen_inferior, 206);
    }

    #[test]
    fn el_tope_entra_en_la_salida() {
        for (w, h) in [
            (2560, 1440),
            (1920, 1080),
            (1080, 1920),
            (800, 600),
            (3840, 2160),
        ] {
            let t = tope(w, h);
            assert!(t.ancho > 0 && t.ancho < w, "{w}×{h}: {t:?}");
            assert!(
                t.alto > 0 && t.margen_inferior + t.alto < h,
                "{w}×{h}: {t:?}"
            );
        }
    }

    #[test]
    fn la_ventana_tiene_el_alto_de_la_tarjeta() {
        assert_eq!(f64::from(ALTO), dibujo::alto_ventana());
    }

    #[test]
    fn glifo_o_nombre() {
        assert!(es_glifo("\u{f0594}"));
        assert!(!es_glifo("osd-volume-high-symbolic"));
        assert!(!es_glifo("tv-symbolic"));
        assert!(!es_glifo(""));
    }

    #[test]
    fn salida_degenerada() {
        let t = tope(1, 1);
        assert!(t.ancho >= 1 && t.alto >= 1);
    }
}
