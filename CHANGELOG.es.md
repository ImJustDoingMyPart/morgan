# Registro de cambios

[English](CHANGELOG.md) · **Español**

## 0.2.0 · 2026-10-25

### Adiós a GTK4

morgan ya no usa GTK4 ni gtk4-layer-shell. Entre el 22 y el 24 de octubre, de a una
superficie, todo pasó a un motor propio: [vello](https://github.com/linebender/vello) sobre
wgpu para dibujar, [parley](https://github.com/linebender/parley) para el texto y
smithay-client-toolkit para Wayland. El orden fue la pantalla de bloqueo, el OSD, la barra con
el calendario, el centro de control con el menú de la bandeja, las notificaciones, el
launcher, el menú de sistema, la calculadora, el selector de fondos, la vista de ventanas y,
al final, el arranque mismo, que ahora corre sobre el bucle de glib.

Por qué: GTK dibuja en el mismo hilo que hace el trabajo, así que cualquier cosa lenta (leer
una config, hablar por D-Bus) congelaba la animación. Ahora cada grupo de superficies tiene su
hilo y su conexión de Wayland, y dibuja sin esperar al resto de morgan. Con el hilo principal
trabado a propósito durante un segundo y medio, el centro de control sigue animando sin perder
un cuadro. La pantalla de bloqueo, medida contra su versión con GTK, bloquea más rápido (70 ms
en vez de unos 100), usa alrededor de un 40 % menos de memoria y sigue animando mientras el
resto trabaja, en vez de congelarse.

De paso se reemplazó: los íconos se buscan según la especificación de freedesktop (los SVG con
resvg), la carpeta de fondos se elige con el portal del escritorio, el mismo selector de
archivos que usa cualquier app, y el tema ya no carga una hoja CSS.

Qué significa para quien lo usa:

- **Ya no hacen falta gtk4 ni gtk4-layer-shell.** Si nada más los usa, se pueden desinstalar.
- **morgan necesita una GPU con un driver de Vulkan que funcione** (Mesa o NVIDIA).
- Cada cambio redibuja solo la capa que lo muestra: un reloj que avanza no repinta el panel
  entero.

### Nuevo

- **Pedidos de clave (polkit)**: morgan pide la clave de administrador él mismo, en una caja
  como la del launcher, y reemplaza a hyprpolkitagent. Cancelar no cuenta como un intento
  fallido.
- **Centro de control**: conectarse a Wi-Fi nuevas, con la clave escrita ahí mismo (una mala
  tiembla y se pone roja); olvidar una red guardada deslizándola a la derecha y copiar su
  clave deslizándola a la izquierda; emparejar dispositivos Bluetooth, con la confirmación del
  código en el lugar.
- **Menú de sistema**: monitores (resolución, frecuencia, escala, rotación, posición, encendido,
  un nombre propio, ubicados arrastrándolos); fecha y hora; atajos propios y reglas por app;
  modos propios (submaps de Hyprland); el modo juego como sección de Notificaciones.
- **Launcher**: los emoji de Unicode y CLDR incluidos, con nombres en castellano, en una
  carpeta por grupo. Ya no hace falta rofimoji.
- **Sonidos**: los temas se leen como dice la especificación (`Directories`, `Inherits`,
  alias), y Escuchar dice en cada sonido si el tema trae uno propio o usa el de fábrica.
- **Aspecto**: el brillo tenue del calendario en todos los paneles, toasts, OSD, bloqueo y
  ventanas; los workspaces se funden con un leve viaje vertical.
- **`morgan setup`** avisa también por Quickshell, AGS, eww, swayidle, swaylock y Noctalia v5.

### Arreglado

- El `replaces_id` de una notificación solo reemplaza una de la misma app.
- Los tiempos del idle cuentan solo el teclado y el mouse, y solo eso prende las pantallas.
- El selector de fondos devuelve la memoria al cerrarse (antes se quedaba con cientos de MB de
  miniaturas).
- Olvidar una red ya no deja un hueco en la lista.
- La pantalla de bloqueo aparece fundiéndose con el escritorio, y un `morgan-lock` relanzado
  retoma el bloqueo que dejó.

### Quitado

- Las notas por app.

## 0.1.1 y anteriores

Ver los [releases](https://github.com/ImJustDoingMyPart/morgan/releases).
