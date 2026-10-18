# morgan

[English](README.md) · **Español**

Un shell de escritorio para [Hyprland](https://hyprland.org), escrito en Rust con GTK 4:
barra, centro de control, notificaciones, launcher, OSD, pantalla de bloqueo e idle, en un
solo proceso y con un mismo aspecto.

https://github.com/user-attachments/assets/2c7eb327-ed32-4eef-89ac-40cdce4034c0

## Por qué morgan

Armé morgan para mi propio escritorio, después de vivir con las dos formas habituales de
armar uno en Hyprland:

- **Los shells completos**, como Noctalia y DankMaterialShell. Se ven muy bien y hacen
  mucho, pero el aspecto y el comportamiento son los suyos: ir más allá de las opciones que
  ofrecen era pelearse con ellos en lugar de configurarlos.
- **Un shell hecho de programas sueltos**: rofi o walker, swaync o mako, Waybar, SwayOSD,
  hyprlock. Cada uno se puede estilar, pero cada uno es una caja cerrada. Un botón guarda
  su propio estado en vez de preguntarle al sistema, así que dos salidas de audio que se
  excluyen pueden quedar marcadas a la vez. No hay audio por aplicación, los paneles no
  crecen con su contenido ni respetan el borde de la pantalla, el estilo llega hasta donde
  llega cada programa, cada menú es un proceso nuevo, y ninguno sabe lo que hacen los demás.

morgan es un solo proceso, con un solo aspecto, que le pregunta al sistema qué está pasando
y dibuja todo como yo lo quería.

## Antes que nada: está hecho para mí

morgan cubre **mis** necesidades, en **mi** máquina, y con esa vara se hizo. **No es muy
personalizable**: no hay módulos para agregar o reordenar, la disposición es fija, y el
aspecto cambia con los colores del tema, no con una hoja de estilos propia. Algunas
decisiones solo tienen sentido para cómo uso yo la computadora. Si buscás un shell para
moldear a tu gusto, Noctalia, DankMaterialShell o un armado modular te van a servir más. Si
lo que ves acá es lo que querés, bienvenido.

## Qué trae

- **Barra** con los workspaces, la bandeja, el reloj (con un calendario y el clima) y un
  indicador de privacidad cuando el micrófono o una captura de pantalla están en uso.
- **Centro de control**: red, Bluetooth, No molestar, caffeine, luz nocturna, energía,
  cuotas de IA, volumen por aplicación, micrófono, brillo, el reproductor y las
  notificaciones. [Más abajo](#el-centro-de-control).
- **Notificaciones**: un servidor completo de la especificación freedesktop, con No molestar
  y un modo juego que hace silencio mientras se juega.
- **Launcher**: aplicaciones, símbolos y emoji, historial del portapapeles, calculadora y
  los atajos de teclado.
- **Menú de sistema**: los ajustes de morgan y de Hyprland mismo (apariencia, entrada,
  atajos, fecha y hora…), escritos en la config. Ahí también se crean atajos propios (para
  abrir una app o correr un comando) y reglas por app (workspace, flotante, tamaño,
  opacidad).
- **Pantalla de bloqueo e idle** (`morgan-lock`): reemplaza a hyprlock y a hypridle. Bloquea
  a través de logind, así que `loginctl lock-session` funciona, y puede bloquear antes de
  suspender.
- **OSD** de volumen, brillo y Bloq Mayús; una **vista de ventanas** (`Super+Tab`); un
  **selector de fondos**; un **calendario** que lee las URL iCal de cada uno.
- **Temas**: nueve paletas incluidas (morgan, Catppuccin, Dracula, Everforest, Gruvbox,
  Kanagawa, Nord, Rosé Pine, Tokyo Night) o los colores del fondo de pantalla, aplicados a
  morgan y, si se elige, a Hyprland, GTK, kitty, btop y más.
- **Inglés y castellano**, según el idioma del sistema.

## El centro de control

El corazón de morgan. `Super+N` lo abre arriba a la derecha. Crece con lo que tiene, nunca
más allá del borde de la pantalla, y si no entra se desplaza por dentro.

- **Tarjetas**, en dos columnas, cada una con su estado en palabras («Cable», «2
  conectados»):
  - **Red**: cable y VPN, con las conexiones guardadas, y la radio Wi-Fi; con la radio
    prendida, debajo, las Wi-Fi guardadas y las que están a la vista: una abierta conecta
    al click y una con clave la pide ahí mismo; si la clave no sirve, lo dice y no queda
    guardada. Una guardada se olvida deslizándola a la derecha, como una notificación, y a la
    izquierda copia su clave (sin que quede en el historial del portapapeles).
  - **Bluetooth**: el adaptador y los dispositivos emparejados, cada uno se conecta o se
    desconecta por separado; deslizarlo a la derecha lo olvida. Debajo, los que están
    cerca (se buscan mientras el centro está abierto): elegir uno lo empareja y lo conecta,
    y si pide confirmar o escribir un código, la caja aparece ahí mismo.
  - **No molestar**, **Caffeine** (la pantalla no se apaga) y **Luz nocturna**.
  - **Energía**: ahorro, equilibrado o rendimiento.
  - **Cuotas de IA**: lo que queda de los límites de Claude Code, Codex y Gemini (Antigravity).
  - **Sesión**: cuánto lleva encendida la máquina, y bloquear, suspender, cerrar sesión,
    reiniciar y apagar. Las tres últimas preguntan antes, y un doble clic no confirma sin
    querer.
- **Lo marcado es lo que dice el sistema**, nunca lo que se tocó. Si algo cambia por otro
  lado, la tarjeta lo sigue.
- **Niveles**: la salida (un clic en el nombre cambia de salida), **cada aplicación que
  suena con su propio volumen**, silencio y salida, el micrófono, y el brillo (la pantalla
  de la notebook o monitores externos por DDC/CI). Cambiar el volumen con el panel abierto
  no muestra el OSD: el slider ya lo muestra.
- **El reproductor**: el título (que se desplaza si no entra), los controles, aleatorio y
  repetir si el reproductor los tiene, y un ecualizador dibujado detrás mientras suena.
- **Notificaciones**: agrupadas por aplicación, las críticas primero, cada grupo apilado
  hasta que se abre. Un toque muestra sus acciones, deslizar a la derecha la descarta, a la
  izquierda copia su texto. Con el centro abierto no aparece nada encima.
- **Todo con el teclado**: Tab y las flechas recorren, las flechas mueven un slider, Enter
  abre una lista o hace una acción, Supr cierra una notificación, Escape cierra el centro.
- **No gasta cerrado**: escucha el audio, la red, el Bluetooth, la energía y el reproductor
  solo mientras está abierto.

## Requisitos

- **Arch Linux** o una derivada (en otras distros, compilar desde el código con
  `install.sh`).
- **Hyprland 0.56 o más nuevo, con la config en Lua** (`~/.config/hypr/hyprland.lua`).
  morgan no funciona con un `hyprland.conf`.
- GTK 4.22 o más nuevo y `gtk4-layer-shell`.

## Sobre qué está hecho

morgan no reinventa lo que otros programas ya hacen bien: los maneja y les da una sola cara.
Todo lo que sigue es opcional: morgan funciona sin eso, y la parte que lo necesita falta.
**Son los programas con los que se construyó y se probó.** Con una alternativa lo básico
suele andar, pero ahí puede haber faltantes o errores. Para quien recién llega a Hyprland,
la lista es además un buen punto de partida.

**Servicios del sistema**, que la mayoría de las instalaciones de Arch ya tienen:

| Servicio | Para qué lo usa morgan |
|---|---|
| PipeWire con `pipewire-pulse` y WirePlumber | Volumen, audio por app, micrófono, el indicador de privacidad |
| NetworkManager | La tarjeta de red (iwd o systemd-networkd solos no aparecen) |
| BlueZ | La tarjeta de Bluetooth |
| power-profiles-daemon | La tarjeta de energía |
| systemd-logind | Bloquear, suspender, apagar |

**Herramientas**:

| Programa | Qué hace en morgan |
|---|---|
| `matugen` | Los temas: todos, los incluidos y los del fondo, pasan por él |
| `awww` | Los fondos (morgan no maneja hyprpaper ni swaybg) |
| `libvips` | Las miniaturas del selector de fondos |
| `hyprsunset` | La luz nocturna |
| `ddcutil` | El brillo de monitores externos (DDC/CI) |
| `hyprshot` | Las capturas (`Print`), configuradas desde el menú de sistema |
| GPU Screen Recorder (`gpu-screen-recorder-ui`) | La grabación de pantalla: la barra muestra el tiempo y la corta con un clic. Otros grabadores no tienen ese contador |
| `cliphist` (con `wl-clipboard`) | El historial del portapapeles en el launcher |
| `libqalculate` | La calculadora |
| `cava` | El ecualizador detrás del reproductor del centro de control |
| `wtype` | Escribir emoji desde el launcher |
| `libcanberra` y un tema de sonidos | Los sonidos del sistema |
| Íconos Papirus y `papirus-folders` | El color de las carpetas, que sigue al tema |
| `uwsm` | Lanzar las apps como unidades de systemd (sin él, morgan usa un scope propio) |

**Apps que siguen al tema**: morgan trae plantillas para kitty, GTK, Qt, btop, yazi,
starship, Vesktop (Discord) y Obsidian. Cuáles aplica se elige en Apariencia. Los juegos
lanzados desde Steam prenden el modo juego solos.

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
