import { test } from 'node:test';
import assert from 'node:assert/strict';
import { categoryLabel, contextSegments, formatShare, formatTokens, totalTokens, untilLabel } from '../src/registro.js';

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

test('contextSegments pone cada categoría a escala de la ventana', () => {
  const segments = contextSegments(
    [
      { name: 'System prompt', tokens: 5_143, kind: 'used' },
      { name: 'System tools (deferred)', tokens: 21_767, kind: 'deferred' },
      { name: 'Skills', tokens: 7_272, kind: 'used' },
      { name: 'Messages', tokens: 106_424, kind: 'used' },
      { name: 'Autocompact buffer', tokens: 33_000, kind: 'buffer' },
      { name: 'Free space', tokens: 828_141, kind: 'free' },
    ],
    1_000_000,
  );
  // Lo diferido y lo libre no tienen tramo; la reserva va al final.
  assert.deepEqual(
    segments.map((s) => s.name),
    ['System prompt', 'Skills', 'Messages', 'Autocompact buffer'],
  );
  // Lo diminuto se ve igual (mínimo 1,2 %); lo demás va a escala.
  assert.equal(segments[0].pct, 1.2);
  assert.equal(segments[1].pct, 1.2);
  assert.ok(Math.abs(segments[2].pct - 10.6424) < 1e-9);
  assert.ok(Math.abs(segments[3].pct - 3.3) < 1e-9);
});

test('contextSegments no se pasa de 100 aunque el contexto esté lleno', () => {
  const full = contextSegments(
    [
      { name: 'System prompt', tokens: 100, kind: 'used' },
      { name: 'Messages', tokens: 250_000, kind: 'used' },
      { name: 'Autocompact buffer', tokens: 33_000, kind: 'buffer' },
    ],
    200_000,
  );
  // Sin sitio libre no queda reserva, y los tramos se encogen para caber.
  assert.deepEqual(full.map((s) => s.kind), ['used', 'used']);
  assert.ok(Math.abs(full.reduce((sum, s) => sum + s.pct, 0) - 100) < 1e-9);
  // La reserva solo ocupa lo que queda libre.
  const tight = contextSegments(
    [
      { name: 'Messages', tokens: 180_000, kind: 'used' },
      { name: 'Autocompact buffer', tokens: 33_000, kind: 'buffer' },
    ],
    200_000,
  );
  assert.ok(Math.abs(tight[1].pct - 10) < 1e-9);
  assert.deepEqual(contextSegments(null, 1000), []);
});

test('percent da la parte de la ventana sin redondear a cero lo pequeño', async () => {
  const { percent } = await import('../src/registro.js');
  assert.equal(percent(0), '0 %');
  assert.equal(percent(0.0183), '< 0,1 %');
  assert.equal(percent(0.51), '0,5 %');
  assert.equal(percent(10.64), '11 %');
});
