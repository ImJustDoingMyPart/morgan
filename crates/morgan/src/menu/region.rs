//! Idioma y formatos de la sesión del usuario, para morgan y las apps.
//!
//! Lo elegido va a `~/.config/locale.conf`, el archivo por usuario que leen el perfil de
//! las shells de login (`/etc/profile.d/locale.sh` y `__fish_set_locale` de fish; greetd
//! corre la sesión a través de ellas) en lugar de `/etc/locale.conf`. Para los servicios de
//! `systemctl --user`, que no pasan por un login, `environment.d` lleva un enlace al mismo
//! archivo. Vale al volver a entrar a la sesión: cada programa lee el idioma al arrancar.
//!
//! El archivo va siempre completo (`LANG`, `LC_MESSAGES` y los nueve de formatos): fish y
//! systemd mezclan lo que falta con lo del sistema, y `/etc/locale.conf` suele poner cada
//! `LC_*` a mano. Solo se ofrecen los idiomas generados (`locale -a`); sumar otro es de root
//! (`locale-gen`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::arbol::{Control, Fila, Que, ajuste, opcion};

/// Los `LC_*` de formatos; los de idioma son `LANG` y `LC_MESSAGES`.
const FORMATOS: [&str; 9] = [
    "LC_NUMERIC",
    "LC_TIME",
    "LC_MONETARY",
    "LC_PAPER",
    "LC_NAME",
    "LC_ADDRESS",
    "LC_TELEPHONE",
    "LC_MEASUREMENT",
    "LC_IDENTIFICATION",
];
const IDIOMA: [&str; 2] = ["LANG", "LC_MESSAGES"];
const ENLACE: &str = "90-morgan-locale.conf";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Locale {
    /// Como va en `locale.conf`: `es_AR.UTF-8`.
    pub id: String,
    /// En su propio idioma: «Español (Argentina)».
    pub nombre: String,
    /// La fecha de hoy y un número, como los escribe.
    pub muestra: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lectura {
    pub disponibles: Vec<Locale>,
    /// `LANG` de `/etc/locale.conf`.
    pub sistema: String,
    /// Lo del archivo del usuario; `None`, lo del sistema.
    pub idioma: Option<String>,
    /// `None`: los del idioma.
    pub formatos: Option<String>,
    /// Con qué corre esta sesión (`LANG` y `LC_TIME`), para avisar que falta volver a entrar.
    pub sesion: (String, String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cambio {
    Idioma(Option<String>),
    Formatos(Option<String>),
}

fn ruta() -> PathBuf {
    glib::user_config_dir().join("locale.conf")
}

fn ruta_enlace() -> PathBuf {
    glib::user_config_dir().join("environment.d").join(ENLACE)
}

/// `CLAVE=valor` por línea, con o sin comillas; lo demás se ignora.
fn valores(texto: &str) -> BTreeMap<String, String> {
    texto
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with('#') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            Some((
                k.trim().to_owned(),
                v.trim().trim_matches(['"', '\'']).to_owned(),
            ))
        })
        .collect()
}

/// `es_AR.utf8` y `es_AR.UTF-8` son el mismo.
fn mismo(a: &str, b: &str) -> bool {
    let n = |s: &str| s.to_lowercase().replace('-', "");
    n(a) == n(b)
}

fn normal(id: &str) -> String {
    match id.split_once('.') {
        Some((base, cod)) if mismo(cod, "utf8") => format!("{base}.UTF-8"),
        _ => id.to_owned(),
    }
}

/// Lo que dice el archivo del usuario, contra el `LANG` del sistema.
fn elegido(usuario: &str, sistema: &str) -> (Option<String>, Option<String>) {
    let v = valores(usuario);
    let lang = v.get("LANG").filter(|l| !l.is_empty());
    let idioma = lang.filter(|l| !mismo(l, sistema)).cloned();
    let base = lang.map_or(sistema, String::as_str);
    let formatos = v
        .get("LC_TIME")
        .filter(|t| !t.is_empty() && !mismo(t, base))
        .cloned();
    (idioma, formatos)
}

/// El archivo nuevo, conservando las líneas que no son de morgan; `None` para borrarlo.
fn escribir(
    antes: &str,
    idioma: Option<&str>,
    formatos: Option<&str>,
    sistema: &str,
) -> Option<String> {
    let propias = |l: &str| {
        l.split_once('=')
            .is_some_and(|(k, _)| IDIOMA.contains(&k.trim()) || FORMATOS.contains(&k.trim()))
    };
    let mut s: String = antes
        .lines()
        .filter(|l| !propias(l))
        .map(|l| format!("{l}\n"))
        .collect();
    if idioma.is_none() && formatos.is_none() {
        return (!s.trim().is_empty()).then_some(s);
    }
    let lang = idioma.unwrap_or(sistema);
    let fmt = formatos.unwrap_or(lang);
    for k in IDIOMA {
        s.push_str(&format!("{k}={lang}\n"));
    }
    for k in FORMATOS {
        s.push_str(&format!("{k}={fmt}\n"));
    }
    Some(s)
}

/// Nombre y muestra de cada locale generado, en una sola corrida de `sh`.
fn de_salida(ids: &[String], salida: &str) -> Vec<Locale> {
    let bloques: Vec<&str> = salida.split("--\n").collect();
    ids.iter()
        .enumerate()
        .map(|(i, id)| {
            let l: Vec<&str> = bloques.get(i).map_or(Vec::new(), |b| b.lines().collect());
            let campo = |n: usize| l.get(n).copied().unwrap_or("").trim();
            let lengua = campo(0);
            let nombre = match (lengua, campo(1)) {
                ("", _) => id.clone(),
                (lengua, "") => mayuscula(lengua),
                (lengua, pais) => format!("{} ({pais})", mayuscula(lengua)),
            };
            let numero = format!("1{}234{}5", campo(3), campo(2));
            let muestra = match campo(4) {
                "" => String::new(),
                fecha => format!("{fecha} · {numero}"),
            };
            Locale {
                id: id.clone(),
                nombre,
                muestra,
            }
        })
        .collect()
}

fn mayuscula(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|p| p.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

pub async fn leer() -> Lectura {
    let lista = super::lectura::leer(&["locale", "-a"]).await;
    let mut ids: Vec<String> = lista
        .lines()
        .map(str::trim)
        .filter(|l| l.split_once('.').is_some_and(|(_, c)| mismo(c, "utf8")))
        .filter(|l| !l.starts_with("C.") && !l.starts_with("POSIX"))
        .map(normal)
        .collect();
    ids.sort();
    ids.dedup();
    let guion: String = ids
        .iter()
        .map(|id| {
            format!(
                "LC_ALL='{id}' locale lang_name country_name decimal_point thousands_sep; LC_ALL='{id}' date +%x; echo --;" // no-tr
            )
        })
        .collect();
    let salida = super::lectura::leer(&["sh", "-c", &guion]).await;
    let sistema = valores(&std::fs::read_to_string("/etc/locale.conf").unwrap_or_default())
        .remove("LANG")
        .unwrap_or_else(|| "C.UTF-8".into());
    let (idioma, formatos) = elegido(
        &std::fs::read_to_string(ruta()).unwrap_or_default(),
        &sistema,
    );
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let lang = var("LANG");
    let tiempo = std::env::var("LC_TIME").unwrap_or_else(|_| lang.clone());
    Lectura {
        disponibles: de_salida(&ids, &salida),
        sistema,
        idioma,
        formatos,
        sesion: (lang, tiempo),
    }
}

pub fn aplicar(c: Cambio) -> Result<(), String> {
    let r = ruta();
    let antes = std::fs::read_to_string(&r).unwrap_or_default();
    let sistema = valores(&std::fs::read_to_string("/etc/locale.conf").unwrap_or_default())
        .remove("LANG")
        .unwrap_or_else(|| "C.UTF-8".into());
    let (idioma, formatos) = elegido(&antes, &sistema);
    let (idioma, formatos) = match c {
        Cambio::Idioma(i) => (i, formatos),
        Cambio::Formatos(f) => (idioma, f),
    };
    // Unos formatos iguales al idioma son «los del idioma».
    let formatos = formatos.filter(|f| !mismo(f, idioma.as_deref().unwrap_or(&sistema)));
    let enlace = ruta_enlace();
    match escribir(&antes, idioma.as_deref(), formatos.as_deref(), &sistema) {
        Some(texto) => {
            guardar(&r, &texto).map_err(|e| format!("{}: {e}", r.display()))?;
            if std::fs::symlink_metadata(&enlace).is_err() {
                std::fs::create_dir_all(enlace.parent().unwrap_or(Path::new(".")))
                    .and_then(|()| std::os::unix::fs::symlink("../locale.conf", &enlace)) // no-tr
                    .map_err(|e| format!("{}: {e}", enlace.display()))?;
            }
        }
        None => {
            if r.exists() {
                std::fs::remove_file(&r).map_err(|e| format!("{}: {e}", r.display()))?;
            }
            if std::fs::read_link(&enlace).is_ok() {
                std::fs::remove_file(&enlace).map_err(|e| format!("{}: {e}", enlace.display()))?;
            }
        }
    }
    Ok(())
}

/// Por un temporal y `rename`; si el archivo es un enlace (dotfiles), se escribe su destino.
fn guardar(r: &Path, texto: &str) -> std::io::Result<()> {
    let destino = std::fs::canonicalize(r).unwrap_or_else(|_| r.to_owned());
    let tmp = destino.with_extension("conf.morgan-tmp");
    std::fs::write(&tmp, texto)?;
    std::fs::rename(&tmp, &destino)
}

pub fn filas(l: &Lectura) -> Vec<Fila> {
    let nombre = |id: &str| {
        l.disponibles
            .iter()
            .find(|x| mismo(&x.id, id))
            .map_or_else(|| id.to_owned(), |x| x.nombre.clone())
    };
    let lang = l.idioma.clone().unwrap_or_else(|| l.sistema.clone());
    let fmt = l.formatos.clone().unwrap_or_else(|| lang.clone());
    let pendiente = !mismo(&lang, &l.sesion.0) || !mismo(&fmt, &l.sesion.1);
    let aviso = |s: String| {
        if pendiente {
            trf!("{} · se aplica al volver a entrar a la sesión", s)
        } else {
            s
        }
    };
    let mut idiomas = vec![
        opcion(tr!("El del sistema"), "", Que::Region(Cambio::Idioma(None)))
            .nota(&nombre(&l.sistema))
            .activa(l.idioma.is_none()),
    ];
    idiomas.extend(l.disponibles.iter().map(|x| {
        opcion(
            &x.nombre,
            "",
            Que::Region(Cambio::Idioma(Some(x.id.clone()))),
        )
        .activa(l.idioma.as_deref().is_some_and(|i| mismo(i, &x.id)))
    }));
    let muestra = |id: &str| {
        l.disponibles
            .iter()
            .find(|x| mismo(&x.id, id))
            .map_or(String::new(), |x| x.muestra.clone())
    };
    let mut formatos = vec![
        opcion(
            tr!("Los del idioma"),
            "",
            Que::Region(Cambio::Formatos(None)),
        )
        .nota(&muestra(&lang))
        .activa(l.formatos.is_none()),
    ];
    formatos.extend(l.disponibles.iter().map(|x| {
        opcion(
            &x.nombre,
            "",
            Que::Region(Cambio::Formatos(Some(x.id.clone()))),
        )
        .nota(&x.muestra)
        .activa(l.formatos.as_deref().is_some_and(|f| mismo(f, &x.id)))
    }));
    vec![
        ajuste(
            tr!("Idioma"),
            &aviso(trf!("morgan y las apps · {}", nombre(&lang))),
            Control::Lista(idiomas),
        ),
        ajuste(
            tr!("Formatos"),
            &aviso(trf!("fecha, hora, números y moneda · {}", nombre(&fmt))),
            Control::Lista(formatos),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const ETC: &str = "LANG=es_AR.UTF-8\nLC_TIME=es_AR.UTF-8\n";

    #[test]
    fn sin_archivo_es_lo_del_sistema() {
        assert_eq!(elegido("", "es_AR.UTF-8"), (None, None));
        // Un archivo que repite lo del sistema también.
        assert_eq!(elegido(ETC, "es_AR.UTF-8"), (None, None));
    }

    #[test]
    fn lee_idioma_y_formatos() {
        let t = "# a mano\nLANG=\"en_US.UTF-8\"\nLC_TIME=es_AR.utf8\n";
        assert_eq!(
            elegido(t, "es_AR.UTF-8"),
            (Some("en_US.UTF-8".into()), Some("es_AR.utf8".into()))
        );
        // Formatos iguales al idioma son los del idioma.
        let t = "LANG=en_US.UTF-8\nLC_TIME=en_US.UTF-8\n";
        assert_eq!(
            elegido(t, "es_AR.UTF-8"),
            (Some("en_US.UTF-8".into()), None)
        );
    }

    #[test]
    fn escribe_todo_y_conserva_lo_ajeno() {
        let antes = "# mío\nLANGUAGE=en\nLANG=es_AR.UTF-8\nLC_TIME=es_AR.UTF-8\n";
        let s = escribir(antes, Some("en_US.UTF-8"), None, "es_AR.UTF-8").unwrap();
        assert!(s.starts_with("# mío\nLANGUAGE=en\n"));
        let v = valores(&s);
        assert_eq!(v["LANG"], "en_US.UTF-8");
        assert_eq!(v["LC_MESSAGES"], "en_US.UTF-8");
        // Los formatos van a mano aunque sean los del idioma: /etc pone cada LC_* a mano.
        assert!(FORMATOS.iter().all(|k| v[*k] == "en_US.UTF-8"));
        assert_eq!(s.matches("LANG=").count(), 1);
        // Solo formatos: el idioma queda el del sistema, escrito.
        let v = valores(&escribir("", None, Some("en_US.UTF-8"), "es_AR.UTF-8").unwrap());
        assert_eq!(v["LANG"], "es_AR.UTF-8");
        assert_eq!(v["LC_TIME"], "en_US.UTF-8");
        assert_eq!(
            elegido(
                &escribir("", None, Some("en_US.UTF-8"), "es_AR.UTF-8").unwrap(),
                "es_AR.UTF-8"
            ),
            (None, Some("en_US.UTF-8".into()))
        );
    }

    #[test]
    fn volver_a_lo_del_sistema_borra_el_archivo() {
        assert_eq!(
            escribir("LANG=en_US.UTF-8\nLC_TIME=x\n", None, None, "es_AR.UTF-8"),
            None
        );
        // Salvo que tenga algo que no es de morgan.
        assert_eq!(
            escribir("LANGUAGE=en\nLANG=en_US.UTF-8\n", None, None, "es_AR.UTF-8"),
            Some("LANGUAGE=en\n".into())
        );
    }

    #[test]
    fn nombra_cada_locale_en_su_idioma() {
        let ids = vec![
            "en_US.UTF-8".to_owned(),
            "es_AR.UTF-8".to_owned(),
            "xx.UTF-8".to_owned(),
        ];
        let salida = "English\nUnited States\n.\n,\n11/26/2026\n--\nespañol\nArgentina\n,\n.\n26/11/26\n--\n";
        let v = de_salida(&ids, salida);
        assert_eq!(v[0].nombre, "English (United States)");
        assert_eq!(v[0].muestra, "11/26/2026 · 1,234.5");
        assert_eq!(v[1].nombre, "Español (Argentina)");
        assert_eq!(v[1].muestra, "26/11/26 · 1.234,5");
        // Sin datos, el id.
        assert_eq!(v[2].nombre, "xx.UTF-8");
        assert_eq!(normal("es_AR.utf8"), "es_AR.UTF-8");
    }

    #[test]
    fn marca_lo_elegido_y_avisa_si_falta_entrar() {
        let disponibles = vec![
            Locale {
                id: "en_US.UTF-8".into(),
                nombre: "English (United States)".into(),
                muestra: String::new(),
            },
            Locale {
                id: "es_AR.UTF-8".into(),
                nombre: "Español (Argentina)".into(),
                muestra: String::new(),
            },
        ];
        let mut l = Lectura {
            disponibles,
            sistema: "es_AR.UTF-8".into(),
            sesion: ("es_AR.UTF-8".into(), "es_AR.UTF-8".into()),
            ..Default::default()
        };
        let activas = |f: &Fila| match f {
            Fila::Ajuste {
                control: Control::Lista(v),
                sub,
                ..
            } => (
                v.iter()
                    .filter(|o| o.activa)
                    .map(|o| o.titulo.clone())
                    .collect::<Vec<_>>(),
                sub.clone(),
            ),
            _ => panic!(),
        };
        let f = filas(&l);
        assert_eq!(activas(&f[0]).0, ["El del sistema"]);
        assert_eq!(activas(&f[1]).0, ["Los del idioma"]);
        assert!(!activas(&f[0]).1.contains("volver a entrar"));
        l.idioma = Some("en_US.utf8".into());
        let f = filas(&l);
        assert_eq!(activas(&f[0]).0, ["English (United States)"]);
        assert!(activas(&f[0]).1.contains("volver a entrar"));
        assert!(activas(&f[1]).1.contains("English"));
    }
}
