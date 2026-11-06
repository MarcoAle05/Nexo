// Las tablas y los tableros del lienzo: los dos elementos con interacción propia dentro. Lo que
// cambia va por partes (`celdas`, `tarjeta-mover`…): así lo que hace el usuario no le impide al
// agente seguir añadiendo, y deshacer no se lleva lo que añadió otro. Las cuentas están en
// canvas-table.js y canvas-board.js, sin DOM.

import { colName, evaluate, inRange, moveCell, parseTSV, pastePlan, rangeOf, sortedRows, toTSV, MAX_COLS, MAX_ROWS } from './canvas-table.js';
import { cardsIn, columnStats, dropIndex, moveWithKey } from './canvas-board.js';

const HEAD_ROW = 34;
const ROW = 34;
const MAX_AUTO_H = 660;
const newPart = (p) => `${p}${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;

export function createGrid(api) {
  // La celda activa: en qué tabla, dónde está y hasta dónde llega el rango.
  let table = null; // { id, active: {row, col}, anchor: {row, col} }
  let editing = null; // { id, input, fila, col, usekey }
  let card = null; // { id, tarjeta }
  // Un elemento en el que se está escribiendo o arrastrando dentro: no se repinta bajo las manos.
  const holding = new Set();

  const fit = (el, datos) => {
    const w = (datos.columnas ?? []).reduce((n, c) => n + (c.ancho ?? 132), 0) + 2;
    const head = datos.cabecera === false ? 0 : HEAD_ROW;
    const h = Math.min(MAX_AUTO_H, head + ROW * (datos.filas?.length ?? 0) + 2 + (datos.titulo ? 34 : 0));
    return { op: 'cambiar', id: el.id, w, h };
  };

  // ------------------------------------------------------------ tabla: pintar

  function paintTable(body, el) {
    const d = el.datos;
    const values = evaluate(d);
    body.replaceChildren();
    if (d.titulo) {
      const head = document.createElement('div');
      head.className = 'cv-card-head cv-grip';
      const title = document.createElement('div');
      title.className = 'cv-card-title';
      title.textContent = d.titulo;
      head.append(title);
      body.append(head);
    }
    const wrap = document.createElement('div');
    wrap.className = 'cv-table-wrap cv-scroll';
    const t = document.createElement('table');
    t.className = 'cv-table';
    t.setAttribute('role', 'grid');
    t.setAttribute('aria-label', d.titulo || 'Tabla');
    const colgroup = document.createElement('colgroup');
    for (const c of d.columnas) {
      const col = document.createElement('col');
      col.style.width = `${c.ancho ?? 132}px`;
      colgroup.append(col);
    }
    t.append(colgroup);
    const header = d.cabecera !== false;
    if (header) {
      const thead = document.createElement('thead');
      const tr = document.createElement('tr');
      d.columnas.forEach((c, j) => {
        const th = document.createElement('th');
        th.className = 'cv-grip';
        th.scope = 'col';
        th.dataset.col = c.id;
        th.dataset.letter = colName(j);
        const text = document.createElement('span');
        text.textContent = c.titulo;
        const grip = document.createElement('span');
        grip.className = 'cv-col-resize';
        grip.setAttribute('aria-hidden', 'true');
        th.append(text, grip);
        tr.append(th);
      });
      thead.append(tr);
      t.append(thead);
    }
    const tbody = document.createElement('tbody');
    d.filas.forEach((f, i) => {
      const tr = document.createElement('tr');
      tr.dataset.row = f.id;
      d.columnas.forEach((c, j) => {
        const td = document.createElement('td');
        td.setAttribute('role', 'gridcell');
        td.dataset.col = c.id;
        td.dataset.r = i;
        td.dataset.c = j;
        td.tabIndex = -1;
        const v = values.get(`${f.id}:${c.id}`);
        if (v) {
          td.textContent = v.text;
          td.classList.toggle('is-error', v.error);
          if (/^-?\d/.test(v.text) && !Number.isNaN(Number(v.text))) td.classList.add('is-num');
          if (String(f.celdas?.[c.id] ?? '').startsWith('=')) td.title = f.celdas[c.id];
        }
        if (j === 0) td.dataset.n = String(i + (header ? 2 : 1));
        tr.append(td);
      });
      tbody.append(tr);
    });
    t.append(tbody);
    wrap.append(t);
    body.append(wrap);
    if (table?.id === el.id) showActive(el, { focus: false });
  }

  const tableNode = (id) => api.nodeOf(id)?.querySelector('.cv-table');
  const sizeOf = (d) => ({ rows: d.filas.length, cols: d.columnas.length });

  function showActive(el, { focus = true } = {}) {
    const t = tableNode(el.id);
    if (!t || !table) return;
    const size = sizeOf(el.datos);
    table.active = { row: Math.min(table.active.row, size.rows - 1), col: Math.min(table.active.col, size.cols - 1) };
    table.anchor = { row: Math.min(table.anchor.row, size.rows - 1), col: Math.min(table.anchor.col, size.cols - 1) };
    const range = rangeOf(table.active, table.anchor);
    for (const td of t.querySelectorAll('td')) {
      const r = Number(td.dataset.r);
      const c = Number(td.dataset.c);
      const on = r === table.active.row && c === table.active.col;
      td.classList.toggle('is-active', on);
      td.classList.toggle('in-range', inRange(range, r, c) && !(range.top === range.bottom && range.left === range.right));
      td.tabIndex = on ? 0 : -1;
      if (on && focus) td.focus({ preventScroll: false });
    }
    api.barChanged();
  }

  function setActive(id, row, col, extend = false) {
    if (table?.id !== id) table = { id, active: { row, col }, anchor: { row, col } };
    table.active = { row, col };
    if (!extend) table.anchor = { row, col };
    showActive(api.get(id));
  }

  // ------------------------------------------------------------ tabla: escribir

  function beginEdit(el, seed = null) {
    if (!table || table.id !== el.id) return;
    const f = el.datos.filas[table.active.row];
    const c = el.datos.columnas[table.active.col];
    if (!f || !c) return;
    const td = tableNode(el.id)?.querySelector(`td[data-r="${table.active.row}"][data-c="${table.active.col}"]`);
    if (!td) return;
    const raw = f.celdas?.[c.id] ?? '';
    const usekey = `${el.id}/${f.id}:${c.id}`;
    api.use(usekey, true);
    const input = document.createElement('input');
    input.className = 'cv-cell-input';
    input.value = seed ?? raw;
    input.setAttribute('aria-label', `Celda ${colName(table.active.col)}${Number(td.dataset.r) + (el.datos.cabecera !== false ? 2 : 1)}`);
    editing = { id: el.id, input, fila: f.id, col: c.id, usekey, raw };
    holding.add(el.id);
    td.textContent = '';
    td.classList.remove('is-error');
    td.append(input);
    input.focus();
    input.setSelectionRange(input.value.length, input.value.length);
    input.addEventListener('keydown', (event) => {
      event.stopPropagation();
      if (event.key === 'Escape') {
        finishEdit(false);
        showActive(api.get(el.id));
      } else if (event.key === 'Enter' || event.key === 'Tab') {
        event.preventDefault();
        finishEdit(true);
        const now = api.get(el.id);
        const next = moveCell(table.active, event.key, sizeOf(now.datos), event.shiftKey);
        setActive(el.id, next.row, next.col);
      }
    });
    input.addEventListener('blur', () => finishEdit(true));
  }

  function finishEdit(save) {
    if (!editing) return;
    const { id, input, fila, col, usekey, raw } = editing;
    editing = null;
    holding.delete(id);
    api.use(usekey, false);
    const value = input.value;
    input.remove();
    if (save && value !== raw) {
      api.apply([{ op: 'celdas', id, celdas: [{ fila, col, valor: value }] }]);
    } else {
      api.repaint(id);
    }
  }

  function selectedCells(el) {
    const range = rangeOf(table.active, table.anchor);
    const out = [];
    for (let r = range.top; r <= range.bottom; r++) {
      for (let c = range.left; c <= range.right; c++) {
        out.push({ fila: el.datos.filas[r]?.id, col: el.datos.columnas[c]?.id, r, c });
      }
    }
    return out.filter((x) => x.fila && x.col);
  }

  function copyRange(el) {
    const values = evaluate(el.datos);
    const range = rangeOf(table.active, table.anchor);
    const rows = [];
    for (let r = range.top; r <= range.bottom; r++) {
      const row = [];
      for (let c = range.left; c <= range.right; c++) {
        const f = el.datos.filas[r];
        const col = el.datos.columnas[c];
        row.push(f && col ? (values.get(`${f.id}:${col.id}`)?.text ?? '') : '');
      }
      rows.push(row);
    }
    navigator.clipboard?.writeText(toTSV(rows)).catch(() => {});
  }

  function pasteText(el, text) {
    const grid = parseTSV(text);
    if (!grid.length) return;
    const plan = pastePlan(sizeOf(el.datos), table.active, grid);
    const ops = [];
    const rows = el.datos.filas.map((f) => f.id);
    const cols = el.datos.columnas.map((c) => c.id);
    for (let i = 0; i < plan.addCols; i++) {
      const id = newPart('c');
      cols.push(id);
      ops.push({ op: 'columna-nueva', id: el.id, col: id, titulo: '' });
    }
    for (let i = 0; i < plan.addRows; i++) {
      const id = newPart('f');
      rows.push(id);
      ops.push({ op: 'fila-nueva', id: el.id, fila: id });
    }
    ops.push({ op: 'celdas', id: el.id, celdas: plan.cells.map((c) => ({ fila: rows[c.row], col: cols[c.col], valor: c.value })) });
    if (plan.addRows || plan.addCols) {
      const w = (el.datos.columnas ?? []).reduce((n, c) => n + (c.ancho ?? 132), 0) + 132 * plan.addCols + 2;
      const h = Math.min(MAX_AUTO_H, HEAD_ROW + ROW * rows.length + 2 + (el.datos.titulo ? 34 : 0));
      ops.push({ op: 'cambiar', id: el.id, w, h });
    }
    api.apply(ops);
    if (plan.cut) api.status(`No cabe todo: una tabla llega hasta ${MAX_ROWS} filas y ${MAX_COLS} columnas.`);
  }

  // Se engancha una vez sobre la superficie: los elementos van y vienen al mover la vista.
  api.surface.addEventListener('pointerdown', (event) => {
    const td = event.target.closest?.('.cv-table td');
    if (td && event.button === 0) {
      const id = td.closest('.cv-el')?.dataset.id;
      const el = api.get(id);
      if (!el) return;
      event.stopPropagation();
      if (!api.isSelected(id)) api.select([id], { focus: false });
      const row = Number(td.dataset.r);
      const col = Number(td.dataset.c);
      if (editing && editing.id === id && td.contains(editing.input)) return;
      event.preventDefault();
      setActive(id, row, col, event.shiftKey);
      const move = (e) => {
        const over = document.elementFromPoint(e.clientX, e.clientY)?.closest?.('.cv-table td');
        if (over && over.closest('.cv-el')?.dataset.id === id) {
          table.active = { row: Number(over.dataset.r), col: Number(over.dataset.c) };
          showActive(api.get(id), { focus: false });
        }
      };
      const up = () => {
        window.removeEventListener('pointermove', move);
        window.removeEventListener('pointerup', up);
      };
      window.addEventListener('pointermove', move);
      window.addEventListener('pointerup', up);
      return;
    }
    const resize = event.target.closest?.('.cv-col-resize');
    if (resize && event.button === 0) {
      event.stopPropagation();
      event.preventDefault();
      const th = resize.closest('th');
      const id = th.closest('.cv-el').dataset.id;
      const el = api.get(id);
      const at = el.datos.columnas.findIndex((c) => c.id === th.dataset.col);
      const col = th.closest('table').querySelectorAll('col')[at];
      const start = { px: event.clientX, w: el.datos.columnas[at].ancho ?? 132 };
      let width = start.w;
      const move = (e) => {
        width = Math.max(44, start.w + (e.clientX - start.px) / api.scale());
        col.style.width = `${width}px`;
      };
      const up = () => {
        window.removeEventListener('pointermove', move);
        window.removeEventListener('pointerup', up);
        if (width === start.w) return;
        const total = el.datos.columnas.reduce((n, c, i) => n + (i === at ? width : (c.ancho ?? 132)), 0) + 2;
        api.apply([{ op: 'columna-cambiar', id, col: th.dataset.col, ancho: Math.round(width) }, { op: 'cambiar', id, w: total }]);
      };
      window.addEventListener('pointermove', move);
      window.addEventListener('pointerup', up);
    }
  }, true);

  api.surface.addEventListener('dblclick', (event) => {
    const td = event.target.closest?.('.cv-table td');
    if (td) {
      const el = api.get(td.closest('.cv-el').dataset.id);
      setActive(el.id, Number(td.dataset.r), Number(td.dataset.c));
      beginEdit(el);
      event.stopPropagation();
      return;
    }
    const th = event.target.closest?.('.cv-table th');
    if (th && !event.target.closest('.cv-col-resize')) {
      const el = api.get(th.closest('.cv-el').dataset.id);
      event.stopPropagation();
      api.editInline(th.firstElementChild, th.firstElementChild.textContent, (value) => api.apply([{ op: 'columna-cambiar', id: el.id, col: th.dataset.col, titulo: value }]));
    }
  });

  function tableKey(event, el) {
    if (editing) return false;
    const mod = event.ctrlKey || event.metaKey;
    if (mod && event.key.toLowerCase() === 'c') {
      copyRange(el);
      return true;
    }
    if (mod && event.key.toLowerCase() === 'x') {
      copyRange(el);
      clearRange(el);
      return true;
    }
    if (mod && event.key.toLowerCase() === 'v') {
      navigator.clipboard?.readText().then((text) => text && pasteText(api.get(el.id), text)).catch(() => {});
      return true;
    }
    if (mod) return false;
    if (['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Tab', 'Home', 'End'].includes(event.key)) {
      const next = moveCell(table.active, event.key, sizeOf(el.datos), event.shiftKey);
      setActive(el.id, next.row, next.col, event.shiftKey && event.key !== 'Tab');
      return true;
    }
    if (event.key === 'Enter' || event.key === 'F2') {
      beginEdit(el);
      return true;
    }
    if (event.key === 'Delete' || event.key === 'Backspace') {
      clearRange(el);
      return true;
    }
    if (event.key === 'Escape') {
      table = null;
      api.nodeOf(el.id)?.querySelectorAll('td.is-active, td.in-range').forEach((td) => td.classList.remove('is-active', 'in-range'));
      api.focusEl(el.id);
      api.barChanged();
      return true;
    }
    if (event.key.length === 1) {
      beginEdit(el, event.key);
      return true;
    }
    return false;
  }

  function clearRange(el) {
    const celdas = selectedCells(el).map((c) => ({ fila: c.fila, col: c.col, valor: '' }));
    if (celdas.length) api.apply([{ op: 'celdas', id: el.id, celdas }]);
  }

  // Lo que hace la barra de la tabla.
  function tableAction(el, action) {
    const d = el.datos;
    const at = table?.id === el.id ? table.active : { row: d.filas.length - 1, col: d.columnas.length - 1 };
    const ops = [];
    const after = (list, id) => {
      // Una barra de la capa siguiente se calcula con la tabla ya cambiada.
      const copy = structuredClone(d);
      list(copy);
      return { op: 'cambiar', ...fit(el, copy), id };
    };
    switch (action) {
      case 'fila-nueva': {
        if (d.filas.length >= MAX_ROWS) return api.status(`Una tabla llega hasta ${MAX_ROWS} filas.`);
        const fila = newPart('f');
        ops.push({ op: 'fila-nueva', id: el.id, fila, pos: at.row + 1 }, after((c) => c.filas.push({ id: fila, celdas: {} }), el.id));
        break;
      }
      case 'columna-nueva': {
        if (d.columnas.length >= MAX_COLS) return api.status(`Una tabla llega hasta ${MAX_COLS} columnas.`);
        const col = newPart('c');
        ops.push({ op: 'columna-nueva', id: el.id, col, pos: at.col + 1, titulo: '' }, after((c) => c.columnas.push({ id: col, ancho: 132 }), el.id));
        break;
      }
      case 'fila-borrar': {
        if (d.filas.length <= 1) return api.status('Tiene que quedar al menos una fila.');
        const f = d.filas[at.row];
        ops.push({ op: 'fila-borrar', id: el.id, fila: f.id }, after((c) => c.filas.splice(at.row, 1), el.id));
        break;
      }
      case 'columna-borrar': {
        if (d.columnas.length <= 1) return api.status('Tiene que quedar al menos una columna.');
        const c = d.columnas[at.col];
        ops.push({ op: 'columna-borrar', id: el.id, col: c.id }, after((x) => x.columnas.splice(at.col, 1), el.id));
        break;
      }
      case 'orden-asc':
      case 'orden-desc': {
        const c = d.columnas[at.col];
        ops.push({ op: 'filas-orden', id: el.id, orden: sortedRows(d, c.id, action === 'orden-asc') });
        break;
      }
      case 'titulo':
        if (d.titulo) return api.beginText(el.id);
        return api.apply([{ op: 'cambiar', id: el.id, datos: { titulo: 'Tabla' } }, after((c) => (c.titulo = 'Tabla'), el.id)]).then(() => api.beginText(el.id));
      case 'cabecera':
        ops.push({ op: 'cambiar', id: el.id, datos: { cabecera: d.cabecera === false } }, after((c) => (c.cabecera = d.cabecera === false), el.id));
        break;
      default:
        return undefined;
    }
    return api.apply(ops);
  }

  // Cómo se llama la celda activa y qué tiene escrito, para la barra.
  function activeCell(el) {
    if (table?.id !== el.id) return null;
    const f = el.datos.filas[table.active.row];
    const c = el.datos.columnas[table.active.col];
    if (!f || !c) return null;
    const row = table.active.row + (el.datos.cabecera !== false ? 2 : 1);
    return { name: `${colName(table.active.col)}${row}`, raw: f.celdas?.[c.id] ?? '' };
  }

  // ------------------------------------------------------------ tablero: pintar

  function paintBoard(body, el) {
    const d = el.datos;
    body.replaceChildren();
    const head = document.createElement('div');
    head.className = 'cv-card-head cv-grip';
    const title = document.createElement('div');
    title.className = 'cv-card-title';
    title.textContent = d.titulo || 'Tablero';
    head.append(title);
    const board = document.createElement('div');
    board.className = 'cv-board cv-scroll';
    for (const c of d.columnas) {
      const col = document.createElement('section');
      col.className = 'cv-col';
      col.dataset.col = c.id;
      const stats = columnStats(d, c.id);
      col.classList.toggle('is-over', stats.over);
      const colHead = document.createElement('header');
      colHead.className = 'cv-col-head';
      const name = document.createElement('span');
      name.className = 'cv-col-title';
      name.textContent = c.titulo;
      const meta = document.createElement('span');
      meta.className = 'cv-col-meta';
      const pts = stats.points ? ` · ${stats.points} pts` : '';
      meta.textContent = stats.limit ? `${stats.count}/${stats.limit}${pts}` : `${stats.count}${pts}`;
      if (stats.over) meta.title = `Se pasa del límite de ${stats.limit}`;
      const add = document.createElement('button');
      add.type = 'button';
      add.className = 'cv-col-add';
      add.textContent = '+';
      add.setAttribute('aria-label', `Añadir tarjeta a ${c.titulo}`);
      add.title = 'Añadir tarjeta';
      const more = document.createElement('button');
      more.type = 'button';
      more.className = 'cv-col-more';
      more.textContent = '⋯';
      more.setAttribute('aria-haspopup', 'menu');
      more.setAttribute('aria-label', `Opciones de la columna ${c.titulo}`);
      more.title = 'Opciones de la columna';
      colHead.append(name, meta, add, more);
      const list = document.createElement('div');
      list.className = 'cv-col-cards';
      for (const t of cardsIn(d, c.id)) {
        const item = document.createElement('article');
        item.className = 'cv-tcard';
        item.dataset.card = t.id;
        item.tabIndex = -1;
        const text = document.createElement('div');
        text.className = 'cv-tcard-title';
        text.textContent = t.titulo || 'Sin título';
        item.append(text);
        const facts = [t.puntos != null ? `${t.puntos} pts` : '', t.responsable, t.etiqueta].filter(Boolean);
        if (facts.length) {
          const row = document.createElement('div');
          row.className = 'cv-tcard-meta';
          row.textContent = facts.join(' · ');
          item.append(row);
        }
        item.setAttribute('aria-label', `${t.titulo || 'Sin título'}${facts.length ? `, ${facts.join(', ')}` : ''}. En ${c.titulo}`);
        list.append(item);
      }
      col.append(colHead, list);
      board.append(col);
    }
    body.append(head, board);
    if (card?.id === el.id) focusCard(el.id, card.tarjeta, false);
  }

  function focusCard(id, tarjeta, focus = true) {
    const node = api.nodeOf(id)?.querySelector(`[data-card="${tarjeta}"]`);
    api.nodeOf(id)?.querySelectorAll('.cv-tcard.is-active').forEach((n) => n.classList.remove('is-active'));
    if (!node) return;
    node.classList.add('is-active');
    node.tabIndex = 0;
    if (focus) node.focus();
    card = { id, tarjeta };
  }

  const svgIcon = (paths) => `<svg width="14" height="14" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
  const CHECK = svgIcon('<path d="m4.5 10.5 3.5 3.5 7.5-8"/>');
  const TRASH = svgIcon('<path d="M4 6h12M8 6V4h4v2M6 6l1 10h6l1-10"/>');

  // Editar una tarjeta: Intro o «Guardar» la confirman, Escape la deja como estaba y salir de ella
  // también guarda. Al acabar se vuelve a pintar siempre, haya cambios o no: antes, si no se
  // cambiaba nada, se quedaba en modo edición. Una tarjeta nueva que se deja vacía desaparece, e
  // Intro en una nueva abre la siguiente debajo, para apuntar varias seguidas.
  function editCard(el, tarjeta, { isNew = false } = {}) {
    const t = el.datos.tarjetas.find((x) => x.id === tarjeta);
    const node = api.nodeOf(el.id)?.querySelector(`[data-card="${tarjeta}"]`);
    if (!t || !node) return;
    const usekey = `${el.id}/${tarjeta}`;
    api.use(usekey, true);
    holding.add(el.id);
    const form = document.createElement('form');
    form.className = 'cv-tcard-form';
    const field = (name, label, value, attrs = {}) => {
      const input = document.createElement('input');
      input.name = name;
      input.value = value ?? '';
      input.placeholder = label;
      input.setAttribute('aria-label', label);
      Object.assign(input, attrs);
      form.append(input);
      return input;
    };
    const titulo = field('titulo', 'Título', t.titulo);
    field('puntos', 'Puntos', t.puntos ?? '', { inputMode: 'decimal' });
    field('responsable', 'Responsable', t.responsable);
    field('etiqueta', 'Etiqueta', t.etiqueta);
    const bar = document.createElement('div');
    bar.className = 'cv-tcard-actions';
    const save = document.createElement('button');
    save.type = 'submit';
    save.className = 'cv-tcard-save';
    save.innerHTML = `${CHECK}<span>Guardar</span>`;
    const remove = document.createElement('button');
    remove.type = 'button';
    remove.className = 'cv-tcard-remove';
    remove.innerHTML = TRASH;
    remove.title = 'Borrar la tarjeta';
    remove.setAttribute('aria-label', 'Borrar la tarjeta');
    bar.append(save, remove);
    form.append(bar);

    const read = () => {
      const v = new FormData(form);
      const n = Number(String(v.get('puntos')).replace(',', '.'));
      const puntos = String(v.get('puntos')).trim() === '' || Number.isNaN(n) ? null : n;
      return { titulo: String(v.get('titulo')).trim(), puntos, responsable: String(v.get('responsable')).trim(), etiqueta: String(v.get('etiqueta')).trim() };
    };
    let done = false;
    // `mode`: save (Intro o Guardar), blur (salir de la tarjeta), cancel (Escape) o remove.
    const finish = (mode, { next = false } = {}) => {
      if (done) return;
      done = true;
      holding.delete(el.id);
      api.use(usekey, false);
      const v = read();
      const empty = !v.titulo && v.puntos == null && !v.responsable && !v.etiqueta;
      const changed = v.titulo !== (t.titulo ?? '') || v.puntos !== (t.puntos ?? null) || v.responsable !== (t.responsable ?? '') || v.etiqueta !== (t.etiqueta ?? '');
      if (mode === 'remove' || (isNew && (mode === 'cancel' || empty))) {
        if (card?.tarjeta === tarjeta) card = null;
        api.apply([{ op: 'tarjeta-borrar', id: el.id, tarjeta }], { record: !isNew }).finally(() => api.repaint(el.id));
        if (mode === 'remove') api.status('Tarjeta borrada. Ctrl+Z la devuelve.');
        api.focusEl(el.id);
        return;
      }
      if (mode !== 'cancel' && changed) {
        api.apply([{ op: 'tarjeta-cambiar', id: el.id, tarjeta, ...v }]).finally(() => {
          api.repaint(el.id);
          if (next) addCard(api.get(el.id), t.col);
          else if (mode === 'save') focusCard(el.id, tarjeta);
        });
        return;
      }
      api.repaint(el.id);
      if (mode !== 'blur') focusCard(el.id, tarjeta);
    };
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      finish('save');
    });
    remove.addEventListener('click', () => finish('remove'));
    form.addEventListener('keydown', (event) => {
      event.stopPropagation();
      if (event.key === 'Enter') {
        event.preventDefault();
        finish('save', { next: isNew && Boolean(read().titulo) && !event.shiftKey });
      } else if (event.key === 'Escape') {
        event.preventDefault();
        finish('cancel');
      }
    });
    form.addEventListener('focusout', (event) => {
      if (!form.contains(event.relatedTarget)) finish('blur');
    });
    node.replaceChildren(form);
    node.classList.add('is-editing');
    titulo.focus();
    titulo.select();
  }

  async function addCard(el, col) {
    if (!el) return;
    if (el.datos.tarjetas.length >= 500) return api.status('El tablero ya tiene 500 tarjetas, el tope.');
    const tarjeta = newPart('t');
    await api.apply([{ op: 'tarjeta-nueva', id: el.id, tarjeta, col, titulo: '' }]);
    editCard(api.get(el.id), tarjeta, { isNew: true });
    return undefined;
  }

  // ------------------------------------------------------------ tablero: columnas

  let colMenu = null;
  function closeColumnMenu(refocus = false) {
    if (!colMenu) return;
    const { menu, button, id } = colMenu;
    colMenu = null;
    menu.remove();
    holding.delete(id);
    if (refocus && button.isConnected) button.focus();
  }

  function openColumnMenu(el, colId, button) {
    closeColumnMenu();
    const cols = el.datos.columnas;
    const at = cols.findIndex((c) => c.id === colId);
    const col = cols[at];
    if (!col) return;
    const menu = document.createElement('div');
    menu.className = 'cv-col-menu';
    menu.setAttribute('role', 'menu');
    menu.setAttribute('aria-label', `Columna ${col.titulo}`);
    holding.add(el.id);
    colMenu = { menu, button, id: el.id };
    const item = (label, handler, disabled = false) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'cv-col-menu-item';
      b.setAttribute('role', 'menuitem');
      b.textContent = label;
      b.disabled = disabled;
      b.addEventListener('click', (event) => {
        event.stopPropagation();
        handler();
      });
      menu.append(b);
    };
    const order = (to) => {
      const ids = cols.map((c) => c.id);
      ids.splice(to, 0, ids.splice(at, 1)[0]);
      closeColumnMenu();
      api.apply([{ op: 'columnas-orden', id: el.id, orden: ids }]);
    };
    item('Renombrar', () => {
      const name = button.closest('.cv-col').querySelector('.cv-col-title');
      closeColumnMenu();
      api.editInline(name, col.titulo, (value) => api.apply([{ op: 'columna-cambiar', id: el.id, col: colId, titulo: value }]));
    });
    item(col.limite ? `Límite de tarjetas: ${col.limite}` : 'Poner un límite de tarjetas', () => {
      menu.replaceChildren();
      const input = document.createElement('input');
      input.type = 'number';
      input.min = '0';
      input.className = 'cv-col-limit';
      input.value = col.limite ?? '';
      input.placeholder = 'Sin límite';
      input.setAttribute('aria-label', `Límite de tarjetas de ${col.titulo} (vacío: sin límite)`);
      menu.append(input);
      input.focus();
      const set = () => {
        const n = Number.parseInt(input.value, 10);
        const limite = Number.isFinite(n) && n > 0 ? n : null;
        closeColumnMenu(true);
        if (limite !== (col.limite ?? null)) api.apply([{ op: 'columna-cambiar', id: el.id, col: colId, limite }]);
      };
      input.addEventListener('keydown', (event) => {
        event.stopPropagation();
        // Sin preventDefault, la misma pulsación activaría el botón «⋯», que recibe el foco, y
        // volvería a abrir el menú.
        if (event.key === 'Enter') {
          event.preventDefault();
          set();
        } else if (event.key === 'Escape') {
          event.preventDefault();
          closeColumnMenu(true);
        }
      });
      input.addEventListener('blur', () => colMenu?.menu === menu && set());
    });
    item('Mover a la izquierda', () => order(at - 1), at === 0);
    item('Mover a la derecha', () => order(at + 1), at === cols.length - 1);
    item('Borrar la columna', async () => {
      closeColumnMenu();
      if (cols.length <= 1) return api.status('Tiene que quedar al menos una columna.');
      const n = cardsIn(el.datos, colId).length;
      if (n && !(await api.ask(`¿Borrar la columna «${col.titulo}» con sus ${n === 1 ? 'tarjeta' : `${n} tarjetas`}? Ctrl+Z la devuelve.`, { title: 'Borrar columna', kind: 'warning', okLabel: 'Borrar', cancelLabel: 'Cancelar' }))) return undefined;
      return api.apply([{ op: 'columna-borrar', id: el.id, col: colId }, { op: 'cambiar', id: el.id, w: Math.max(264, el.w - 234) }]);
    });
    menu.addEventListener('keydown', (event) => {
      event.stopPropagation();
      const list = [...menu.querySelectorAll('.cv-col-menu-item:not(:disabled)')];
      const i = list.indexOf(document.activeElement);
      if (event.key === 'ArrowDown') list[(i + 1) % list.length]?.focus();
      else if (event.key === 'ArrowUp') list[(i - 1 + list.length) % list.length]?.focus();
      else if (event.key === 'Escape') closeColumnMenu(true);
      else return;
      event.preventDefault();
    });
    button.closest('.cv-col-head').append(menu);
    menu.querySelector('.cv-col-menu-item:not(:disabled)')?.focus();
  }
  document.addEventListener('pointerdown', (event) => {
    if (colMenu && !colMenu.menu.contains(event.target)) closeColumnMenu();
  }, true);

  function dragCard(event, el, node) {
    const tarjeta = node.dataset.card;
    const start = { x: event.clientX, y: event.clientY };
    const usekey = `${el.id}/${tarjeta}`;
    let dragging = false;
    const move = (e) => {
      const scale = api.scale();
      if (!dragging && Math.hypot(e.clientX - start.x, e.clientY - start.y) < 4) return;
      if (!dragging) {
        dragging = true;
        api.use(usekey, true);
        holding.add(el.id);
        node.classList.add('is-dragging');
      }
      node.style.translate = `${(e.clientX - start.x) / scale}px ${(e.clientY - start.y) / scale}px`;
    };
    const up = (e) => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', up);
      if (!dragging) return;
      holding.delete(el.id);
      node.classList.remove('is-dragging');
      node.style.translate = '';
      api.use(usekey, false);
      const nodeEl = api.nodeOf(el.id);
      const cols = [...(nodeEl?.querySelectorAll('.cv-col') ?? [])];
      const target = cols.find((c) => {
        const r = c.getBoundingClientRect();
        return e.clientX >= r.left && e.clientX <= r.right;
      });
      if (!target) return api.repaint(el.id);
      const centers = [...target.querySelectorAll('.cv-tcard')]
        .filter((n) => n !== node)
        .map((n) => {
          const r = n.getBoundingClientRect();
          return r.top + r.height / 2;
        });
      const col = target.dataset.col;
      return api.apply([{ op: 'tarjeta-mover', id: el.id, tarjeta, col, pos: dropIndex(centers, e.clientY) }]);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', up);
  }

  api.surface.addEventListener('pointerdown', (event) => {
    if (event.button !== 0) return;
    const add = event.target.closest?.('.cv-col-add, .cv-col-more, .cv-col-menu');
    if (add) {
      event.stopPropagation();
      return;
    }
    const node = event.target.closest?.('.cv-tcard');
    if (!node || event.target.closest('input, form')) return;
    const id = node.closest('.cv-el').dataset.id;
    event.stopPropagation();
    event.preventDefault();
    if (!api.isSelected(id)) api.select([id], { focus: false });
    focusCard(id, node.dataset.card);
    dragCard(event, api.get(id), node);
  }, true);

  api.surface.addEventListener('click', (event) => {
    const more = event.target.closest?.('.cv-col-more');
    if (more) {
      event.stopPropagation();
      if (colMenu?.button === more) return closeColumnMenu(true);
      return openColumnMenu(api.get(more.closest('.cv-el').dataset.id), more.closest('.cv-col').dataset.col, more);
    }
    const add = event.target.closest?.('.cv-col-add');
    if (add) {
      event.stopPropagation();
      addCard(api.get(add.closest('.cv-el').dataset.id), add.closest('.cv-col').dataset.col);
    }
  });

  api.surface.addEventListener('dblclick', (event) => {
    const node = event.target.closest?.('.cv-tcard');
    if (node && !event.target.closest('form') && !node.classList.contains('is-editing')) {
      event.stopPropagation();
      editCard(api.get(node.closest('.cv-el').dataset.id), node.dataset.card);
      return;
    }
    const name = event.target.closest?.('.cv-col-title');
    if (name) {
      event.stopPropagation();
      const el = api.get(name.closest('.cv-el').dataset.id);
      const col = name.closest('.cv-col').dataset.col;
      api.editInline(name, name.textContent, (value) => api.apply([{ op: 'columna-cambiar', id: el.id, col, titulo: value }]));
    }
  });

  function boardKey(event, el) {
    if (!card || card.id !== el.id || event.target.closest?.('form')) return false;
    const mod = event.ctrlKey || event.metaKey;
    if (mod && event.key.startsWith('Arrow')) {
      const to = moveWithKey(el.datos, card.tarjeta, event.key);
      if (to) {
        api.apply([{ op: 'tarjeta-mover', id: el.id, tarjeta: card.tarjeta, ...to }]);
        const name = el.datos.columnas.find((c) => c.id === to.col)?.titulo;
        api.status(`Tarjeta en ${name}, puesto ${to.pos + 1}`);
      }
      return true;
    }
    if (mod) return false;
    const cards = el.datos.tarjetas;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      const here = cardsIn(el.datos, cards.find((t) => t.id === card.tarjeta)?.col);
      const i = here.findIndex((t) => t.id === card.tarjeta) + (event.key === 'ArrowDown' ? 1 : -1);
      if (here[i]) focusCard(el.id, here[i].id);
      return true;
    }
    if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
      const cur = cards.find((t) => t.id === card.tarjeta);
      const at = el.datos.columnas.findIndex((c) => c.id === cur?.col);
      const dir = event.key === 'ArrowLeft' ? -1 : 1;
      for (let i = at + dir; i >= 0 && i < el.datos.columnas.length; i += dir) {
        const first = cardsIn(el.datos, el.datos.columnas[i].id)[0];
        if (first) {
          focusCard(el.id, first.id);
          break;
        }
      }
      return true;
    }
    if (event.key === 'Enter' || event.key === 'F2') {
      editCard(el, card.tarjeta);
      return true;
    }
    if (event.key === 'Delete' || event.key === 'Backspace') {
      const tarjeta = card.tarjeta;
      card = null;
      api.apply([{ op: 'tarjeta-borrar', id: el.id, tarjeta }]);
      api.focusEl(el.id);
      return true;
    }
    if (event.key === 'Escape') {
      card = null;
      api.nodeOf(el.id)?.querySelectorAll('.cv-tcard.is-active').forEach((n) => n.classList.remove('is-active'));
      api.focusEl(el.id);
      return true;
    }
    return false;
  }

  return {
    paintOwn(body, el) {
      if (el.tipo === 'tabla') paintTable(body, el);
      else if (el.tipo === 'tablero') paintBoard(body, el);
    },
    // Una tecla dentro de una tabla o un tablero. `true`: ya está atendida.
    key(event, el) {
      if (el.tipo === 'tabla' && table?.id === el.id) return tableKey(event, el);
      if (el.tipo === 'tablero') return boardKey(event, el);
      return false;
    },
    // Enter sobre el elemento entero: se entra en él.
    enter(el) {
      if (el.tipo === 'tabla') {
        setActive(el.id, table?.id === el.id ? table.active.row : 0, table?.id === el.id ? table.active.col : 0);
        return true;
      }
      if (el.tipo === 'tablero') {
        const first = el.datos.tarjetas[0];
        if (first) focusCard(el.id, first.id);
        return Boolean(first);
      }
      return false;
    },
    tableAction,
    activeCell,
    addColumn: (el) => api.apply([{ op: 'columna-nueva', id: el.id, titulo: 'Columna' }, { op: 'cambiar', id: el.id, w: el.w + 242 }]),
    forget(id) {
      if (table?.id === id) table = null;
      if (card?.id === id) card = null;
    },
    // ¿Se está trabajando dentro de este elemento? Entonces no se repinta hasta que se acabe.
    holds: (id) => holding.has(id),
    addCard: (el, col) => addCard(el, col),
    // Tras repintarlo, el foco vuelve a donde estaba: la celda o la tarjeta.
    refocus(el) {
      if (table?.id === el.id) {
        tableNode(el.id)?.querySelector('td.is-active')?.focus({ preventScroll: true });
        return true;
      }
      if (card?.id === el.id) {
        focusCard(el.id, card.tarjeta);
        return true;
      }
      return false;
    },
    finish: () => finishEdit(true),
  };
}
