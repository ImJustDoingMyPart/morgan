
use super::arbol::{Cambio, Control, Fila, Opcion, Que, Seccion, ajuste, opcion};
use crate::config::Capturas;

/// La primera es la que manda al leer la puesta; elegir una app la pone para todos los tipos
/// del grupo.
pub const PREDETERMINADAS: &[(&str, &[&str])] = &[
    (
        "Navegador",
        &[
            "x-scheme-handler/https",
            "x-scheme-handler/http",
            "text/html",
        ],
    ),
    ("Archivos", &["inode/directory"]),
    ("Editor de texto", &["text/plain"]),
    (
        "Imágenes",
        &["image/png", "image/jpeg", "image/webp", "image/gif"],
    ),
    ("PDF", &["application/pdf"]),
    ("Video", &["video/mp4", "video/x-matroska", "video/webm"]),
    ("Música", &["audio/mpeg", "audio/flac", "audio/ogg"]),
    ("Correo", &["x-scheme-handler/mailto"]),
];

#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub id: String,
    pub nombre: String,
}

#[derive(Debug, Clone, Default)]
pub struct Predeterminada {
    pub candidatas: Vec<App>,
    pub puesta: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Autoarranque {
    /// El del usuario pisa al del sistema.
    pub archivo: String,
    pub nombre: String,
    pub comentario: String,
    pub activo: bool,
}

pub fn aplicaciones(
    pred: &[Predeterminada],
    auto: &[Autoarranque],
    capturas: &Capturas,
    imagenes: &str,
) -> Vec<Seccion> {
    let predeterminadas = PREDETERMINADAS
        .iter()
        .zip(pred)
        .map(|((titulo, mimes), p)| {
            let puesta = p
                .puesta
                .as_ref()
                .and_then(|id| p.candidatas.iter().find(|a| &a.id == id))
                .map_or_else(
                    || p.puesta.clone().unwrap_or_else(|| "ninguna".into()),
                    |a| a.nombre.clone(),
                );
            let opciones: Vec<Opcion> = p
                .candidatas
                .iter()
                .map(|a| {
                    opcion(
                        &a.nombre,
                        "",
                        Que::Predeterminada(
                            a.id.clone(),
                            mimes.iter().map(|m| (*m).to_owned()).collect(),
                        ),
                    )
                    .activa(p.puesta.as_ref() == Some(&a.id))
                })
                .collect();
            let control = if opciones.is_empty() {
                Control::Nada
            } else {
                Control::Lista(opciones)
            };
            ajuste(titulo, &puesta, control)
        })
        .collect();
    let autoarranque: Vec<Fila> = if auto.is_empty() {
        vec![ajuste(
            "Nada arranca solo",
            "no hay archivos en ~/.config/autostart ni en /etc/xdg/autostart",
            Control::Nada,
        )]
    } else {
        auto.iter()
            .map(|a| {
                ajuste(
                    &a.nombre,
                    &a.comentario,
                    Control::Casilla {
                        activa: a.activo,
                        que: Que::Autoarranque(a.archivo.clone(), !a.activo),
                    },
                )
            })
            .collect()
    };
    vec![
        Seccion {
            titulo: "Predeterminadas",
            filas: predeterminadas,
        },
        Seccion {
            titulo: "Autoarranque",
            filas: autoarranque,
        },
        Seccion {
            titulo: "Capturas de pantalla",
            filas: filas_capturas(capturas, imagenes),
        },
    ]
}

fn filas_capturas(c: &Capturas, imagenes: &str) -> Vec<Fila> {
    let con = |f: &dyn Fn(&mut Capturas)| {
        let mut n = c.clone();
        f(&mut n);
        Que::Config(Cambio::Capturas(n))
    };
    let destinos = [
        ("Imágenes › Screenshots", !c.portapapeles && c.carpeta.is_empty(), con(&|n| {
            n.portapapeles = false;
            n.carpeta = String::new();
        })),
        ("Imágenes", !c.portapapeles && c.carpeta == imagenes, con(&|n| {
            n.portapapeles = false;
            n.carpeta = imagenes.to_owned();
        })),
        ("Solo al portapapeles, sin archivo", c.portapapeles, con(&|n| n.portapapeles = true)),
    ];
    let puesto = destinos
        .iter()
        .find(|(_, si, _)| *si)
        .map_or(c.carpeta.as_str(), |(t, _, _)| t);
    vec![
        ajuste(
            "Congelar la pantalla al elegir una región",
            "la región se marca sobre una imagen quieta",
            Control::Casilla {
                activa: c.congelar,
                que: con(&|n| n.congelar = !n.congelar),
            },
        ),
        ajuste(
            "Dónde van",
            &format!("{puesto} · siempre también al portapapeles"),
            Control::Lista(
                destinos
                    .into_iter()
                    .map(|(t, si, que)| opcion(t, "", que).activa(si))
                    .collect(),
            ),
        ),
    ]
}

pub fn equipo(datos: &[(&'static str, String)]) -> Vec<Fila> {
    datos
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(t, v)| ajuste(t, v, Control::Nada))
        .collect()
}

pub const COLUMNAS_SNAPPER: &str = "number,date,description,pre-number";
const SNAPSHOTS: usize = 15;

pub fn hay_snapper() -> bool {
    static HAY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *HAY.get_or_init(|| gtk::glib::find_program_in_path("snapper").is_some())
}

/// `None` si snapper no los dio: solo los lista para quien figura en `ALLOW_USERS`.
pub fn snapshots(csv: Option<&str>) -> Vec<Fila> {
    let Some(csv) = csv else {
        return vec![ajuste(
            "Sin permiso para listarlos",
            "snapper los muestra a quien figura en ALLOW_USERS de /etc/snapper/configs/root",
            Control::Nada,
        )];
    };
    let mut v: Vec<(u64, Fila)> = csv
        .lines()
        .filter_map(|l| {
            let c = campos_csv(l);
            let n: u64 = c.first()?.trim_end_matches(['-', '+', '*']).parse().ok()?;
            // El 0 es el sistema en uso, no un snapshot.
            if n == 0 {
                return None;
            }
            let fecha = c.get(1).map_or("", |s| s.as_str());
            let desc = c.get(2).map_or("", |s| s.as_str());
            let despues = c.get(3).is_some_and(|p| !p.is_empty());
            let titulo = match (desc.is_empty(), despues) {
                (true, _) => "Sin descripción".to_owned(),
                (false, true) => format!("{desc} · después"),
                (false, false) => desc.to_owned(),
            };
            Some((n, ajuste(&titulo, &format!("nº {n} · {fecha}"), Control::Nada)))
        })
        .collect();
    if v.is_empty() {
        return vec![ajuste("Ninguno", "snapper no tiene snapshots", Control::Nada)];
    }
    v.sort_by_key(|x| std::cmp::Reverse(x.0));
    v.into_iter().take(SNAPSHOTS).map(|(_, f)| f).collect()
}

/// Los campos de un renglón CSV (RFC 4180: entre comillas puede haber comas, y `""` es una
/// comilla).
fn campos_csv(l: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut campo = String::new();
    let mut comillas = false;
    let mut c = l.chars().peekable();
    while let Some(x) = c.next() {
        match (x, comillas) {
            ('"', true) if c.peek() == Some(&'"') => {
                campo.push('"');
                c.next();
            }
            ('"', _) => comillas = !comillas,
            (',', false) => v.push(std::mem::take(&mut campo)),
            (x, _) => campo.push(x),
        }
    }
    v.push(campo);
    v
}

pub fn procesador(cpuinfo: &str) -> String {
    let nucleos = cpuinfo
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count();
    let modelo = cpuinfo
        .lines()
        .find_map(|l| l.strip_prefix("model name")?.split_once(':'))
        .map(|(_, m)| m.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    match nucleos {
        0 => modelo,
        n => format!("{modelo} · {n} hilos"),
    }
}

pub fn memoria(meminfo: &str) -> String {
    meminfo
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))
        .and_then(|v| v.split_whitespace().next()?.parse::<f64>().ok())
        .map(|kib| format!("{:.1} GiB", kib / 1024.0 / 1024.0).replace('.', ","))
        .unwrap_or_default()
}

pub fn distribucion(os_release: &str) -> String {
    os_release
        .lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_owned())
        .unwrap_or_default()
}

pub fn graficas(lspci: &str) -> String {
    lspci
        .lines()
        .filter(|l| l.contains("\"VGA compatible controller\"") || l.contains("\"3D controller\""))
        .filter_map(|l| {
            let campos: Vec<&str> = l.split('"').collect();
            // 0: slot, 1: clase, 3: fabricante, 5: modelo.
            Some(format!("{} {}", campos.get(3)?, campos.get(5)?))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

pub fn encendida(uptime: &str) -> String {
    let Some(s) = uptime
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<f64>().ok())
    else {
        return String::new();
    };
    let m = (s / 60.0) as u64;
    let (d, h, m) = (m / 1440, m / 60 % 24, m % 60);
    match (d, h) {
        (0, 0) => format!("{m} min"),
        (0, h) => format!("{h} h {m} min"),
        (d, h) => format!("{d} d {h} h"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predeterminadas_marca_la_puesta() {
        let pred = vec![Predeterminada {
            candidatas: vec![
                App {
                    id: "firefox.desktop".into(),
                    nombre: "Firefox".into(),
                },
                App {
                    id: "brave.desktop".into(),
                    nombre: "Brave".into(),
                },
            ],
            puesta: Some("brave.desktop".into()),
        }];
        let p = aplicaciones(&pred, &[], &Capturas::default(), "/home/r/Imágenes");
        let Fila::Ajuste {
            sub,
            control: Control::Lista(v),
            ..
        } = &p[0].filas[0]
        else {
            panic!()
        };
        assert_eq!(sub, "Brave");
        assert!(!v[0].activa && v[1].activa);
        let Que::Predeterminada(id, mimes) = &v[0].que else {
            panic!()
        };
        assert_eq!(id, "firefox.desktop");
        assert_eq!(mimes.len(), 3);
        // Sin datos para los otros grupos, solo aparece el que se leyó.
        assert_eq!(p[0].filas.len(), 1);
        assert!(matches!(
            &p[1].filas[0],
            Fila::Ajuste {
                control: Control::Nada,
                ..
            }
        ));
    }

    #[test]
    fn autoarranque_alterna() {
        let a = Autoarranque {
            archivo: "x.desktop".into(),
            nombre: "X".into(),
            comentario: String::new(),
            activo: true,
        };
        let p = aplicaciones(&[], &[a], &Capturas::default(), "/home/r/Imágenes");
        let Fila::Ajuste {
            control: Control::Casilla { activa, que },
            ..
        } = &p[1].filas[0]
        else {
            panic!()
        };
        assert!(activa);
        assert_eq!(que, &Que::Autoarranque("x.desktop".into(), false));
    }

    #[test]
    fn lee_el_equipo() {
        let cpu = "processor\t: 0\nmodel name\t: AMD Ryzen 7   7800X3D 8-Core Processor\nprocessor\t: 1\nmodel name\t: AMD Ryzen 7   7800X3D 8-Core Processor\n";
        assert_eq!(
            procesador(cpu),
            "AMD Ryzen 7 7800X3D 8-Core Processor · 2 hilos"
        );
        assert_eq!(memoria("MemTotal:       32768000 kB\n"), "31,2 GiB");
        assert_eq!(
            distribucion("NAME=\"CachyOS\"\nPRETTY_NAME=\"CachyOS Linux\"\n"),
            "CachyOS Linux"
        );
        let lspci = "01:00.0 \"VGA compatible controller\" \"NVIDIA Corporation\" \"AD104 [GeForce RTX 4070]\" -ra1 \"\" \"\"\n00:14.0 \"USB controller\" \"Intel\" \"x\"\n";
        assert_eq!(
            graficas(lspci),
            "NVIDIA Corporation AD104 [GeForce RTX 4070]"
        );
        assert_eq!(encendida("185.2 1.0"), "3 min");
        assert_eq!(encendida("7500 1"), "2 h 5 min");
        assert_eq!(encendida("200000 1"), "2 d 7 h");
        assert_eq!(equipo(&[("Kernel", "7.2".into()), ("Vacío", String::new())]).len(), 1);
    }

    #[test]
    fn capturas_marca_y_cambia() {
        let img = "/home/r/Imágenes";
        let mut c = Capturas::default();
        let destinos = |c: &Capturas| -> Vec<Opcion> {
            match &filas_capturas(c, img)[1] {
                Fila::Ajuste {
                    control: Control::Lista(v),
                    ..
                } => v.clone(),
                otra => panic!("{otra:?}"),
            }
        };
        let v = destinos(&c);
        assert!(v[0].activa && !v[1].activa && !v[2].activa);
        let Que::Config(Cambio::Capturas(n)) = &v[1].que else { panic!() };
        assert_eq!(n.carpeta, img);
        let Que::Config(Cambio::Capturas(n)) = &v[2].que else { panic!() };
        assert!(n.portapapeles);
        c = n.clone();
        assert!(destinos(&c)[2].activa);
        // La casilla de congelar no toca el destino.
        let Fila::Ajuste {
            control: Control::Casilla { activa, que: Que::Config(Cambio::Capturas(n)) },
            ..
        } = &filas_capturas(&c, img)[0]
        else {
            panic!()
        };
        assert!(*activa && !n.congelar && n.portapapeles);
    }

    #[test]
    fn snapshots_del_mas_nuevo_al_mas_viejo() {
        let csv = "0,,current,\n12,2026-10-07 21:40:01,\"pacman -S foo, bar\",11\n11,2026-10-07 21:39:58,\"pacman -S foo, bar\",\n13+,2026-10-08 15:21:00,,\n";
        let f: Vec<(String, String)> = snapshots(Some(csv))
            .into_iter()
            .map(|f| match f {
                Fila::Ajuste { titulo, sub, .. } => (titulo, sub),
                otra => panic!("{otra:?}"),
            })
            .collect();
        assert_eq!(
            f,
            [
                ("Sin descripción".into(), "nº 13 · 2026-10-08 15:21:00".into()),
                ("pacman -S foo, bar · después".into(), "nº 12 · 2026-10-07 21:40:01".into()),
                ("pacman -S foo, bar".into(), "nº 11 · 2026-10-07 21:39:58".into()),
            ]
        );
        assert_eq!(campos_csv(r#"1,"di ""hola""","#), ["1", r#"di "hola""#, ""]);
        assert!(matches!(&snapshots(None)[0], Fila::Ajuste { titulo, .. } if titulo.starts_with("Sin permiso")));
    }
}
