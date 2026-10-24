//! Lo que el hilo de GTK le manda al del centro: qué se ve, ya en palabras y glifos. Cada cosa
//! que se toca lleva un `Id`; el hilo del centro lo devuelve en un `Evento` y el de GTK sabe qué
//! hacer con él. Lo que es solo de la vista (qué lista está abierta, el foco, lo escrito en un
//! campo, un arrastre) lo lleva el hilo del centro.

use morgan_motor::vello::peniko::ImageData;

/// Una acción registrada en el hilo de GTK.
pub type Id = u64;

/// Las listas que se despliegan: abrir una cierra las otras (salvo las de adentro de las
/// cuotas, que son de su propio grupo).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListaId {
    Red,
    Bluetooth,
    Energia,
    Cuotas,
    Sesion,
    Salida,
    Microfono,
    App(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NivelId {
    Salida,
    App(u32),
    Microfono,
    Brillo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tarjeta {
    pub glifo: String,
    pub titulo: String,
    pub estado: String,
    pub encendida: bool,
    pub alerta: bool,
    pub que: Toque,
}

/// Lo que hace un click en una tarjeta.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Toque {
    Accion(Id),
    Lista(ListaId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Deslizar {
    pub glifo: &'static str,
    pub texto: String,
    /// Al pasar el umbral, la fila se va (olvidar) o vuelve (copiar).
    pub se_va: bool,
    pub accion: Id,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Opcion {
    pub glifo: String,
    pub texto: String,
    pub marcada: Option<bool>,
    pub accion: Id,
    pub derecha: Option<Deslizar>,
    pub izquierda: Option<Deslizar>,
    /// Resaltada: tiene abajo su campo (una clave, un emparejamiento).
    pub abierta: bool,
    /// Se ve atenuada y no responde (el «Sí» de una confirmación, al principio).
    pub habilitada: bool,
    pub campo: Option<Campo>,
}

impl Opcion {
    pub fn new(
        glifo: impl Into<String>,
        texto: impl Into<String>,
        marcada: Option<bool>,
        accion: Id,
    ) -> Self {
        Opcion {
            glifo: glifo.into(),
            texto: texto.into(),
            marcada,
            accion,
            derecha: None,
            izquierda: None,
            abierta: false,
            habilitada: true,
            campo: None,
        }
    }
}

/// El botón a la derecha de un campo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BotonCampo {
    /// Ver u ocultar la clave; lo resuelve el hilo del centro.
    Ojo,
    Tilde(Id),
}

/// La caja de escribir debajo de una opción (`cc-clave`): la clave de una Wi-Fi o lo que pide
/// un emparejamiento.
#[derive(Clone, Debug, PartialEq)]
pub struct Campo {
    /// Sube cuando el campo es otro (otra red, otro pedido): lo escrito se borra y toma el foco.
    pub generacion: u64,
    /// `None`: sin caja, solo el aviso («Emparejando…»).
    pub caja: Option<Caja>,
    pub aviso: Option<(String, bool)>,
    /// Sube con cada fallo (una clave mala): la caja tiembla y se tiñe de rojo, y el aviso
    /// aparece después.
    pub fallo: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Caja {
    pub glifo: &'static str,
    pub placeholder: String,
    /// Tapada con «•» hasta que se toca el ojo.
    pub clave: bool,
    /// Un código para leer: no se escribe.
    pub fijo: Option<String>,
    pub boton: Option<BotonCampo>,
    /// Mientras algo está en curso, no se escribe ni se toca.
    pub ocupado: bool,
    /// Enter, con lo escrito.
    pub enviar: Id,
    /// Escape con el foco en la caja: cierra el campo, no el centro.
    pub escape: Id,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Contenido {
    Opciones(Vec<Opcion>),
    Cuotas(Vec<Cuota>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Nivel {
    pub id: NivelId,
    pub glifo: String,
    pub nombre: String,
    /// De 0 a 100.
    pub valor: f64,
    pub mudo: bool,
    /// El glifo silencia; sin acción no responde.
    pub glifo_accion: Option<Id>,
    /// Un click en el nombre despliega esta lista.
    pub lista: Option<ListaId>,
    pub mover: Id,
}

/// Una fila de las cuotas de IA: un nivel con la barra del OSD en lugar del slider.
#[derive(Clone, Debug, PartialEq)]
pub struct Cuota {
    pub glifo: String,
    pub nombre: String,
    pub renueva: String,
    pub porcentaje: String,
    pub fraccion: f64,
    pub apagada: bool,
    pub alerta: bool,
    /// Las otras ventanas del agente, que despliega un click en el nombre.
    pub otras: Vec<Cuota>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BotonRepro {
    Mezcla,
    Anterior,
    Play,
    Siguiente,
    Repetir,
}

/// Los íconos simbólicos del tema (solo cuenta su alfa), dibujados por GTK: los del
/// reproductor y los del encabezado de un grupo de notificaciones.
#[derive(Clone, Default)]
pub struct Iconos {
    pub play: Option<ImageData>,
    pub pausa: Option<ImageData>,
    pub anterior: Option<ImageData>,
    pub siguiente: Option<ImageData>,
    pub mezcla: Option<ImageData>,
    pub repetir: Option<ImageData>,
    pub repetir_uno: Option<ImageData>,
    pub contraer: Option<ImageData>,
    pub cerrar: Option<ImageData>,
}

#[derive(Clone)]
pub struct Reproductor {
    pub caratula: Option<ImageData>,
    pub titulo: String,
    pub subtitulo: String,
    pub sonando: bool,
    pub mezcla: Option<bool>,
    /// «None», «Track» o «Playlist», si el reproductor lo publica.
    pub repetir: Option<String>,
    pub puede_anterior: bool,
    pub puede_siguiente: bool,
}

/// Una notificación de la lista, ya armada para dibujar.
#[derive(Clone)]
pub struct Aviso {
    pub id: u32,
    pub titulo: String,
    /// Markup ya seguro (`notificaciones::markup`): `b`, `i`, `u` y entidades.
    pub cuerpo: String,
    pub urgencia: crate::notificaciones::modelo::Urgencia,
    pub llegada: i64,
    /// En la columna de la imagen (36 px en el centro; en un toast, 24 o 40).
    pub imagen: Option<ImageData>,
    /// La imagen es un ícono del tema: en un toast va a 24, en la columna de 36.
    pub icono: bool,
    /// El de la app, chico, en la esquina de la imagen.
    pub chico: Option<ImageData>,
    /// Una captura o una foto, abajo y a lo ancho, con su tamaño en lógicos.
    pub previa: Option<(ImageData, u32, u32)>,
    pub progreso: Option<i32>,
    /// Clave, etiqueta y glifo propio de cada botón (los de una captura).
    pub acciones: Vec<(String, String, Option<&'static str>)>,
}

pub struct Grupo {
    pub clave: String,
    pub nombre: String,
    /// La de adelante primero.
    pub avisos: Vec<Aviso>,
}

/// El menú de una app de la bandeja.
#[derive(Clone, Debug)]
pub struct Menu {
    /// La salida donde está el ícono y la distancia del borde derecho del ícono al de ella.
    pub salida: String,
    pub derecha: i32,
    pub secciones: Vec<Vec<NodoMenu>>,
}

#[derive(Clone, Debug)]
pub struct NodoMenu {
    pub id: i32,
    pub etiqueta: String,
    pub habilitado: bool,
    pub marca: Option<crate::bandeja::menu::Marca>,
    pub hijos: Vec<Vec<NodoMenu>>,
}

/// El ícono de una fila del launcher: una imagen tal cual, o un simbólico que se pinta del
/// color del texto.
#[derive(Clone)]
pub enum IconoFila {
    Imagen(ImageData),
    Simbolico(ImageData),
}

/// Qué pasa al elegir una fila del launcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipoFila {
    /// Se hace y el launcher se cierra (con el destello de la elegida).
    Cierra,
    /// Se hace con el launcher abierto (una carpeta de emoji, probar un calendario).
    Queda,
    /// Una fila que explica: no hace nada.
    Nada,
}

#[derive(Clone)]
pub struct FilaLauncher {
    /// Sin ícono, el texto arranca donde iría (los símbolos, el portapapeles).
    pub icono: Option<IconoFila>,
    pub nombre: String,
    pub generico: Option<String>,
    /// Lo que coincide con lo escrito, en caracteres: del nombre y del genérico.
    pub resaltes: Vec<(usize, usize)>,
    pub resaltes_generico: Vec<(usize, usize)>,
    pub tipo: TipoFila,
    /// Una carpeta de emoji (la flecha a la derecha la abre).
    pub carpeta: bool,
}

/// El turno de lo escrito en el launcher: la apertura (o el vaciado) que lo empezó, y cada
/// cambio desde ahí.
pub type Turno = (u64, u64);

/// Lo que muestra el launcher para lo escrito.
#[derive(Clone)]
pub struct ListaLauncher {
    /// El turno del texto al que responde (`EventoLauncher::Texto`): Enter espera a la lista
    /// del texto que se ve.
    pub texto: Turno,
    /// Sube con cada lista: lo elegido se reconoce por ella.
    pub version: u64,
    pub filas: Vec<FilaLauncher>,
    pub elegida: usize,
    /// Adentro de una carpeta de emoji: Escape, ← y Retroceso con el campo vacío salen.
    pub en_carpeta: bool,
    /// El campo vuelve a vacío con este placeholder (al entrar o salir de una carpeta).
    pub vaciar: Option<String>,
}

/// Lo que pide el launcher al hilo de GTK.
#[derive(Debug)]
pub enum EventoLauncher {
    /// Lo escrito, con su turno.
    Texto(Turno, String),
    Elegido { version: u64, indice: usize },
    SalirCarpeta,
    /// Lo cerró el launcher mismo; con Escape (que desde el menú vuelve a él) o no.
    Cerrado { escape: bool },
}

/// Una fila del índice del menú de sistema: una categoría (con su glifo), una de sus secciones
/// o un resultado de la búsqueda (con dónde está).
#[derive(Clone, Debug)]
pub struct FilaIndice {
    pub glifo: Option<char>,
    pub titulo: String,
    pub donde: Option<String>,
    /// Lo que coincide con lo buscado, en caracteres.
    pub resaltes: Vec<(usize, usize)>,
    pub resaltes_donde: Vec<(usize, usize)>,
    /// Una sección de la página que se ve: su número (se marca la que está a la vista).
    pub seccion: Option<usize>,
    /// Adónde lleva en la página al pasar a ella: sección y fila.
    pub destino: Option<(usize, Option<usize>)>,
}

pub struct IndiceAjustes {
    pub filas: Vec<FilaIndice>,
    pub elegida: usize,
    /// Las secciones de la categoría recién abierta bajan.
    pub desplegar: bool,
    pub version: u64,
}

pub struct PaginaAjustes {
    pub titulo: String,
    /// Sin leer todavía: «Leyendo…».
    pub secciones: Option<Vec<crate::menu::arbol::Seccion>>,
    /// Una página nueva entra subiendo; releer la misma, no.
    pub entra: bool,
    pub version: u64,
    /// Lo elegido con el teclado tras releer (tras probar un monitor, «Mantener»).
    pub elegir: Option<usize>,
}

/// Dónde está lo que se tocó en la página: la sección, la fila y, si la fila tiene varias
/// (una lista, una galería, los monitores a escala), cuál.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Lugar {
    pub seccion: usize,
    pub fila: usize,
    pub opcion: Option<usize>,
}

/// Lo que pide el menú de sistema al hilo de GTK.
#[derive(Debug)]
pub enum EventoAjustes {
    /// Lo escrito en el buscador.
    Texto(String),
    /// La fila del índice elegida.
    Navegar { version: u64, indice: usize },
    Hacer {
        version: u64,
        lugar: Lugar,
        que: crate::menu::arbol::Que,
    },
    /// Grabando un atajo: la tecla, con su código (de xkb), su keysym y los modificadores
    /// (Super, Ctrl, Alt, Shift).
    Tecla {
        codigo: u32,
        keysym: u32,
        mods: [bool; 4],
    },
    /// Lo cerró el menú mismo (Escape, un click afuera).
    Cerrado,
}

/// Lo que muestra la calculadora: la pantalla (la cuenta o el resultado grande, y debajo lo
/// otro, tenue) y la cinta de las cuentas hechas, la última abajo.
#[derive(Clone, Debug, Default)]
pub struct VistaCalculadora {
    pub grande: String,
    /// Sin nada escrito: el «0» atenuado.
    pub vacia: bool,
    pub chica: String,
    /// «copiado», en `tertiary`, un instante.
    pub copiado: bool,
    /// Cuenta y resultado de cada una.
    pub cinta: Vec<(String, String)>,
}

/// Lo que la banda de fondos necesita para abrirse: la carpeta y sus imágenes. Las miniaturas,
/// la elegida, lo escrito y el desplazamiento los lleva el hilo del centro.
pub struct VistaFondos {
    /// La salida enfocada (su conector); sin ella, la que elija el compositor.
    pub salida: Option<String>,
    /// El nombre de la carpeta, ya con mayúscula.
    pub carpeta: String,
    pub rutas: Vec<std::path::PathBuf>,
    /// El fondo de ahora: arranca elegido y marcado.
    pub actual: Option<std::path::PathBuf>,
    /// Las que todavía no tienen miniatura.
    pub faltan: Vec<std::path::PathBuf>,
    pub cache: std::path::PathBuf,
    /// Escape vuelve al menú (se entró desde ahí) en vez de cerrar.
    pub volver: bool,
}

/// Lo que pide la banda de fondos al hilo de GTK.
#[derive(Debug)]
pub enum EventoFondos {
    /// Enter o un click en una imagen.
    Aplicar(std::path::PathBuf),
    /// La cerró ella misma (Escape, o la capa se fue); con `volver`, al menú.
    Cerrada { volver: bool },
}

/// Lo que pide la calculadora al hilo de GTK.
#[derive(Debug)]
pub enum EventoCalculadora {
    Accion(crate::calculadora::modelo::Accion),
    /// Lo escrito con el teclado, ya pasado a lo que se ve (`modelo::del_teclado`), o el
    /// resultado de una fila de la cinta.
    Escribir(String),
    Copiar,
    /// La cerró ella misma (Escape, un click afuera).
    Cerrada,
}

/// Lo que se tocó, para el hilo de GTK.
#[derive(Debug)]
pub enum Evento {
    Tocar(Id),
    /// Un slider que se movió, con su nivel de 0 a 100.
    Nivel(Id, f64),
    /// Enter en un campo, con lo escrito.
    Enviar(Id, String),
    Repro(BotonRepro),
    /// La acción por defecto de una notificación (el click, Enter).
    Accion(u32),
    /// Uno de sus botones, por clave.
    Boton(u32, String),
    Cerrar(u32),
    /// Título y cuerpo al portapapeles.
    Copiar(u32),
    CerrarGrupo(String),
    /// Las que había al tocar «limpiar», cuando terminaron de salir.
    Limpiar(Vec<u32>),
    /// Lo cerró el centro mismo (Escape, un click afuera).
    Cerrado,
    /// Del menú de la bandeja: la fila elegida, o que se cerró sin elegir.
    Elegido(i32),
    MenuCerrado,
    Toast(EventoToast),
    Launcher(EventoLauncher),
    Ajustes(EventoAjustes),
    Calculadora(EventoCalculadora),
    Fondos(EventoFondos),
}

/// Lo que se hizo en un toast. Salvo copiar, el toast se va.
#[derive(Debug)]
pub enum EventoToast {
    Accion(u32),
    Boton(u32, String),
    Cerrar(u32),
    Copiar(u32),
    /// El puntero sobre un toast (o sobre ninguno): su cuenta se frena.
    Encima(Option<u32>),
}
