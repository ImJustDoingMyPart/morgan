//! Vive en el crate compartido porque la escribe el menú de morgan y la lee `morgan-lock`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bloqueo {
    #[serde(rename = "typing")]
    pub escritura: Escritura,
    #[serde(rename = "spark")]
    pub chispa: bool,
    #[serde(rename = "verifying")]
    pub verificar: Verificar,
    #[serde(rename = "shake")]
    pub sacudida: bool,
    #[serde(rename = "red_glow")]
    pub rojo: bool,
    #[serde(rename = "reaction")]
    pub reaccion: bool,
    #[serde(rename = "flash")]
    pub destello: bool,
}

impl Default for Bloqueo {
    fn default() -> Self {
        Bloqueo {
            escritura: Escritura::Onda,
            chispa: true,
            verificar: Verificar::Ambos,
            sacudida: true,
            rojo: true,
            reaccion: true,
            destello: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Escritura {
    #[serde(rename = "wave")]
    Onda,
    #[serde(rename = "dots")]
    Puntos,
    #[serde(rename = "symbols")]
    Simbolos,
    #[serde(rename = "fixed")]
    Fijo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Verificar {
    #[serde(rename = "ring")]
    Anillo,
    #[serde(rename = "pulse")]
    Latido,
    #[serde(rename = "both")]
    Ambos,
    #[serde(rename = "border")]
    Borde,
}

impl Verificar {
    pub fn late(self) -> bool {
        matches!(self, Verificar::Latido | Verificar::Ambos)
    }

    pub fn gira(self) -> bool {
        matches!(self, Verificar::Anillo | Verificar::Ambos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_valores_se_escriben_en_ingles() {
        let j = serde_json::to_string(&Escritura::Simbolos).unwrap();
        assert_eq!(j, "\"symbols\"");
        let v: Verificar = serde_json::from_str("\"border\"").unwrap();
        assert_eq!(v, Verificar::Borde);
    }

    #[test]
    fn lo_que_falta_es_default_y_lo_desconocido_es_error() {
        let b: Bloqueo = serde_json::from_str(r#"{"spark": false}"#).unwrap();
        assert_eq!(
            b,
            Bloqueo {
                chispa: false,
                ..Bloqueo::default()
            }
        );
        assert!(serde_json::from_str::<Bloqueo>(r#"{"sparks": false}"#).is_err());
    }
}
