-- Busca la ventana de la app (entre las de sus clases, en orden) y si no hay ninguna la abre;
-- con varias, va pasando de una a la otra.
local function morgan_buscar(clases, cmd)
    return function()
        local wins = {}
        for _, w in ipairs(hl.get_windows()) do
            if w.mapped then
                for _, c in ipairs(clases) do
                    if w.class == c or w.initial_class == c then
                        wins[#wins + 1] = w
                        break
                    end
                end
            end
        end
        if #wins == 0 then
            hl.exec_cmd(cmd)
            return
        end
        table.sort(wins, function(a, b) return a.stable_id < b.stable_id end)
        local activa, n = hl.get_active_window(), 0
        if activa then
            for i, w in ipairs(wins) do
                if w.stable_id == activa.stable_id then
                    n = i
                    break
                end
            end
        end
        hl.dispatch(hl.dsp.focus({ window = wins[n % #wins + 1] }))
    end
end
