#!/usr/bin/env python3
"""Empaqueta los cuadros de `morgan about` (ver PLAN.md, "Acerca de").

Entrada: rgb24 crudo de ffmpeg, ya con la paleta aplicada (pocos colores).
Salida, todo en bytes y enteros little-endian:

    b"MRGN1", ancho u8, alto u8, cuadros u16, fps u8, colores u8, paleta (colores × rgb)

y después cada cuadro como diferencia contra el anterior (el primero, contra uno todo en el
color 0), recorrido en orden de filas hasta cubrir ancho × alto píxeles:

    0x00–0x7F  n = byte + 1: n píxeles iguales al cuadro anterior
    0x80–0xFF  n = byte − 0x7F: siguen n índices de paleta

El tramado ordenado deja casi sin corridas dentro de un cuadro, pero tres de cada cuatro
píxeles se repiten del cuadro anterior: por eso diferencia y no RLE.

uso: acerca-cuadros.py ANCHO ALTO FPS ENTRADA.rgb SALIDA.bin
"""
import sys


def main():
    ancho, alto, fps = (int(a) for a in sys.argv[1:4])
    with open(sys.argv[4], "rb") as f:
        crudo = f.read()
    area = ancho * alto
    tam = area * 3
    if len(crudo) % tam:
        sys.exit("acerca-cuadros: el tamaño no es múltiplo de un cuadro")

    paleta = {}
    cuadros = []
    for i in range(0, len(crudo), tam):
        c = crudo[i : i + tam]
        cuadros.append(bytes(paleta.setdefault(c[p : p + 3], len(paleta)) for p in range(0, tam, 3)))
    if len(paleta) > 256:
        sys.exit("acerca-cuadros: más de 256 colores; falta la paleta en ffmpeg")

    salida = bytearray(b"MRGN1")
    salida += bytes([ancho, alto]) + len(cuadros).to_bytes(2, "little") + bytes([fps, len(paleta) % 256])
    for color in paleta:  # los dict guardan el orden de inserción: el índice es la posición
        salida += color

    anterior = bytes(area)
    for c in cuadros:
        p = 0
        while p < area:
            n = 0
            while p + n < area and c[p + n] == anterior[p + n] and n < 128:
                n += 1
            if n:
                salida.append(n - 1)
                p += n
                continue
            n = 0
            while p + n < area and c[p + n] != anterior[p + n] and n < 128:
                n += 1
            salida.append(0x7F + n)
            salida += c[p : p + n]
            p += n
        anterior = c

    with open(sys.argv[5], "wb") as f:
        f.write(salida)
    print(f"{len(cuadros)} cuadros, {len(paleta)} colores, {len(salida)} bytes")


main()
