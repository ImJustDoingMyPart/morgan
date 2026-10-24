//! Los íconos del tema, sin GTK: se buscan como dice la especificación de freedesktop (el tema
//! elegido, los que hereda, después `hicolor` y al final `/usr/share/pixmaps`) y se pasan a
//! píxeles con resvg (los SVG) o `image` (los PNG). Los simbólicos se pintan del color pedido.
//!
//! Un SVG con `var(--…)` no se dibuja: es una hoja de estilos de otra app (el de Antigravity es
//! una miniatura de su editor, no su logo) y con GTK tampoco cargaba. Se usa el PNG de
//! `/usr/share/pixmaps` si la app trae uno.

mod indice;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gio::prelude::*;
use morgan_motor::color::Color;
use morgan_motor::vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

const PIXMAPS: &str = "/usr/share/pixmaps";
/// Las que se pueden pasar a píxeles, en el orden de la especificación (sin XPM).
const EXTENSIONES: [&str; 2] = ["png", "svg"];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tipo {
    Fijo,
    Escalable,
    Umbral,
}

/// Una carpeta de un tema, con su sección de `index.theme`.
#[derive(Clone, Debug, PartialEq)]
struct Carpeta {
    ruta: String,
    tam: u32,
    escala: u32,
    tipo: Tipo,
    min: u32,
    max: u32,
    umbral: u32,
}

impl Carpeta {
    /// `DirectoryMatchesSize` de la especificación.
    fn coincide(&self, tam: u32, escala: u32) -> bool {
        if self.escala != escala {
            return false;
        }
        match self.tipo {
            Tipo::Fijo => self.tam == tam,
            Tipo::Escalable => (self.min..=self.max).contains(&tam),
            Tipo::Umbral => self.tam.abs_diff(tam) <= self.umbral,
        }
    }

    /// `DirectorySizeDistance` de la especificación, en píxeles de verdad.
    fn distancia(&self, tam: u32, escala: u32) -> u32 {
        let (pedido, propio) = (tam * escala, self.tam * self.escala);
        match self.tipo {
            Tipo::Fijo => propio.abs_diff(pedido),
            Tipo::Escalable => {
                (self.min * self.escala).saturating_sub(pedido)
                    + pedido.saturating_sub(self.max * self.escala)
            }
            Tipo::Umbral => {
                let a = self.tam.saturating_sub(self.umbral) * self.escala;
                let b = (self.tam + self.umbral) * self.escala;
                a.saturating_sub(pedido) + pedido.saturating_sub(b)
            }
        }
    }
}

#[derive(Default)]
struct Tema {
    /// Las carpetas del tema en cada lugar donde está (el de usuario y el del sistema), cada
    /// una con su índice, si tiene uno al día.
    raices: Vec<PathBuf>,
    indices: Vec<Option<indice::Indice>>,
    carpetas: Vec<Carpeta>,
    /// El lugar de cada carpeta en `carpetas`, por su ruta.
    por_ruta: HashMap<String, usize>,
    hereda: Vec<String>,
}

/// `index.theme`: las carpetas que declara, con sus medidas, y de quién hereda.
fn leer_indice(texto: &str) -> Tema {
    let mut secciones: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut actual = String::new();
    for linea in texto.lines() {
        let l = linea.trim();
        if l.starts_with('#') || l.is_empty() {
            continue;
        }
        if let Some(s) = l.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            actual = s.to_owned();
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            secciones
                .entry(actual.clone())
                .or_default()
                .insert(k.trim().to_owned(), v.trim().to_owned());
        }
    }
    let lista = |s: Option<&String>| -> Vec<String> {
        s.map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
    };
    let cabecera = secciones.get("Icon Theme");
    let mut nombres = lista(cabecera.and_then(|c| c.get("Directories")));
    nombres.extend(lista(cabecera.and_then(|c| c.get("ScaledDirectories"))));
    let mut vistos = HashSet::new();
    let carpetas: Vec<Carpeta> = nombres
        .into_iter()
        .filter(|n| vistos.insert(n.clone()))
        .filter_map(|n| {
            let s = secciones.get(&n)?;
            let num = |k: &str| s.get(k).and_then(|v| v.parse::<u32>().ok());
            let tam = num("Size")?;
            let tipo = match s.get("Type").map(String::as_str) {
                Some("Fixed") => Tipo::Fijo,
                Some("Scalable") => Tipo::Escalable,
                _ => Tipo::Umbral,
            };
            Some(Carpeta {
                ruta: n,
                tam,
                escala: num("Scale").unwrap_or(1).max(1),
                tipo,
                min: num("MinSize").unwrap_or(tam),
                max: num("MaxSize").unwrap_or(tam),
                umbral: num("Threshold").unwrap_or(2),
            })
        })
        .collect();
    let por_ruta = carpetas
        .iter()
        .enumerate()
        .map(|(i, c): (usize, &Carpeta)| (c.ruta.clone(), i))
        .collect();
    Tema {
        raices: Vec::new(),
        indices: Vec::new(),
        carpetas,
        por_ruta,
        hereda: lista(cabecera.and_then(|c| c.get("Inherits"))),
    }
}

/// Donde se buscan los temas, en el orden de la especificación.
fn bases() -> Vec<PathBuf> {
    let mut v = Vec::new();
    let casa = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(c) = &casa {
        v.push(c.join(".icons"));
    }
    match std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        Some(d) if d.is_absolute() => v.push(d.join("icons")),
        _ => {
            if let Some(c) = &casa {
                v.push(c.join(".local/share/icons"));
            }
        }
    }
    let datos = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    v.extend(datos.split(':').map(|d| Path::new(d).join("icons")));
    v
}

fn cargar_tema(nombre: &str, bases: &[PathBuf]) -> Option<Tema> {
    let raices: Vec<PathBuf> = bases
        .iter()
        .map(|b| b.join(nombre))
        .filter(|r| r.is_dir())
        .collect();
    let indice = raices
        .iter()
        .find_map(|r| std::fs::read_to_string(r.join("index.theme")).ok())?;
    let mut t = leer_indice(&indice);
    t.indices = raices
        .iter()
        .map(|r| indice::Indice::abrir(r, t.carpetas.iter().map(|c| c.ruta.as_str())))
        .collect();
    t.raices = raices;
    Some(t)
}

/// El tema, los que hereda (en orden, sin repetir) y `hicolor` al final.
fn cadena(nombre: &str) -> Vec<Tema> {
    cadena_en(nombre, &bases())
}

fn cadena_en(nombre: &str, bases: &[PathBuf]) -> Vec<Tema> {
    let mut v = Vec::new();
    let mut vistos = HashSet::new();
    let mut pendientes = vec![nombre.to_owned()];
    while let Some(n) = pendientes.pop() {
        if !vistos.insert(n.clone()) {
            continue;
        }
        if let Some(t) = cargar_tema(&n, bases) {
            pendientes.extend(t.hereda.iter().rev().cloned());
            v.push(t);
        }
    }
    if !vistos.contains("hicolor")
        && let Some(t) = cargar_tema("hicolor", bases)
    {
        v.push(t);
    }
    v
}

/// En un tema: primero lo que coincide con el tamaño y después lo más cercano. Lo que hay se
/// lee del índice de cada lugar del tema; sin índice, se mira el disco carpeta por carpeta.
fn buscar_en(t: &Tema, nombre: &str, tam: u32, escala: u32) -> Option<PathBuf> {
    // (carpeta, lugar, extensión, archivo), en el orden de la especificación.
    let mut hay: Vec<(usize, usize, usize, PathBuf)> = Vec::new();
    let rango = |ext: &str| EXTENSIONES.iter().position(|e| *e == ext).unwrap_or(9);
    for (ri, raiz) in t.raices.iter().enumerate() {
        match t.indices.get(ri).and_then(Option::as_ref) {
            Some(ix) => {
                for (carpeta, ext) in ix.buscar(nombre) {
                    if let Some(&ci) = t.por_ruta.get(carpeta) {
                        let p = raiz.join(carpeta).join(format!("{nombre}.{ext}"));
                        hay.push((ci, ri, rango(ext), p));
                    }
                }
            }
            None => {
                for (ci, c) in t.carpetas.iter().enumerate() {
                    for (ei, e) in EXTENSIONES.iter().enumerate() {
                        let p = raiz.join(&c.ruta).join(format!("{nombre}.{e}"));
                        if p.is_file() {
                            hay.push((ci, ri, ei, p));
                        }
                    }
                }
            }
        }
    }
    hay.sort_by_key(|(ci, ri, ei, _)| (*ci, *ri, *ei));
    if let Some((_, _, _, p)) = hay
        .iter()
        .find(|(ci, ..)| t.carpetas[*ci].coincide(tam, escala))
    {
        return Some(p.clone());
    }
    hay.into_iter()
        .min_by_key(|(ci, ..)| t.carpetas[*ci].distancia(tam, escala))
        .map(|(_, _, _, p)| p)
}

fn en_pixmaps(nombre: &str) -> Option<PathBuf> {
    EXTENSIONES
        .iter()
        .map(|e| Path::new(PIXMAPS).join(format!("{nombre}.{e}")))
        .find(|p| p.is_file())
}

/// El tema elegido: el de GNOME si alguien lo eligió (lo pone Apariencia); si no, el de la
/// configuración de GTK, como hace GTK; si no, el de GNOME por defecto.
fn tema_elegido(ajustes: Option<&gio::Settings>) -> String {
    if let Some(s) = ajustes
        && let Some(v) = s.user_value("icon-theme")
        && let Some(t) = v.str()
        && !t.is_empty()
    {
        return t.to_owned();
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    let del_ini = config.and_then(|c| {
        ["gtk-4.0", "gtk-3.0"].iter().find_map(|v| {
            let t = std::fs::read_to_string(c.join(v).join("settings.ini")).ok()?;
            tema_del_ini(&t)
        })
    });
    del_ini
        .or_else(|| ajustes.map(|s| s.string("icon-theme").to_string()))
        .unwrap_or_else(|| "hicolor".to_owned())
}

fn tema_del_ini(t: &str) -> Option<String> {
    t.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == "gtk-icon-theme-name")
            .then(|| v.trim().trim_matches('"').to_owned())
            .filter(|v| !v.is_empty())
    })
}

#[derive(Default)]
struct Cache {
    tema: String,
    cadena: Vec<Tema>,
    rutas: HashMap<(String, u32, u32), Option<PathBuf>>,
    ajustes: Option<gio::Settings>,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::default();
}

/// El tema elegido (`org.gnome.desktop.interface icon-theme`, el que pone Apariencia); al
/// cambiar, lo encontrado se olvida.
fn con_cache<R>(f: impl FnOnce(&mut Cache) -> R) -> R {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.ajustes.is_none() {
            let s = gio::Settings::new("org.gnome.desktop.interface");
            s.connect_changed(Some("icon-theme"), |_, _| {
                CACHE.with(|c| {
                    if let Ok(mut c) = c.try_borrow_mut() {
                        c.tema.clear();
                    }
                });
            });
            c.ajustes = Some(s);
        }
        let tema = tema_elegido(c.ajustes.as_ref());
        if tema != c.tema || c.cadena.is_empty() {
            c.cadena = cadena(&tema);
            c.tema = tema;
            c.rutas.clear();
        }
        f(&mut c)
    })
}

/// El archivo del ícono `nombre` para `tam` lógicos a `escala`. Un simbólico que el tema no
/// tiene se busca en su versión de color.
pub fn buscar(nombre: &str, tam: u32, escala: u32) -> Option<PathBuf> {
    con_cache(|c| {
        let clave = (nombre.to_owned(), tam, escala);
        if let Some(r) = c.rutas.get(&clave) {
            return r.clone();
        }
        let mut candidatos = vec![nombre];
        if let Some(sin) = nombre.strip_suffix("-symbolic") {
            candidatos.push(sin);
        }
        let r = candidatos.iter().find_map(|n| {
            c.cadena
                .iter()
                .find_map(|t| buscar_en(t, n, tam, escala))
                .or_else(|| en_pixmaps(n))
        });
        c.rutas.insert(clave, r.clone());
        r
    })
}

/// Un tema elegido por nombre, con carpetas propias además de las de siempre (el de la
/// bandeja, que pisa al del sistema para algunas apps).
pub struct TemaPropio {
    cadena: Vec<Tema>,
}

impl TemaPropio {
    pub fn nuevo(nombre: &str, carpetas: &[PathBuf]) -> TemaPropio {
        let mut b = carpetas.to_vec();
        b.extend(bases());
        TemaPropio {
            cadena: cadena_en(nombre, &b),
        }
    }

    pub fn buscar(&self, nombre: &str, tam: u32, escala: u32) -> Option<PathBuf> {
        self.cadena
            .iter()
            .find_map(|t| buscar_en(t, nombre, tam, escala))
    }
}

/// Un SVG con `var(--…)` es de otra app (ver arriba): no se dibuja.
fn svg_usable(datos: &[u8]) -> bool {
    !datos.windows(4).any(|w| w == b"var(")
}

/// `ruta` (un SVG o una imagen) en un cuadrado de `lado` píxeles, centrada y con su
/// proporción; con `color`, como simbólico (solo cuenta su alfa).
pub fn dibujar(ruta: &Path, lado: u32, color: Option<Color>) -> Option<ImageData> {
    let lado = lado.max(1);
    let es_svg = ruta.extension().is_some_and(|e| e == "svg" || e == "svgz");
    let mut img = if es_svg {
        let datos = std::fs::read(ruta).ok()?;
        if !svg_usable(&datos) {
            return None;
        }
        svg(&datos, lado)?
    } else {
        let i = image::open(ruta).ok()?.into_rgba8();
        cuadrado(&i, lado)
    };
    if let Some(c) = color {
        teñir(&mut img, c);
    }
    Some(imagen(img, lado, lado))
}

/// El ícono `nombre` a `tam` lógicos y `escala`, en píxeles; los simbólicos, de `color`. Un
/// SVG que no se dibuja cae al PNG de `/usr/share/pixmaps`.
pub fn de_nombre(nombre: &str, tam: u32, escala: u32, color: Color) -> Option<ImageData> {
    let lado = tam * escala;
    let simbolico = nombre.ends_with("-symbolic");
    let ruta = buscar(nombre, tam, escala)?;
    let tinte = simbolico.then_some(color);
    dibujar(&ruta, lado, tinte).or_else(|| {
        let png = Path::new(PIXMAPS).join(format!("{nombre}.png"));
        dibujar(&png, lado, tinte)
    })
}

/// El ícono de una app (`GIcon` de su `.desktop`): por nombre del tema o un archivo.
pub fn de_gicon(icono: Option<&gio::Icon>, tam: u32, escala: u32) -> Option<ImageData> {
    let blanco = [1.0; 4];
    let Some(i) = icono else {
        return de_nombre("application-x-executable", tam, escala, blanco);
    };
    if let Some(t) = i.downcast_ref::<gio::ThemedIcon>() {
        return t
            .names()
            .iter()
            .find_map(|n| de_nombre(n, tam, escala, blanco))
            .or_else(|| de_nombre("application-x-executable", tam, escala, blanco));
    }
    if let Some(f) = i.downcast_ref::<gio::FileIcon>()
        && let Some(p) = f.file().path()
    {
        return dibujar(&p, tam * escala, None);
    }
    de_nombre("application-x-executable", tam, escala, blanco)
}

/// Premultiplicado, como lo deja tiny-skia.
fn svg(datos: &[u8], lado: u32) -> Option<Vec<u8>> {
    let arbol = resvg::usvg::Tree::from_data(datos, &resvg::usvg::Options::default()).ok()?;
    let mut px = resvg::tiny_skia::Pixmap::new(lado, lado)?;
    let s = arbol.size();
    let k = lado as f32 / s.width().max(s.height());
    let (dx, dy) = (
        (lado as f32 - s.width() * k) / 2.0,
        (lado as f32 - s.height() * k) / 2.0,
    );
    resvg::render(
        &arbol,
        resvg::tiny_skia::Transform::from_row(k, 0.0, 0.0, k, dx, dy),
        &mut px.as_mut(),
    );
    Some(px.take())
}

/// Píxeles RGBA (los de una app de la bandeja) en un cuadrado de `lado`, como un ícono.
pub fn de_rgba(i: &image::RgbaImage, lado: u32) -> ImageData {
    imagen(cuadrado(i, lado.max(1)), lado.max(1), lado.max(1))
}

/// Píxeles RGBA ya premultiplicados, `w` por `h`, tal cual.
pub fn de_pixeles(px: Vec<u8>, w: u32, h: u32) -> ImageData {
    imagen(px, w, h)
}

/// Una imagen con su proporción y su tamaño, premultiplicada.
pub fn de_imagen(i: &image::RgbaImage) -> ImageData {
    imagen(premultiplicar(i.clone().into_raw()), i.width(), i.height())
}

/// Si el tema tiene el ícono (uno de los nombres, si es del tema; los archivos siempre).
pub fn tiene_gicon(i: &gio::Icon) -> bool {
    match i.downcast_ref::<gio::ThemedIcon>() {
        Some(t) => t.names().iter().any(|n| buscar(n, 48, 1).is_some()),
        None => true,
    }
}

/// Si un archivo es de un ícono simbólico (se pinta de un color).
pub fn es_simbolico(ruta: &Path) -> bool {
    ruta.file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.ends_with("-symbolic"))
}

/// Una imagen en un cuadrado de `lado`, con su proporción, centrada; premultiplicada.
fn cuadrado(i: &image::RgbaImage, lado: u32) -> Vec<u8> {
    let (w, h) = i.dimensions();
    let k = lado as f64 / f64::from(w.max(h).max(1));
    let (nw, nh) = (
        ((f64::from(w) * k).round() as u32).clamp(1, lado),
        ((f64::from(h) * k).round() as u32).clamp(1, lado),
    );
    let chica = if (nw, nh) == (w, h) {
        i.clone()
    } else {
        image::imageops::resize(i, nw, nh, image::imageops::FilterType::CatmullRom)
    };
    let mut lienzo = image::RgbaImage::new(lado, lado);
    image::imageops::overlay(
        &mut lienzo,
        &chica,
        i64::from((lado - nw) / 2),
        i64::from((lado - nh) / 2),
    );
    premultiplicar(lienzo.into_raw())
}

fn premultiplicar(mut px: Vec<u8>) -> Vec<u8> {
    for p in px.as_chunks_mut::<4>().0 {
        let a = u16::from(p[3]);
        for c in &mut p[..3] {
            *c = ((u16::from(*c) * a + 127) / 255) as u8;
        }
    }
    px
}

/// Un simbólico: solo cuenta su alfa, y se pinta de `c` (premultiplicado).
fn teñir(px: &mut [u8], c: Color) {
    let b = |v: f32| v.clamp(0.0, 1.0);
    for p in px.as_chunks_mut::<4>().0 {
        let a = f32::from(p[3]) / 255.0 * b(c[3]);
        p[0] = (b(c[0]) * a * 255.0).round() as u8;
        p[1] = (b(c[1]) * a * 255.0).round() as u8;
        p[2] = (b(c[2]) * a * 255.0).round() as u8;
        p[3] = (a * 255.0).round() as u8;
    }
}

fn imagen(px: Vec<u8>, w: u32, h: u32) -> ImageData {
    ImageData {
        data: Blob::new(Arc::new(px)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::AlphaPremultiplied,
        width: w,
        height: h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDICE: &str = "\
[Icon Theme]
Name=Prueba
Inherits=breeze-dark,hicolor
Directories=16x16/apps,48x48/apps,scalable/apps
ScaledDirectories=24x24@2x/apps

[16x16/apps]
Size=16
Type=Fixed

[48x48/apps]
Size=48

[scalable/apps]
Size=64
MinSize=8
MaxSize=512
Type=Scalable

[24x24@2x/apps]
Size=24
Scale=2
Type=Fixed
";

    #[test]
    fn lee_el_indice() {
        let t = leer_indice(INDICE);
        assert_eq!(t.hereda, ["breeze-dark", "hicolor"]);
        assert_eq!(t.carpetas.len(), 4);
        assert_eq!(t.carpetas[1].tipo, Tipo::Umbral);
        assert_eq!(t.carpetas[1].umbral, 2);
        assert_eq!(t.carpetas[3].escala, 2);
    }

    #[test]
    fn tamanos_como_la_especificacion() {
        let t = leer_indice(INDICE);
        let (fijo, umbral, escalable, doble) = (
            &t.carpetas[0],
            &t.carpetas[1],
            &t.carpetas[2],
            &t.carpetas[3],
        );
        assert!(fijo.coincide(16, 1) && !fijo.coincide(17, 1));
        assert!(umbral.coincide(46, 1) && umbral.coincide(50, 1) && !umbral.coincide(51, 1));
        assert!(escalable.coincide(24, 1) && !escalable.coincide(24, 2));
        assert!(doble.coincide(24, 2) && !doble.coincide(24, 1));
        assert_eq!(fijo.distancia(24, 1), 8);
        assert_eq!(escalable.distancia(4, 1), 4);
        assert_eq!(umbral.distancia(24, 1), 22);
    }

    #[test]
    fn el_tema_de_settings_ini() {
        let t = "[Settings]\ngtk-font-name=Inter 11\ngtk-icon-theme-name = Papirus-Dark\n";
        assert_eq!(tema_del_ini(t).as_deref(), Some("Papirus-Dark"));
        assert_eq!(tema_del_ini("[Settings]\n"), None);
    }

    #[test]
    fn un_svg_con_variables_no_se_dibuja() {
        assert!(!svg_usable(b"<path fill=\"var(--x, #fff)\"/>"));
        assert!(svg_usable(b"<path fill=\"#fff\"/>"));
    }

    #[test]
    fn el_simbolico_toma_el_color() {
        let mut px = vec![9, 9, 9, 255, 0, 0, 0, 0, 1, 2, 3, 128];
        teñir(&mut px, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(&px[..4], [255, 0, 0, 255]);
        assert_eq!(&px[4..8], [0, 0, 0, 0]);
        assert_eq!(&px[8..], [128, 0, 0, 128]);
    }

    #[test]
    fn una_imagen_angosta_queda_centrada() {
        let i = image::RgbaImage::from_pixel(2, 4, image::Rgba([10, 20, 30, 255]));
        let px = cuadrado(&i, 4);
        // La columna 0 queda vacía; la 1, pintada.
        assert_eq!(px[3], 0);
        assert_eq!(px[4 + 3], 255);
    }
}
