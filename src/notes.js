// Apartado Notas: la carta de la izquierda muestra temas, subtemas y notas de
// wiki/notas/, y el centro es un escritorio donde cada nota abierta es una carta que se
// mueve, se redimensiona y se apila. Las notas se escriben directamente en la carta, línea a
// línea (note-editor.js), y se guardan en Markdown (notas.rs); la disposición del
// escritorio, en .nexo/escritorio.json (Claude Code también la edita).

import { reconcile } from './dom.js';
import { createNoteEditor, isoToday, parseDate, relativeDay, taskCount } from './note-editor.js';

const $ = (id) => document.getElementById(id);

const icon = (paths, size = 16) =>
  `<svg width="${size}" height="${size}" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
const ICON = {
  nota: '<path d="M5 2.5h7l3 3v12H5z"/><path d="M7.5 9h5M7.5 12h5M7.5 15h3"/>',
  lista: '<rect x="3" y="4" width="4" height="4" rx="1"/><path d="m3.8 13.6 1.2 1.2 2.2-2.4M10 6h7M10 13.5h7"/>',
  tema: '<path d="M2.5 6V15.5h15V7.5H9.5L8 5.5H2.5z"/>',
  chevron: '<path d="m8 5 5 5-5 5"/>',
  pencil: '<path d="M13.5 3.5l3 3L7 16H4v-3z"/>',
  trash: '<path d="M4 6h12M8 6V4h4v2M6 6l1 10h6l1-10"/>',
  close: '<path d="m5 5 10 10M15 5 5 15"/>',
  markdown: '<path d="m7 6-4 4 4 4M13 6l4 4-4 4"/>',
  front: '<rect x="7" y="7" width="10" height="10" rx="2"/><path d="M4 13V5a1 1 0 0 1 1-1h8"/>',
  back: '<rect x="3" y="3" width="10" height="10" rx="2"/><path d="M16 7v8a1 1 0 0 1-1 1H7" stroke-dasharray="2 2"/>',
};

const MIN_W = 220;
const MIN_H = 150;
const GAP = 14;
const CARD_W = 380;
const CARD_H = 440;

// «2026-10-02 18:30» → «hoy 18:30», «ayer 9:05» o «2 oct 18:30».
function editedText(stamp) {
  const date = parseDate(stamp);
  const time = /\d{2}:\d{2}/.exec(stamp)?.[0] ?? '';
  if (!date) return stamp;
  const rel = relativeDay(date);
  const day = rel === 'hoy' || rel === 'ayer' ? rel : date.toLocaleDateString('es', { day: 'numeric', month: 'short' });
  return `${day} ${time}`.trim();
}
const shortDate = (date) => date.toLocaleDateString('es', { weekday: 'short', day: 'numeric', month: 'short' });
// El backend guarda el cuerpo con un salto final y sin líneas vacías al principio.
const same = (a, b) => (a ?? '').replace(/^\n+|\s+$/g, '') === (b ?? '').replace(/^\n+|\s+$/g, '');

export function createNotes({ invoke, ask, renderMarkdown, fail, onLink }) {
  let tree = null;
  let current = '';
  let loaded = false;
  let createMode = null; // 'nota' | 'tema'
  const cards = new Map(); // ruta → carta
  const surface = $('desk-surface');

  // ------------------------------------------------------------ árbol y panel

  function findTopic(path, node = tree) {
    if (!node) return null;
    if (node.path === path) return node;
    for (const t of node.topics) {
      const found = findTopic(path, t);
      if (found) return found;
    }
    return null;
  }

  function chain(path) {
    const out = [];
    let node = tree;
    while (node) {
      out.push(node);
      if (node.path === path) break;
      node = node.topics.find((t) => path === t.path || path.startsWith(`${t.path}/`));
    }
    return out;
  }

  const countNotes = (t) => t.notes.length + t.topics.reduce((n, s) => n + countNotes(s), 0);

  async function loadTree() {
    try {
      tree = await invoke('notes_tree');
      loaded = true;
    } catch (error) {
      tree = null;
      fail(error);
    }
    if (tree && !findTopic(current)) current = '';
    renderPanel();
  }

  function rowShell(className) {
    const li = document.createElement('li');
    const row = document.createElement('div');
    row.className = `notes-row ${className}`;
    li.append(row);
    return { li, row };
  }

  function actionButton(paths, label, handler) {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'row-action';
    b.innerHTML = icon(paths, 14);
    b.setAttribute('aria-label', label);
    b.title = label;
    b.addEventListener('click', (event) => {
      event.stopPropagation();
      handler();
    });
    return b;
  }

  function topicRow(topic) {
    const { li, row } = rowShell('topic-row');
    const main = document.createElement('button');
    main.type = 'button';
    main.className = 'row-main';
    const total = countNotes(topic);
    const parts = [];
    if (topic.topics.length) parts.push(`${topic.topics.length} ${topic.topics.length === 1 ? 'subtema' : 'subtemas'}`);
    parts.push(`${topic.notes.length} ${topic.notes.length === 1 ? 'nota' : 'notas'}`);
    main.innerHTML = `<span class="row-icon">${icon(ICON.tema)}</span>
      <span class="row-text"><span class="row-title"></span><span class="row-meta"></span></span>
      <span class="topic-count"></span><span class="row-chevron">${icon(ICON.chevron, 14)}</span>`;
    main.querySelector('.row-title').textContent = topic.title;
    main.querySelector('.row-meta').textContent = parts.join(' · ');
    main.querySelector('.topic-count').textContent = String(total);
    main.title = `Entrar en ${topic.title}`;
    main.addEventListener('click', () => goTo(topic.path));

    const actions = document.createElement('div');
    actions.className = 'row-actions';
    actions.append(
      actionButton(ICON.pencil, `Renombrar ${topic.title}`, () => renameTopic(row, topic)),
      actionButton(ICON.trash, `Borrar ${topic.title}`, () => remove(topic.path, topic.title, true)),
    );
    row.append(main, actions);
    return li;
  }

  const startOfToday = () => new Date(new Date().setHours(0, 0, 0, 0));

  function noteMeta(note) {
    const parts = [];
    const date = parseDate(note.date);
    if (date) parts.push(`${shortDate(date)} · ${relativeDay(date)}`);
    if (note.total) parts.push(`${note.done}/${note.total}`);
    if (!parts.length && note.preview) parts.push(note.preview);
    return parts.join(' · ');
  }

  function noteRow(note) {
    const { li, row } = rowShell('note-row');
    row.classList.toggle('is-open', cards.has(note.path));
    const date = parseDate(note.date);
    if (date && date < startOfToday() && note.done < note.total) row.classList.add('overdue');
    const main = document.createElement('button');
    main.type = 'button';
    main.className = 'row-main';
    main.innerHTML = `<span class="row-icon">${icon(note.total ? ICON.lista : ICON.nota)}</span>
      <span class="row-text"><span class="row-title"></span><span class="row-meta"><span class="row-meta-text"></span></span></span>
      <span class="row-open-dot" aria-hidden="true"></span>`;
    main.querySelector('.row-title').textContent = note.title;
    main.querySelector('.row-meta-text').textContent = noteMeta(note);
    main.title = cards.has(note.path) ? 'Abierta en el escritorio' : 'Abrir en el escritorio';
    main.addEventListener('click', () => openNote(note.path, { focus: true }));
    const actions = document.createElement('div');
    actions.className = 'row-actions';
    actions.append(actionButton(ICON.trash, `Borrar ${note.title}`, () => remove(note.path, note.title, false)));
    row.append(main, actions);
    return li;
  }

  function renderPanel() {
    const topic = (tree && findTopic(current)) || tree;
    $('notes-count').textContent = String(tree ? countNotes(tree) : 0);
    const crumbs = $('notes-crumbs');
    crumbs.replaceChildren();
    if (!topic) {
      $('notes-topics').replaceChildren();
      $('notes-notes').replaceChildren();
      $('notes-topics-label').hidden = $('notes-notes-label').hidden = true;
      $('notes-description').hidden = true;
      $('notes-empty').hidden = false;
      $('notes-empty-title').textContent = 'Elige tu vault';
      $('notes-empty-text').textContent = 'Las notas se guardan en wiki/notas/ dentro del vault.';
      return;
    }
    const path = chain(topic.path);
    path.forEach((node, i) => {
      if (i) {
        const sep = document.createElement('span');
        sep.className = 'crumb-sep';
        sep.textContent = '›';
        sep.setAttribute('aria-hidden', 'true');
        crumbs.append(sep);
      }
      const last = i === path.length - 1;
      const crumb = document.createElement(last ? 'span' : 'button');
      crumb.className = 'crumb';
      crumb.textContent = node.title;
      if (last) crumb.setAttribute('aria-current', 'page');
      else {
        crumb.type = 'button';
        crumb.addEventListener('click', () => goTo(node.path));
      }
      crumbs.append(crumb);
    });
    crumbs.hidden = path.length < 2;

    $('notes-description').textContent = topic.description;
    $('notes-description').hidden = !topic.description || !topic.path;
    // Solo cambian las filas que cambiaron: recargar el árbol (cada vez que se guarda una
    // nota) no hace parpadear la lista.
    reconcile($('notes-topics'), topic.topics, {
      key: (t) => t.path,
      sig: (t) => JSON.stringify([t.title, t.topics.length, t.notes.length, countNotes(t)]),
      build: topicRow,
    });
    reconcile($('notes-notes'), topic.notes, {
      key: (n) => n.path,
      sig: (n) => JSON.stringify([n, cards.has(n.path), isoToday()]),
      build: noteRow,
    });
    $('notes-topics-label').hidden = !topic.topics.length;
    $('notes-notes-label').hidden = !topic.notes.length;
    const empty = !topic.topics.length && !topic.notes.length;
    $('notes-empty').hidden = !empty;
    $('notes-empty-title').textContent = topic.path ? 'Este tema está vacío' : 'Aún no hay notas';
    $('notes-empty-text').textContent = topic.path
      ? 'Añade notas o subtemas aquí, o pídeselo a Claude Code.'
      : 'Crea una nota o un tema, o pídeselo a Claude Code.';
    if (createMode) $('notes-create-where').textContent = `en ${topic.title}`;
  }

  function goTo(path) {
    current = path;
    closeCreate();
    renderPanel();
    $('notes-scroll').scrollTop = 0;
  }

  function renameTopic(row, topic) {
    const title = row.querySelector('.row-title');
    const input = document.createElement('input');
    input.className = 'row-rename';
    input.value = topic.title;
    input.setAttribute('aria-label', 'Nuevo nombre del tema');
    const main = row.querySelector('.row-main');
    main.replaceWith(input);
    input.focus();
    input.select();
    let done = false;
    const finish = async (save) => {
      if (done) return;
      done = true;
      const value = input.value.trim();
      if (save && value && value !== topic.title) {
        try {
          await invoke('topic_rename', { path: topic.path, title: value });
        } catch (error) {
          fail(error);
        }
        await loadTree();
      } else {
        input.replaceWith(main);
        title.textContent = topic.title;
      }
    };
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') finish(true);
      if (event.key === 'Escape') finish(false);
    });
    input.addEventListener('blur', () => finish(true));
  }

  async function remove(path, title, isTopic) {
    const message = isTopic
      ? `¿Mandar el tema «${title}» a la papelera? Se irán también todos sus subtemas y notas.`
      : `¿Mandar la nota «${title}» a la papelera?`;
    if (!(await ask(message, { title: isTopic ? 'Borrar tema' : 'Borrar nota', kind: 'warning', okLabel: 'Borrar', cancelLabel: 'Cancelar' }))) return;
    try {
      await invoke('notes_delete', { path });
    } catch (error) {
      return fail(error);
    }
    for (const key of [...cards.keys()]) {
      if (key === path || key.startsWith(`${path}/`)) closeCard(key, { persist: false, save: false });
    }
    saveDesk();
    await loadTree();
  }

  // ------------------------------------------------------------ crear

  function openCreate(mode) {
    createMode = mode;
    const form = $('notes-create');
    form.hidden = false;
    const topic = findTopic(current) ?? tree;
    $('notes-create-label').textContent = mode === 'tema' ? (topic?.path ? 'Nuevo subtema' : 'Nuevo tema') : 'Nueva nota';
    $('notes-create-where').textContent = topic ? `en ${topic.title}` : '';
    const input = $('notes-create-input');
    input.placeholder = mode === 'tema' ? 'Nombre del tema' : 'Título (opcional)';
    $('notes-create-input-label').textContent = mode === 'tema' ? 'Nombre del tema' : 'Título de la nota';
    input.value = '';
    input.focus();
  }

  function closeCreate() {
    createMode = null;
    $('notes-create').hidden = true;
  }

  $('notes-new-note').addEventListener('click', () => (createMode === 'nota' ? closeCreate() : openCreate('nota')));
  $('notes-new-topic').addEventListener('click', () => (createMode === 'tema' ? closeCreate() : openCreate('tema')));
  $('notes-create-cancel').addEventListener('click', closeCreate);
  $('notes-create').addEventListener('keydown', (event) => {
    if (event.key === 'Escape') closeCreate();
  });
  $('notes-create').addEventListener('submit', async (event) => {
    event.preventDefault();
    const mode = createMode;
    const title = $('notes-create-input').value.trim() || (mode === 'nota' ? 'Sin título' : '');
    if (!title) return $('notes-create-input').focus();
    try {
      if (mode === 'tema') {
        const path = await invoke('topic_create', { parent: current, title });
        closeCreate();
        current = path;
        await loadTree();
      } else {
        const path = await invoke('note_create', { parent: current, title });
        closeCreate();
        await loadTree();
        // La nota se abre lista para escribir.
        await openNote(path, { focus: true });
      }
    } catch (error) {
      fail(error);
    }
  });

  // ------------------------------------------------------------ escritorio

  const deskSize = () => ({ w: surface.clientWidth, h: surface.clientHeight });
  const zValues = () => [...cards.values()].map((c) => c.z);

  function normalizeLayers() {
    [...cards.values()]
      .sort((a, b) => a.z - b.z)
      .forEach((card, i) => {
        card.z = i + 1;
        card.el.style.zIndex = String(card.z);
      });
  }

  // Pinta la carta donde la dejó el usuario. Nunca la recoloca ni la encoge por su
  // cuenta: si el escritorio se estrecha (al expandir o minimizar las cartas laterales),
  // las cartas conservan posición y tamaño y el escritorio se desplaza.
  function place(card) {
    Object.assign(card.el.style, {
      left: `${card.x}px`,
      top: `${card.y}px`,
      width: `${card.w}px`,
      height: `${card.h}px`,
      zIndex: String(card.z),
    });
  }

  let saveTimer = 0;
  function saveDesk() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      const state = {
        cards: [...cards.values()].map(({ path, x, y, w, h, z }) => ({ path, x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h), z })),
      };
      invoke('desk_save', { state }).catch(fail);
    }, 350);
    renderDeskChrome();
  }

  function renderDeskChrome() {
    const n = cards.size;
    $('desk-empty').hidden = n > 0;
    $('desk-count').textContent = `${n} ${n === 1 ? 'carta' : 'cartas'}`;
    $('desk-arrange').disabled = n < 1;
    $('desk-close-all').disabled = n < 1;
  }

  function bringToFront(card) {
    const max = Math.max(0, ...zValues());
    if (card.z === max && zValues().filter((z) => z === max).length === 1) return;
    card.z = max + 1;
    normalizeLayers();
    saveDesk();
  }

  function sendToBack(card) {
    card.z = Math.min(...zValues()) - 1;
    normalizeLayers();
    saveDesk();
  }

  function cardButton(paths, label, handler, className = '') {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = `card-tool ${className}`;
    b.innerHTML = icon(paths, 14);
    b.setAttribute('aria-label', label);
    b.title = label;
    b.addEventListener('click', (event) => {
      event.stopPropagation();
      handler();
    });
    return b;
  }

  function createCardEl(card) {
    const el = document.createElement('article');
    el.className = 'desk-card';
    el.dataset.path = card.path;
    el.tabIndex = -1;
    el.innerHTML = `
      <header class="desk-card-head">
        <span class="desk-card-icon"></span>
        <div class="desk-card-heading">
          <h3 class="desk-card-title"></h3>
          <span class="desk-card-meta"></span>
        </div>
        <div class="desk-card-tools"></div>
      </header>
      <div class="desk-card-body"></div>
      <textarea class="desk-editor" spellcheck="true" aria-label="Markdown de la nota" hidden></textarea>
      <span class="desk-resize" aria-hidden="true"></span>`;
    card.el = el;
    const body = el.querySelector('.desk-card-body');
    const source = el.querySelector('.desk-editor');
    const tools = el.querySelector('.desk-card-tools');
    card.sourceButton = cardButton(ICON.markdown, 'Ver el Markdown', () => setSource(card, !card.source), 'card-source');
    tools.append(
      cardButton(ICON.pencil, 'Renombrar', () => renameCard(card)),
      card.sourceButton,
      cardButton(ICON.front, 'Traer al frente', () => bringToFront(card)),
      cardButton(ICON.back, 'Enviar al fondo', () => sendToBack(card)),
      cardButton(ICON.close, 'Cerrar del escritorio', () => closeCard(card.path)),
    );

    card.editor = createNoteEditor(body, {
      onChange(markdown) {
        card.draft = markdown;
        scheduleSave(card);
        scheduleHead(card);
      },
      onLink: (link) => onLink(link, card.path),
      onEscape: () => el.focus({ preventScroll: true }),
      renderTable: renderMarkdown,
    });

    // Al tocar una carta, pasa al frente.
    el.addEventListener('pointerdown', () => bringToFront(card), true);
    // Al salir de la carta se guarda lo pendiente y, si la nota cambió fuera, se recarga.
    el.addEventListener('focusout', (event) => {
      if (el.contains(event.relatedTarget)) return;
      flush(card).then(() => {
        if (card.stale && !busy(card)) {
          card.stale = false;
          reloadCard(card);
        }
      });
    });

    // Mover: se arrastra por la cabecera. Mientras se arrastra solo cambia `translate` (no
    // hay que recalcular la página en cada movimiento del ratón). No vale `transform`: la
    // animación de entrada de la carta lo fija al terminar y anularía el desplazamiento.
    const head = el.querySelector('.desk-card-head');
    head.addEventListener('pointerdown', (event) => {
      if (event.button !== 0 || event.target.closest('button, input, textarea')) return;
      event.preventDefault();
      bringToFront(card);
      const start = { px: event.clientX, py: event.clientY, x: card.x, y: card.y };
      el.classList.add('dragging');
      head.setPointerCapture(event.pointerId);
      const move = (e) => {
        const { w: W } = deskSize();
        card.x = Math.max(-card.w + 80, Math.min(start.x + e.clientX - start.px, W - 80 + surface.scrollLeft));
        card.y = Math.max(0, start.y + e.clientY - start.py);
        el.style.translate = `${card.x - start.x}px ${card.y - start.y}px`;
      };
      const up = () => {
        head.removeEventListener('pointermove', move);
        el.classList.remove('dragging');
        card.x = Math.max(0, card.x);
        el.style.translate = '';
        place(card);
        saveDesk();
      };
      head.addEventListener('pointermove', move);
      head.addEventListener('pointerup', up, { once: true });
      head.addEventListener('pointercancel', up, { once: true });
    });
    head.addEventListener('dblclick', (event) => {
      if (!event.target.closest('button')) renameCard(card);
    });

    // Redimensionar: por la esquina inferior derecha.
    const grip = el.querySelector('.desk-resize');
    grip.addEventListener('pointerdown', (event) => {
      if (event.button !== 0) return;
      event.preventDefault();
      event.stopPropagation();
      bringToFront(card);
      const start = { px: event.clientX, py: event.clientY, w: card.w, h: card.h };
      el.classList.add('resizing');
      grip.setPointerCapture(event.pointerId);
      const move = (e) => {
        const { w: W } = deskSize();
        card.w = Math.max(MIN_W, Math.min(start.w + e.clientX - start.px, W - card.x + surface.scrollLeft));
        card.h = Math.max(MIN_H, start.h + e.clientY - start.py);
        el.style.width = `${card.w}px`;
        el.style.height = `${card.h}px`;
      };
      const up = () => {
        grip.removeEventListener('pointermove', move);
        el.classList.remove('resizing');
        saveDesk();
      };
      grip.addEventListener('pointermove', move);
      grip.addEventListener('pointerup', up, { once: true });
      grip.addEventListener('pointercancel', up, { once: true });
    });

    // Markdown a la vista: para tablas largas o pegar una nota entera.
    source.addEventListener('input', () => {
      card.draft = source.value;
      scheduleSave(card);
      scheduleHead(card);
    });
    source.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') setSource(card, false);
    });
    return el;
  }

  // Cabecera de la carta: icono, título y «3/5 · editada hoy 18:30».
  function renderHead(card) {
    const { el, doc } = card;
    if (!el) return;
    const markdown = card.draft ?? doc?.body ?? '';
    const [done, total] = taskCount(markdown);
    el.querySelector('.desk-card-icon').innerHTML = icon(total ? ICON.lista : ICON.nota);
    const title = el.querySelector('.desk-card-title');
    if (!title.querySelector('input')) title.textContent = doc?.title ?? 'Cargando…';
    const meta = [];
    if (total) meta.push(`${done}/${total}`);
    const date = parseDate(doc?.date);
    if (date) meta.push(`${date.toLocaleDateString('es', { day: 'numeric', month: 'short' })} · ${relativeDay(date)}`);
    if (doc?.updated) meta.push(`editada ${editedText(doc.updated)}`);
    el.querySelector('.desk-card-meta').textContent = meta.join(' · ');
  }

  function scheduleHead(card) {
    if (card.headFrame) return;
    card.headFrame = requestAnimationFrame(() => {
      card.headFrame = 0;
      renderHead(card);
    });
  }

  const busy = (card) =>
    card.draft != null || card.editor.hasFocus() || card.el.querySelector('.desk-title-input') != null || document.activeElement === card.el.querySelector('.desk-editor');

  // Guardar: las escrituras de una carta van en fila, nunca dos a la vez (si no, una más
  // vieja podría llegar al disco después de una más nueva).
  function save(card, changes) {
    card.saving = (card.saving ?? Promise.resolve()).then(async () => {
      try {
        const doc = await invoke('note_save', { path: card.path, changes });
        card.doc = doc;
        renderHead(card);
        scheduleTreeReload();
      } catch (error) {
        fail(error);
      }
    });
    return card.saving;
  }

  function scheduleSave(card) {
    clearTimeout(card.timer);
    card.timer = setTimeout(() => flush(card), 600);
  }

  function flush(card) {
    clearTimeout(card.timer);
    const body = card.draft;
    card.draft = null;
    if (body == null || same(body, card.doc?.body)) return card.saving ?? Promise.resolve();
    return save(card, { body });
  }

  function setSource(card, on) {
    const body = card.el.querySelector('.desk-card-body');
    const source = card.el.querySelector('.desk-editor');
    if (on === card.source) return;
    if (!on) card.editor.setValue(source.value);
    else source.value = card.editor.getValue();
    card.source = on;
    body.hidden = on;
    source.hidden = !on;
    card.el.classList.toggle('source', on);
    card.sourceButton.title = on ? 'Volver a la nota' : 'Ver el Markdown';
    card.sourceButton.setAttribute('aria-label', card.sourceButton.title);
    card.sourceButton.setAttribute('aria-pressed', String(on));
    if (on) source.focus();
    else card.editor.focusEnd();
  }

  function renameCard(card) {
    if (!card.doc) return;
    const title = card.el.querySelector('.desk-card-title');
    if (title.querySelector('input')) return title.querySelector('input').focus();
    const input = document.createElement('input');
    input.className = 'desk-title-input';
    input.value = card.doc.title;
    input.setAttribute('aria-label', 'Título de la nota');
    let done = false;
    const finish = (keep) => {
      if (done) return;
      done = true;
      const value = input.value.trim();
      title.textContent = card.doc.title;
      if (keep && value && value !== card.doc.title) {
        title.textContent = value;
        save(card, { title: value });
      }
    };
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') {
        event.preventDefault();
        finish(true);
        card.editor.focusEnd();
      }
      if (event.key === 'Escape') finish(false);
    });
    input.addEventListener('blur', () => finish(true));
    title.replaceChildren(input);
    input.focus();
    input.select();
  }

  async function reloadCard(card) {
    try {
      const doc = await invoke('note_read', { path: card.path });
      card.doc = doc;
      if (!same(doc.body, card.editor.getValue())) {
        card.editor.setValue(doc.body);
        if (card.source) card.el.querySelector('.desk-editor').value = doc.body;
      }
      renderHead(card);
    } catch {
      // La nota ya no existe (borrada o movida fuera de nexo).
      closeCard(card.path, { save: false });
    }
  }

  // Sitio libre en la parte visible del escritorio para una carta nueva: la primera
  // posición (de arriba abajo y de izquierda a derecha) donde no tapa a ninguna otra. Si no
  // cabe con su tamaño, se prueba algo más pequeña; si tampoco, va en cascada.
  function newGeometry() {
    const { w: W, h: H } = deskSize();
    const left = surface.scrollLeft;
    const top = surface.scrollTop;
    const taken = [...cards.values()].filter((c) => c.el?.isConnected);
    const w = Math.max(MIN_W, Math.min(CARD_W, W - 2 * GAP));
    const h = Math.max(MIN_H, Math.min(CARD_H, H - 2 * GAP));
    const small = Math.max(MIN_H + 110, Math.min(h, Math.round(H * 0.42)));
    for (const [cw, ch] of [[w, h], [w, small], [Math.max(MIN_W + 60, Math.round(w * 0.8)), small]]) {
      for (let y = top + GAP; y + ch <= top + H - GAP; y += 24) {
        for (let x = left + GAP; x + cw <= left + W - GAP; x += 24) {
          const free = !taken.some((c) => x < c.x + c.w + GAP && x + cw + GAP > c.x && y < c.y + c.h + GAP && y + ch + GAP > c.y);
          if (free) return { x, y, w: cw, h: ch };
        }
      }
    }
    // Sin hueco: en cascada desde la esquina, para que se vea que hay una carta nueva.
    const k = cards.size;
    return { x: left + 24 + ((k * 28) % 220), y: top + 20 + ((k * 28) % 160), w, h };
  }

  async function openNote(path, { focus = false, geometry = null } = {}) {
    const existing = cards.get(path);
    if (existing) {
      bringToFront(existing);
      existing.el.classList.remove('pulse');
      void existing.el.offsetWidth;
      existing.el.classList.add('pulse');
      if (focus) existing.editor.focusEnd();
      return existing;
    }
    const card = {
      path,
      ...(geometry ?? newGeometry()),
      z: geometry?.z ?? Math.max(0, ...zValues()) + 1,
      doc: null,
      draft: null,
      source: false,
    };
    cards.set(path, card);
    surface.append(createCardEl(card));
    place(card);
    renderHead(card);
    if (!geometry) saveDesk();
    renderDeskChrome();
    try {
      card.doc = await invoke('note_read', { path });
    } catch (error) {
      closeCard(path, { save: false });
      if (!geometry) fail(error);
      return null;
    }
    if (!cards.has(path)) return null;
    card.editor.setValue(card.doc.body);
    renderHead(card);
    renderPanel();
    if (!geometry) card.el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    if (focus) card.editor.focusEnd();
    return card;
  }

  function closeCard(path, { persist = true, save: keep = true } = {}) {
    const card = cards.get(path);
    if (!card) return;
    if (keep) flush(card);
    else clearTimeout(card.timer);
    cancelAnimationFrame(card.headFrame);
    card.editor.destroy();
    card.el.remove();
    cards.delete(path);
    if (persist) saveDesk();
    renderDeskChrome();
    renderPanel();
  }

  // Ordenar: todas del mismo tamaño, en rejilla y sin encimarse.
  function arrange() {
    const list = [...cards.values()].sort((a, b) => a.y - b.y || a.x - b.x);
    const n = list.length;
    if (!n) return;
    const { w: W, h: H } = deskSize();
    let best = null;
    for (let cols = 1; cols <= n; cols++) {
      const rows = Math.ceil(n / cols);
      const w = (W - GAP * (cols + 1)) / cols;
      const h = (H - GAP * (rows + 1)) / rows;
      if (w < MIN_W || h < MIN_H) continue;
      const score = Math.min(w, h * 1.2);
      if (!best || score > best.score) best = { cols, w, h, score };
    }
    if (!best) {
      // No caben todas: columnas de ancho mínimo y el escritorio se desplaza hacia abajo.
      const cols = Math.max(1, Math.floor((W - GAP) / (MIN_W + 40 + GAP)));
      best = { cols, w: (W - GAP * (cols + 1)) / cols, h: 260 };
    }
    surface.classList.add('arranging');
    list.forEach((card, i) => {
      const col = i % best.cols;
      const row = Math.floor(i / best.cols);
      Object.assign(card, { x: GAP + col * (best.w + GAP), y: GAP + row * (best.h + GAP), w: best.w, h: best.h, z: i + 1 });
      place(card);
    });
    surface.scrollTo({ left: 0, top: 0 });
    setTimeout(() => surface.classList.remove('arranging'), 520);
    saveDesk();
  }

  $('desk-arrange').addEventListener('click', arrange);
  $('desk-close-all').addEventListener('click', () => {
    for (const path of [...cards.keys()]) closeCard(path, { persist: false });
    saveDesk();
  });

  // Aplica el escritorio guardado (al empezar o cuando Claude Code lo cambia).
  async function applyDesk() {
    let state;
    try {
      state = await invoke('desk_load');
    } catch {
      return;
    }
    const wanted = Array.isArray(state?.cards) ? state.cards.filter((c) => typeof c?.path === 'string') : [];
    const keep = new Set(wanted.map((c) => c.path));
    for (const path of [...cards.keys()]) if (!keep.has(path)) closeCard(path, { persist: false });
    for (const c of wanted) {
      const geometry = {
        x: Number(c.x) || 0,
        y: Number(c.y) || 0,
        w: Number(c.w) || CARD_W,
        h: Number(c.h) || CARD_H,
        z: Number(c.z) || 1,
      };
      const card = cards.get(c.path);
      if (card) {
        Object.assign(card, geometry);
        place(card);
      } else {
        await openNote(c.path, { geometry });
      }
    }
    normalizeLayers();
    renderDeskChrome();
    renderPanel();
  }

  let treeTimer = 0;
  function scheduleTreeReload() {
    clearTimeout(treeTimer);
    treeTimer = setTimeout(loadTree, 250);
  }

  return {
    get loaded() {
      return loaded;
    },
    // Se llama al entrar en la vista Notas (o al cambiar de vault).
    async activate({ reset = false } = {}) {
      if (reset) {
        for (const path of [...cards.keys()]) closeCard(path, { persist: false });
        current = '';
        loaded = false;
      }
      await loadTree();
      if (reset || !cards.size) await applyDesk();
      for (const card of cards.values()) place(card);
      renderDeskChrome();
    },
    // Cambios en wiki/ (rutas relativas a wiki/).
    onWikiChanged(changed) {
      if (!loaded) return;
      const notes = changed.filter((p) => p.startsWith('notas/')).map((p) => p.slice('notas/'.length));
      if (!notes.length) return;
      scheduleTreeReload();
      for (const path of notes) {
        const card = cards.get(path);
        if (!card || !card.doc) continue;
        // Si el usuario está escribiendo en ella, se recarga al salir de la carta.
        if (busy(card)) card.stale = true;
        else reloadCard(card);
      }
    },
    onDeskChanged() {
      if (loaded) applyDesk();
    },
    openNote,
    // Abre un tema (ruta relativa a wiki/notas/) en la carta de la izquierda.
    async openTopic(path) {
      if (!loaded) await loadTree();
      goTo(findTopic(path) ? path : '');
    },
    relayout() {
      for (const card of cards.values()) place(card);
    },
  };
}
