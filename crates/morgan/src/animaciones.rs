//! La velocidad es lo único que cambia cuánto tarda todo, en Hyprland y en el shell: el ritmo
//! escala los `ms` de la hoja y las duraciones de las animaciones propias.
//!
//! Todas son rápidas, y lo fija `toda_velocidad_bajo_los_topes`. Las salidas frenan en vez de
//! acelerar o rebotar: una curva que arranca lenta parece una demora antes de responder,
//! aunque termine antes.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

pub struct Composicion {
    pub id: &'static str,
    pub nombre: &'static str,
    pub descripcion: &'static str,
    pub ventanas: &'static str,
    /// Cerrar siempre frena.
    pub curva: &'static str,
    /// Siempre vertical (o sin viaje): con el layout `scrolling`, las columnas que no entran
    /// siguen a los costados del monitor, y un `slide` horizontal las hace cruzar la pantalla
    /// durante el cambio (Hyprland no recorta al borde).
    pub workspaces: &'static str,
    pub curva_workspace: &'static str,
    pub especial: &'static str,
}

/// Solo los estilos que trae Hyprland 0.56: sin shaders ni plugins.
pub const COMPOSICIONES: &[Composicion] = &[
    Composicion {
        id: "precise",
        nombre: "Precisa",
        descripcion: "Crece y se desliza",
        ventanas: "popin 85%",
        curva: DECEL,
        workspaces: "slidevert",
        curva_workspace: EXPO,
        especial: "slidevert",
    },
    Composicion {
        id: "elastic",
        nombre: "Elástica",
        descripcion: "Entra con un rebote",
        ventanas: "popin 80%",
        curva: REBOTE,
        workspaces: "slidevert",
        curva_workspace: REBOTE,
        especial: "slidevert",
    },
    Composicion {
        id: "fold",
        nombre: "Plegada",
        descripcion: "Se despliega y asoma",
        ventanas: "gnomed",
        curva: DECEL,
        workspaces: "slidefadevert 20%",
        curva_workspace: EXPO,
        especial: "slidefadevert 20%",
    },
    Composicion {
        id: "fade",
        nombre: "Fundido",
        descripcion: "Se funde con un leve viaje",
        ventanas: "popin 97%",
        curva: DECEL,
        // El workspace se funde y viaja apenas en vertical: la ventana que se lleva a otro
        // (`window.move`, que lo sigue) sube o baja con él.
        workspaces: "slidefadevert 20%",
        curva_workspace: DECEL,
        especial: "slidefadevert 20%",
    },
];

const DECEL: &str = "morgan_decel";
const EXPO: &str = "morgan_expo";
const REBOTE: &str = "morgan_rebote";

pub struct Velocidad {
    pub id: &'static str,
    pub nombre: &'static str,
    pub descripcion: &'static str,
    pub ritmo: f64,
}

pub const VELOCIDADES: &[Velocidad] = &[
    Velocidad {
        id: "fast",
        nombre: "Rápida",
        descripcion: "Abrir en 120 ms",
        ritmo: 0.6,
    },
    Velocidad {
        id: "medium",
        nombre: "Media",
        descripcion: "Abrir en 200 ms",
        ritmo: 1.0,
    },
    Velocidad {
        id: "calm",
        nombre: "Serena",
        descripcion: "Abrir en 240 ms",
        ritmo: 1.2,
    },
];

const BASE: Tiempos = Tiempos {
    entrar: 200,
    salir: 150,
    mover: 200,
    workspace: 250,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tiempos {
    pub entrar: u32,
    pub salir: u32,
    pub mover: u32,
    pub workspace: u32,
}

impl Velocidad {
    pub fn tiempos(&self) -> Tiempos {
        let k = |ms: u32| (f64::from(ms) * self.ritmo).round() as u32;
        Tiempos {
            entrar: k(BASE.entrar),
            salir: k(BASE.salir),
            mover: k(BASE.mover),
            workspace: k(BASE.workspace),
        }
    }
}

pub fn por_id(id: &str) -> &'static Composicion {
    COMPOSICIONES
        .iter()
        .find(|c| c.id == id)
        .unwrap_or(&COMPOSICIONES[0])
}

pub fn velocidad(id: &str) -> &'static Velocidad {
    VELOCIDADES
        .iter()
        .find(|v| v.id == id)
        .unwrap_or(&VELOCIDADES[1])
}

pub fn lua(c: &Composicion, v: &Velocidad) -> String {
    let t = v.tiempos();
    // Los fundidos acompañan al movimiento y terminan antes: un poco más cortos.
    let fundir = t.entrar * 3 / 4;
    let desvanecer = t.salir * 4 / 5;
    let mut s = format!(
        "-- Animación: {nombre} ({desc}), velocidad {vel} ({vdesc}).\n\
         hl.config({{ animations = {{ enabled = true }} }})\n\
         hl.curve({DECEL:?}, {{type = \"bezier\", points = {{{{0.05, 0.7}}, {{0.1, 1}}}}}})\n\
         hl.curve({EXPO:?}, {{type = \"bezier\", points = {{{{0.16, 1}}, {{0.3, 1}}}}}})\n\
         hl.curve({REBOTE:?}, {{type = \"bezier\", points = {{{{0.34, 1.3}}, {{0.64, 1}}}}}})\n\
         -- Al enfocar, el degradé del borde y el de la sombra dan una vuelta y frenan en sus\n\
         -- 45° (`once`; `loop` redibujaría el borde en cada cuadro aunque nada cambie).\n\
         hl.curve(\"borde_giro\", {{type = \"bezier\", points = {{{{0.33, 1}}, {{0.68, 1}}}}}})\n\n\
         -- Los padres primero: los hijos definidos después los pisan.\n",
        nombre = c.nombre,
        desc = c.descripcion,
        vel = v.nombre,
        vdesc = v.descripcion,
    );
    let mut hoja = |leaf: &str, ms: u32, curva: &str, estilo: Option<&str>| {
        let estilo = estilo
            .map(|e| format!(", style = {e:?}"))
            .unwrap_or_default();
        s.push_str(&format!(
            "hl.animation({{leaf = {leaf:?}, enabled = true, speed = {}, bezier = {curva:?}{estilo}}})\n",
            ds(ms)
        ));
    };
    hoja("global", t.mover, DECEL, None);
    hoja("windows", t.entrar, c.curva, Some(c.ventanas));
    hoja("windowsIn", t.entrar, c.curva, Some(c.ventanas));
    hoja("windowsOut", t.salir, DECEL, Some(c.ventanas));
    hoja("windowsMove", t.mover, c.curva, None);
    // El estilo de las capas lo pone cada regla; el shell anima lo suyo.
    hoja("layers", t.entrar, DECEL, None);
    hoja("layersIn", t.entrar, DECEL, None);
    hoja("layersOut", t.salir, DECEL, None);
    for (padre, hijos) in [
        (
            "fade",
            &["fadeSwitch", "fadeShadow", "fadeDim", "fadeDpms"][..],
        ),
        ("fadeLayers", &[]),
        ("fadePopups", &[]),
    ] {
        hoja(padre, fundir, DECEL, None);
        for h in hijos {
            hoja(h, fundir, DECEL, None);
        }
        hoja(&format!("{padre}In"), fundir, DECEL, None);
        hoja(&format!("{padre}Out"), desvanecer, DECEL, None);
    }
    hoja("border", t.mover, DECEL, None);
    for (padre, estilo) in [
        ("workspaces", c.workspaces),
        ("specialWorkspace", c.especial),
    ] {
        for sufijo in ["", "In", "Out"] {
            hoja(
                &format!("{padre}{sufijo}"),
                t.workspace,
                c.curva_workspace,
                Some(estilo),
            );
        }
    }
    // Hijos de `border`: van después para que no los arrastre. Igual en todas.
    for leaf in ["borderangle", "shadowangle"] {
        hoja(leaf, GIRO, "borde_giro", Some("once"));
    }
    s
}

/// Un adorno que se asienta, no algo que haga esperar: no sigue el ritmo.
const GIRO: u32 = 800;

fn ds(ms: u32) -> String {
    let s = format!("{:.2}", f64::from(ms) / 100.0);
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

static RITMO: AtomicU32 = AtomicU32::new(1000);

pub fn poner(id: &str) {
    RITMO.store(
        (velocidad(id).ritmo * 1000.0).round() as u32,
        Ordering::Relaxed,
    );
}

fn ritmo() -> f64 {
    f64::from(RITMO.load(Ordering::Relaxed)) / 1000.0
}

pub fn dur(base: Duration) -> Duration {
    base.mul_f64(ritmo())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lo más que puede tardar cada cosa a cualquier velocidad.
    const TOPE_ENTRAR: u32 = 250;
    const TOPE_SALIR: u32 = 180;
    const TOPE_WORKSPACE: u32 = 300;

    fn todas() -> impl Iterator<Item = (&'static Composicion, &'static Velocidad)> {
        COMPOSICIONES
            .iter()
            .flat_map(|c| VELOCIDADES.iter().map(move |v| (c, v)))
    }

    #[test]
    fn toda_velocidad_bajo_los_topes() {
        for v in VELOCIDADES {
            let t = v.tiempos();
            assert!(t.entrar <= TOPE_ENTRAR, "{}: entrar", v.id);
            assert!(t.salir <= TOPE_SALIR, "{}: salir", v.id);
            assert!(t.mover <= TOPE_ENTRAR, "{}: mover", v.id);
            assert!(t.workspace <= TOPE_WORKSPACE, "{}: workspace", v.id);
            // Salir nunca tarda más que entrar.
            assert!(t.salir <= t.entrar, "{}: salir > entrar", v.id);
        }
        // De la más rápida a la más lenta, en orden.
        assert!(VELOCIDADES.windows(2).all(|p| p[0].ritmo < p[1].ritmo));
        assert_eq!(velocidad("medium").tiempos(), BASE);
    }

    #[test]
    fn las_salidas_frenan() {
        // Una curva que arranca lenta (como md3_accel) hace esperar antes de responder, y un
        // rebote al cerrar hace volver lo que se iba.
        for (c, v) in todas() {
            let lua = lua(c, v);
            assert!(!lua.contains("accel"), "{}", c.id);
            for l in lua.lines().filter(|l| l.contains("Out\"")) {
                // El workspace que se va y el que llega se mueven juntos: misma curva.
                let rebota = c.curva_workspace == REBOTE && l.contains("orkspace");
                assert!(l.contains(DECEL) || l.contains(EXPO) || rebota, "{l}");
            }
        }
    }

    #[test]
    fn las_composiciones_difieren_en_estilo_no_en_tiempo() {
        let mut estilos: Vec<_> = COMPOSICIONES
            .iter()
            .map(|c| (c.ventanas, c.curva, c.workspaces))
            .collect();
        estilos.sort();
        estilos.dedup();
        assert_eq!(estilos.len(), COMPOSICIONES.len());
        // A una misma velocidad, todas tardan lo mismo en abrir.
        let v = velocidad("medium");
        for c in COMPOSICIONES {
            assert!(lua(c, v).contains("leaf = \"windowsIn\", enabled = true, speed = 2,"));
        }
    }

    #[test]
    fn los_ids_son_unicos() {
        let mut ids: Vec<_> = COMPOSICIONES.iter().map(|c| c.id).collect();
        ids.extend(VELOCIDADES.iter().map(|v| v.id));
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), COMPOSICIONES.len() + VELOCIDADES.len());
        // Uno que ya no existe cae en la primera; una velocidad, en Media.
        assert_eq!(por_id("x").id, COMPOSICIONES[0].id);
        assert_eq!(velocidad("x").id, "medium");
    }

    #[test]
    fn los_workspaces_nunca_se_deslizan_de_costado() {
        for (c, v) in todas() {
            for l in lua(c, v).lines().filter(|l| l.contains("orkspace")) {
                let estilo = l.split("style = \"").nth(1).unwrap_or("");
                let horizontal = estilo.starts_with("slide") && !estilo.contains("vert");
                assert!(!horizontal, "{}: {l}", c.id);
            }
        }
    }

    #[test]
    fn el_lua_lleva_cada_leaf() {
        let lua = lua(por_id("precise"), velocidad("medium"));
        for leaf in [
            "global",
            "windowsIn",
            "windowsOut",
            "windowsMove",
            "layersIn",
            "layersOut",
            "fadeIn",
            "fadeOut",
            "fadeLayersIn",
            "fadeLayersOut",
            "fadePopupsIn",
            "fadePopupsOut",
            "border",
            "workspacesIn",
            "workspacesOut",
            "specialWorkspaceOut",
            "borderangle",
            "shadowangle",
        ] {
            assert!(lua.contains(&format!("leaf = \"{leaf}\"")), "{leaf}");
        }
        assert!(lua.contains(
            "leaf = \"windowsOut\", enabled = true, speed = 1.5, bezier = \"morgan_decel\", style = \"popin 85%\""
        ));
        assert!(lua.contains("leaf = \"fadeLayersOut\", enabled = true, speed = 1.2,"));
        assert!(lua.contains("leaf = \"workspacesIn\", enabled = true, speed = 2.5, bezier = \"morgan_expo\", style = \"slidevert\""));
        assert!(lua.contains("animations = { enabled = true }"));
        // Rápida: todo a 0,6.
        let rapida = super::lua(por_id("precise"), velocidad("fast"));
        assert!(rapida.contains("leaf = \"windowsIn\", enabled = true, speed = 1.2,"));
        // El cambio de workspace también sigue la velocidad, en todas las composiciones.
        for c in COMPOSICIONES {
            let rapida = super::lua(c, velocidad("fast"));
            assert!(
                rapida.contains("leaf = \"workspacesIn\", enabled = true, speed = 1.5,"),
                "{}",
                c.id
            );
            let serena = super::lua(c, velocidad("calm"));
            assert!(
                serena.contains("leaf = \"workspacesIn\", enabled = true, speed = 3,"),
                "{}",
                c.id
            );
        }
    }

    #[test]
    fn decisegundos() {
        assert_eq!(ds(200), "2");
        assert_eq!(ds(150), "1.5");
        assert_eq!(ds(187), "1.87");
        assert_eq!(ds(90), "0.9");
    }
}
