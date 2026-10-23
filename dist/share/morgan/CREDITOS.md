# Créditos de morgan

Lo que va en `themes/` y `templates/` es propio de morgan, con licencia MIT (`LICENSE`, al
lado de este archivo), salvo lo que se nombra acá, que conserva la suya. El resto de morgan,
el código, es GPL-3.0-or-later.

## Los emoji del launcher (`crates/morgan/src/launcher/simbolos.tsv`)

La lista y el orden salen de `emoji-test.txt` de Unicode, y los nombres y las palabras clave
en castellano y en inglés, de las anotaciones de **CLDR** (<https://cldr.unicode.org>,
versión 48.2.3). Los dos están bajo la Licencia de Unicode v3, compatible con la GPL; la
tabla los junta sin cambiarlos (`dev/simbolos.py`).

> UNICODE LICENSE V3
>
> COPYRIGHT AND PERMISSION NOTICE
>
> Copyright © 1991-2026 Unicode, Inc.
>
> NOTICE TO USER: Carefully read the following legal agreement. BY
> DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
> SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
> TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
> DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.
>
> Permission is hereby granted, free of charge, to any person obtaining a
> copy of data files and any associated documentation (the "Data Files") or
> software and any associated documentation (the "Software") to deal in the
> Data Files or Software without restriction, including without limitation
> the rights to use, copy, modify, merge, publish, distribute, and/or sell
> copies of the Data Files or Software, and to permit persons to whom the
> Data Files or Software are furnished to do so, provided that either (a)
> this copyright and permission notice appear with all copies of the Data
> Files or Software, or (b) this copyright and permission notice appear in
> associated Documentation.
>
> THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
> KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
> MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
> THIRD PARTY RIGHTS.
>
> IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
> BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
> OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
> WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
> ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
> FILES OR SOFTWARE.
>
> Except as contained in this notice, the name of a copyright holder shall
> not be used in advertising or otherwise to promote the sale, use or other
> dealings in these Data Files or Software without prior written
> authorization of the copyright holder.

## El bloqueo (`crates/morgan-lock/src/pintor.rs`)

Cómo se le pasa a wgpu la superficie de Wayland (el display de libwayland envuelto para
`HasDisplayHandle`, la superficie por `SurfaceTargetUnsafe`) y cómo se elige su
configuración (formato sin sRGB, alfa premultiplicado, Mailbox) siguen a **amane**
(<https://github.com/MystiaFin/amane>, `src/graphics/gpu/target.rs`), bajo licencia MIT:

> MIT License
>
> Copyright (c) 2026 MystiaFin
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.

## templates/vesktop.css

Los nombres de las variables de color de Discord son los que mantiene al día **Midnight**
(<https://github.com/refact0r/midnight-discord>), que los sigue con cada versión de Discord.
La plantilla no copia su código: solo recolorea esas variables.

> MIT License — Copyright (c) 2025 refact0r

## templates/qt.conf, btop.theme, tmtheme.xml, yazi-flavor.toml, obsidian.css

Portadas de las plantillas de **Noctalia** (<https://github.com/noctalia-dev/noctalia-shell>),
a la sintaxis de matugen; cada cabecera dice qué se cambió.

> MIT License — Copyright (c) 2026 noctalia-dev

## themes/

Las paletas curadas son las oficiales de cada proyecto, todas MIT salvo Tokyo Night
(Apache-2.0); cada `palette.toml` dice de qué archivo sale. Los 16 colores de terminal salen
de las versiones para kitty de cada tema (en kovidgoyal/kitty-themes, cada archivo con su
autor y su licencia, MIT en todos los que se usan).

| Tema | Proyecto | Licencia |
|---|---|---|
| catppuccin-mocha | catppuccin/palette, catppuccin/kitty | MIT |
| dracula | dracula/visual-studio-code, dracula/kitty | MIT |
| everforest | sainnhe/everforest | MIT |
| gruvbox-material | sainnhe/gruvbox-material | MIT |
| kanagawa | rebelot/kanagawa.nvim | MIT |
| nord | nordtheme/nord | MIT |
| rose-pine | rose-pine/palette, rose-pine/kitty | MIT |
| tokyo-night | folke/tokyonight.nvim | Apache-2.0 |

`morgan` es propio.
