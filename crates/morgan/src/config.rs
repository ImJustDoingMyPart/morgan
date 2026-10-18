//! Esquema cerrado: cada tabla lleva `deny_unknown_fields`, así que una clave mal escrita o que
//! ya no existe es error al cargar, con la línea, en vez de quedar ignorada en silencio.
//!
//! Las claves y los valores del archivo van en inglés (es la API pública, como `morganctl`);
//! los campos siguen en castellano, cada uno con su `rename`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub shell: Shell,
    #[serde(rename = "notifications")]
    pub notificaciones: Notificaciones,
    #[serde(rename = "gaming")]
    pub juego: Juego,
    #[serde(rename = "bar")]
    pub barra: Barra,
    pub idle: Idle,
    #[serde(rename = "sound")]
    pub sonido: Sonido,
    #[serde(rename = "nightlight")]
    pub luz: Luz,
    #[serde(rename = "wallpapers")]
    pub fondos: Fondos,
    #[serde(rename = "appearance")]
    pub apariencia: Apariencia,
    #[serde(rename = "weather")]
    pub clima: Clima,
    pub matugen: Matugen,
    #[serde(rename = "screenshots")]
    pub capturas: Capturas,
    #[serde(rename = "keybinds")]
    pub atajos: Atajos,
    /// Retirada después de 0.1.1 (las notas por app): se acepta y se ignora, y al guardar se borra.
    #[serde(rename = "notes", skip_serializing)]
    pub retirada_notas: Option<toml::Value>,
    #[serde(rename = "lock")]
    pub bloqueo: morgan_ipc::bloqueo::Bloqueo,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Capturas {
    #[serde(rename = "freeze")]
    pub congelar: bool,
    #[serde(rename = "folder")]
    pub carpeta: String,
    #[serde(rename = "clipboard_only")]
    pub portapapeles: bool,
}

impl Default for Capturas {
    fn default() -> Self {
        Capturas {
            congelar: true,
            carpeta: String::new(),
            portapapeles: false,
        }
    }
}

/// `morgan.lua` las pone al final de la config de Hyprland: pisan lo que hubiera en esas
/// teclas.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Atajos {
    #[serde(deserialize_with = "tecla")]
    pub launcher: String,
    #[serde(deserialize_with = "tecla")]
    #[serde(rename = "symbols")]
    pub simbolos: String,
    #[serde(deserialize_with = "tecla")]
    #[serde(rename = "clipboard")]
    pub portapapeles: String,
    #[serde(deserialize_with = "tecla")]
    #[serde(rename = "keybinds")]
    pub atajos: String,
    #[serde(deserialize_with = "tecla")]
    #[serde(rename = "calculator")]
    pub calculadora: String,
    #[serde(deserialize_with = "tecla")]
    pub menu: String,
    #[serde(deserialize_with = "tecla")]
    #[serde(rename = "wallpapers")]
    pub fondos: String,
    #[serde(deserialize_with = "tecla")]
    #[serde(rename = "menu_wallpapers")]
    pub menu_fondos: String,
    #[serde(deserialize_with = "tecla")]
    #[serde(rename = "windows")]
    pub ventanas: String,
    #[serde(deserialize_with = "tecla")]
    #[serde(rename = "center")]
    pub centro: String,
    #[serde(deserialize_with = "tecla")]
    #[serde(rename = "dnd")]
    pub no_molestar: String,
    /// Retirada después de 0.1.1, como `[notes]`.
    #[serde(rename = "notes", skip_serializing)]
    pub retirada_notas: Option<toml::Value>,
    #[serde(deserialize_with = "tecla")]
    #[serde(rename = "session")]
    pub sesion: String,
    #[serde(deserialize_with = "tecla")]
    #[serde(rename = "lock")]
    pub bloquear: String,
    #[serde(deserialize_with = "tecla")]
    #[serde(rename = "rescue")]
    pub rescate: String,
    #[serde(rename = "media_keys")]
    pub multimedia: bool,
}

impl Default for Atajos {
    fn default() -> Self {
        let t = |s: &str| s.to_owned();
        Atajos {
            launcher: t("SUPER + space"),
            simbolos: t("SUPER + ALT + U"),
            portapapeles: t("SUPER + V"),
            atajos: String::new(),
            calculadora: t("SUPER + ALT + C"),
            menu: t("SUPER + comma"),
            fondos: t("SUPER + Y"),
            menu_fondos: t("SUPER + SHIFT + Y"),
            ventanas: t("SUPER + Tab"),
            centro: t("SUPER + N"),
            no_molestar: t("SUPER + SHIFT + N"),
            retirada_notas: None,
            sesion: t("SUPER + X"),
            bloquear: t("SUPER + ALT + L"),
            rescate: t("SUPER + ALT + SHIFT + L"),
            multimedia: true,
        }
    }
}

impl Atajos {
    pub fn tecla_mut(&mut self, id: &str) -> Option<&mut String> {
        Some(match id {
            "launcher" => &mut self.launcher,
            "symbols" => &mut self.simbolos,
            "clipboard" => &mut self.portapapeles,
            "keybinds" => &mut self.atajos,
            "calculator" => &mut self.calculadora,
            "menu" => &mut self.menu,
            "wallpapers" => &mut self.fondos,
            "menu_wallpapers" => &mut self.menu_fondos,
            "windows" => &mut self.ventanas,
            "center" => &mut self.centro,
            "dnd" => &mut self.no_molestar,
            "session" => &mut self.sesion,
            "lock" => &mut self.bloquear,
            "rescue" => &mut self.rescate,
            _ => return None,
        })
    }

    /// Por la misma tabla que `tecla_mut`: un id nuevo se agrega en un solo lugar.
    pub fn tecla(&self, id: &str) -> Option<String> {
        self.clone().tecla_mut(id).map(|t| t.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Luz {
    #[serde(deserialize_with = "kelvin")]
    #[serde(rename = "temperature")]
    pub temperatura: u32,
    #[serde(rename = "schedule")]
    pub horario: bool,
    #[serde(deserialize_with = "hora")]
    #[serde(rename = "start")]
    pub prender: String,
    #[serde(deserialize_with = "hora")]
    #[serde(rename = "end")]
    pub apagar: String,
}

impl Default for Luz {
    fn default() -> Self {
        Luz {
            temperatura: 4000,
            horario: false,
            prender: "20:00".into(),
            apagar: "07:00".into(),
        }
    }
}

/// Un id que ya no existe vale como el default.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Apariencia {
    #[serde(rename = "animation")]
    pub animacion: String,
    #[serde(rename = "speed")]
    pub velocidad: String,
    #[serde(rename = "shadow")]
    pub sombra: String,
}

impl Default for Apariencia {
    fn default() -> Self {
        Apariencia {
            animacion: "precise".into(),
            velocidad: "medium".into(),
            sombra: "black".into(),
        }
    }
}

/// Sin coordenadas no hay clima: no se geolocaliza por IP.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Clima {
    #[serde(rename = "place")]
    pub lugar: String,
    #[serde(rename = "latitude")]
    pub latitud: Option<f64>,
    #[serde(rename = "longitude")]
    pub longitud: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Fondos {
    #[serde(rename = "folder")]
    pub carpeta: String,
    #[serde(rename = "rotate_hours")]
    pub rotar: u32,
    #[serde(rename = "exclude")]
    pub excluir: Vec<String>,
    #[serde(rename = "transition")]
    pub transicion: String,
}

impl Fondos {
    pub fn raiz(&self) -> PathBuf {
        let elegida = PathBuf::from(&self.carpeta);
        if !self.carpeta.is_empty() && elegida.is_dir() {
            return elegida;
        }
        gtk::glib::user_special_dir(gtk::glib::UserDirectory::Pictures)
            .unwrap_or_else(gtk::glib::home_dir)
    }
}

pub fn minutos_del_dia(hora: &str) -> u32 {
    let (h, m) = hora.split_once(':').unwrap_or(("0", "0"));
    h.parse::<u32>().unwrap_or(0) * 60 + m.parse::<u32>().unwrap_or(0)
}

fn kelvin<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = i64::deserialize(d)?;
    match u32::try_from(v) {
        Ok(v @ 2500..=6500) => Ok(v),
        _ => Err(de::Error::invalid_value(
            de::Unexpected::Signed(v),
            &"kelvin from 2500 to 6500",
        )),
    }
}

fn hora<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    let ok = v.split_once(':').is_some_and(|(h, m)| {
        h.len() == 2
            && m.len() == 2
            && h.parse::<u32>().is_ok_and(|h| h < 24)
            && m.parse::<u32>().is_ok_and(|m| m < 60)
    });
    if ok {
        Ok(v)
    } else {
        Err(de::Error::invalid_value(
            de::Unexpected::Str(&v),
            &"a time HH:MM",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Matugen {
    #[serde(deserialize_with = "tipo_matugen")]
    #[serde(rename = "type")]
    pub tipo: String,
    #[serde(deserialize_with = "modo_matugen")]
    #[serde(rename = "mode")]
    pub modo: String,
    #[serde(deserialize_with = "contraste")]
    #[serde(rename = "contrast")]
    pub contraste: f64,
}

impl Default for Matugen {
    fn default() -> Self {
        Matugen {
            tipo: "scheme-tonal-spot".into(),
            modo: "dark".into(),
            contraste: 0.0,
        }
    }
}

pub const TIPOS_MATUGEN: &[&str] = &[
    "scheme-content",
    "scheme-expressive",
    "scheme-fidelity",
    "scheme-fruit-salad",
    "scheme-monochrome",
    "scheme-neutral",
    "scheme-rainbow",
    "scheme-tonal-spot",
    "scheme-vibrant",
    "scheme-smart",
];

fn tipo_matugen<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    if TIPOS_MATUGEN.contains(&v.as_str()) {
        Ok(v)
    } else {
        Err(de::Error::invalid_value(
            de::Unexpected::Str(&v),
            &"a matugen type (scheme-tonal-spot, scheme-vibrant, …)",
        ))
    }
}

fn modo_matugen<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    match v.as_str() {
        "dark" | "light" => Ok(v),
        _ => Err(de::Error::invalid_value(
            de::Unexpected::Str(&v),
            &"dark or light",
        )),
    }
}

fn contraste<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    let v = f64::deserialize(d)?;
    if (-1.0..=1.0).contains(&v) {
        Ok(v)
    } else {
        Err(de::Error::invalid_value(
            de::Unexpected::Float(v),
            &"a number from -1 to 1",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sonido {
    #[serde(deserialize_with = "paso")]
    #[serde(rename = "step")]
    pub paso: u32,
    #[serde(rename = "overamplify")]
    pub amplificar: bool,
    #[serde(rename = "volume_change")]
    pub al_cambiar_volumen: bool,
    #[serde(rename = "devices")]
    pub dispositivos: bool,
    #[serde(rename = "network")]
    pub red: bool,
    #[serde(rename = "lock")]
    pub bloqueo: bool,
    #[serde(rename = "recording")]
    pub grabacion: bool,
    #[serde(rename = "hidden_outputs")]
    pub ocultas: Vec<String>,
}

impl Default for Sonido {
    fn default() -> Self {
        Sonido {
            paso: 5,
            amplificar: false,
            al_cambiar_volumen: true,
            dispositivos: true,
            red: true,
            bloqueo: true,
            grabacion: true,
            ocultas: Vec::new(),
        }
    }
}

impl Sonido {
    pub fn oculta(&self, salida: &str) -> bool {
        self.ocultas.iter().any(|s| s == salida)
    }

    pub fn tope(&self) -> f64 {
        if self.amplificar { 150.0 } else { 100.0 }
    }
}

fn paso<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = i64::deserialize(d)?;
    match v {
        2 | 5 | 10 => Ok(v as u32),
        _ => Err(de::Error::invalid_value(
            de::Unexpected::Signed(v),
            &"2, 5 or 10 (percentage points)",
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Shell {
    #[serde(deserialize_with = "fraccion")]
    #[serde(rename = "opacity")]
    pub opacidad: f64,
    #[serde(deserialize_with = "idioma")]
    #[serde(rename = "language")]
    pub idioma: String,
}

impl Default for Shell {
    fn default() -> Self {
        Shell {
            opacidad: 0.85,
            idioma: "auto".into(),
        }
    }
}

/// Va entre comillas en `morgan.lua`, así que sin comillas, barras ni saltos. Si Hyprland no
/// la entiende, `--verify-config` la rechaza y queda el archivo de antes.
fn tecla<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    if v.contains(['"', '\\', '\n', '\r']) {
        return Err(serde::de::Error::custom(format!("key {v:?}: no quotes, backslashes or line breaks")));
    }
    Ok(v.trim().to_owned())
}

fn idioma<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    match morgan_idioma::Idioma::de_config(&v) {
        Some(_) => Ok(v),
        None => Err(de::Error::invalid_value(de::Unexpected::Str(&v), &"auto, en or es")),
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Notificaciones {
    #[serde(rename = "muted")]
    pub silenciadas: Vec<String>,
    #[serde(deserialize_with = "dias")]
    #[serde(rename = "retention_days")]
    pub retencion_dias: u32,
    #[serde(rename = "sound")]
    pub sonido: bool,
    #[serde(rename = "silent")]
    pub sin_sonido: Vec<String>,
    #[serde(rename = "replace")]
    pub una_sola: Vec<String>,
}

impl Default for Notificaciones {
    fn default() -> Self {
        Notificaciones {
            silenciadas: Vec::new(),
            retencion_dias: 7,
            sonido: true,
            sin_sonido: Vec::new(),
            una_sola: Vec::new(),
        }
    }
}

impl Notificaciones {
    pub fn silenciada(&self, app: &str) -> bool {
        self.silenciadas.iter().any(|s| s.eq_ignore_ascii_case(app))
    }

    pub fn sin_sonido(&self, app: &str) -> bool {
        self.sin_sonido.iter().any(|s| s.eq_ignore_ascii_case(app))
    }

    pub fn una_sola(&self, app: &str) -> bool {
        self.una_sola.iter().any(|s| s.eq_ignore_ascii_case(app))
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Juego {
    #[serde(rename = "summary")]
    pub resumen: bool,
}

impl Default for Juego {
    fn default() -> Self {
        Juego { resumen: true }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Idle {
    #[serde(deserialize_with = "minutos")]
    #[serde(rename = "screen_off")]
    pub apagar: u32,
    #[serde(deserialize_with = "minutos")]
    #[serde(rename = "lock")]
    pub bloquear: u32,
    #[serde(deserialize_with = "minutos")]
    #[serde(rename = "suspend")]
    pub suspender: u32,
}

impl Default for Idle {
    fn default() -> Self {
        Idle {
            apagar: 10,
            bloquear: 20,
            suspender: 0,
        }
    }
}

fn minutos<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = i64::deserialize(d)?;
    match u32::try_from(v) {
        Ok(v @ 0..=1440) => Ok(v),
        _ => Err(de::Error::invalid_value(
            de::Unexpected::Signed(v),
            &"whole minutes from 0 (never) to 1440",
        )),
    }
}

fn dias<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = i64::deserialize(d)?;
    match u32::try_from(v) {
        Ok(v @ 1..=365) => Ok(v),
        _ => Err(de::Error::invalid_value(
            de::Unexpected::Signed(v),
            &"whole days from 1 to 365",
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Barra {
    #[serde(rename = "persistent_workspaces")]
    pub fijos: BTreeMap<String, Vec<NonZeroU32>>,
    #[serde(rename = "tray_icons")]
    pub iconos_bandeja: BTreeMap<String, String>,
    #[serde(rename = "tv")]
    pub tele: String,
}

impl Default for Barra {
    fn default() -> Self {
        Barra {
            fijos: BTreeMap::new(),
            // vesktop y openrgb publican un pixmap crudo y Steam trae su propio `IconThemePath`: no
            // pasan por el tema.
            iconos_bandeja: BTreeMap::from([
                ("steam".into(), "steam".into()),
                ("openrgb".into(), "openrgb".into()),
                ("vesktop_status_icon_1".into(), "vesktop".into()),
            ]),
            tele: String::new(),
        }
    }
}

/// TOML no lee `0` y `1` enteros como flotantes.
fn fraccion<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    struct V;
    impl Visitor<'_> for V {
        type Value = f64;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a number from 0 to 1")
        }

        fn visit_f64<E: de::Error>(self, v: f64) -> Result<f64, E> {
            if (0.0..=1.0).contains(&v) {
                Ok(v)
            } else {
                Err(E::invalid_value(de::Unexpected::Float(v), &self))
            }
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<f64, E> {
            match v {
                0 | 1 => Ok(v as f64),
                _ => Err(E::invalid_value(de::Unexpected::Signed(v), &self)),
            }
        }
    }
    d.deserialize_any(V)
}

#[derive(Debug)]
pub enum Error {
    NoConfigDir,
    Read(PathBuf, io::Error),
    Parse(PathBuf, toml::de::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::NoConfigDir => f.write_str("no hay XDG_CONFIG_HOME ni HOME"),
            Error::Read(p, e) => write!(f, "no se pudo leer {}: {e}", p.display()),
            // El error de toml ya trae la línea, la columna y el fragmento señalado.
            Error::Parse(p, e) => write!(f, "{}: {e}", p.display()),
        }
    }
}

impl std::error::Error for Error {}

pub fn path() -> Result<PathBuf, Error> {
    path_from(|k| std::env::var_os(k))
}

pub fn path_from(env: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf, Error> {
    // La spec de XDG pide ignorar un XDG_CONFIG_HOME relativo.
    let base = env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            env("HOME")
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(".config"))
        })
        .ok_or(Error::NoConfigDir)?;
    Ok(base.join("morgan").join("config.toml"))
}

pub fn load(path: &Path) -> Result<Config, Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text).map_err(|e| Error::Parse(path.to_owned(), e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(Error::Read(path.to_owned(), e)),
    }
}

pub fn parse(text: &str) -> Result<Config, toml::de::Error> {
    toml::from_str(text)
}

/// A un temporal que después se renombra: un corte a mitad de camino no deja un archivo que
/// impida arrancar. Quedan solo las claves que difieren del default, con los comentarios y el
/// orden de quien la escribió.
pub fn guardar(path: &Path, c: &Config) -> Result<(), String> {
    let previo = std::fs::read_to_string(path).unwrap_or_default();
    let texto = escribir_sobre(&previo, c)?;
    let r = (|| {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, texto)?;
        std::fs::rename(&tmp, path)
    })();
    r.map_err(|e| format!("no se pudo escribir {}: {e}", path.display()))
}

pub fn escribir_sobre(previo: &str, c: &Config) -> Result<String, String> {
    let tabla = |c: &Config| toml::Table::try_from(c).map_err(|e| e.to_string());
    let (nuevo, defecto) = (tabla(c)?, tabla(&Config::default())?);
    let mut doc: toml_edit::DocumentMut = previo.parse().unwrap_or_default();
    fundir(doc.as_table_mut(), &nuevo, Some(&defecto));
    Ok(doc.to_string())
}

/// Lo comentado se respeta aunque sea el default.
fn comentado(d: &toml_edit::Decor) -> bool {
    let tiene = |r: Option<&toml_edit::RawString>| r.and_then(|r| r.as_str()).is_some_and(|s| s.contains('#'));
    tiene(d.prefix()) || tiene(d.suffix())
}

/// `defecto` es `None` dentro de un mapa sin default por clave.
fn fundir(doc: &mut toml_edit::Table, nuevo: &toml::Table, defecto: Option<&toml::Table>) {
    let sobran: Vec<String> = doc
        .iter()
        .map(|(k, _)| k.to_owned())
        .filter(|k| !nuevo.contains_key(k))
        .collect();
    for k in sobran {
        doc.remove(&k);
    }
    for (k, v) in nuevo {
        let def = defecto.and_then(|d| d.get(k));
        if let toml::Value::Table(t) = v {
            let sub = doc
                .entry(k)
                .or_insert_with(|| {
                    let mut t = toml_edit::Table::new();
                    t.set_implicit(true);
                    toml_edit::Item::Table(t)
                });
            let Some(sub) = sub.as_table_mut() else {
                // Estaba escrita como tabla en línea: se reemplaza entera.
                *sub = toml_edit::Item::Table(toml_edit::Table::new());
                fundir(doc[k.as_str()].as_table_mut().unwrap(), t, def.and_then(toml::Value::as_table));
                continue;
            };
            fundir(sub, t, def.and_then(toml::Value::as_table));
            if sub.is_empty() && !comentado(sub.decor()) {
                doc.remove(k);
            }
            continue;
        }
        let igual_al_default = def == Some(v);
        let puesto = doc.get(k).and_then(|i| i.as_value()).map(|x| x.to_string().trim().to_owned());
        let comentada = doc.key(k).is_some_and(|key| comentado(key.leaf_decor()))
            || doc.get(k).and_then(|i| i.as_value()).is_some_and(|x| comentado(x.decor()));
        if igual_al_default && !comentada {
            doc.remove(k);
            continue;
        }
        let Ok(valor) = v.to_string().parse::<toml_edit::Value>() else { continue };
        if puesto.as_deref() == Some(valor.to_string().trim()) {
            continue;
        }
        match doc.get_mut(k).and_then(|i| i.as_value_mut()) {
            // En su lugar, con el comentario que tenga al lado.
            Some(x) => {
                let decor = x.decor().clone();
                *x = valor;
                *x.decor_mut() = decor;
            }
            None => {
                doc.insert(k, toml_edit::value(valor));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarda_solo_lo_distinto() {
        let mut c = Config::default();
        assert_eq!(escribir_sobre("", &c).unwrap().trim(), "", "todo default: archivo vacío");
        c.shell.opacidad = 0.9;
        c.notificaciones.una_sola = vec!["kitty".into()];
        let t = escribir_sobre("", &c).unwrap();
        assert_eq!(parse(&t).unwrap(), c);
        assert!(t.contains("[shell]\nopacity = 0.9\n"), "{t}");
        assert!(!t.contains("retention_days") && !t.contains("[gaming]"), "{t}");
    }

    #[test]
    fn respeta_lo_escrito_a_mano() {
        let previo = "# Mi config\n\n[bar]\n# la tele\ntv = \"HDMI-A-1\"\n\n[bar.persistent_workspaces]\nDP-3 = [1, 2] # el monitor\nHDMI-A-1 = [4]\n\n[idle]\n# lo dejo explícito\nscreen_off = 10\nlock = 20\n";
        let mut c = parse(previo).unwrap();
        c.barra.fijos.remove("HDMI-A-1");
        c.barra.fijos.insert("DP-3".into(), vec![NonZeroU32::new(3).unwrap()]);
        let t = escribir_sobre(previo, &c).unwrap();
        assert_eq!(parse(&t).unwrap(), c);
        assert!(t.starts_with("# Mi config\n"), "{t}");
        assert!(t.contains("# la tele\ntv = \"HDMI-A-1\""), "{t}");
        assert!(t.contains("DP-3 = [3] # el monitor"), "{t}");
        assert!(!t.contains("HDMI-A-1 = "), "la entrada borrada se va: {t}");
        assert!(t.contains("# lo dejo explícito\nscreen_off = 10"), "comentada, aunque sea default: {t}");
        assert!(!t.contains("lock"), "default sin comentario: {t}");
        // Volver a guardar lo mismo no cambia nada.
        assert_eq!(escribir_sobre(&t, &c).unwrap(), t);
    }

    fn err(text: &str) -> String {
        parse(text).unwrap_err().to_string()
    }

    #[test]
    fn vacio_es_default() {
        assert_eq!(parse("").unwrap(), Config::default());
        assert_eq!(parse("[shell]").unwrap(), Config::default());
    }

    #[test]
    fn idioma() {
        assert_eq!(parse("[shell]\nlanguage = \"en\"").unwrap().shell.idioma, "en");
        assert!(err("[shell]\nlanguage = \"fr\"").contains("auto, en or es"));
    }

    #[test]
    fn atajos() {
        let c = parse("[keybinds]\nlauncher = \" SUPER + R \"\n").unwrap();
        assert_eq!(c.atajos.launcher, "SUPER + R");
        assert_eq!(c.atajos.centro, "SUPER + N");
        assert!(err("[keybinds]\nmenu = \"SUPER + \\\"\"").contains("no quotes"));
    }

    #[test]
    fn fondos() {
        let c = parse("[wallpapers]\nrotate_hours = 12\nexclude = [\"Screenshots\"]\n").unwrap();
        assert_eq!(c.fondos.rotar, 12);
        assert_eq!(c.fondos.excluir, ["Screenshots"]);
        assert!(c.fondos.carpeta.is_empty() && c.fondos.transicion.is_empty());
        assert!(err("[wallpapers]\nrotate_hours = -1").contains("rotate_hours"));
        // Una carpeta que no existe cae a la de imágenes, no a una lista vacía.
        let f = Fondos {
            carpeta: "/no/existe".into(),
            ..Fondos::default()
        };
        assert_ne!(f.raiz(), PathBuf::from("/no/existe"));
    }

    #[test]
    fn default_duplica_la_estetica_actual() {
        assert_eq!(Config::default().shell.opacidad, 0.85);
    }

    #[test]
    fn clave_desconocida_es_error() {
        let e = err("[shell]\nopacit = 0.5\n");
        assert!(e.contains("opacit"), "{e}");
        assert!(e.contains("line 2"), "{e}");
    }

    #[test]
    fn seccion_desconocida_es_error() {
        assert!(err("[tray]\n").contains("tray"));
        assert!(err("opacity = 0.5\n").contains("opacity"));
    }

    #[test]
    fn tipo_equivocado_es_error() {
        assert!(parse("[shell]\nopacity = \"alta\"\n").is_err());
    }

    #[test]
    fn opacidad() {
        let o = |v: &str| parse(&format!("[shell]\nopacity = {v}\n")).map(|c| c.shell.opacidad);
        assert_eq!(o("0.5").unwrap(), 0.5);
        assert_eq!(o("1").unwrap(), 1.0);
        assert_eq!(o("0").unwrap(), 0.0);
        assert!(o("\"hyprland\"").is_err());
        assert!(o("1.5").is_err());
        assert!(o("-0.1").is_err());
        assert!(o("2").is_err());
        assert!(o("nan").is_err());
    }

    #[test]
    fn notificaciones() {
        let c =
            parse("[notifications]\nmuted = [\"Steam\"]\nretention_days = 30\n").unwrap();
        assert!(c.notificaciones.silenciada("steam"));
        assert!(!c.notificaciones.silenciada("discord"));
        assert_eq!(c.notificaciones.retencion_dias, 30);
        assert_eq!(Config::default().notificaciones.retencion_dias, 7);
        let d = |v: &str| parse(&format!("[notifications]\nretention_days = {v}\n"));
        assert!(d("0").is_err());
        assert!(d("366").is_err());
        assert!(d("-1").is_err());
        assert!(d("1.5").is_err());
        assert!(err("[notifications]\nmute = []\n").contains("mute"));
        let u = parse("[notifications]\nreplace = [\"kitty\"]\n").unwrap();
        assert!(u.notificaciones.una_sola("Kitty"));
        assert!(!Config::default().notificaciones.una_sola("kitty"));
    }

    #[test]
    fn juego() {
        assert!(Config::default().juego.resumen);
        assert!(!parse("[gaming]\nsummary = false\n").unwrap().juego.resumen);
        assert!(parse("[gaming]\nvolume_away = 20\n").is_err());
    }

    #[test]
    fn las_notas_retiradas_se_ignoran_y_se_borran_al_guardar() {
        let previo = "[notes]\napps = [\"kitty\"]\n\n[keybinds]\nnotes = \"SUPER + ALT + N\"\nlauncher = \"SUPER + R\"\n";
        let c = parse(previo).unwrap();
        let t = escribir_sobre(previo, &c).unwrap();
        assert!(!t.contains("notes"), "{t}");
        assert!(t.contains("launcher = \"SUPER + R\""), "{t}");
    }

    #[test]
    fn barra() {
        assert!(Config::default().barra.fijos.is_empty());
        assert!(Config::default().barra.tele.is_empty());
        let c = parse("[bar.persistent_workspaces]\nDP-3 = [1, 2]\n").unwrap();
        let ids: Vec<u32> = c.barra.fijos["DP-3"].iter().map(|i| i.get()).collect();
        assert_eq!(ids, [1, 2]);
        assert!(parse("[bar.persistent_workspaces]\n").unwrap().barra.fijos.is_empty());
        assert!(parse("[bar.persistent_workspaces]\nDP-3 = [0]\n").is_err());
        assert!(parse("[bar.persistent_workspaces]\nDP-3 = [-1]\n").is_err());
        assert!(parse("[bar.persistent_workspaces]\nDP-3 = 1\n").is_err());
        assert!(err("[bar]\npersistent = {}\n").contains("persistent"));
        let iconos = Config::default().barra.iconos_bandeja;
        assert_eq!(iconos["vesktop_status_icon_1"], "vesktop");
        let c = parse("[bar.tray_icons]\nsteam = \"/x/steam.svg\"\n").unwrap();
        assert_eq!(c.barra.iconos_bandeja.len(), 1);
        assert!(parse("[bar.tray_icons]\nsteam = 1\n").is_err());
    }

    #[test]
    fn idle() {
        let d = Config::default().idle;
        assert_eq!((d.apagar, d.bloquear), (10, 20));
        let c = parse("[idle]\nscreen_off = 0\nlock = 5\n").unwrap();
        assert_eq!((c.idle.apagar, c.idle.bloquear), (0, 5));
        let m = |v: &str| parse(&format!("[idle]\nlock = {v}\n"));
        assert!(m("-1").is_err());
        assert!(m("1441").is_err());
        assert!(m("1.5").is_err());
        assert_eq!(
            parse("[idle]\nsuspend = 30\n").unwrap().idle.suspender,
            30
        );
        assert_eq!(d.suspender, 0);
    }

    #[test]
    fn sonido() {
        let d = Config::default().sonido;
        assert_eq!((d.paso, d.amplificar, d.tope()), (5, false, 100.0));
        let c = parse("[sound]\nstep = 10\noveramplify = true\n").unwrap();
        assert_eq!((c.sonido.paso, c.sonido.tope()), (10, 150.0));
        assert!(err("[sound]\nstep = 3\n").contains("2, 5 or 10"));
        let n = parse("[notifications]\nsound = false\nsilent = [\"Steam\"]\n").unwrap();
        assert!(!n.notificaciones.sonido && n.notificaciones.sin_sonido("steam"));
    }

    #[test]
    fn ruta() {
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |k: &str| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.into())
        };
        assert_eq!(
            path_from(env(&[("XDG_CONFIG_HOME", "/x"), ("HOME", "/h")])).unwrap(),
            Path::new("/x/morgan/config.toml")
        );
        assert_eq!(
            path_from(env(&[("XDG_CONFIG_HOME", "rel"), ("HOME", "/h")])).unwrap(),
            Path::new("/h/.config/morgan/config.toml")
        );
        assert!(matches!(path_from(env(&[])), Err(Error::NoConfigDir)));
    }

    #[test]
    fn guardar_ida_y_vuelta() {
        let dir = std::env::temp_dir().join(format!("morgan-config-{}", std::process::id()));
        let path = dir.join("morgan/config.toml");
        let mut c = Config::default();
        c.shell.opacidad = 1.0;
        c.notificaciones.silenciadas = vec!["Steam".into()];
        c.notificaciones.retencion_dias = 30;
        c.notificaciones.una_sola = vec!["kitty".into()];
        c.juego.resumen = false;
        c.barra.fijos.insert("DP-3".into(), vec![NonZeroU32::new(2).unwrap()]);
        c.idle.apagar = 0;
        guardar(&path, &c).unwrap();
        assert_eq!(load(&path).unwrap(), c);
        guardar(&path, &Config::default()).unwrap();
        assert_eq!(load(&path).unwrap(), Config::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn archivo_inexistente_es_default() {
        let c = load(Path::new("/nonexistent/morgan/config.toml")).unwrap();
        assert_eq!(c, Config::default());
    }
}
