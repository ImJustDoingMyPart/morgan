//! La expresión se escribe como se ve (÷ × −, coma decimal) y va así a `qalc`. La coma no es
//! estética: con un locale de coma decimal `qalc` toma el punto como separador de miles
//! (`2.5+1` da 26), así que el punto del teclado también escribe coma.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accion {
    Escribir(&'static str),
    Parentesis,
    Borrar,
    Limpiar,
    Igual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clase {
    Numero,
    Operador,
    Funcion,
    Igual,
}

pub struct Tecla {
    pub etiqueta: &'static str,
    pub accion: Accion,
    pub clase: Clase,
}

const fn t(etiqueta: &'static str, accion: Accion, clase: Clase) -> Tecla {
    Tecla {
        etiqueta,
        accion,
        clase,
    }
}

const fn num(n: &'static str) -> Tecla {
    t(n, Accion::Escribir(n), Clase::Numero)
}

const fn op(o: &'static str) -> Tecla {
    t(o, Accion::Escribir(o), Clase::Operador)
}

pub const BOTONERA: [[Tecla; 4]; 5] = [
    [
        t("C", Accion::Limpiar, Clase::Funcion),
        t("( )", Accion::Parentesis, Clase::Funcion),
        t("%", Accion::Escribir("%"), Clase::Funcion),
        op("÷"),
    ],
    [num("7"), num("8"), num("9"), op("×")],
    [num("4"), num("5"), num("6"), op("−")],
    [num("1"), num("2"), num("3"), op("+")],
    [
        t("󰭜", Accion::Borrar, Clase::Funcion),
        num("0"),
        t(",", Accion::Escribir(","), Clase::Numero),
        t("=", Accion::Igual, Clase::Igual),
    ],
];

/// El resto (letras, `^`, `√`…) va tal cual: `qalc` entiende funciones, unidades y monedas.
pub fn del_teclado(c: char) -> Option<String> {
    let s = match c {
        '*' => "×".to_owned(),
        '/' => "÷".to_owned(),
        '-' => "−".to_owned(),
        '.' => ",".to_owned(),
        c if c.is_control() => return None,
        c => c.to_string(),
    };
    Some(s)
}

pub fn tecla_de(texto: &str) -> Option<(usize, usize)> {
    BOTONERA.iter().enumerate().find_map(|(f, fila)| {
        fila.iter()
            .position(|t| matches!(t.accion, Accion::Escribir(s) if s == texto))
            .map(|c| (f, c))
    })
}

fn es_operador(c: char) -> bool {
    matches!(c, '÷' | '×' | '−' | '+' | '^' | '(')
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Expresion {
    texto: String,
    resultado: bool,
}

impl Expresion {
    pub fn texto(&self) -> &str {
        &self.texto
    }

    pub fn es_resultado(&self) -> bool {
        self.resultado
    }

    pub fn escribir(&mut self, s: &str) {
        // Un operador sigue desde el resultado; `(` empieza otra cuenta, como un número.
        let sigue = s
            .chars()
            .next()
            .is_some_and(|c| (es_operador(c) && c != '(') || c == '%');
        if self.resultado && !sigue {
            self.texto.clear();
        }
        self.resultado = false;
        self.texto.push_str(s);
    }

    pub fn parentesis(&mut self) {
        let abiertos = self.texto.matches('(').count();
        let cerrados = self.texto.matches(')').count();
        let antes = self.texto.chars().next_back();
        let abrir = antes.is_none_or(es_operador) || abiertos <= cerrados;
        self.escribir(if abrir { "(" } else { ")" });
    }

    pub fn borrar(&mut self) {
        self.resultado = false;
        self.texto.pop();
    }

    pub fn limpiar(&mut self) {
        *self = Expresion::default();
    }

    pub fn poner_resultado(&mut self, r: &str) {
        self.texto = r.to_owned();
        self.resultado = true;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cuenta {
    pub cuenta: String,
    pub resultado: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Historial(Vec<Cuenta>);

impl Historial {
    pub const TOPE: usize = 50;

    pub fn cuentas(&self) -> &[Cuenta] {
        &self.0
    }

    /// Repetir la última no la duplica.
    pub fn agregar(&mut self, cuenta: &str, resultado: &str) -> bool {
        let nueva = Cuenta {
            cuenta: cuenta.to_owned(),
            resultado: resultado.to_owned(),
        };
        if self.0.last() == Some(&nueva) {
            return false;
        }
        self.0.push(nueva);
        let sobra = self.0.len().saturating_sub(Self::TOPE);
        self.0.drain(..sobra);
        true
    }
}

/// `None` si es la misma expresión que se mandó: `1/0` vuelve como `1 / 0`.
pub fn resultado(expresion: &str, salida: &str) -> Option<String> {
    let r = salida.lines().next()?.trim();
    let sin_espacios = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let igual = sin_espacios(r) == sin_espacios(expresion)
        || sin_espacios(r) == sin_espacios(&expresion.replace('÷', "/"));
    (!r.is_empty() && !igual).then(|| r.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teclado() {
        assert_eq!(del_teclado('*').as_deref(), Some("×"));
        assert_eq!(del_teclado('/').as_deref(), Some("÷"));
        assert_eq!(del_teclado('-').as_deref(), Some("−"));
        assert_eq!(del_teclado('.').as_deref(), Some(","));
        assert_eq!(del_teclado('7').as_deref(), Some("7"));
        assert_eq!(del_teclado('s').as_deref(), Some("s"));
        assert_eq!(del_teclado('\n'), None);
    }

    #[test]
    fn la_tecla_que_se_ilumina() {
        assert_eq!(tecla_de("7"), Some((1, 0)));
        assert_eq!(tecla_de("÷"), Some((0, 3)));
        assert_eq!(tecla_de(","), Some((4, 2)));
        assert_eq!(tecla_de("s"), None);
    }

    #[test]
    fn escribir_despues_de_un_resultado() {
        let mut e = Expresion::default();
        e.poner_resultado("36");
        e.escribir("+");
        e.escribir("4");
        assert_eq!(e.texto(), "36+4");
        e.poner_resultado("40");
        e.escribir("5");
        assert_eq!(e.texto(), "5");
        e.poner_resultado("5");
        e.escribir("%");
        assert_eq!(e.texto(), "5%");
        e.poner_resultado("1815");
        e.escribir("(");
        assert_eq!(e.texto(), "(");
    }

    #[test]
    fn parentesis_inteligente() {
        let mut e = Expresion::default();
        e.parentesis();
        e.escribir("1");
        e.escribir("+");
        e.escribir("2");
        e.parentesis();
        e.escribir("×");
        e.parentesis();
        e.escribir("3");
        e.parentesis();
        assert_eq!(e.texto(), "(1+2)×(3)");
        e.parentesis();
        assert_eq!(e.texto(), "(1+2)×(3)(");
    }

    #[test]
    fn borrar_y_limpiar() {
        let mut e = Expresion::default();
        e.escribir("12");
        e.escribir("÷");
        e.borrar();
        assert_eq!(e.texto(), "12");
        e.poner_resultado("3");
        e.borrar();
        assert_eq!(e.texto(), "");
        assert!(!e.es_resultado());
        e.escribir("9");
        e.limpiar();
        assert_eq!(e, Expresion::default());
    }

    #[test]
    fn historial() {
        let mut h = Historial::default();
        assert!(h.agregar("12×3", "36"));
        assert!(!h.agregar("12×3", "36"));
        assert!(h.agregar("36+4", "40"));
        assert!(h.agregar("12×3", "36"));
        assert_eq!(h.cuentas().len(), 3);
        for n in 0..Historial::TOPE {
            h.agregar(&n.to_string(), "x");
        }
        assert_eq!(h.cuentas().len(), Historial::TOPE);
        assert_eq!(h.cuentas()[0].cuenta, "0");
        let json = serde_json::to_string(&h).unwrap();
        assert_eq!(serde_json::from_str::<Historial>(&json).unwrap(), h);
    }

    #[test]
    fn respuestas_de_qalc() {
        assert_eq!(resultado("12×3", "36\n").as_deref(), Some("36"));
        assert_eq!(
            resultado("1÷3", "0,3333333333\n").as_deref(),
            Some("0,3333333333")
        );
        assert_eq!(resultado("1÷0", "1 / 0\n"), None);
        assert_eq!(resultado("7", "7\n"), None);
        assert_eq!(resultado("2+", ""), None);
    }
}
