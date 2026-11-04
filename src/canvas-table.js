// La tabla tipo Excel, sin DOM (se prueba con node): moverse por las celdas, pegar y copiar como
// Excel y Google Sheets, y las fórmulas. La celda guarda lo escrito; el resultado se calcula al
// pintar. Con cabecera, la fila 1 es la cabecera, como en una hoja de cálculo.

export const MAX_ROWS = 200;
export const MAX_COLS = 30;

// ------------------------------------------------------------ moverse

// La celda a la que se va con una tecla. `size` = { rows, cols }.
export function moveCell(at, key, { rows, cols }, shift = false) {
  const clamp = (v, max) => Math.max(0, Math.min(max - 1, v));
  switch (key) {
    case 'ArrowUp':
      return { row: clamp(at.row - 1, rows), col: at.col };
    case 'ArrowDown':
      return { row: clamp(at.row + 1, rows), col: at.col };
    case 'ArrowLeft':
      return { row: at.row, col: clamp(at.col - 1, cols) };
    case 'ArrowRight':
      return { row: at.row, col: clamp(at.col + 1, cols) };
    case 'Enter':
      return { row: clamp(at.row + (shift ? -1 : 1), rows), col: at.col };
    case 'Tab': {
      // Al llegar al borde sigue por la fila de al lado, como en una hoja de cálculo.
      const n = at.row * cols + at.col + (shift ? -1 : 1);
      const i = Math.max(0, Math.min(rows * cols - 1, n));
      return { row: Math.floor(i / cols), col: i % cols };
    }
    case 'Home':
      return { row: at.row, col: 0 };
    case 'End':
      return { row: at.row, col: cols - 1 };
    default:
      return at;
  }
}

export const rangeOf = (a, b) => ({
  top: Math.min(a.row, b.row),
  bottom: Math.max(a.row, b.row),
  left: Math.min(a.col, b.col),
  right: Math.max(a.col, b.col),
});
export const inRange = (r, row, col) => row >= r.top && row <= r.bottom && col >= r.left && col <= r.right;

// ------------------------------------------------------------ pegar y copiar

// Lo que da Excel o Sheets al copiar: tabuladores y saltos de línea; una celda con un salto de
// línea, un tabulador o comillas va entre comillas, y sus comillas van dobles.
export function parseTSV(text) {
  const rows = [];
  let row = [];
  let cell = '';
  let quoted = false;
  const s = String(text ?? '').replace(/\r\n?/g, '\n');
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (quoted) {
      if (c === '"' && s[i + 1] === '"') {
        cell += '"';
        i++;
      } else if (c === '"') {
        quoted = false;
      } else {
        cell += c;
      }
    } else if (c === '"' && cell === '') {
      quoted = true;
    } else if (c === '\t') {
      row.push(cell);
      cell = '';
    } else if (c === '\n') {
      row.push(cell);
      rows.push(row);
      row = [];
      cell = '';
    } else {
      cell += c;
    }
  }
  if (cell !== '' || row.length) {
    row.push(cell);
    rows.push(row);
  }
  return rows;
}

export function toTSV(rows) {
  const cell = (v) => {
    const s = String(v ?? '');
    return /[\t\n"]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  };
  return rows.map((r) => r.map(cell).join('\t')).join('\n');
}

// Qué hay que hacer para pegar `grid` en la celda `at`: las filas y columnas que faltan y las
// celdas. Lo que no cabe en los topes se dice en `cut`, no se pierde en silencio.
export function pastePlan(size, at, grid) {
  const height = grid.length;
  const width = Math.max(0, ...grid.map((r) => r.length));
  const wantRows = at.row + height;
  const wantCols = at.col + width;
  const rows = Math.min(wantRows, MAX_ROWS);
  const cols = Math.min(wantCols, MAX_COLS);
  const cells = [];
  grid.forEach((r, i) => {
    r.forEach((value, j) => {
      if (at.row + i < rows && at.col + j < cols) cells.push({ row: at.row + i, col: at.col + j, value });
    });
  });
  return {
    addRows: Math.max(0, rows - size.rows),
    addCols: Math.max(0, cols - size.cols),
    cells,
    cut: wantRows > MAX_ROWS || wantCols > MAX_COLS,
  };
}

// ------------------------------------------------------------ fórmulas

export const ERRORS = { ref: '#REF!', div: '#DIV/0!', cycle: '#CICLO!', error: '#ERROR!' };

export const colName = (i) => {
  let n = i + 1;
  let s = '';
  while (n > 0) {
    const r = (n - 1) % 26;
    s = String.fromCharCode(65 + r) + s;
    n = Math.floor((n - 1) / 26);
  }
  return s;
};
const colIndex = (name) => [...name].reduce((n, c) => n * 26 + c.charCodeAt(0) - 64, 0) - 1;

class FormulaError extends Error {
  constructor(code) {
    super(code);
    this.code = code;
  }
}

function tokenize(src) {
  const out = [];
  let i = 0;
  while (i < src.length) {
    const c = src[i];
    if (/\s/.test(c)) {
      i++;
    } else if (/\d|\./.test(c)) {
      const m = /^(\d+(\.\d*)?|\.\d+)/.exec(src.slice(i));
      if (!m) throw new FormulaError(ERRORS.error);
      out.push({ t: 'num', v: Number(m[1]) });
      i += m[1].length;
    } else if (c === '"') {
      let j = i + 1;
      let s = '';
      for (; j < src.length; j++) {
        if (src[j] === '"' && src[j + 1] === '"') {
          s += '"';
          j++;
        } else if (src[j] === '"') break;
        else s += src[j];
      }
      if (j >= src.length) throw new FormulaError(ERRORS.error);
      out.push({ t: 'str', v: s });
      i = j + 1;
    } else if (/[a-zA-ZÀ-ÿ]/.test(c)) {
      const m = /^[a-zA-ZÀ-ÿ_][\wÀ-ÿ.]*/.exec(src.slice(i));
      const word = m[0].toUpperCase();
      i += m[0].length;
      if (/^[A-Z]{1,2}\d+$/.test(word)) out.push({ t: 'ref', v: word });
      else out.push({ t: 'name', v: word });
    } else if (src.startsWith('<=', i) || src.startsWith('>=', i) || src.startsWith('<>', i)) {
      out.push({ t: 'op', v: src.slice(i, i + 2) });
      i += 2;
    } else if ('+-*/^%&=<>(),;:'.includes(c)) {
      out.push({ t: 'op', v: c });
      i++;
    } else {
      throw new FormulaError(ERRORS.error);
    }
  }
  return out;
}

function parseCellName(name) {
  const m = /^([A-Z]+)(\d+)$/.exec(name);
  return { col: colIndex(m[1]), row: Number(m[2]) - 1 };
}

const num = (v) => {
  if (v == null || v === '') return 0;
  if (typeof v === 'number') return v;
  if (typeof v === 'boolean') return v ? 1 : 0;
  const n = toNumber(v);
  if (n == null) throw new FormulaError(ERRORS.error);
  return n;
};

// Un texto como número: admite la coma decimal si no hay punto («1,5»).
export function toNumber(text) {
  const s = String(text).trim();
  if (/^-?\d+(\.\d+)?$/.test(s) || /^-?\.\d+$/.test(s)) return Number(s);
  if (/^-?\d+,\d+$/.test(s)) return Number(s.replace(',', '.'));
  return null;
}

const flat = (args) => args.flatMap((a) => (Array.isArray(a) ? a : [a]));
// En un rango, lo que no es número no cuenta; un número suelto escrito a mano, sí.
const numbers = (args) =>
  flat(args)
    .map((v) => (typeof v === 'number' ? v : typeof v === 'string' ? toNumber(v) : null))
    .filter((v) => v != null);

const FUNCTIONS = {
  SUMA: (a) => numbers(a).reduce((x, y) => x + y, 0),
  PROMEDIO: (a) => {
    const n = numbers(a);
    if (!n.length) throw new FormulaError(ERRORS.div);
    return n.reduce((x, y) => x + y, 0) / n.length;
  },
  MIN: (a) => {
    const n = numbers(a);
    return n.length ? Math.min(...n) : 0;
  },
  MAX: (a) => {
    const n = numbers(a);
    return n.length ? Math.max(...n) : 0;
  },
  CONTAR: (a) => numbers(a).length,
  ABS: ([v]) => Math.abs(num(v)),
  REDONDEAR: ([v, d]) => {
    const f = 10 ** num(d ?? 0);
    return Math.round(num(v) * f) / f;
  },
};
Object.assign(FUNCTIONS, { SUM: FUNCTIONS.SUMA, AVERAGE: FUNCTIONS.PROMEDIO, COUNT: FUNCTIONS.CONTAR, ROUND: FUNCTIONS.REDONDEAR });

// Analizador descendente: comparación > concatenación > suma > producto > potencia > unario.
function evaluateFormula(src, getCell) {
  const tokens = tokenize(src);
  let p = 0;
  const peek = () => tokens[p];
  const isOp = (...ops) => peek()?.t === 'op' && ops.includes(peek().v);
  const expectOp = (op) => {
    if (!isOp(op)) throw new FormulaError(ERRORS.error);
    p++;
  };

  function compare() {
    let a = concat();
    while (isOp('=', '<>', '<', '<=', '>', '>=')) {
      const op = tokens[p++].v;
      const b = concat();
      const [x, y] = typeof a === 'string' || typeof b === 'string' ? [String(a ?? ''), String(b ?? '')] : [num(a), num(b)];
      a = { '=': x === y, '<>': x !== y, '<': x < y, '<=': x <= y, '>': x > y, '>=': x >= y }[op];
    }
    return a;
  }
  function concat() {
    let a = sum();
    while (isOp('&')) {
      p++;
      a = `${a ?? ''}${sum() ?? ''}`;
    }
    return a;
  }
  function sum() {
    let a = product();
    while (isOp('+', '-')) {
      const op = tokens[p++].v;
      const b = product();
      a = op === '+' ? num(a) + num(b) : num(a) - num(b);
    }
    return a;
  }
  function product() {
    let a = power();
    while (isOp('*', '/')) {
      const op = tokens[p++].v;
      const b = power();
      if (op === '/') {
        if (num(b) === 0) throw new FormulaError(ERRORS.div);
        a = num(a) / num(b);
      } else a = num(a) * num(b);
    }
    return a;
  }
  function power() {
    const a = unary();
    if (isOp('^')) {
      p++;
      return num(a) ** num(power());
    }
    return a;
  }
  function unary() {
    if (isOp('-')) {
      p++;
      return -num(unary());
    }
    if (isOp('+')) {
      p++;
      return num(unary());
    }
    let v = atom();
    while (isOp('%')) {
      p++;
      v = num(v) / 100;
    }
    return v;
  }
  function atom() {
    const t = peek();
    if (!t) throw new FormulaError(ERRORS.error);
    if (t.t === 'num' || t.t === 'str') {
      p++;
      return t.v;
    }
    if (t.t === 'ref') {
      p++;
      const a = parseCellName(t.v);
      if (isOp(':')) {
        p++;
        const e = peek();
        if (e?.t !== 'ref') throw new FormulaError(ERRORS.error);
        p++;
        const b = parseCellName(e.v);
        const list = [];
        for (let r = Math.min(a.row, b.row); r <= Math.max(a.row, b.row); r++) {
          for (let c = Math.min(a.col, b.col); c <= Math.max(a.col, b.col); c++) list.push(getCell(r, c));
        }
        return list;
      }
      return getCell(a.row, a.col);
    }
    if (t.t === 'name') {
      p++;
      if (t.v === 'VERDADERO' || t.v === 'TRUE') return true;
      if (t.v === 'FALSO' || t.v === 'FALSE') return false;
      expectOp('(');
      if (t.v === 'SI' || t.v === 'IF') return conditional();
      const args = [];
      if (!isOp(')')) {
        args.push(compare());
        while (isOp(',', ';')) {
          p++;
          args.push(compare());
        }
      }
      expectOp(')');
      const fn = FUNCTIONS[t.v];
      if (!fn) throw new FormulaError(ERRORS.error);
      return fn(args);
    }
    if (isOp('(')) {
      p++;
      const v = compare();
      expectOp(')');
      return v;
    }
    throw new FormulaError(ERRORS.error);
  }
  // SI evalúa solo la rama que toca: `=SI(A1=0;0;B1/A1)` no da #DIV/0! con A1 a cero.
  function conditional() {
    const test = compare();
    const sep = () => {
      if (!isOp(',', ';')) throw new FormulaError(ERRORS.error);
      p++;
    };
    const branch = (take) => {
      if (take) return compare();
      // Se salta la rama sin evaluarla.
      let depth = 0;
      while (peek()) {
        if (isOp('(')) depth++;
        else if (isOp(')')) {
          if (depth === 0) return undefined;
          depth--;
        } else if (depth === 0 && isOp(',', ';')) return undefined;
        p++;
      }
      throw new FormulaError(ERRORS.error);
    };
    const yes = Boolean(typeof test === 'string' ? test : num(test));
    sep();
    const a = branch(yes);
    let b = false;
    if (isOp(',', ';')) {
      p++;
      b = branch(!yes);
    }
    expectOp(')');
    return yes ? a : b;
  }

  const value = compare();
  if (p < tokens.length) throw new FormulaError(ERRORS.error);
  return Array.isArray(value) ? (value.length === 1 ? value[0] : (() => { throw new FormulaError(ERRORS.error); })()) : value;
}

const shown = (v) => {
  if (typeof v === 'number') return Number.isFinite(v) ? String(Number(v.toFixed(10))) : ERRORS.error;
  if (typeof v === 'boolean') return v ? 'VERDADERO' : 'FALSO';
  return String(v ?? '');
};

// Calcula una tabla. Da, para cada celda con algo escrito, lo que se enseña: `{ text, error }` con
// la clave `fila:col`. Un error o un ciclo se queda en su celda: no rompe la tabla.
export function evaluate(datos) {
  const cols = datos?.columnas ?? [];
  const rows = datos?.filas ?? [];
  const header = datos?.cabecera !== false;
  const memo = new Map();
  const walking = new Set();
  // La fila 1 de la hoja es la cabecera si la hay.
  const rawAt = (row, col) => {
    if (col < 0 || col >= cols.length || row < 0) return { missing: true };
    if (header) {
      if (row === 0) return { raw: cols[col].titulo ?? '' };
      row -= 1;
    }
    if (row >= rows.length) return { missing: true };
    return { raw: rows[row].celdas?.[cols[col].id] ?? '' };
  };
  const compute = (row, col) => {
    const key = `${row}:${col}`;
    if (memo.has(key)) return memo.get(key);
    const { raw, missing } = rawAt(row, col);
    if (missing) throw new FormulaError(ERRORS.ref);
    if (walking.has(key)) throw new FormulaError(ERRORS.cycle);
    let value;
    if (typeof raw === 'string' && raw.startsWith('=')) {
      walking.add(key);
      try {
        value = evaluateFormula(raw.slice(1), compute);
      } finally {
        walking.delete(key);
      }
    } else {
      value = raw;
      const n = typeof raw === 'string' ? toNumber(raw) : null;
      if (n != null) value = n;
    }
    memo.set(key, value);
    return value;
  };
  const out = new Map();
  rows.forEach((f, i) => {
    cols.forEach((c, j) => {
      const raw = f.celdas?.[c.id];
      if (raw == null || raw === '') return;
      const key = `${f.id}:${c.id}`;
      if (typeof raw !== 'string' || !raw.startsWith('=')) {
        out.set(key, { text: String(raw), error: false });
        return;
      }
      try {
        out.set(key, { text: shown(compute(i + (header ? 1 : 0), j)), error: false });
      } catch (e) {
        out.set(key, { text: e instanceof FormulaError ? e.code : ERRORS.error, error: true });
      }
    });
  });
  return out;
}

// El orden de las filas al ordenar por una columna: números antes que texto, los vacíos al final
// y, a igualdad, como estaban.
export function sortedRows(datos, colId, asc = true) {
  const values = evaluate(datos);
  const key = (f) => {
    const text = values.get(`${f.id}:${colId}`)?.text ?? '';
    const n = toNumber(text);
    return { empty: text === '', n, text };
  };
  return (datos?.filas ?? [])
    .map((f, i) => ({ f, i, k: key(f) }))
    .sort((a, b) => {
      if (a.k.empty !== b.k.empty) return a.k.empty ? 1 : -1;
      if (a.k.empty) return a.i - b.i;
      let d;
      if (a.k.n != null && b.k.n != null) d = a.k.n - b.k.n;
      else if (a.k.n != null) d = -1;
      else if (b.k.n != null) d = 1;
      else d = a.k.text.localeCompare(b.k.text, 'es', { numeric: true, sensitivity: 'base' });
      return (asc ? d : -d) || a.i - b.i;
    })
    .map(({ f }) => f.id);
}
