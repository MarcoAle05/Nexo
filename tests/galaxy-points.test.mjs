// Pruebas de los puntos de la galaxia y del marco que ocupa (funciones puras, sin WebGL).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { LOOK, makePoints, frameFor, galaxyExtent } from '../src/galaxy.js';

const TOTAL = LOOK.core.count + LOOK.disk.count + LOOK.ring.count + LOOK.halo.count;

test('makePoints genera la suma de los puntos de cada grupo', () => {
  const p = makePoints(LOOK);
  assert.equal(p.count, TOTAL);
  assert.equal(p.orbit.length, TOTAL * 4);
  assert.equal(p.style.length, TOTAL * 4);
});

test('makePoints da solo números finitos', () => {
  const p = makePoints(LOOK);
  assert.ok(p.orbit.every(Number.isFinite), 'órbitas no finitas');
  assert.ok(p.style.every(Number.isFinite), 'estilos no finitos');
});

test('makePoints da tamaños positivos, brillo en (0, 1] y tipos válidos', () => {
  const p = makePoints(LOOK);
  for (let i = 0; i < p.count; i++) {
    const o = i * 4;
    assert.ok(p.style[o] > 0, `tamaño no positivo en el punto ${i}`);
    assert.ok(p.style[o + 1] > 0 && p.style[o + 1] <= 1, `brillo fuera de (0, 1] en el punto ${i}`);
    assert.ok([0, 1, 2].includes(p.style[o + 3]), `tipo desconocido en el punto ${i}`);
  }
});

test('makePoints es determinista con la misma semilla y cambia con otra', () => {
  const a = makePoints({ ...LOOK, seed: 11 });
  const b = makePoints({ ...LOOK, seed: 11 });
  assert.deepEqual(a.orbit, b.orbit);
  assert.deepEqual(a.style, b.style);

  const c = makePoints({ ...LOOK, seed: 12 });
  assert.notDeepEqual(a.orbit, c.orbit);
});

// Escenarios de tamaño muy distinto: del diminuto al de pantalla grande.
const RECTS = [
  { left: 0, top: 0, width: 10, height: 10 },
  { left: 40, top: 30, width: 100, height: 60 },
  { left: 40, top: 30, width: 400, height: 300 },
  { left: 40, top: 30, width: 1280, height: 720 },
  { left: 0, top: 0, width: 1920, height: 1080 },
  { left: 0, top: 0, width: 4000, height: 3000 },
  { left: 0, top: 0, width: 700, height: 900 },
];

test('frameFor acota la escala a [80, 420]', () => {
  for (const rect of RECTS) {
    const { scale } = frameFor(rect);
    assert.ok(scale >= 80 && scale <= 420, `escala ${scale} fuera de [80, 420] para ${rect.width}×${rect.height}`);
  }
  assert.equal(frameFor(RECTS[0]).scale, 80, 'el mínimo debe ser 80');
  assert.equal(frameFor(RECTS[5]).scale, 420, 'el máximo debe ser 420');
});

test('frameFor centra la galaxia en el rectángulo (±10 px)', () => {
  for (const rect of RECTS) {
    const { cx, cy } = frameFor(rect);
    assert.ok(Math.abs(cx - (rect.left + rect.width / 2)) <= 10, `cx desalineado para ${rect.width}×${rect.height}`);
    assert.ok(Math.abs(cy - (rect.top + rect.height / 2)) <= 10, `cy desalineado para ${rect.width}×${rect.height}`);
  }
});

test('galaxyExtent contiene cada punto en cualquier momento de su giro', () => {
  const p = makePoints(LOOK);
  const { x, y } = galaxyExtent(p, LOOK);
  const ci = LOOK.tilt;
  const si = Math.sqrt(1 - ci * ci);
  const turn = (LOOK.angle * Math.PI) / 180;
  let maxU = 0;
  let maxV = 0;
  for (let i = 0; i < p.count; i++) {
    const r = p.orbit[i * 4];
    const z = p.orbit[i * 4 + 2];
    for (let k = 0; k < 90; k++) {
      const a = (k / 90) * Math.PI * 2;
      const px = Math.cos(a) * r;
      const sy = Math.sin(a) * r * ci - z * si;
      const u = Math.abs(px * Math.cos(turn) - sy * Math.sin(turn));
      const v = Math.abs(px * Math.sin(turn) + sy * Math.cos(turn));
      assert.ok(u <= x + 1e-6 && v <= y + 1e-6, `punto ${i} fuera de la caja`);
      maxU = Math.max(maxU, u);
      maxV = Math.max(maxV, v);
    }
  }
  // La caja es ajustada: no sobra más de un 2 %.
  assert.ok(maxU > x * 0.98 && maxV > y * 0.98);
});
