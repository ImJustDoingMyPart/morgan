
use gtk::prelude::*;

use super::ecualizador::Ecualizador;
use super::marquesina::Marquesina;
use crate::mpris::{Mpris, Reproduccion};
use crate::reproductor::Estado;

const CARATULA: i32 = 72;

#[derive(Clone)]
pub struct Vista {
    pub raiz: gtk::Box,
    caratula: gtk::Image,
    titulo: Marquesina,
    ecualizador: Ecualizador,
    /// Al reabrir el centro se retoma, aunque el reproductor no avise nada (no cambió).
    sonando: std::cell::Cell<bool>,
    subtitulo: gtk::Label,
    play: gtk::Button,
    anterior: gtk::Button,
    siguiente: gtk::Button,
    mezcla: gtk::Button,
    repetir: gtk::Button,
}

const APAGADO: f64 = 0.5;

impl Vista {
    pub fn new(mpris: &Mpris) -> Self {
        let raiz = gtk::Box::new(gtk::Orientation::Vertical, 0);
        raiz.add_css_class("widget");
        raiz.add_css_class("widget-mpris");
        let carrusel = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        raiz.append(&carrusel);
        let jugador = gtk::Box::new(gtk::Orientation::Vertical, 0);
        jugador.add_css_class("widget-mpris-player");
        jugador.set_overflow(gtk::Overflow::Hidden);
        jugador.set_hexpand(true);
        carrusel.append(&jugador);
        let capa = gtk::Box::new(gtk::Orientation::Vertical, 0);
        capa.add_css_class("mpris-overlay");
        // El ecualizador es el fondo de la tarjeta: debajo de todo, del tamaño que la tarjeta
        // tome por su contenido (la capa es la que se mide).
        let ecualizador = Ecualizador::new();
        let fondo = gtk::Overlay::new();
        fondo.set_child(Some(&ecualizador));
        fondo.add_overlay(&capa);
        fondo.set_measure_overlay(&capa, true);
        jugador.append(&fondo);

        let arriba = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        arriba.set_vexpand(true);
        capa.append(&arriba);
        let caratula = gtk::Image::new();
        caratula.set_pixel_size(CARATULA);
        caratula.set_overflow(gtk::Overflow::Hidden);
        caratula.add_css_class("widget-mpris-album-art");
        arriba.append(&caratula);
        let textos = gtk::Box::new(gtk::Orientation::Vertical, 4);
        textos.set_hexpand(true);
        textos.set_valign(gtk::Align::Center);
        arriba.append(&textos);
        let etiqueta = |clase: &str| {
            let l = gtk::Label::new(None);
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            l.set_xalign(0.0);
            l.set_yalign(0.0);
            l.add_css_class(clase);
            textos.append(&l);
            l
        };
        let titulo = Marquesina::new();
        titulo.etiqueta.add_css_class("widget-mpris-title");
        textos.append(&titulo.raiz);

        let subtitulo = etiqueta("widget-mpris-subtitle");

        let botones = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        botones.set_halign(gtk::Align::Center);
        capa.append(&botones);
        let boton = |icono: &str, f: Box<dyn Fn(&Mpris)>| {
            let b = gtk::Button::from_icon_name(icono);
            b.set_has_frame(false);
            for c in ["circular", "image-button", "flat"] {
                b.add_css_class(c);
            }
            let m = mpris.clone();
            b.connect_clicked(move |_| f(&m));
            botones.append(&b);
            b
        };
        let mezcla = boton(
            "media-playlist-shuffle-symbolic",
            Box::new(|m| m.alternar_mezcla()),
        );
        let anterior = boton(
            "media-seek-backward-symbolic",
            Box::new(|m| m.pedir("Previous")),
        );
        let play = boton(
            "media-playback-start-symbolic",
            Box::new(|m| m.pedir("PlayPause")),
        );
        let siguiente = boton("media-seek-forward-symbolic", Box::new(|m| m.pedir("Next")));
        let repetir = boton(
            "media-playlist-repeat-symbolic",
            Box::new(|m| m.ciclar_repeticion()),
        );

        let v = Vista {
            raiz,
            caratula,
            titulo,
            ecualizador,
            sonando: Default::default(),
            subtitulo,
            play,
            anterior,
            siguiente,
            mezcla,
            repetir,
        };
        v.mostrar(None);
        v
    }

    pub fn detener(&self) {
        self.ecualizador.apagar();
    }

    pub fn reanudar(&self) {
        if self.sonando.get() {
            self.ecualizador.encender();
        } else {
            self.ecualizador.apagar();
        }
    }

    pub fn mostrar(&self, r: Option<Reproduccion>) {
        let Some(r) = r else {
            self.raiz.set_visible(false);
            self.sonando.set(false);
            self.ecualizador.apagar();
            return;
        };
        self.sonando.set(r.estado == Some(Estado::Reproduciendo));
        self.reanudar();
        self.raiz.set_visible(true);
        self.titulo.set_text(&r.titulo);
        self.subtitulo.set_text(&r.subtitulo);
        self.subtitulo.set_visible(!r.subtitulo.is_empty());
        match &r.caratula {
            Some(p) if p.exists() => {
                self.caratula.set_from_file(Some(p));
                self.caratula.set_visible(true);
            }
            _ => self.caratula.set_visible(false),
        }
        let opacidad = |b: &gtk::Button, encendido: bool| {
            if let Some(h) = b.child() {
                h.set_opacity(if encendido { 1.0 } else { APAGADO });
            }
        };
        self.mezcla.set_visible(r.mezcla.is_some());
        opacidad(&self.mezcla, r.mezcla == Some(true));
        self.repetir.set_visible(r.repetir.is_some());
        let repite = r.repetir.as_deref().unwrap_or("None");
        self.repetir.set_icon_name(if repite == "Track" {
            "media-playlist-repeat-song-symbolic"
        } else {
            "media-playlist-repeat-symbolic"
        });
        opacidad(&self.repetir, repite != "None");
        self.play.set_icon_name(match r.estado {
            Some(Estado::Reproduciendo) => "media-playback-pause-symbolic",
            _ => "media-playback-start-symbolic",
        });
        self.anterior.set_sensitive(r.puede_anterior);
        self.siguiente.set_sensitive(r.puede_siguiente);
    }
}
