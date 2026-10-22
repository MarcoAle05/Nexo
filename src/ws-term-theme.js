// Los colores de una terminal de un workspace, sacados de los tokens del tema que ya tiene su
// ventana (ws-terminal.js los lee con getComputedStyle; las pruebas, de styles.css). Así la
// terminal va con el tema y su contraste es el que ya se mide para el código del lector:
// verde, amarillo, azul y magenta son los del código (--code-string, --code-type,
// --code-function, --code-keyword) y rojo y cian, los propios de la terminal (--term-red,
// --term-cyan). Sin DOM: se prueba con node.

// Un color de CSS ya resuelto (hex o rgb/rgba) a [r, g, b, a]; null si no se entiende.
export function parseColor(value) {
  const v = String(value ?? '').trim();
  let m = v.match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (m) {
    const h = m[1].length === 3 ? [...m[1]].map((c) => c + c).join('') : m[1];
    return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)).concat(1);
  }
  m = v.match(/^rgba?\(([^)]+)\)$/);
  if (!m) return null;
  const parts = m[1].split(/[\s,/]+/).filter(Boolean).map(Number);
  if (parts.length < 3 || parts.some((n) => Number.isNaN(n))) return null;
  return [parts[0], parts[1], parts[2], parts[3] ?? 1];
}

// Un color con transparencia encima de otro opaco.
export const over = ([r, g, b, a], [R, G, B]) => [r * a + R * (1 - a), g * a + G * (1 - a), b * a + B * (1 - a), 1];
const mix = (a, b, t) => [0, 1, 2].map((i) => a[i] * (1 - t) + b[i] * t).concat(1);
export const hex = (c) => `#${c.slice(0, 3).map((n) => Math.round(Math.max(0, Math.min(255, n))).toString(16).padStart(2, '0')).join('')}`;
export function luminance([r, g, b]) {
  const lin = (c) => ((c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}
export function contrast(a, b) {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

// Los colores que se usan para escribir: el texto y los ocho de ANSI que no son el fondo (los
// demás, el negro de los temas oscuros y el blanco de los claros, son para rellenos).
export const TEXT_COLORS = ['foreground', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'brightBlack', 'brightRed', 'brightGreen', 'brightYellow', 'brightBlue', 'brightMagenta', 'brightCyan'];

// El tema de xterm.js. `get(nombre)` da el valor de un token en la ventana y `pageBg`, el fondo
// de la página (lo que hay detrás de la ventana, que es translúcida).
export function terminalTheme(get, pageBg) {
  const page = parseColor(pageBg) ?? [5, 5, 7, 1];
  const solid = (name, fallback) => {
    const c = parseColor(get(name));
    return c ? over(c, background) : fallback;
  };
  const background = over(parseColor(get('--glass')) ?? [12, 12, 16, 1], page);
  const fg = solid('--fg', [245, 245, 245, 1]);
  // Oscura si el texto es más claro que el fondo (Arena tiene un fondo de tono medio y texto oscuro).
  const dark = luminance(fg) > luminance(background);
  const color = (name, fallback) => hex(solid(name, fallback));
  const ansi = {
    red: color('--term-red', dark ? [255, 154, 146, 1] : [180, 35, 42, 1]),
    green: color('--code-string', fg),
    yellow: color('--code-type', fg),
    blue: color('--code-function', fg),
    magenta: color('--code-keyword', fg),
    cyan: color('--term-cyan', fg),
  };
  const faint = color('--faint', fg);
  const dim = color('--dim', fg);
  return {
    background: hex(background),
    foreground: hex(fg),
    cursor: color('--accent-soft', fg),
    cursorAccent: hex(background),
    // La selección es un lavado del acento, con su transparencia (xterm la pinta encima).
    selectionBackground: get('--accent-wash-strong')?.trim() || 'rgba(128, 128, 128, 0.35)',
    black: dark ? hex(mix(background, fg, 0.28)) : hex(fg),
    brightBlack: faint,
    white: dark ? dim : hex(mix(background, fg, 0.18)),
    brightWhite: dark ? hex(fg) : hex(mix(background, fg, 0.08)),
    ...ansi,
    brightRed: ansi.red,
    brightGreen: ansi.green,
    brightYellow: ansi.yellow,
    brightBlue: ansi.blue,
    brightMagenta: ansi.magenta,
    brightCyan: ansi.cyan,
  };
}

// Los tokens que lee terminalTheme (para leerlos de una vez).
export const THEME_TOKENS = ['--glass', '--fg', '--dim', '--faint', '--accent-soft', '--accent-wash-strong', '--term-red', '--term-cyan', '--code-string', '--code-type', '--code-function', '--code-keyword'];
