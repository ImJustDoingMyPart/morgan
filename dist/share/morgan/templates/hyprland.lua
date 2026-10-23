-- Plantilla del motor de temas de morgan -> ~/.local/state/morgan/tema/hyprland.lua
hl.config({
    general = {
        col = {
            active_border   = { colors = { "rgb({{colors.primary.default.hex_stripped}})", "rgb({{colors.tertiary.default.hex_stripped}})" }, angle = 45 },
            inactive_border = "rgba(00000000)",
        },
    },
    decoration = {
        shadow = {
            -- Mismo degradé que el borde (primary -> tertiary, 45°) al 30 %, el glow tenue de
            -- los paneles de morgan. Así `shadowangle`
            -- lo hace girar junto con el borde al enfocar (lo anima morgan: crates/morgan/src/animaciones.rs).
            color = { colors = { "rgba({{colors.primary.default.hex_stripped}}4d)", "rgba({{colors.tertiary.default.hex_stripped}}4d)" }, angle = 45 },
        },
    },
    -- Grupos (pestañas, Mod+W). El borde es el mismo degradé que el de cualquier ventana
    -- activa, y el inactivo transparente como el resto: un grupo se reconoce por su barra de
    -- pestañas, no por un marco distinto. En la barra, la pestaña activa lleva el tinte de
    -- selección de todo el sistema (primary al 25 %, como la fila elegida del launcher de
    -- morgan) y las demás el fondo de tarjeta al 85 %, como morgan y la barra.
    group = {
        col = {
            border_active   = { colors = { "rgb({{colors.primary.default.hex_stripped}})", "rgb({{colors.tertiary.default.hex_stripped}})" }, angle = 45 },
            border_inactive = "rgba(00000000)",
        },
        groupbar = {
            col = {
                active   = "rgba({{colors.primary.default.hex_stripped}}40)",
                inactive = "rgba({{colors.surface_container.default.hex_stripped}}d9)",
            },
            text_color          = "rgb({{colors.on_surface.default.hex_stripped}})",
            text_color_inactive = "rgb({{colors.on_surface_variant.default.hex_stripped}})",
        },
    },
    misc = {
        background_color = "0x{{colors.surface_container_lowest.default.hex_stripped}}",
    },
})
