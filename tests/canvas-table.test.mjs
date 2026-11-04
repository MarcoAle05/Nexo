import test from 'node:test';
import assert from 'node:assert/strict';
import { ERRORS, colName, evaluate, inRange, moveCell, parseTSV, pastePlan, rangeOf, sortedRows, toNumber, toTSV } from '../src/canvas-table.js';

const size = { rows: 3, cols: 3 };

test('se recorre como una hoja de cálculo', () => {
  assert.deepEqual(moveCell({ row: 1, col: 1 }, 'ArrowUp', size), { row: 0, col: 1 });
  assert.deepEqual(moveCell({ row: 0, col: 0 }, 'ArrowUp', size), { row: 0, col: 0 }, 'no se sale');
  assert.deepEqual(moveCell({ row: 0, col: 2 }, 'Tab', size), { row: 1, col: 0 }, 'Tab sigue en la fila siguiente');
  assert.deepEqual(moveCell({ row: 1, col: 0 }, 'Tab', size, true), { row: 0, col: 2 }, 'y Mayús+Tab vuelve');
  assert.deepEqual(moveCell({ row: 2, col: 2 }, 'Tab', size), { row: 2, col: 2 });
  assert.deepEqual(moveCell({ row: 1, col: 1 }, 'Enter', size), { row: 2, col: 1 });
  assert.deepEqual(moveCell({ row: 1, col: 1 }, 'Enter', size, true), { row: 0, col: 1 });
  assert.deepEqual(moveCell({ row: 1, col: 1 }, 'Home', size), { row: 1, col: 0 });
  assert.deepEqual(moveCell({ row: 1, col: 1 }, 'End', size), { row: 1, col: 2 });
});

test('un rango da igual en qué sentido se seleccione', () => {
  const r = rangeOf({ row: 3, col: 0 }, { row: 1, col: 2 });
  assert.deepEqual(r, { top: 1, bottom: 3, left: 0, right: 2 });
  assert.ok(inRange(r, 2, 1));
  assert.ok(!inRange(r, 0, 1));
});

test('entiende lo que copia Excel: tabuladores, saltos y comillas', () => {
  assert.deepEqual(parseTSV('a\tb\nc\td\n'), [['a', 'b'], ['c', 'd']]);
  assert.deepEqual(parseTSV('a\tb'), [['a', 'b']], 'sin salto final');
  assert.deepEqual(parseTSV('"con\nsalto"\tx\r\ny\tz'), [['con\nsalto', 'x'], ['y', 'z']]);
  assert.deepEqual(parseTSV('"di ""hola"""\t'), [['di "hola"', '']]);
  assert.deepEqual(parseTSV('5" de largo\tx'), [['5" de largo', 'x']], 'unas comillas en medio son texto');
  assert.deepEqual(parseTSV(''), []);
});

test('copia para que Excel lo entienda y se puede volver a pegar', () => {
  const grid = [['a', 'con\nsalto'], ['di "hola"', 'tab\tdentro'], ['', 'x']];
  assert.deepEqual(parseTSV(toTSV(grid)), grid);
  assert.equal(toTSV([['a', 'b']]), 'a\tb');
});

test('pegar dice qué falta crear y lo que no cabe no se pierde en silencio', () => {
  const plan = pastePlan({ rows: 2, cols: 2 }, { row: 1, col: 1 }, [['a', 'b'], ['c', 'd']]);
  assert.equal(plan.addRows, 1);
  assert.equal(plan.addCols, 1);
  assert.equal(plan.cut, false);
  assert.deepEqual(plan.cells[3], { row: 2, col: 2, value: 'd' });

  const grande = pastePlan({ rows: 2, cols: 2 }, { row: 199, col: 0 }, [['a'], ['b'], ['c']]);
  assert.equal(grande.cut, true);
  assert.equal(grande.cells.length, 1, 'solo lo que cabe');
  assert.equal(grande.addRows, 198);
});

// Una tabla: la cabecera es la fila 1, como en Excel.
function tabla(filas, titulos = ['A', 'B', 'C'], cabecera = true) {
  const columnas = titulos.map((t, i) => ({ id: `c${i}`, titulo: t }));
  return {
    cabecera,
    columnas,
    filas: filas.map((f, i) => ({ id: `f${i}`, celdas: Object.fromEntries(f.map((v, j) => [`c${j}`, v]).filter(([, v]) => v !== '')) })),
  };
}
const valor = (d, f, c) => evaluate(d).get(`f${f}:c${c}`)?.text;

test('las fórmulas calculan', () => {
  const d = tabla([
    ['1', '=A2+1', '=SUMA(A2:B3)'],
    ['2,5', '=A3*2', '=PROMEDIO(A2:A3)'],
    ['texto', '=MAX(A2:A4)', '=CONTAR(A2:A4)'],
    ['=2^3', '=MIN(A2;A3;0)', '=REDONDEAR(1/3;2)'],
    ['=-A2', '=50%', '=ABS(-4)'],
    ['="a"&"b"', '=(1+2)*3', '=1<2'],
  ]);
  assert.equal(valor(d, 0, 1), '2');
  assert.equal(valor(d, 0, 2), '10.5');
  assert.equal(valor(d, 1, 1), '5');
  assert.equal(valor(d, 1, 2), '1.75');
  assert.equal(valor(d, 2, 1), '2.5');
  assert.equal(valor(d, 2, 2), '2', 'lo que no es número no cuenta');
  assert.equal(valor(d, 3, 0), '8');
  assert.equal(valor(d, 3, 1), '0');
  assert.equal(valor(d, 3, 2), '0.33');
  assert.equal(valor(d, 4, 0), '-1');
  assert.equal(valor(d, 4, 1), '0.5');
  assert.equal(valor(d, 5, 0), 'ab');
  assert.equal(valor(d, 5, 1), '9');
  assert.equal(valor(d, 5, 2), 'VERDADERO');
  assert.equal(valor(d, 2, 0), 'texto', 'el texto se enseña tal cual');
});

test('SI elige una rama y solo evalúa esa', () => {
  const d = tabla([['0', '=SI(A2=0;"cero";"otro")', '=SI(A2=0;0;B2/A2)'], ['3', '=SI(A3>1,10,20)', '=SI(A3<1;1)']]);
  assert.equal(valor(d, 0, 1), 'cero');
  assert.equal(valor(d, 0, 2), '0', 'la rama con la división no se calcula');
  assert.equal(valor(d, 1, 1), '10');
  assert.equal(valor(d, 1, 2), 'FALSO');
});

test('una fórmula mal escrita o circular da su error en su celda y no rompe la tabla', () => {
  const d = tabla([
    ['=A2', '=C2', '=B2'],
    ['=1/0', '=Z99', '=NOEXISTE(1)'],
    ['=1+', '=SUMA(', '="sin cerrar'],
    ['bien', '=A5', '=1 2'],
  ]);
  assert.equal(valor(d, 0, 0), ERRORS.cycle);
  assert.equal(valor(d, 0, 1), ERRORS.cycle);
  assert.equal(valor(d, 1, 0), ERRORS.div);
  assert.equal(valor(d, 1, 1), ERRORS.ref);
  assert.equal(valor(d, 1, 2), ERRORS.error);
  assert.equal(valor(d, 2, 0), ERRORS.error);
  assert.equal(valor(d, 2, 1), ERRORS.error);
  assert.equal(valor(d, 2, 2), ERRORS.error);
  assert.equal(valor(d, 3, 1), 'bien', 'las demás siguen calculando');
  assert.equal(valor(d, 3, 2), ERRORS.error);
  assert.equal(evaluate(d).get('f1:c1').error, true);
  assert.equal(evaluate(d).get('f3:c1').error, false);
});

test('sin cabecera, la fila 1 es la primera de datos; con ella, se puede sumar por su título', () => {
  const sin = tabla([['5', '=A1']], ['x', 'y', 'z'], false);
  assert.equal(valor(sin, 0, 1), '5');
  const con = tabla([['5', '=A1']], ['x', 'y', 'z'], true);
  assert.equal(valor(con, 0, 1), 'x');
});

test('toNumber admite la coma decimal', () => {
  assert.equal(toNumber('1,5'), 1.5);
  assert.equal(toNumber('-2.5'), -2.5);
  assert.equal(toNumber('1,234.5'), null);
  assert.equal(toNumber('abc'), null);
});

test('ordenar: números antes que texto, los vacíos al final, y estable', () => {
  const d = tabla([['10'], ['b'], [''], ['2'], ['a'], ['2']]);
  assert.deepEqual(sortedRows(d, 'c0', true), ['f3', 'f5', 'f0', 'f4', 'f1', 'f2']);
  assert.deepEqual(sortedRows(d, 'c0', false), ['f1', 'f4', 'f0', 'f3', 'f5', 'f2']);
  // Ordena por lo que se ve, no por la fórmula.
  const f = tabla([['=5'], ['=1'], ['=3']]);
  assert.deepEqual(sortedRows(f, 'c0', true), ['f1', 'f2', 'f0']);
});

test('los nombres de columna siguen tras la Z', () => {
  assert.deepEqual([0, 25, 26, 27, 701].map(colName), ['A', 'Z', 'AA', 'AB', 'ZZ']);
});
