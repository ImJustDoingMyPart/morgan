//! Lo que el menú marca como activo sale siempre de lo que se lee o de la configuración ya
//! releída, nunca del click.

use std::rc::Rc;

use super::cursores::Muestra;
use crate::config::Config;
use crate::launcher::busqueda::{self, Entrada, Orden};
use crate::sonidos::{MUESTRAS, TemaSonido};
use morgan_idioma as idioma;
use morgan_ipc::bloqueo::{Bloqueo, Escritura, Verificar};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Categoria {
    Apariencia,
    Shell,
    Compositor,
    Atajos,
    Pantalla,
    Notificaciones,
    Sonido,
    Aplicaciones,
    Sistema,
}

impl Categoria {
    pub const TODAS: [Categoria; 9] = [
        Categoria::Apariencia,
        Categoria::Shell,
        Categoria::Compositor,
        Categoria::Atajos,
        Categoria::Pantalla,
        Categoria::Notificaciones,
        Categoria::Sonido,
        Categoria::Aplicaciones,
        Categoria::Sistema,
    ];

    pub fn titulo(self) -> &'static str {
        match self {
            Categoria::Apariencia => tr!("Apariencia"),
            Categoria::Shell => tr!("Shell"),
            Categoria::Compositor => tr!("Compositor"),
            Categoria::Atajos => tr!("Atajos"),
            Categoria::Aplicaciones => tr!("Aplicaciones"),
            Categoria::Sistema => tr!("Sistema"),
            Categoria::Sonido => tr!("Sonido"),
            Categoria::Pantalla => tr!("Pantalla y energía"),
            Categoria::Notificaciones => tr!("Notificaciones"),
        }
    }

    pub fn glifo(self) -> char {
        let c = match self {
            Categoria::Apariencia => 0xF03D8,
            Categoria::Shell => 0xF018D,
            Categoria::Compositor => 0xF05B7,
            Categoria::Atajos => 0xF030C,
            Categoria::Aplicaciones => 0xF003B,
            Categoria::Sistema => 0xF02FD,
            Categoria::Sonido => 0xF057E,
            Categoria::Pantalla => 0xF0379,
            Categoria::Notificaciones => 0xF009A,
        };
        char::from_u32(c).unwrap_or('?')
    }

    pub fn secciones(self) -> &'static [&'static str] {
        match self {
            Categoria::Apariencia => &[
                tr_const!("Tema y colores"),
                tr_const!("Fondos"),
                tr_const!("Fuentes"),
                tr_const!("Íconos y cursor"),
                tr_const!("Animaciones"),
                tr_const!("Sombras"),
            ],
            Categoria::Shell => &[tr_const!("General"), tr_const!("Pantalla de bloqueo")],
            Categoria::Compositor => &[
                tr_const!("Ventanas"),
                tr_const!("Desenfoque"),
                tr_const!("Distribución"),
                tr_const!("Grupos"),
                tr_const!("Foco y cursor"),
                tr_const!("Teclado"),
                tr_const!("Mouse y touchpad"),
            ],
            Categoria::Atajos => super::atajos::secciones(),
            Categoria::Aplicaciones => &[tr_const!("Predeterminadas"), tr_const!("Autoarranque"), tr_const!("Capturas de pantalla")],
            Categoria::Sistema => &[tr_const!("Equipo"), tr_const!("Snapshots"), tr_const!("Privacidad")],
            Categoria::Sonido => &[tr_const!("Volumen"), tr_const!("Salidas"), tr_const!("Tema"), tr_const!("Sonidos del sistema")],
            Categoria::Pantalla => &[tr_const!("Monitores"), tr_const!("Luz nocturna"), tr_const!("Inactividad")],
            Categoria::Notificaciones => &[tr_const!("Generales"), tr_const!("Modo juego"), tr_const!("Por app")],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Que {
    Aplicar(Vec<String>),
    Fondos,
    Ubicacion,
    Calendario,
    Carpetas(std::path::PathBuf),
    Config(Cambio),
    Apariencia(crate::apariencia::Cambio),
    Hypr(String, Option<super::compositor::Valor>),
    Predeterminada(String, Vec<String>),
    Autoarranque(String, bool),
    BorrarRecientes,
    ElegirTemaSonido(String, String),
    Sonar(String),
    GrabarTecla(String),
    Monitor(String, super::monitores::Campo),
    MantenerMonitores,
    VolverMonitores,
}

impl Que {
    /// La acción de un [`Control::Texto`] con lo escrito.
    pub fn con_texto(&self, t: &str) -> Que {
        match self {
            Que::Monitor(s, super::monitores::Campo::Apodo(_)) => {
                Que::Monitor(s.clone(), super::monitores::Campo::Apodo(t.to_owned()))
            }
            otro => otro.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cambio {
    Opacidad(f64),
    Idioma(String),
    Bloqueo(CambioBloqueo),
    Retencion(u32),
    ModoApp(String, ModoApp),
    SonidoNotificaciones(bool),
    Paso(u32),
    Amplificar(bool),
    OcultarSalida(String, bool),
    SonidoSistema(SonidoSistema, bool),
    Temperatura(u32),
    Horario(bool),
    Animacion(String),
    Velocidad(String),
    Sombra(String),
    Ubicacion(crate::config::Clima),
    CarpetaFondos(String),
    RotarFondos(u32),
    PrenderLuz(String),
    ApagarLuz(String),
    MatugenTipo(String),
    MatugenModo(String),
    MatugenContraste(f64),
    Capturas(crate::config::Capturas),
    /// De una vez: reasignar una tecla de otro bind de morgan lo deja sin ella.
    Teclas(Vec<(String, String)>),
    Multimedia(bool),
    Resumen(bool),
    Apagar(u32),
    Bloquear(u32),
    Suspender(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CambioBloqueo {
    Escritura(Escritura),
    Chispa(bool),
    Verificar(Verificar),
    Sacudida(bool),
    Rojo(bool),
    Reaccion(bool),
    Destello(bool),
}

impl CambioBloqueo {
    fn aplicar(self, b: &mut Bloqueo) {
        match self {
            CambioBloqueo::Escritura(v) => b.escritura = v,
            CambioBloqueo::Chispa(v) => b.chispa = v,
            CambioBloqueo::Verificar(v) => b.verificar = v,
            CambioBloqueo::Sacudida(v) => b.sacudida = v,
            CambioBloqueo::Rojo(v) => b.rojo = v,
            CambioBloqueo::Reaccion(v) => b.reaccion = v,
            CambioBloqueo::Destello(v) => b.destello = v,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SonidoSistema {
    Volumen,
    Dispositivos,
    Red,
    Bloqueo,
    Grabacion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModoApp {
    Normal,
    SinSonido,
    UnaSola,
    Silenciada,
}

impl Cambio {
    pub fn aplicar(&self, c: &mut Config) {
        match self {
            Cambio::Opacidad(v) => c.shell.opacidad = *v,
            Cambio::Idioma(v) => c.shell.idioma = v.clone(),
            Cambio::Bloqueo(b) => b.aplicar(&mut c.bloqueo),
            Cambio::Retencion(d) => c.notificaciones.retencion_dias = *d,
            Cambio::Resumen(v) => c.juego.resumen = *v,
            Cambio::Apagar(m) => c.idle.apagar = *m,
            Cambio::Bloquear(m) => c.idle.bloquear = *m,
            Cambio::Suspender(m) => c.idle.suspender = *m,
            Cambio::SonidoNotificaciones(v) => c.notificaciones.sonido = *v,
            Cambio::Paso(p) => c.sonido.paso = *p,
            Cambio::Amplificar(v) => c.sonido.amplificar = *v,
            Cambio::OcultarSalida(nombre, ocultar) => {
                let o = &mut c.sonido.ocultas;
                o.retain(|s| s != nombre);
                if *ocultar {
                    o.push(nombre.clone());
                }
            }
            Cambio::SonidoSistema(q, v) => {
                let s = &mut c.sonido;
                *match q {
                    SonidoSistema::Volumen => &mut s.al_cambiar_volumen,
                    SonidoSistema::Dispositivos => &mut s.dispositivos,
                    SonidoSistema::Red => &mut s.red,
                    SonidoSistema::Bloqueo => &mut s.bloqueo,
                    SonidoSistema::Grabacion => &mut s.grabacion,
                } = *v;
            }
            Cambio::Temperatura(k) => c.luz.temperatura = *k,
            Cambio::Animacion(id) => c.apariencia.animacion = id.clone(),
            Cambio::Velocidad(id) => c.apariencia.velocidad = id.clone(),
            Cambio::Sombra(id) => c.apariencia.sombra = id.clone(),
            Cambio::Ubicacion(u) => c.clima = u.clone(),
            Cambio::CarpetaFondos(r) => c.fondos.carpeta = r.clone(),
            Cambio::RotarFondos(h) => c.fondos.rotar = *h,
            Cambio::Horario(v) => c.luz.horario = *v,
            Cambio::PrenderLuz(h) => c.luz.prender = h.clone(),
            Cambio::ApagarLuz(h) => c.luz.apagar = h.clone(),
            Cambio::MatugenTipo(t) => c.matugen.tipo = t.clone(),
            Cambio::MatugenModo(m) => c.matugen.modo = m.clone(),
            Cambio::MatugenContraste(v) => c.matugen.contraste = *v,
            Cambio::Capturas(v) => c.capturas = v.clone(),
            Cambio::Teclas(v) => {
                for (id, tecla) in v {
                    if let Some(t) = c.atajos.tecla_mut(id) {
                        *t = tecla.clone();
                    }
                }
            }
            Cambio::Multimedia(v) => c.atajos.multimedia = *v,
            Cambio::ModoApp(app, modo) => {
                let n = &mut c.notificaciones;
                n.silenciadas.retain(|a| !a.eq_ignore_ascii_case(app));
                n.sin_sonido.retain(|a| !a.eq_ignore_ascii_case(app));
                n.una_sola.retain(|a| !a.eq_ignore_ascii_case(app));
                match modo {
                    ModoApp::Normal => {}
                    ModoApp::SinSonido => n.sin_sonido.push(app.clone()),
                    ModoApp::UnaSola => n.una_sola.push(app.clone()),
                    ModoApp::Silenciada => n.silenciadas.push(app.clone()),
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Opcion {
    pub titulo: String,
    pub sub: String,
    pub activa: bool,
    pub que: Que,
    pub fuente: Option<String>,
    pub cursor: Option<Rc<[Muestra]>>,
}

pub fn opcion(titulo: &str, sub: &str, que: Que) -> Opcion {
    Opcion {
        titulo: titulo.into(),
        sub: sub.into(),
        activa: false,
        que,
        fuente: None,
        cursor: None,
    }
}

impl Opcion {
    pub fn activa(mut self, v: bool) -> Self {
        self.activa = v;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    Casilla { activa: bool, que: Que },
    Lista(Vec<Opcion>),
    Boton(Que),
    /// El primer toque pide confirmar, el segundo lo hace.
    Confirmar(Que),
    /// El toque abre la caja del buscador debajo de la fila; Enter guarda lo escrito con
    /// [`Que::con_texto`], Escape lo deja.
    Texto { valor: String, pista: String, que: Que },
    Nada,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Fila {
    Ajuste {
        titulo: String,
        sub: String,
        control: Control,
        nota: String,
    },
    Galeria {
        titulo: Option<String>,
        opciones: Vec<Opcion>,
    },
    /// Los monitores a escala, para ubicarlos arrastrándolos.
    Disposicion(Vec<super::monitores::Pantalla>),
}

pub fn ajuste(titulo: &str, sub: &str, control: Control) -> Fila {
    Fila::Ajuste {
        titulo: titulo.into(),
        sub: sub.into(),
        control,
        nota: String::new(),
    }
}

pub fn con_nota(fila: Fila, texto: &str) -> Fila {
    match fila {
        Fila::Ajuste {
            titulo,
            sub,
            control,
            ..
        } => Fila::Ajuste {
            titulo,
            sub,
            control,
            nota: texto.into(),
        },
        otra => otra,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Seccion {
    pub titulo: &'static str,
    pub filas: Vec<Fila>,
}

pub fn armar(cat: Categoria, filas: Vec<Vec<Fila>>) -> Vec<Seccion> {
    debug_assert_eq!(cat.secciones().len(), filas.len());
    cat.secciones()
        .iter()
        .zip(filas)
        .map(|(t, filas)| Seccion { titulo: idioma::t(t), filas })
        .collect()
}

pub fn pagina_fija(
    cat: Categoria,
    c: &Config,
    apps_historial: &[String],
) -> Option<Vec<Seccion>> {
    let filas = match cat {
        Categoria::Apariencia
        | Categoria::Compositor
        | Categoria::Atajos
        | Categoria::Aplicaciones
        | Categoria::Sonido
        | Categoria::Sistema => return None,
        Categoria::Shell => vec![
            vec![
                ajuste(
                    tr!("Opacidad"),
                    &trf!("fondo de las superficies · {}", porcentaje(c.shell.opacidad)),
                    Control::Lista(
                        OPACIDADES
                            .iter()
                            .map(|&v| {
                                opcion(&porcentaje(v), "", Que::Config(Cambio::Opacidad(v)))
                                    .activa((v - c.shell.opacidad).abs() < 0.001)
                            })
                            .collect(),
                    ),
                ),
                idioma_(&c.shell.idioma),
                ajuste(
                    tr!("Clima"),
                    &if c.clima.latitud.is_some() && !c.clima.lugar.is_empty() {
                        trf!("el pronóstico del calendario · {}", c.clima.lugar)
                    } else {
                        tr!("sin ubicación · el calendario no muestra el pronóstico").into()
                    },
                    Control::Boton(Que::Ubicacion),
                ),
                ajuste(
                    tr!("Calendario"),
                    &match crate::agenda::resumen_calendarios() {
                        Some(r) => trf!("los eventos del calendario · {}", r),
                        None => tr!("sin calendario · pegá la URL iCal secreta de Google").into(),
                    },
                    Control::Boton(Que::Calendario),
                ),
            ],
            bloqueo(&c.bloqueo),
        ],
        Categoria::Notificaciones => vec![
            vec![
                ajuste(
                    tr!("Retención del historial"),
                    &dias(c.notificaciones.retencion_dias),
                    Control::Lista(
                        RETENCIONES
                            .iter()
                            .map(|&d| {
                                opcion(&dias(d), "", Que::Config(Cambio::Retencion(d)))
                                    .activa(d == c.notificaciones.retencion_dias)
                            })
                            .collect(),
                    ),
                ),
                ajuste(
                    tr!("Sonido al llegar"),
                    tr!("el de mensaje nuevo del tema de sonidos; las críticas, el de advertencia"),
                    Control::Casilla {
                        activa: c.notificaciones.sonido,
                        que: Que::Config(Cambio::SonidoNotificaciones(!c.notificaciones.sonido)),
                    },
                ),
            ],
            juego(c),
            por_app(apps_historial, c),
        ],
        // Los monitores salen de Hyprland: los pone `lectura::monitores`.
        Categoria::Pantalla => vec![Vec::new(), luz(c), vec![
            ajuste(
                tr!("Apagar pantallas"),
                &trf!("sin actividad · {}", minutos(c.idle.apagar)),
                Control::Lista(opciones_idle(c.idle.apagar, Cambio::Apagar)),
            ),
            ajuste(
                tr!("Bloquear"),
                &trf!("sin actividad · {}", minutos(c.idle.bloquear)),
                Control::Lista(opciones_idle(c.idle.bloquear, Cambio::Bloquear)),
            ),
            ajuste(
                tr!("Suspender"),
                &trf!("sin actividad · {}", minutos(c.idle.suspender)),
                Control::Lista(
                    [0, 30, 60, 120]
                        .iter()
                        .map(|&m| {
                            opcion(&minutos(m), "", Que::Config(Cambio::Suspender(m)))
                                .activa(m == c.idle.suspender)
                        })
                        .collect(),
                ),
            ),
        ]],
    };
    Some(armar(cat, filas))
}

/// El modo juego es automático: lo único que se elige es qué pasa con lo que llegó mientras.
fn juego(c: &Config) -> Vec<Fila> {
    let r = c.juego.resumen;
    vec![ajuste(
        tr!("Resumen al volver"),
        tr!("al salir del juego, un solo aviso con lo que llegó mientras tanto"),
        Control::Casilla {
            activa: r,
            que: Que::Config(Cambio::Resumen(!r)),
        },
    )]
}

pub type MuestrasCursor = std::collections::HashMap<String, Rc<[Muestra]>>;

#[derive(Debug, Default)]
pub struct Lecturas {
    pub temas: Vec<String>,
    pub tema_activo: String,
    pub carpeta: std::path::PathBuf,
    pub carpeta_vista: String,
    pub duracion: String,
    pub fuente_sistema: String,
    pub fuentes_sistema: Vec<(String, String)>,
    pub fuente_terminal: String,
    pub fuentes_terminal: Vec<(String, String)>,
    pub iconos: Vec<String>,
    pub icono: String,
    pub cursores: Vec<String>,
    pub cursor: (String, u32),
    pub muestras_cursor: MuestrasCursor,
}

fn aplicar(args: &[&str]) -> Que {
    Que::Aplicar(args.iter().map(|a| (*a).to_owned()).collect())
}

pub fn apariencia(l: &Lecturas, c: &Config) -> Vec<Seccion> {
    let temas = l
        .temas
        .iter()
        .map(|n| {
            let sub = if n == "matugen" {
                tr!("generado del fondo")
            } else {
                tr!("curado")
            };
            opcion(&bonito(n), sub, aplicar(&["morganctl", "theme", "apply", n])).activa(*n == l.tema_activo)
        })
        .collect();

    let duracion = DURACIONES
        .iter()
        .find(|(v, _)| *v == l.duracion)
        .map_or(l.duracion.as_str(), |(_, t)| idioma::t(t));
    let fondos = vec![
        ajuste(
            tr!("Selector"),
            tr!("elegir entre los fondos de la carpeta · recalcula los colores"),
            Control::Boton(Que::Fondos),
        ),
        con_nota(
            ajuste(
                tr!("Carpeta"),
                &l.carpeta_vista,
                Control::Boton(Que::Carpetas(l.carpeta.clone())),
            ),
            tr!("con miles de imágenes, la primera vez las miniaturas aparecen de a poco"),
        ),
        ajuste(
            tr!("Cambio automático"),
            &trf!("cada cuánto cambia solo · {}", duracion.to_lowercase()),
            Control::Lista(
                DURACIONES
                    .iter()
                    .map(|(v, t)| {
                        let horas = v.parse().unwrap_or(0);
                        opcion(idioma::t(t), "", Que::Config(Cambio::RotarFondos(horas)))
                            .activa(*v == l.duracion)
                    })
                    .collect(),
            ),
        ),
    ];

    use crate::apariencia::{Ambito, Cambio as Ap};
    let fuentes = |ambito: Ambito, activa: &str, lista: &[(String, String)], titulo: &str| {
        Fila::Galeria {
            titulo: Some(titulo.into()),
            opciones: lista
                .iter()
                .map(|(f, sub)| {
                    let que = Que::Apariencia(Ap::Fuente(ambito, f.clone()));
                    let mut o = opcion(f, sub, que).activa(f == activa);
                    o.fuente = Some(f.clone());
                    o
                })
                .collect(),
        }
    };

    let iconos: Vec<Opcion> = l
        .iconos
        .iter()
        .map(|n| {
            let sub = if crate::apariencia::iconos::tiene_variantes(n) {
                tr!("sigue el color del tema")
            } else {
                tr!("sin variantes de color")
            };
            opcion(&bonito(n), sub, Que::Apariencia(Ap::Iconos(n.clone()))).activa(*n == l.icono)
        })
        .collect();
    let (cursor, tamano) = (&l.cursor.0, l.cursor.1);
    let mut cursores: Vec<Opcion> = l
        .cursores
        .iter()
        .map(|n| {
            opcion(&bonito(n), n, Que::Apariencia(Ap::Cursor(n.clone(), tamano)))
                .activa(n == cursor)
        })
        .collect();
    for (o, n) in cursores.iter_mut().zip(&l.cursores) {
        o.cursor = l.muestras_cursor.get(n).filter(|m| !m.is_empty()).cloned();
    }
    // Lo puesto que ya no existe se marca como el default, que es lo que vale.
    let animacion = crate::animaciones::por_id(&c.apariencia.animacion).id;
    let animaciones = crate::animaciones::COMPOSICIONES
        .iter()
        .map(|a| {
            opcion(idioma::t(a.nombre), idioma::t(a.descripcion), Que::Config(Cambio::Animacion(a.id.into())))
                .activa(a.id == animacion)
        })
        .collect();
    let velocidad = crate::animaciones::velocidad(&c.apariencia.velocidad).id;
    let velocidades = crate::animaciones::VELOCIDADES
        .iter()
        .map(|v| {
            opcion(idioma::t(v.nombre), idioma::t(v.descripcion), Que::Config(Cambio::Velocidad(v.id.into())))
                .activa(v.id == velocidad)
        })
        .collect();
    let sombras_ = crate::hypr_cfg::SOMBRAS;
    let sombra = sombras_
        .iter()
        .find(|s| s.0 == c.apariencia.sombra)
        .map_or(sombras_[0].0, |s| s.0);
    let sombras = sombras_
        .iter()
        .map(|(id, nombre, desc, _)| {
            opcion(idioma::t(nombre), idioma::t(desc), Que::Config(Cambio::Sombra((*id).into()))).activa(*id == sombra)
        })
        .collect();

    let galeria = |opciones| Fila::Galeria {
        titulo: None,
        opciones,
    };
    armar(
        Categoria::Apariencia,
        vec![
            std::iter::once(galeria(temas)).chain(matugen(c)).collect(),
            fondos,
            vec![
                fuentes(
                    Ambito::Sistema,
                    &l.fuente_sistema,
                    &l.fuentes_sistema,
                    tr!("Sistema · apps GTK y Qt"),
                ),
                fuentes(
                    Ambito::Terminal,
                    &l.fuente_terminal,
                    &l.fuentes_terminal,
                    tr!("Terminal · kitty"),
                ),
            ],
            vec![
                Fila::Galeria {
                    titulo: Some(tr!("Íconos").into()),
                    opciones: iconos,
                },
                Fila::Galeria {
                    titulo: Some(tr!("Cursor").into()),
                    opciones: cursores,
                },
                tamano_cursor(&l.cursor),
            ],
            vec![
                Fila::Galeria {
                    titulo: Some(tr!("Estilo").into()),
                    opciones: animaciones,
                },
                Fila::Galeria {
                    titulo: Some(tr!("Velocidad").into()),
                    opciones: velocidades,
                },
            ],
            vec![galeria(sombras)],
        ],
    )
}

fn tamano_cursor((tema, tamano): &(String, u32)) -> Fila {
    let tamano = *tamano;
    ajuste(
        tr!("Tamaño del cursor"),
        &if tamano > 0 {
            trf!("{} px", tamano)
        } else {
            String::new()
        },
        Control::Lista(
            [16, 20, 24, 32, 40, 48]
                .iter()
                .map(|&t| {
                    opcion(
                        &trf!("{} px", t),
                        "",
                        Que::Apariencia(crate::apariencia::Cambio::Cursor(tema.clone(), t)),
                    )
                    .activa(t == tamano)
                })
                .collect(),
        ),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct Resultado {
    pub categoria: Categoria,
    pub seccion: usize,
    pub fila: Option<usize>,
    pub titulo: String,
    pub donde: String,
}

pub fn buscar(paginas: &[(Categoria, &[Seccion])], consulta: &str) -> Vec<Resultado> {
    let mut resultados = Vec::new();
    let mut entradas = Vec::new();
    for (cat, secciones) in paginas {
        for (si, s) in secciones.iter().enumerate() {
            let donde = format!("{} › {}", cat.titulo(), s.titulo);
            let mut sumar = |titulo: &str, campos: &[&str], fila: Option<usize>| {
                entradas.push(Entrada::new("", titulo, campos));
                resultados.push(Resultado {
                    categoria: *cat,
                    seccion: si,
                    fila,
                    titulo: titulo.into(),
                    donde: donde.clone(),
                });
            };
            sumar(s.titulo, &[cat.titulo()], None);
            for (fi, f) in s.filas.iter().enumerate() {
                match f {
                    Fila::Ajuste { titulo, sub, .. } => sumar(titulo, &[sub], Some(fi)),
                    Fila::Galeria { titulo, opciones } => {
                        if let Some(t) = titulo {
                            sumar(t, &[], Some(fi));
                        }
                        for o in opciones {
                            sumar(&o.titulo, &[&o.sub], Some(fi));
                        }
                    }
                    Fila::Disposicion(_) => {}
                }
            }
        }
    }
    busqueda::filtrar(&entradas, consulta, Orden::Original)
        .into_iter()
        .map(|i| resultados[i].clone())
        .collect()
}

/// Con tope en el 70 % de la salida.
pub fn tamano(ancho_salida: i32, alto_salida: i32) -> (i32, i32) {
    let tope = |medida: i32, salida: i32| medida.min(salida * 7 / 10);
    (tope(ANCHO, ancho_salida), tope(ALTO, alto_salida))
}

pub const ANCHO: i32 = 960;
pub const ALTO: i32 = 620;

/// Vale desde el próximo bloqueo: `morgan-lock` lee `[lock]` al arrancar.
fn bloqueo(b: &Bloqueo) -> Vec<Fila> {
    let con = |c: CambioBloqueo| Que::Config(Cambio::Bloqueo(c));
    let casilla = |titulo: &str, sub: &str, activa: bool, c: fn(bool) -> CambioBloqueo| {
        ajuste(
            titulo,
            sub,
            Control::Casilla {
                activa,
                que: con(c(!activa)),
            },
        )
    };
    const ESCRITURAS: [(Escritura, &str, &str); 4] = [
        (
            Escritura::Onda,
            tr_const!("Onda"),
            tr_const!("cada tecla golpea una línea; no muestra el largo"),
        ),
        (
            Escritura::Puntos,
            tr_const!("Puntos"),
            tr_const!("como hyprlock: cada punto crece y la fila se centra"),
        ),
        (
            Escritura::Simbolos,
            tr_const!("Símbolos"),
            tr_const!("cada carácter se revuelve y queda en un glifo"),
        ),
        (Escritura::Fijo, tr_const!("Fijo"), tr_const!("un glifo por carácter, sin animar")),
    ];
    const VERIFICAR: [(Verificar, &str, &str); 4] = [
        (
            Verificar::Anillo,
            tr_const!("Anillo girando"),
            tr_const!("dos tramos de luz por el borde"),
        ),
        (Verificar::Latido, tr_const!("Latido"), tr_const!("lo tipeado queda latiendo")),
        (Verificar::Ambos, tr_const!("Los dos"), tr_const!("el giro y el latido")),
        (
            Verificar::Borde,
            tr_const!("Solo el borde"),
            tr_const!("el borde invertido y el texto"),
        ),
    ];
    fn nombre<T: PartialEq>(l: &[(T, &'static str, &str)], v: T) -> &'static str {
        l.iter().find(|(x, _, _)| *x == v).map_or("", |(_, t, _)| t)
    }
    vec![
        ajuste(
            tr!("Al escribir"),
            &trf!("lo que se ve en el campo · {}", idioma::t(nombre(&ESCRITURAS, b.escritura)).to_lowercase()),
            Control::Lista(
                ESCRITURAS
                    .iter()
                    .map(|&(v, t, s)| {
                        opcion(idioma::t(t), idioma::t(s), con(CambioBloqueo::Escritura(v))).activa(v == b.escritura)
                    })
                    .collect(),
            ),
        ),
        casilla(
            tr!("Chispa sobre el anillo"),
            tr!("cada tecla larga un destello que recorre el borde"),
            b.chispa,
            CambioBloqueo::Chispa,
        ),
        ajuste(
            tr!("Mientras verifica"),
            &trf!("lo que se mueve hasta que responde la contraseña · {}", idioma::t(nombre(&VERIFICAR, b.verificar)).to_lowercase()),
            Control::Lista(
                VERIFICAR
                    .iter()
                    .map(|&(v, t, s)| {
                        opcion(idioma::t(t), idioma::t(s), con(CambioBloqueo::Verificar(v))).activa(v == b.verificar)
                    })
                    .collect(),
            ),
        ),
        casilla(
            tr!("Sacudida al fallar"),
            tr!("el campo va y vuelve tres veces"),
            b.sacudida,
            CambioBloqueo::Sacudida,
        ),
        casilla(
            tr!("Rojo que se apaga"),
            tr!("al fallar, el brillo en rojo arranca fuerte y baja"),
            b.rojo,
            CambioBloqueo::Rojo,
        ),
        casilla(
            tr!("Reacción de lo tipeado"),
            tr!("al fallar, la onda da un golpe rojo, los puntos caen, los símbolos se tachan"),
            b.reaccion,
            CambioBloqueo::Reaccion,
        ),
        casilla(
            tr!("Destello al acertar"),
            tr!("el anillo se completa antes de que la pantalla se vaya"),
            b.destello,
            CambioBloqueo::Destello,
        ),
        ajuste(
            tr!("Probar"),
            tr!("bloquea la sesión con lo elegido; se sale con la contraseña"),
            Control::Boton(aplicar(&["loginctl", "lock-session"])),
        ),
    ]
}

/// Los idiomas van con su propio nombre: quien no entiende el actual igual encuentra el suyo.
/// Vale desde que morgan vuelve a arrancar (`morgan-idioma` lo decide una vez).
fn idioma_(puesto: &str) -> Fila {
    let auto = !matches!(idioma::Idioma::de_config(puesto), Some(Some(_)));
    let opciones = [
        ("auto", tr!("Automático"), tr!("el del sistema")),
        ("en", "English", ""), // no-tr
        ("es", "Español", ""), // no-tr
    ];
    let nombre = opciones
        .iter()
        .find(|(v, _, _)| *v == puesto || (auto && *v == "auto"))
        .map_or(puesto, |(_, n, _)| n);
    ajuste(
        tr!("Idioma"),
        &trf!("{} · se aplica al volver a entrar a la sesión", nombre),
        Control::Lista(
            opciones
                .iter()
                .map(|&(v, n, sub)| {
                    opcion(n, sub, Que::Config(Cambio::Idioma(v.into())))
                        .activa(v == puesto || (auto && v == "auto"))
                })
                .collect(),
        ),
    )
}

const OPACIDADES: &[f64] = &[0.7, 0.75, 0.8, 0.85, 0.9, 0.95, 1.0];
const RETENCIONES: &[u32] = &[1, 3, 7, 14, 30, 90, 365];
const MINUTOS_IDLE: &[u32] = &[0, 5, 10, 15, 20, 30, 45, 60];

fn minutos(m: u32) -> String {
    match m {
        0 => tr!("nunca").into(),
        1 => tr!("1 minuto").into(),
        m => trf!("{} minutos", m),
    }
}

fn opciones_idle(actual: u32, cambio: fn(u32) -> Cambio) -> Vec<Opcion> {
    MINUTOS_IDLE
        .iter()
        .map(|&m| opcion(&minutos(m), "", Que::Config(cambio(m))).activa(m == actual))
        .collect()
}

fn porcentaje(v: f64) -> String {
    format!("{} %", (v * 100.0).round() as i64)
}

fn dias(d: u32) -> String {
    if d == 1 {
        tr!("1 día").into()
    } else {
        trf!("{} días", d)
    }
}

pub fn por_app(apps_historial: &[String], c: &Config) -> Vec<Fila> {
    let n = &c.notificaciones;
    let mut apps: Vec<String> = apps_historial.to_vec();
    for s in n.silenciadas.iter().chain(&n.sin_sonido).chain(&n.una_sola) {
        if !apps.iter().any(|a| a.eq_ignore_ascii_case(s)) {
            apps.push(s.clone());
        }
    }
    if apps.is_empty() {
        return vec![ajuste(
            tr!("Ninguna app en el historial"),
            tr!("las que manden notificaciones van a aparecer acá"),
            Control::Nada,
        )];
    }
    apps.sort_by_key(|a| a.to_lowercase());
    apps.into_iter()
        .map(|a| {
            let modo = if n.silenciada(&a) {
                ModoApp::Silenciada
            } else if n.sin_sonido(&a) {
                ModoApp::SinSonido
            } else if n.una_sola(&a) {
                ModoApp::UnaSola
            } else {
                ModoApp::Normal
            };
            let modos = [
                (ModoApp::Normal, tr!("flota y suena")),
                (ModoApp::SinSonido, tr!("flota, sin sonido")),
                (ModoApp::UnaSola, tr!("flota y suena · sin acumular")),
                (ModoApp::Silenciada, tr!("silenciada · directo al historial")),
            ];
            let sub = modos
                .iter()
                .find(|(m, _)| *m == modo)
                .map_or("", |(_, t)| t);
            ajuste(
                &a,
                sub,
                Control::Lista(
                    modos
                        .iter()
                        .map(|(m, t)| {
                            opcion(t, "", Que::Config(Cambio::ModoApp(a.clone(), *m)))
                                .activa(*m == modo)
                        })
                        .collect(),
                ),
            )
        })
        .collect()
}

/// Las ocultas que no están conectadas también van, para poder volver a mostrarlas.
fn salidas(c: &Config, conectadas: &[(String, String)]) -> Vec<Fila> {
    let s = &c.sonido;
    let casilla = |nombre: &str, titulo: &str, conectada: bool| {
        let oculta = s.oculta(nombre);
        let sub = match (oculta, conectada) {
            (false, _) => tr!("se ofrece en el centro"),
            (true, true) => tr!("oculta en el centro"),
            (true, false) => tr!("oculta en el centro · no está conectada"),
        };
        ajuste(
            titulo,
            sub,
            Control::Casilla {
                activa: !oculta,
                que: Que::Config(Cambio::OcultarSalida(nombre.into(), !oculta)),
            },
        )
    };
    let mut v: Vec<Fila> = conectadas
        .iter()
        .map(|(n, d)| casilla(n, d, true))
        .collect();
    for n in &s.ocultas {
        if !conectadas.iter().any(|(c, _)| c == n) {
            v.push(casilla(n, n, false));
        }
    }
    if v.is_empty() {
        v.push(ajuste(tr!("Sin salidas de audio"), "", Control::Nada));
    }
    v
}

pub fn sonido(
    c: &Config,
    temas: &[TemaSonido],
    puesto: &str,
    conectadas: &[(String, String)],
) -> Vec<Seccion> {
    let s = &c.sonido;
    let volumen = vec![
        ajuste(
            tr!("Paso de las teclas de volumen"),
            &format!("{} %", s.paso),
            Control::Lista(
                [2, 5, 10]
                    .iter()
                    .map(|&p| {
                        opcion(&format!("{p} %"), "", Que::Config(Cambio::Paso(p)))
                            .activa(p == s.paso)
                    })
                    .collect(),
            ),
        ),
        ajuste(
            tr!("Permitir más de 100 %"),
            tr!("las teclas suben hasta 150 %; puede distorsionar"),
            Control::Casilla {
                activa: s.amplificar,
                que: Que::Config(Cambio::Amplificar(!s.amplificar)),
            },
        ),
    ];
    let casilla = |titulo: &str, sub: &str, q: SonidoSistema, v: bool| {
        ajuste(
            titulo,
            sub,
            Control::Casilla {
                activa: v,
                que: Que::Config(Cambio::SonidoSistema(q, !v)),
            },
        )
    };
    let sistema = vec![
        casilla(
            tr!("Al cambiar el volumen con las teclas"),
            tr!("para oír el nivel nuevo"),
            SonidoSistema::Volumen,
            s.al_cambiar_volumen,
        ),
        casilla(
            tr!("Al conectar o desconectar un dispositivo"),
            tr!("pendrives, discos y salidas de audio, como auriculares bluetooth"),
            SonidoSistema::Dispositivos,
            s.dispositivos,
        ),
        casilla(
            tr!("Al conectarse o perder la red"),
            "",
            SonidoSistema::Red,
            s.red,
        ),
        casilla(
            tr!("Al bloquear y desbloquear"),
            "",
            SonidoSistema::Bloqueo,
            s.bloqueo,
        ),
        casilla(
            tr!("Al terminar una grabación"),
            tr!("GPU Screen Recorder; el replay no cuenta"),
            SonidoSistema::Grabacion,
            s.grabacion,
        ),
    ];
    let total = MUESTRAS.len();
    let opciones = temas
        .iter()
        .map(|t| {
            opcion(
                &t.nombre,
                &trf!("{} de {} propios", t.propios, total),
                Que::ElegirTemaSonido(t.carpeta.clone(), t.demo.into()),
            )
            .activa(t.carpeta == puesto)
        })
        .collect();
    let escuchar = MUESTRAS
        .iter()
        .chain([&("audio-test-signal", tr!("Señal de prueba"))])
        .map(|(id, titulo)| opcion(idioma::t(titulo), "", Que::Sonar((*id).into())))
        .collect();
    let tema = vec![
        Fila::Galeria {
            titulo: None,
            opciones,
        },
        ajuste(
            tr!("Escuchar"),
            tr!("cada sonido de morgan con el tema puesto; lo que no trae propio, suena del Default"),
            Control::Lista(escuchar),
        ),
    ];
    armar(
        Categoria::Sonido,
        vec![volumen, salidas(c, conectadas), tema, sistema],
    )
}

fn horas(desde: u32, hasta: u32) -> Vec<String> {
    (desde * 2..=hasta * 2)
        .map(|m| format!("{:02}:{:02}", (m / 2) % 24, (m % 2) * 30))
        .collect()
}

fn luz(c: &Config) -> Vec<Fila> {
    let l = &c.luz;
    let color = vec![ajuste(
        tr!("Temperatura"),
        &trf!("{} K · más baja, más cálida", l.temperatura),
        Control::Lista(
            [2500, 3000, 3500, 4000, 4500, 5000, 5500, 6000]
                .iter()
                .map(|&k| {
                    let t = match k {
                        2500 => tr!("2500 K · muy cálida").to_owned(),
                        4000 => tr!("4000 K · la de siempre").to_owned(),
                        k => trf!("{} K", k),
                    };
                    opcion(&t, "", Que::Config(Cambio::Temperatura(k))).activa(k == l.temperatura)
                })
                .collect(),
        ),
    )];
    let lista = |actual: &str, opciones: Vec<String>, cambio: fn(String) -> Cambio| {
        Control::Lista(
            opciones
                .into_iter()
                .map(|h| {
                    let si = h == actual;
                    opcion(&h, "", Que::Config(cambio(h.clone()))).activa(si)
                })
                .collect(),
        )
    };
    let mut filas = color;
    filas.push(ajuste(
        tr!("Prender y apagar sola"),
        tr!("el centro la sigue prendiendo y apagando a mano"),
        Control::Casilla {
            activa: l.horario,
            que: Que::Config(Cambio::Horario(!l.horario)),
        },
    ));
    if l.horario {
        filas.push(ajuste(
            tr!("Se prende a las"),
            &l.prender,
            lista(&l.prender, horas(17, 23), Cambio::PrenderLuz),
        ));
        filas.push(ajuste(
            tr!("Se apaga a las"),
            &l.apagar,
            lista(&l.apagar, horas(5, 10), Cambio::ApagarLuz),
        ));
    }
    filas
}

fn matugen(c: &Config) -> Vec<Fila> {
    let m = &c.matugen;
    fn nombre(t: &str) -> &str {
        match t {
            "scheme-tonal-spot" => tr!("Tonal · la de siempre"),
            "scheme-content" => tr!("Fiel a la imagen"),
            "scheme-fidelity" => tr!("Fidelidad"),
            "scheme-expressive" => tr!("Expresiva"),
            "scheme-vibrant" => tr!("Vibrante"),
            "scheme-fruit-salad" => tr!("Ensalada de frutas"),
            "scheme-rainbow" => tr!("Arcoíris"),
            "scheme-neutral" => tr!("Neutra"),
            "scheme-monochrome" => tr!("Monocromo"),
            "scheme-smart" => tr!("Inteligente"),
            otro => otro,
        }
    }
    let modo = if m.modo == "light" { tr!("clara") } else { tr!("oscura") };
    let contraste = |v: f64| match v {
        v if v < -0.25 => tr!("bajo"),
        v if v > 0.75 => tr!("máximo"),
        v if v > 0.25 => tr!("alto"),
        _ => tr!("estándar"),
    };
    vec![
        ajuste(
            tr!("Paleta de matugen"),
            &trf!("{} · con el tema Matugen, se regenera al cambiar", nombre(&m.tipo)),
            Control::Lista(
                crate::config::TIPOS_MATUGEN
                    .iter()
                    .map(|t| {
                        opcion(nombre(t), "", Que::Config(Cambio::MatugenTipo((*t).into())))
                            .activa(*t == m.tipo)
                    })
                    .collect(),
            ),
        ),
        ajuste(
            tr!("Variante"),
            modo,
            Control::Lista(
                [("dark", tr!("oscura")), ("light", tr!("clara"))]
                    .iter()
                    .map(|(v, t)| {
                        opcion(t, "", Que::Config(Cambio::MatugenModo((*v).into())))
                            .activa(*v == m.modo)
                    })
                    .collect(),
            ),
        ),
        ajuste(
            tr!("Contraste"),
            contraste(m.contraste),
            Control::Lista(
                [-0.5, 0.0, 0.5, 1.0]
                    .iter()
                    .map(|&v| {
                        opcion(contraste(v), "", Que::Config(Cambio::MatugenContraste(v)))
                            .activa((v - m.contraste).abs() < 1e-6)
                    })
                    .collect(),
            ),
        ),
    ]
}

pub fn privacidad(portapapeles: usize, recientes: usize) -> Vec<Fila> {
    vec![
            ajuste(
                tr!("Borrar el historial del portapapeles"),
                &trf!("{} guardadas por cliphist", plural(portapapeles, tr!("entrada"), tr!("entradas"))),
                Control::Confirmar(Que::Aplicar(vec!["cliphist".into(), "wipe".into()])),
            ),
            ajuste(
                tr!("Borrar la lista de archivos recientes"),
                &trf!("{} en la lista que comparten las apps GTK", plural(recientes, tr!("archivo"), tr!("archivos"))),
                Control::Confirmar(Que::BorrarRecientes),
            ),
    ]
}

pub fn bonito(nombre: &str) -> String {
    nombre
        .split(['_', '-', ' '])
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(f) => f.to_uppercase().chain(c).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub const DURACIONES: &[(&str, &str)] = &[
    ("off", tr_const!("Desactivado")),
    ("3", tr_const!("3 horas")),
    ("6", tr_const!("6 horas")),
    ("12", tr_const!("12 horas")),
];

pub fn plural(n: usize, sing: &str, plur: &str) -> String {
    format!("{n} {}", if n == 1 { sing } else { plur })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tema_prueba(carpeta: &str, propios: usize) -> TemaSonido {
        TemaSonido {
            carpeta: carpeta.into(),
            nombre: carpeta.to_uppercase(),
            propios,
            demo: "service-login",
        }
    }

    #[test]
    fn el_tema_de_sonidos_puesto_y_lo_que_trae() {
        let c = Config::default();
        let temas = [
            tema_prueba("COSMIC", 0),
            tema_prueba("freedesktop", MUESTRAS.len()),
            tema_prueba("ocean", 7),
        ];
        let p = sonido(&c, &temas, "ocean", &[]);
        let Fila::Galeria { opciones, .. } = &p[2].filas[0] else {
            panic!("el tema no es una galería")
        };
        assert_eq!(activas(opciones), ["OCEAN"]);
        let subs: Vec<_> = opciones.iter().map(|o| o.sub.as_str()).collect();
        assert_eq!(
            subs,
            ["0 de 10 propios", "10 de 10 propios", "7 de 10 propios"]
        );
        assert_eq!(
            opciones[2].que,
            Que::ElegirTemaSonido("ocean".into(), "service-login".into())
        );
        // Escuchar: cada sonido de morgan y la señal de prueba, sin nada marcado.
        let escuchar = lista(&p, 2, 1);
        assert_eq!(escuchar.len(), MUESTRAS.len() + 1);
        assert!(activas(escuchar).is_empty());
        assert_eq!(escuchar[0].que, Que::Sonar("message-new-instant".into()));
    }

    fn lista(secciones: &[Seccion], s: usize, f: usize) -> &[Opcion] {
        match &secciones[s].filas[f] {
            Fila::Ajuste {
                control: Control::Lista(v),
                ..
            } => v,
            otra => panic!("no es una lista: {otra:?}"),
        }
    }

    fn activas(v: &[Opcion]) -> Vec<&str> {
        v.iter()
            .filter(|o| o.activa)
            .map(|o| o.titulo.as_str())
            .collect()
    }

    #[test]
    fn cada_pagina_tiene_sus_secciones() {
        let c = Config::default();
        for cat in Categoria::TODAS {
            let p = match (pagina_fija(cat, &c, &[]), cat) {
                // Los monitores los pone la lectura de Hyprland.
                (Some(mut p), Categoria::Pantalla) => {
                    p[0].filas = super::super::monitores::filas(&[], &Default::default(), true);
                    p
                }
                (Some(p), _) => p,
                (None, Categoria::Compositor) => {
                    super::super::compositor::pagina(&Default::default(), &Default::default(), &[])
                }
                (None, Categoria::Atajos) => super::super::atajos::pagina(&[], &[], &c.atajos),
                (None, Categoria::Aplicaciones) => super::super::sistema::aplicaciones(
                    &vec![Default::default(); super::super::sistema::PREDETERMINADAS.len()],
                    &[],
                    &Default::default(),
                    "/home/r/Imágenes",
                ),
                (None, Categoria::Sonido) => sonido(&c, &[tema_prueba("ocean", 7)], "ocean", &[]),
                (None, Categoria::Sistema) => armar(
                    cat,
                    vec![
                        super::super::sistema::equipo(&[("Kernel", "7.2".into())]),
                        super::super::sistema::snapshots(None),
                        privacidad(0, 0),
                    ],
                ),
                (None, _) => apariencia(&Lecturas::default(), &c),
            };
            let titulos: Vec<_> = p.iter().map(|s| s.titulo).collect();
            assert_eq!(titulos, cat.secciones(), "{cat:?}");
            assert!(
                p.iter().all(|s| !s.filas.is_empty()),
                "{cat:?}: sección vacía"
            );
        }
    }

    #[test]
    fn lo_marcado_sale_de_la_config() {
        let mut c = Config::default();
        let p = pagina_fija(Categoria::Shell, &c, &[]).unwrap();
        assert_eq!(activas(lista(&p, 0, 0)), ["85 %"]);
        c.shell.opacidad = 1.0;
        c.notificaciones.retencion_dias = 30;
        let p = pagina_fija(Categoria::Shell, &c, &[]).unwrap();
        assert_eq!(activas(lista(&p, 0, 0)), ["100 %"]);
        let p = pagina_fija(Categoria::Notificaciones, &c, &[]).unwrap();
        assert_eq!(activas(lista(&p, 0, 0)), ["30 días"]);
        // Una opacidad escrita a mano que no está en la lista no marca ninguna.
        c.shell.opacidad = 0.33;
        let p = pagina_fija(Categoria::Shell, &c, &[]).unwrap();
        assert!(activas(lista(&p, 0, 0)).is_empty());
    }

    #[test]
    fn el_bloqueo_marca_lo_puesto_y_cambia_solo_lo_suyo() {
        let mut c = Config::default();
        let p = pagina_fija(Categoria::Shell, &c, &[]).unwrap();
        assert_eq!(activas(lista(&p, 1, 0)), ["Onda"]);
        assert_eq!(activas(lista(&p, 1, 2)), ["Los dos"]);
        let puntos = lista(&p, 1, 0)
            .iter()
            .find(|o| o.titulo == "Puntos")
            .unwrap();
        let Que::Config(cambio) = &puntos.que else {
            panic!()
        };
        cambio.aplicar(&mut c);
        assert_eq!(c.bloqueo.escritura, Escritura::Puntos);
        assert_eq!(c.bloqueo.verificar, Verificar::Ambos);
        // Una casilla lleva el valor contrario al puesto, sin tocar lo demás.
        let Fila::Ajuste {
            control:
                Control::Casilla {
                    activa,
                    que: Que::Config(cambio),
                },
            ..
        } = &p[1].filas[1]
        else {
            panic!("la chispa no es una casilla")
        };
        assert!(*activa);
        cambio.aplicar(&mut c);
        assert!(!c.bloqueo.chispa);
        // Aunque la casilla sea de la página de antes, no deshace el cambio anterior.
        assert_eq!(c.bloqueo.escritura, Escritura::Puntos);
    }

    #[test]
    fn idle_marca_y_cambia() {
        let mut c = Config::default();
        let p = pagina_fija(Categoria::Pantalla, &c, &[]).unwrap();
        assert_eq!(activas(lista(&p, 2, 0)), ["10 minutos"]);
        assert_eq!(activas(lista(&p, 2, 1)), ["20 minutos"]);
        let nunca = &lista(&p, 2, 1)[0];
        assert_eq!(nunca.titulo, "nunca");
        let Que::Config(cambio) = &nunca.que else {
            panic!()
        };
        cambio.aplicar(&mut c);
        assert_eq!(c.idle.bloquear, 0);
        let p = pagina_fija(Categoria::Pantalla, &c, &[]).unwrap();
        assert_eq!(activas(lista(&p, 2, 1)), ["nunca"]);
    }

    #[test]
    fn resumen_alterna() {
        let mut c = Config::default();
        let casilla = |c: &Config| match &pagina_fija(Categoria::Notificaciones, c, &[]).unwrap()[1].filas[0]
        {
            Fila::Ajuste {
                control: Control::Casilla { activa, que },
                ..
            } => (*activa, que.clone()),
            otra => panic!("{otra:?}"),
        };
        let (activa, que) = casilla(&c);
        assert!(activa);
        let Que::Config(cambio) = que else { panic!() };
        cambio.aplicar(&mut c);
        assert!(!c.juego.resumen);
        assert!(!casilla(&c).0);
    }

    #[test]
    fn modo_por_app() {
        let mut c = Config::default();
        c.notificaciones.silenciadas = vec!["Steam".into()];
        let apps = vec!["discord".to_owned(), "steam".to_owned()];
        let v = por_app(&apps, &c);
        // "Steam" ya está como "steam" en el historial: no se repite.
        assert_eq!(v.len(), 2);
        let puesta = |f: &Fila| match f {
            Fila::Ajuste {
                control: Control::Lista(o),
                ..
            } => o.iter().find(|o| o.activa).unwrap().titulo.clone(),
            _ => panic!(),
        };
        assert_eq!(puesta(&v[0]), "flota y suena");
        assert_eq!(puesta(&v[1]), "silenciada · directo al historial");
        // Un modo saca al otro: una app no queda silenciada y sin sonido a la vez.
        Cambio::ModoApp("STEAM".into(), ModoApp::SinSonido).aplicar(&mut c);
        Cambio::ModoApp("discord".into(), ModoApp::Silenciada).aplicar(&mut c);
        assert_eq!(c.notificaciones.silenciadas, ["discord"]);
        assert_eq!(c.notificaciones.sin_sonido, ["STEAM"]);
        Cambio::ModoApp("steam".into(), ModoApp::Normal).aplicar(&mut c);
        assert!(c.notificaciones.sin_sonido.is_empty());
        Cambio::ModoApp("kitty".into(), ModoApp::UnaSola).aplicar(&mut c);
        assert_eq!(c.notificaciones.una_sola, ["kitty"]);
        let v = por_app(&[], &c);
        assert_eq!(puesta(&v[1]), "flota y suena · sin acumular");
        Cambio::ModoApp("KITTY".into(), ModoApp::Silenciada).aplicar(&mut c);
        assert!(c.notificaciones.una_sola.is_empty());
    }

    #[test]
    fn sonido_y_suspender() {
        let mut c = Config::default();
        let p = sonido(&c, &[], "freedesktop", &[]);
        assert_eq!(activas(lista(&p, 0, 0)), ["5 %"]);
        let p = pagina_fija(Categoria::Pantalla, &c, &[]).unwrap();
        assert_eq!(activas(lista(&p, 2, 2)), ["nunca"]);
        Cambio::Suspender(60).aplicar(&mut c);
        Cambio::Paso(10).aplicar(&mut c);
        Cambio::Amplificar(true).aplicar(&mut c);
        Cambio::SonidoSistema(SonidoSistema::Red, false).aplicar(&mut c);
        assert_eq!(c.sonido.tope(), 150.0);
        assert!(!c.sonido.red && c.sonido.bloqueo);
        let p = pagina_fija(Categoria::Pantalla, &c, &[]).unwrap();
        assert_eq!(activas(lista(&p, 2, 2)), ["60 minutos"]);
    }

    #[test]
    fn salidas_que_se_ofrecen_en_el_centro() {
        let mut c = Config::default();
        let conectadas = [
            ("alsa_output.a".to_owned(), "Auriculares".to_owned()),
            ("alsa_output.hdmi".to_owned(), "HDMI".to_owned()),
        ];
        let casillas = |c: &Config| -> Vec<(String, bool, Que)> {
            sonido(c, &[], "freedesktop", &conectadas)[1]
                .filas
                .iter()
                .map(|f| match f {
                    Fila::Ajuste {
                        titulo,
                        control: Control::Casilla { activa, que },
                        ..
                    } => (titulo.clone(), *activa, que.clone()),
                    otra => panic!("{otra:?}"),
                })
                .collect()
        };
        let v = casillas(&c);
        assert_eq!(v.len(), 2);
        assert!(v.iter().all(|(_, activa, _)| *activa));
        let Que::Config(ocultar) = &v[1].2 else { panic!() };
        ocultar.aplicar(&mut c);
        assert_eq!(c.sonido.ocultas, ["alsa_output.hdmi"]);
        // Desconectada, sigue en la lista para poder volver a mostrarla.
        c.sonido.ocultas.push("alsa_output.vieja".into());
        let v = casillas(&c);
        assert_eq!(
            v.iter().map(|(t, a, _)| (t.as_str(), *a)).collect::<Vec<_>>(),
            [("Auriculares", true), ("HDMI", false), ("alsa_output.vieja", false)]
        );
        let Que::Config(mostrar) = &v[1].2 else { panic!() };
        mostrar.aplicar(&mut c);
        assert_eq!(c.sonido.ocultas, ["alsa_output.vieja"]);
    }

    #[test]
    fn fondos_en_la_config() {
        let mut c = Config::default();
        Cambio::RotarFondos(6).aplicar(&mut c);
        Cambio::CarpetaFondos("/f".into()).aplicar(&mut c);
        assert_eq!((c.fondos.rotar, c.fondos.carpeta.as_str()), (6, "/f"));
    }

    #[test]
    fn luz_nocturna() {
        let mut c = Config::default();
        let p = pagina_fija(Categoria::Pantalla, &c, &[]).unwrap();
        assert_eq!(activas(lista(&p, 1, 0)), ["4000 K · la de siempre"]);
        // Sin horario no se muestran las horas: temperatura y la casilla.
        assert_eq!(p[1].filas.len(), 2);
        Cambio::Horario(true).aplicar(&mut c);
        Cambio::PrenderLuz("21:30".into()).aplicar(&mut c);
        let p = pagina_fija(Categoria::Pantalla, &c, &[]).unwrap();
        assert_eq!(activas(lista(&p, 1, 2)), ["21:30"]);
        assert_eq!(activas(lista(&p, 1, 3)), ["07:00"]);
        assert_eq!(horas(22, 23), ["22:00", "22:30", "23:00"]);
    }

    #[test]
    fn paleta_de_matugen() {
        let mut c = Config::default();
        let p = apariencia(&Lecturas::default(), &c);
        // Tema y colores: la galería y las tres de matugen.
        assert_eq!(p[0].filas.len(), 4);
        assert_eq!(activas(lista(&p, 0, 1)), ["Tonal · la de siempre"]);
        Cambio::MatugenModo("light".into()).aplicar(&mut c);
        Cambio::MatugenContraste(0.5).aplicar(&mut c);
        let p = apariencia(&Lecturas::default(), &c);
        assert_eq!(activas(lista(&p, 0, 2)), ["clara"]);
        assert_eq!(activas(lista(&p, 0, 3)), ["alto"]);
    }

    #[test]
    fn tamano_del_cursor() {
        let f = tamano_cursor(&("Bibata-Original-Classic".into(), 24));
        let Fila::Ajuste {
            control: Control::Lista(o),
            ..
        } = &f
        else {
            panic!()
        };
        assert_eq!(activas(o), ["24 px"]);
        assert_eq!(
            o[0].que,
            Que::Apariencia(crate::apariencia::Cambio::Cursor(
                "Bibata-Original-Classic".into(),
                16
            ))
        );
    }

    #[test]
    fn apariencia_marca_lo_puesto() {
        let l = Lecturas {
            temas: vec!["gruvbox-dark-medium".into(), "matugen".into()],
            tema_activo: "matugen".into(),
            duracion: "6".into(),
            fuente_sistema: "Inter".into(),
            fuentes_sistema: vec![
                ("Inter".into(), "20 variantes".into()),
                ("Arial".into(), String::new()),
            ],
            iconos: vec!["Tela-circle".into(), "Papirus".into()],
            icono: "Papirus".into(),
            ..Default::default()
        };
        let p = apariencia(&l, &Config::default());
        let galeria = |s: usize, f: usize| match &p[s].filas[f] {
            Fila::Galeria { opciones, .. } => opciones.clone(),
            otra => panic!("{otra:?}"),
        };
        assert_eq!(activas(&galeria(0, 0)), ["Matugen"]);
        assert_eq!(activas(lista(&p, 1, 2)), ["6 horas"]);
        let fuentes = galeria(2, 0);
        assert_eq!(activas(&fuentes), ["Inter"]);
        assert_eq!(fuentes[1].fuente.as_deref(), Some("Arial"));
        assert_eq!(activas(&galeria(3, 0)), ["Papirus"]);
    }

    #[test]
    fn busca_en_todas_las_paginas() {
        let c = Config::default();
        let ap = apariencia(
            &Lecturas {
                iconos: vec!["Tela-circle".into(), "Papirus".into()],
                icono: "Papirus".into(),
                ..Default::default()
            },
            &c,
        );
        let en = pagina_fija(Categoria::Pantalla, &c, &[]).unwrap();
        let paginas = [
            (Categoria::Apariencia, &ap[..]),
            (Categoria::Pantalla, &en[..]),
        ];
        let r = buscar(&paginas, "papirus");
        assert_eq!(r.len(), 1);
        assert_eq!(
            (r[0].categoria, r[0].seccion, r[0].fila),
            (Categoria::Apariencia, 3, Some(0))
        );
        assert_eq!(r[0].donde, "Apariencia › Íconos y cursor");
        // Sin tildes, y por el subtítulo.
        let r = buscar(&paginas, "inactividad");
        assert_eq!(r[0].fila, None);
        let r = buscar(&paginas, "sin actividad");
        assert_eq!(
            r.iter().map(|r| r.titulo.as_str()).collect::<Vec<_>>(),
            ["Apagar pantallas", "Bloquear", "Suspender"]
        );
        assert!(
            buscar(&paginas, "fondos")
                .iter()
                .any(|r| r.titulo == "Fondos" && r.fila.is_none())
        );
        // El título de una galería lleva a su fila.
        let r = buscar(&paginas, "velocidad");
        assert_eq!(r[0].titulo, "Velocidad");
        assert_eq!(r[0].donde, "Apariencia › Animaciones");
        assert_eq!(r[0].fila, Some(1));
    }

    #[test]
    fn tamano_con_tope() {
        assert_eq!(tamano(2560, 1440), (960, 620));
        assert_eq!(tamano(1280, 720), (896, 504));
    }

    #[test]
    fn nombres_bonitos() {
        assert_eq!(bonito("vertical"), "Vertical");
        assert_eq!(bonito("diablo-1"), "Diablo 1");
        assert_eq!(bonito("gruvbox-dark_medium"), "Gruvbox Dark Medium");
    }

}
