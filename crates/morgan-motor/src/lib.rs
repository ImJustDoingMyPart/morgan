//! El motor de dibujo de morgan, sin toolkit: wgpu y vello para pintar, parley para el texto.
//! Lo comparten las superficies que ya dejaron GTK (`PLAN.md`, "Motor propio").

pub mod color;
pub mod forma;
pub mod gpu;
pub mod texto;

pub use parley;
pub use vello;
