//! Los binds de la config del usuario se leen nada más: su acción es Lua suyo.

use super::arbol::{self, Cambio, Categoria, Control, Fila, Que, Seccion, ajuste, opcion};
use crate::config::Atajos;
use crate::launcher::binds;
use crate::launcher::fuentes::Atajo;

/// Lo que no cae en ninguna sección va a la última.
const GRUPOS: &[(&str, &[&str])] = &[
    ("Menús y avisos", &["menú", "lanzador", "notificac", "bloqueo"]),
    ("Aplicaciones", &["aplicac"]),
    ("Ventanas y workspaces", &["ventana", "layout", "workspace", "foco"]),
    ("Multimedia y brillo", &["multimedia", "brillo", "volumen"]),
    ("Capturas y grabación", &["captura", "grabaci"]),
    ("Sesión y otros", &[]),
];

pub fn secciones() -> &'static [&'static str] {
    const T: [&str; 6] = [
        GRUPOS[0].0,
        GRUPOS[1].0,
        GRUPOS[2].0,
        GRUPOS[3].0,
        GRUPOS[4].0,
        GRUPOS[5].0,
    ];
    &T
}

fn grupo_de(cabecera: &str) -> usize {
    let c = cabecera.to_lowercase();
    GRUPOS
        .iter()
        .position(|(_, claves)| claves.iter().any(|k| c.contains(k)))
        .unwrap_or(GRUPOS.len() - 1)
}

/// Una descripción armada en Lua (`desc = "Workspace " .. i`) vale como prefijo.
pub fn grupos_de_binds_lua(lua: &str) -> Vec<(String, bool, usize)> {
    let mut grupo = GRUPOS.len() - 1;
    let mut v = Vec::new();
    for l in lua.lines() {
        if let Some(c) = l.trim_start().strip_prefix("-- ─── ") {
            grupo = grupo_de(c.trim_end_matches(['─', ' ']));
            continue;
        }
        let Some((_, resto)) = l.split_once("desc = \"") else {
            continue;
        };
        let Some((desc, despues)) = resto.split_once('"') else {
            continue;
        };
        let prefijo = despues.trim_start().starts_with("..");
        v.push((desc.to_owned(), prefijo, grupo));
    }
    v
}

fn grupo_de_desc(desc: &str, grupos: &[(String, bool, usize)]) -> usize {
    grupos
        .iter()
        .find(|(d, prefijo, _)| if *prefijo { desc.starts_with(d.as_str()) } else { d == desc })
        .map_or(GRUPOS.len() - 1, |(_, _, g)| *g)
}

fn juntar_series(g: Vec<(String, Vec<String>)>) -> Vec<(String, Vec<String>)> {
    fn partir(d: &str) -> Option<(&str, u32)> {
        let corte = d.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        Some((&d[..corte], d[corte..].parse().ok()?))
    }
    let mut v: Vec<(String, Vec<String>)> = Vec::new();
    let mut i = 0;
    while i < g.len() {
        // Cuántas siguen la serie de la i: mismo prefijo, de a uno, y entre medio filas de
        // otras series que corren a la par (el bucle arma tres binds por número).
        let Some((prefijo, n0)) = partir(&g[i].0) else {
            v.push(g[i].clone());
            i += 1;
            continue;
        };
        let miembros: Vec<usize> = (i..g.len())
            .filter(|&j| partir(&g[j].0).is_some_and(|(p, _)| p == prefijo))
            .collect();
        let seguidos = miembros
            .iter()
            .enumerate()
            .take_while(|(k, j)| partir(&g[**j].0).is_some_and(|(_, n)| n == n0 + *k as u32))
            .count();
        if seguidos < 3 {
            v.push(g[i].clone());
            i += 1;
            continue;
        }
        let ultima = &g[miembros[seguidos - 1]];
        let n1 = n0 + seguidos as u32 - 1;
        let teclas = vec![format!("{} … {}", g[i].1[0], ultima.1[0])];
        v.push((format!("{prefijo}{n0} a {n1}"), teclas));
        // Los de esta serie salen; lo demás queda en su lugar.
        let fuera: Vec<usize> = miembros[..seguidos].to_vec();
        let resto: Vec<(String, Vec<String>)> = g[i + 1..]
            .iter()
            .enumerate()
            .filter(|(k, _)| !fuera.contains(&(i + 1 + k)))
            .map(|(_, x)| x.clone())
            .collect();
        return [v, juntar_series(resto)].concat();
    }
    v
}

fn fila_propia(p: &binds::Propio, defecto: &Atajos) -> Fila {
    let quitar = |tecla: &str| Que::Config(Cambio::Teclas(vec![(p.id.to_owned(), tecla.to_owned())]));
    let mut opciones = vec![opcion("Cambiar la tecla…", "", Que::GrabarTecla(p.id.to_owned()))];
    if !p.tecla.is_empty() {
        opciones.push(opcion("Sin tecla", "", quitar("")));
    }
    let siempre = defecto.tecla(p.id).unwrap_or_default();
    if !siempre.is_empty() && binds::normalizar(&siempre) != binds::normalizar(&p.tecla) {
        opciones.push(opcion(&format!("La de siempre: {siempre}"), "", quitar(&siempre)));
    }
    let sub = if p.tecla.is_empty() { "Sin tecla" } else { &p.tecla };
    ajuste(p.desc, sub, Control::Lista(opciones))
}

/// Los binds de morgan van también sin tecla: no están en `j/binds`.
pub fn pagina(atajos: &[Atajo], grupos: &[(String, bool, usize)], c: &Atajos) -> Vec<Seccion> {
    let propios = binds::propios(c);
    let defecto = Atajos::default();
    let mut filas: Vec<Vec<(String, Vec<String>)>> = vec![Vec::new(); GRUPOS.len()];
    for a in atajos {
        if propios.iter().any(|p| p.desc == a.descripcion) {
            continue;
        }
        let g = &mut filas[grupo_de_desc(&a.descripcion, grupos)];
        match g.iter_mut().find(|(d, _)| *d == a.descripcion) {
            Some((_, teclas)) => teclas.push(a.teclas.clone()),
            None => g.push((a.descripcion.clone(), vec![a.teclas.clone()])),
        }
    }
    let filas = filas
        .into_iter()
        .enumerate()
        .map(|(n, g)| {
            let mut v: Vec<Fila> = propios
                .iter()
                .filter(|p| grupo_de(p.cabecera) == n)
                .map(|p| fila_propia(p, &defecto))
                .collect();
            if GRUPOS[n].1.contains(&"multimedia") {
                v.push(ajuste(
                    "Teclas multimedia de morgan",
                    "Volumen, brillo y reproductor con su OSD, y el aviso de Bloq Mayús",
                    Control::Casilla {
                        activa: c.multimedia,
                        que: Que::Config(Cambio::Multimedia(!c.multimedia)),
                    },
                ));
            }
            v.extend(juntar_series(g).into_iter().map(|(d, teclas)| Fila::Ajuste {
                titulo: d,
                sub: teclas.join("  ·  "),
                control: Control::Nada,
                nota: String::new(),
            }));
            if v.is_empty() {
                v.push(ajuste("Ninguno", "", Control::Nada));
            }
            v
        })
        .collect();
    arbol::armar(Categoria::Atajos, filas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::arbol::Opcion;

    const LUA: &str = r#"
-- ─── Menús y Lanzadores (morgan) ───
bind(mod .. " + space", hl.dsp.exec_cmd("morganctl launcher toggle"), { desc = "Lanzador de prueba" })
-- ─── Foco y Movimiento de Ventanas ───
bind(mod .. " + H", hl.dsp.focus({ direction = "l" }), { desc = "Foco izquierda" })
bind(mod .. " + Left", hl.dsp.focus({ direction = "l" }), { desc = "Foco izquierda" })
for i = 1, 9 do
    bind(mod .. " + " .. i, hl.dsp.focus({ workspace = i }), { desc = "Workspace " .. i })
end
-- ─── Algo Nuevo ───
bind(mod .. " + Z", x, { desc = "Zeta" })
"#;

    fn atajo(d: &str, t: &str) -> Atajo {
        Atajo {
            descripcion: d.into(),
            teclas: t.into(),
            referencia: None,
        }
    }

    fn solo_lectura(f: &Fila) -> bool {
        matches!(f, Fila::Ajuste { control: Control::Nada, .. })
    }

    #[test]
    fn agrupa_por_la_cabecera_y_junta_las_combinaciones() {
        let g = grupos_de_binds_lua(LUA);
        let p = pagina(
            &[
                atajo("Lanzador de prueba", "SUPER + space"),
                // Uno de morgan: su fila es la editable, no la de `j/binds`.
                atajo("Launcher", "SUPER + space"),
                atajo("Foco izquierda", "SUPER + H"),
                atajo("Foco izquierda", "SUPER + Left"),
                atajo("Workspace 3", "SUPER + 3"),
                atajo("Zeta", "SUPER + Z"),
                atajo("Sin cabecera", "SUPER + Q"),
            ],
            &g,
            &Atajos::default(),
        );
        let titulos: Vec<_> = p.iter().map(|s| s.titulo).collect();
        assert_eq!(titulos, secciones());
        let filas = |s: usize| -> Vec<(String, String)> {
            p[s].filas
                .iter()
                .filter(|f| solo_lectura(f))
                .map(|f| match f {
                    Fila::Ajuste { titulo, sub, .. } => (titulo.clone(), sub.clone()),
                    otra => panic!("{otra:?}"),
                })
                .collect()
        };
        assert_eq!(filas(0), [("Lanzador de prueba".into(), "SUPER + space".into())]);
        assert_eq!(
            filas(2),
            [
                ("Foco izquierda".into(), "SUPER + H  ·  SUPER + Left".into()),
                ("Workspace 3".into(), "SUPER + 3".into()),
            ]
        );
        // Las series de un bucle, en una fila cada una, en el lugar de su primera.
        let serie = pagina(
            &(1..=4)
                .flat_map(|n| {
                    [
                        atajo(&format!("Workspace {n}"), &format!("SUPER + {n}")),
                        atajo(&format!("Mover a workspace {n}"), &format!("SUPER + CTRL + {n}")),
                    ]
                })
                .chain([atajo("Workspace de juegos", "SUPER + 0")])
                .collect::<Vec<_>>(),
            &g,
            &Atajos::default(),
        );
        let titulos = |s: usize| -> Vec<String> {
            serie[s]
                .filas
                .iter()
                .filter(|f| solo_lectura(f))
                .map(|f| match f {
                    Fila::Ajuste { titulo, sub, .. } => format!("{titulo}: {sub}"),
                    otra => panic!("{otra:?}"),
                })
                .collect()
        };
        assert_eq!(
            titulos(2),
            [
                "Workspace 1 a 4: SUPER + 1 … SUPER + 4",
                "Workspace de juegos: SUPER + 0",
            ]
        );
        // "Mover a workspace" no figura en el Lua de prueba: va a la última, igual junta.
        assert_eq!(
            titulos(5),
            ["Mover a workspace 1 a 4: SUPER + CTRL + 1 … SUPER + CTRL + 4"]
        );
        // Una cabecera desconocida y lo que no figura en binds.lua, a la última.
        assert_eq!(filas(5).len(), 2);
        // Un grupo sin atajos no queda vacío.
        assert_eq!(filas(1)[0].0, "Ninguno");
    }

    #[test]
    fn los_de_morgan_se_editan_y_figuran_sin_tecla() {
        let mut c = Atajos {
            launcher: "SUPER + K".into(),
            ..Atajos::default()
        };
        c.multimedia = false;
        let p = pagina(&[atajo("Launcher", "SUPER + K")], &[], &c);
        let fila = |s: usize, t: &str| -> (String, Vec<Opcion>) {
            p[s].filas
                .iter()
                .find_map(|f| match f {
                    Fila::Ajuste { titulo, sub, control: Control::Lista(o), .. } if titulo == t => {
                        Some((sub.clone(), o.clone()))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("falta {t}"))
        };
        let titulos = |o: &[Opcion]| o.iter().map(|o| o.titulo.clone()).collect::<Vec<_>>();
        // Una sola fila de Launcher, la editable, con la vuelta a la de siempre.
        let (sub, o) = fila(0, "Launcher");
        assert_eq!(sub, "SUPER + K");
        assert_eq!(titulos(&o), ["Cambiar la tecla…", "Sin tecla", "La de siempre: SUPER + space"]);
        assert_eq!(o[0].que, Que::GrabarTecla("launcher".into()));
        assert_eq!(
            o[2].que,
            Que::Config(Cambio::Teclas(vec![("launcher".into(), "SUPER + space".into())]))
        );
        // Sin tecla por defecto: solo grabar una.
        let (sub, o) = fila(0, "Atajos de teclado");
        assert_eq!(sub, "Sin tecla");
        assert_eq!(titulos(&o), ["Cambiar la tecla…"]);
        // El lienzo, con las ventanas; la casilla de las multimedia, con las suyas.
        let (_, o) = fila(2, "Lienzo de ventanas");
        assert_eq!(titulos(&o), ["Cambiar la tecla…", "Sin tecla"]);
        assert!(p[3].filas.iter().any(|f| matches!(f, Fila::Ajuste {
            control: Control::Casilla { activa: false, que: Que::Config(Cambio::Multimedia(true)) },
            ..
        })));
        assert_eq!(p[0].filas.iter().filter(|f| matches!(f, Fila::Ajuste { titulo, .. } if titulo == "Launcher")).count(), 1);
    }
}
