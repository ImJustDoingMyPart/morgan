
-- ─── Lienzo de ventanas (morgan) ───
-- Quien sabe cuándo se suelta {mod} es Hyprland, no morgan: la tecla entra al submap
-- `morgan-ventanas` acá mismo (sin carrera con morgan) y pide el lienzo; adentro, la misma
-- tecla pide un paso más y soltar {mod} sale del submap y le avisa a morgan. El submap no
-- tiene flechas ni Escape: van a la capa del lienzo. {escape} sale a mano si algo se traba.
--
-- Un bind `release` de una tecla modificadora no se dispara en 0.56.2 (#6946): se mira el
-- evento de la tecla y, por si se pierde, un timer mientras dure el submap.
local timer_ventanas = nil

local function soltar_ventanas()
    if timer_ventanas then
        timer_ventanas:set_enabled(false)
        timer_ventanas = nil
    end
    if hl.get_current_submap() ~= "morgan-ventanas" then
        return
    end
    hl.dispatch(hl.dsp.submap("reset"))
    hl.exec_cmd("morganctl windows release")
end

local function vigilar_ventanas()
    if timer_ventanas then
        timer_ventanas:set_enabled(false)
    end
    timer_ventanas = hl.timer(function()
        if hl.get_current_submap() ~= "morgan-ventanas" then
            if timer_ventanas then
                timer_ventanas:set_enabled(false)
                timer_ventanas = nil
            end
            return
        end
        if not (hl.is_key_down("{mod}_L") or hl.is_key_down("{mod}_R")) then
            soltar_ventanas()
        end
    end, { timeout = 40, type = "repeat" })
end

hl.unbind("{tecla}")
hl.define_submap("morgan-ventanas", function()
    hl.bind("{tecla}", function()
        vigilar_ventanas()
        hl.exec_cmd("morganctl windows")
    end)
    hl.bind("{escape}", soltar_ventanas){captura}
end)

-- state 0 es soltar; los códigos son los de xkb (evdev + 8).
hl.on("input.keyboard.key", function(keycode, _time, state)
    if state == 0 and (keycode == {codigo_l} or keycode == {codigo_r}) then
        if hl.get_current_submap() == "morgan-ventanas" then
            soltar_ventanas()
        end
    end
end)

hl.bind("{tecla}", function()
    hl.dispatch(hl.dsp.submap("morgan-ventanas"))
    vigilar_ventanas()
    hl.exec_cmd("morganctl windows")
end, { desc = "{desc}" })
