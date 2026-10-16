// Ventanas del workspace: reparto en mosaico y qué se ve al abrir, minimizar y cerrar.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { MAX_OPEN, addTab, closeTab, emptyWindows, fits, focusWindow, minimizeWindow, neighbor, normalizeWindows, openWindow, setMode, tileLayout, wheelDelta } from '../src/ws-layout.js';

const shape = (l) => `${l.cols}x${l.rows}`;

test('las ventanas van lado a lado mientras caben y se apilan cuando no', () => {
  assert.deepEqual(tileLayout(0, 900, 800), { cols: 0, rows: 0, cells: [] });
  assert.equal(shape(tileLayout(1, 600, 800)), '1x1');
  // 1920 px con las dos cartas abiertas deja ~950 px: dos columnas.
  assert.equal(shape(tileLayout(2, 950, 800)), '2x1');
  // 1440 px deja ~600: una encima de otra.
  assert.equal(shape(tileLayout(2, 600, 800)), '1x2');
  // Tres: tres columnas si caben; si no, la primera a la izquierda y dos a la derecha.
  assert.equal(shape(tileLayout(3, 1300, 800)), '3x1');
  const three = tileLayout(3, 950, 800);
  assert.equal(shape(three), '2x2');
  assert.deepEqual(three.cells[0], { col: 1, row: 1, colSpan: 1, rowSpan: 2 });
  assert.equal(shape(tileLayout(3, 600, 900)), '1x3');
  // Cuatro: 2×2; en un hueco estrecho y bajo, el reparto en que la más justa respira más.
  assert.equal(shape(tileLayout(4, 950, 800)), '2x2');
  assert.equal(shape(tileLayout(4, 1700, 420)), '4x1');
  assert.equal(shape(tileLayout(4, 600, 800)), '2x2');
  assert.equal(tileLayout(9, 950, 800).cells.length, MAX_OPEN);
});

test('cada celda de la rejilla está dentro y no pisa otra', () => {
  for (const n of [1, 2, 3, 4]) {
    for (const [w, h] of [[600, 700], [950, 800], [1700, 420], [2400, 1300]]) {
      const l = tileLayout(n, w, h);
      const used = new Set();
      for (const c of l.cells) {
        for (let x = c.col; x < c.col + c.colSpan; x++) for (let y = c.row; y < c.row + c.rowSpan; y++) {
          assert.ok(x <= l.cols && y <= l.rows, `fuera de la rejilla con ${n} en ${w}x${h}`);
          assert.ok(!used.has(`${x},${y}`), `celda repetida con ${n} en ${w}x${h}`);
          used.add(`${x},${y}`);
        }
      }
    }
  }
});

test('en mosaico se ven hasta cuatro y la quinta ocupa el sitio de la usada hace más', () => {
  let s = emptyWindows();
  for (const k of ['main', 'a', 'b', 'c']) s = openWindow(s, k).state;
  assert.deepEqual(s.open, ['main', 'a', 'b', 'c']);
  s = focusWindow(s, 'main');
  const { state, evicted } = openWindow(s, 'f:README.md');
  assert.equal(evicted, 'a', 'la de uso más antiguo deja su sitio');
  assert.deepEqual(state.open, ['main', 'f:README.md', 'b', 'c']);
  assert.equal(state.active, 'f:README.md');
  assert.deepEqual(state.tabs, ['main', 'a', 'b', 'c', 'f:README.md'], 'las pestañas no tienen fin');
  // Abrir una que ya se ve solo la enfoca.
  const again = openWindow(state, 'b');
  assert.equal(again.evicted, null);
  assert.deepEqual(again.state.open, state.open);
});

test('minimizar deja la pestaña; cerrar la quita; la activa pasa a la usada antes', () => {
  let s = emptyWindows();
  for (const k of ['main', 'a', 'f:x.js']) s = openWindow(s, k).state;
  s = minimizeWindow(s, 'f:x.js');
  assert.deepEqual(s.open, ['main', 'a']);
  assert.ok(s.tabs.includes('f:x.js'));
  assert.equal(s.active, 'a');
  s = closeTab(s, 'f:x.js');
  assert.ok(!s.tabs.includes('f:x.js') && !s.mru.includes('f:x.js'));
});

test('en «sola» cada ventana ocupa todo el centro', () => {
  let s = emptyWindows();
  for (const k of ['main', 'a']) s = openWindow(s, k).state;
  s = setMode(s, 'sola');
  assert.deepEqual(s.open, ['a']);
  const { state, evicted } = openWindow(s, 'main');
  assert.deepEqual(state.open, ['main']);
  assert.equal(evicted, 'a');
  assert.deepEqual(setMode(state, 'mosaico').open, ['main']);
});

test('lo guardado se normaliza contra las pestañas que existen', () => {
  const keys = new Set(['main', 'a:1', 'f:src/x.rs']);
  const s = normalizeWindows({ tabs: ['main', 'a:1', 'a:borrado', 'main', 'f:src/x.rs'], open: ['a:borrado', 'main', 'a:1', 'f:src/x.rs', 'main'], mode: 'mosaico', active: 'a:borrado' }, keys);
  assert.deepEqual(s.tabs, ['main', 'a:1', 'f:src/x.rs']);
  assert.deepEqual(s.open, ['main', 'a:1', 'f:src/x.rs']);
  assert.equal(s.active, 'main');
  assert.deepEqual(normalizeWindows(null), emptyWindows());
  const sola = normalizeWindows({ tabs: ['main', 'a:1'], open: ['main', 'a:1'], mode: 'sola', active: 'a:1' });
  assert.deepEqual(sola.open, ['a:1'], 'en «sola» se ve solo la activa');
  assert.equal(addTab(sola, 'a:1'), sola, 'una pestaña no se repite');
});

test('la rueda mueve la barra de pestañas en horizontal y las flechas dan la vuelta', () => {
  assert.equal(wheelDelta({ deltaX: 0, deltaY: 120, deltaMode: 0 }), 120);
  assert.equal(wheelDelta({ deltaX: -40, deltaY: 3, deltaMode: 0 }), -40);
  assert.equal(wheelDelta({ deltaX: 0, deltaY: 3, deltaMode: 1 }), 96);
  assert.equal(neighbor(['a', 'b', 'c'], 'c', 1), 'a');
  assert.equal(neighbor(['a', 'b', 'c'], 'a', -1), 'c');
  assert.equal(neighbor([], 'a', 1), null);
});

test('un agente nuevo solo se abre solo si cabe con su medida', () => {
  // 1440 px con las dos cartas: caben dos (una encima de otra), no tres.
  assert.equal(fits(2, 608, 740), true);
  assert.equal(fits(3, 608, 740), false);
  // 1920 px: caben cuatro en 2×2.
  assert.equal(fits(4, 950, 900), true);
});
