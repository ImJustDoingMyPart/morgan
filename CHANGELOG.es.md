# Registro de cambios

[English](CHANGELOG.md) · **Español**

## Sin publicar

### Las notificaciones se limpian solas

Una notificación nueva reemplaza a la anterior de la misma app cuando dice lo mismo con otro número, como «63 actualizaciones disponibles» y «6 actualizaciones disponibles», o «Batería al 20 %». No reemplaza nada si trae una vista previa (una captura), si es un mensaje, un correo o un archivo recibido, o si la app está en `[notifications] accumulate`.

El historial guarda como mucho 5 notificaciones por app (`per_app`). Lo que ya se vio al abrir el centro se va a las 24 horas (`seen_hours`), y lo que nunca se miró sigue durando `retention_days`. Las críticas se quedan hasta que se cierran, salvo las que piden reiniciar («Reboot recommended»), que se van solas después de reiniciar. Las apps de `close_on_focus` (por ejemplo `["kitty"]`, donde avisan los agentes de terminal) cierran su notificación al volver a la ventana que la mandó.

### La bandeja pasa al dock

Los íconos de la bandeja ahora están en el dock. El de una app que ya tiene lugar ahí (Steam, Vesktop) se junta a ese ícono, y su menú sale con el click derecho. Lo que está en el dock solo por la bandeja (OpenRGB, Cachy-Update) va al final, después de una raya. Una app que corre de fondo sin ventanas lleva un punto hueco. La raya que separaba las apps más usadas de las otras abiertas ya no está.

El click derecho en cualquier ícono del dock abre un menú con sus ventanas, «Ventana nueva», las acciones de la app (como «Ventana privada nueva» en Brave), «Cerrar todas» y las opciones de su bandeja. Sale al apretar el botón, y mientras está abierto el dock no se esconde. El click medio cierra la ventana si hay una sola. Si hay varias, abre la lista, y el click medio sobre una fila cierra esa. «Ventana nueva» pasó del click medio al menú.

En la barra, sin la flecha de la bandeja, los avisos de grabación, micrófono y pantalla compartida quedan junto a la campana. Con el dock apagado (`[dock] show = false`), la bandeja vuelve a la barra como antes. `[dock] tray_icons` cambia el ícono de algo de la bandeja en el dock, por su `Id` (por ejemplo `"Cachy-Update" = "octopi"`).

En Umbriel, si `UMBRIEL_SOCKET` apunta a un socket que ya no existe (lo deja así un Umbriel de prueba en el entorno de `systemctl --user`), morgan usa el de la sesión y lo corrige para los comandos `umbriel` que lanza. Antes la barra mostraba todos los workspaces apagados y el menú no podía hablar con Umbriel.

## 0.3.2 · 2026-11-22

### morgan-greet en Umbriel, y pantallas que ya no se apagan solas

morgan-greet ahora también corre sobre Umbriel, sin depender de Hyprland, y cada monitor arranca en su frecuencia más alta. `morgan-greet install` usa el compositor que esté instalado y, si están los dos, pregunta cuál. Para fijarlo sin que pregunte, `sudo morgan-greet install --compositor umbriel --apply`.

La pantalla de inicio anota lo que pasa al arrancar en `/var/lib/morgan-greet/pantalla.log`, que se vacía en cada arranque. Ahí queda, por ejemplo, si el teclado entró.

Si `morgan-lock` terminaba sin desbloquear (por un cierre inesperado, por ejemplo), la sesión seguía marcada como bloqueada y las pantallas se apagaban a los 30 segundos sin uso. Ahora morgan ve que el bloqueo ya no corre y saca la marca.

### Umbriel sin restos de Hyprland, y las apps en el modo del tema

En Umbriel, la página Atajos del menú, el modo Atajos del launcher y la grabación de una tecla nueva leen los atajos de Umbriel. Antes le preguntaban a Hyprland y quedaban vacíos. Las reglas por app de Escritorio se escriben para Umbriel, salvo «Sin animación», que Umbriel no tiene. El cursor elegido en Apariencia cambia en el momento, y Información del sistema muestra la versión de Umbriel.

Las apps siguen el modo del tema. morgan pone `color-scheme` de GNOME y `gtk-application-prefer-dark-theme` en oscuro con los temas oscuros y en claro con los claros, incluida la variante clara de Matugen. Los navegadores y las apps de Electron lo leen por `xdg-desktop-portal-gtk`, que `morgan setup` ahora ofrece. Se apaga con `disabled = ["color-scheme"]` en `themes.toml`.

Si el Bluetooth está bloqueado por software (rfkill), prenderlo desde el centro lo desbloquea. Si lo bloquea un interruptor del equipo, la tarjeta lo dice en vez de rebotar. `morgan-greet install` avisa si falta greetd o si el inicio de sesión lo maneja otro gestor, con el comando para cambiarlo.

### Atajos de ventanas, y menos primeros arranques rotos

`morgan setup` ahora ofrece los atajos de ventanas con los que se armó morgan: foco y movimiento de columnas, workspaces, monitores, layout scrolling, apps que ciclan y grabación de pantalla. Los escribe para Hyprland (`morgan-atajos.lua`) y para Umbriel (`morgan-atajos.toml`) desde una sola tabla, como archivos aparte que se pueden editar. Las teclas que ya usás no se tocan. `morgan setup --deshacer` los aparta como `.bak`. En Umbriel, las teclas que ciclan usan el script nuevo `umbriel-ciclar`, que necesita `jq`.

Los íconos son glifos de JetBrainsMono Nerd Font y sin ella salían como cuadros vacíos. `morgan-bin` y `morgan-git` ahora dependen de `ttf-jetbrains-mono-nerd`, `install.sh` avisa si falta y `morgan setup` la ofrece. El README también dice que el interruptor de Bluetooth necesita `bluez` y su servicio, que `morgan setup` instala y activa.

## 0.3.1 · 2026-11-20

### `morgan setup` prepara el equipo

`morgan setup` ya no solo avisa qué compite con morgan. Busca 21 programas (barras, servidores de notificaciones, idle, bloqueos, shells completos, agentes de polkit, fondos y luz nocturna), corriendo o configurados para arrancar, y ofrece pararlos. Deshabilita su unidad de systemd, comenta la línea de la config de Hyprland que los lanza (con una copia del archivo al lado) o los saca de las apps de inicio.

También ofrece instalar lo que falte de los programas recomendados, con pacman, y `papirus-folders` con yay o paru. Si hay Bluetooth o no hay perfiles de energía, los agrega a la lista. Cambiar quién maneja la red, el audio o la energía no lo hace solo: con iwd, PulseAudio o TLP, por ejemplo, escribe los pasos para pasar a NetworkManager, PipeWire o power-profiles-daemon.

Todo se muestra en una lista antes de hacerse. `morgan setup --seco` muestra lo que haría sin hacer nada, y `morgan setup --deshacer` devuelve lo que se paró y saca morgan del arranque.

## 0.3.0 · 2026-11-20

### Una pantalla para entrar: morgan-greet

morgan ahora tiene su propia pantalla de inicio de sesión. Es la del bloqueo, con el mismo
reloj, el mismo fondo y el mismo tema, pero para entrar: corre en
[greetd](https://sr.ht/~kennylevinsen/greetd/) y deja elegir el usuario y la sesión de a una
cosa, con la clave al final. Cada monitor arranca en su frecuencia más alta, y la clave abre
también el llavero de GNOME si está instalado.

Es **opcional** y no se instala sola. `sudo morgan-greet install` muestra qué va a cambiar sin
tocar nada; con `--apply` lo hace, respaldando antes la config de greetd y su PAM. Vale desde
el próximo arranque. Para volver atrás: `sudo morgan-greet restore --apply`, también desde un
login de texto (Ctrl+Alt+F2) si el greeter no arrancara. El tema y el fondo de cada cuenta se
copian a `/var/lib/morgan-greet` cada vez que cambian; nada más de la cuenta sale de ella.

### Más allá de Hyprland: Umbriel (experimental)

Lo que morgan le pedía a Hyprland pasa por una capa común con un driver por compositor. El
primero además de Hyprland es [Umbriel](https://github.com/noctalia-dev/umbriel): workspaces,
ventanas, el idle y las pantallas, la luz nocturna (ahora sin hyprsunset fuera de Hyprland),
las miniaturas, la pantalla compartida y los colores, bordes, sombras y animaciones del tema.
`morgan setup` le arma la config. En el menú de sistema, lo que es solo de Hyprland aparece
deshabilitado y dice por qué. Con un compositor que trae su propia vista de ventanas, la tecla
y la esquina abren esa.

Hyprland sigue siendo el compositor principal; en Umbriel, morgan anda pero todavía puede
tener huecos.

### Nuevo

- **Dock**: abajo, con las apps más usadas y las abiertas, un punto por ventana y un globo con
  los títulos al pasar el mouse. Puede ocultarse solo y nunca aparece sobre un juego.
- **Vista de ventanas desde la esquina**: llevar el mouse a la esquina superior izquierda la
  abre, con una animación que se despliega desde el medio. Se elige con un click. Se apaga en
  General › Preferencias.
- **Escucharme**: en la lista de micrófonos del centro de control, oír tu propio micrófono,
  con el nivel de la voz en el slider.
- **Bloqueo**: con varios monitores, el campo sigue al puntero y los demás muestran solo el
  fondo. El campo ya no tiene letras: un intento fallido se ve en la sacudida, el rojo y una
  muesca por intento.
- **Menús**: el de la bandeja y los globos de la barra y el dock comparten un mismo aspecto,
  se ajustan a su contenido y tienen el blur de la barra.

### Arreglado

- Un monitor que se reconecta con las pantallas apagadas por el idle se vuelve a apagar.
- Elegir una ventana desde la vista de ventanas (o con Mod+Tab) lleva a esa ventana, no solo a
  su workspace.
- Miniaturas grises y sin ícono en la vista de ventanas (un error de vello que afectaba a toda
  superficie).
- El dock ya no mueve el puntero al saltar a una ventana.

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
