// Geometría de los elementos del lienzo, sin DOM (se prueba con node): la retícula, dónde cae la
// punta de una flecha, por dónde va y qué se selecciona. Las coordenadas son las del lienzo, las
// mismas que las cartas de notas y las del motor (nexo_core::lienzo).

export const GRID = 22;
export const WORLD = { w: 40000, h: 28000 };
// La tabla relacional: lo que mide su cabecera y cada fila, para unir una flecha a una columna.
export const ENTITY = { head: 44, row: 30 };

// La retícula de puntos está en el (0, 0) de la superficie, que en el lienzo es la esquina del mundo.
const snap = (v, half) => Math.round((v + half) / GRID) * GRID - half;
export const snapX = (v) => snap(v, WORLD.w / 2);
export const snapY = (v) => snap(v, WORLD.h / 2);

export const centerOf = (r) => ({ x: r.x + r.w / 2, y: r.y + r.h / 2 });
export const intersects = (a, b) => a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
export const contains = (outer, r) => r.x >= outer.x && r.y >= outer.y && r.x + r.w <= outer.x + outer.w && r.y + r.h <= outer.y + outer.h;

export function boundsOfRects(rects) {
  if (!rects.length) return null;
  const x = Math.min(...rects.map((r) => r.x));
  const y = Math.min(...rects.map((r) => r.y));
  return { x, y, w: Math.max(...rects.map((r) => r.x + r.w)) - x, h: Math.max(...rects.map((r) => r.y + r.h)) - y };
}

// El lado de un rectángulo que mira hacia un punto.
export function sideToward(rect, point) {
  const c = centerOf(rect);
  const dx = (point.x - c.x) / Math.max(rect.w, 1);
  const dy = (point.y - c.y) / Math.max(rect.h, 1);
  if (Math.abs(dx) >= Math.abs(dy)) return dx >= 0 ? 'e' : 'o';
  return dy >= 0 ? 's' : 'n';
}

export function anchorPoint(rect, side) {
  const c = centerOf(rect);
  switch (side) {
    case 'n':
      return { x: c.x, y: rect.y };
    case 's':
      return { x: c.x, y: rect.y + rect.h };
    case 'e':
      return { x: rect.x + rect.w, y: c.y };
    default:
      return { x: rect.x, y: c.y };
  }
}

// Una punta de flecha como punto: `{ x, y }` si va suelta; si va a un elemento, su centro.
function endCenter(end, get) {
  if (end?.id == null) return { x: Number(end?.x) || 0, y: Number(end?.y) || 0 };
  const el = get(end.id);
  return el ? centerOf(el) : null;
}

// El punto exacto de una punta, y el lado por el que sale. `toward` es hacia dónde va la flecha.
function endPoint(end, get, toward) {
  if (end?.id == null) return { x: Number(end?.x) || 0, y: Number(end?.y) || 0, side: null };
  const el = get(end.id);
  if (!el) return null;
  // En una tabla relacional, a la fila de esa columna, por el lado que mire al otro extremo.
  if (end.columna && el.tipo === 'entidad') {
    const at = (el.datos?.columnas ?? []).findIndex((c) => c.nombre === end.columna || c.id === end.columna);
    if (at >= 0) {
      const side = toward.x >= el.x + el.w / 2 ? 'e' : 'o';
      return { x: side === 'e' ? el.x + el.w : el.x, y: el.y + ENTITY.head + ENTITY.row * (at + 0.5), side };
    }
  }
  const side = ['n', 'e', 's', 'o'].includes(end.ancla) ? end.ancla : sideToward(el, toward);
  return { ...anchorPoint(el, side), side };
}

// Las dos puntas de una flecha. Null si alguna apunta a algo que no está (aún no ha llegado).
export function arrowEnds(datos, get) {
  const a = endCenter(datos?.desde, get);
  const b = endCenter(datos?.hasta, get);
  if (!a || !b) return null;
  const from = endPoint(datos.desde, get, b);
  const to = endPoint(datos.hasta, get, a);
  return from && to ? { from, to } : null;
}

const isHorizontal = (side) => side === 'e' || side === 'o';
const OUT = { n: [0, -1], s: [0, 1], e: [1, 0], o: [-1, 0] };
const STUB = 22;

// Los vértices de una ruta en ángulo: sale y llega perpendicular al lado de cada elemento.
export function elbowPoints(from, to) {
  const a = { x: from.x, y: from.y };
  const b = { x: to.x, y: to.y };
  const sideA = from.side ?? (Math.abs(b.x - a.x) >= Math.abs(b.y - a.y) ? (b.x >= a.x ? 'e' : 'o') : b.y >= a.y ? 's' : 'n');
  const sideB = to.side ?? (Math.abs(b.x - a.x) >= Math.abs(b.y - a.y) ? (b.x >= a.x ? 'o' : 'e') : b.y >= a.y ? 'n' : 's');
  const p = { x: a.x + OUT[sideA][0] * STUB, y: a.y + OUT[sideA][1] * STUB };
  const q = { x: b.x + OUT[sideB][0] * STUB, y: b.y + OUT[sideB][1] * STUB };
  const mid = [];
  if (isHorizontal(sideA) && isHorizontal(sideB)) {
    const x = (p.x + q.x) / 2;
    mid.push({ x, y: p.y }, { x, y: q.y });
  } else if (!isHorizontal(sideA) && !isHorizontal(sideB)) {
    const y = (p.y + q.y) / 2;
    mid.push({ x: p.x, y }, { x: q.x, y });
  } else if (isHorizontal(sideA)) {
    mid.push({ x: q.x, y: p.y });
  } else {
    mid.push({ x: p.x, y: q.y });
  }
  const points = [a, p, ...mid, q, b];
  // Sin vértices repetidos ni en línea recta: las esquinas redondeadas salen mal con ellos.
  const clean = [];
  for (const pt of points) {
    const last = clean.at(-1);
    if (last && Math.abs(last.x - pt.x) < 0.01 && Math.abs(last.y - pt.y) < 0.01) continue;
    const prev = clean.at(-2);
    if (prev && last && ((Math.abs(prev.x - last.x) < 0.01 && Math.abs(last.x - pt.x) < 0.01) || (Math.abs(prev.y - last.y) < 0.01 && Math.abs(last.y - pt.y) < 0.01))) clean.pop();
    clean.push(pt);
  }
  return clean;
}

function roundedPath(points, radius = 8) {
  let d = `M ${points[0].x} ${points[0].y}`;
  for (let i = 1; i < points.length - 1; i++) {
    const [p, c, n] = [points[i - 1], points[i], points[i + 1]];
    const r = Math.min(radius, Math.hypot(c.x - p.x, c.y - p.y) / 2, Math.hypot(n.x - c.x, n.y - c.y) / 2);
    const toward = (a, b, len) => {
      const l = Math.hypot(b.x - a.x, b.y - a.y) || 1;
      return { x: a.x + ((b.x - a.x) / l) * len, y: a.y + ((b.y - a.y) / l) * len };
    };
    const s = toward(c, p, r);
    const e = toward(c, n, r);
    d += ` L ${s.x} ${s.y} Q ${c.x} ${c.y} ${e.x} ${e.y}`;
  }
  const last = points.at(-1);
  return `${d} L ${last.x} ${last.y}`;
}

// El punto medio de una línea quebrada: ahí va la etiqueta.
function midOf(points) {
  const lens = points.slice(1).map((p, i) => Math.hypot(p.x - points[i].x, p.y - points[i].y));
  let left = lens.reduce((a, b) => a + b, 0) / 2;
  for (let i = 0; i < lens.length; i++) {
    if (left <= lens[i] && lens[i] > 0) {
      const t = left / lens[i];
      return { x: points[i].x + (points[i + 1].x - points[i].x) * t, y: points[i].y + (points[i + 1].y - points[i].y) * t };
    }
    left -= lens[i];
  }
  return { ...points[0] };
}

// Un extremo dibujado en `p`, con `dir` la dirección en que llega la línea hasta él. Da trazos:
// `{ d, fill }` (fill: relleno; si no, solo línea).
export function headShapes(kind, p, dir) {
  const len = Math.hypot(dir.x, dir.y) || 1;
  const u = { x: dir.x / len, y: dir.y / len };
  const n = { x: -u.y, y: u.x };
  const at = (back, side) => `${p.x - u.x * back + n.x * side} ${p.y - u.y * back + n.y * side}`;
  switch (kind) {
    case 'flecha':
      return [{ d: `M ${at(10, 5.5)} L ${p.x} ${p.y} L ${at(10, -5.5)}`, fill: false }];
    case 'triangulo':
      return [{ d: `M ${at(12, 6.5)} L ${p.x} ${p.y} L ${at(12, -6.5)} Z`, fill: true }];
    case 'rombo':
      return [{ d: `M ${p.x} ${p.y} L ${at(8, 5)} L ${at(16, 0)} L ${at(8, -5)} Z`, fill: true }];
    case 'uno':
      return [{ d: `M ${at(9, 6)} L ${at(9, -6)}`, fill: false }];
    case 'muchos':
      return [{ d: `M ${at(12, 0)} L ${at(0, 6.5)} M ${at(12, 0)} L ${at(0, -6.5)} M ${at(12, 6.5)} L ${at(12, -6.5)}`, fill: false }];
    case 'cero-muchos':
      return [{ d: `M ${at(11, 0)} L ${at(0, 6.5)} M ${at(11, 0)} L ${at(0, -6.5)} ${circleAt(p, u, 16, 5)}`, fill: false }];
    default:
      return [];
  }
}

function circleAt(p, u, back, r) {
  const c = { x: p.x - u.x * back, y: p.y - u.y * back };
  return `M ${c.x - r} ${c.y} a ${r} ${r} 0 1 0 ${2 * r} 0 a ${r} ${r} 0 1 0 ${-2 * r} 0`;
}

// Todo lo que hace falta para pintar una flecha, ya en coordenadas del lienzo.
export function arrowGeometry(datos, get) {
  const ends = arrowEnds(datos, get);
  if (!ends) return null;
  const { from, to } = ends;
  let d;
  let points;
  let dirStart;
  let dirEnd;
  if (datos.ruta === 'recta') {
    points = [from, to];
    d = `M ${from.x} ${from.y} L ${to.x} ${to.y}`;
    dirStart = { x: from.x - to.x, y: from.y - to.y };
    dirEnd = { x: to.x - from.x, y: to.y - from.y };
  } else if (datos.ruta === 'curva') {
    const span = Math.max(44, Math.hypot(to.x - from.x, to.y - from.y) / 2.4);
    const side = (end, other) => end.side ?? (Math.abs(other.x - end.x) >= Math.abs(other.y - end.y) ? (other.x >= end.x ? 'e' : 'o') : other.y >= end.y ? 's' : 'n');
    const sa = side(from, to);
    const sb = side(to, from);
    const c1 = { x: from.x + OUT[sa][0] * span, y: from.y + OUT[sa][1] * span };
    const c2 = { x: to.x + OUT[sb][0] * span, y: to.y + OUT[sb][1] * span };
    d = `M ${from.x} ${from.y} C ${c1.x} ${c1.y} ${c2.x} ${c2.y} ${to.x} ${to.y}`;
    points = [from, c1, c2, to];
    dirStart = { x: from.x - c1.x, y: from.y - c1.y };
    dirEnd = { x: to.x - c2.x, y: to.y - c2.y };
  } else {
    points = elbowPoints(from, to);
    d = roundedPath(points);
    const [a, b] = [points[0], points[1]];
    const [y, z] = [points.at(-2), points.at(-1)];
    dirStart = { x: a.x - b.x, y: a.y - b.y };
    dirEnd = { x: z.x - y.x, y: z.y - y.y };
  }
  const heads = [...headShapes(datos.inicio, from, dirStart), ...headShapes(datos.fin, to, dirEnd)];
  const box = boundsOfRects(points.map((p) => ({ x: p.x, y: p.y, w: 0, h: 0 })));
  const mid = datos.ruta === 'curva' ? bezierMid(points) : midOf(points);
  return { d, heads, from, to, mid, box };
}

const bezierMid = ([a, b, c, e]) => ({
  x: 0.125 * a.x + 0.375 * b.x + 0.375 * c.x + 0.125 * e.x,
  y: 0.125 * a.y + 0.375 * b.y + 0.375 * c.y + 0.125 * e.y,
});

// Lo que ocupa cualquier elemento: una flecha no guarda su rectángulo, sale de sus puntas.
export function rectOf(el, get) {
  if (el.tipo !== 'flecha') return { x: el.x, y: el.y, w: el.w, h: el.h };
  const g = arrowGeometry(el.datos, get);
  return g ? g.box : { x: 0, y: 0, w: 0, h: 0 };
}

// Los elementos que toca un área al seleccionar arrastrando.
export function inArea(els, area, get) {
  return els.filter((el) => intersects(padded(rectOf(el, get)), area)).map((el) => el.id);
}
const padded = (r) => ({ x: r.x - 1, y: r.y - 1, w: r.w + 2, h: r.h + 2 });

// Lo que un marco lleva dentro: lo que cabe entero en él, y nunca él mismo.
export function insideFrame(frame, items) {
  return items.filter((it) => it.id !== frame.id && contains(frame, it)).map((it) => it.id);
}

// Alinear: da la x o la y nuevas de cada uno. `modo`: izquierda, centro, derecha, arriba, medio, abajo.
export function aligned(rects, modo) {
  const b = boundsOfRects(rects);
  if (!b) return [];
  return rects.map((r) => {
    switch (modo) {
      case 'izquierda':
        return { id: r.id, x: b.x };
      case 'centro':
        return { id: r.id, x: b.x + (b.w - r.w) / 2 };
      case 'derecha':
        return { id: r.id, x: b.x + b.w - r.w };
      case 'arriba':
        return { id: r.id, y: b.y };
      case 'medio':
        return { id: r.id, y: b.y + (b.h - r.h) / 2 };
      default:
        return { id: r.id, y: b.y + b.h - r.h };
    }
  });
}

// Repartir: mismos huecos entre uno y otro, con los dos de los extremos quietos.
export function distributed(rects, eje) {
  if (rects.length < 3) return [];
  const [pos, size] = eje === 'x' ? ['x', 'w'] : ['y', 'h'];
  const orden = [...rects].sort((a, b) => a[pos] - b[pos]);
  const total = orden.reduce((n, r) => n + r[size], 0);
  const first = orden[0];
  const last = orden.at(-1);
  const hueco = (last[pos] + last[size] - first[pos] - total) / (orden.length - 1);
  let at = first[pos];
  return orden.map((r) => {
    const out = { id: r.id, [pos]: at };
    at += r[size] + hueco;
    return out;
  });
}

// El orden de lectura: de arriba abajo por filas y, dentro de una fila, de izquierda a derecha.
export function readingOrder(items, band = 44) {
  return [...items].sort((a, b) => {
    const dy = a.y - b.y;
    return Math.abs(dy) > band ? dy : a.x - b.x || dy;
  });
}

// Lo que ocupa una flecha a grandes rasgos, sin calcular su ruta: para decidir si está cerca de la
// vista cuando se recorren miles de elementos en cada fotograma.
export function arrowRoughBox(datos, get, pad = 66) {
  const a = endCenter(datos?.desde, get);
  const b = endCenter(datos?.hasta, get);
  if (!a || !b) return null;
  return { x: Math.min(a.x, b.x) - pad, y: Math.min(a.y, b.y) - pad, w: Math.abs(a.x - b.x) + pad * 2, h: Math.abs(a.y - b.y) + pad * 2 };
}
