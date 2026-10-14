// Geometría del lienzo de notas que no necesita el DOM (se prueba con node). El lienzo es
// tan grande que hace de infinito: las cartas viven en coordenadas del lienzo (pueden ser
// negativas) y la ventana enseña un trozo.

const overlaps = (a, b, gap) => a.x < b.x + b.w + gap && a.x + a.w + gap > b.x && a.y < b.y + b.h + gap && a.y + a.h + gap > b.y;

// Dónde se abre una carta nueva: a la derecha de la última que se abrió (`anchor`, su ruta),
// a su altura y con su tamaño, para que queden en fila y en orden. Si ese sitio lo ocupa
// otra carta, sigue hacia la derecha hasta el primer hueco. Sin `anchor` (o si ya no está),
// la referencia es la carta que más a la derecha llega. Sin cartas, en `home`.
export function nextSpot(cards, { anchor = null, home, size, gap = 16 } = {}) {
  if (!cards.length) return { x: home.x, y: home.y, w: size.w, h: size.h };
  const base = cards.find((c) => c.path === anchor) ?? cards.reduce((best, c) => (c.x + c.w > best.x + best.w ? c : best));
  const spot = { x: base.x + base.w + gap, y: base.y, w: base.w, h: base.h };
  // Cada vuelta salta por encima de todo lo que pisa: como mucho, una por carta.
  for (let guard = 0; guard <= cards.length; guard++) {
    const hits = cards.filter((c) => overlaps(spot, c, gap));
    if (!hits.length) break;
    spot.x = Math.max(...hits.map((c) => c.x + c.w)) + gap;
  }
  return spot;
}

// Rectángulo que abarca todos los dados (null si no hay ninguno).
export function boundsOf(rects) {
  if (!rects.length) return null;
  const x = Math.min(...rects.map((r) => r.x));
  const y = Math.min(...rects.map((r) => r.y));
  return { x, y, w: Math.max(...rects.map((r) => r.x + r.w)) - x, h: Math.max(...rects.map((r) => r.y + r.h)) - y };
}

// Cuánto hay que desplazar la vista para que `rect` quede dentro de `area` (el hueco libre
// entre las cartas laterales, en las mismas coordenadas) con un margen. Si no cabe, manda
// su esquina superior izquierda. { dx, dy } en píxeles; 0 si ya se ve.
export function revealShift(rect, area, margin = 16) {
  const axis = (start, size, from, span) => {
    const low = from + margin;
    const high = from + span - margin;
    if (start < low || size > high - low) return start - low;
    if (start + size > high) return start + size - high;
    return 0;
  };
  return { dx: axis(rect.x, rect.w, area.x, area.w), dy: axis(rect.y, rect.h, area.y, area.h) };
}

// Minimapa: el trozo del lienzo que enseña (todas las cartas y lo que se ve ahora, con aire
// alrededor y nunca menos que `minSpan` veces la ventana, para que el indicador no lo llene
// todo) y la escala con la que cabe, centrado, en un recuadro de `map.w` × `map.h`.
export function mapFrame(cards, view, map, { pad = 0.1, minSpan = 2.2 } = {}) {
  const all = boundsOf([...cards, view]);
  const w = Math.max(all.w, view.w * minSpan) * (1 + 2 * pad);
  const h = Math.max(all.h, view.h * minSpan) * (1 + 2 * pad);
  const scale = Math.min(map.w / w, map.h / h);
  // El recuadro puede ser más ancho o más alto que el trozo: lo que sobra se reparte.
  const spanW = map.w / scale;
  const spanH = map.h / scale;
  return { x: all.x + all.w / 2 - spanW / 2, y: all.y + all.h / 2 - spanH / 2, w: spanW, h: spanH, scale };
}

// Del lienzo al minimapa (px dentro del recuadro) y al revés.
export const toMap = (frame, rect) => ({
  left: (rect.x - frame.x) * frame.scale,
  top: (rect.y - frame.y) * frame.scale,
  width: rect.w * frame.scale,
  height: rect.h * frame.scale,
});
export const fromMap = (frame, px, py) => ({ x: frame.x + px / frame.scale, y: frame.y + py / frame.scale });

// Zoom del lienzo: solo hacia fuera, de 0 % (tamaño real) a −200 %. El porcentaje dice cuánto
// más se ve: a −100 % entra el doble de ancho y a −200 %, el triple (las cartas, a un tercio).
export const ZOOM = { min: -200, max: 0, step: 25 };
export const clampZoom = (pct) => Math.min(ZOOM.max, Math.max(ZOOM.min, Math.round(Number(pct) || 0)));
export const scaleOf = (pct) => 1 / (1 - clampZoom(pct) / 100);
// «−50 %», «0 %»: con el signo menos de verdad y el espacio antes del %, como en el resto.
export const zoomLabel = (pct) => `${clampZoom(pct) < 0 ? '−' : ''}${Math.abs(clampZoom(pct))} %`;

// Cámara tras cambiar de escala sin que se mueva el punto del lienzo que hay bajo `point`
// (px de pantalla dentro de la ventana): lo que se mira se queda en su sitio.
export function zoomAround(cam, point, from, to) {
  const world = { x: cam.x + point.x / from, y: cam.y + point.y / from };
  return { x: world.x - point.x / to, y: world.y - point.y / to };
}
