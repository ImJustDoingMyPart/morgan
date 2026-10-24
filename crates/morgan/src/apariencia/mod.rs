//! Los archivos de apps que pueden no estar (kitty, qt5ct/qt6ct, el env de uwsm) se tocan solo
//! si existen: no se crean.

mod archivos;
pub mod cursores;
pub mod fuentes;
pub mod iconos;

pub use fuentes::Ambito;

/// En un Hyprland anidado de pruebas su `~/.config` son symlinks a los de la sesión de
/// verdad: no se escribe nada real.
pub fn seco() -> bool {
    std::env::var_os("MORGAN_APARIENCIA_SECO").is_some()
}

#[derive(Debug, Clone, Default)]
pub struct Puesto {
    pub fuente_sistema: String,
    pub fuente_terminal: String,
    pub iconos: String,
    pub cursor: (String, u32),
}

pub fn puesto() -> Puesto {
    Puesto {
        fuente_sistema: fuentes::actual(Ambito::Sistema),
        fuente_terminal: fuentes::actual(Ambito::Terminal),
        iconos: iconos::actual(),
        cursor: cursores::actual(),
    }
}

#[derive(Debug, Clone, Default)]
pub struct Opciones {
    pub fuentes_sistema: Vec<(String, String)>,
    pub fuentes_terminal: Vec<(String, String)>,
    pub iconos: Vec<String>,
    pub cursores: Vec<String>,
}

pub fn opciones() -> Opciones {
    let fc = fuentes::fc_list();
    Opciones {
        fuentes_sistema: fuentes::candidatas(&fc, Ambito::Sistema),
        fuentes_terminal: fuentes::candidatas(&fc, Ambito::Terminal),
        iconos: iconos::listar(),
        cursores: cursores::listar(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cambio {
    Fuente(Ambito, String),
    Iconos(String),
    Cursor(String, u32),
}

pub async fn aplicar(c: Cambio) -> Result<(), String> {
    match c {
        Cambio::Fuente(a, f) => {
            let familia = f.clone();
            gio::spawn_blocking(move || fuentes::validar(a, &familia))
                .await
                .map_err(|_| "apariencia: el hilo se cayó".to_owned())??;
            fuentes::aplicar(a, &f);
            Ok(())
        }
        Cambio::Iconos(f) => iconos::aplicar(&f),
        Cambio::Cursor(t, n) => cursores::aplicar(&t, n).await,
    }
}

#[cfg(test)]
#[test]
#[ignore]
fn comparar_con_la_sesion() {
    let p = puesto();
    let o = opciones();
    println!("PUESTO\t{}\t{}\t{}\t{} {}", p.fuente_sistema, p.fuente_terminal, p.iconos, p.cursor.0, p.cursor.1);
    println!("CANTIDADES\t{}\t{}\t{}\t{}", o.fuentes_sistema.len(), o.fuentes_terminal.len(), o.iconos.len(), o.cursores.len());
    for (f, s) in o.fuentes_sistema.iter().chain(&o.fuentes_terminal) {
        println!("FUENTE\t{f}\t{s}");
    }
    for n in o.iconos.iter().chain(&o.cursores) {
        println!("NOMBRE\t{n}");
    }
}
