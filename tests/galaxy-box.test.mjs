// Pruebas de la caja que ocupa el lienzo de la galaxia (funciones puras, sin WebGL).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { LOOK, MAX_SCALE, boxAround, galaxyExtent, makePoints, planBox } from '../src/galaxy.js';

const extent = galaxyExtent(makePoints(LOOK), LOOK);
const parent = { left: 0, top: 0, width: 1832, height: 1042 };
const plan = (over) => planBox({ spread: false, moving: false, current: null, cx: 900, cy: 520, scale: 320, extent, parent, ...over });
const center = (b) => [b.x + b.w / 2, b.y + b.h / 2];

test('boxAround centra la caja y su tamaño solo depende de la escala', () => {
  const a = boxAround(900, 520, 320, extent);
  const b = boxAround(431.4, 207.6, 320, extent);
  assert.deepEqual(center(a), [900, 520]);
  assert.deepEqual([a.w, a.h], [b.w, b.h]);
  assert.deepEqual(center(b), [431, 208]);
  const bigger = boxAround(900, 520, 400, extent);
  assert.ok(bigger.w > a.w && bigger.h > a.h);
});

test('boxAround cubre la galaxia entera a esa escala', () => {
  const b = boxAround(900, 520, 320, extent);
  assert.ok(b.w / 2 >= extent.x * 320, 'se queda corta de ancho');
  assert.ok(b.h / 2 >= extent.y * 320, 'se queda corta de alto');
});

test('dispersa, el lienzo cubre todo el contenedor', () => {
  assert.deepEqual(plan({ spread: true, moving: true }), { x: 0, y: 0, w: 1832, h: 1042, full: true });
});

test('en reposo, el lienzo es lo justo para la galaxia', () => {
  assert.deepEqual(plan(), boxAround(900, 520, 320, extent));
});

test('mientras el hueco se mueve, la caja conserva su tamaño si la galaxia cabe', () => {
  // La terminal se vuelve a abrir: la galaxia encoge (320 → 280) y se desplaza.
  const current = boxAround(900, 520, 320, extent);
  const next = plan({ moving: true, current, cx: 760, cy: 520, scale: 280 });
  assert.deepEqual([next.w, next.h], [current.w, current.h]);
  assert.deepEqual(center(next), [760, 520]);
});

test('mientras el hueco se mueve y la galaxia crece, pasa a la caja de la escala máxima', () => {
  // La terminal se minimiza: la galaxia crece (320 → 400) y ya no cabe en su caja.
  const current = boxAround(900, 520, 320, extent);
  const first = plan({ moving: true, current, cx: 1000, cy: 520, scale: 330 });
  assert.deepEqual([first.w, first.h], [boxAround(0, 0, MAX_SCALE, extent).w, boxAround(0, 0, MAX_SCALE, extent).h]);
  // …y ya no cambia de tamaño en lo que queda de movimiento.
  const later = plan({ moving: true, current: first, cx: 1100, cy: 520, scale: 400 });
  assert.deepEqual([later.w, later.h], [first.w, first.h]);
  assert.deepEqual(center(later), [1100, 520]);
});

test('mientras el hueco se mueve no se reutiliza el lienzo de pantalla completa', () => {
  const current = { x: 0, y: 0, w: 1832, h: 1042, full: true };
  const next = plan({ moving: true, current });
  assert.deepEqual(next, boxAround(900, 520, MAX_SCALE, extent));
});

test('al quedarse quieto vuelve a lo justo', () => {
  const during = plan({ moving: true, current: boxAround(900, 520, 320, extent), scale: 400 });
  const after = plan({ moving: false, current: during, scale: 400 });
  assert.deepEqual(after, boxAround(900, 520, 400, extent));
  assert.ok(after.w < during.w);
});
