// Temas de nexo: cada uno es un fondo y unas tarjetas («Negro con gris»: fondo negro, tarjetas
// grises). Los colores de la interfaz están en styles.css (un bloque `[data-theme]` por tema);
// aquí va la lista que enseña Configuración (`scheme` es el del fondo) y las paletas de los
// lienzos (galaxia y grafo), que van sobre el fondo y no heredan el CSS. El tema elegido se
// guarda en `localStorage` (`nexo.theme`) y `public/tema.js` lo pone en <html> antes de pintar.

// Paletas del grafo: neones suaves sobre los fondos oscuros y los mismos tonos más hondos sobre
// los claros (un neón claro no se ve sobre blanco). `spark` es el núcleo de cada estrella
// (blanco en los oscuros; en los claros, el propio color) e `ink` el RGB de las etiquetas.
const GRAPH_DARK = {
  nebula: ['#b69cff', '#7fe7ff', '#ff8ad8', '#7dffcf', '#8fa8ff', '#ffb0c8', '#c9a8ff', '#9ff0ff'],
  star: '#ffd89a',
  nova: '#d6f4ff',
  core: '#ece6ff',
  ghost: '#a99bd6',
  spark: '#ffffff',
  ink: [255, 255, 255],
};
const GRAPH_LIGHT = {
  nebula: ['#6a4bd6', '#0a86a8', '#c2378a', '#0e9068', '#3b5bd0', '#c4456a', '#7a52d6', '#127f96'],
  star: '#b36b00',
  nova: '#1f7aa6',
  core: '#4a3c99',
  ghost: '#8a80b0',
  spark: null,
  ink: [0, 0, 0],
};

// Negro claro con gris (grafito) toma la paleta de la captura de referencia: naranja, verde,
// rojo y amarillo, y sus grises.
const GRAPH_GRAFITO = {
  nebula: ['#fc8c45', '#00d594', '#ff696d', '#febc2e', '#c1c1c1', '#ffb27a', '#7fe8c4', '#ff9a9d'],
  star: '#febc2e',
  nova: '#cecece',
  core: '#e6e6e6',
  ghost: '#7a7a7a',
  spark: '#ffffff',
  ink: [206, 206, 206],
};

// Galaxia: tintes de los puntos (RGB lineal de 0 a 1, en el orden de TINTS de galaxy.js:
// blanco, lila, violeta, azul y rosa en Órbita), cómo se mezclan y cuánto se apagan. Sobre un
// fondo oscuro la luz se suma (`add`); sobre uno claro, sumar luz no se ve, así que los puntos
// son tinta que se pinta encima (`over`) y algo más tenue (`gain`) para que el núcleo no sea
// una mancha.
const galaxy = (blend, gain, tints) => ({ blend, gain, tints });

export const THEMES = [
  {
    id: 'orbita',
    name: 'Órbita',
    hint: 'Negro frío con violeta',
    scheme: 'dark',
    galaxy: galaxy('add', 1, [[1, 1, 1], [0.8, 0.68, 1], [0.66, 0.46, 1], [0.38, 0.52, 1], [1, 0.52, 0.84]]),
    graph: GRAPH_DARK,
  },
  {
    id: 'negro',
    name: 'Negro con blanco',
    hint: 'Tarjetas blancas sobre negro',
    scheme: 'dark',
    galaxy: galaxy('add', 1, [[1, 1, 1], [0.86, 0.86, 0.9], [0.72, 0.72, 0.76], [0.6, 0.62, 0.68], [0.92, 0.92, 0.94]]),
    graph: GRAPH_DARK,
  },
  {
    id: 'grafito',
    name: 'Negro claro con gris',
    hint: 'Tarjetas grises con naranja',
    scheme: 'dark',
    galaxy: galaxy('add', 0.9, [[0.93, 0.93, 0.93], [0.81, 0.81, 0.81], [0.99, 0.55, 0.27], [0.6, 0.6, 0.6], [0.9, 0.9, 0.9]]),
    graph: GRAPH_GRAFITO,
  },
  {
    id: 'azul',
    name: 'Negro con azul',
    hint: 'Tarjetas azul marino',
    scheme: 'dark',
    galaxy: galaxy('add', 1, [[0.93, 0.95, 1], [0.62, 0.78, 1], [0.45, 0.65, 1], [0.3, 0.5, 0.95], [0.75, 0.85, 1]]),
    graph: GRAPH_DARK,
  },
  {
    id: 'verde',
    name: 'Negro con verde',
    hint: 'Tarjetas verde bosque',
    scheme: 'dark',
    galaxy: galaxy('add', 1, [[0.92, 1, 0.95], [0.55, 0.9, 0.72], [0.42, 0.82, 0.62], [0.3, 0.65, 0.5], [0.75, 0.95, 0.85]]),
    graph: GRAPH_DARK,
  },
  {
    id: 'vino',
    name: 'Negro con vino',
    hint: 'Tarjetas color vino',
    scheme: 'dark',
    galaxy: galaxy('add', 1, [[1, 0.94, 0.94], [1, 0.66, 0.7], [0.95, 0.5, 0.56], [0.75, 0.35, 0.42], [1, 0.8, 0.82]]),
    graph: GRAPH_DARK,
  },
  {
    id: 'ambar',
    name: 'Negro con ámbar',
    hint: 'Tarjetas café con ámbar',
    scheme: 'dark',
    galaxy: galaxy('add', 1, [[1, 0.96, 0.9], [0.95, 0.75, 0.45], [0.93, 0.66, 0.32], [0.75, 0.5, 0.25], [1, 0.86, 0.66]]),
    graph: GRAPH_DARK,
  },
  {
    id: 'gris',
    name: 'Gris con negro',
    hint: 'Tarjetas negras sobre gris',
    scheme: 'light',
    galaxy: galaxy('over', 0.55, [[0.04, 0.04, 0.05], [0.16, 0.16, 0.18], [0.26, 0.26, 0.29], [0.1, 0.1, 0.12], [0.2, 0.2, 0.22]]),
    graph: GRAPH_LIGHT,
  },
  {
    id: 'niebla',
    name: 'Gris claro con azul claro',
    hint: 'Tarjetas azul claro',
    scheme: 'light',
    galaxy: galaxy('over', 0.75, [[0.07, 0.1, 0.16], [0.18, 0.42, 0.78], [0.33, 0.6, 0.92], [0.1, 0.26, 0.55], [0.4, 0.66, 0.94]]),
    graph: { ...GRAPH_LIGHT, ink: [15, 23, 42] },
  },
  {
    id: 'blanco',
    name: 'Blanco con negro',
    hint: 'Tarjetas negras sobre blanco',
    scheme: 'light',
    galaxy: galaxy('over', 0.65, [[0.02, 0.02, 0.02], [0.14, 0.14, 0.14], [0.26, 0.26, 0.26], [0.08, 0.08, 0.08], [0.2, 0.2, 0.2]]),
    graph: GRAPH_LIGHT,
  },
  {
    id: 'claro',
    name: 'Claro',
    hint: 'Blanco con letras negras, a lo Apple',
    scheme: 'light',
    galaxy: galaxy('over', 0.75, [[0.11, 0.11, 0.12], [0, 0.44, 0.89], [0.37, 0.36, 0.9], [0.04, 0.32, 0.7], [0.62, 0.32, 0.86]]),
    graph: GRAPH_LIGHT,
  },
];

export const DEFAULT_THEME = 'orbita';
const KEY = 'nexo.theme';

export const themeById = (id) => THEMES.find((t) => t.id === id) ?? THEMES.find((t) => t.id === DEFAULT_THEME);

// El tema que hay puesto (el de <html>, que puso public/tema.js con lo guardado).
export function currentTheme(root = document.documentElement) {
  return themeById(root.dataset.theme ?? DEFAULT_THEME);
}

const listeners = new Set();
// Avisa de cada cambio de tema (los lienzos cambian su paleta). Devuelve cómo dejar de oír.
export function onTheme(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

// Pone un tema y lo guarda. Las transiciones de color se apagan durante el cambio (si no,
// cada borde y cada fondo se fundiría a su ritmo y el cambio se vería a trozos): se aplica con
// una hoja que las anula, se fuerza el cálculo de estilos y se quita a los dos fotogramas.
export function applyTheme(id, root = document.documentElement) {
  const theme = themeById(id);
  const calm = document.createElement('style');
  calm.textContent = '*,*::before,*::after{transition:none !important}';
  document.head.append(calm);
  if (theme.id === DEFAULT_THEME) delete root.dataset.theme;
  else root.dataset.theme = theme.id;
  void document.body.offsetHeight;
  requestAnimationFrame(() => requestAnimationFrame(() => calm.remove()));
  try {
    if (theme.id === DEFAULT_THEME) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, theme.id);
  } catch {
    // Sin almacenamiento, el tema dura lo que la ventana.
  }
  for (const fn of listeners) fn(theme);
  return theme;
}
