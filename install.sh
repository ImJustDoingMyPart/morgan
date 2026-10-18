#!/usr/bin/env bash
# Instala morgan para el usuario, sin paquete (con paquete: dist/arch/PKGBUILD).
#
#   ./install.sh                 compila e instala en ~/.local
#   ./install.sh --prefijo DIR   los binarios en DIR/bin (las unidades apuntan ahí)
#   ./install.sh --seco          muestra lo que tocaría el sistema (sudo, systemctl) sin hacerlo
#   ./install.sh --desinstalar   saca lo instalado; la config (~/.config/morgan) queda
#
# Lo que hace:
#   - compila morgan, morganctl y morgan-lock (cargo, --release --locked);
#   - instala los binarios y los scripts de dist/ (morgan-captura, morgan-cuota);
#   - instala el tema propio y sus plantillas en ~/.local/share/morgan (se elige en el menú
#     de sistema, Apariencia; mientras no se elige ninguno, morgan usa esos colores);
#   - instala las unidades de systemd --user y la activación por D-Bus de las
#     notificaciones (en ~/.local/share, que se busca antes que la de los paquetes);
#   - instala el servicio PAM de morgan-lock en /etc/pam.d (pide sudo);
#   - corre `morgan setup`: agrega `pcall(require, "morgan")` al final de
#     ~/.config/hypr/hyprland.lua, habilita morgan.service, avisa qué compite con morgan y, si
#     no hay otro servidor de notificaciones, lo arranca.
set -euo pipefail

raiz=$(cd "$(dirname "$0")" && pwd)
prefijo="$HOME/.local"
seco=0
accion=instalar

while (($#)); do
    case "$1" in
    --prefijo) prefijo=${2:?falta el directorio}; shift ;;
    --seco) seco=1 ;;
    --desinstalar) accion=desinstalar ;;
    -h | --help) sed -n '2,/^set -e/p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 0 ;;
    *) echo "install.sh: no conozco «$1» (ver --help)" >&2; exit 2 ;;
    esac
    shift
done

config="${XDG_CONFIG_HOME:-$HOME/.config}"
datos="${XDG_DATA_HOME:-$HOME/.local/share}"
bin="$prefijo/bin"
unidades="$config/systemd/user"
dbus="$datos/dbus-1/services/org.freedesktop.Notifications.service"
compartido="$datos/morgan"
pam=/etc/pam.d/morgan-lock
hypr="$config/hypr/hyprland.lua"

paso() { printf '\033[1m==>\033[0m %s\n' "$*"; }
aviso() { printf '\033[33m  !\033[0m %s\n' "$*" >&2; }
error() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# Lo que toca el sistema o los servicios del usuario: con --seco solo se muestra.
sistema() {
    if ((seco)); then
        printf '  [seco] %s\n' "$*"
    else
        "$@"
    fi
}

# --- Dependencias ------------------------------------------------------------------------

# programa, paquete de Arch, para qué. Sin ellos morgan anda; le falta eso.
opcionales=(
    "matugen|matugen|temas (también los curados: las plantillas pasan por matugen)"
    "awww|awww|fondos de pantalla"
    "vipsthumbnail|libvips|miniaturas de la banda de fondos"
    "hyprsunset|hyprsunset|luz nocturna"
    "ddcutil|ddcutil|brillo de monitores externos (DDC/CI)"
    "hyprshot|hyprshot|capturas de pantalla (Print)"
    "cliphist|cliphist|launcher: portapapeles"
    "wtype|wtype|launcher: escribir símbolos"
    "qalc|libqalculate|calculadora"
    "rofimoji|rofimoji|launcher: datos de los símbolos"
    "canberra-gtk-play|libcanberra|sonidos del sistema"
    "uwsm|uwsm|lanzar apps como unidades de systemd (sin uwsm, en un scope propio)"
    "gsr-ui-cli|gpu-screen-recorder-ui|indicador y control de grabación"
    "papirus-folders|papirus-folders|color de las carpetas del tema de íconos"
    "jq|jq|morgan-cuota (cuotas de Claude Code y Antigravity)"
)

revisar_dependencias() {
    paso "Dependencias"
    command -v cargo >/dev/null || error "falta cargo (rust)"
    command -v pkg-config >/dev/null || error "falta pkg-config"
    local m faltan=()
    for m in "gtk4 >= 4.22" gtk4-layer-shell-0 libpulse libpulse-mainloop-glib pam; do
        pkg-config --exists "$m" || faltan+=("$m")
    done
    ((${#faltan[@]} == 0)) || error "faltan bibliotecas para compilar: ${faltan[*]}"

    command -v Hyprland >/dev/null || error "falta Hyprland"
    local v
    v=$(Hyprland --version 2>/dev/null | sed -n 's/^Hyprland \([0-9]*\.[0-9]*\).*/\1/p' | head -1)
    if [[ -n $v ]] && ! printf '0.56\n%s\n' "$v" | sort -V -C; then
        error "morgan necesita Hyprland 0.56 o más nuevo (hay $v)"
    fi
    [[ -f $hypr ]] || error "morgan necesita la config de Hyprland en Lua ($hypr)"

    local o p paquete para falta=0
    for o in "${opcionales[@]}"; do
        IFS='|' read -r p paquete para <<<"$o"
        if ! command -v "$p" >/dev/null; then
            ((falta++)) || aviso "opcionales que no están (morgan anda sin ellos):"
            printf '      %-24s %s\n' "$paquete" "$para" >&2
        fi
    done
}

# --- Instalar ----------------------------------------------------------------------------

instalar() {
    revisar_dependencias

    paso "Compilando"
    (cd "$raiz" && cargo build --release --locked -p morgan -p morganctl -p morgan-lock)

    paso "Binarios en $bin"
    local b
    for b in morgan morganctl morgan-lock; do
        install -Dm755 "$raiz/target/release/$b" "$bin/$b"
    done
    install -Dm755 "$raiz/dist/morgan-captura" "$bin/morgan-captura"
    install -Dm755 "$raiz/dist/morgan-cuota" "$bin/morgan-cuota"
    case ":$PATH:" in
    *":$bin:"*) ;;
    *) aviso "$bin no está en el PATH: los binds de Hyprland llaman a morganctl por nombre" ;;
    esac

    paso "Tema y plantillas en $compartido"
    mkdir -p "$compartido"
    cp -r "$raiz/dist/share/morgan/." "$compartido/"

    paso "Unidades de systemd y D-Bus"
    local u
    mkdir -p "$unidades"
    for u in morgan.service morgan-lock.service; do
        sed "s|%h/.local/bin|$bin|g" "$raiz/dist/$u" >"$unidades/$u"
    done
    install -Dm644 "$raiz/dist/org.freedesktop.Notifications.service" "$dbus"

    paso "PAM ($pam)"
    if cmp -s "$raiz/dist/pam.d/morgan-lock" "$pam"; then
        echo "  ya está"
    else
        sistema sudo install -Dm644 "$raiz/dist/pam.d/morgan-lock" "$pam"
    fi

    sistema systemctl --user daemon-reload

    paso "morgan setup"
    sistema "$bin/morgan" setup
    echo "  Config: $config/morgan/config.toml, o el menú de sistema (Super+coma)"
}

# --- Desinstalar -------------------------------------------------------------------------

desinstalar() {
    paso "Servicios"
    sistema systemctl --user disable --now morgan.service || true

    paso "Archivos"
    local f
    for f in "$bin"/{morgan,morganctl,morgan-lock,morgan-captura,morgan-cuota} \
        "$unidades"/{morgan.service,morgan-lock.service} "$dbus" "$config/hypr/morgan.lua"; do
        if [[ -e $f ]]; then rm -v "$f"; fi
    done
    # Lo que trajo install.sh; lo que el usuario agregó en esa carpeta queda.
    (cd "$raiz/dist/share/morgan" && find . -type f) | while read -r f; do
        if [[ -e $compartido/$f ]]; then rm -v "$compartido/$f"; fi
    done
    if [[ -e $pam ]]; then sistema sudo rm -v "$pam"; fi
    sistema systemctl --user daemon-reload

    paso "Hyprland ($hypr)"
    if [[ -f $hypr ]] && grep -qxF 'pcall(require, "morgan")' "$hypr"; then
        # La línea y el comentario que puso `morgan setup`; si la escribió otro, solo la línea.
        sed -i -e '/^-- morgan: sus binds, los colores del tema/,/^pcall(require, "morgan")$/d' \
            -e '/^pcall(require, "morgan")$/d' "$hypr"
        echo "  sacada la línea de morgan"
    fi
    echo "  La config ($config/morgan) y el estado (~/.local/state/morgan) quedan."
}

"$accion"
