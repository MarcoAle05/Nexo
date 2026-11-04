import test from 'node:test';
import assert from 'node:assert/strict';
import { cardsIn, columnStats, dropIndex, layoutSequence, layoutTimeline, moveWithKey } from '../src/canvas-board.js';

const tablero = {
  columnas: [{ id: 'c1', titulo: 'Por hacer', limite: 1 }, { id: 'c2', titulo: 'En curso', limite: null }, { id: 'c3', titulo: 'Hecho', limite: null }],
  tarjetas: [
    { id: 't1', col: 'c1', titulo: 'A', puntos: 3 },
    { id: 't2', col: 'c1', titulo: 'B', puntos: 2 },
    { id: 't3', col: 'c2', titulo: 'C', puntos: null },
  ],
};

test('cuenta tarjetas y puntos, y avisa al pasar el límite', () => {
  assert.deepEqual(columnStats(tablero, 'c1'), { count: 2, points: 5, limit: 1, over: true });
  assert.deepEqual(columnStats(tablero, 'c2'), { count: 1, points: 0, limit: 0, over: false });
  assert.deepEqual(cardsIn(tablero, 'c3'), []);
});

test('con Ctrl y las flechas una tarjeta cambia de columna o de puesto, y en el borde no hace nada', () => {
  assert.deepEqual(moveWithKey(tablero, 't1', 'ArrowRight'), { col: 'c2', pos: 0 });
  assert.equal(moveWithKey(tablero, 't1', 'ArrowLeft'), null);
  assert.deepEqual(moveWithKey(tablero, 't2', 'ArrowRight'), { col: 'c2', pos: 1 }, 'no pasa del final de la otra');
  assert.deepEqual(moveWithKey(tablero, 't1', 'ArrowDown'), { col: 'c1', pos: 1 });
  assert.equal(moveWithKey(tablero, 't1', 'ArrowUp'), null);
  assert.equal(moveWithKey(tablero, 't2', 'ArrowDown'), null);
  assert.equal(moveWithKey(tablero, 'no', 'ArrowRight'), null);
});

test('al soltar, el puesto sale de la altura', () => {
  assert.equal(dropIndex([100, 200, 300], 50), 0);
  assert.equal(dropIndex([100, 200, 300], 150), 1);
  assert.equal(dropIndex([100, 200, 300], 900), 3);
  assert.equal(dropIndex([], 10), 0);
});

test('la secuencia se coloca sola: un participante por columna y un mensaje por fila', () => {
  const d = {
    participantes: [{ id: 'p1', nombre: 'Chat' }, { id: 'p2', nombre: 'Motor' }],
    mensajes: [
      { id: 'm1', de: 'p1', a: 'p2', texto: 'pregunta', tipo: 'sincrono' },
      { id: 'm2', de: 'p2', a: 'p2', texto: 'busca', tipo: 'sincrono' },
      { id: 'm3', de: 'p2', a: 'p1', texto: 'respuesta', tipo: 'respuesta' },
      { id: 'm4', de: 'p9', a: 'p1', texto: 'de alguien que se fue' },
    ],
  };
  const l = layoutSequence(d, 400);
  assert.ok(l.people[0].x < l.people[1].x);
  assert.equal(l.rows.length, 3, 'un mensaje de alguien que no está no se pinta');
  assert.ok(l.rows[0].y < l.rows[1].y && l.rows[1].y < l.rows[2].y);
  assert.equal(l.rows[1].self, true);
  assert.ok(l.height > l.rows[2].y);
  assert.ok(l.lineBottom <= l.height);
});

test('la línea de tiempo: la barra va de su inicio a su fin y las dependencias se unen', () => {
  const d = {
    tareas: [
      { id: 't1', titulo: 'Diseño', inicio: '2026-11-01', fin: '2026-11-10', dep: [], avance: 50 },
      { id: 't2', titulo: 'Código', inicio: '2026-11-11', fin: '2026-11-20', dep: ['t1'] },
      { id: 't3', titulo: 'Sin fechas', inicio: '', fin: '', dep: ['t1'] },
    ],
    hitos: [{ id: 'h1', titulo: 'Entrega', fecha: '2026-11-21' }],
  };
  const l = layoutTimeline(d, 800, '2026-11-05');
  const [a, b, c] = l.bars;
  assert.ok(a.x < b.x);
  assert.ok(Math.abs(a.x + a.w - b.x) < 1, 'acaba el día antes de que empiece la siguiente');
  assert.equal(a.progress, 50);
  assert.equal(c.x, null, 'sin fechas no hay barra, pero sigue en su fila');
  assert.equal(l.links.length, 1, 'solo se une lo que tiene barra');
  assert.ok(l.marks[0].x > b.x);
  assert.ok(l.today > a.x && l.today < b.x);
  assert.ok(l.ticks.length >= 2 && l.ticks.length <= 7);
  assert.ok(l.height > c.y);
  // Sin nada, no se rompe.
  const vacia = layoutTimeline({}, 600);
  assert.deepEqual([vacia.bars, vacia.marks, vacia.ticks, vacia.today], [[], [], [], null]);
});

test('las fechas de una línea de tiempo no se pisan, por estrecha que sea', () => {
  const d = { tareas: [{ id: 't', titulo: 'x', inicio: '2026-01-01', fin: '2026-12-31' }] };
  for (const width of [300, 400, 660, 1200]) {
    const { ticks } = layoutTimeline(d, width);
    assert.ok(ticks.length >= 1, `${width}`);
    for (let i = 1; i < ticks.length; i++) assert.ok(ticks[i].x - ticks[i - 1].x >= 40, `${width}: ${ticks[i].x - ticks[i - 1].x}`);
  }
});
