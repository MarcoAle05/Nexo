import { test } from 'node:test';
import assert from 'node:assert/strict';
import { categoryLabel, contextCells, formatShare, formatTokens, totalTokens, untilLabel } from '../src/registro.js';

test('formatTokens abrevia con k y M a la española', () => {
  assert.equal(formatTokens(0), '0');
  assert.equal(formatTokens(812), '812');
  assert.equal(formatTokens(48_120), '48,1 k');
  assert.equal(formatTokens(122_859), '123 k');
  assert.equal(formatTokens(2_533_170), '2,53 M');
  assert.equal(formatTokens(12_400_000), '12,4 M');
  assert.equal(formatTokens(undefined), '0');
});

test('formatShare marca la parte de la sesión como aproximada', () => {
  assert.equal(formatShare(null), '—');
  assert.equal(formatShare(0), '0 %');
  assert.equal(formatShare(0.04), '< 0,1 %');
  assert.equal(formatShare(6.14), '≈ 6,1 %');
  assert.equal(formatShare(23.6), '≈ 24 %');
});

test('totalTokens suma todo lo que leyó y escribió el modelo', () => {
  assert.equal(totalTokens({ input: 1, output: 2, cache_read: 3, cache_write: 4 }), 10);
  assert.equal(totalTokens(null), 0);
});

test('untilLabel cuenta lo que falta para renovarse', () => {
  const now = Date.UTC(2026, 9, 8, 20, 0);
  assert.equal(untilLabel(now + 125 * 60000, now), 'en 2 h 05 min');
  assert.equal(untilLabel(now + 12 * 60000, now), 'en 12 min');
  assert.equal(untilLabel(now - 60000, now), 'en 0 min');
});

test('categoryLabel traduce las categorías de /context', () => {
  assert.equal(categoryLabel('System prompt'), 'Instrucciones del sistema');
  assert.equal(categoryLabel('Messages'), 'Conversación');
  assert.equal(categoryLabel('Messages', 'sub'), 'Encargo y trabajo');
  assert.equal(categoryLabel('System tools (deferred)'), 'Herramientas que se cargan al usarlas');
  assert.equal(categoryLabel('Algo nuevo'), 'Algo nuevo');
});

test('contextCells reparte la ventana en 100 puntos como /context', () => {
  const max = 1_000_000;
  const cells = contextCells(
    [
      { name: 'System prompt', tokens: 5_143, kind: 'used' },
      { name: 'System tools (deferred)', tokens: 21_767, kind: 'deferred' },
      { name: 'Skills', tokens: 7_272, kind: 'used' },
      { name: 'Messages', tokens: 106_424, kind: 'used' },
      { name: 'Autocompact buffer', tokens: 33_000, kind: 'buffer' },
      { name: 'Free space', tokens: 844_141, kind: 'free' },
    ],
    max,
  );
  assert.equal(cells.length, 100);
  const count = (name) => cells.filter((c) => c.name === name).length;
  // Las categorías pequeñas tienen al menos un punto, a medias.
  assert.equal(count('System prompt'), 1);
  assert.ok(cells[0].fill > 0.5 && cells[0].fill < 0.52);
  assert.equal(count('Skills'), 1);
  // Lo diferido no ocupa contexto.
  assert.equal(count('System tools (deferred)'), 0);
  assert.equal(count('Messages'), 11);
  assert.equal(count('Autocompact buffer'), 3);
  assert.deepEqual(cells.at(-1), { name: 'Autocompact buffer', kind: 'buffer', fill: 1 });
  assert.equal(count('Free space'), 100 - 1 - 1 - 11 - 3);
});

test('contextCells no se pasa de 100 aunque el contexto esté lleno', () => {
  const cells = contextCells(
    [
      { name: 'Messages', tokens: 250_000, kind: 'used' },
      { name: 'Autocompact buffer', tokens: 33_000, kind: 'buffer' },
    ],
    200_000,
  );
  assert.equal(cells.length, 100);
  assert.ok(cells.every((c) => c.name === 'Messages'));
});

test('percent da la parte de la ventana sin redondear a cero lo pequeño', async () => {
  const { percent } = await import('../src/registro.js');
  assert.equal(percent(0), '0 %');
  assert.equal(percent(0.0183), '< 0,1 %');
  assert.equal(percent(0.51), '0,5 %');
  assert.equal(percent(10.64), '11 %');
});
