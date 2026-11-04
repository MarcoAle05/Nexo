import test from 'node:test';
import assert from 'node:assert/strict';
import { ENTITY, aligned, anchorPoint, arrowEnds, arrowGeometry, boundsOfRects, distributed, elbowPoints, headShapes, inArea, insideFrame, readingOrder, rectOf, sideToward, snapX, snapY } from '../src/canvas-geometry.js';

const box = (id, x, y, w = 100, h = 60, extra = {}) => ({ id, tipo: 'figura', x, y, w, h, datos: {}, ...extra });
const worldOf = (...els) => {
  const m = new Map(els.map((e) => [e.id, e]));
  return (id) => m.get(id);
};

test('la retícula es la de los puntos que se ven, igual que en el motor', () => {
  // (0, 0) de la superficie = esquina del mundo: 20000 y 14000 no son múltiplos de 22.
  assert.equal((snapX(0) + 20000) % 22, 0);
  assert.equal((snapY(0) + 14000) % 22, 0);
  assert.equal(snapY(0), -8);
  assert.equal(snapX(snapX(137)), snapX(137), 'ajustar dos veces no mueve nada');
});

test('la flecha sale por el lado que mira al otro extremo', () => {
  const r = { x: 0, y: 0, w: 100, h: 60 };
  assert.equal(sideToward(r, { x: 500, y: 30 }), 'e');
  assert.equal(sideToward(r, { x: -500, y: 30 }), 'o');
  assert.equal(sideToward(r, { x: 50, y: 500 }), 's');
  assert.equal(sideToward(r, { x: 50, y: -500 }), 'n');
  assert.deepEqual(anchorPoint(r, 'e'), { x: 100, y: 30 });
});

test('la punta de una flecha sigue a su elemento', () => {
  const a = box('a', 0, 0);
  const b = box('b', 400, 0);
  const datos = { desde: { id: 'a' }, hasta: { id: 'b' } };
  let ends = arrowEnds(datos, worldOf(a, b));
  assert.deepEqual([ends.from.x, ends.from.y, ends.from.side], [100, 30, 'e']);
  assert.deepEqual([ends.to.x, ends.to.y, ends.to.side], [400, 30, 'o']);
  // Se mueve el de destino hacia abajo: la flecha llega por arriba.
  ends = arrowEnds(datos, worldOf(a, { ...b, x: 0, y: 500 }));
  assert.equal(ends.to.side, 'n');
  assert.equal(ends.from.side, 's');
});

test('un ancla fija el lado; una punta suelta es un punto; si falta el elemento, no se pinta', () => {
  const a = box('a', 0, 0);
  const ends = arrowEnds({ desde: { id: 'a', ancla: 'n' }, hasta: { x: 500, y: 500 } }, worldOf(a));
  assert.deepEqual([ends.from.x, ends.from.y], [50, 0]);
  assert.deepEqual([ends.to.x, ends.to.y, ends.to.side], [500, 500, null]);
  assert.equal(arrowEnds({ desde: { id: 'a' }, hasta: { id: 'x' } }, worldOf(a)), null);
});

test('una relación une la fila de su columna', () => {
  const tabla = box('t', 0, 0, 264, 200, { tipo: 'entidad', datos: { columnas: [{ id: 'c1', nombre: 'id' }, { id: 'c2', nombre: 'user_id' }] } });
  const otra = box('o', 600, 0);
  const ends = arrowEnds({ desde: { id: 't', columna: 'user_id' }, hasta: { id: 'o' } }, worldOf(tabla, otra));
  assert.equal(ends.from.x, 264);
  assert.equal(ends.from.y, ENTITY.head + ENTITY.row * 1.5);
  // Una columna que ya no existe no rompe la flecha: va al borde.
  const sin = arrowEnds({ desde: { id: 't', columna: 'no' }, hasta: { id: 'o' } }, worldOf(tabla, otra));
  assert.equal(sin.from.y, 100);
});

test('la ruta en ángulo solo va en horizontal y vertical, y sale perpendicular', () => {
  const casos = [
    [{ x: 100, y: 30, side: 'e' }, { x: 400, y: 200, side: 'o' }],
    [{ x: 100, y: 30, side: 'e' }, { x: 40, y: 300, side: 'n' }],
    [{ x: 50, y: 60, side: 's' }, { x: 300, y: 200, side: 'n' }],
    [{ x: 0, y: 0, side: null }, { x: 300, y: 200, side: null }],
    [{ x: 100, y: 30, side: 'e' }, { x: 300, y: 30, side: 'o' }],
  ];
  for (const [a, b] of casos) {
    const pts = elbowPoints(a, b);
    assert.deepEqual([pts[0].x, pts[0].y], [a.x, a.y]);
    assert.deepEqual([pts.at(-1).x, pts.at(-1).y], [b.x, b.y]);
    for (let i = 1; i < pts.length; i++) {
      const recto = Math.abs(pts[i].x - pts[i - 1].x) < 0.01 || Math.abs(pts[i].y - pts[i - 1].y) < 0.01;
      assert.ok(recto, `tramo ${i} de ${JSON.stringify(pts)}`);
    }
  }
  // Frente a frente y a la misma altura, una línea recta sin vértices de más.
  assert.equal(elbowPoints({ x: 100, y: 30, side: 'e' }, { x: 300, y: 30, side: 'o' }).length, 2);
});

test('cada ruta da una línea, sus extremos y su caja', () => {
  const get = worldOf(box('a', 0, 0), box('b', 400, 200));
  for (const ruta of ['recta', 'curva', 'angulada']) {
    const g = arrowGeometry({ desde: { id: 'a' }, hasta: { id: 'b' }, ruta, inicio: 'nada', fin: 'flecha' }, get);
    assert.match(g.d, /^M /);
    assert.equal(g.heads.length, 1);
    assert.ok(g.box.w > 0 && g.box.h > 0, ruta);
    assert.ok(g.mid.x > 0 && g.mid.x < 500, ruta);
  }
  assert.equal(arrowGeometry({ desde: { id: 'a' }, hasta: { id: 'no' } }, get), null);
});

test('todos los extremos se dibujan hacia donde llega la línea', () => {
  for (const kind of ['flecha', 'triangulo', 'rombo', 'uno', 'muchos', 'cero-muchos']) {
    const shapes = headShapes(kind, { x: 100, y: 100 }, { x: 1, y: 0 });
    assert.ok(shapes.length, kind);
    for (const s of shapes) assert.doesNotMatch(s.d, /NaN|undefined/, kind);
  }
  assert.deepEqual(headShapes('nada', { x: 0, y: 0 }, { x: 1, y: 0 }), []);
});

test('rectOf de una flecha sale de sus puntas', () => {
  const a = box('a', 0, 0);
  const b = box('b', 400, 0);
  const f = { id: 'f', tipo: 'flecha', x: 0, y: 0, w: 0, h: 0, datos: { desde: { id: 'a' }, hasta: { id: 'b' }, ruta: 'recta' } };
  assert.deepEqual(rectOf(f, worldOf(a, b)), { x: 100, y: 30, w: 300, h: 0 });
  assert.deepEqual(rectOf(a, worldOf(a)), { x: 0, y: 0, w: 100, h: 60 });
});

test('seleccionar por área coge lo que toca, también una flecha', () => {
  const a = box('a', 0, 0);
  const b = box('b', 400, 0);
  const lejos = box('c', 5000, 5000);
  const f = { id: 'f', tipo: 'flecha', x: 0, y: 0, w: 0, h: 0, datos: { desde: { id: 'a' }, hasta: { id: 'b' }, ruta: 'recta' } };
  const get = worldOf(a, b, lejos, f);
  assert.deepEqual(inArea([a, b, lejos, f], { x: 200, y: 0, w: 50, h: 100 }, get), ['f']);
  assert.deepEqual(inArea([a, b, lejos, f], { x: -10, y: -10, w: 600, h: 100 }, get), ['a', 'b', 'f']);
});

test('un marco lleva lo que cabe entero dentro, no a sí mismo', () => {
  const marco = { id: 'm', x: 0, y: 0, w: 500, h: 500 };
  const dentro = insideFrame(marco, [marco, { id: 'a', x: 10, y: 10, w: 100, h: 100 }, { id: 'b', x: 450, y: 450, w: 100, h: 100 }, { id: 'nota.md', x: 200, y: 200, w: 100, h: 100 }]);
  assert.deepEqual(dentro, ['a', 'nota.md']);
});

test('alinear y repartir', () => {
  const rs = [{ id: 'a', x: 0, y: 0, w: 100, h: 50 }, { id: 'b', x: 300, y: 80, w: 50, h: 50 }, { id: 'c', x: 700, y: 40, w: 100, h: 20 }];
  assert.deepEqual(aligned(rs, 'izquierda').map((r) => r.x), [0, 0, 0]);
  assert.deepEqual(aligned(rs, 'derecha').map((r) => r.x), [700, 750, 700]);
  assert.deepEqual(aligned(rs, 'arriba').map((r) => r.y), [0, 0, 0]);
  assert.deepEqual(aligned(rs, 'centro').map((r) => r.x), [350, 375, 350]);
  const rep = distributed(rs, 'x');
  assert.deepEqual(rep.map((r) => r.x), [0, 375, 700], 'los extremos no se mueven y los huecos son iguales');
  assert.deepEqual(distributed(rs.slice(0, 2), 'x'), [], 'con dos no hay nada que repartir');
  assert.deepEqual(aligned([], 'izquierda'), []);
});

test('el orden de lectura va por filas', () => {
  const items = [{ id: 'c', x: 0, y: 300 }, { id: 'b', x: 500, y: 10 }, { id: 'a', x: 0, y: 0 }];
  assert.deepEqual(readingOrder(items).map((i) => i.id), ['a', 'b', 'c']);
});

test('boundsOfRects', () => {
  assert.equal(boundsOfRects([]), null);
  assert.deepEqual(boundsOfRects([{ x: 0, y: 0, w: 10, h: 10 }, { x: 20, y: 5, w: 10, h: 10 }]), { x: 0, y: 0, w: 30, h: 15 });
});
