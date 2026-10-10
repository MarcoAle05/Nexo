// Editor de notas por bloques, al estilo de Notion, que guarda Markdown.
//
// Cada línea del Markdown es un bloque: párrafo, título, casilla, viñeta, número, cita o
// separador (el código y las tablas ocupan varias líneas). Se escribe como en un bloc de
// notas: Intro abre la línea siguiente (y continúa la lista), Retroceso al principio la
// une con la anterior y las flechas pasan de una línea a otra. Las líneas vacías muestran
// un «+» (y escribir «/» al principio de una línea hace lo mismo) para elegir el tipo de
// bloque. También valen los atajos de Markdown al empezar una línea: «- », «[] », «1. »,
// «# », «> », «---» y «```».
//
// La línea con el cursor se ve como Markdown; las demás, con formato (negrita, enlaces,
// [[enlaces]] de Obsidian, fechas 📅). Las líneas que no se tocan se guardan exactamente
// como estaban.

// ------------------------------------------------------------------ Markdown ↔ bloques

const FENCE = /^(\s{0,3})(`{3,}|~{3,})(.*)$/;
const HR = /^\s{0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const HEADING = /^(#{1,6})(?:[ \t]+(.*))?$/;
const TODO = /^([ \t]*)([-*+])[ \t]+\[([ xX])\](?:[ \t]+(.*)|[ \t]*)$/;
const BULLET = /^([ \t]*)([-*+])(?:[ \t]+(.*))?$/;
const NUM = /^([ \t]*)(\d{1,9})([.)])(?:[ \t]+(.*))?$/;
const QUOTE = /^>[ \t]?(.*)$/;
const TABLE = /^[ \t]*\|/;

const LISTS = new Set(['todo', 'bullet', 'num']);
const MULTILINE = new Set(['code', 'table']);

let uid = 0;
const makeBlock = (type, props = {}) => ({ id: ++uid, type, text: '', indent: '', raw: null, ...props });

function parseLine(line) {
  let m;
  if (HR.test(line)) return makeBlock('hr', { raw: line });
  if ((m = HEADING.exec(line))) return makeBlock('h', { level: m[1].length, text: m[2] ?? '', raw: line });
  if ((m = TODO.exec(line))) return makeBlock('todo', { indent: m[1], marker: m[2], checked: m[3] !== ' ', text: m[4] ?? '', raw: line });
  if ((m = BULLET.exec(line))) return makeBlock('bullet', { indent: m[1], marker: m[2], text: m[3] ?? '', raw: line });
  if ((m = NUM.exec(line))) return makeBlock('num', { indent: m[1], num: Number(m[2]), delim: m[3], text: m[4] ?? '', raw: line });
  if ((m = QUOTE.exec(line))) return makeBlock('quote', { text: m[1], raw: line });
  return makeBlock('p', { text: line, raw: line });
}

export function parseMarkdown(markdown) {
  const lines = (markdown ?? '').replace(/\r\n?/g, '\n').split('\n');
  if (lines.at(-1) === '') lines.pop(); // el salto de línea final no es una línea más
  const blocks = [];
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const fence = FENCE.exec(line);
    if (fence) {
      const mark = fence[2];
      const close = new RegExp(`^\\s{0,3}${mark[0] === '`' ? '`' : '~'}{${mark.length},}\\s*$`);
      let j = i + 1;
      while (j < lines.length && !close.test(lines[j])) j++;
      const end = Math.min(j, lines.length - 1);
      blocks.push(makeBlock('code', {
        fence: mark,
        lang: fence[3].trim(),
        text: lines.slice(i + 1, j).join('\n'),
        raw: lines.slice(i, end + 1).join('\n'),
      }));
      i = end;
      continue;
    }
    if (TABLE.test(line)) {
      let j = i;
      while (j + 1 < lines.length && TABLE.test(lines[j + 1])) j++;
      const raw = lines.slice(i, j + 1).join('\n');
      blocks.push(makeBlock('table', { text: raw, raw }));
      i = j;
      continue;
    }
    blocks.push(parseLine(line));
  }
  return blocks;
}

function lineOf(b) {
  if (b.raw != null) return b.raw;
  switch (b.type) {
    case 'h':
      return `${'#'.repeat(b.level || 1)} ${b.text}`;
    case 'todo':
      return `${b.indent}${b.marker || '-'} [${b.checked ? 'x' : ' '}] ${b.text}`;
    case 'bullet':
      return `${b.indent}${b.marker || '-'} ${b.text}`;
    case 'num':
      return `${b.indent}${b.num || 1}${b.delim || '.'} ${b.text}`;
    case 'quote':
      return `> ${b.text}`;
    case 'hr':
      return '---';
    case 'code': {
      const fence = b.fence || '```';
      return `${fence}${b.lang || ''}\n${b.text ? `${b.text}\n` : ''}${fence}`;
    }
    default:
      return b.text;
  }
}

export function serializeBlocks(blocks) {
  return blocks
    .filter((b) => !b.ghost)
    .map(lineOf)
    .join('\n');
}

// Casillas de una nota: [hechas, total].
export function taskCount(markdown) {
  const tasks = parseMarkdown(markdown).filter((b) => b.type === 'todo');
  return [tasks.filter((b) => b.checked).length, tasks.length];
}

// ------------------------------------------------------------------ fechas

const pad = (n) => String(n).padStart(2, '0');
export const isoToday = (offset = 0) => {
  const d = new Date();
  d.setDate(d.getDate() + offset);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};
export function parseDate(text) {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(text ?? '');
  return m ? new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3])) : null;
}
export function relativeDay(date) {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const days = Math.round((date - today) / 86_400_000);
  if (days === 0) return 'hoy';
  if (days === 1) return 'mañana';
  if (days === -1) return 'ayer';
  return days > 0 ? `en ${days} días` : `hace ${-days} días`;
}
const dayDiff = (date) => {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  return Math.round((date - today) / 86_400_000);
};

// ------------------------------------------------------------------ formato en línea

// Cada marca se prueba en la posición actual; la primera que encaja gana.
const MARKS = [
  { kind: 'code', first: '`', re: /`([^`\n]+?)`/y },
  { kind: 'wiki', first: '[', re: /\[\[([^\]|\n]+?)(?:\|([^\]\n]+?))?\]\]/y },
  { kind: 'link', first: '[', re: /\[([^\]\n]+)\]\(([^)\s]+)(?:\s+"[^"\n]*")?\)/y },
  { kind: 'strong', first: '*_', re: /(\*\*|__)(?=\S)(.+?)(?<=\S)\1/y },
  { kind: 's', first: '~', re: /~~(?=\S)(.+?)(?<=\S)~~/y },
  { kind: 'mark', first: '=', re: /==(?=\S)(.+?)(?<=\S)==/y },
  { kind: 'em', first: '*_', re: /([*_])(?=\S)(.+?)(?<=\S)\1/y },
  { kind: 'date', first: '\uD83D', re: /📅 ?(\d{4}-\d{2}-\d{2})/uy },
  { kind: 'url', first: 'h', re: /https?:\/\/[^\s<>()]*[^\s<>().,;:!?'"*_~]/y },
];
const WORD = /[\p{L}\p{N}]/u;
const RICH = /[`[*_~=]|📅|https?:\/\//u;
const SAFE_URL = /^(https?:|mailto:)/i;

function textNode(text, src) {
  const node = document.createTextNode(text);
  node.nexoSrc = src; // posición en el Markdown de la línea: para colocar el cursor al hacer clic
  return node;
}

function anchor(href, target) {
  const a = document.createElement('a');
  if (target != null) {
    a.className = 'wikilink';
    a.href = '#';
    a.dataset.target = target;
  } else {
    a.href = SAFE_URL.test(href) ? href : '#';
    if (!SAFE_URL.test(href)) a.dataset.target = href;
    a.title = href;
  }
  return a;
}

function inline(text, base, out, done) {
  let i = 0;
  let plain = 0;
  const flush = (end) => {
    if (end > plain) out.append(textNode(text.slice(plain, end), base + plain));
  };
  while (i < text.length) {
    let hit = null;
    for (const mark of MARKS) {
      if (!mark.first.includes(text[i])) continue;
      mark.re.lastIndex = i;
      const m = mark.re.exec(text);
      if (!m) continue;
      // «_» y «__» no cuentan dentro de palabras (snake_case).
      if ((mark.kind === 'em' || mark.kind === 'strong') && m[1][0] === '_') {
        if (WORD.test(text[i - 1] ?? '') || WORD.test(text[i + m[0].length] ?? '')) continue;
      }
      hit = [mark.kind, m];
      break;
    }
    if (!hit) {
      i += 1;
      continue;
    }
    flush(i);
    const [kind, m] = hit;
    const at = base + i;
    let el;
    if (kind === 'code') {
      el = document.createElement('code');
      el.append(textNode(m[1], at + 1));
    } else if (kind === 'wiki') {
      el = anchor(null, m[1].split(/[#^]/)[0].trim());
      el.append(m[2] != null ? textNode(m[2], at + 3 + m[1].length) : textNode(m[1], at + 2));
    } else if (kind === 'link') {
      el = anchor(m[2]);
      el.append(textNode(m[1], at + 1));
    } else if (kind === 'url') {
      el = anchor(m[0]);
      el.append(textNode(m[0], at));
    } else if (kind === 'date') {
      el = document.createElement('span');
      el.className = 'ne-date';
      const date = parseDate(m[1]);
      if (date) {
        const diff = dayDiff(date);
        if (!done && diff < 0) el.classList.add('overdue');
        if (!done && diff === 0) el.classList.add('today');
        el.textContent = `${date.toLocaleDateString('es', { day: 'numeric', month: 'short' })} · ${relativeDay(date)}`;
      } else el.textContent = m[1];
      el.title = m[1];
      el.dataset.srcEnd = String(at + m[0].length);
    } else {
      el = document.createElement(kind);
      const lead = kind === 'em' || kind === 'strong' ? m[1].length : 2;
      inline(kind === 'em' || kind === 'strong' ? m[2] : m[1], at + lead, el, done);
    }
    out.append(el);
    i += m[0].length;
    plain = i;
  }
  flush(text.length);
}

// Posición en el Markdown de la línea para un punto del texto ya formateado.
function rawOffsetAt(el, node, offset) {
  if (node.nodeType === 3 && node.nexoSrc != null) return node.nexoSrc + Math.min(offset, node.length);
  const holder = (node.nodeType === 3 ? node.parentElement : node)?.closest?.('[data-src-end]');
  if (holder && el.contains(holder)) return Number(holder.dataset.srcEnd);
  if (node.nodeType === 1) {
    // Entre nodos: el final del texto anterior al punto.
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    let last = null;
    const limit = node.childNodes[offset] ?? null;
    for (let t = walker.nextNode(); t; t = walker.nextNode()) {
      if (limit && (limit === t || limit.compareDocumentPosition(t) & Node.DOCUMENT_POSITION_FOLLOWING)) break;
      if (t.nexoSrc != null) last = t;
    }
    if (last) return last.nexoSrc + last.length;
    return 0;
  }
  return null;
}

// ------------------------------------------------------------------ menú de bloques

const svg = (paths) =>
  `<svg width="16" height="16" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;

const OPTIONS = [
  { type: 'p', label: 'Texto', hint: '', words: 'texto parrafo normal', icon: '<path d="M4 5h12M10 5v11"/>' },
  { type: 'todo', label: 'Casilla', hint: '[ ]', words: 'casilla tarea pendiente lista marcar checklist todo', icon: '<rect x="3.5" y="3.5" width="13" height="13" rx="3"/><path d="m7 10 2.2 2.2L13.5 8"/>' },
  { type: 'bullet', label: 'Lista con viñetas', hint: '-', words: 'lista vinetas puntos', icon: '<circle cx="5" cy="6" r="1" fill="currentColor"/><circle cx="5" cy="14" r="1" fill="currentColor"/><path d="M9 6h8M9 14h8"/>' },
  { type: 'num', label: 'Lista numerada', hint: '1.', words: 'lista numerada numeros pasos', icon: '<path d="M4 4.5h1.5V9M4 9h3M4 12.5h2.6L4 16h3M10 6h7M10 14h7"/>' },
  { type: 'h', level: 1, label: 'Título', hint: '#', words: 'titulo encabezado grande', icon: '<path d="M4 4v12M12 4v12M4 10h8M15.5 9 17 8v8"/>' },
  { type: 'h', level: 2, label: 'Subtítulo', hint: '##', words: 'subtitulo encabezado mediano', icon: '<path d="M3 4v12M10 4v12M3 10h7M13.5 9.5a1.8 1.8 0 1 1 3 1.4L13.5 16h3.5"/>' },
  { type: 'h', level: 3, label: 'Apartado', hint: '###', words: 'apartado encabezado pequeno seccion', icon: '<path d="M3 4v12M10 4v12M3 10h7M13.5 8.5h3l-1.8 2.4a1.8 1.8 0 1 1-1.6 2.7"/>' },
  { type: 'quote', label: 'Cita', hint: '>', words: 'cita nota destacada', icon: '<path d="M5 4v12M9 7h7M9 11h7M9 15h4"/>' },
  { type: 'code', label: 'Código', hint: '```', words: 'codigo programa comando', icon: '<path d="m7 6-4 4 4 4M13 6l4 4-4 4"/>' },
  { type: 'table', label: 'Tabla', hint: '|', words: 'tabla columnas filas registro', icon: '<rect x="3" y="4" width="14" height="12" rx="2"/><path d="M3 8.5h14M3 12.5h14M8.5 4v12"/>' },
  { type: 'hr', label: 'Separador', hint: '---', words: 'separador linea division', icon: '<path d="M3 10h14"/>' },
  { type: 'date', label: 'Fecha de hoy', hint: '📅', words: 'fecha hoy dia calendario vence', icon: '<rect x="3" y="4.5" width="14" height="12.5" rx="2"/><path d="M3 8.5h14M7 2.5v4M13 2.5v4"/>' },
  { type: 'date', offset: 1, label: 'Fecha de mañana', hint: '📅', words: 'fecha manana dia calendario vence', icon: '<rect x="3" y="4.5" width="14" height="12.5" rx="2"/><path d="M3 8.5h14M7 2.5v4M13 2.5v4M10 11v3"/>' },
];
const fold = (s) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();

function createMenu(onPick) {
  const el = document.createElement('div');
  el.className = 'ne-menu';
  el.setAttribute('role', 'listbox');
  el.setAttribute('aria-label', 'Tipo de bloque');
  el.hidden = true;
  document.body.append(el);
  let items = [];
  let active = 0;
  let state = null; // { block, slash }

  function render() {
    el.replaceChildren(
      ...items.map((opt, i) => {
        const b = document.createElement('button');
        b.type = 'button';
        b.className = 'ne-menu-item';
        b.setAttribute('role', 'option');
        b.setAttribute('aria-selected', String(i === active));
        b.innerHTML = `<span class="ne-menu-icon">${svg(opt.icon)}</span><span class="ne-menu-label"></span><span class="ne-menu-hint"></span>`;
        b.querySelector('.ne-menu-label').textContent = opt.label;
        b.querySelector('.ne-menu-hint').textContent = opt.hint;
        b.addEventListener('mousedown', (e) => e.preventDefault()); // el foco sigue en la línea
        b.addEventListener('click', () => pick(i));
        b.addEventListener('mousemove', () => {
          if (active !== i) {
            active = i;
            mark();
          }
        });
        return b;
      }),
    );
  }
  function mark() {
    [...el.children].forEach((b, i) => b.setAttribute('aria-selected', String(i === active)));
    el.children[active]?.scrollIntoView({ block: 'nearest' });
  }
  function pick(i) {
    const opt = items[i];
    const s = state;
    close();
    if (opt && s) onPick(s.block, opt, s.slash);
  }
  function place(anchor) {
    const r = anchor.getBoundingClientRect();
    el.hidden = false;
    const h = el.offsetHeight;
    const w = el.offsetWidth;
    const below = r.bottom + 6 + h < window.innerHeight - 8;
    el.style.top = `${below ? r.bottom + 6 : Math.max(8, r.top - h - 6)}px`;
    el.style.left = `${Math.max(8, Math.min(r.left, window.innerWidth - w - 8))}px`;
  }
  function open(block, anchor, slash = false) {
    state = { block, slash, anchor };
    items = OPTIONS;
    active = 0;
    render();
    place(anchor);
  }
  function filter(query) {
    if (!state) return;
    const q = fold(query);
    items = OPTIONS.filter((o) => fold(`${o.label} ${o.words}`).includes(q));
    if (!items.length) return close();
    active = Math.min(active, items.length - 1);
    render();
    place(state.anchor);
  }
  function close() {
    state = null;
    el.hidden = true;
  }
  // Teclas mientras el menú está abierto; devuelve si las ha usado.
  function key(e) {
    if (!state) return false;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      active = (active + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
      mark();
    } else if (e.key === 'Enter' || e.key === 'Tab') pick(active);
    else if (e.key === 'Escape') close();
    else return false;
    e.preventDefault();
    return true;
  }
  const outside = (e) => {
    if (state && !el.contains(e.target) && !e.target.closest?.('.ne-plus')) close();
  };
  document.addEventListener('mousedown', outside);
  return {
    open,
    filter,
    close,
    key,
    get block() {
      return state?.block ?? null;
    },
    destroy() {
      document.removeEventListener('mousedown', outside);
      el.remove();
    },
  };
}

// ------------------------------------------------------------------ editor

const PLACEHOLDER = {
  p: 'Escribe, o pulsa «/» para elegir un bloque',
  h: 'Título',
  todo: 'Tarea',
  bullet: 'Elemento',
  num: 'Paso',
  quote: 'Cita',
  code: 'Código',
  table: 'Tabla',
};
const PLUS = svg('<path d="M10 5v10M5 10h10" stroke-width="1.7"/>');

const depthOf = (indent) => {
  let depth = 0;
  let spaces = 0;
  for (const c of indent) {
    if (c === '\t') depth += 1;
    else if (++spaces === 2) {
      depth += 1;
      spaces = 0;
    }
  }
  return Math.min(depth, 8);
};

// Texto de un elemento editable: los <br> y <div> que pueda meter el navegador cuentan como saltos.
function readText(el) {
  let out = '';
  const walk = (node) => {
    for (const child of node.childNodes) {
      if (child.nodeType === 3) out += child.data;
      else if (child.nodeName === 'BR') out += '\n';
      else if (child.nodeType === 1) {
        if ((child.nodeName === 'DIV' || child.nodeName === 'P') && out && !out.endsWith('\n')) out += '\n';
        walk(child);
      }
    }
  };
  walk(el);
  if (out.endsWith('\n') && el.lastChild?.nodeName === 'BR') out = out.slice(0, -1);
  return out;
}

// Cursor contando los <br> como saltos de línea (para normalizar un bloque de varias líneas).
function caretWithBreaks(el) {
  const sel = getSelection();
  if (!sel?.rangeCount) return null;
  const { startContainer: node, startOffset: offset } = sel.getRangeAt(0);
  let count = 0;
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (n === node && n.nodeType === 3) return count + offset;
    if (n.parentNode === node && Array.prototype.indexOf.call(node.childNodes, n) >= offset) return count;
    if (n.nodeType === 3) count += n.length;
    else if (n.nodeName === 'BR') count += 1;
  }
  return count;
}

// Deja un bloque de varias líneas como un solo nodo de texto con «\n» (WebKit mete <br>).
function flatten(el) {
  const at = caretWithBreaks(el);
  el.textContent = readText(el);
  if (at != null) setCaret(el, at);
}

function offsets(el) {
  const sel = getSelection();
  if (!sel?.rangeCount) return null;
  const r = sel.getRangeAt(0);
  if (!el.contains(r.startContainer)) return null;
  const pre = document.createRange();
  pre.selectNodeContents(el);
  pre.setEnd(r.startContainer, r.startOffset);
  const start = pre.toString().length;
  pre.setEnd(r.endContainer, r.endOffset);
  return { start, end: pre.toString().length };
}

function setCaret(el, offset) {
  const sel = getSelection();
  const range = document.createRange();
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  let left = offset;
  let placed = false;
  for (let t = walker.nextNode(); t; t = walker.nextNode()) {
    if (left <= t.length) {
      range.setStart(t, left);
      placed = true;
      break;
    }
    left -= t.length;
  }
  if (!placed) range.selectNodeContents(el), range.collapse(false);
  else range.collapse(true);
  sel.removeAllRanges();
  sel.addRange(range);
}

export function createNoteEditor(host, { onChange, onLink, onEscape, renderTable }) {
  const root = document.createElement('div');
  root.className = 'ne';
  host.append(root);
  let blocks = [];
  const rows = new Map(); // id → fila
  const menu = createMenu(applyOption);
  const history = { past: [], future: [] };
  let typing = 0;

  const indexOf = (b) => blocks.indexOf(b);
  const blockOf = (el) => blocks.find((b) => String(b.id) === el?.closest('.ne-row')?.dataset.id) ?? null;
  const textOf = (b) => rows.get(b.id)?.querySelector('.ne-text') ?? null;
  const focusable = (b) => b && !b.hidden;

  // ---------- filas

  // Siempre hay una línea vacía al final para seguir escribiendo (el «+» se ve en ella). Si
  // nadie escribe en ella no se guarda: abrir una nota no la cambia.
  function ensureGhost() {
    const last = blocks.at(-1);
    for (const b of blocks) if (b !== last) b.ghost = false;
    if (last && last.type === 'p' && !last.text) {
      last.ghost = true;
      return false;
    }
    if (last) last.ghost = false;
    blocks.push(makeBlock('p', { ghost: true }));
    return true;
  }

  function row(b, index) {
    const el = document.createElement('div');
    el.className = 'ne-row';
    el.dataset.id = String(b.id);
    el.dataset.type = b.type;
    if (b.type === 'h') el.dataset.level = String(Math.min(b.level || 1, 3));
    if (b.ghost) el.classList.add('ne-ghost');
    if (b.type === 'todo' && b.checked) el.classList.add('done');
    const depth = LISTS.has(b.type) ? depthOf(b.indent) : 0;
    el.style.setProperty('--depth', String(depth));
    if (depth) el.dataset.depth = String(Math.min(depth, 2));

    const plus = document.createElement('button');
    plus.type = 'button';
    plus.className = 'ne-plus';
    plus.tabIndex = -1;
    plus.innerHTML = PLUS;
    plus.title = 'Añadir un bloque (o escribe «/»)';
    plus.setAttribute('aria-label', 'Añadir un bloque');
    plus.addEventListener('mousedown', (e) => e.preventDefault());
    plus.addEventListener('click', () => openMenuFor(b));
    el.append(plus);

    if (b.type === 'todo') {
      const box = document.createElement('input');
      box.type = 'checkbox';
      box.className = 'ne-check';
      box.checked = Boolean(b.checked);
      box.setAttribute('aria-label', b.checked ? 'Marcar como pendiente' : 'Marcar como hecha');
      box.addEventListener('mousedown', (e) => e.preventDefault());
      box.addEventListener('change', () => {
        snapshot();
        b.checked = box.checked;
        b.raw = null;
        refresh(b);
        changed();
      });
      el.append(box);
    } else if (b.type === 'bullet') {
      const dot = document.createElement('span');
      dot.className = 'ne-bullet';
      dot.setAttribute('aria-hidden', 'true');
      el.append(dot);
    } else if (b.type === 'num') {
      el.append(Object.assign(document.createElement('span'), { className: 'ne-num', textContent: `${b.shown ?? b.num ?? 1}.` }));
    }

    let text;
    if (b.type === 'hr') {
      text = document.createElement('div');
      text.className = 'ne-text ne-hr';
      text.tabIndex = 0;
      text.setAttribute('role', 'separator');
      text.setAttribute('aria-label', 'Separador');
    } else {
      text = document.createElement('div');
      text.className = `ne-text${b.type === 'code' ? ' ne-code' : b.type === 'table' ? ' ne-table' : ''}`;
      text.spellcheck = b.type !== 'code' && b.type !== 'table';
      text.dataset.placeholder = b.ghost && index === 0 ? 'Escribe aquí, o pulsa + para elegir un bloque' : PLACEHOLDER[b.type] ?? '';
      if (b.type === 'code') {
        const lang = document.createElement('input');
        lang.className = 'ne-lang';
        lang.value = b.lang ?? '';
        lang.placeholder = 'lenguaje';
        lang.spellcheck = false;
        lang.setAttribute('aria-label', 'Lenguaje del código');
        lang.addEventListener('input', () => {
          b.lang = lang.value.trim();
          b.raw = null;
          changed();
        });
        el.append(lang);
      }
      showView(b, text);
    }
    el.append(text);
    rows.set(b.id, el);
    return el;
  }

  // Formato en línea si la línea no tiene el foco; Markdown tal cual si lo tiene.
  function showView(b, el = textOf(b)) {
    if (!el || b.type === 'hr') return;
    const rich = b.type === 'table' ? Boolean(b.text.trim()) : RICH.test(b.text);
    el.dataset.mode = 'view';
    el.classList.toggle('ne-empty', !b.text);
    if (!rich) {
      el.removeAttribute('data-rich');
      el.contentEditable = 'plaintext-only';
      if (el.childElementCount || el.textContent !== b.text) el.textContent = b.text;
      el.removeAttribute('tabindex');
      return;
    }
    el.dataset.rich = '1';
    el.contentEditable = 'false';
    el.tabIndex = 0;
    if (b.type === 'table') {
      el.innerHTML = renderTable ? renderTable(b.text) : '';
      if (!renderTable) el.textContent = b.text;
    } else {
      el.replaceChildren();
      inline(b.text, 0, el, b.type === 'todo' && b.checked);
    }
  }

  function showRaw(b, el = textOf(b)) {
    if (!el || b.type === 'hr' || el.dataset.mode === 'raw') return;
    el.dataset.mode = 'raw';
    if (el.dataset.rich) {
      el.textContent = b.text;
      el.contentEditable = 'plaintext-only';
      el.removeAttribute('tabindex');
    }
    if (MULTILINE.has(b.type) && b.text.endsWith('\n') && el.lastChild?.nodeName !== 'BR') el.append(document.createElement('br'));
  }

  function render() {
    rows.clear();
    root.replaceChildren(...blocks.map(row));
    refreshNumbers();
  }

  // Vuelve a pintar una fila (cambió su tipo o su estado) conservando el foco si lo tenía.
  function refresh(b) {
    const old = rows.get(b.id);
    if (!old) return;
    const had = old.contains(document.activeElement);
    const at = had ? offsets(textOf(b))?.start : null;
    const el = row(b, indexOf(b));
    old.replaceWith(el);
    if (had) focusBlock(b, at ?? 'end');
  }

  // Las listas numeradas se ven seguidas (1, 2, 3…) aunque el Markdown repita números,
  // como en cualquier visor de Markdown. Una viñeta del mismo nivel corta la cuenta.
  function refreshNumbers() {
    const counters = [];
    for (const b of blocks) {
      const d = depthOf(b.indent);
      if (b.type === 'num') {
        counters.length = d + 1;
        counters[d] = counters[d] == null ? b.num || 1 : counters[d] + 1;
        b.shown = counters[d];
        const n = rows.get(b.id)?.querySelector('.ne-num');
        if (n) n.textContent = `${b.shown}.`;
      } else counters.length = LISTS.has(b.type) ? d : 0;
    }
  }

  function insertAfter(ref, b) {
    blocks.splice(indexOf(ref) + 1, 0, b);
    rows.get(ref.id).after(row(b, indexOf(b)));
  }

  function insertBefore(ref, b) {
    blocks.splice(indexOf(ref), 0, b);
    rows.get(ref.id).before(row(b, indexOf(b)));
  }

  function remove(b) {
    blocks.splice(indexOf(b), 1);
    rows.get(b.id)?.remove();
    rows.delete(b.id);
  }

  function syncGhost() {
    if (ensureGhost()) root.append(row(blocks.at(-1), blocks.length - 1));
    for (const b of blocks) rows.get(b.id)?.classList.toggle('ne-ghost', Boolean(b.ghost));
  }

  // ---------- foco y cursor

  function focusBlock(b, offset = 'end') {
    const el = textOf(b);
    if (!el) return;
    if (b.type !== 'hr') showRaw(b, el);
    el.focus({ preventScroll: true });
    if (b.type !== 'hr') setCaret(el, offset === 'end' ? b.text.length : Math.max(0, Math.min(offset, b.text.length)));
    rows.get(b.id)?.scrollIntoView({ block: 'nearest' });
  }

  // Coloca el cursor en la línea `b` a la altura horizontal `x` (al subir o bajar con las flechas).
  function focusAtX(b, x, fromBelow) {
    const el = textOf(b);
    if (!el) return;
    if (b.type === 'hr') return el.focus();
    showRaw(b, el);
    el.focus({ preventScroll: true });
    const rect = el.getBoundingClientRect();
    const line = parseFloat(getComputedStyle(el).lineHeight) || 20;
    const y = fromBelow ? rect.bottom - line / 2 : rect.top + line / 2;
    const pos = document.caretRangeFromPoint?.(x, y);
    if (pos && el.contains(pos.startContainer)) {
      const sel = getSelection();
      sel.removeAllRanges();
      sel.addRange(pos);
    } else setCaret(el, fromBelow ? b.text.length : 0);
    rows.get(b.id)?.scrollIntoView({ block: 'nearest' });
  }

  function caretRect() {
    const sel = getSelection();
    if (!sel?.rangeCount) return null;
    const r = sel.getRangeAt(0).cloneRange();
    r.collapse(true);
    return r.getClientRects()[0] ?? null;
  }

  function onEdge(el, top) {
    const c = caretRect();
    if (!c) return true;
    const rect = el.getBoundingClientRect();
    const line = parseFloat(getComputedStyle(el).lineHeight) || 20;
    return top ? c.top - rect.top < line * 0.75 : rect.bottom - c.bottom < line * 0.75;
  }

  const prevOf = (b) => blocks[indexOf(b) - 1] ?? null;
  const nextOf = (b) => blocks[indexOf(b) + 1] ?? null;

  // ---------- historial (Ctrl+Z / Ctrl+Mayús+Z)

  function current() {
    const el = document.activeElement?.closest?.('.ne-text');
    const b = el && root.contains(el) ? blockOf(el) : null;
    return { index: b ? indexOf(b) : -1, offset: b && el ? offsets(el)?.start ?? 0 : 0 };
  }
  const state = () => ({ md: serializeBlocks(blocks), focus: current() });

  function snapshot() {
    history.past.push(state());
    if (history.past.length > 200) history.past.shift();
    history.future = [];
  }
  // Escribir seguido cuenta como un solo paso de deshacer.
  function typingSnapshot() {
    if (!typing) snapshot();
    clearTimeout(typing);
    typing = setTimeout(() => (typing = 0), 800);
  }
  function restore(s) {
    blocks = parseMarkdown(s.md);
    ensureGhost();
    render();
    const b = blocks[Math.max(0, Math.min(s.focus.index, blocks.length - 1))];
    if (s.focus.index >= 0 && b) focusBlock(b, s.focus.offset);
    changed();
  }
  function undo() {
    clearTimeout(typing);
    typing = 0;
    const s = history.past.pop();
    if (!s) return;
    history.future.push(state());
    restore(s);
  }
  function redo() {
    const s = history.future.pop();
    if (!s) return;
    history.past.push(state());
    restore(s);
  }

  // ---------- cambios

  function changed() {
    onChange?.(serializeBlocks(blocks));
  }

  function setType(b, type, level) {
    const wasList = LISTS.has(b.type);
    b.type = type;
    b.raw = null;
    if (type === 'h') b.level = level || 1;
    if (type === 'todo') {
      b.checked = false;
      b.marker = b.marker || '-';
    }
    if (type === 'bullet') b.marker = b.marker || '-';
    if (type === 'num') {
      b.num = 1;
      b.delim = '.';
    }
    if (!LISTS.has(type) || !wasList) b.indent = LISTS.has(type) ? b.indent ?? '' : '';
    if (type === 'code') {
      b.fence = '```';
      b.lang = b.lang ?? '';
    }
  }

  // Lo que sigue a una línea al pulsar Intro: la lista continúa, el título pasa a texto.
  function continuation(b, text) {
    if (b.type === 'todo') return makeBlock('todo', { indent: b.indent, marker: b.marker, checked: false, text });
    if (b.type === 'bullet') return makeBlock('bullet', { indent: b.indent, marker: b.marker, text });
    if (b.type === 'num') return makeBlock('num', { indent: b.indent, num: (b.shown || b.num || 1) + 1, delim: b.delim, text });
    if (b.type === 'quote') return makeBlock('quote', { text });
    return makeBlock('p', { text });
  }

  // Atajos de Markdown al empezar una línea de texto.
  function shortcut(b, el) {
    if (b.type !== 'p' && b.type !== 'bullet') return false;
    const t = b.text;
    let m;
    let apply = null;
    if ((m = /^(?:[-*+] )?\[([ xX]?)\] /.exec(t))) apply = () => (setType(b, 'todo'), (b.checked = /x/i.test(m[1])), m[0].length);
    else if (b.type !== 'p') return false;
    else if ((m = /^([-*+]) /.exec(t))) apply = () => (setType(b, 'bullet'), (b.marker = m[1]), 2);
    else if ((m = /^(\d{1,3})([.)]) /.exec(t))) apply = () => (setType(b, 'num'), (b.num = Number(m[1])), (b.delim = m[2]), m[0].length);
    else if ((m = /^(#{1,3}) /.exec(t))) apply = () => (setType(b, 'h', m[1].length), m[0].length);
    else if (/^> /.test(t)) apply = () => (setType(b, 'quote'), 2);
    else if (/^(---|\*\*\*|___)$/.test(t)) {
      snapshot();
      b.text = '';
      setType(b, 'hr');
      refresh(b);
      let next = nextOf(b);
      if (!next || next.type !== 'p' || next.text) {
        next = makeBlock('p');
        insertAfter(b, next);
      }
      syncGhost();
      focusBlock(next, 0);
      return true;
    }
    if (!apply) return false;
    snapshot(); // deshacer devuelve el texto tal cual se escribió («- », «1. »…)
    const caret = offsets(el)?.start ?? t.length;
    const cut = apply();
    b.text = t.slice(cut);
    b.raw = null;
    refresh(b);
    focusBlock(b, Math.max(0, caret - cut));
    refreshNumbers();
    return true;
  }

  function applyOption(b, opt, slash) {
    if (!blocks.includes(b)) return;
    snapshot();
    if (slash) {
      const el = textOf(b);
      b.text = (el ? readText(el) : b.text).replace(/^\/\S*/, '');
      b.raw = null;
    }
    // Desde el «+» de una línea con texto, el bloque nuevo va debajo.
    if (b.text.trim() && opt.type !== 'date' && !slash) {
      const fresh = makeBlock('p');
      insertAfter(b, fresh);
      b = fresh;
    }
    if (b.ghost) b.ghost = false;
    if (opt.type === 'date') {
      b.text = `${b.text.replace(/\s+$/, '')}${b.text.trim() ? ' ' : ''}📅 ${isoToday(opt.offset ?? 0)} `;
      b.raw = null;
      refresh(b);
      syncGhost();
      focusBlock(b, 'end');
      return changed();
    }
    if (opt.type === 'table') {
      setType(b, 'table');
      b.text = '| Columna | Columna |\n| --- | --- |\n|  |  |';
    } else setType(b, opt.type, opt.level);
    refresh(b);
    if (opt.type === 'hr') {
      let next = nextOf(b);
      if (!next || next.type !== 'p' || next.text) {
        next = makeBlock('p');
        insertAfter(b, next);
      }
      syncGhost();
      focusBlock(next, 0);
    } else {
      syncGhost();
      focusBlock(b, opt.type === 'table' ? 2 : 'end');
    }
    refreshNumbers();
    changed();
  }

  function openMenuFor(b) {
    const el = textOf(b);
    if (b.type !== 'hr' && el && !el.contains(document.activeElement)) focusBlock(b, 'end');
    menu.open(b, rows.get(b.id)?.querySelector('.ne-text') ?? rows.get(b.id), false);
  }

  // Inserta texto con varias líneas: cada línea es un bloque (se interpreta como Markdown).
  function insertLines(b, el, text) {
    snapshot();
    const { start, end } = offsets(el) ?? { start: b.text.length, end: b.text.length };
    const current = readText(el);
    const before = current.slice(0, start);
    const after = current.slice(end);
    const lines = text.replace(/\r\n?/g, '\n').replace(/\n$/, '').split('\n');
    const fresh = parseMarkdown((b.type === 'p' ? before : '') + lines.join('\n'));
    if (!fresh.length) fresh.push(makeBlock('p'));
    let last;
    if (b.type === 'p') {
      // La línea actual se sustituye por lo pegado (su principio ya va dentro).
      const at = indexOf(b);
      remove(b);
      blocks.splice(at, 0, ...fresh);
      render();
      last = fresh.at(-1);
    } else {
      b.text = before + lines[0];
      b.raw = null;
      refresh(b);
      let ref = b;
      for (const nb of parseMarkdown(lines.slice(1).join('\n'))) {
        insertAfter(ref, nb);
        ref = nb;
      }
      last = ref;
    }
    const caret = last.text.length;
    last.text += after;
    last.raw = null;
    if (after) refresh(last);
    syncGhost();
    focusBlock(last, caret);
    refreshNumbers();
    changed();
  }

  // Salto de línea dentro de un bloque de código o tabla. Se hace a mano: el «\n» que inserta
  // WebKit en un contenteditable de texto plano acaba como <br> mal colocados. Si el texto
  // termina en salto, un <br> final deja ver (y poner el cursor en) la última línea vacía.
  function setMultiline(b, el, text, caret) {
    b.text = text;
    b.raw = null;
    el.textContent = text;
    if (text.endsWith('\n')) el.append(document.createElement('br'));
    setCaret(el, caret);
    el.classList.toggle('ne-empty', !text);
  }

  // ---------- teclado

  function onKey(e) {
    const el = e.target.closest?.('.ne-text');
    if (!el || !root.contains(el)) return;
    const b = blockOf(el);
    if (!b) return;
    if (menu.block === b && menu.key(e)) return;
    if (e.isComposing || e.keyCode === 229) return;
    const mod = e.ctrlKey || e.metaKey;
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;

    if (mod && k === 'z') {
      e.preventDefault();
      return e.shiftKey ? redo() : undo();
    }
    if (mod && k === 'y') {
      e.preventDefault();
      return redo();
    }
    if (mod && e.key === 'Enter' && b.type === 'todo') {
      e.preventDefault();
      snapshot();
      b.checked = !b.checked;
      b.raw = null;
      refresh(b);
      return changed();
    }
    if (mod && (k === 'b' || k === 'i') && b.type !== 'code' && b.type !== 'table' && b.type !== 'hr') {
      e.preventDefault();
      const o = offsets(el);
      if (!o) return;
      const markText = k === 'b' ? '**' : '*';
      const sel = b.text.slice(o.start, o.end);
      document.execCommand('insertText', false, `${markText}${sel}${markText}`);
      if (!sel) setCaret(el, o.start + markText.length);
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      el.blur();
      return onEscape?.();
    }

    if (b.type === 'hr') {
      if (e.key === 'Backspace' || e.key === 'Delete') {
        e.preventDefault();
        snapshot();
        const target = e.key === 'Backspace' ? prevOf(b) ?? nextOf(b) : nextOf(b) ?? prevOf(b);
        remove(b);
        syncGhost();
        if (target) focusBlock(target, e.key === 'Backspace' ? 'end' : 0);
        return changed();
      }
      if (e.key === 'Enter') {
        e.preventDefault();
        snapshot();
        const fresh = makeBlock('p');
        insertAfter(b, fresh);
        syncGhost();
        focusBlock(fresh, 0);
        return changed();
      }
      if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') {
        const p = prevOf(b);
        if (p) (e.preventDefault(), focusBlock(p, 'end'));
      } else if (e.key === 'ArrowDown' || e.key === 'ArrowRight') {
        const n = nextOf(b);
        if (n) (e.preventDefault(), focusBlock(n, 0));
      }
      return;
    }

    const o = offsets(el) ?? { start: 0, end: 0 };
    const text = readText(el);
    const collapsed = o.start === o.end;

    if (e.key === 'Enter') {
      if (MULTILINE.has(b.type) && !e.shiftKey && !mod) {
        e.preventDefault();
        typingSnapshot();
        setMultiline(b, el, `${text.slice(0, o.start)}\n${text.slice(o.end)}`, o.start + 1);
        return changed();
      }
      e.preventDefault();
      if (MULTILINE.has(b.type)) {
        // Mayús+Intro sale del bloque de código o de la tabla.
        snapshot();
        const fresh = makeBlock('p');
        insertAfter(b, fresh);
        syncGhost();
        focusBlock(fresh, 0);
        return changed();
      }
      snapshot();
      if (b.type === 'p' && /^(```|~~~)[\w+-]*$/.test(text)) {
        setType(b, 'code');
        b.fence = text.slice(0, 3);
        b.lang = text.slice(3);
        b.text = '';
        refresh(b);
        syncGhost();
        focusBlock(b, 0);
        return changed();
      }
      if ((LISTS.has(b.type) || b.type === 'quote') && !text) {
        // Intro en un elemento vacío: sube de nivel o termina la lista.
        if (b.indent) b.indent = b.indent.replace(/(\t| {1,4})$/, '');
        else setType(b, 'p');
        b.raw = null;
        refresh(b);
        refreshNumbers();
        return changed();
      }
      if (o.start === 0 && collapsed && text) {
        // Al principio de una línea con texto: se abre una línea vacía encima.
        const fresh = b.type === 'h' ? makeBlock('p') : continuation(b, '');
        if (b.type === 'num') fresh.num = b.num;
        insertBefore(b, fresh);
        refreshNumbers();
        focusBlock(b, 0);
        return changed();
      }
      b.text = text.slice(0, o.start);
      b.raw = null;
      const fresh = continuation(b, text.slice(o.end));
      if (b.ghost) b.ghost = false;
      el.textContent = b.text;
      insertAfter(b, fresh);
      syncGhost();
      focusBlock(fresh, 0);
      refreshNumbers();
      return changed();
    }

    if (e.key === 'Backspace' && collapsed && o.start === 0) {
      const p = prevOf(b);
      if (b.type !== 'p' && !(MULTILINE.has(b.type) && text)) {
        // Al principio de una casilla, viñeta, título o cita: pasa a texto normal.
        e.preventDefault();
        snapshot();
        b.text = text;
        setType(b, 'p');
        refresh(b);
        focusBlock(b, 0);
        refreshNumbers();
        return changed();
      }
      if (!p || MULTILINE.has(b.type)) return;
      e.preventDefault();
      if (b.ghost && !text) return focusBlock(p, 'end');
      snapshot();
      if (p.type === 'hr') {
        remove(p);
        refreshNumbers();
        return changed();
      }
      if (MULTILINE.has(p.type)) {
        if (!text && !b.ghost) remove(b);
        focusBlock(p, 'end');
        return changed();
      }
      const join = p.text.length;
      p.text += text;
      p.raw = null;
      remove(b);
      refresh(p);
      syncGhost();
      focusBlock(p, join);
      refreshNumbers();
      return changed();
    }

    if (e.key === 'Delete' && collapsed && o.start === text.length && !MULTILINE.has(b.type)) {
      const n = nextOf(b);
      if (!n || n.ghost) return;
      e.preventDefault();
      snapshot();
      if (n.type === 'hr') remove(n);
      else if (!MULTILINE.has(n.type)) {
        b.text = text + n.text;
        b.raw = null;
        remove(n);
        refresh(b);
        focusBlock(b, text.length);
      }
      syncGhost();
      return changed();
    }

    if (e.key === 'Tab') {
      e.preventDefault();
      if (MULTILINE.has(b.type)) {
        typingSnapshot();
        setMultiline(b, el, `${text.slice(0, o.start)}\t${text.slice(o.end)}`, o.start + 1);
        return changed();
      }
      if (!LISTS.has(b.type)) return;
      snapshot();
      if (e.shiftKey) b.indent = b.indent.replace(/(\t| {1,4})$/, '');
      else {
        const p = prevOf(b);
        if (!p || !LISTS.has(p.type) || depthOf(p.indent) < depthOf(b.indent)) return;
        b.indent += '\t';
      }
      b.raw = null;
      refresh(b);
      refreshNumbers();
      return changed();
    }

    if ((e.key === 'ArrowUp' || e.key === 'ArrowDown') && !e.shiftKey && collapsed) {
      const up = e.key === 'ArrowUp';
      if (!onEdge(el, up)) return;
      const target = up ? prevOf(b) : nextOf(b);
      if (!focusable(target)) return;
      e.preventDefault();
      const x = caretRect()?.left ?? el.getBoundingClientRect().left;
      return focusAtX(target, x, up);
    }
    if (e.key === 'ArrowLeft' && !e.shiftKey && collapsed && o.start === 0) {
      const p = prevOf(b);
      if (p) (e.preventDefault(), focusBlock(p, 'end'));
    } else if (e.key === 'ArrowRight' && !e.shiftKey && collapsed && o.start === text.length) {
      const n = nextOf(b);
      if (n) (e.preventDefault(), focusBlock(n, 0));
    }
  }

  // ---------- eventos

  root.addEventListener('keydown', onKey);

  root.addEventListener('beforeinput', (e) => {
    if (e.inputType === 'historyUndo' || e.inputType === 'historyRedo') {
      e.preventDefault();
      return e.inputType === 'historyUndo' ? undo() : redo();
    }
    if (e.target.closest?.('.ne-text')) typingSnapshot();
  });

  root.addEventListener('input', (e) => {
    const el = e.target.closest?.('.ne-text');
    if (!el) return;
    const b = blockOf(el);
    if (!b) return;
    if (MULTILINE.has(b.type) && el.childElementCount) flatten(el);
    const text = readText(el);
    if (text.includes('\n') && !MULTILINE.has(b.type)) {
      // Un salto de línea que se coló (arrastrar texto, autocorrección): se reparte en líneas.
      const [first, ...rest] = text.split('\n');
      b.text = first;
      b.raw = null;
      el.textContent = first;
      let ref = b;
      for (const nb of parseMarkdown(rest.join('\n'))) {
        insertAfter(ref, nb);
        ref = nb;
      }
      syncGhost();
      focusBlock(ref, 'end');
      refreshNumbers();
      return changed();
    }
    b.text = text;
    b.raw = null;
    el.classList.toggle('ne-empty', !text);
    if (b.ghost && text) {
      b.ghost = false;
      el.dataset.placeholder = PLACEHOLDER[b.type] ?? '';
      syncGhost();
    }
    if (shortcut(b, el)) return changed();
    // «/» al principio de una línea: menú de bloques filtrado por lo que se escribe.
    if (/^\/[^\s/]*$/.test(text) && b.type !== 'code' && b.type !== 'table') {
      if (menu.block !== b) menu.open(b, el, true);
      menu.filter(text.slice(1));
    } else if (menu.block === b) menu.close();
    changed();
  });

  root.addEventListener('paste', (e) => {
    const el = e.target.closest?.('.ne-text');
    if (!el) return;
    const b = blockOf(el);
    const text = e.clipboardData?.getData('text/plain') ?? '';
    e.preventDefault();
    if (!b || !text) return;
    if (MULTILINE.has(b.type)) {
      const o = offsets(el) ?? { start: b.text.length, end: b.text.length };
      const current = readText(el);
      const clean = text.replace(/\r\n?/g, '\n');
      snapshot();
      setMultiline(b, el, current.slice(0, o.start) + clean + current.slice(o.end), o.start + clean.length);
      return changed();
    }
    if (!/\n/.test(text.replace(/\r?\n$/, ''))) {
      document.execCommand('insertText', false, text.replace(/\r?\n$/, ''));
      return;
    }
    insertLines(b, el, text);
  });

  root.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    const el = e.target.closest?.('.ne-text');
    const link = e.target.closest?.('a');
    if (el && link && el.dataset.mode === 'view') return e.preventDefault(); // el clic abre el enlace
    if (el && el.dataset.rich && el.dataset.mode === 'view') {
      // Línea con formato: se cambia a su Markdown con el cursor donde se hizo clic.
      const pos = document.caretRangeFromPoint?.(e.clientX, e.clientY);
      const at = pos && el.contains(pos.startContainer) ? rawOffsetAt(el, pos.startContainer, pos.startOffset) : null;
      e.preventDefault();
      return focusBlock(blockOf(el), at ?? 'end');
    }
    if (!el && !e.target.closest('.ne-row, input, button')) {
      // Clic en el hueco bajo las líneas: a la última.
      e.preventDefault();
      focusBlock(blocks.at(-1), 'end');
    }
  });

  root.addEventListener('click', (e) => {
    const link = e.target.closest?.('a');
    if (!link || !root.contains(link)) return;
    e.preventDefault();
    onLink?.(link);
  });

  root.addEventListener('focusin', (e) => {
    const el = e.target.closest?.('.ne-text');
    const b = el && blockOf(el);
    if (b && el.dataset.mode === 'view' && el.dataset.rich) {
      showRaw(b, el);
      setCaret(el, b.text.length);
    }
    rows.get(b?.id)?.classList.add('focused');
  });

  root.addEventListener('focusout', (e) => {
    const el = e.target.closest?.('.ne-text');
    const b = el && blockOf(el);
    if (!b) return;
    rows.get(b.id)?.classList.remove('focused');
    if (menu.block === b && !root.contains(e.relatedTarget)) menu.close();
    if (el.dataset.mode === 'raw') showView(b, el);
  });

  // ---------- API

  function setValue(markdown) {
    blocks = parseMarkdown(markdown);
    ensureGhost();
    render();
  }

  return {
    setValue,
    getValue: () => serializeBlocks(blocks),
    hasFocus: () => root.contains(document.activeElement),
    focusEnd: () => focusBlock(blocks.at(-1), 'end'),
    destroy() {
      menu.destroy();
      root.remove();
    },
  };
}
