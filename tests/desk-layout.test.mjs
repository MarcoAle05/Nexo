import test from 'node:test';
import assert from 'node:assert/strict';
import { nextSpot, boundsOf, revealShift, mapFrame, toMap, fromMap } from '../src/desk-layout.js';

const home = { x: 300, y: 120 };
const size = { w: 380, h: 440 };
const card = (path, x, y, w = 380, h = 440) => ({ path, x, y, w, h });

test('la primera carta se abre en el sitio de partida, con el tamaño por defecto', () => {
  assert.deepEqual(nextSpot([], { home, size }), { x: 300, y: 120, w: 380, h: 440 });
});

test('la siguiente se abre a la derecha de la última, a su altura y con su tamaño', () => {
  const cards = [card('a', 300, 120, 420, 500)];
  assert.deepEqual(nextSpot(cards, { anchor: 'a', home, size }), { x: 300 + 420 + 16, y: 120, w: 420, h: 500 });
});

test('abriendo varias seguidas quedan en fila y en orden, sin pisarse', () => {
  const cards = [];
  for (const path of ['a', 'b', 'c', 'd']) cards.push({ path, ...nextSpot(cards, { anchor: cards.at(-1)?.path, home, size }) });
  assert.deepEqual(cards.map((c) => c.x), [300, 696, 1092, 1488]);
  assert.ok(cards.every((c) => c.y === 120 && c.w === 380 && c.h === 440));
});

test('si el sitio de la derecha está ocupado, salta al primer hueco', () => {
  // «b» se movió justo donde iría la nueva y «c» está pegada a ella.
  const cards = [card('a', 0, 0), card('b', 400, 40), card('c', 800, -20)];
  const spot = nextSpot(cards, { anchor: 'a', home, size });
  assert.deepEqual(spot, { x: 800 + 380 + 16, y: 0, w: 380, h: 440 });
});

test('una carta en otra fila no estorba', () => {
  const cards = [card('a', 0, 0), card('abajo', 396, 600)];
  assert.deepEqual(nextSpot(cards, { anchor: 'a', home, size }), { x: 396, y: 0, w: 380, h: 440 });
});

test('sin saber cuál fue la última, la referencia es la que más a la derecha llega', () => {
  const cards = [card('a', -500, 30), card('b', 900, 200, 300, 260), card('c', 100, 700)];
  assert.deepEqual(nextSpot(cards, { home, size }), { x: 900 + 300 + 16, y: 200, w: 300, h: 260 });
  assert.deepEqual(nextSpot(cards, { anchor: 'cerrada', home, size }), { x: 1216, y: 200, w: 300, h: 260 });
});

test('el lienzo admite coordenadas negativas', () => {
  const cards = [card('a', -2000, -900)];
  assert.deepEqual(nextSpot(cards, { anchor: 'a', home, size }), { x: -1604, y: -900, w: 380, h: 440 });
});

test('boundsOf abarca todos los rectángulos', () => {
  assert.equal(boundsOf([]), null);
  assert.deepEqual(boundsOf([{ x: -10, y: 5, w: 20, h: 20 }, { x: 100, y: -40, w: 50, h: 10 }]), { x: -10, y: -40, w: 160, h: 65 });
});

test('revealShift solo mueve la vista lo justo para que la carta se vea en el hueco libre', () => {
  const area = { x: 370, y: 90, w: 780, h: 660 };
  assert.deepEqual(revealShift({ x: 400, y: 120, w: 380, h: 440 }, area), { dx: 0, dy: 0 }, 'ya se ve');
  assert.deepEqual(revealShift({ x: 1000, y: 120, w: 380, h: 440 }, area), { dx: 1380 - (370 + 780 - 16), dy: 0 }, 'se sale por la derecha');
  assert.deepEqual(revealShift({ x: 100, y: 20, w: 380, h: 440 }, area), { dx: 100 - 386, dy: 20 - 106 }, 'queda bajo la carta izquierda y la barra');
  // Más grande que el hueco: manda su esquina superior izquierda.
  assert.deepEqual(revealShift({ x: 500, y: 300, w: 900, h: 900 }, area), { dx: 500 - 386, dy: 300 - 106 });
});

test('mapFrame enseña todas las cartas y lo que se ve, a escala y centrado', () => {
  const view = { x: 0, y: 0, w: 1600, h: 800 };
  const map = { w: 120, h: 54 };
  const cards = [card('a', 300, 120), card('lejos', 6000, 2600)];
  const frame = mapFrame(cards, view, map);
  for (const rect of [...cards, view]) {
    const box = toMap(frame, rect);
    assert.ok(box.left >= -1e-9 && box.top >= -1e-9 && box.left + box.width <= map.w + 1e-9 && box.top + box.height <= map.h + 1e-9, `fuera del mapa: ${JSON.stringify(box)}`);
  }
  // La escala es la misma en los dos ejes.
  const box = toMap(frame, cards[0]);
  assert.ok(Math.abs(box.width / 380 - box.height / 440) < 1e-9);
});

test('con pocas cartas el indicador no llena el mapa', () => {
  const view = { x: 0, y: 0, w: 1600, h: 800 };
  const frame = mapFrame([card('a', 300, 120)], view, { w: 120, h: 54 });
  const box = toMap(frame, view);
  assert.ok(box.width < 120 * 0.6 && box.height < 54 * 0.6, JSON.stringify(box));
  // Y queda centrado.
  assert.ok(Math.abs(box.left + box.width / 2 - 60) < 1e-6 && Math.abs(box.top + box.height / 2 - 27) < 1e-6);
});

test('fromMap deshace toMap: un clic en el mapa da el punto del lienzo', () => {
  const frame = mapFrame([card('a', -800, 300)], { x: 200, y: 100, w: 1280, h: 720 }, { w: 120, h: 54 });
  const box = toMap(frame, { x: 512, y: -64, w: 0, h: 0 });
  const point = fromMap(frame, box.left, box.top);
  assert.ok(Math.abs(point.x - 512) < 1e-6 && Math.abs(point.y + 64) < 1e-6);
});

test('el zoom va de 0 % a −200 %: a −100 % se ve el doble y a −200 %, el triple', async () => {
  const { ZOOM, clampZoom, scaleOf, zoomLabel } = await import('../src/desk-layout.js');
  assert.deepEqual(ZOOM, { min: -200, max: 0, step: 25 });
  assert.equal(scaleOf(0), 1);
  assert.equal(scaleOf(-100), 0.5);
  assert.ok(Math.abs(scaleOf(-200) - 1 / 3) < 1e-12);
  assert.equal(scaleOf(-50), 1 / 1.5);
  assert.equal(clampZoom(40), 0, 'no se acerca más allá del tamaño real');
  assert.equal(clampZoom(-350), -200);
  assert.equal(clampZoom('mal'), 0);
  assert.equal(clampZoom(-37.4), -37);
  assert.equal(zoomLabel(0), '0 %');
  assert.equal(zoomLabel(-125), '−125 %');
});

test('zoomAround deja quieto el punto que se está mirando', async () => {
  const { zoomAround } = await import('../src/desk-layout.js');
  const cam = { x: -400, y: -120 };
  const point = { x: 900, y: 420 };
  const before = { x: cam.x + point.x / 1, y: cam.y + point.y / 1 };
  const next = zoomAround(cam, point, 1, 0.5);
  assert.deepEqual({ x: next.x + point.x / 0.5, y: next.y + point.y / 0.5 }, before);
  // Ida y vuelta: la cámara vuelve a donde estaba.
  const back = zoomAround(next, point, 0.5, 1);
  assert.ok(Math.abs(back.x - cam.x) < 1e-9 && Math.abs(back.y - cam.y) < 1e-9);
});
