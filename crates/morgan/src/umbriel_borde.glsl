// El borde de la ventana con foco y su sombra, de morgan para Umbriel. Lo escribe morgan
// (umbriel_cfg.rs) con la sombra elegida en el menú: no editar a mano.
//
// Umbriel pinta el borde con colores lisos; esto lo pinta en degradé de accent_primary a
// accent_secondary, con la fórmula de Hyprland 0.57.1 para un gradiente de dos colores
// (src/render/shaders/glsl/gradient.glsl): progreso y·sin(a) + x·(1 − sin(a)) sobre la caja,
// en coordenadas normalizadas, con la interpolación en OkLab. La forma del anillo (ancho,
// radio, antialias) sale de la alfa del anillo nativo.
//
// Alrededor va la sombra de los paneles de morgan (morgan_motor::forma::panel), no la de
// Hyprland: capas de `box-shadow` de GTK sobre la caja del anillo, cada una agrandada en
// `spread`, corrida y desenfocada con una gaussiana de sigma = blur / 2. La fórmula de
// Hyprland con `range 6` y alfa 0,8 dejaba 0,45 de negro a 1,5 px del anillo, el doble que el
// panel. El rectángulo dibujado es el anillo más MARGEN (= `padding`) por lado, así que el
// grosor del borde sale de dónde empieza el hueco.
//   GLOW = 0: una línea negra de LINEA px y negro al 50 % con spread 1 y blur 6.
//   GLOW = 1: accent_secondary (tertiary) corrido (1, 1) y accent_primary corrido (−1, −1),
//   los dos al 30 % con spread 1 y blur 5.

const float ANGULO = 0.785398; // 45°
const float MARGEN = @MARGEN@;
const float GLOW = @GLOW@;
const float LINEA = 0.5; // ancho de la línea negra, en px lógicos (Roberto, 2026-11-22)

vec3 a_lineal(vec3 c) {
  return mix(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), step(0.04045, c));
}

vec3 a_srgb(vec3 c) {
  c = max(c, vec3(0.0));
  return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(0.0031308, c));
}

vec3 a_oklab(vec3 c) {
  c = a_lineal(c);
  vec3 lms = vec3(
    0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b,
    0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b,
    0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b);
  lms = pow(lms, vec3(1.0 / 3.0));
  return vec3(
    0.2104542553 * lms.x + 0.7936177850 * lms.y - 0.0040720468 * lms.z,
    1.9779984951 * lms.x - 2.4285922050 * lms.y + 0.4505937099 * lms.z,
    0.0259040371 * lms.x + 0.7827717662 * lms.y - 0.8086757660 * lms.z);
}

vec3 desde_oklab(vec3 lab) {
  vec3 lms = vec3(
    lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z,
    lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z,
    lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z);
  lms = lms * lms * lms;
  return a_srgb(vec3(
     4.0767416621 * lms.x - 3.3077115913 * lms.y + 0.2309699292 * lms.z,
    -1.2684380046 * lms.x + 2.6097574011 * lms.y - 0.3413193965 * lms.z,
    -0.0041960863 * lms.x - 0.7034186147 * lms.y + 1.7076147010 * lms.z));
}

vec3 degrade(vec2 caja) {
  // Paleta: 0.0 es accent_primary y 0.25 accent_secondary (cuatro colores en [0, 1)).
  vec3 desde = a_oklab(umbriel_palette_at(0.0).rgb);
  vec3 hasta = a_oklab(umbriel_palette_at(0.25).rgb);
  float s = sin(ANGULO);
  float progreso = clamp(caja.y * s + caja.x * (1.0 - s), 0.0, 1.0);
  return desde_oklab(mix(desde, hasta, progreso));
}

// `box-shadow` sin offset a distancia d de la caja: la gaussiana integrada,
// 0,5 · erfc((d − spread) / (sigma · √2)), con erf de Winitzki (error < 2e-4).
float cubre(float d, float spread, float blur) {
  float x = (d - spread) / (blur * 0.5 * 1.41421356);
  float x2 = x * x;
  float e = sign(x) * sqrt(1.0 - exp(-x2 * (1.27323954 + 0.147 * x2) / (1.0 + 0.147 * x2)));
  return 0.5 * (1.0 - e);
}

// Distancia al borde exterior del anillo del punto uv corrido en `px` píxeles lógicos.
float distancia(vec2 uv, vec2 px, float grosor) {
  return umbriel_border_distance(uv - px / umbriel_size) - grosor;
}

// a sobre b, premultiplicados.
vec4 sobre(vec4 a, vec4 b) {
  return a + b * (1.0 - a.a);
}

vec4 border(vec2 uv) {
  // Caja del anillo en uv: lo dibujado menos MARGEN píxeles por lado.
  vec2 margen = vec2(MARGEN) / umbriel_size;
  vec2 tamano = vec2(1.0) - 2.0 * margen;
  float anillo = umbriel_sample(uv).a;
  vec4 borde = vec4(degrade((uv - margen) / tamano) * anillo, anillo);

  // Umbriel 0.1.0 deja un píxel más entre el hueco y el margen: el antialias del anillo, casi
  // transparente (alfa 11/255 con border_width 2, medido en la salida). Sin restarlo, la
  // línea negra cae sobre ese píxel y se ven 2 px de negro.
  float grosor = umbriel_border_hole.x * umbriel_size.x - MARGEN - 1.0;
  vec4 sombra;
  if (GLOW > 0.5) {
    vec3 primario = umbriel_palette_at(0.0).rgb;
    vec3 terciario = umbriel_palette_at(0.25).rgb;
    float a1 = 0.3 * cubre(distancia(uv, vec2(1.0), grosor), 1.0, 5.0);
    float a2 = 0.3 * cubre(distancia(uv, vec2(-1.0), grosor), 1.0, 5.0);
    sombra = sobre(vec4(primario * a2, a2), vec4(terciario * a1, a1));
  } else {
    float d = distancia(uv, vec2(0.0), grosor);
    // La línea cubre de 0 a LINEA px; el píxel centrado en d va de d − 0,5 a d + 0,5. A escala 1,
    // con LINEA 0,5 el primer píxel queda negro al 50 %.
    float linea = clamp(LINEA + 0.5 - d, 0.0, 1.0);
    float suave = 0.5 * cubre(d, 1.0, 6.0);
    sombra = vec4(0.0, 0.0, 0.0, linea + suave * (1.0 - linea));
  }
  return sobre(borde, sombra);
}
