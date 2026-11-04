// Ventanas del centro de un workspace: qué pestañas hay, cuáles se ven (hasta MAX_OPEN a la vez
// en mosaico, una sola en el otro modo) y cómo se reparten en el hueco que dejan las cartas.
// Sin DOM: se prueba con node.

export const MAX_OPEN = 4;
// Medidas por debajo de las que una ventana deja de servir: una terminal necesita unas 40
// columnas y unas líneas; el código, unas columnas.
export const MIN_W = 380;
export const MIN_H = 300;

// Cómo caben `n` ventanas en `width` × `height`: columnas, filas y la celda de cada una
// (líneas de la rejilla desde 1, como `grid-column`). Se ponen lado a lado mientras cada una
// tenga su ancho mínimo; si no, gana el reparto en el que la ventana más justa respira más.
export function tileLayout(n, width, height, { minW = MIN_W, minH = MIN_H, gap = 12 } = {}) {
  const count = Math.max(0, Math.min(MAX_OPEN, n));
  if (count === 0) return { cols: 0, rows: 0, cells: [] };
  const cell = (col, row, colSpan = 1, rowSpan = 1) => ({ col, row, colSpan, rowSpan });
  const grid = (cols, rows) => {
    const cells = [];
    for (let i = 0; i < count; i++) cells.push(cell((i % cols) + 1, Math.floor(i / cols) + 1));
    return { cols, rows, cells };
  };
  const options = {
    1: [grid(1, 1)],
    2: [grid(2, 1), grid(1, 2)],
    // Con tres en dos columnas, la primera ocupa la columna izquierda entera.
    3: [grid(3, 1), { cols: 2, rows: 2, cells: [cell(1, 1, 1, 2), cell(2, 1), cell(2, 2)] }, grid(1, 3)],
    4: [grid(2, 2), grid(4, 1), grid(1, 4)],
  }[count];
  const size = (layout, c) => ({
    w: ((width - gap * (layout.cols - 1)) / layout.cols) * c.colSpan + gap * (c.colSpan - 1),
    h: ((height - gap * (layout.rows - 1)) / layout.rows) * c.rowSpan + gap * (c.rowSpan - 1),
  });
  const fits = (layout) => layout.cells.every((c) => {
    const s = size(layout, c);
    return s.w >= minW && s.h >= minH;
  });
  // El primero que cabe entero, en el orden de preferencia de arriba.
  const fit = options.find(fits);
  if (fit) return fit;
  const score = (layout) => Math.min(...layout.cells.map((c) => {
    const s = size(layout, c);
    return Math.min(s.w / minW, s.h / minH);
  }));
  return options.reduce((best, layout) => (score(layout) > score(best) + 1e-9 ? layout : best));
}

// Caben `n` ventanas sin que ninguna baje de su medida mínima.
export function fits(n, width, height, { minW = MIN_W, minH = MIN_H, gap = 12 } = {}) {
  const layout = tileLayout(n, width, height, { minW, minH, gap });
  return layout.cells.every((c) => {
    const w = ((width - gap * (layout.cols - 1)) / layout.cols) * c.colSpan + gap * (c.colSpan - 1);
    const h = ((height - gap * (layout.rows - 1)) / layout.rows) * c.rowSpan + gap * (c.rowSpan - 1);
    return w >= minW && h >= minH;
  });
}

// Estado de las ventanas: `tabs` (todas, en su orden), `open` (las que se ven, en su sitio),
// `mru` (de la usada hace menos a la que más) y `mode` (`mosaico` o `sola`).
export function emptyWindows() {
  return { tabs: [], open: [], mru: [], mode: 'mosaico', active: null };
}

const without = (list, key) => list.filter((k) => k !== key);
const touch = (mru, key) => [key, ...without(mru, key)];

// Normaliza lo guardado (o lo que venga) para que siempre cuadre.
export function normalizeWindows(saved, keys = null) {
  const base = emptyWindows();
  const s = saved && typeof saved === 'object' ? saved : {};
  const valid = (k) => typeof k === 'string' && (!keys || keys.has(k));
  const tabs = [...new Set((Array.isArray(s.tabs) ? s.tabs : []).filter(valid))];
  const mode = s.mode === 'sola' ? 'sola' : 'mosaico';
  let open = [...new Set((Array.isArray(s.open) ? s.open : []).filter((k) => tabs.includes(k)))].slice(0, mode === 'sola' ? 1 : MAX_OPEN);
  const mru = [...new Set([...(Array.isArray(s.mru) ? s.mru : []).filter((k) => tabs.includes(k)), ...tabs])];
  const active = tabs.includes(s.active) ? s.active : (open[0] ?? null);
  if (mode === 'sola' && active && !open.includes(active)) open = [active];
  return { ...base, tabs, open, mru, mode, active };
}

// Añade una pestaña al final si no está (no la abre).
export function addTab(state, key) {
  if (state.tabs.includes(key)) return state;
  return { ...state, tabs: [...state.tabs, key], mru: [...state.mru, key] };
}

// Abre (o enseña) una ventana. En mosaico va junto a las demás; si ya hay MAX_OPEN, deja su
// sitio la que se usó hace más. En «sola», ocupa todo el centro. Devuelve el estado y la
// ventana que se minimizó para hacerle sitio, si alguna.
export function openWindow(state, key) {
  const s = addTab(state, key);
  let open = s.open;
  let evicted = null;
  if (s.mode === 'sola') {
    evicted = open.find((k) => k !== key) ?? null;
    open = [key];
  } else if (!open.includes(key)) {
    if (open.length < MAX_OPEN) open = [...open, key];
    else {
      // La de uso más antiguo de las que se ven.
      evicted = [...s.mru].reverse().find((k) => open.includes(k) && k !== key) ?? open[0];
      open = open.map((k) => (k === evicted ? key : k));
    }
  }
  return { state: { ...s, open, mru: touch(s.mru, key), active: key }, evicted };
}

// Una ventana nueva (una terminal que se abre): se ve si estamos en mosaico y cabe junto a las
// que ya se ven (`room`: el hueco tiene sitio para una más con su medida mínima); si no, queda
// en su pestaña, sin minimizar ninguna. En «sola» ocupa el centro.
export function openIfFits(state, key, room) {
  if (state.mode === 'sola') return openWindow(state, key).state;
  if (state.open.length < MAX_OPEN && room) return openWindow(state, key).state;
  return addTab(state, key);
}

// La ventana que se usa ahora (foco en ella), sin cambiar cuáles se ven.
export function focusWindow(state, key) {
  if (!state.open.includes(key)) return state;
  return { ...state, mru: touch(state.mru, key), active: key };
}

// Minimizar: deja de verse pero su pestaña sigue. La activa pasa a la usada justo antes.
export function minimizeWindow(state, key) {
  const open = without(state.open, key);
  const active = state.active === key ? (state.mru.find((k) => open.includes(k)) ?? null) : state.active;
  return { ...state, open, active };
}

// Cerrar una pestaña (los archivos): desaparece del todo.
export function closeTab(state, key) {
  const s = minimizeWindow(state, key);
  return { ...s, tabs: without(s.tabs, key), mru: without(s.mru, key) };
}

// Cambiar de modo. A «sola» se queda la activa; en mosaico sigue esa y las que se abran
// después van a su lado, hasta MAX_OPEN.
export function setMode(state, mode) {
  const next = mode === 'sola' ? 'sola' : 'mosaico';
  if (next === state.mode) return state;
  if (next === 'sola') {
    const keep = state.active ?? state.open[0] ?? null;
    return { ...state, mode: next, open: keep ? [keep] : [] };
  }
  return { ...state, mode: next };
}

// Pestaña anterior o siguiente (flechas en la barra de pestañas).
export function neighbor(tabs, key, step) {
  if (!tabs.length) return null;
  const at = tabs.indexOf(key);
  return tabs[(at + step + tabs.length) % tabs.length];
}

// La rueda sobre la barra de pestañas la desplaza en horizontal: lo que mueve una rueda normal
// (vertical) pasa a ser horizontal y las líneas o páginas se pasan a píxeles.
export function wheelDelta(event, page = 400) {
  const scale = event.deltaMode === 1 ? 32 : event.deltaMode === 2 ? page : 1;
  const main = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY;
  return main * scale;
}
