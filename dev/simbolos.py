#!/usr/bin/env python3
"""Arma crates/morgan/src/launcher/simbolos.tsv, los emoji del launcher con sus nombres.

    dev/simbolos.py [carpeta]

Baja la lista de Unicode (emoji-test.txt: cuáles y en qué orden) y las anotaciones de CLDR
en castellano y en inglés (nombre y palabras clave), las dos con la Licencia de Unicode v3,
y escribe una línea por emoji:

    emoji \t nombre es \t claves es \t nombre en \t claves en \t grupo

Solo los totalmente calificados y sin los tonos de piel ni de pelo (salen en otro lugar del
teclado del sistema; acá serían seis filas iguales por cada persona). Lo bajado va a
`carpeta` (por defecto una temporal nueva), que no se ejecuta ni se importa.
"""

import json
import sys
import tempfile
import urllib.request
from pathlib import Path

CLDR = "48.2.3"
EMOJI = "https://unicode.org/Public/emoji/latest/emoji-test.txt"
RAIZ = f"https://raw.githubusercontent.com/unicode-org/cldr-json/{CLDR}/cldr-json"
ANOTACIONES = {
    (idioma, derivadas): f"{RAIZ}/cldr-annotations{'-derived' if derivadas else ''}-full/"
    f"annotations{'Derived' if derivadas else ''}/{idioma}/annotations.json"
    for idioma in ("es", "en")
    for derivadas in (False, True)
}
MODIFICADORES = {chr(c) for c in range(0x1F3FB, 0x1F400)} | {
    "\U0001F9B0", "\U0001F9B1", "\U0001F9B2", "\U0001F9B3",
}
SALIDA = Path(__file__).resolve().parent.parent / "crates/morgan/src/launcher/simbolos.tsv"


def bajar(url: str, destino: Path) -> Path:
    if not destino.exists():
        with urllib.request.urlopen(url, timeout=60) as r:
            destino.write_bytes(r.read())
    return destino


def sin_fe0f(s: str) -> str:
    return s.replace("️", "")


def anotaciones(archivo: Path) -> dict[str, tuple[str, list[str]]]:
    datos = json.loads(archivo.read_text(encoding="utf-8"))
    raiz = datos.get("annotations") or datos.get("annotationsDerived")
    out = {}
    for emoji, a in raiz["annotations"].items():
        nombre = (a.get("tts") or [""])[0]
        out[sin_fe0f(emoji)] = (nombre, a.get("default", []))
    return out


def limpio(s: str) -> str:
    return " ".join(s.replace("\t", " ").split())


def main() -> None:
    carpeta = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(tempfile.mkdtemp(prefix="simbolos-"))
    carpeta.mkdir(parents=True, exist_ok=True)
    lista = bajar(EMOJI, carpeta / "emoji-test.txt").read_text(encoding="utf-8")
    nombres = {"es": {}, "en": {}}
    for (idioma, derivadas), url in ANOTACIONES.items():
        nombre = f"{idioma}{'-derivadas' if derivadas else ''}.json"
        nombres[idioma].update(anotaciones(bajar(url, carpeta / nombre)))

    filas, faltan, grupo = [], 0, ""
    for linea in lista.splitlines():
        if linea.startswith("# group:"):
            grupo = linea.split(":", 1)[1].strip()
            continue
        if not linea or linea.startswith("#") or "; fully-qualified" not in linea:
            continue
        emoji = linea.split("#", 1)[1].split()[0]
        if any(m in emoji for m in MODIFICADORES) and len(emoji) > 1:
            continue
        clave = sin_fe0f(emoji)
        es, en = nombres["es"].get(clave), nombres["en"].get(clave)
        if not es or not en or not es[0] or not en[0]:
            faltan += 1
            continue
        claves = lambda n, c: ", ".join(limpio(x) for x in c if x.lower() != n.lower())
        filas.append("\t".join([
            emoji,
            limpio(es[0]), claves(es[0], es[1]),
            limpio(en[0]), claves(en[0], en[1]),
            grupo,
        ]))

    cabecera = (
        f"# Emoji de Unicode (emoji-test.txt) con los nombres de CLDR {CLDR} en castellano e\n"
        "# inglés. Licencia de Unicode v3 (ver CREDITOS.md). Lo genera dev/simbolos.py: no se\n"
        "# edita a mano.\n"
    )
    SALIDA.write_text(cabecera + "\n".join(filas) + "\n", encoding="utf-8")
    print(f"{len(filas)} emoji en {SALIDA} ({faltan} sin nombre en CLDR); datos en {carpeta}")


if __name__ == "__main__":
    main()
