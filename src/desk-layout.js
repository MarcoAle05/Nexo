// Geometría del escritorio de notas que no necesita el DOM (se prueba con node).

const overlaps = (a, b, gap) => a.x < b.x + b.w + gap && a.x + a.w + gap > b.x && a.y < b.y + b.h + gap && a.y + a.h + gap > b.y;

// Cartas que ya no caben a lo ancho del escritorio (se estrecha al volver a abrir la
// terminal): cada una va al hueco libre más cercano a donde estaba, sin tapar a ninguna otra,
// y si es más ancha que el escritorio se estrecha lo justo. Las que siguen cabiendo no se
// tocan: no es «Ordenar cartas». Devuelve solo las que cambian: ruta → { x, y, w }.
export function fitCards(cards, width, { gap = 14, minW = 220, step = 12 } = {}) {
  const moved = new Map();
  // Con el escritorio plegado (una carta lateral expandida) no hay dónde acomodar nada.
  if (!(width >= minW + 2 * gap)) return moved;
  const out = cards.filter((c) => c.x + c.w > width).sort((a, b) => a.y - b.y || a.x - b.x);
  const placed = cards.filter((c) => !out.includes(c)).map((c) => ({ ...c }));
  for (const card of out) {
    const w = Math.min(card.w, width - 2 * gap);
    const maxX = width - gap - w;
    const want = { x: Math.max(gap, Math.min(card.x, maxX)), y: Math.max(gap, card.y) };
    // Por debajo de todas las cartas siempre hay sitio: el escritorio se desplaza hacia abajo.
    const bottom = Math.max(want.y, ...placed.map((c) => c.y + c.h + gap));
    const xs = new Set([want.x, maxX]);
    for (let x = gap; x <= maxX; x += step) xs.add(x);
    const ys = new Set([want.y, bottom]);
    for (let y = gap; y <= bottom; y += step) ys.add(y);
    let best = null;
    for (const y of ys) {
      for (const x of xs) {
        // Moverse de lado cuesta menos que cambiar de fila: la carta se queda a su altura si puede.
        const cost = (x - want.x) ** 2 + (2 * (y - want.y)) ** 2;
        if (best && cost >= best.cost) continue;
        if (placed.some((c) => overlaps({ x, y, w, h: card.h }, c, gap))) continue;
        best = { x, y, cost };
      }
    }
    const spot = best ?? { x: want.x, y: bottom };
    moved.set(card.path, { x: spot.x, y: spot.y, w });
    placed.push({ ...card, x: spot.x, y: spot.y, w });
  }
  return moved;
}
