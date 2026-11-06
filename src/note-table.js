// Las tablas de Markdown de una nota, sin DOM (se prueba con node): leerlas en celdas para
// editarlas como una tabla y volver a escribirlas. Las filas que no se tocan se escriben byte a
// byte igual, para que el historial del vault solo enseñe lo que cambió. Una celda guarda su
// texto con los «\|» ya convertidos en «|»: al escribirla se vuelven a escapar.

const SEPARATOR = /^:?-+:?$/;

// Las celdas de una línea de la tabla. Un «|» con barra delante es texto, no un separador.
export function splitRow(line) {
  let s = String(line ?? '').trim();
  if (s.startsWith('|')) s = s.slice(1);
  if (s.endsWith('|') && !s.endsWith('\\|')) s = s.slice(0, -1);
  const cells = [];
  let cell = '';
  for (let i = 0; i < s.length; i++) {
    if (s[i] === '\\' && s[i + 1] === '|') {
      cell += '|';
      i++;
    } else if (s[i] === '|') {
      cells.push(cell.trim());
      cell = '';
    } else cell += s[i];
  }
  cells.push(cell.trim());
  return cells;
}

// La tabla de un bloque, o `null` si no es una tabla de Markdown (falta la línea de guiones o
// no tiene tantas columnas como la cabecera): entonces se edita como texto.
export function parseTable(text) {
  const lines = String(text ?? '').split('\n');
  if (lines.length < 2) return null;
  const head = splitRow(lines[0]);
  const align = splitRow(lines[1]);
  if (align.length !== head.length || !align.every((c) => SEPARATOR.test(c))) return null;
  return {
    indent: /^[ \t]*/.exec(lines[0])[0],
    align,
    alignRaw: lines[1],
    rows: lines.filter((_, i) => i !== 1).map((raw) => ({ cells: splitRow(raw), raw })),
  };
}

const escapeCell = (text) => String(text ?? '').replace(/\r?\n/g, ' ').replace(/\|/g, '\\|');

export function serializeTable(t) {
  const line = (r) => r.raw ?? `${t.indent}| ${r.cells.map(escapeCell).join(' | ')} |`;
  const sep = t.alignRaw ?? `${t.indent}| ${t.align.join(' | ')} |`;
  const [head, ...body] = t.rows;
  return [line(head), sep, ...body.map(line)].join('\n');
}

export const colCount = (t) => t.align.length;

// Alineación de una columna para el `text-align` de sus celdas.
export function alignOf(t, col) {
  const s = t.align[col] ?? '';
  if (s.startsWith(':') && s.endsWith(':')) return 'center';
  if (s.endsWith(':')) return 'right';
  return '';
}

export const cellOf = (t, row, col) => t.rows[row]?.cells[col] ?? '';

// Cambia una celda (la fila 0 es la cabecera). Si la fila tenía menos celdas, se completa.
export function setCell(t, row, col, text) {
  const r = t.rows[row];
  if (!r) return;
  while (r.cells.length <= col) r.cells.push('');
  r.cells[col] = String(text ?? '').replace(/\r?\n/g, ' ');
  r.raw = null;
}

// Una fila vacía debajo de `after` (0 = justo bajo la cabecera).
export function addRow(t, after) {
  const at = Math.max(0, Math.min(after, t.rows.length - 1)) + 1;
  t.rows.splice(at, 0, { cells: Array(colCount(t)).fill(''), raw: null });
  return at;
}

// Quita una fila del cuerpo; la cabecera no se quita.
export function removeRow(t, row) {
  if (row < 1 || row >= t.rows.length) return false;
  t.rows.splice(row, 1);
  return true;
}

// Una columna vacía a la derecha de `after`.
export function addCol(t, after) {
  const at = Math.max(0, Math.min(after, colCount(t) - 1)) + 1;
  for (const r of t.rows) {
    while (r.cells.length < at) r.cells.push('');
    r.cells.splice(at, 0, '');
    r.raw = null;
  }
  t.align.splice(at, 0, '---');
  t.alignRaw = null;
  return at;
}

// Quita una columna; siempre queda una.
export function removeCol(t, col) {
  if (colCount(t) < 2 || col < 0 || col >= colCount(t)) return false;
  for (const r of t.rows) {
    if (col < r.cells.length) r.cells.splice(col, 1);
    r.raw = null;
  }
  t.align.splice(col, 1);
  t.alignRaw = null;
  return true;
}

// Pega una rejilla (lo que copia Excel o Sheets, ya leído) desde la celda `row`, `col`: añade
// las filas y columnas que falten.
export function pasteGrid(t, row, col, grid) {
  const height = grid.length;
  const width = Math.max(0, ...grid.map((r) => r.length));
  while (colCount(t) < col + width) addCol(t, colCount(t) - 1);
  while (t.rows.length < row + height) addRow(t, t.rows.length - 1);
  grid.forEach((r, i) => r.forEach((value, j) => setCell(t, row + i, col + j, value)));
}
