//! Lo que hace cada cosa tocable del centro, del lado de GTK: el hilo del centro solo conoce su
//! número (`vista::Id`). Cada parte que se rearma (una lista, los niveles) suelta los números de
//! la vez anterior al registrar los nuevos.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use super::vista::{Id, ListaId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Parte {
    /// Lo que no cambia: las tarjetas.
    Fija,
    Lista(ListaId),
    Niveles,
    /// El campo de una lista (sus botones y Enter).
    Campo(ListaId),
}

enum Accion {
    Tocar(Rc<dyn Fn()>),
    Mover(Rc<dyn Fn(f64)>),
    Enviar(Rc<dyn Fn(String)>),
}

#[derive(Default)]
pub struct Acciones {
    siguiente: Cell<Id>,
    acciones: RefCell<HashMap<Id, Accion>>,
    partes: RefCell<HashMap<Parte, Vec<Id>>>,
}

impl Acciones {
    fn registrar(&self, p: Parte, a: Accion) -> Id {
        let id = self.siguiente.get() + 1;
        self.siguiente.set(id);
        self.acciones.borrow_mut().insert(id, a);
        self.partes.borrow_mut().entry(p).or_default().push(id);
        id
    }

    pub fn tocar(&self, p: Parte, f: impl Fn() + 'static) -> Id {
        self.registrar(p, Accion::Tocar(Rc::new(f)))
    }

    pub fn mover(&self, p: Parte, f: impl Fn(f64) + 'static) -> Id {
        self.registrar(p, Accion::Mover(Rc::new(f)))
    }

    pub fn enviar(&self, p: Parte, f: impl Fn(String) + 'static) -> Id {
        self.registrar(p, Accion::Enviar(Rc::new(f)))
    }

    /// Suelta lo registrado por `p`.
    pub fn soltar(&self, p: Parte) {
        if let Some(ids) = self.partes.borrow_mut().remove(&p) {
            let mut a = self.acciones.borrow_mut();
            for id in ids {
                a.remove(&id);
            }
        }
    }

    // Lo que se va a hacer se saca antes de hacerlo: puede volver a registrar.

    pub fn hacer(&self, id: Id) {
        let f = match self.acciones.borrow().get(&id) {
            Some(Accion::Tocar(f)) => f.clone(),
            _ => return,
        };
        f();
    }

    pub fn hacer_mover(&self, id: Id, v: f64) {
        let f = match self.acciones.borrow().get(&id) {
            Some(Accion::Mover(f)) => f.clone(),
            _ => return,
        };
        f(v);
    }

    pub fn hacer_enviar(&self, id: Id, s: String) {
        let f = match self.acciones.borrow().get(&id) {
            Some(Accion::Enviar(f)) => f.clone(),
            _ => return,
        };
        f(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rearmar_suelta_lo_viejo() {
        let a = Acciones::default();
        let n = Rc::new(Cell::new(0));
        let n2 = n.clone();
        let viejo = a.tocar(Parte::Niveles, move || n2.set(n2.get() + 1));
        a.hacer(viejo);
        assert_eq!(n.get(), 1);
        a.soltar(Parte::Niveles);
        a.hacer(viejo);
        assert_eq!(n.get(), 1);
        let n3 = n.clone();
        let fijo = a.tocar(Parte::Fija, move || n3.set(10));
        a.soltar(Parte::Niveles);
        a.hacer(fijo);
        assert_eq!(n.get(), 10);
        // Un número de otra clase no hace nada.
        a.hacer_mover(fijo, 1.0);
        assert_eq!(n.get(), 10);
    }
}
