//! Los colores del tema: los roles de Material que matugen escribe en `theme.css`, como RGBA
//! sin premultiplicar de 0 a 1.

/// RGBA sin premultiplicar, de 0 a 1.
pub type Color = [f32; 4];

pub const BLANCO: Color = [1.0, 1.0, 1.0, 1.0];

pub fn mezclar(a: &Color, b: &Color, t: f32) -> Color {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

pub fn con_alfa(c: &Color, a: f32) -> Color {
    [c[0], c[1], c[2], a.clamp(0.0, 1.0)]
}

pub fn pincel(c: &Color) -> vello::peniko::Color {
    vello::peniko::Color::new(*c)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paleta {
    pub primary: Color,
    pub tertiary: Color,
    pub on_surface: Color,
    pub on_surface_variant: Color,
    pub error: Color,
    pub surface: Color,
    pub surface_container: Color,
    pub surface_container_low: Color,
    pub outline_variant: Color,
}

fn hex(v: &str) -> Option<Color> {
    let h = v.strip_prefix('#')?;
    if h.len() != 6 && h.len() != 8 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    let a = if h.len() == 8 { byte(6)? } else { 255 };
    Some([byte(0)?, byte(2)?, byte(4)?, a].map(|b| b as f32 / 255.0))
}

impl Paleta {
    /// Los `@define-color` de `theme.css`; lo que falta o no se entiende, de fábrica.
    pub fn desde_css(css: &str) -> Self {
        let rol = |nombre: &str, defecto: &str| {
            css.lines()
                .filter_map(|l| l.trim().strip_prefix("@define-color "))
                .filter_map(|l| l.trim().split_once(char::is_whitespace))
                .find(|(n, _)| *n == nombre)
                .and_then(|(_, v)| hex(v.trim().trim_end_matches(';').trim()))
                .unwrap_or_else(|| hex(defecto).expect("color de fábrica"))
        };
        Paleta {
            primary: rol("primary", "#9fcafd"),
            tertiary: rol("tertiary", "#d6bee5"),
            on_surface: rol("on_surface", "#e1e2e8"),
            on_surface_variant: rol("on_surface_variant", "#c2c7cf"),
            error: rol("error", "#ffb4ab"),
            surface: rol("surface", "#101418"),
            surface_container: rol("surface_container", "#1d2024"),
            surface_container_low: rol("surface_container_low", "#191c20"),
            outline_variant: rol("outline_variant", "#42474e"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paleta_lee_los_roles_y_cae_en_los_de_fabrica() {
        let p =
            Paleta::desde_css("@define-color primary #ff0000;\n@define-color  error  #00ff00 ;\n");
        assert_eq!(p.primary, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(p.error, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(p.tertiary, hex("#d6bee5").unwrap());
        assert_eq!(hex("rgba(0,0,0,1)"), None);
    }
}
