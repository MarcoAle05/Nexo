import test from 'node:test';
import assert from 'node:assert/strict';
import { CATALOG, COLORS, FORMS, FORM_LABELS, ICONS, catalogItem, shapePath, sizeOf, templateOps } from '../src/canvas-catalog.js';

test('cada entrada del menú tiene su nombre, para qué sirve y su icono', () => {
  const keys = new Set();
  for (const g of CATALOG) {
    for (const it of [...g.items, ...(g.strip ?? [])]) {
      assert.ok(it.label && it.hint, it.key);
      assert.match(it.icon, /^<svg /, it.key);
      assert.ok(!keys.has(it.key), `repetido: ${it.key}`);
      keys.add(it.key);
      assert.equal(catalogItem(it.key), it);
    }
  }
  assert.equal(catalogItem('no-existe'), null);
  // Lo que pidió el usuario por su nombre está.
  for (const k of ['texto', 'figura', 'flecha', 'tabla', 'tablero', 'entidad']) assert.ok(keys.has(k), k);
});

test('cada forma tiene nombre e icono, y su dibujo no tiene números rotos', () => {
  for (const forma of FORMS) {
    assert.ok(FORM_LABELS[forma], forma);
    assert.ok(ICONS[forma], forma);
    for (const [w, h] of [[176, 88], [22, 22], [800, 44]]) {
      const d = shapePath(forma, w, h);
      if (d) assert.doesNotMatch(d, /NaN|undefined|Infinity/, `${forma} ${w}×${h}`);
    }
  }
  assert.equal(shapePath('rectangulo', 100, 100), null, 'una caja va con CSS');
});

test('los tamaños de partida existen para todo', () => {
  for (const g of CATALOG) {
    for (const it of g.items) {
      if (!it.tipo || it.tipo === 'flecha') continue;
      const [w, h] = sizeOf(it.tipo, it.datos?.forma);
      assert.ok(w > 0 && h > 0, it.key);
    }
  }
});

test('hay seis roles de color, los que acepta el motor', () => {
  assert.deepEqual(COLORS.map((c) => c.id), ['neutro', 'acento', 'azul', 'verde', 'ambar', 'rojo']);
});

test('las plantillas salen en su sitio y sus flechas nombran piezas de la propia plantilla', () => {
  for (const g of CATALOG) {
    for (const it of g.items) {
      if (!it.template) continue;
      const ops = templateOps(it.template, 1000, 2000);
      assert.ok(ops.length, it.key);
      const refs = new Set(ops.map((o) => o.ref).filter(Boolean));
      for (const op of ops) {
        assert.equal(op.op, 'crear');
        if (op.tipo === 'flecha') {
          assert.ok(refs.has(op.datos.desde.id) && refs.has(op.datos.hasta.id), it.key);
        } else {
          assert.ok(op.x >= 1000 && op.y >= 2000, it.key);
        }
      }
    }
  }
  assert.deepEqual(templateOps('no-existe', 0, 0), []);
});

test('el total de la matriz de decisión usa filas que existen', () => {
  const [{ datos }] = templateOps('decision', 0, 0);
  // La fila 1 es la cabecera: las de datos empiezan en la 2.
  assert.equal(datos.filas[3][2], '=B2*C2+B3*C3+B4*C4');
  assert.equal(datos.filas.length, 4);
});
