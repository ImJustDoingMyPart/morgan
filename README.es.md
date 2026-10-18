# morgan

[English](README.md) · **Español**

Un shell de escritorio para [Hyprland](https://hyprland.org), escrito en Rust con GTK 4:
barra, centro de control, notificaciones, launcher, OSD, pantalla de bloqueo e idle, en un
solo proceso y con un mismo aspecto.

<!-- video -->

## Qué trae

- **Barra** con los workspaces, la bandeja, el reloj (con un calendario y el clima) y un
  indicador de privacidad cuando el micrófono o una captura de pantalla están en uso.
- **Centro de control**: red, Bluetooth, No molestar, caffeine, luz nocturna, energía,
  volumen por aplicación, micrófono, brillo, el reproductor y las notificaciones.
- **Notificaciones**: un servidor completo de la especificación freedesktop, con No molestar
  y un modo juego que hace silencio mientras se juega.
- **Launcher**: aplicaciones, símbolos y emoji, historial del portapapeles, calculadora y
  los atajos de teclado.
- **Menú de sistema**: los ajustes de morgan y de Hyprland mismo (apariencia, entrada,
  atajos…), escritos en la config.
- **Pantalla de bloqueo e idle** (`morgan-lock`): reemplaza a hyprlock y a hypridle. Bloquea
  a través de logind, así que `loginctl lock-session` funciona, y puede bloquear antes de
  suspender.
- **OSD** de volumen, brillo y Bloq Mayús; una **vista de ventanas** (`Super+Tab`); un
  **selector de fondos**; **notas de atajos** por aplicación; un **calendario** que lee las
  URL iCal de cada uno.
- **Temas**: nueve paletas incluidas (morgan, Catppuccin, Dracula, Everforest, Gruvbox,
  Kanagawa, Nord, Rosé Pine, Tokyo Night) o los colores del fondo de pantalla, aplicados a
  morgan y, si se elige, a Hyprland, GTK, kitty, btop y más.
- **Inglés y castellano**, según el idioma del sistema.

## Requisitos

- **Arch Linux** o una derivada (en otras distros, compilar desde el código con
  `install.sh`).
- **Hyprland 0.56 o más nuevo, con la config en Lua** (`~/.config/hypr/hyprland.lua`).
  morgan no funciona con un `hyprland.conf`.
- GTK 4.22 o más nuevo y `gtk4-layer-shell`.

Las herramientas opcionales (fondos, luz nocturna, capturas, portapapeles, calculadora…) las
listan el paquete e `install.sh`, cada una con para qué sirve. morgan funciona sin ellas.

## Antes de instalar: parar lo que morgan reemplaza

morgan es un shell completo. **Cualquier otro programa que dibuje una barra, muestre
notificaciones o bloquee la pantalla va a chocar con él**: dos barras una encima de la otra,
el otro programa quedándose con las notificaciones, dos pantallas de bloqueo o la pantalla
que se bloquea dos veces.

**No hace falta desinstalar** nada. Hace falta **pararlo y que no arranque con la sesión**:

| Si se usa… | Qué hacer |
|---|---|
| Otro shell: **Noctalia, DankMaterialShell, Caelestia, HyprPanel**, o cualquier armado con Quickshell, AGS o eww | Sacar la línea que lo arranca de la config de Hyprland, o deshabilitar su unidad de systemd (`systemctl --user disable --now <unidad>`). |
| Una barra: **Waybar** | Lo mismo. |
| Un servidor de notificaciones: **swaync, mako, dunst** | Lo mismo. Las notificaciones solo pueden ser de un programa. |
| Un OSD: **SwayOSD** | Lo mismo. |
| Un bloqueo o un idle: **hyprlock, hypridle, swaylock, swayidle** | Lo mismo. morgan bloquea y maneja el idle, incluidos los inhibidores de navegadores, reproductores de video y juegos. |
| Luz nocturna: **wlsunset, gammastep** | Lo mismo, si se quiere la luz nocturna de morgan (usa hyprsunset). |
| Un programa de fondos: **hyprpaper, swaybg** | Lo mismo, si se quiere el selector de fondos de morgan (usa awww). |

El **gestor de inicio de sesión** (SDDM, GDM, greetd, ly…) no molesta: corre antes de la
sesión, y la pantalla de bloqueo de morgan es otra cosa. uwsm tampoco, y es opcional.

`morgan setup` (más abajo) avisa de los más comunes si están corriendo, y no arranca morgan
mientras otro programa tenga las notificaciones.

## Instalación

Desde el AUR, ya compilado o compilado desde el último commit:

```sh
yay -S morgan-bin     # o: yay -S morgan-git
```

O desde el código, en `~/.local` (revisa las dependencias y dice qué falta):

```sh
git clone https://github.com/ImJustDoingMyPart/morgan
cd morgan
./install.sh          # --help para ver las opciones
```

Después, **una vez por usuario y desde Hyprland**:

```sh
morgan setup
```

Agrega una línea al final de `~/.config/hypr/hyprland.lua`, habilita el servicio del
usuario, avisa qué compite con morgan y lo arranca.

## Uso

| Teclas | |
|---|---|
| `Super+Espacio` | Launcher |
| `Super+,` | Menú de sistema (todos los ajustes) |
| `Super+N` | Centro de control |
| `Super+Tab` | Vista de ventanas |
| `Super+V` | Historial del portapapeles |
| `Super+Alt+U` | Símbolos y emoji |
| `Super+Alt+C` | Calculadora |
| `Super+Y` | Selector de fondos |
| `Super+Alt+N` | Notas de atajos de la app con foco |
| `Super+Shift+N` | No molestar |
| `Super+X` | Apagar, reiniciar, cerrar sesión |
| `Super+Alt+L` | Bloquear |

Todos los atajos se cambian desde el menú de sistema o en `~/.config/morgan/config.toml`.
`morganctl` maneja morgan desde scripts y atajos propios (`morganctl` sin argumentos lista
todo).

## Desinstalar

Quitar el paquete (`pacman -R morgan-bin` o `morgan-git`), o correr
`./install.sh --desinstalar` si se instaló desde el código. Después, borrar la línea
`pcall(require, "morgan")` de `~/.config/hypr/hyprland.lua` y volver a habilitar lo que se
paró antes de instalar. La config de `~/.config/morgan` queda.

## Licencia

GPL-3.0-or-later. Los temas y las plantillas incluidos son MIT, con sus créditos en
[`dist/share/morgan/CREDITOS.md`](dist/share/morgan/CREDITOS.md).
