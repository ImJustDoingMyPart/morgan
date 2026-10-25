
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// La crítica no vence.
pub const DURACION_NORMAL: Duration = Duration::from_secs(10);
pub const DURACION_BAJA: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Urgencia {
    Baja,
    Normal,
    Critica,
}

impl Urgencia {
    pub fn de(b: u8) -> Self {
        match b {
            0 => Urgencia::Baja,
            2 => Urgencia::Critica,
            _ => Urgencia::Normal,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pixeles {
    pub ancho: i32,
    pub alto: i32,
    pub fila: i32,
    pub alfa: bool,
    pub canales: i32,
    pub datos: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Imagen {
    Nombre(String),
    Archivo(PathBuf),
    /// No va a disco tal cual: el historial la guarda como PNG y la cambia por `Archivo`.
    #[serde(skip)]
    Datos(Pixeles),
}

impl Imagen {
    pub fn de_texto(s: &str) -> Option<Imagen> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        if let Some(ruta) = s.strip_prefix("file://") {
            let ruta = glib::uri_unescape_string(ruta, None::<&str>)
                .map(|r| r.to_string())
                .unwrap_or_else(|| ruta.to_owned());
            return Some(Imagen::Archivo(ruta.into()));
        }
        if s.starts_with('/') {
            return Some(Imagen::Archivo(s.into()));
        }
        Some(Imagen::Nombre(s.to_owned()))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Accion {
    pub clave: String,
    pub etiqueta: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notificacion {
    pub id: u32,
    pub app: String,
    #[serde(default)]
    pub entrada: Option<String>,
    #[serde(default)]
    pub nombre_app: Option<String>,
    #[serde(default)]
    pub icono_app: Option<Imagen>,
    pub resumen: String,
    pub cuerpo: String,
    #[serde(default)]
    pub acciones: Vec<Accion>,
    pub urgencia: Urgencia,
    #[serde(default)]
    pub categoria: Option<String>,
    #[serde(default)]
    pub imagen: Option<Imagen>,
    #[serde(default)]
    pub residente: bool,
    #[serde(default)]
    pub transitoria: bool,
    #[serde(default)]
    pub progreso: Option<i32>,
    pub vence_ms: i32,
    pub llegada: i64,
    /// Solo cuenta al llegar: no se guarda.
    #[serde(skip)]
    pub sonido: PedidoSonido,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum PedidoSonido {
    #[default]
    Normal,
    Callar,
    Archivo(String),
    Nombre(String),
}

/// No suena un reemplazo: una barra de progreso sonaría en cada avance.
pub fn sonido(
    n: &Notificacion,
    destino: Destino,
    reemplaza: bool,
    c: &crate::config::Notificaciones,
) -> Option<(bool, String)> {
    if destino != Destino::Flota || reemplaza || !c.sonido || c.sin_sonido(&n.app) {
        return None;
    }
    match &n.sonido {
        PedidoSonido::Callar => None,
        PedidoSonido::Archivo(f) => Some((true, f.clone())),
        PedidoSonido::Nombre(s) => Some((false, s.clone())),
        PedidoSonido::Normal if n.urgencia == Urgencia::Critica => {
            Some((false, "dialog-warning".into()))
        }
        PedidoSonido::Normal => Some((false, "message-new-instant".into())),
    }
}

/// Un `replaces_id` vale solo sobre una notificación de la misma app: otra no puede pisarla
/// adivinando el número (lo mismo hace `wayle-notification`).
pub fn reemplaza(lista: &[Notificacion], n: &Notificacion) -> bool {
    n.id != 0 && lista.iter().any(|x| x.id == n.id && x.app == n.app)
}

/// Llega con id propio y no como reemplazo, porque el emisor guarda ese id para sus acciones.
pub fn desplazadas(lista: &[Notificacion], n: &Notificacion) -> Vec<u32> {
    lista
        .iter()
        .filter(|x| x.id != n.id && x.app.eq_ignore_ascii_case(&n.app) && x.resumen == n.resumen)
        .map(|x| x.id)
        .collect()
}

impl Notificacion {
    pub fn grupo(&self) -> &str {
        self.entrada.as_deref().unwrap_or(&self.app)
    }

    pub fn nombre_visible(&self) -> &str {
        self.nombre_app.as_deref().unwrap_or(&self.app)
    }

    pub fn entradas(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for base in self.entrada.iter().chain(std::iter::once(&self.app)) {
            let base = base.strip_suffix(".desktop").unwrap_or(base);
            for e in [base.to_owned(), base.to_lowercase()] {
                if !e.is_empty() && !v.contains(&e) {
                    v.push(e);
                }
            }
        }
        v
    }

    pub fn titulo(&self) -> &str {
        if self.resumen.trim().is_empty() {
            &self.app
        } else {
            &self.resumen
        }
    }

    pub fn botones(&self) -> impl Iterator<Item = &Accion> {
        self.acciones.iter().filter(|a| a.clave != "default")
    }
}

/// 268 px de JetBrains Mono a 14 px, que avanza 0,6 em por carácter.
const COLUMNAS_TOAST: usize = 31;

/// La letra es monoespaciada: contar caracteres alcanza, sin medir nada.
pub fn acciones_en_fila<'a>(acciones: impl IntoIterator<Item = (bool, &'a str)>) -> bool {
    let columnas: usize = acciones
        .into_iter()
        .map(|(glifo, texto)| texto.chars().count() + 2 + if glifo { 2 } else { 0 })
        .sum();
    columnas <= COLUMNAS_TOAST
}

/// Los íconos y las tapas de disco son cuadrados; una captura o una foto, no.
pub fn es_vista_previa(ancho: i32, alto: i32) -> bool {
    let (mayor, menor) = (ancho.max(alto), ancho.min(alto));
    if menor <= 0 {
        return false;
    }
    let cuadrada = (mayor - menor) * 10 <= mayor;
    (!cuadrada && mayor >= 96) || mayor > 512
}

/// Una crítica no vence nunca, ni aunque el emisor pida un tiempo; 0 es el de su urgencia.
pub fn duracion(n: &Notificacion) -> Option<Duration> {
    match (n.urgencia, n.vence_ms) {
        (Urgencia::Critica, _) => None,
        (_, ms) if ms > 0 => Some(Duration::from_millis(ms as u64)),
        (Urgencia::Baja, _) => Some(DURACION_BAJA),
        _ => Some(DURACION_NORMAL),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destino {
    Flota,
    Calla,
    Juego,
}

/// El juego gana a todo: ninguna capa se abre sobre él.
pub fn destino(n: &Notificacion, dnd: bool, juego: bool, silenciada: bool) -> Destino {
    if silenciada {
        Destino::Calla
    } else if juego {
        Destino::Juego
    } else if dnd && n.urgencia != Urgencia::Critica {
        Destino::Calla
    } else {
        Destino::Flota
    }
}

/// En orden de llegada: el título es de una línea y no entra todo.
pub fn resumen_juego(apps: &[String]) -> Option<(String, String)> {
    if apps.is_empty() {
        return None;
    }
    let mut cuentas: Vec<(&str, usize)> = Vec::new();
    for a in apps {
        match cuentas.iter_mut().find(|(n, _)| n == a) {
            Some((_, c)) => *c += 1,
            None => cuentas.push((a, 1)),
        }
    }
    let titulo = match apps.len() {
        1 => "1 notificación".to_owned(),
        n => format!("{n} notificaciones"),
    };
    let quienes = cuentas
        .iter()
        .map(|(a, c)| {
            if *c > 1 {
                format!("{a} ({c})")
            } else {
                (*a).to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some((titulo, format!("mientras jugabas: {quienes}")))
}

/// Los que tienen una crítica primero, después la más reciente; en un empate, el que apareció
/// antes.
pub fn grupos(lista: &[Notificacion]) -> Vec<Vec<u32>> {
    struct Grupo<'a> {
        clave: &'a str,
        ids: Vec<u32>,
        ultima: i64,
        critica: bool,
    }
    let mut grupos: Vec<Grupo> = Vec::new();
    for n in lista {
        let g = match grupos.iter_mut().position(|g| g.clave == n.grupo()) {
            Some(i) => &mut grupos[i],
            None => {
                grupos.push(Grupo {
                    clave: n.grupo(),
                    ids: Vec::new(),
                    ultima: n.llegada,
                    critica: false,
                });
                grupos.last_mut().expect("recién agregado")
            }
        };
        g.ids.insert(0, n.id);
        g.ultima = g.ultima.max(n.llegada);
        g.critica |= n.urgencia == Urgencia::Critica;
    }
    // Estable: los empates quedan en el orden en que aparecieron.
    grupos.sort_by(|a, b| b.critica.cmp(&a.critica).then(b.ultima.cmp(&a.ultima)));
    grupos.into_iter().map(|g| g.ids).collect()
}

pub fn vencidas(lista: &[Notificacion], ahora: i64, dias: u32) -> Vec<u32> {
    let limite = ahora - i64::from(dias) * 86_400;
    lista
        .iter()
        .filter(|n| n.llegada < limite)
        .map(|n| n.id)
        .collect()
}

pub fn relativo(segundos: i64) -> String {
    let min = segundos / 60;
    let horas = min / 60;
    let dias = horas / 24;
    match () {
        _ if min < 1 => "ahora".into(),
        _ if horas < 1 => format!("hace {min} min"),
        _ if dias < 1 => format!("hace {horas} h"),
        _ if dias == 1 => "hace 1 día".into(),
        _ => format!("hace {dias} días"),
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Guardado {
    #[serde(default)]
    pub no_molestar: bool,
    /// Los nuevos siguen de ahí: un emisor que sobrevivió al reinicio de morgan no confunde una
    /// vieja con una nueva.
    #[serde(default)]
    pub ultimo_id: u32,
    #[serde(default)]
    pub lista: Vec<Notificacion>,
}

#[cfg(test)]
pub fn prueba(id: u32, app: &str) -> Notificacion {
    Notificacion {
        id,
        app: app.into(),
        entrada: None,
        nombre_app: None,
        icono_app: None,
        resumen: format!("n{id}"),
        cuerpo: String::new(),
        acciones: Vec::new(),
        urgencia: Urgencia::Normal,
        categoria: None,
        imagen: None,
        residente: false,
        transitoria: false,
        progreso: None,
        vence_ms: -1,
        llegada: 0,
        sonido: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn que_suena() {
        use super::*;
        let mut c = crate::config::Notificaciones::default();
        let mut n = prueba(1, "Discord");
        let suena = |n: &Notificacion, d, r, c: &crate::config::Notificaciones| sonido(n, d, r, c);
        assert_eq!(
            suena(&n, Destino::Flota, false, &c),
            Some((false, "message-new-instant".into()))
        );
        // Lo que no flota, un reemplazo o el sonido apagado: nada.
        assert_eq!(suena(&n, Destino::Calla, false, &c), None);
        assert_eq!(suena(&n, Destino::Juego, false, &c), None);
        assert_eq!(suena(&n, Destino::Flota, true, &c), None);
        n.urgencia = Urgencia::Critica;
        assert_eq!(
            suena(&n, Destino::Flota, false, &c),
            Some((false, "dialog-warning".into()))
        );
        n.sonido = PedidoSonido::Archivo("/x.oga".into());
        assert_eq!(
            suena(&n, Destino::Flota, false, &c),
            Some((true, "/x.oga".into()))
        );
        n.sonido = PedidoSonido::Callar;
        assert_eq!(suena(&n, Destino::Flota, false, &c), None);
        n.sonido = PedidoSonido::Normal;
        c.sin_sonido = vec!["discord".into()];
        assert_eq!(suena(&n, Destino::Flota, false, &c), None);
        c.sin_sonido.clear();
        c.sonido = false;
        assert_eq!(suena(&n, Destino::Flota, false, &c), None);
    }

    #[test]
    fn las_acciones_van_en_fila_si_entran_enteras() {
        use super::acciones_en_fila as fila;
        // La captura, con sus glifos.
        assert!(fila([(true, "Abrir"), (true, "Carpeta"), (true, "Borrar")]));
        // Cachy-Update, sin glifos.
        assert!(fila([(false, "Run Cachy-Update"), (false, "Close")]));
        // Tres largas: en columna.
        assert!(!fila([
            (false, "Unirse a la llamada"),
            (false, "Posponer 10 minutos"),
            (false, "Descartar"),
        ]));
        // Una sola que no entra tampoco va en fila: en columna tiene todo el ancho.
        assert!(!fila([(
            false,
            "Abrir la página de descargas del navegador"
        )]));
        // Los caracteres, no los bytes.
        assert!(fila([(false, "Ñandú áéíóú ñandú áéíóú ñ")]));
    }

    #[test]
    fn vista_previa_solo_para_lo_que_no_es_icono() {
        use super::es_vista_previa as vp;
        // Capturas y fotos.
        assert!(vp(1920, 1080));
        assert!(vp(400, 900));
        assert!(vp(800, 800));
        // Íconos, tapas de disco, imágenes chicas o rotas.
        assert!(!vp(48, 48));
        assert!(!vp(256, 256));
        assert!(!vp(512, 512));
        assert!(!vp(300, 290));
        assert!(!vp(80, 40));
        assert!(!vp(0, 0));
    }

    use super::*;

    #[test]
    fn duracion_por_urgencia_y_por_emisor() {
        let mut n = prueba(1, "a");
        assert_eq!(duracion(&n), Some(DURACION_NORMAL));
        n.urgencia = Urgencia::Baja;
        assert_eq!(duracion(&n), Some(DURACION_BAJA));
        n.vence_ms = 3000;
        assert_eq!(duracion(&n), Some(Duration::from_secs(3)));
        // 0 no es "para siempre": el de la urgencia.
        n.vence_ms = 0;
        assert_eq!(duracion(&n), Some(DURACION_BAJA));
        // La crítica no vence, pida lo que pida el emisor.
        n.urgencia = Urgencia::Critica;
        assert_eq!(duracion(&n), None);
        n.vence_ms = 3000;
        assert_eq!(duracion(&n), None);
    }

    #[test]
    fn el_juego_gana_a_todo() {
        let mut n = prueba(1, "a");
        n.urgencia = Urgencia::Critica;
        assert_eq!(destino(&n, false, true, false), Destino::Juego);
        assert_eq!(destino(&n, true, true, false), Destino::Juego);
        // Silenciada no cuenta ni para el resumen.
        assert_eq!(destino(&n, false, true, true), Destino::Calla);
    }

    #[test]
    fn no_molestar_deja_pasar_las_criticas() {
        let mut n = prueba(1, "a");
        assert_eq!(destino(&n, true, false, false), Destino::Calla);
        assert_eq!(destino(&n, false, false, false), Destino::Flota);
        n.urgencia = Urgencia::Critica;
        assert_eq!(destino(&n, true, false, false), Destino::Flota);
        assert_eq!(destino(&n, true, false, true), Destino::Calla);
    }

    #[test]
    fn resumen_del_juego() {
        assert_eq!(resumen_juego(&[]), None);
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            resumen_juego(&a(&["Discord"])),
            Some(("1 notificación".into(), "mientras jugabas: Discord".into()))
        );
        assert_eq!(
            resumen_juego(&a(&["Discord", "Steam", "Discord"])),
            Some((
                "3 notificaciones".into(),
                "mientras jugabas: Discord (2), Steam".into()
            ))
        );
    }

    #[test]
    fn grupos_por_app_el_mas_nuevo_primero() {
        let en = |id, app, t| {
            let mut n = prueba(id, app);
            n.llegada = t;
            n
        };
        let lista = vec![
            en(1, "a", 10),
            en(2, "b", 20),
            en(3, "a", 30),
            en(4, "c", 40),
            en(5, "b", 50),
        ];
        assert_eq!(grupos(&lista), vec![vec![5, 2], vec![4], vec![3, 1]]);
        assert!(grupos(&[]).is_empty());
    }

    #[test]
    fn grupos_en_el_mismo_segundo_quedan_en_el_orden_en_que_aparecieron() {
        // Discord (dos) y Steam en el mismo segundo: Discord arriba.
        let lista = vec![
            prueba(1, "Discord"),
            prueba(2, "Discord"),
            prueba(3, "Steam"),
        ];
        assert_eq!(grupos(&lista), vec![vec![2, 1], vec![3]]);
    }

    #[test]
    fn las_criticas_van_primero() {
        let mut vieja = prueba(1, "a");
        vieja.urgencia = Urgencia::Critica;
        let mut nueva = prueba(2, "b");
        nueva.llegada = 100;
        assert_eq!(grupos(&[vieja, nueva]), vec![vec![1], vec![2]]);
    }

    #[test]
    fn retencion() {
        let mut vieja = prueba(1, "a");
        vieja.llegada = 1_000_000 - 8 * 86_400;
        let mut nueva = prueba(2, "a");
        nueva.llegada = 1_000_000 - 6 * 86_400;
        assert_eq!(vencidas(&[vieja, nueva], 1_000_000, 7), vec![1]);
    }

    #[test]
    fn tiempo_relativo() {
        assert_eq!(relativo(0), "ahora");
        assert_eq!(relativo(59), "ahora");
        assert_eq!(relativo(60), "hace 1 min");
        assert_eq!(relativo(3599), "hace 59 min");
        assert_eq!(relativo(3600), "hace 1 h");
        assert_eq!(relativo(86_399), "hace 23 h");
        assert_eq!(relativo(86_400), "hace 1 día");
        assert_eq!(relativo(3 * 86_400), "hace 3 días");
        // Un reloj que volvió para atrás no da negativos.
        assert_eq!(relativo(-30), "ahora");
    }

    #[test]
    fn imagen_de_texto() {
        assert_eq!(Imagen::de_texto(""), None);
        assert_eq!(
            Imagen::de_texto("dialog-information"),
            Some(Imagen::Nombre("dialog-information".into()))
        );
        assert_eq!(
            Imagen::de_texto("/tmp/a.png"),
            Some(Imagen::Archivo("/tmp/a.png".into()))
        );
        assert_eq!(
            Imagen::de_texto("file:///tmp/con%20espacio.png"),
            Some(Imagen::Archivo("/tmp/con espacio.png".into()))
        );
    }

    #[test]
    fn entradas_de_escritorio() {
        let mut n = prueba(1, "Steam");
        assert_eq!(n.entradas(), ["Steam", "steam"]);
        n.entrada = Some("org.telegram.desktop.desktop".into());
        assert_eq!(n.entradas(), ["org.telegram.desktop", "Steam", "steam"]);
        assert_eq!(n.grupo(), "org.telegram.desktop.desktop");
    }

    #[test]
    fn titulo_y_acciones() {
        let mut n = prueba(1, "Firefox");
        n.resumen = "  ".into();
        assert_eq!(n.titulo(), "Firefox");
        n.acciones = vec![
            Accion {
                clave: "default".into(),
                etiqueta: "Abrir".into(),
            },
            Accion {
                clave: "x".into(),
                etiqueta: "Otra".into(),
            },
        ];
        assert_eq!(
            n.botones().map(|a| a.clave.as_str()).collect::<Vec<_>>(),
            ["x"]
        );
    }

    #[test]
    fn el_guardado_va_y_vuelve() {
        let mut n = prueba(7, "a");
        n.imagen = Some(Imagen::Archivo("/x.png".into()));
        n.acciones = vec![Accion {
            clave: "k".into(),
            etiqueta: "e".into(),
        }];
        let g = Guardado {
            no_molestar: true,
            ultimo_id: 9,
            lista: vec![n.clone()],
        };
        let texto = serde_json::to_string(&g).unwrap();
        let g2: Guardado = serde_json::from_str(&texto).unwrap();
        assert!(g2.no_molestar);
        assert_eq!(g2.ultimo_id, 9);
        assert_eq!(g2.lista, vec![n]);
        // Un archivo vacío o de una versión vieja sin campos es un historial vacío.
        let g3: Guardado = serde_json::from_str("{}").unwrap();
        assert!(g3.lista.is_empty());
    }

    #[test]
    fn una_sola_desplaza_misma_app_y_titulo() {
        use super::*;
        let con = |id, app: &str, t: &str| Notificacion {
            resumen: t.into(),
            ..prueba(id, app)
        };
        let lista = vec![
            con(1, "kitty", "Claude Code"),
            con(2, "kitty", "Gemini"),
            con(3, "Kitty", "Claude Code"),
            con(4, "vesktop", "Claude Code"),
        ];
        assert_eq!(desplazadas(&lista, &con(9, "kitty", "Claude Code")), [1, 3]);
        // Un reemplazo por id no se desplaza a sí mismo.
        assert_eq!(desplazadas(&lista, &con(3, "kitty", "Claude Code")), [1]);
        assert!(desplazadas(&lista, &con(9, "kitty", "Codex")).is_empty());
    }

    #[test]
    fn solo_la_misma_app_reemplaza() {
        let lista = vec![prueba(7, "kitty")];
        assert!(reemplaza(&lista, &prueba(7, "kitty")));
        assert!(!reemplaza(&lista, &prueba(7, "firefox")));
        assert!(!reemplaza(&lista, &prueba(8, "kitty")));
        assert!(!reemplaza(&lista, &prueba(0, "kitty")));
    }
}
