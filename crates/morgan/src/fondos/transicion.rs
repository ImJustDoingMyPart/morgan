//! awww trae `--transition-type random`, pero sortea solo el tipo y deja los parámetros en el
//! default: la variedad de verdad sale de los parámetros. Los flags son los de `awww img
//! --help` de la 0.12.1.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receta {
    Fade,
    Wipe,
    Wave,
    Grow,
    Outer,
    Lado,
}

const RECETAS: [Receta; 6] = [
    Receta::Fade,
    Receta::Wipe,
    Receta::Wave,
    Receta::Grow,
    Receta::Outer,
    Receta::Lado,
];

/// El default de awww es 30, que en una barrida diagonal se nota escalonado.
const FPS: &str = "60";

/// Con 6 recetas, sortear a ciegas repetiría una de cada seis veces.
pub fn sortear(anterior: Option<Receta>, azar: &mut impl FnMut(u32) -> u32) -> (Receta, Vec<String>) {
    let candidatas: Vec<Receta> = RECETAS.into_iter().filter(|r| Some(*r) != anterior).collect();
    let receta = candidatas[azar(candidatas.len() as u32) as usize];
    let duracion = format!("1.{}", azar(10));
    let mut a: Vec<String> = Vec::new();
    let mut par = |k: &str, v: String| {
        a.push(k.to_owned());
        a.push(v);
    };
    match receta {
        // La variedad viene de la curva: la misma duración se siente distinta si la imagen
        // entra de golpe y frena o si acelera y desacelera parejo.
        Receta::Fade => {
            par("--transition-type", "fade".into());
            par(
                "--transition-bezier",
                uno_de(azar, &[".25,.1,.25,1", ".42,0,.58,1", ".16,1,.3,1", ".54,0,.34,.99", "0,0,1,1"]),
            );
        }
        // Múltiplos de 15°: un barrido a 7° se lee como un horizontal mal hecho.
        Receta::Wipe => {
            par("--transition-type", "wipe".into());
            par("--transition-angle", (azar(24) * 15).to_string());
        }
        // `--transition-wave` es ancho,alto de cada onda: chicos dan un filo de sierra fino y
        // grandes una ola gruesa.
        Receta::Wave => {
            par("--transition-type", "wave".into());
            par("--transition-angle", (azar(24) * 15).to_string());
            par(
                "--transition-wave",
                uno_de(azar, &["20,20", "35,25", "12,45", "60,20", "45,40", "15,15"]),
            );
        }
        // En flotante: awww lee los enteros como píxeles y los flotantes como fracción, lo
        // único que sirve igual en cualquier resolución.
        Receta::Grow | Receta::Outer => {
            let tipo = if receta == Receta::Grow { "grow" } else { "outer" };
            par("--transition-type", tipo.into());
            par(
                "--transition-pos",
                format!("0.{},0.{}", 10 + azar(81), 10 + azar(81)),
            );
        }
        Receta::Lado => par(
            "--transition-type",
            uno_de(azar, &["left", "right", "top", "bottom"]),
        ),
    }
    par("--transition-duration", duracion);
    par("--transition-fps", FPS.into());
    (receta, a)
}

fn uno_de(azar: &mut impl FnMut(u32) -> u32, o: &[&str]) -> String {
    o[azar(o.len() as u32) as usize].to_owned()
}

pub fn fija(tipo: &str) -> Vec<String> {
    ["--transition-type", tipo, "--transition-fps", FPS]
        .map(str::to_owned)
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nunca_repite_la_receta_anterior() {
        for anterior in RECETAS {
            for semilla in 0..12 {
                let mut azar = |n: u32| semilla % n;
                let (r, _) = sortear(Some(anterior), &mut azar);
                assert_ne!(r, anterior);
            }
        }
    }

    #[test]
    fn argumentos_en_pares_con_tipo_duracion_y_fps() {
        for semilla in 0..40 {
            let mut x: u32 = semilla;
            let mut azar = |n: u32| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (x >> 8) % n
            };
            let (_, a) = sortear(None, &mut azar);
            assert_eq!(a.len() % 2, 0);
            assert_eq!(a[0], "--transition-type");
            assert!(a.ends_with(&["--transition-fps".into(), "60".into()]));
            let d = a.iter().position(|s| s == "--transition-duration").unwrap();
            assert!(a[d + 1].starts_with("1."));
            if let Some(p) = a.iter().position(|s| s == "--transition-pos") {
                let (x, y) = a[p + 1].split_once(',').unwrap();
                for v in [x, y] {
                    let v: f64 = v.parse().unwrap();
                    assert!((0.1..=0.9).contains(&v), "{v}");
                }
            }
        }
    }
}
