// Cómo se pinta cada elemento del lienzo que no tiene interacción propia: texto, figuras, marcos,
// flechas, tablas relacionales, secuencias, líneas de tiempo, código, referencias e imágenes.
// Solo construye su interior; moverlos, seleccionarlos y guardarlos es de canvas.js. Lo que viene
// del agente se trata como no fiable: el Markdown pasa por `renderMarkdown` (DOMPurify) y lo demás
// siempre va como texto, nunca como HTML.

import { arrowGeometry } from './canvas-geometry.js';
import { layoutSequence, layoutTimeline } from './canvas-board.js';
import { ICONS, shapePath } from './canvas-catalog.js';

const SVG = 'http://www.w3.org/2000/svg';
export const svgEl = (name, attrs = {}) => {
  const el = document.createElementNS(SVG, name);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  return el;
};
const div = (className, text) => {
  const el = document.createElement('div');
  el.className = className;
  if (text != null) el.textContent = text;
  return el;
};

// El nombre de un elemento para un lector de pantalla.
export function labelOf(el, get) {
  const d = el.datos ?? {};
  const short = (s) => String(s ?? '').replace(/\s+/g, ' ').trim().slice(0, 80);
  switch (el.tipo) {
    case 'texto':
      return `Cuadro de texto: ${short(d.texto) || 'vacío'}`;
    case 'adhesiva':
      return `Nota adhesiva: ${short(d.texto) || 'vacía'}`;
    case 'figura':
      return `Figura: ${short(d.texto) || d.forma}`;
    case 'marco':
      return `Marco: ${short(d.titulo) || 'sin título'}`;
    case 'flecha': {
      const end = (e) => (e?.id != null ? short(labelOf(get(e.id) ?? { datos: {} }, get)) || 'un elemento' : 'un punto suelto');
      return `Flecha de ${end(d.desde)} a ${end(d.hasta)}${d.texto ? `: ${short(d.texto)}` : ''}`;
    }
    case 'tabla':
      return `Tabla ${short(d.titulo)}: ${d.columnas?.length ?? 0} columnas y ${d.filas?.length ?? 0} filas`;
    case 'tablero':
      return `Tablero ${short(d.titulo)}: ${d.tarjetas?.length ?? 0} tarjetas`;
    case 'entidad':
      return `Tabla relacional ${short(d.nombre)}: ${d.columnas?.length ?? 0} columnas`;
    case 'secuencia':
      return `Diagrama de secuencia ${short(d.titulo)}: ${d.mensajes?.length ?? 0} mensajes`;
    case 'linea-tiempo':
      return `Línea de tiempo ${short(d.titulo)}: ${d.tareas?.length ?? 0} tareas`;
    case 'codigo':
      return `Bloque de código ${short(d.titulo || d.lenguaje)}`;
    case 'referencia':
      return `Referencia a ${short(d.titulo || d.ruta) || 'nada todavía'}`;
    case 'imagen':
      return `Imagen: ${short(d.alt) || 'sin descripción'}`;
    default:
      return el.tipo;
  }
}

function markdownBody(text, render, placeholder, scroll = false) {
  const body = div(`cv-md${scroll ? ' cv-scroll' : ''}`);
  if (text) body.innerHTML = render(text);
  else if (placeholder) body.append(div('cv-placeholder', placeholder));
  return body;
}

// Dónde cabe el texto en cada forma: en porcentajes de su caja (`inset`, no `padding`, que se mediría
// siempre por el ancho).
const FORM_INSET = { rombo: '24% 20%', elipse: '12% 10%', nube: '22% 14% 10%', hexagono: '4% 14%', paralelogramo: '4% 16%', documento: '4% 6% 16%', cilindro: '22% 4% 8%', cola: '6% 12%', persona: '64% 0 0' };

function paintFigure(body, el, ctx) {
  const { forma, texto } = el.datos;
  body.replaceChildren();
  const d = shapePath(forma, el.w, el.h);
  if (d) {
    const svg = svgEl('svg', { class: 'cv-shape', viewBox: `0 0 ${el.w} ${el.h}`, width: el.w, height: el.h, 'aria-hidden': 'true' });
    svg.append(svgEl('path', { d, class: forma === 'inicio' ? 'is-solid' : '' }));
    body.append(svg);
  } else {
    body.append(div(`cv-box cv-box-${forma}`));
    // Las de arquitectura que son una caja llevan su marca en la esquina, para distinguirlas.
    if (['servicio', 'cache', 'api', 'externo'].includes(forma)) {
      const mark = div('cv-mark');
      mark.innerHTML = ICONS[forma];
      body.append(mark);
    }
  }
  if (forma === 'inicio') return;
  const label = div('cv-label');
  if (FORM_INSET[forma]) label.style.inset = FORM_INSET[forma];
  label.append(markdownBody(texto, ctx.renderMarkdown));
  body.append(label);
}

// Una flecha es un SVG pequeño, de su propio tamaño: nunca uno del tamaño del lienzo.
const ARROW_PAD = 28;
export function paintArrow(node, el, get) {
  const g = arrowGeometry(el.datos, get);
  node.hidden = !g;
  if (!g) return null;
  const x = g.box.x - ARROW_PAD;
  const y = g.box.y - ARROW_PAD;
  const w = g.box.w + ARROW_PAD * 2;
  const h = g.box.h + ARROW_PAD * 2;
  Object.assign(node.style, { left: `${x + ctxOrigin.x}px`, top: `${y + ctxOrigin.y}px`, width: `${w}px`, height: `${h}px` });
  let svg = node.firstElementChild;
  if (!svg || svg.tagName.toLowerCase() !== 'svg') {
    svg = svgEl('svg', { class: 'cv-arrow-svg', 'aria-hidden': 'true' });
    svg.append(svgEl('path', { class: 'cv-arrow-hit' }), svgEl('path', { class: 'cv-arrow-line' }), svgEl('g', { class: 'cv-arrow-heads' }));
    node.prepend(svg);
  }
  svg.setAttribute('viewBox', `${x} ${y} ${w} ${h}`);
  svg.setAttribute('width', w);
  svg.setAttribute('height', h);
  svg.children[0].setAttribute('d', g.d);
  svg.children[1].setAttribute('d', g.d);
  svg.children[1].classList.toggle('is-dashed', el.datos.trazo === 'discontinuo');
  svg.children[2].replaceChildren(...g.heads.map((s) => svgEl('path', { d: s.d, class: s.fill ? 'is-fill' : '' })));
  let label = node.querySelector('.cv-arrow-label');
  const text = el.datos.texto ?? '';
  if (!text && label && !label.querySelector('[contenteditable]')) {
    label.remove();
    label = null;
  }
  if (text && !label) {
    label = div('cv-arrow-label');
    node.append(label);
  }
  if (label) {
    if (!label.querySelector('[contenteditable]')) label.textContent = text;
    Object.assign(label.style, { left: `${g.mid.x - x}px`, top: `${g.mid.y - y}px` });
  }
  return g;
}

// El origen del lienzo en el DOM (lo pone canvas.js, es el `ORIGIN` de notes.js).
const ctxOrigin = { x: 0, y: 0 };
export function setOrigin(origin) {
  Object.assign(ctxOrigin, origin);
}

function paintEntity(body, el) {
  const d = el.datos;
  const head = div('cv-card-head cv-grip');
  head.append(div('cv-card-title', d.nombre || 'tabla'));
  const rows = div('cv-ent-rows');
  for (const c of d.columnas ?? []) {
    const row = div('cv-ent-row');
    row.dataset.col = c.nombre;
    const keys = div('cv-ent-key', c.pk ? 'PK' : c.fk ? 'FK' : '');
    const flags = [c.nulo ? 'nulo' : '', c.unico ? 'único' : ''].filter(Boolean).join(' · ');
    row.append(keys, div('cv-ent-name', c.nombre), div('cv-ent-type', c.tipo));
    if (flags) row.title = flags;
    rows.append(row);
  }
  body.replaceChildren(head, rows);
}

function paintSequence(body, el) {
  const d = el.datos;
  const l = layoutSequence(d, el.w);
  body.replaceChildren();
  if (d.titulo) {
    const head = div('cv-card-head cv-grip');
    head.append(div('cv-card-title', d.titulo));
    body.append(head);
  }
  const svg = svgEl('svg', { class: 'cv-seq-svg', viewBox: `0 0 ${el.w} ${Math.max(el.h, l.height)}`, width: el.w, height: Math.max(el.h, l.height), 'aria-hidden': 'true' });
  for (const p of l.people) svg.append(svgEl('path', { class: 'cv-seq-life', d: `M ${p.x} ${l.lineTop} L ${p.x} ${l.lineBottom}` }));
  const labels = [];
  for (const r of l.rows) {
    const cls = r.tipo === 'respuesta' ? 'cv-seq-msg is-dashed' : 'cv-seq-msg';
    if (r.self) {
      svg.append(svgEl('path', { class: cls, d: `M ${r.from} ${r.y - 10} h 36 v 20 h -36` }));
      svg.append(svgEl('path', { class: 'cv-seq-head', d: `M ${r.from + 9} ${r.y + 5} L ${r.from} ${r.y + 10} L ${r.from + 9} ${r.y + 15}` }));
      labels.push({ x: r.from + 42, y: r.y, text: r.text, align: 'left' });
    } else {
      const dir = Math.sign(r.to - r.from);
      svg.append(svgEl('path', { class: cls, d: `M ${r.from} ${r.y} L ${r.to} ${r.y}` }));
      const head = `M ${r.to - dir * 9} ${r.y - 5} L ${r.to} ${r.y} L ${r.to - dir * 9} ${r.y + 5}`;
      svg.append(svgEl('path', { class: `cv-seq-head${r.tipo === 'sincrono' ? ' is-fill' : ''}`, d: r.tipo === 'sincrono' ? `${head} Z` : head }));
      labels.push({ x: (r.from + r.to) / 2, y: r.y - 13, text: r.text, align: 'center' });
    }
  }
  body.append(svg);
  for (const p of l.people) {
    const b = div('cv-seq-actor', p.name);
    Object.assign(b.style, { left: `${p.x}px`, top: `${l.headTop}px`, maxWidth: `${l.colWidth - 12}px` });
    body.append(b);
  }
  for (const t of labels) {
    const b = div(`cv-seq-text is-${t.align}`, t.text);
    Object.assign(b.style, { left: `${t.x}px`, top: `${t.y}px`, maxWidth: `${l.colWidth * 1.4}px` });
    body.append(b);
  }
}

function paintTimeline(body, el, ctx) {
  const d = el.datos;
  const l = layoutTimeline(d, el.w, ctx.today());
  body.replaceChildren();
  if (d.titulo) {
    const head = div('cv-card-head cv-grip');
    head.append(div('cv-card-title', d.titulo));
    body.append(head);
  }
  const svg = svgEl('svg', { class: 'cv-time-svg', viewBox: `0 0 ${el.w} ${Math.max(el.h, l.height)}`, width: el.w, height: Math.max(el.h, l.height), 'aria-hidden': 'true' });
  for (const t of l.ticks) svg.append(svgEl('path', { class: 'cv-time-tick', d: `M ${t.x} ${l.top - 6} L ${t.x} ${l.height}` }));
  for (const link of l.links) {
    const midX = Math.max(link.from.x + 8, link.to.x - 8);
    svg.append(svgEl('path', { class: 'cv-time-link', d: `M ${link.from.x} ${link.from.y} H ${midX} V ${link.to.y} H ${link.to.x}` }));
  }
  if (l.today != null) svg.append(svgEl('path', { class: 'cv-time-today', d: `M ${l.today} ${l.top - 6} L ${l.today} ${l.height}` }));
  for (const m of l.marks) svg.append(svgEl('path', { class: 'cv-time-mark', d: `M ${m.x} ${l.top - 22} l 6 6 l -6 6 l -6 -6 Z` }));
  body.append(svg);
  for (const t of l.ticks) {
    const b = div('cv-time-date', t.date.slice(5));
    Object.assign(b.style, { left: `${t.x}px`, top: `${l.top - 22}px` });
    body.append(b);
  }
  for (const m of l.marks) {
    const b = div('cv-time-milestone', m.title);
    b.title = `${m.title} · ${m.date}`;
    Object.assign(b.style, { left: `${m.x + 10}px`, top: `${l.top - 24}px` });
    body.append(b);
  }
  for (const r of l.bars) {
    const name = div('cv-time-name', r.title);
    Object.assign(name.style, { top: `${r.y}px` });
    body.append(name);
    if (r.x == null) continue;
    const bar = div('cv-time-bar');
    Object.assign(bar.style, { left: `${r.x}px`, top: `${r.y + 8}px`, width: `${r.w}px` });
    if (r.progress) {
      const done = div('cv-time-done');
      done.style.width = `${Math.min(100, r.progress)}%`;
      bar.append(done);
    }
    body.append(bar);
  }
}

function paintCode(body, el, ctx) {
  const d = el.datos;
  body.replaceChildren();
  const head = div('cv-card-head cv-grip');
  head.append(div('cv-card-title', d.titulo || d.lenguaje || 'Código'));
  const pre = document.createElement('pre');
  pre.className = 'cv-code cv-scroll';
  const code = document.createElement('code');
  code.textContent = d.texto ?? '';
  pre.append(code);
  body.append(head, pre);
  // El coloreado se carga solo la primera vez que hace falta, como en el lector de archivos.
  ctx.highlight(d.texto ?? '', d.lenguaje).then((html) => {
    if (html != null && code.isConnected) code.innerHTML = html;
  });
}

function paintReference(body, el, ctx) {
  const d = el.datos;
  const chip = document.createElement('button');
  chip.type = 'button';
  chip.className = 'cv-ref cv-grip-soft';
  const broken = Boolean(d.ruta) && ctx.refMissing(d);
  chip.classList.toggle('is-broken', broken);
  const icon = div('cv-ref-icon');
  icon.innerHTML = ICONS.referencia;
  const text = div('cv-ref-text', d.titulo || d.ruta || 'Elige a qué apunta');
  chip.append(icon, text);
  if (broken) chip.append(div('cv-ref-note', 'Ya no existe'));
  chip.title = broken ? `${d.ruta} ya no existe` : d.ruta ? `Abrir ${d.ruta}` : 'Elige a qué apunta';
  chip.addEventListener('click', (event) => {
    event.stopPropagation();
    ctx.openRef(el);
  });
  body.replaceChildren(chip);
}

function paintImage(body, el, ctx) {
  const d = el.datos;
  body.replaceChildren();
  if (!d.recurso) {
    body.append(div('cv-placeholder', 'Sin imagen'));
    return;
  }
  const img = document.createElement('img');
  img.className = 'cv-image';
  img.alt = d.alt ?? '';
  img.draggable = false;
  body.append(img);
  ctx.readImage(d.recurso).then((src) => {
    if (src) img.src = src;
    else body.replaceChildren(div('cv-placeholder', 'La imagen ya no existe'));
  });
}

function paintFrame(body, el) {
  const title = div('cv-frame-title cv-grip', el.datos.titulo || 'Marco');
  title.classList.toggle('is-empty', !el.datos.titulo);
  body.replaceChildren(title);
}

// Pinta el interior de un elemento en su `.cv-body`. Las tablas y los tableros van aparte
// (canvas-grid.js): tienen interacción propia y llegan por `ctx.paintOwn`.
export function paintBody(body, el, ctx) {
  const d = el.datos ?? {};
  switch (el.tipo) {
    case 'texto':
      body.replaceChildren(markdownBody(d.texto, ctx.renderMarkdown, 'Escribe algo…', true));
      break;
    case 'adhesiva':
      body.replaceChildren(markdownBody(d.texto, ctx.renderMarkdown, 'Una idea…', true));
      break;
    case 'figura':
      paintFigure(body, el, ctx);
      break;
    case 'marco':
      paintFrame(body, el);
      break;
    case 'entidad':
      paintEntity(body, el);
      break;
    case 'secuencia':
      paintSequence(body, el);
      break;
    case 'linea-tiempo':
      paintTimeline(body, el, ctx);
      break;
    case 'codigo':
      paintCode(body, el, ctx);
      break;
    case 'referencia':
      paintReference(body, el, ctx);
      break;
    case 'imagen':
      paintImage(body, el, ctx);
      break;
    default:
      ctx.paintOwn(body, el);
  }
}
