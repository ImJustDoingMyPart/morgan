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
    /// En la columna de la imagen, a 36 px.
    pub imagen: Option<ImageData>,
    /// El de la app, chico, en la esquina de la imagen.
    pub chico: Option<ImageData>,
    /// Una captura o una foto, abajo y a lo ancho, con su tamaño en lógicos.
    pub previa: Option<(ImageData, u32, u32)>,
    pub progreso: Option<i32>,
    /// Clave, etiqueta y glifo de cada botón.
    pub acciones: Vec<(String, String, &'static str)>,
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
}
