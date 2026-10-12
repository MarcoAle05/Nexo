// Pruebas de la recolocación de cartas del escritorio de notas (función pura, sin DOM).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fitCards } from '../src/desk-layout.js';

const GAP = 14;
const card = (path, x, y, w = 380, h = 440) => ({ path, x, y, w, h });
const overlap = (a, b) => a.x < b.x + b.w + GAP && a.x + a.w + GAP > b.x && a.y < b.y + b.h + GAP && a.y + a.h + GAP > b.y;
// Las cartas tal como quedan tras aplicar lo que devuelve fitCards.
const applied = (cards, moved) => cards.map((c) => ({ ...c, ...(moved.get(c.path) ?? {}) }));

test('las cartas que caben no se tocan', () => {
  const cards = [card('a.md', 14, 14), card('b.md', 408, 14)];
  assert.equal(fitCards(cards, 900).size, 0);
});

test('una carta que se sale se desliza hacia dentro a su misma altura', () => {
  // Con la terminal minimizada cabían tres en fila; al abrirla el escritorio mide 900.
  const cards = [card('a.md', 14, 14), card('c.md', 802, 500)];
  const moved = fitCards(cards, 900);
  assert.deepEqual([...moved.keys()], ['c.md']);
  assert.deepEqual(moved.get('c.md'), { x: 900 - GAP - 380, y: 500, w: 380 });
});

test('si a su altura taparía a otra, va al hueco libre más cercano sin encimarse', () => {
  const cards = [card('a.md', 14, 14), card('b.md', 408, 14), card('c.md', 802, 14)];
  const moved = fitCards(cards, 900);
  assert.deepEqual([...moved.keys()], ['c.md'], 'solo se mueve la que no cabía');
  const after = applied(cards, moved);
  const c = after.find((x) => x.path === 'c.md');
  assert.ok(c.x >= GAP && c.x + c.w <= 900 - GAP, 'queda dentro del ancho');
  for (const other of after.filter((x) => x !== c)) assert.ok(!overlap(c, other), `tapa a ${other.path}`);
  assert.ok(c.y >= 14 + 440 + GAP, 'baja a la siguiente fila');
});

test('varias cartas desplazadas no acaban unas encima de otras', () => {
  const cards = [card('a.md', 14, 14), card('c.md', 802, 14), card('d.md', 1196, 14), card('e.md', 1000, 480)];
  const after = applied(cards, fitCards(cards, 900));
  for (const c of after) assert.ok(c.x + c.w <= 900, `${c.path} sigue fuera`);
  for (let i = 0; i < after.length; i++) for (let j = i + 1; j < after.length; j++) assert.ok(!overlap(after[i], after[j]), `${after[i].path} tapa a ${after[j].path}`);
  assert.deepEqual(after[0], cards[0], 'la que cabía sigue en su sitio');
});

test('una carta más ancha que el escritorio se estrecha lo justo', () => {
  const moved = fitCards([card('ancha.md', 14, 14, 1200, 300)], 900);
  assert.deepEqual(moved.get('ancha.md'), { x: GAP, y: 14, w: 900 - 2 * GAP });
});

test('con el escritorio plegado no se mueve nada', () => {
  const cards = [card('a.md', 14, 14), card('b.md', 802, 14)];
  assert.equal(fitCards(cards, 0).size, 0);
  assert.equal(fitCards(cards, 120).size, 0);
});
