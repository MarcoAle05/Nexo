// Los elementos del lienzo de Notas: lo que no son notas (cuadros de texto, figuras, flechas,
// tablas, tableros…) y sus capas. Ver docs/spec-lienzo-elementos.md y `notas.md`.
//
// Todo cambio es una operación y lo aplica el motor (nexo_core::lienzo), que es el único que
// escribe. Aquí solo se pinta lo que devuelve, en `cambios`, y se lleva el gesto en curso. Lo que
// llega del agente nunca toca la vista, el foco, la selección ni el historial: solo crea o cambia
// sus propios nodos. Solo está montado lo que cae cerca de la vista.

import { arrowRoughBox, aligned, boundsOfRects, distributed, insideFrame, intersects, readingOrder, rectOf, snapX, snapY, WORLD } from './canvas-geometry.js';
import { WRITES_AT_ONCE, catalogItem, sizeOf, templateOps } from './canvas-catalog.js';
import { labelOf, paintArrow, paintBody, setOrigin, svgEl } from './canvas-render.js';
import { createGrid } from './canvas-grid.js';
import { createChrome } from './canvas-chrome.js';

const USER_LAYER = 'tu';
const LAYER_BASE = 100000;
const MIN = 22;
const GIVE = 4; // píxeles antes de que un clic pase a ser un arrastre

export function createCanvas(deps) {
  const { invoke, ask, fail, renderMarkdown, surface, viewport, origin } = deps;
  setOrigin(origin);
  let desk = null;
  const els = new Map();
  const capas = new Map();
  const nodes = new Map(); // id → nodo montado
  const layerNodes = new Map();
  const selection = new Set();
  const arrowsOf = new Map(); // id de un elemento → las flechas que lo tocan
  let undoStack = [];
  let redoStack = [];
  let queue = Promise.resolve();
  let gesture = null; // { ids:Set } mientras se arrastra: lo que llegue no mueve esos
  let editing = null; // { id, finish }
  let armed = null; // la entrada del catálogo que espera su sitio
  let clip = null; // lo último copiado: { text, items }
  let frame = 0;
  const statusEl = document.getElementById('cv-status');
  const get = (id) => els.get(id);
  const scale = () => deps.scale();

  // Lo que hay que decir sin interrumpir: se ve un momento y lo oye el lector de pantalla. Lo de
  // colocar algo se queda mientras se esté eligiendo el sitio.
  let statusTimer = 0;
  const status = (text, { keep = false } = {}) => {
    if (!statusEl) return;
    clearTimeout(statusTimer);
    statusEl.textContent = text;
    statusEl.hidden = !text;
    if (text && !keep) statusTimer = setTimeout(() => (statusEl.hidden = true), 4000);
  };

  // ------------------------------------------------------------ operaciones

  function apply(ops, { record = true } = {}) {
    const target = desk;
    if (!target || !ops.length) return Promise.resolve(null);
    const run = queue.then(async () => {
      const r = await invoke('canvas_apply', { desk: target, ops });
      if (desk === target) absorb(r.cambios);
      if (record && r.inversas.length) {
        undoStack.push({ ops: r.inversas });
        if (undoStack.length > 200) undoStack.shift();
        redoStack = [];
      }
      // Lo del usuario se aplica siempre: esto solo salta con un dato imposible o un tope.
      const first = r.omitidas.find((o) => !/ya no existe/.test(o.motivo ?? ''));
      if (first?.motivo) status(first.motivo);
      if (r.omitidas.length && !first) status('Parte de lo que se iba a deshacer ya no existe.');
      chrome.changed();
      return r;
    });
    queue = run.catch((error) => fail(error));
    return run.catch(() => null);
  }

  const use = (key, on) => {
    if (desk) invoke('canvas_use', { desk, key, on }).catch(() => {});
  };

  async function undo() {
    const entry = undoStack.pop();
    if (!entry) return status('No hay nada que deshacer.');
    if (entry.cards) deps.moveCards(entry.cards);
    const r = await apply(entry.ops, { record: false });
    if (r) redoStack.push({ ops: r.inversas, cards: entry.cardsBack, cardsBack: entry.cards });
    chrome.changed();
    return undefined;
  }
  async function redo() {
    const entry = redoStack.pop();
    if (!entry) return status('No hay nada que rehacer.');
    if (entry.cards) deps.moveCards(entry.cards);
    const r = await apply(entry.ops, { record: false });
    if (r) undoStack.push({ ops: r.inversas, cards: entry.cardsBack, cardsBack: entry.cards });
    chrome.changed();
    return undefined;
  }

  // ------------------------------------------------------------ recibir

  function indexArrow(el, add) {
    for (const end of [el.datos?.desde, el.datos?.hasta]) {
      if (end?.id == null) continue;
      if (add) {
        if (!arrowsOf.has(end.id)) arrowsOf.set(end.id, new Set());
        arrowsOf.get(end.id).add(el.id);
      } else arrowsOf.get(end.id)?.delete(el.id);
    }
  }

  function absorb(c, { fromAgent = false } = {}) {
    if (!c) return;
    for (const capa of c.capas ?? []) {
      const before = capas.get(capa.id);
      capas.set(capa.id, capa);
      if (fromAgent && before?.estado !== capa.estado) announceLayer(capa);
      if (fromAgent && !before) announceLayer(capa);
    }
    const repaintArrows = new Set();
    for (const el of c.poner ?? []) {
      const prev = els.get(el.id);
      if (prev?.tipo === 'flecha') indexArrow(prev, false);
      // Lo que tengo en las manos no se mueve: al soltar, gana lo mío.
      const next = gesture?.ids.has(el.id) && prev ? { ...el, x: prev.x, y: prev.y, w: prev.w, h: prev.h } : el;
      if (prev && nodes.has(el.id) && el.tipo !== 'flecha' && (prev.x !== next.x || prev.y !== next.y || prev.w !== next.w || prev.h !== next.h || prev.z !== next.z)) flush(prev);
      els.set(el.id, next);
      if (el.tipo === 'flecha') indexArrow(el, true);
      for (const a of arrowsOf.get(el.id) ?? []) repaintArrows.add(a);
      const node = nodes.get(el.id);
      if (node) {
        paint(el);
      } else if (fromAgent) entering.add(el.id);
    }
    for (const id of c.quitar ?? []) {
      const prev = els.get(id);
      if (prev && nodes.has(id)) flush(prev.tipo === 'flecha' ? rectOf(prev, get) : prev);
      if (prev?.tipo === 'flecha') indexArrow(prev, false);
      for (const a of arrowsOf.get(id) ?? []) repaintArrows.add(a);
      els.delete(id);
      arrowsOf.delete(id);
      unmount(id);
      if (selection.delete(id)) chrome.selectionChanged();
      grid.forget(id);
    }
    for (const id of c.capas_quitadas ?? []) {
      capas.delete(id);
      for (const key of [id, `${id}:fondo`]) {
        layerNodes.get(key)?.remove();
        layerNodes.delete(key);
      }
    }
    for (const id of repaintArrows) {
      const a = els.get(id);
      if (a && nodes.has(id)) paint(a);
    }
    syncLayers();
    queueVisible();
    deps.onChange();
    chrome.changed();
  }

  function announceLayer(capa) {
    if (capa.autor !== 'nexo') return;
    const text = {
      dibujando: `Nexo está dibujando «${capa.nombre}» en el lienzo.`,
      terminada: `Nexo terminó «${capa.nombre}».`,
      sin_terminar: `«${capa.nombre}» se quedó sin terminar.`,
    }[capa.estado];
    if (text) status(text);
  }

  // ------------------------------------------------------------ pintar

  const entering = new Set();

  // Fuerza a repintar un trozo del lienzo: se pone y se quita un velo casi transparente encima.
  // El usuario vio en la app, pintando por GPU, que los tiradores de algo borrado o que cambió de
  // tamaño se quedaban a la vista hasta la siguiente acción (en el banco, sin GPU, no se reproduce).
  const FLUSH_PAD = 30;
  let flushRects = [];
  let flushFrame = 0;
  function flush(rect) {
    if (!rect || viewport.hidden) return;
    flushRects.push(rect);
    if (flushFrame) return;
    flushFrame = requestAnimationFrame(() => {
      flushFrame = 0;
      const rects = flushRects.length > 8 ? [boundsOfRects(flushRects)] : flushRects;
      flushRects = [];
      const veils = rects.map((r) => {
        const v = document.createElement('div');
        v.className = 'cv-flush';
        Object.assign(v.style, { left: `${r.x + origin.x - FLUSH_PAD}px`, top: `${r.y + origin.y - FLUSH_PAD}px`, width: `${r.w + FLUSH_PAD * 2}px`, height: `${r.h + FLUSH_PAD * 2}px` });
        surface.append(v);
        return v;
      });
      requestAnimationFrame(() => veils.forEach((v) => v.remove()));
    });
  }

  const paintCtx = {
    renderMarkdown,
    highlight: deps.highlight,
    today: () => new Date().toISOString().slice(0, 10),
    refMissing: (datos) => deps.refMissing(datos),
    openRef: (el) => {
      if (el.datos.ruta) deps.openRef(el.datos);
      else chrome.openEditor(el);
    },
    readImage: (recurso) => invoke('canvas_image_read', { resource: recurso }).catch(() => null),
    paintOwn: (body, el) => grid.paintOwn(body, el),
  };

  // Cada capa tiene dos contextos de apilamiento: uno por encima de las cartas de notas (una
  // flecha o un cuadro pueden señalar una nota) y otro por debajo, para lo que tiene `z` 0 o menos
  // (lo enviado al fondo y los marcos, que así abrazan las notas sin taparlas).
  const behind = (el) => el.z <= 0;
  function layerOf(id, back = false) {
    const key = back ? `${id}:fondo` : id;
    let n = layerNodes.get(key);
    if (!n) {
      n = document.createElement('div');
      n.className = back ? 'cv-layer is-back' : 'cv-layer';
      n.dataset.layer = id;
      // Los de detrás, antes que las cartas en el DOM: con z-index 0 quedan por debajo de ellas.
      if (back) surface.prepend(n);
      else surface.append(n);
      layerNodes.set(key, n);
    }
    return n;
  }
  const containerOf = (el) => layerOf(el.capa, behind(el));

  // Oculta, una capa no se pinta ni se le puede dar.
  function syncLayers() {
    let i = 0;
    for (const capa of [...capas.values()].sort((a, b) => (a.id === USER_LAYER ? -1 : b.id === USER_LAYER ? 1 : 0))) {
      const n = layerOf(capa.id);
      n.style.zIndex = String(LAYER_BASE + i++);
      n.hidden = capa.visible === false;
      const back = layerOf(capa.id, true);
      back.style.zIndex = '0';
      back.hidden = capa.visible === false;
      paintTag(capa, n);
    }
  }

  // Dónde está dibujando el agente, sobre lo que lleva hecho.
  function paintTag(capa, layer) {
    let tag = layer.querySelector(':scope > .cv-layer-tag');
    const live = capa.estado === 'dibujando';
    const box = live ? boundsOfRects([...els.values()].filter((e) => e.capa === capa.id && e.tipo !== 'flecha').map((e) => rectOf(e, get))) : null;
    if (!box) return tag?.remove();
    if (!tag) {
      tag = document.createElement('div');
      tag.className = 'cv-layer-tag';
      layer.append(tag);
    }
    tag.textContent = `Nexo · ${capa.nombre} · dibujando`;
    Object.assign(tag.style, { left: `${box.x + origin.x}px`, top: `${box.y + origin.y - 30}px` });
    return undefined;
  }

  function placeNode(node, el) {
    Object.assign(node.style, { left: `${el.x + origin.x}px`, top: `${el.y + origin.y}px`, width: `${el.w}px`, height: `${el.h}px`, zIndex: String(el.z) });
  }

  function sigOf(el) {
    const size = ['figura', 'secuencia', 'linea-tiempo'].includes(el.tipo) ? `${el.w}×${el.h}` : '';
    return `${size}|${JSON.stringify(el.datos)}`;
  }

  function paint(el) {
    const node = nodes.get(el.id);
    if (!node) return;
    const home = containerOf(el);
    if (node.parentNode !== home) home.append(node);
    node.className = `cv-el cv-t-${el.tipo}${el.datos?.color ? ` cv-c-${el.datos.color}` : ''}${selection.has(el.id) ? ' is-selected' : ''}${el.tipo === 'figura' ? ` cv-f-${el.datos.forma}` : ''}${el.tipo === 'texto' ? ` cv-size-${el.datos.tamano} cv-align-${el.datos.alineado}` : ''}${['tabla', 'tablero', 'entidad', 'secuencia', 'linea-tiempo', 'codigo'].includes(el.tipo) ? ' cv-card' : ''}`;
    node.setAttribute('aria-label', labelOf(el, get));
    if (el.tipo === 'flecha') {
      paintArrow(node, el, get);
      node.style.zIndex = String(el.z);
      decorateArrow(node, el);
      return;
    }
    placeNode(node, el);
    const body = node.querySelector(':scope > .cv-body');
    // Lo que se está escribiendo o arrastrando dentro no se repinta bajo las manos: se pondrá al
    // día al acabar. Es lo que permite que el agente añada algo a la misma tabla a la vez.
    if (editing?.id === el.id || grid.holds(el.id)) return;
    const sig = sigOf(el);
    if (body.dataset.sig !== sig) {
      const hadFocus = body.contains(document.activeElement);
      paintBody(body, el, paintCtx);
      body.dataset.sig = sig;
      if (hadFocus && !body.contains(document.activeElement) && !grid.refocus(el)) node.focus({ preventScroll: true });
    }
    decorate(node, el);
  }

  // Los tiradores de lo seleccionado y los puntos de donde salen las flechas.
  function decorate(node, el) {
    const selected = selection.has(el.id);
    const single = selected && selection.size === 1;
    node.classList.toggle('is-selected', selected);
    node.querySelectorAll(':scope > .cv-handle').forEach((h) => h.remove());
    if (single) {
      for (const corner of ['nw', 'ne', 'sw', 'se']) {
        const h = document.createElement('span');
        h.className = `cv-handle cv-h-${corner}`;
        h.dataset.handle = corner;
        node.append(h);
      }
    }
    if (!node.querySelector(':scope > .cv-port')) {
      for (const side of ['n', 'e', 's', 'o']) {
        const p = document.createElement('span');
        p.className = `cv-port cv-p-${side}`;
        p.dataset.port = side;
        p.title = 'Tirar una flecha';
        node.append(p);
      }
    }
  }

  function decorateArrow(node, el) {
    const selected = selection.has(el.id);
    node.classList.toggle('is-selected', selected);
    node.querySelectorAll(':scope > .cv-end').forEach((h) => h.remove());
    if (!selected || selection.size !== 1) return;
    const box = { x: parseFloat(node.style.left) - origin.x, y: parseFloat(node.style.top) - origin.y };
    const geo = paintArrow(node, el, get);
    if (!geo) return;
    for (const [end, p] of [['desde', geo.from], ['hasta', geo.to]]) {
      const h = document.createElement('span');
      h.className = 'cv-end';
      h.dataset.end = end;
      Object.assign(h.style, { left: `${p.x - box.x}px`, top: `${p.y - box.y}px` });
      node.append(h);
    }
  }

  function mount(el) {
    const node = document.createElement('div');
    node.dataset.id = el.id;
    node.tabIndex = -1;
    node.setAttribute('role', 'group');
    nodes.set(el.id, node);
    if (el.tipo !== 'flecha') {
      const body = document.createElement('div');
      body.className = 'cv-body';
      node.append(body);
    }
    containerOf(el).append(node);
    paint(el);
    if (entering.delete(el.id) && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      node.classList.add('cv-enter');
      setTimeout(() => node.classList.remove('cv-enter'), 200);
    }
  }

  function unmount(id) {
    nodes.get(id)?.remove();
    nodes.delete(id);
  }

  // Solo se monta lo que cae cerca de la vista: una pantalla más alrededor.
  function renderVisible() {
    frame = 0;
    if (viewport.hidden || !desk) return;
    const k = scale();
    const cam = deps.camera();
    const w = viewport.clientWidth / k;
    const h = viewport.clientHeight / k;
    const view = { x: cam.x - w, y: cam.y - h, w: w * 3, h: h * 3 };
    const active = document.activeElement;
    for (const el of els.values()) {
      const has = nodes.has(el.id);
      const box = el.tipo === 'flecha' ? arrowRoughBox(el.datos, get) : el;
      const visible = capas.get(el.capa)?.visible !== false;
      const near = visible && (selection.has(el.id) || editing?.id === el.id || (box && intersects(box, view)));
      if (near && !has) mount(el);
      else if (!near && has && !nodes.get(el.id).contains(active)) unmount(el.id);
    }
    chrome.place();
  }
  const queueVisible = () => {
    if (!frame) frame = requestAnimationFrame(renderVisible);
  };

  // ------------------------------------------------------------ seleccionar

  function select(ids, { focus = true } = {}) {
    const before = [...selection];
    for (const id of before) if (!ids.includes(id) && els.get(id)) flush(rectOf(els.get(id), get));
    selection.clear();
    for (const id of ids) selection.add(id);
    for (const id of new Set([...before, ...ids])) {
      const el = els.get(id);
      if (el && nodes.has(id)) paint(el);
    }
    queueVisible();
    if (focus && ids.length === 1) focusEl(ids[0]);
    chrome.selectionChanged();
  }
  const clearSelection = () => select([], { focus: false });

  function focusEl(id) {
    if (!nodes.has(id)) {
      const el = els.get(id);
      if (el) mount(el);
    }
    nodes.get(id)?.focus({ preventScroll: true });
  }

  // Lo que viene con lo seleccionado al moverlo: lo que tienen dentro los marcos.
  function withContents(ids) {
    const out = new Set(ids);
    const cards = [];
    const items = [...els.values()].filter((e) => e.tipo !== 'flecha');
    const notes = deps.cards();
    for (const id of ids) {
      const el = els.get(id);
      if (el?.tipo !== 'marco') continue;
      for (const inner of insideFrame(el, items)) out.add(inner);
      for (const c of insideFrame(el, notes)) if (!cards.includes(c)) cards.push(c);
    }
    return { ids: out, cards };
  }

  // ------------------------------------------------------------ gestos

  const pointWorld = (e) => {
    const r = viewport.getBoundingClientRect();
    const c = deps.camera();
    const k = scale();
    return { x: c.x + (e.clientX - r.left) / k, y: c.y + (e.clientY - r.top) / k };
  };
  const clampX = (x, w) => Math.max(-WORLD.w / 2, Math.min(x, WORLD.w / 2 - w));
  const clampY = (y, h) => Math.max(-WORLD.h / 2, Math.min(y, WORLD.h / 2 - h));

  function dragOnce(onMove, onUp) {
    const up = (e) => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', up);
      onUp(e);
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', up);
  }

  // Durante un gesto, lo de alrededor no se recalcula en cada fotograma: la barra se oculta y el
  // minimapa se pone al día de vez en cuando. Medido en WebKitGTK sin GPU, arrastrar una caja con
  // sus flechas entre 400 elementos pasó de 22 a 55 fps.
  let mapStamp = 0;
  function gestureTick() {
    const now = performance.now();
    if (now - mapStamp > 120) {
      mapStamp = now;
      deps.onChange();
    }
  }

  function repaintArrowsOf(ids) {
    const done = new Set();
    for (const id of ids) {
      for (const a of arrowsOf.get(id) ?? []) {
        if (done.has(a)) continue;
        done.add(a);
        const arrow = els.get(a);
        if (arrow && nodes.has(a)) paintArrow(nodes.get(a), arrow, get);
      }
    }
  }

  function startMove(event, el) {
    const start = { px: event.clientX, py: event.clientY };
    const { ids, cards } = withContents([...selection]);
    const arrows = [...ids].filter((id) => els.get(id)?.tipo === 'flecha');
    for (const id of arrows) ids.delete(id);
    const from = new Map([...ids].map((id) => [id, { x: els.get(id).x, y: els.get(id).y }]));
    const cardFrom = deps.cards().filter((c) => cards.includes(c.id));
    let moved = false;
    const lead = from.get(el.id) ?? { x: el.x, y: el.y };
    const move = (e) => {
      const k = scale();
      const rawX = (e.clientX - start.px) / k;
      const rawY = (e.clientY - start.py) / k;
      if (!moved && Math.hypot(rawX, rawY) * k < GIVE) return;
      if (!moved) {
        moved = true;
        gesture = { ids: new Set(ids) };
        for (const id of ids) use(id, true);
        chrome.hideBar();
      }
      // La retícula manda sobre el que se arrastra; Alt la apaga. Los demás se mueven lo mismo.
      const dx = e.altKey ? rawX : snapX(lead.x + rawX) - lead.x;
      const dy = e.altKey ? rawY : snapY(lead.y + rawY) - lead.y;
      for (const [id, p] of from) {
        const it = els.get(id);
        it.x = clampX(p.x + dx, it.w);
        it.y = clampY(p.y + dy, it.h);
        const node = nodes.get(id);
        if (node) node.style.translate = `${it.x - p.x}px ${it.y - p.y}px`;
      }
      repaintArrowsOf(ids);
      deps.dragCards?.(cardFrom, dx, dy);
      gestureTick();
    };
    dragOnce(move, () => {
      if (!moved) return;
      gesture = null;
      for (const id of ids) {
        use(id, false);
        const node = nodes.get(id);
        if (node) {
          node.style.translate = '';
          placeNode(node, els.get(id));
        }
      }
      const ops = [...ids].map((id) => ({ op: 'cambiar', id, x: Math.round(els.get(id).x), y: Math.round(els.get(id).y) }));
      apply(ops);
      if (cardFrom.length) deps.commitCards?.();
      chrome.selectionChanged();
      deps.onChange();
    });
  }

  function startResize(event, el, corner) {
    event.preventDefault();
    event.stopPropagation();
    const start = { px: event.clientX, py: event.clientY, x: el.x, y: el.y, w: el.w, h: el.h };
    const node = nodes.get(el.id);
    let moved = false;
    const move = (e) => {
      const k = scale();
      const dx = (e.clientX - start.px) / k;
      const dy = (e.clientY - start.py) / k;
      if (!moved) {
        moved = true;
        gesture = { ids: new Set([el.id]) };
        use(el.id, true);
        chrome.hideBar();
      }
      const free = e.altKey;
      let { x, y, w, h } = start;
      if (corner.includes('e')) w = Math.max(MIN, free ? start.w + dx : snapX(start.x + start.w + dx) - start.x);
      if (corner.includes('s')) h = Math.max(MIN, free ? start.h + dy : snapY(start.y + start.h + dy) - start.y);
      if (corner.includes('w')) {
        const right = start.x + start.w;
        x = Math.min(right - MIN, free ? start.x + dx : snapX(start.x + dx));
        w = right - x;
      }
      if (corner.includes('n')) {
        const bottom = start.y + start.h;
        y = Math.min(bottom - MIN, free ? start.y + dy : snapY(start.y + dy));
        h = bottom - y;
      }
      Object.assign(el, { x, y, w, h });
      placeNode(node, el);
      if (['figura', 'secuencia', 'linea-tiempo'].includes(el.tipo)) {
        const body = node.querySelector(':scope > .cv-body');
        paintBody(body, el, paintCtx);
        body.dataset.sig = sigOf(el);
      }
      repaintArrowsOf([el.id]);
      gestureTick();
    };
    dragOnce(move, () => {
      gesture = null;
      if (!moved) return;
      use(el.id, false);
      apply([{ op: 'cambiar', id: el.id, x: Math.round(el.x), y: Math.round(el.y), w: Math.round(el.w), h: Math.round(el.h) }]);
      chrome.selectionChanged();
      deps.onChange();
    });
  }

  // Tirar una flecha: de un elemento (o de un punto) a otro elemento, o a un punto suelto.
  function endAt(e, exclude) {
    const over = document.elementFromPoint(e.clientX, e.clientY);
    const node = over?.closest?.('.cv-el');
    const id = node?.dataset.id;
    const el = id && els.get(id);
    if (!el || el.tipo === 'flecha' || id === exclude) {
      const p = pointWorld(e);
      return { x: Math.round(p.x), y: Math.round(p.y) };
    }
    const row = over.closest?.('.cv-ent-row');
    return row ? { id, columna: row.dataset.col } : { id };
  }

  function draftArrow(datos) {
    const node = document.createElement('div');
    node.className = 'cv-el cv-t-flecha is-draft';
    layerOf(USER_LAYER).append(node);
    return node;
  }

  function startArrow(event, from) {
    event.preventDefault();
    event.stopPropagation();
    const start = from ?? endFromPoint(event);
    const draft = draftArrow();
    const temp = { id: 'draft', tipo: 'flecha', x: 0, y: 0, w: 0, h: 0, datos: { desde: start, hasta: start, ruta: 'angulada', fin: 'flecha', inicio: 'nada' } };
    let moved = false;
    const move = (e) => {
      moved = true;
      const p = pointWorld(e);
      temp.datos.hasta = { x: p.x, y: p.y };
      paintArrow(draft, temp, get);
    };
    dragOnce(move, (e) => {
      draft.remove();
      if (!moved) return;
      const hasta = endAt(e, start.id);
      createAndSelect([{ op: 'crear', tipo: 'flecha', datos: { desde: start, hasta } }]);
    });
  }

  const endFromPoint = (event) => {
    const end = endAt(event);
    return end;
  };

  // Mover una punta de una flecha ya hecha.
  function startEndDrag(event, el, which) {
    event.preventDefault();
    event.stopPropagation();
    const node = nodes.get(el.id);
    const before = el.datos[which];
    gesture = { ids: new Set([el.id]) };
    use(el.id, true);
    const move = (e) => {
      const p = pointWorld(e);
      el.datos = { ...el.datos, [which]: { x: p.x, y: p.y } };
      paintArrow(node, el, get);
    };
    dragOnce(move, (e) => {
      gesture = null;
      use(el.id, false);
      const other = which === 'desde' ? el.datos.hasta?.id : el.datos.desde?.id;
      const end = endAt(e, other);
      el.datos = { ...el.datos, [which]: before };
      apply([{ op: 'cambiar', id: el.id, datos: { [which]: end } }]);
    });
  }

  async function createAndSelect(ops, { write = false } = {}) {
    const r = await apply(ops);
    const ids = (r?.hechas ?? []).map((h) => h.id).filter((id) => id && els.has(id));
    if (!ids.length) return ids;
    if (ops.length === 1) select(ids);
    else select(ids, { focus: false });
    if (write && ids.length === 1) beginText(els.get(ids[0]));
    return ids;
  }

  // Dónde poner algo sin ratón: en el centro de lo que se ve, en el hueco libre.
  function centerOfFree(w, h) {
    const free = deps.free();
    const c = deps.camera();
    const k = scale();
    return { x: snapX(c.x + (free.x + free.w / 2) / k - w / 2), y: snapY(c.y + (free.y + free.h / 2) / k - h / 2) };
  }

  function createFromItem(item, x, y, size) {
    if (item.template) return createAndSelect(templateOps(item.template, x, y));
    const [dw, dh] = sizeOf(item.tipo, item.datos?.forma);
    const op = { op: 'crear', tipo: item.tipo, x: Math.round(x), y: Math.round(y), datos: item.datos };
    // Tablas y compuestos: su tamaño lo calcula el motor con lo que traen, salvo que se dibuje.
    if (size) Object.assign(op, size);
    else if (!['tabla', 'tablero', 'entidad', 'secuencia'].includes(item.tipo)) Object.assign(op, { w: dw, h: dh });
    return createAndSelect([op], { write: WRITES_AT_ONCE.has(item.tipo) });
  }

  function arm(item) {
    armed = item;
    viewport.classList.add('is-armed');
    status(`Elige dónde poner ${item.label.toLowerCase()}: clic en el lienzo, o arrastra para darle tamaño. Escape cancela.`, { keep: true });
    chrome.armed(item);
  }
  function disarm() {
    armed = null;
    viewport.classList.remove('is-armed');
    status('');
    chrome.armed(null);
  }

  // Colocar lo elegido: va antes que cualquier otro gesto del lienzo.
  viewport.addEventListener('pointerdown', (event) => {
    if (!armed || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const item = armed;
    disarm();
    if (item.tipo === 'imagen') return chrome.pickImage(pointWorld(event));
    if (item.tipo === 'flecha') return startArrow(event);
    const start = pointWorld(event);
    const ghost = document.createElement('div');
    ghost.className = 'cv-ghost';
    surface.append(ghost);
    let box = null;
    const move = (e) => {
      const p = pointWorld(e);
      if (Math.hypot(p.x - start.x, p.y - start.y) * scale() < GIVE) return;
      const x = snapX(Math.min(start.x, p.x));
      const y = snapY(Math.min(start.y, p.y));
      box = { x, y, w: Math.max(MIN, snapX(Math.max(start.x, p.x)) - x), h: Math.max(MIN, snapY(Math.max(start.y, p.y)) - y) };
      Object.assign(ghost.style, { left: `${box.x + origin.x}px`, top: `${box.y + origin.y}px`, width: `${box.w}px`, height: `${box.h}px` });
    };
    dragOnce(move, () => {
      ghost.remove();
      if (box) return createFromItem(item, box.x, box.y, { w: box.w, h: box.h });
      const [w, h] = item.template ? [0, 0] : sizeOf(item.tipo, item.datos?.forma);
      return createFromItem(item, snapX(start.x - w / 2), snapY(start.y - h / 2));
    });
    return undefined;
  }, true);

  surface.addEventListener('pointerdown', (event) => {
    if (event.button !== 0) return;
    if (event.target.closest?.('.desk-card')) {
      if (selection.size) clearSelection();
      return;
    }
    const nodeEl = event.target.closest?.('.cv-el');
    if (!nodeEl) return;
    const el = els.get(nodeEl.dataset.id);
    if (!el) return;
    if (event.target.closest('[contenteditable], input, textarea, select, .cv-tcard-form, .cv-col-add, .cv-ref')) return;
    const handle = event.target.dataset?.handle;
    if (handle) return startResize(event, el, handle);
    const port = event.target.dataset?.port;
    if (port) return startArrow(event, { id: el.id, ancla: port });
    const end = event.target.dataset?.end;
    if (end) return startEndDrag(event, el, end);
    if (event.shiftKey) {
      const next = new Set(selection);
      if (next.has(el.id)) next.delete(el.id);
      else next.add(el.id);
      select([...next], { focus: false });
      return undefined;
    }
    if (!selection.has(el.id)) select([el.id], { focus: false });
    nodeEl.focus({ preventScroll: true });
    // Las tablas y los tableros se mueven por su cabecera: dentro se trabaja.
    const compound = ['tabla', 'tablero', 'entidad', 'secuencia', 'linea-tiempo', 'codigo'].includes(el.tipo);
    if (compound && !event.target.closest('.cv-grip')) return undefined;
    if (el.tipo === 'flecha') return undefined;
    event.preventDefault();
    startMove(event, el);
    return undefined;
  });

  // El fondo: un clic deja de seleccionar; Mayús y arrastrar selecciona por área. Lo llama notes.js
  // antes de mover el lienzo. `true`: ya está atendido, no hay que mover nada.
  function backgroundDown(event) {
    viewport.focus({ preventScroll: true });
    if (!event.shiftKey) {
      if (selection.size) clearSelection();
      return false;
    }
    event.preventDefault();
    const start = pointWorld(event);
    const box = document.createElement('div');
    box.className = 'cv-area';
    surface.append(box);
    let area = null;
    const move = (e) => {
      const p = pointWorld(e);
      area = { x: Math.min(start.x, p.x), y: Math.min(start.y, p.y), w: Math.abs(p.x - start.x), h: Math.abs(p.y - start.y) };
      Object.assign(box.style, { left: `${area.x + origin.x}px`, top: `${area.y + origin.y}px`, width: `${area.w}px`, height: `${area.h}px` });
    };
    dragOnce(move, () => {
      box.remove();
      if (!area) return;
      const hit = [...els.values()].filter((e) => capas.get(e.capa)?.visible !== false && intersects(rectOf(e, get), area)).map((e) => e.id);
      select(hit, { focus: false });
      status(hit.length ? `${hit.length} seleccionados` : 'Nada seleccionado');
    });
    return true;
  }

  // ------------------------------------------------------------ escribir

  const TEXT_FIELD = { texto: 'texto', figura: 'texto', adhesiva: 'texto', marco: 'titulo', flecha: 'texto', tabla: 'titulo', tablero: 'titulo' };

  // Un campo simple en su sitio: al salir se guarda; con Escape, no.
  function editInline(host, value, commit, { multiline = false, onEnd = () => {} } = {}) {
    const input = document.createElement('div');
    input.className = 'cv-edit';
    input.contentEditable = 'plaintext-only';
    input.textContent = value;
    input.setAttribute('role', 'textbox');
    input.spellcheck = true;
    if (multiline) input.setAttribute('aria-multiline', 'true');
    host.replaceChildren(input);
    input.focus();
    const sel = getSelection();
    sel.selectAllChildren(input);
    sel.collapseToEnd();
    let done = false;
    const finish = (save) => {
      if (done) return;
      done = true;
      const text = input.innerText.replace(/\n$/, '');
      onEnd();
      if (save && text !== value) commit(text);
      else commit(null);
    };
    input.addEventListener('keydown', (event) => {
      event.stopPropagation();
      if (event.key === 'Escape') {
        event.preventDefault();
        finish(false);
      } else if (event.key === 'Enter' && (!multiline || event.ctrlKey || event.metaKey)) {
        event.preventDefault();
        finish(true);
      }
    });
    input.addEventListener('blur', () => finish(true));
    return { finish };
  }

  // Lo que mide de alto lo escrito en un cuadro, en la retícula; null si ya cabe.
  function fitHeight(node, el) {
    const box = node.querySelector('.cv-edit') ?? node.querySelector('.cv-md');
    if (!box) return null;
    const pad = el.tipo === 'adhesiva' ? 24 : 8;
    const needed = Math.ceil((box.scrollHeight + pad) / 22) * 22;
    return needed > el.h ? needed : null;
  }

  function beginText(el) {
    if (!el) return;
    const field = TEXT_FIELD[el.tipo];
    if (el.tipo === 'codigo') return editCode(el);
    // El punto de inicio o fin de un diagrama no lleva texto.
    if (!field || el.datos?.forma === 'inicio') return;
    if (!nodes.has(el.id)) mount(el);
    const node = nodes.get(el.id);
    use(el.id, true);
    let host;
    if (el.tipo === 'flecha') {
      host = node.querySelector('.cv-arrow-label');
      if (!host) {
        host = document.createElement('div');
        host.className = 'cv-arrow-label';
        node.append(host);
        const geo = paintArrow(node, el, get);
        const box = { x: parseFloat(node.style.left) - origin.x, y: parseFloat(node.style.top) - origin.y };
        if (geo) Object.assign(host.style, { left: `${geo.mid.x - box.x}px`, top: `${geo.mid.y - box.y}px` });
      }
    } else if (el.tipo === 'marco') {
      host = node.querySelector('.cv-frame-title');
      host.classList.remove('is-empty');
    } else if (el.tipo === 'tabla' || el.tipo === 'tablero') {
      // Una tabla sin título no tiene cabecera: el título se le pone desde la barra.
      host = node.querySelector('.cv-card-title');
      if (!host) {
        use(el.id, false);
        return undefined;
      }
    } else if (el.tipo === 'figura') {
      host = node.querySelector('.cv-label') ?? node.querySelector('.cv-body');
    } else {
      host = node.querySelector('.cv-body');
    }
    const multiline = !['marco', 'flecha', 'tabla', 'tablero'].includes(el.tipo);
    const ed = editInline(
      host,
      el.datos[field] ?? '',
      (text) => {
        const id = el.id;
        editing = null;
        use(id, false);
        const body = node.querySelector(':scope > .cv-body');
        if (body) delete body.dataset.sig;
        // Un cuadro crece con lo que se escribe en él: el texto nunca se corta sin avisar.
        const grow = text != null && (el.tipo === 'texto' || el.tipo === 'adhesiva') ? fitHeight(node, el) : null;
        if (text != null) apply([{ op: 'cambiar', id, datos: { [field]: text }, ...(grow ? { h: grow } : {}) }]);
        const now = els.get(id);
        if (now) paint(now);
      },
      { multiline },
    );
    editing = { id: el.id, finish: ed.finish };
    return undefined;
  }

  function editCode(el) {
    const node = nodes.get(el.id);
    const pre = node?.querySelector('.cv-code');
    if (!pre) return;
    use(el.id, true);
    const area = document.createElement('textarea');
    area.className = 'cv-code-edit';
    area.value = el.datos.texto ?? '';
    area.spellcheck = false;
    area.setAttribute('aria-label', 'Código');
    pre.replaceWith(area);
    area.focus();
    let done = false;
    const finish = (save) => {
      if (done) return;
      done = true;
      editing = null;
      use(el.id, false);
      const body = node.querySelector(':scope > .cv-body');
      delete body.dataset.sig;
      if (save && area.value !== el.datos.texto) apply([{ op: 'cambiar', id: el.id, datos: { texto: area.value } }]);
      else paint(el);
    };
    area.addEventListener('keydown', (event) => {
      event.stopPropagation();
      if (event.key === 'Escape') finish(false);
      if (event.key === 'Tab') {
        event.preventDefault();
        area.setRangeText('  ', area.selectionStart, area.selectionEnd, 'end');
      }
    });
    area.addEventListener('blur', () => finish(true));
    editing = { id: el.id, finish };
  }

  surface.addEventListener('dblclick', (event) => {
    const nodeEl = event.target.closest?.('.cv-el');
    const el = nodeEl && els.get(nodeEl.dataset.id);
    if (!el || event.target.closest('[contenteditable], input, textarea')) return;
    if (['entidad', 'secuencia', 'linea-tiempo', 'referencia', 'imagen'].includes(el.tipo)) return chrome.openEditor(el);
    // En tablas y tableros, doble clic dentro es de sus celdas y tarjetas; en la cabecera, el título.
    if ((el.tipo === 'tabla' || el.tipo === 'tablero') && !event.target.closest('.cv-card-head')) return undefined;
    beginText(el);
    return undefined;
  });

  // Los enlaces de un cuadro de texto abren lo suyo y no navegan la app.
  surface.addEventListener('click', (event) => {
    const a = event.target.closest?.('.cv-md a');
    if (!a) return;
    event.preventDefault();
    deps.onLink(a);
  });

  // ------------------------------------------------------------ teclado

  const inField = (t) => t.closest?.('input, textarea, select, [contenteditable], .desk-card, dialog');

  function deleteSelection() {
    if (!selection.size) return;
    const ids = [...selection];
    apply(ids.map((id) => ({ op: 'borrar', id })));
    status(ids.length === 1 ? 'Borrado. Ctrl+Z lo devuelve.' : `${ids.length} borrados. Ctrl+Z los devuelve.`);
  }

  function nudge(dx, dy) {
    const { ids } = withContents([...selection]);
    const ops = [];
    for (const id of ids) {
      const el = els.get(id);
      if (el && el.tipo !== 'flecha') ops.push({ op: 'cambiar', id, x: el.x + dx, y: el.y + dy });
    }
    apply(ops);
    const first = els.get([...selection][0]);
    if (first && first.tipo !== 'flecha') deps.revealRect({ x: first.x + dx, y: first.y + dy, w: first.w, h: first.h }, { smooth: false });
  }

  // Tab recorre los elementos por su posición. Al pasar del último (o de antes del primero) sale
  // del lienzo: no atrapa el teclado. `false`: que el navegador lleve el foco a lo siguiente.
  function tabTo(dir) {
    const items = readingOrder([...els.values()].filter((e) => e.tipo !== 'flecha' && capas.get(e.capa)?.visible !== false));
    const current = [...selection][0];
    const at = items.findIndex((e) => e.id === current);
    // Desde el lienzo en sí se entra por lo seleccionado o, si no hay nada, por el primero.
    const entering = document.activeElement === viewport;
    const next = entering ? (dir > 0 ? (items[at] ?? items[0]) : null) : items[at + dir];
    if (!next) {
      if (selection.size) clearSelection();
      return false;
    }
    select([next.id]);
    deps.revealRect(next, { smooth: false });
    return true;
  }

  function reorder(a) {
    apply([...selection].map((id) => ({ op: 'apilar', id, a })));
  }

  document.addEventListener('keydown', (event) => {
    if (viewport.hidden) return;
    const t = event.target;
    const inCanvas = viewport.contains(t) || t === viewport;
    if (!inCanvas || inField(t)) {
      if (event.key === 'Escape' && armed) {
        disarm();
        status('Cancelado.');
      }
      return;
    }
    const mod = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    if (selection.size === 1) {
      const el = els.get([...selection][0]);
      if (el && t.closest?.('.cv-table td, .cv-tcard') && grid.key(event, el)) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
    }
    if (mod && key === 'z') {
      event.preventDefault();
      if (event.shiftKey) redo();
      else undo();
    } else if (mod && key === 'y') {
      event.preventDefault();
      redo();
    } else if (mod && key === 'a') {
      event.preventDefault();
      select([...els.values()].filter((e) => capas.get(e.capa)?.visible !== false).map((e) => e.id), { focus: false });
    } else if (mod && key === 'd') {
      event.preventDefault();
      duplicate();
    } else if (mod && (event.key === ']' || event.key === '[')) {
      event.preventDefault();
      reorder(event.key === ']' ? 'frente' : 'fondo');
    } else if (mod) {
      return;
    } else if (event.key === 'Escape') {
      if (armed) disarm();
      else {
        clearSelection();
        viewport.focus({ preventScroll: true });
      }
    } else if (event.key === 'Delete' || event.key === 'Backspace') {
      event.preventDefault();
      deleteSelection();
    } else if (event.key === 'Tab') {
      if (tabTo(event.shiftKey ? -1 : 1)) event.preventDefault();
    } else if (event.key === 'Enter' || event.key === 'F2') {
      const el = els.get([...selection][0]);
      if (!el) return;
      event.preventDefault();
      if (grid.enter(el)) return;
      if (['entidad', 'secuencia', 'linea-tiempo', 'referencia', 'imagen'].includes(el.tipo)) chrome.openEditor(el);
      else beginText(el);
    } else if (event.key.startsWith('Arrow') && selection.size) {
      event.preventDefault();
      const step = 22 * (event.shiftKey ? 5 : 1);
      const dx = event.key === 'ArrowLeft' ? -step : event.key === 'ArrowRight' ? step : 0;
      const dy = event.key === 'ArrowUp' ? -step : event.key === 'ArrowDown' ? step : 0;
      nudge(dx, dy);
    } else {
      const quick = { t: 'texto', r: 'figura', f: 'flecha', m: 'marco' }[key];
      if (quick) {
        event.preventDefault();
        arm(catalogItem(quick));
      }
    }
  }, true);

  // ------------------------------------------------------------ copiar y pegar

  // Las referencias dentro de lo copiado se cambian: dos cajas y su flecha siguen unidas.
  function cloneOps(items, dx, dy) {
    const inside = new Set(items.map((e) => e.id));
    const remapEnd = (end) => {
      if (end?.id == null) return end;
      if (inside.has(end.id)) return { ...end, id: `r_${end.id}` };
      return end;
    };
    return items.map((e) => {
      const datos = e.tipo === 'flecha' ? { ...e.datos, desde: remapEnd(e.datos.desde), hasta: remapEnd(e.datos.hasta) } : e.datos;
      return { op: 'crear', ref: `r_${e.id}`, tipo: e.tipo, x: e.x + dx, y: e.y + dy, w: e.w, h: e.h, datos };
    });
  }

  const selectedItems = () => {
    const { ids } = withContents([...selection]);
    return [...ids].map((id) => structuredClone(els.get(id))).filter(Boolean).sort((a, b) => (a.tipo === 'flecha') - (b.tipo === 'flecha'));
  };

  function duplicate() {
    const items = selectedItems();
    if (items.length) createAndSelect(cloneOps(items, 44, 44));
  }

  function plainText(items) {
    return items
      .map((e) => e.datos?.texto ?? e.datos?.titulo ?? e.datos?.nombre ?? '')
      .filter(Boolean)
      .join('\n');
  }

  document.addEventListener('copy', (event) => {
    if (viewport.hidden || !viewport.contains(event.target) || inField(event.target) || !selection.size || event.target.closest?.('.cv-table td')) return;
    const items = selectedItems();
    const text = plainText(items) || `${items.length} elementos de nexo`;
    clip = { text, items };
    event.clipboardData?.setData('text/plain', text);
    event.preventDefault();
  });
  document.addEventListener('cut', (event) => {
    if (viewport.hidden || !viewport.contains(event.target) || inField(event.target) || !selection.size || event.target.closest?.('.cv-table td')) return;
    const items = selectedItems();
    const text = plainText(items) || `${items.length} elementos de nexo`;
    clip = { text, items };
    event.clipboardData?.setData('text/plain', text);
    event.preventDefault();
    deleteSelection();
  });
  document.addEventListener('paste', async (event) => {
    if (viewport.hidden || !(viewport.contains(event.target) || event.target === document.body) || inField(event.target) || event.target.closest?.('.cv-table td')) return;
    const data = event.clipboardData;
    const file = [...(data?.items ?? [])].find((i) => i.kind === 'file' && i.type.startsWith('image/'))?.getAsFile();
    const text = data?.getData('text/plain') ?? '';
    if (!file && !text) return;
    event.preventDefault();
    if (file) return chrome.placeImage(file, null);
    if (clip && text === clip.text) {
      const b = boundsOfRects(clip.items.filter((e) => e.tipo !== 'flecha').map((e) => ({ x: e.x, y: e.y, w: e.w, h: e.h })));
      if (!b) return undefined;
      const at = centerOfFree(b.w, b.h);
      return createAndSelect(cloneOps(clip.items, at.x - b.x, at.y - b.y));
    }
    // Una tabla copiada de Excel llega como tabla; lo demás, como un cuadro de texto.
    if (text.includes('\t')) {
      const { parseTSV } = await import('./canvas-table.js');
      const rows = parseTSV(text);
      const at = centerOfFree(400, 200);
      return createAndSelect([{ op: 'crear', tipo: 'tabla', x: at.x, y: at.y, datos: { columnas: rows[0], filas: rows.slice(1) } }]);
    }
    const at = centerOfFree(264, 66);
    return createAndSelect([{ op: 'crear', tipo: 'texto', x: at.x, y: at.y, w: 264, h: 66, datos: { texto: text } }]);
  });

  // ------------------------------------------------------------ alinear

  function align(modo) {
    const rects = [...selection].map((id) => els.get(id)).filter((e) => e && e.tipo !== 'flecha');
    apply(aligned(rects, modo).map((r) => ({ op: 'cambiar', ...r })));
  }
  function distribute(eje) {
    const rects = [...selection].map((id) => els.get(id)).filter((e) => e && e.tipo !== 'flecha');
    apply(distributed(rects, eje).map((r) => ({ op: 'cambiar', ...r })));
  }

  // ------------------------------------------------------------ piezas

  const api = {
    surface,
    get,
    all: () => [...els.values()],
    capas: () => [...capas.values()],
    nodeOf: (id) => nodes.get(id),
    isSelected: (id) => selection.has(id),
    selected: () => [...selection].map((id) => els.get(id)).filter(Boolean),
    select,
    focusEl,
    scale,
    apply,
    use,
    status,
    ask,
    repaint: (id) => {
      const el = els.get(id);
      if (!el || !nodes.has(id)) return;
      const body = nodes.get(id).querySelector(':scope > .cv-body');
      if (body) delete body.dataset.sig;
      paint(el);
    },
    barChanged: () => chrome.selectionChanged(),
    editInline(host, value, commit) {
      const id = host.closest('.cv-el')?.dataset.id;
      use(id, true);
      const ed = editInline(host, value, (text) => {
        editing = null;
        use(id, false);
        if (text != null && text !== value) Promise.resolve(commit(text)).finally(() => api.repaint(id));
        else api.repaint(id);
      });
      editing = { id, finish: ed.finish };
    },
    beginText: (id) => beginText(els.get(id)),
  };
  const grid = createGrid(api);
  const chrome = createChrome({
    ...api,
    grid,
    ask,
    fail,
    invoke,
    arm,
    disarm,
    isArmed: () => armed,
    createFromItem,
    centerOfFree,
    createAndSelect,
    duplicate,
    deleteSelection,
    reorder,
    align,
    distribute,
    undo,
    redo,
    canUndo: () => undoStack.length > 0,
    canRedo: () => redoStack.length > 0,
    goTo,
    convertToNote: deps.convertToNote,
    notePaths: deps.notePaths,
    viewport,
    screenOf: (rect) => {
      const r = viewport.getBoundingClientRect();
      const c = deps.camera();
      const k = scale();
      return { x: r.left + (rect.x - c.x) * k, y: r.top + (rect.y - c.y) * k, w: rect.w * k, h: rect.h * k };
    },
    rectOf: (el) => rectOf(el, get),
  });

  // Lleva la vista a un elemento o a una capa entera. Solo cuando el usuario lo pide.
  function goTo(id) {
    let rect = null;
    if (capas.has(id)) rect = boundsOfRects([...els.values()].filter((e) => e.capa === id && e.tipo !== 'flecha').map((e) => rectOf(e, get)));
    else if (els.has(id)) rect = rectOf(els.get(id), get);
    if (!rect) return false;
    deps.revealRect(rect, { smooth: true });
    return true;
  }

  return {
    get desk() {
      return desk;
    },
    // Se abre un escritorio: su lienzo. Lo del anterior ya está guardado en el motor.
    async load(next) {
      // Lo que se estaba escribiendo se guarda antes de cambiar de lienzo.
      editing?.finish(true);
      grid.finish();
      gesture = null;
      await queue;
      for (const id of [...nodes.keys()]) unmount(id);
      for (const n of layerNodes.values()) n.remove();
      layerNodes.clear();
      els.clear();
      capas.clear();
      arrowsOf.clear();
      selection.clear();
      undoStack = [];
      redoStack = [];
      desk = next;
      if (!next) return chrome.changed();
      let data;
      try {
        data = await invoke('canvas_load', { desk: next });
      } catch (error) {
        fail(error);
        return chrome.changed();
      }
      if (desk !== next) return undefined;
      for (const a of data.avisos ?? []) fail(a);
      for (const c of data.lienzo.capas) capas.set(c.id, c);
      for (const e of data.lienzo.elementos) {
        els.set(e.id, e);
        if (e.tipo === 'flecha') indexArrow(e, true);
      }
      syncLayers();
      queueVisible();
      deps.onChange();
      chrome.changed();
      return undefined;
    },
    // Lo que hizo el agente. Si es de otro escritorio, ya está guardado y se verá al abrirlo.
    onChanged(payload) {
      if (payload.cartas?.length && payload.escritorio === desk) {
        const back = deps.moveCards(payload.cartas);
        if (payload.deshacer?.length || back.length) {
          undoStack.push({ ops: payload.deshacer ?? [], cards: back, cardsBack: payload.cartas });
          redoStack = [];
        }
      } else if (payload.deshacer?.length && payload.escritorio === desk) {
        undoStack.push({ ops: payload.deshacer });
        redoStack = [];
      }
      if (payload.escritorio === desk) absorb(payload.cambios, { fromAgent: true });
    },
    goTo,
    queueVisible,
    themeChanged() {
      for (const id of nodes.keys()) api.repaint(id);
      deps.onChange();
    },
    // Lo que ocupa cada cosa, para el minimapa y para no perderse.
    rects: () => [...els.values()].filter((e) => e.tipo !== 'flecha' && capas.get(e.capa)?.visible !== false).map((e) => ({ x: e.x, y: e.y, w: e.w, h: e.h })),
    mapItems: () =>
      [...els.values()]
        .filter((e) => e.tipo !== 'flecha' && capas.get(e.capa)?.visible !== false)
        .map((e) => ({ x: e.x, y: e.y, w: e.w, h: e.h, agent: e.capa !== USER_LAYER, frame: e.tipo === 'marco' })),
    live: () => {
      const capa = [...capas.values()].find((c) => c.estado === 'dibujando');
      if (!capa) return null;
      const box = boundsOfRects([...els.values()].filter((e) => e.capa === capa.id && e.tipo !== 'flecha').map((e) => rectOf(e, get)));
      return box && { ...box, capa: capa.id };
    },
    count: () => els.size,
    apply,
    deselect: clearSelection,
    backgroundDown,
    armed: () => armed,
    setVisible(on) {
      chrome.setVisible(on);
      if (!on) {
        if (armed) disarm();
        editing?.finish(true);
        grid.finish();
      } else queueVisible();
    },
  };
}
