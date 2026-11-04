// El tablero Scrum y las piezas que se colocan solas (secuencia y línea de tiempo), sin DOM: se
// prueban con node. La interfaz solo pinta lo que sale de aquí.

// ------------------------------------------------------------ tablero

// Tarjetas de cada columna, en su orden.
export const cardsIn = (datos, colId) => (datos?.tarjetas ?? []).filter((t) => t.col === colId);

// Cuántas tarjetas y puntos hay en una columna, y si se pasa de su límite.
export function columnStats(datos, colId) {
  const col = (datos?.columnas ?? []).find((c) => c.id === colId);
  const cards = cardsIn(datos, colId);
  const points = cards.reduce((n, t) => n + (Number(t.puntos) || 0), 0);
  const limit = Number(col?.limite) || 0;
  return { count: cards.length, points, limit, over: limit > 0 && cards.length > limit };
}

// A dónde va una tarjeta con Ctrl y una flecha. Da `{ col, pos }` o null si ya está en el borde.
export function moveWithKey(datos, cardId, key) {
  const card = (datos?.tarjetas ?? []).find((t) => t.id === cardId);
  if (!card) return null;
  const cols = datos.columnas ?? [];
  const at = cols.findIndex((c) => c.id === card.col);
  const here = cardsIn(datos, card.col);
  const pos = here.findIndex((t) => t.id === cardId);
  switch (key) {
    case 'ArrowLeft':
    case 'ArrowRight': {
      const next = cols[at + (key === 'ArrowLeft' ? -1 : 1)];
      if (!next) return null;
      return { col: next.id, pos: Math.min(pos, cardsIn(datos, next.id).length) };
    }
    case 'ArrowUp':
      return pos > 0 ? { col: card.col, pos: pos - 1 } : null;
    case 'ArrowDown':
      return pos < here.length - 1 ? { col: card.col, pos: pos + 1 } : null;
    default:
      return null;
  }
}

// El puesto en una columna al soltar a la altura `y`, dados los centros de sus tarjetas (sin la
// que se arrastra).
export const dropIndex = (centers, y) => {
  const i = centers.findIndex((c) => y < c);
  return i < 0 ? centers.length : i;
};

// ------------------------------------------------------------ secuencia

export const SEQUENCE = { head: 44, step: 44, top: 12, foot: 22, minCol: 132 };

// Dónde va cada participante y cada mensaje, para un ancho dado.
export function layoutSequence(datos, width) {
  const people = datos?.participantes ?? [];
  const messages = datos?.mensajes ?? [];
  const titleH = datos?.titulo ? 32 : 0;
  const col = Math.max(SEQUENCE.minCol, (Math.max(width, 1) - 22) / Math.max(people.length, 1));
  const xOf = new Map(people.map((p, i) => [p.id, 11 + col * (i + 0.5)]));
  const headTop = titleH + SEQUENCE.top;
  const lineTop = headTop + SEQUENCE.head;
  const rows = messages
    .map((m, i) => {
      const from = xOf.get(m.de);
      const to = xOf.get(m.a);
      if (from == null || to == null) return null;
      const y = lineTop + SEQUENCE.step * (i + 1) - 8;
      return { id: m.id, y, from, to, self: from === to, text: m.texto ?? '', tipo: m.tipo ?? 'sincrono' };
    })
    .filter(Boolean);
  const height = lineTop + SEQUENCE.step * (messages.length + 1) + SEQUENCE.foot;
  return {
    titleH,
    headTop,
    lineTop,
    colWidth: col,
    width: col * Math.max(people.length, 1) + 22,
    height,
    lineBottom: height - SEQUENCE.foot + 8,
    people: people.map((p) => ({ id: p.id, name: p.nombre ?? '', x: xOf.get(p.id) })),
    rows,
  };
}

// ------------------------------------------------------------ línea de tiempo

const DAY = 86400000;
const parseDay = (s) => {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s ?? '');
  return m ? Date.UTC(+m[1], +m[2] - 1, +m[3]) : null;
};
const isoDay = (t) => new Date(t).toISOString().slice(0, 10);

export const TIMELINE = { head: 44, row: 36, label: 176 };

// Las barras de las tareas y los hitos en un ancho dado. La escala va de la primera fecha a la
// última; una tarea sin fechas no tiene barra, pero sigue en su fila.
export function layoutTimeline(datos, width, today = null) {
  const tasks = datos?.tareas ?? [];
  const milestones = datos?.hitos ?? [];
  const titleH = datos?.titulo ? 32 : 0;
  const dates = [...tasks.flatMap((t) => [parseDay(t.inicio), parseDay(t.fin)]), ...milestones.map((h) => parseDay(h.fecha))].filter((d) => d != null);
  const first = dates.length ? Math.min(...dates) : null;
  const last = dates.length ? Math.max(...dates) + DAY : null;
  const left = TIMELINE.label;
  const span = Math.max(1, Math.max(width, left + 44) - left - 14);
  const xOf = (t) => (first == null ? left : left + ((t - first) / Math.max(last - first, DAY)) * span);
  const top = titleH + TIMELINE.head;
  const bars = tasks.map((t, i) => {
    const a = parseDay(t.inicio);
    const b = parseDay(t.fin) ?? a;
    const y = top + TIMELINE.row * i;
    if (a == null) return { id: t.id, title: t.titulo ?? '', y, x: null, w: 0, progress: 0, dep: t.dep ?? [] };
    const x = xOf(a);
    return { id: t.id, title: t.titulo ?? '', y, x, w: Math.max(6, xOf(Math.max(a, b) + DAY) - x), progress: Number(t.avance) || 0, dep: t.dep ?? [] };
  });
  const byId = new Map(bars.map((b) => [b.id, b]));
  const links = bars.flatMap((b) =>
    b.dep
      .map((d) => byId.get(d))
      .filter((d) => d && d.x != null && b.x != null)
      .map((d) => ({ from: { x: d.x + d.w, y: d.y + TIMELINE.row / 2 }, to: { x: b.x, y: b.y + TIMELINE.row / 2 } })),
  );
  const marks = milestones
    .map((h) => {
      const d = parseDay(h.fecha);
      return d == null ? null : { id: h.id, title: h.titulo ?? '', x: xOf(d), date: h.fecha };
    })
    .filter(Boolean);
  // Las marcas de la escala: en días redondos y las que quepan sin pisarse (unos 56 px cada una).
  const ticks = [];
  if (first != null) {
    const days = Math.max(1, Math.round((last - first) / DAY));
    const room = Math.max(2, Math.min(6, Math.floor(span / 56)));
    const step = [1, 2, 7, 14, 30, 60, 90, 180, 365, 730].find((s) => days / s <= room) ?? 730;
    for (let t = first; t < last; t += step * DAY) ticks.push({ x: xOf(t), date: isoDay(t) });
  }
  const now = parseDay(today);
  return {
    titleH,
    top,
    bars,
    links,
    marks,
    ticks,
    today: now != null && first != null && now >= first && now <= last ? xOf(now) : null,
    height: top + TIMELINE.row * Math.max(tasks.length, 1) + (milestones.length ? 8 : 0) + 8,
  };
}
