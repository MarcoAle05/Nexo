// Apartado Notas: la carta de la izquierda muestra temas, subtemas y notas de
// wiki/notas/, y el centro es un escritorio donde cada nota abierta es una carta que se
// mueve, se redimensiona y se apila. Todo se guarda en Markdown (notas.rs); la
// disposición del escritorio, en .nexo/escritorio.json (Claude Code también la edita).

const $ = (id) => document.getElementById(id);

const KIND_LABEL = { texto: 'Texto', lista: 'Lista', fecha: 'Fecha' };
const icon = (paths, size = 16) =>
  `<svg width="${size}" height="${size}" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
const ICON = {
  texto: '<path d="M5 2.5h7l3 3v12H5z"/><path d="M7.5 9h5M7.5 12h5M7.5 15h3"/>',
  lista: '<rect x="3" y="4" width="4" height="4" rx="1"/><path d="m3.8 13.6 1.2 1.2 2.2-2.4M10 6h7M10 13.5h7"/>',
  fecha: '<rect x="3" y="4.5" width="14" height="12.5" rx="2"/><path d="M3 8.5h14M7 2.5v4M13 2.5v4"/>',
  tema: '<path d="M2.5 6V15.5h15V7.5H9.5L8 5.5H2.5z"/>',
  chevron: '<path d="m8 5 5 5-5 5"/>',
  pencil: '<path d="M13.5 3.5l3 3L7 16H4v-3z"/>',
  trash: '<path d="M4 6h12M8 6V4h4v2M6 6l1 10h6l1-10"/>',
  check: '<path d="m4 10 4 4 8-8"/>',
  close: '<path d="m5 5 10 10M15 5 5 15"/>',
  front: '<rect x="7" y="7" width="10" height="10" rx="2"/><path d="M4 13V5a1 1 0 0 1 1-1h8"/>',
  back: '<rect x="3" y="3" width="10" height="10" rx="2"/><path d="M16 7v8a1 1 0 0 1-1 1H7" stroke-dasharray="2 2"/>',
};

const MIN_W = 220;
const MIN_H = 150;
const GAP = 14;

// Líneas de tarea (`- [ ]`, `1. [x]`) fuera de bloques de código, en el orden en que
// marked dibuja sus casillas.
const TASK = /^(\s*(?:[-*+]|\d+[.)])\s+)\[( |x|X)\](?=\s|$)/;
function taskLines(lines) {
  const out = [];
  let code = false;
  lines.forEach((line, i) => {
    if (/^\s*(```|~~~)/.test(line)) code = !code;
    else if (!code && TASK.test(line)) out.push(i);
  });
  return out;
}

const pad = (n) => String(n).padStart(2, '0');
const isoToday = () => {
  const d = new Date();
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};
function parseDate(text) {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(text ?? '');
  return m ? new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3])) : null;
}
function relativeDay(date) {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const days = Math.round((date - today) / 86_400_000);
  if (days === 0) return 'hoy';
  if (days === 1) return 'mañana';
  if (days === -1) return 'ayer';
  return days > 0 ? `en ${days} días` : `hace ${-days} días`;
}
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

export function createNotes({ invoke, ask, renderMarkdown, fail, onLink }) {
  let tree = null;
  let current = '';
  let loaded = false;
  let createMode = null; // 'nota' | 'tema'
  let createKind = 'texto';
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

  function noteMeta(note) {
    const parts = [];
    if (note.kind === 'fecha' || note.date) {
      const date = parseDate(note.date);
      if (date) parts.push(`${shortDate(date)} · ${relativeDay(date)}`);
    }
    if (note.total) parts.push(`${note.done}/${note.total}`);
    if (!parts.length && note.preview) parts.push(note.preview);
    return parts.join(' · ');
  }

  function noteRow(note) {
    const { li, row } = rowShell('note-row');
    row.classList.toggle('is-open', cards.has(note.path));
    const date = parseDate(note.date);
    if (date && date < new Date(new Date().setHours(0, 0, 0, 0)) && note.done < note.total) row.classList.add('overdue');
    const main = document.createElement('button');
    main.type = 'button';
    main.className = 'row-main';
    main.innerHTML = `<span class="row-icon">${icon(ICON[note.kind] ?? ICON.texto)}</span>
      <span class="row-text"><span class="row-title"></span>
      <span class="row-meta"><span class="note-type-badge"></span><span class="row-meta-text"></span></span></span>
      <span class="row-open-dot" aria-hidden="true"></span>`;
    main.querySelector('.row-title').textContent = note.title;
    main.querySelector('.note-type-badge').textContent = KIND_LABEL[note.kind] ?? 'Texto';
    main.querySelector('.row-meta-text').textContent = noteMeta(note);
    main.title = cards.has(note.path) ? 'Abierta en el escritorio' : 'Abrir en el escritorio';
    main.addEventListener('click', () => openNote(note.path));
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
    $('notes-topics').replaceChildren(...topic.topics.map(topicRow));
    $('notes-notes').replaceChildren(...topic.notes.map(noteRow));
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
      if (key === path || key.startsWith(`${path}/`)) closeCard(key, false);
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
    input.placeholder = mode === 'tema' ? 'Nombre del tema' : 'Título de la nota';
    $('notes-create-input-label').textContent = mode === 'tema' ? 'Nombre del tema' : 'Título de la nota';
    $('notes-kind').hidden = mode === 'tema';
    setCreateKind(mode === 'tema' ? 'texto' : createKind);
    input.value = '';
    input.focus();
  }

  function closeCreate() {
    createMode = null;
    $('notes-create').hidden = true;
  }

  function setCreateKind(kind) {
    createKind = kind;
    $('notes-kind').querySelectorAll('button').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.kind === kind)));
    const dated = createMode === 'nota' && kind === 'fecha';
    $('notes-create-date').hidden = !dated;
    if (dated && !$('notes-create-date-input').value) $('notes-create-date-input').value = isoToday();
  }

  $('notes-new-note').addEventListener('click', () => (createMode === 'nota' ? closeCreate() : openCreate('nota')));
  $('notes-new-topic').addEventListener('click', () => (createMode === 'tema' ? closeCreate() : openCreate('tema')));
  $('notes-kind').addEventListener('click', (event) => {
    const button = event.target.closest('button[data-kind]');
    if (button) setCreateKind(button.dataset.kind);
  });
  $('notes-create-cancel').addEventListener('click', closeCreate);
  $('notes-create').addEventListener('keydown', (event) => {
    if (event.key === 'Escape') closeCreate();
  });
  $('notes-create').addEventListener('submit', async (event) => {
    event.preventDefault();
    const title = $('notes-create-input').value.trim();
    if (!title) return $('notes-create-input').focus();
    const mode = createMode;
    try {
      if (mode === 'tema') {
        const path = await invoke('topic_create', { parent: current, title });
        closeCreate();
        current = path;
        await loadTree();
      } else {
        const date = createKind === 'fecha' ? $('notes-create-date-input').value || isoToday() : null;
        const path = await invoke('note_create', { parent: current, title, kind: createKind, date });
        closeCreate();
        await loadTree();
        await openNote(path, { edit: createKind !== 'lista', focusAdd: createKind === 'lista' });
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
      <div class="desk-card-config" hidden>
        <div class="segmented small desk-kind" role="group" aria-label="Tipo de nota">
          <button type="button" data-kind="texto">Texto</button>
          <button type="button" data-kind="lista">Lista</button>
          <button type="button" data-kind="fecha">Fecha</button>
        </div>
        <input type="date" class="desk-date-input" aria-label="Fecha de la nota">
      </div>
      <div class="desk-date-block" hidden><span class="desk-date"></span><span class="desk-date-sub"></span></div>
      <div class="desk-card-body note-body"></div>
      <textarea class="desk-editor" spellcheck="true" aria-label="Contenido en Markdown" hidden></textarea>
      <form class="desk-add" hidden><input type="text" class="desk-add-item" placeholder="Añadir elemento" aria-label="Añadir elemento a la lista" autocomplete="off"></form>
      <span class="desk-resize" aria-hidden="true"></span>`;
    card.el = el;
    const tools = el.querySelector('.desk-card-tools');
    card.editButton = cardButton(ICON.pencil, 'Editar', () => setEditing(card, !card.editing), 'card-edit');
    tools.append(
      card.editButton,
      cardButton(ICON.front, 'Traer al frente', () => bringToFront(card)),
      cardButton(ICON.back, 'Enviar al fondo', () => sendToBack(card)),
      cardButton(ICON.close, 'Cerrar del escritorio', () => closeCard(card.path)),
    );

    // Mover: se arrastra por la cabecera.
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
        card.x = Math.max(-card.w + 80, Math.min(start.x + e.clientX - start.px, W - 80));
        card.y = Math.max(0, start.y + e.clientY - start.py);
        el.style.left = `${card.x}px`;
        el.style.top = `${card.y}px`;
      };
      const up = () => {
        head.removeEventListener('pointermove', move);
        el.classList.remove('dragging');
        card.x = Math.max(0, card.x);
        place(card);
        saveDesk();
      };
      head.addEventListener('pointermove', move);
      head.addEventListener('pointerup', up, { once: true });
      head.addEventListener('pointercancel', up, { once: true });
    });
    head.addEventListener('dblclick', (event) => {
      if (!event.target.closest('button')) setEditing(card, true, { focusTitle: true });
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
        card.w = Math.max(MIN_W, Math.min(start.w + e.clientX - start.px, W - card.x));
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

    // Edición.
    const editor = el.querySelector('.desk-editor');
    editor.addEventListener('input', () => {
      card.draft = editor.value;
      scheduleSave(card);
    });
    editor.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') setEditing(card, false);
    });
    el.querySelector('.desk-kind').addEventListener('click', (event) => {
      const b = event.target.closest('button[data-kind]');
      if (!b || b.dataset.kind === card.doc?.kind) return;
      const changes = { kind: b.dataset.kind };
      if (b.dataset.kind === 'fecha' && !card.doc?.date) changes.date = isoToday();
      save(card, changes);
    });
    el.querySelector('.desk-date-input').addEventListener('change', (event) => save(card, { date: event.target.value || '' }));
    el.querySelector('.desk-add').addEventListener('submit', (event) => {
      event.preventDefault();
      const input = el.querySelector('.desk-add-item');
      const text = input.value.trim();
      if (!text || !card.doc) return;
      input.value = '';
      const body = card.doc.body.replace(/\s+$/, '');
      save(card, { body: `${body ? `${body}\n` : ''}- [ ] ${text}\n` }, { keepFocus: true });
    });
    el.querySelector('.desk-card-body').addEventListener('click', (event) => {
      const link = event.target.closest('a');
      if (!link) return;
      event.preventDefault();
      onLink(link, card.path);
    });
    return el;
  }

  function renderCard(card) {
    const { el, doc } = card;
    if (!el) return;
    const kind = doc?.kind ?? 'texto';
    el.dataset.kind = kind;
    el.classList.toggle('editing', card.editing);
    el.querySelector('.desk-card-icon').innerHTML = icon(ICON[kind] ?? ICON.texto);
    const title = el.querySelector('.desk-card-title');
    if (!title.querySelector('input')) title.textContent = doc?.title ?? 'Cargando…';

    const lines = (doc?.body ?? '').split('\n');
    const tasks = taskLines(lines);
    const done = tasks.filter((i) => /\[(x|X)\]/.test(lines[i])).length;
    const meta = [KIND_LABEL[kind]];
    if (tasks.length) meta.push(`${done}/${tasks.length}`);
    if (doc?.updated) meta.push(`editada ${editedText(doc.updated)}`);
    el.querySelector('.desk-card-meta').textContent = meta.join(' · ');

    // Fecha destacada.
    const date = parseDate(doc?.date);
    const block = el.querySelector('.desk-date-block');
    block.hidden = !date || card.editing;
    if (date) {
      el.querySelector('.desk-date').textContent = date.toLocaleDateString('es', { day: 'numeric', month: 'short' });
      el.querySelector('.desk-date-sub').textContent = `${date.toLocaleDateString('es', { weekday: 'long' })} · ${relativeDay(date)}`;
    }

    // Configuración (solo editando).
    const config = el.querySelector('.desk-card-config');
    config.hidden = !card.editing;
    config.querySelectorAll('.desk-kind button').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.kind === kind)));
    const dateInput = config.querySelector('.desk-date-input');
    dateInput.hidden = kind !== 'fecha' && !doc?.date;
    if (document.activeElement !== dateInput) dateInput.value = doc?.date ?? '';

    const body = el.querySelector('.desk-card-body');
    const editor = el.querySelector('.desk-editor');
    body.hidden = card.editing;
    editor.hidden = !card.editing;
    el.querySelector('.desk-add').hidden = card.editing || kind !== 'lista';
    card.editButton.innerHTML = icon(card.editing ? ICON.check : ICON.pencil, 14);
    card.editButton.title = card.editing ? 'Terminar de editar' : 'Editar';
    card.editButton.setAttribute('aria-label', card.editButton.title);

    if (card.editing) {
      if (document.activeElement !== editor) editor.value = card.draft ?? doc?.body ?? '';
      return;
    }
    if (!doc) {
      body.innerHTML = '<p class="desk-card-placeholder">Cargando…</p>';
      return;
    }
    if (!doc.body.trim()) {
      body.innerHTML = '';
      const p = document.createElement('p');
      p.className = 'desk-card-placeholder';
      p.textContent = kind === 'lista' ? 'Lista vacía: añade el primer elemento abajo.' : 'Nota vacía. Pulsa el lápiz o haz doble clic en la cabecera para escribir.';
      body.append(p);
      return;
    }
    body.innerHTML = renderMarkdown(doc.body);
    // Casillas: marcarlas edita el Markdown; en las listas, cada elemento se puede quitar.
    body.querySelectorAll('input[type="checkbox"]').forEach((box, index) => {
      box.disabled = false;
      const item = box.closest('li');
      item?.classList.add('desk-check-item');
      item?.classList.toggle('done', box.checked);
      box.addEventListener('change', () => toggleTask(card, index, box.checked));
      if (kind === 'lista' && item) {
        const del = cardButton(ICON.close, 'Quitar elemento', () => removeTask(card, index), 'item-remove');
        item.append(del);
      }
    });
  }

  function toggleTask(card, index, checked) {
    const lines = card.doc.body.split('\n');
    const line = taskLines(lines)[index];
    if (line == null) return;
    lines[line] = lines[line].replace(TASK, (_, lead) => `${lead}[${checked ? 'x' : ' '}]`);
    save(card, { body: lines.join('\n') });
  }

  function removeTask(card, index) {
    const lines = card.doc.body.split('\n');
    const line = taskLines(lines)[index];
    if (line == null) return;
    lines.splice(line, 1);
    save(card, { body: lines.join('\n') });
  }

  async function save(card, changes, { keepFocus = false } = {}) {
    try {
      const doc = await invoke('note_save', { path: card.path, changes });
      card.doc = doc;
      if (changes.body != null && card.draft === changes.body) card.draft = null;
      renderCard(card);
      if (keepFocus) card.el.querySelector('.desk-add-item')?.focus();
      scheduleTreeReload();
    } catch (error) {
      fail(error);
    }
  }

  function scheduleSave(card) {
    clearTimeout(card.timer);
    card.timer = setTimeout(() => {
      if (card.draft != null && card.draft !== card.doc?.body) save(card, { body: card.draft });
    }, 700);
  }

  async function setEditing(card, editing, { focusTitle = false } = {}) {
    if (!card.doc) return;
    if (card.editing && !editing) {
      clearTimeout(card.timer);
      commitTitle(card);
      if (card.draft != null && card.draft !== card.doc.body) await save(card, { body: card.draft });
      card.draft = null;
      if (card.stale) {
        card.stale = false;
        await reloadCard(card);
      }
    }
    card.editing = editing;
    if (editing) card.draft = card.doc.body;
    const title = card.el.querySelector('.desk-card-title');
    if (editing) {
      const input = document.createElement('input');
      input.className = 'desk-title-input';
      input.value = card.doc.title;
      input.setAttribute('aria-label', 'Título de la nota');
      input.addEventListener('keydown', (event) => {
        if (event.key === 'Enter') {
          event.preventDefault();
          commitTitle(card);
          card.el.querySelector('.desk-editor').focus();
        }
        if (event.key === 'Escape') setEditing(card, false);
      });
      title.replaceChildren(input);
    } else {
      title.textContent = card.doc.title;
    }
    renderCard(card);
    bringToFront(card);
    if (editing) {
      const target = focusTitle ? title.querySelector('input') : card.el.querySelector('.desk-editor');
      target?.focus();
      if (!focusTitle) target?.setSelectionRange(target.value.length, target.value.length);
    }
  }

  function commitTitle(card) {
    const input = card.el.querySelector('.desk-title-input');
    const value = input?.value.trim();
    if (value && value !== card.doc.title) save(card, { title: value });
  }

  async function reloadCard(card) {
    try {
      const doc = await invoke('note_read', { path: card.path });
      const same = card.doc && ['title', 'kind', 'date', 'body'].every((k) => card.doc[k] === doc[k]);
      card.doc = doc;
      if (!same) renderCard(card);
    } catch {
      // La nota ya no existe (borrada o movida fuera de nexo).
      closeCard(card.path);
    }
  }

  // Solo para cartas nuevas: que aparezcan dentro del escritorio visible.
  function fitInside(card) {
    const { w: W, h: H } = deskSize();
    if (W < MIN_W || H < MIN_H) return;
    card.w = Math.max(MIN_W, Math.min(card.w, W - 2 * GAP));
    card.h = Math.max(MIN_H, Math.min(card.h, H - 2 * GAP));
    card.x = Math.max(0, Math.min(card.x, W - card.w));
    card.y = Math.max(0, Math.min(card.y, H - card.h));
  }

  function cascadePosition() {
    const k = cards.size;
    return { x: 24 + ((k * 28) % 220), y: 20 + ((k * 28) % 160) };
  }

  async function openNote(path, { edit = false, focusAdd = false, geometry = null } = {}) {
    const existing = cards.get(path);
    if (existing) {
      bringToFront(existing);
      existing.el.classList.remove('pulse');
      void existing.el.offsetWidth;
      existing.el.classList.add('pulse');
      if (edit) setEditing(existing, true);
      return existing;
    }
    const card = {
      path,
      ...(geometry ?? { ...cascadePosition(), w: 340, h: 300 }),
      z: geometry?.z ?? Math.max(0, ...zValues()) + 1,
      doc: null,
      editing: false,
      draft: null,
    };
    cards.set(path, card);
    surface.append(createCardEl(card));
    if (!geometry) fitInside(card);
    place(card);
    renderCard(card);
    if (!geometry) saveDesk();
    renderDeskChrome();
    try {
      card.doc = await invoke('note_read', { path });
    } catch (error) {
      closeCard(path);
      if (!geometry) fail(error);
      return null;
    }
    renderCard(card);
    renderPanel();
    if (edit) setEditing(card, true);
    if (focusAdd) card.el.querySelector('.desk-add-item')?.focus();
    return card;
  }

  function closeCard(path, persist = true) {
    const card = cards.get(path);
    if (!card) return;
    clearTimeout(card.timer);
    if (card.editing && card.draft != null && card.draft !== card.doc?.body) {
      invoke('note_save', { path, changes: { body: card.draft } }).catch(fail);
    }
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
      Object.assign(card.el.style, { left: `${card.x}px`, top: `${card.y}px`, width: `${card.w}px`, height: `${card.h}px`, zIndex: String(card.z) });
    });
    setTimeout(() => surface.classList.remove('arranging'), 520);
    saveDesk();
  }

  $('desk-arrange').addEventListener('click', arrange);
  $('desk-close-all').addEventListener('click', () => {
    for (const path of [...cards.keys()]) closeCard(path, false);
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
    for (const path of [...cards.keys()]) if (!keep.has(path)) closeCard(path, false);
    for (const c of wanted) {
      const geometry = {
        x: Number(c.x) || 0,
        y: Number(c.y) || 0,
        w: Number(c.w) || 340,
        h: Number(c.h) || 300,
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
        for (const path of [...cards.keys()]) closeCard(path, false);
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
        if (!card) continue;
        if (card.editing) card.stale = true;
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
