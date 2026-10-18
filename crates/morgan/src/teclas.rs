//! Sin leer el teclado (eso pide root): un bind no consumidor de Hyprland corre
//! `morganctl capslock` al soltar la tecla, y morgan lee el estado de `j/devices`. Al soltarla
//! el estado ya cambió: xkb lo da vuelta al apretarla.

use crate::osd::Aviso;

const MS: u32 = 2000;

pub fn aviso(prendido: bool) -> Aviso {
    let texto = if prendido { "Activado" } else { "Desactivado" };
    Aviso::estado("󰘲", "Bloq Mayús", texto, prendido, MS)
}

/// El teclado que Hyprland marca `main`; si ninguno lo está, el primero.
pub fn bloq_mayus(devices: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(devices).ok()?;
    let teclados = v["keyboards"].as_array()?;
    let teclado = teclados
        .iter()
        .find(|k| k["main"].as_bool() == Some(true))
        .or_else(|| teclados.first())?;
    teclado["capsLock"].as_bool()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osd::Cuerpo;

    #[test]
    fn el_aviso() {
        let a = aviso(true);
        assert_eq!(a.icono.as_deref(), Some("󰘲"));
        assert_eq!(a.titulo, "Bloq Mayús");
        assert_eq!(
            a.cuerpo,
            Cuerpo::Estado {
                texto: "Activado".into(),
                encendido: true
            }
        );
        assert_eq!(
            aviso(false).cuerpo,
            Cuerpo::Estado {
                texto: "Desactivado".into(),
                encendido: false
            }
        );
    }

    #[test]
    fn del_teclado_principal() {
        let json = r#"{"mice": [], "keyboards": [
            {"name": "power-button", "capsLock": false, "main": false},
            {"name": "sinowealth-gaming-kb-", "capsLock": true, "main": true}
        ]}"#;
        assert_eq!(bloq_mayus(json), Some(true));
        let sin_main = r#"{"keyboards": [{"name": "a", "capsLock": false}]}"#;
        assert_eq!(bloq_mayus(sin_main), Some(false));
        assert_eq!(bloq_mayus(r#"{"keyboards": []}"#), None);
        assert_eq!(bloq_mayus("roto"), None);
    }
}
