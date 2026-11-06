import test from 'node:test';
import assert from 'node:assert/strict';
import { addCol, addRow, alignOf, cellOf, colCount, parseTable, pasteGrid, removeCol, removeRow, serializeTable, setCell, splitRow } from '../src/note-table.js';

const SEMANAS = [
  '| Semana | Fechas | Ingreso | Gastado | Restante |',
  '| --- | --- | --- | --- | --- |',
  '| Semana 1 | 1-7 nov | $500.00 | $0.00 | $500.00 |',
  '| Semana 2 | 8-14 nov | $500.00 | $0.00 | $500.00 |',
  '| **Total** |  | **$2,000.00** | **$0.00** | **$2,000.00** |',
].join('\n');

test('lee las celdas de una tabla de Markdown', () => {
  const t = parseTable(SEMANAS);
  assert.equal(colCount(t), 5);
  assert.equal(t.rows.length, 4, 'la cabecera y tres filas; la de guiones no es una fila');
  assert.equal(cellOf(t, 0, 2), 'Ingreso');
  assert.equal(cellOf(t, 3, 0), '**Total**');
  assert.equal(cellOf(t, 3, 1), '', 'una celda vacía');
  assert.equal(cellOf(t, 9, 9), '', 'fuera de la tabla no hay nada');
});

test('lo que no se toca se escribe igual, byte a byte', () => {
  const raro = '|a|  b  |\n|:-|--:|\n|1|2';
  assert.equal(serializeTable(parseTable(SEMANAS)), SEMANAS);
  assert.equal(serializeTable(parseTable(raro)), raro);
});

test('al cambiar una celda solo se reescribe su fila', () => {
  const t = parseTable(SEMANAS);
  setCell(t, 2, 3, '$120.50');
  const lines = serializeTable(t).split('\n');
  assert.equal(lines[3], '| Semana 2 | 8-14 nov | $500.00 | $120.50 | $500.00 |');
  assert.deepEqual(lines.filter((_, i) => i !== 3), SEMANAS.split('\n').filter((_, i) => i !== 3));
});

test('un «|» escrito en una celda se escapa y vuelve igual', () => {
  assert.deepEqual(splitRow('| a \\| b | c |'), ['a | b', 'c']);
  const t = parseTable('| A | B |\n| --- | --- |\n| x | y |');
  setCell(t, 1, 0, 'si | no');
  const md = serializeTable(t);
  assert.equal(md.split('\n')[2], '| si \\| no | y |');
  assert.equal(cellOf(parseTable(md), 1, 0), 'si | no');
});

test('un salto de línea en una celda pasa a espacio: rompería la tabla', () => {
  const t = parseTable('| A |\n| --- |\n| x |');
  setCell(t, 1, 0, 'uno\ndos');
  assert.equal(serializeTable(t).split('\n')[2], '| uno dos |');
});

test('una fila con menos celdas se completa al escribir en ella', () => {
  const t = parseTable('| A | B | C |\n| --- | --- | --- |\n| x |');
  setCell(t, 1, 2, 'z');
  assert.equal(serializeTable(t).split('\n')[2], '| x |  | z |');
});

test('añadir y quitar filas', () => {
  const t = parseTable(SEMANAS);
  const at = addRow(t, 2);
  assert.equal(at, 3, 'debajo de la fila 2');
  assert.deepEqual(t.rows[3].cells, ['', '', '', '', '']);
  setCell(t, 3, 0, 'Semana 3');
  assert.equal(serializeTable(t).split('\n')[4], '| Semana 3 |  |  |  |  |');
  assert.ok(removeRow(t, 3));
  assert.equal(serializeTable(t), SEMANAS);
  assert.ok(!removeRow(t, 0), 'la cabecera no se quita');
  assert.equal(addRow(t, 99), t.rows.length - 1, 'más allá del final, al final');
});

test('añadir y quitar columnas', () => {
  const t = parseTable('| A | B |\n| :-: | --: |\n| 1 | 2 |');
  assert.equal(alignOf(t, 0), 'center');
  assert.equal(alignOf(t, 1), 'right');
  assert.equal(addCol(t, 0), 1);
  setCell(t, 0, 1, 'Nueva');
  assert.equal(serializeTable(t), '| A | Nueva | B |\n| :-: | --- | --: |\n| 1 |  | 2 |');
  assert.ok(removeCol(t, 1));
  assert.equal(serializeTable(t), '| A | B |\n| :-: | --: |\n| 1 | 2 |');
  removeCol(t, 0);
  assert.ok(!removeCol(t, 0), 'siempre queda una columna');
  assert.equal(colCount(t), 1);
});

test('pegar lo copiado de una hoja de cálculo añade lo que falte', () => {
  const t = parseTable('| A | B |\n| --- | --- |\n| 1 | 2 |');
  pasteGrid(t, 1, 1, [['x', 'y'], ['z', 'w']]);
  assert.equal(serializeTable(t), '| A | B |  |\n| --- | --- | --- |\n| 1 | x | y |\n|  | z | w |');
});

test('no es una tabla sin la línea de guiones o si no cuadran las columnas', () => {
  assert.equal(parseTable('| a | b |'), null);
  assert.equal(parseTable('| a | b |\n| x | y |'), null);
  assert.equal(parseTable('| a | b |\n| --- |'), null);
});

test('una tabla con sangría la conserva en las filas nuevas', () => {
  const t = parseTable('  | A |\n  | --- |\n  | 1 |');
  addRow(t, 1);
  assert.equal(serializeTable(t), '  | A |\n  | --- |\n  | 1 |\n  |  |');
});
