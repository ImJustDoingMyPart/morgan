-- Config del Hyprland anidado de pruebas (`dev/anidado`). Sin autoarranque y sin los binds
-- reales: nada de acá adentro puede actuar sobre la sesión de verdad.

hl.monitor({ output = "", mode = "preferred", position = "auto", scale = 1 })
-- La salida virtual y la de la ventana apagada (las escribe `dev/anidado arrancar`).
pcall(dofile, (os.getenv("XDG_RUNTIME_DIR") or "") .. "/morgan-dev/monitores.lua")

hl.bind("SUPER + SHIFT + E", hl.dsp.exit())

-- Fondo liso y sin logo: el wallpaper por defecto mete ruido en las capturas de comparación.
hl.config({
    misc = {
        disable_hyprland_logo    = true,
        disable_splash_rendering = true,
        force_default_wallpaper  = 0,
        background_color         = "0x110e07",
        -- Para probar morgan-lock: matar el bloqueo y lanzar otro sin cerrar el anidado.
        allow_session_lock_restore = true,
    },
})

-- El lienzo de ventanas muestra cintas de scrolling.
hl.config({ general = { layout = "scrolling", gaps_in = 8, gaps_out = 16 } })

-- Para el modo atajos del launcher: un bind con descripción que solo anota en el log del
-- anidado.
hl.bind("SUPER + F12", hl.dsp.exec_cmd("echo atajo-de-prueba >>\"$XDG_RUNTIME_DIR/morgan-dev/comandos.log\""), { desc = "Atajo de prueba" })

-- morgan.lua, con las reglas de capa y los binds de morgan. En el anidado va a la carpeta
-- de dev/anidado (MORGAN_HYPR_CFG), no a ~/.config/hypr/morgan.lua.
pcall(dofile, (os.getenv("XDG_RUNTIME_DIR") or "") .. "/morgan-dev/hypr-morgan.lua")
