// Apartado Notas: la carta de la izquierda muestra temas, subtemas y notas de
// wiki/notas/, y detrás de toda la app hay un lienzo donde cada nota abierta es una carta
// que se mueve, se redimensiona y se apila. El lienzo es tan grande que hace de infinito: se
// recorre arrastrando el fondo, con la rueda o desde el minimapa de la barra superior. Las
// notas se escriben directamente en la carta, línea a línea (note-editor.js), y se guardan en
// Markdown (notas.rs). La disposición del escritorio abierto va en .nexo/escritorio.json
// (Claude Code también la edita) y la lista de escritorios y los archivados, en
// .nexo/escritorios.json (desks.js).

import { ZOOM, boundsOf, clampZoom, fromMap, mapFrame, nextSpot, revealShift, scaleOf, toMap, zoomAround, zoomLabel } from './desk-layout.js';
import { MAX_DESKS, activeDesk, archiveDesk, createDesk, normalize, removeArchived, renameDesk, restoreDesk, storeActive, switchDesk } from './desks.js';
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
  undo: '<path d="M7.5 4 4 7.5 7.5 11"/><path d="M4 7.5h7.5a4.5 4.5 0 0 1 0 9H8"/>',
  check: '<path d="m4.5 10.5 3.5 3.5 7.5-8"/>',
  archive: '<rect x="3" y="4" width="14" height="4" rx="1"/><path d="M4.5 8v7.5h11V8M8 11.5h4"/>',
};

const MIN_W = 220;
const MIN_H = 150;
const GAP = 16; // entre una carta y la que se abre a su derecha
const CARD_W = 380;
const CARD_H = 440;
// El lienzo. Las cartas guardan su posición en coordenadas del lienzo, con el (0, 0) en su
// centro (pueden ser negativas); en el DOM van desplazadas ORIGIN, porque un desplazamiento
// nativo no admite posiciones negativas.
const WORLD = { w: 40000, h: 28000 };
const ORIGIN = { x: WORLD.w / 2, y: WORLD.h / 2 };
const HOME_PAD = 24; // del borde del hueco libre a la primera carta
const reducedMotion = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;
const newId = () => `d${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;

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
  const viewport = $('desk'); // la ventana: lo que se desplaza
  const surface = $('desk-surface'); // el lienzo entero
  surface.style.setProperty('--desk-w', `${WORLD.w}px`);
  surface.style.setProperty('--desk-h', `${WORLD.h}px`);

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
    actions.append(
      actionButton(ICON.pencil, `Renombrar ${note.title}`, () => renameNote(row, note)),
      actionButton(ICON.trash, `Borrar ${note.title}`, () => remove(note.path, note.title, false)),
    );
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

  // Cambia el nombre de una fila en su sitio: el título se vuelve un campo de texto.
  function renameRow(row, current, label, rename) {
    const title = row.querySelector('.row-title');
    const input = document.createElement('input');
    input.className = 'row-rename';
    input.value = current;
    input.setAttribute('aria-label', label);
    const main = row.querySelector('.row-main');
    main.replaceWith(input);
    input.focus();
    input.select();
    let done = false;
    const finish = async (save) => {
      if (done) return;
      done = true;
      const value = input.value.trim();
      if (save && value && value !== current) {
        try {
          await rename(value);
        } catch (error) {
          fail(error);
        }
        await loadTree();
      } else {
        input.replaceWith(main);
        title.textContent = current;
      }
    };
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') finish(true);
      if (event.key === 'Escape') finish(false);
    });
    input.addEventListener('blur', () => finish(true));
  }

  const renameTopic = (row, topic) =>
    renameRow(row, topic.title, 'Nuevo nombre del tema', (title) => invoke('topic_rename', { path: topic.path, title }));

  // Si la nota está abierta, el cambio va por la fila de guardados de su carta.
  const renameNote = (row, note) =>
    renameRow(row, note.title, 'Nuevo nombre de la nota', (title) => {
      const card = cards.get(note.path);
      return card?.doc ? save(card, { title }) : invoke('note_save', { path: note.path, changes: { title } });
    });

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

  const zValues = () => [...cards.values()].map((c) => c.z);

  // La cámara: el punto del lienzo que cae en la esquina superior izquierda de la ventana.
  // Se guarda aparte del desplazamiento del elemento porque este se pierde mientras el
  // lienzo está oculto (otra vista); `camReady` evita leerlo antes de volver a ponerlo.
  // Con zoom, el lienzo se encoge con `transform: scale` (desde su esquina): el
  // desplazamiento va en píxeles de pantalla y la cámara, en coordenadas del lienzo.
  let cam = null;
  let camReady = false;
  let zoom = 0; // de 0 a −200 (%), ver `scaleOf`
  let scale = 1;
  const readCamera = () => ({ x: viewport.scrollLeft / scale - ORIGIN.x, y: viewport.scrollTop / scale - ORIGIN.y });

  // El hueco libre entre las cartas laterales y bajo la barra, en coordenadas de la ventana
  // del lienzo: ahí es donde se ve lo que no tapa la interfaz.
  function freeArea() {
    const stage = $('stage').getBoundingClientRect();
    const frame = viewport.getBoundingClientRect();
    return { x: Math.round(stage.left - frame.left), y: Math.round(stage.top - frame.top), w: Math.round(stage.width), h: Math.round(stage.height) };
  }

  // Cámara de partida: el (0, 0) del lienzo, en la esquina del hueco libre.
  function homeCamera() {
    const free = freeArea();
    return { x: -(free.x + HOME_PAD) / scale, y: -(free.y + HOME_PAD) / scale };
  }

  function applyCamera() {
    if (viewport.hidden) return;
    cam ??= homeCamera();
    camReady = false;
    viewport.scrollTo({ left: (cam.x + ORIGIN.x) * scale, top: (cam.y + ORIGIN.y) * scale, behavior: 'instant' });
    camReady = true;
    queueMap();
  }

  function moveCamera(next, { smooth = false } = {}) {
    viewport.scrollTo({ left: (next.x + ORIGIN.x) * scale, top: (next.y + ORIGIN.y) * scale, behavior: smooth && !reducedMotion() ? 'smooth' : 'instant' });
  }

  // Trae una carta al hueco libre si no se ve entera ahí (recién abierta, o con el foco).
  // Las cuentas van en píxeles de pantalla, que es donde está el hueco.
  function reveal(card, { smooth = true } = {}) {
    if (viewport.hidden || !camReady) return;
    const at = readCamera();
    const { dx, dy } = revealShift({ x: (card.x - at.x) * scale, y: (card.y - at.y) * scale, w: card.w * scale, h: card.h * scale }, freeArea());
    if (dx || dy) moveCamera({ x: at.x + dx / scale, y: at.y + dy / scale }, { smooth });
  }

  // ------------------------------------------------------------ zoom (solo alejar)
  // Barra y botones a la derecha del gestor de escritorios. La rueda no cambia el zoom: sigue
  // moviendo el lienzo. Lo que hay en el centro del hueco libre se queda en su sitio.
  const zoomRange = $('zoom-range');
  function renderZoom() {
    zoomRange.value = String(zoom);
    zoomRange.setAttribute('aria-valuetext', zoomLabel(zoom));
    $('zoom-value').textContent = zoomLabel(zoom);
    $('zoom-value').setAttribute('aria-label', `Zoom ${zoomLabel(zoom)}${zoom ? ', volver a 0 %' : ''}`);
    $('zoom-out').disabled = zoom <= ZOOM.min;
    $('zoom-in').disabled = zoom >= ZOOM.max;
  }
  function applyScale() {
    surface.style.transform = scale === 1 ? '' : `scale(${scale})`;
    renderZoom();
  }
  // La cámara exacta del último cambio de zoom: el desplazamiento se queda en píxeles enteros
  // y, si cada paso partiera de él, el redondeo se acumularía (arrastrando la barra, el centro
  // se iba unos píxeles por paso). Vale mientras el lienzo no se mueva por otra cosa.
  let zoomCam = null;
  function setZoom(next, { save = true } = {}) {
    const target = clampZoom(next);
    if (target === zoom) return renderZoom();
    const from = scale;
    let at = camReady && !viewport.hidden ? readCamera() : (cam ?? homeCamera());
    if (zoomCam && Math.abs(at.x - zoomCam.x) * from < 1 && Math.abs(at.y - zoomCam.y) * from < 1) at = zoomCam;
    const free = freeArea();
    const center = { x: free.x + free.w / 2, y: free.y + free.h / 2 };
    zoom = target;
    scale = scaleOf(zoom);
    applyScale();
    cam = zoomAround(at, center, from, scale);
    zoomCam = cam;
    applyCamera();
    if (save) {
      clearTimeout(viewTimer);
      viewTimer = setTimeout(() => loaded && writeDesk().catch(fail), 600);
    }
  }
  zoomRange.addEventListener('input', () => setZoom(zoomRange.value));
  $('zoom-out').addEventListener('click', () => setZoom(Math.ceil(zoom / ZOOM.step) * ZOOM.step - ZOOM.step));
  $('zoom-in').addEventListener('click', () => setZoom(Math.floor(zoom / ZOOM.step) * ZOOM.step + ZOOM.step));
  $('zoom-value').addEventListener('click', () => setZoom(0));
  // Al pasar a otro escritorio, el suyo: la vista que se guardó con él (sin animación).
  function useView(view) {
    zoomCam = null;
    zoom = clampZoom(view?.zoom ?? 0);
    scale = scaleOf(zoom);
    applyScale();
    cam = view && Number.isFinite(view.x) && Number.isFinite(view.y) ? { x: view.x, y: view.y } : null;
  }

  function normalizeLayers() {
    [...cards.values()]
      .sort((a, b) => a.z - b.z)
      .forEach((card, i) => {
        card.z = i + 1;
        card.el.style.zIndex = String(card.z);
      });
  }

  // Pinta la carta donde la dejó el usuario. Nunca se recoloca ni se encoge sola: lo que
  // queda bajo una carta lateral se ve moviendo el lienzo o minimizando esa carta.
  function place(card) {
    Object.assign(card.el.style, {
      left: `${card.x + ORIGIN.x}px`,
      top: `${card.y + ORIGIN.y}px`,
      width: `${card.w}px`,
      height: `${card.h}px`,
      zIndex: String(card.z),
    });
  }

  // Lo que hay en pantalla, tal como se guarda: las cartas y la cámara.
  const snapshotCards = () => [...cards.values()].map(({ path, x, y, w, h, z }) => ({ path, x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h), z }));
  const snapshotView = () => (cam ? { x: Math.round(cam.x), y: Math.round(cam.y), ...(zoom ? { zoom } : {}) } : null);
  const writeDesk = () => invoke('desk_save', { state: { cards: snapshotCards(), view: snapshotView() } });

  let saveTimer = 0;
  function saveDesk() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => writeDesk().catch(fail), 350);
    renderDeskChrome();
  }

  function renderDeskChrome() {
    $('desk-empty').hidden = cards.size > 0;
    queueMap();
    renderManager();
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
    card.undoButton = cardButton(ICON.undo, 'Deshacer el último cambio', () => undoCard(card), 'card-undo');
    card.undoButton.disabled = true;
    tools.append(
      card.undoButton,
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
        syncUndo(card);
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
        // El lienzo es enorme, pero tiene bordes: la carta no sale de él.
        // El ratón se mueve en píxeles de pantalla; con zoom, la carta recorre más lienzo.
        card.x = Math.max(-ORIGIN.x, Math.min(start.x + (e.clientX - start.px) / scale, WORLD.w - ORIGIN.x - card.w));
        card.y = Math.max(-ORIGIN.y, Math.min(start.y + (e.clientY - start.py) / scale, WORLD.h - ORIGIN.y - card.h));
        el.style.translate = `${card.x - start.x}px ${card.y - start.y}px`;
        queueMap();
      };
      const up = () => {
        head.removeEventListener('pointermove', move);
        el.classList.remove('dragging');
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
        card.w = Math.max(MIN_W, start.w + (e.clientX - start.px) / scale);
        card.h = Math.max(MIN_H, start.h + (e.clientY - start.py) / scale);
        el.style.width = `${card.w}px`;
        el.style.height = `${card.h}px`;
        queueMap();
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
    // Lo escrito en el Markdown a la vista cuenta como un paso de deshacer.
    if (!on) card.editor.setValue(source.value, { undoable: true });
    else source.value = card.editor.getValue();
    card.source = on;
    body.hidden = on;
    source.hidden = !on;
    // Ojo: `.source` a secas es la clase de las filas de Fuentes y descolocaba la carta.
    card.el.classList.toggle('desk-md', on);
    card.sourceButton.title = on ? 'Volver a la nota' : 'Ver el Markdown';
    card.sourceButton.setAttribute('aria-label', card.sourceButton.title);
    card.sourceButton.setAttribute('aria-pressed', String(on));
    syncUndo(card);
    if (on) source.focus();
    else card.editor.focusEnd();
  }

  // El botón de deshacer solo se enciende cuando hay un paso al que volver.
  function syncUndo(card) {
    card.undoButton.disabled = !card.source && !card.editor.canUndo();
  }

  // Un paso atrás en la nota (lo mismo que Ctrl+Z).
  function undoCard(card) {
    if (card.source) {
      const source = card.el.querySelector('.desk-editor');
      source.focus();
      document.execCommand('undo');
    } else card.editor.undo();
    syncUndo(card);
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
        card.editor.setValue(doc.body, { undoable: true });
        if (card.source) card.el.querySelector('.desk-editor').value = doc.body;
        syncUndo(card);
      }
      renderHead(card);
    } catch {
      // La nota ya no existe (borrada o movida fuera de nexo).
      closeCard(card.path, { save: false });
    }
  }

  // Sitio de una carta nueva: a la derecha de la última que se abrió, a su altura y con su
  // tamaño (`nextSpot`). La primera va a la esquina del hueco libre de lo que se está viendo.
  let lastOpened = null;
  function newGeometry() {
    const at = camReady ? readCamera() : (cam ?? homeCamera());
    const free = freeArea();
    return nextSpot(
      [...cards.values()].map(({ path, x, y, w, h }) => ({ path, x, y, w, h })),
      { anchor: lastOpened, home: { x: Math.round(at.x + (free.x + HOME_PAD) / scale), y: Math.round(at.y + (free.y + HOME_PAD) / scale) }, size: { w: CARD_W, h: CARD_H }, gap: GAP },
    );
  }

  async function openNote(path, { focus = false, geometry = null } = {}) {
    const existing = cards.get(path);
    if (existing) {
      bringToFront(existing);
      existing.el.classList.remove('pulse');
      void existing.el.offsetWidth;
      existing.el.classList.add('pulse');
      if (focus) existing.editor.focusEnd();
      // Después del foco: enfocar ya desplaza por su cuenta y cortaría este movimiento.
      reveal(existing);
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
    if (!geometry) {
      lastOpened = path;
      saveDesk();
    }
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
    if (focus) card.editor.focusEnd();
    // La carta nueva se trae al hueco libre (después del foco, que desplaza por su cuenta).
    if (!geometry) reveal(card);
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

  // ------------------------------------------------------------ lienzo: moverse por él

  // La rueda y el trackpad lo desplazan solos (es un desplazamiento nativo). Aquí, lo que
  // falta: arrastrar el fondo, seguir la cámara y traer una carta al tabular hasta ella.
  viewport.addEventListener('pointerdown', (event) => {
    if (event.button !== 0 || (event.target !== surface && event.target !== viewport)) return;
    const start = { px: event.clientX, py: event.clientY, left: viewport.scrollLeft, top: viewport.scrollTop };
    viewport.setPointerCapture(event.pointerId);
    viewport.classList.add('panning');
    const move = (e) => viewport.scrollTo({ left: start.left - (e.clientX - start.px), top: start.top - (e.clientY - start.py), behavior: 'instant' });
    const up = () => {
      viewport.removeEventListener('pointermove', move);
      viewport.classList.remove('panning');
    };
    viewport.addEventListener('pointermove', move);
    viewport.addEventListener('pointerup', up, { once: true });
    viewport.addEventListener('pointercancel', up, { once: true });
  });

  // Sobre una carta, la rueda desplaza la nota si es larga y el lienzo si no (ver
  // `can-scroll` en styles.css). Se mira al entrar el ratón en la carta y en cada giro.
  const syncScrollable = (event) => {
    const card = event.target.closest?.('.desk-card');
    for (const box of card?.querySelectorAll('.desk-card-body, .desk-editor') ?? []) box.classList.toggle('can-scroll', box.scrollHeight > box.clientHeight + 1);
  };
  surface.addEventListener('pointerover', syncScrollable, { passive: true });
  surface.addEventListener('wheel', syncScrollable, { capture: true, passive: true });

  let viewTimer = 0;
  viewport.addEventListener('scroll', () => {
    if (viewport.hidden || !camReady) return;
    cam = readCamera();
    queueMap();
    // Dónde se quedó uno se recuerda con el escritorio.
    clearTimeout(viewTimer);
    viewTimer = setTimeout(() => loaded && writeDesk().catch(fail), 600);
  });

  // Con el teclado, el foco puede caer en una carta tapada por una carta lateral: se trae
  // al hueco libre. Con el ratón no, que movería la carta bajo el cursor.
  let byKeyboard = false;
  document.addEventListener('keydown', (event) => event.key === 'Tab' && (byKeyboard = true), true);
  document.addEventListener('pointerdown', () => (byKeyboard = false), true);
  surface.addEventListener('focusin', (event) => {
    const card = byKeyboard && cards.get(event.target.closest?.('.desk-card')?.dataset.path);
    if (card) reveal(card, { smooth: false });
  });

  // ------------------------------------------------------------ minimapa
  // Dónde está uno en el lienzo: las cartas en pequeño y un recuadro por lo que se ve. Se
  // pulsa o se arrastra para ir a un sitio; con el foco, las flechas mueven la vista.
  const map = $('desk-map');
  const mapWorld = $('desk-map-world');
  const mapView = $('desk-map-view');
  const mapDots = new Map(); // ruta → su recuadro en el mapa
  let mapNow = null; // el encuadre con el que está pintado
  let mapHeld = null; // encuadre fijo mientras se arrastra en el mapa
  let mapQueued = 0;

  const viewRect = () => ({ ...(camReady ? readCamera() : (cam ?? { x: 0, y: 0 })), w: viewport.clientWidth / scale, h: viewport.clientHeight / scale });

  function renderMap() {
    mapQueued = 0;
    const size = { w: map.clientWidth, h: map.clientHeight };
    if (viewport.hidden || !size.w) return;
    const rects = [...cards.values()].map(({ path, x, y, w, h }) => ({ path, x, y, w, h }));
    const view = viewRect();
    mapNow = mapHeld ?? mapFrame(rects, view, size);
    const put = (node, rect) => {
      const box = toMap(mapNow, rect);
      Object.assign(node.style, { left: `${box.left}px`, top: `${box.top}px`, width: `${box.width}px`, height: `${box.height}px` });
    };
    for (const [path, node] of mapDots) {
      if (cards.has(path)) continue;
      node.remove();
      mapDots.delete(path);
    }
    for (const rect of rects) {
      let node = mapDots.get(rect.path);
      if (!node) {
        node = document.createElement('span');
        node.className = 'desk-map-card';
        mapWorld.insertBefore(node, mapView);
        mapDots.set(rect.path, node);
      }
      put(node, rect);
    }
    put(mapView, view);
  }
  function queueMap() {
    if (!mapQueued) mapQueued = requestAnimationFrame(renderMap);
  }

  // Lleva el centro de la vista al punto del lienzo que hay bajo el puntero en el mapa.
  function goToMapPoint(event) {
    if (!mapNow) return;
    const box = map.getBoundingClientRect();
    const point = fromMap(mapNow, event.clientX - box.left - map.clientLeft, event.clientY - box.top - map.clientTop);
    moveCamera({ x: point.x - viewport.clientWidth / scale / 2, y: point.y - viewport.clientHeight / scale / 2 });
  }
  map.addEventListener('pointerdown', (event) => {
    if (event.button !== 0 || !mapNow) return;
    event.preventDefault();
    map.focus({ preventScroll: true });
    // Mientras se arrastra, el encuadre no cambia: si no, el mapa se movería bajo el puntero.
    mapHeld = mapNow;
    map.setPointerCapture(event.pointerId);
    goToMapPoint(event);
    const up = () => {
      map.removeEventListener('pointermove', goToMapPoint);
      mapHeld = null;
      queueMap();
    };
    map.addEventListener('pointermove', goToMapPoint);
    map.addEventListener('pointerup', up, { once: true });
    map.addEventListener('pointercancel', up, { once: true });
  });

  // Vuelve a las cartas: deja la esquina de todas ellas en la esquina del hueco libre.
  function goToCards({ smooth = true } = {}) {
    const all = boundsOf([...cards.values()]);
    const home = homeCamera();
    moveCamera(all ? { x: all.x + home.x, y: all.y + home.y } : home, { smooth });
  }
  map.addEventListener('dblclick', () => goToCards());
  map.addEventListener('keydown', (event) => {
    const step = event.shiftKey ? 480 : 120;
    const delta = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] }[event.key];
    if (delta) {
      event.preventDefault();
      viewport.scrollBy({ left: delta[0], top: delta[1], behavior: 'instant' });
    } else if (event.key === 'Home') {
      event.preventDefault();
      goToCards();
    }
  });
  window.addEventListener('resize', queueMap);

  // ------------------------------------------------------------ escritorios
  // Hasta cinco en la lista; los demás, archivados con sus notas colocadas, para cargarlos
  // cuando hagan falta. El que está abierto es el de .nexo/escritorio.json.
  let deskState = null;
  let deskWork = Promise.resolve();
  const desksButton = $('desks-button');
  const desksMenu = $('desks-menu');

  const saveDesks = () => invoke('desks_save', { state: deskState });

  // Aplica el escritorio guardado (al empezar o cuando Claude Code lo cambia).
  async function applyDesk({ initial = false } = {}) {
    let state;
    try {
      state = await invoke('desk_load');
    } catch {
      return;
    }
    const wanted = Array.isArray(state?.cards) ? state.cards.filter((c) => typeof c?.path === 'string') : [];
    const keep = new Set(wanted.map((c) => c.path));
    for (const path of [...cards.keys()]) if (!keep.has(path)) closeCard(path, { persist: false });
    const fresh = [];
    for (const c of wanted) {
      const geometry = {
        x: Number(c.x) || 0,
        y: Number(c.y) || 0,
        w: Math.max(MIN_W, Number(c.w) || CARD_W),
        h: Math.max(MIN_H, Number(c.h) || CARD_H),
        z: Number(c.z) || 1,
      };
      const card = cards.get(c.path);
      if (card) {
        Object.assign(card, geometry);
        place(card);
      } else {
        const opened = await openNote(c.path, { geometry });
        if (opened) fresh.push(opened);
      }
    }
    normalizeLayers();
    if (initial) {
      useView(state?.view ?? activeDesk(deskState)?.view);
      applyCamera();
    } else if (fresh.length) {
      // Claude Code puso una carta: la vista va a ella.
      lastOpened = fresh.at(-1).path;
      reveal(fresh.at(-1));
    }
    renderDeskChrome();
    renderPanel();
  }

  // Pone en pantalla el escritorio activo de `deskState` y lo deja en .nexo/escritorio.json.
  async function showActive() {
    clearTimeout(saveTimer);
    clearTimeout(viewTimer);
    for (const path of [...cards.keys()]) closeCard(path, { persist: false });
    lastOpened = null;
    const desk = activeDesk(deskState);
    for (const c of desk.cards) await openNote(c.path, { geometry: { x: c.x, y: c.y, w: Math.max(MIN_W, c.w || CARD_W), h: Math.max(MIN_H, c.h || CARD_H), z: c.z || 1 } });
    normalizeLayers();
    useView(desk.view);
    applyCamera();
    await writeDesk();
    renderDeskChrome();
    renderPanel();
  }

  // Los cambios de escritorio van de uno en uno: cada uno parte de lo que dejó el anterior.
  function deskAction(change) {
    deskWork = deskWork
      .then(async () => {
        if (!deskState) return;
        // Lo que se estaba escribiendo se guarda antes de quitar las cartas de en medio.
        await Promise.all([...cards.values()].map(flush));
        const before = deskState;
        const active = before.active;
        deskState = change(storeActive(before, snapshotCards(), snapshotView()));
        await saveDesks();
        if (deskState.active !== active) await showActive();
        else renderManager();
      })
      .catch(fail);
    return deskWork;
  }

  const cardCount = (n) => `${n} ${n === 1 ? 'nota' : 'notas'}`;
  const deskButton = (className, paths, label, handler) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = className;
    if (paths) b.innerHTML = icon(paths, 14);
    b.setAttribute('aria-label', label);
    b.title = label;
    b.addEventListener('click', handler);
    return b;
  };

  function renameDeskRow(row, desk) {
    const input = document.createElement('input');
    input.className = 'desks-rename';
    input.value = desk.name;
    input.maxLength = 60;
    input.setAttribute('aria-label', `Nombre de ${desk.name}`);
    let done = false;
    const finish = (save) => {
      if (done) return;
      done = true;
      const name = input.value;
      if (save && name.trim() && name.trim() !== desk.name) deskAction((state) => renameDesk(state, desk.id, name)).then(() => focusRow(desk.id));
      else {
        renderManager();
        focusRow(desk.id);
      }
    };
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') finish(true);
      else if (event.key === 'Escape') {
        // Escape cancela el nombre, no cierra el menú.
        event.stopPropagation();
        finish(false);
      }
    });
    input.addEventListener('blur', () => finish(true));
    row.replaceChildren(input);
    input.focus();
    input.select();
  }
  const focusRow = (id) => desksMenu.querySelector(`[data-desk="${id}"] .desks-pick`)?.focus();

  function renderManager() {
    if (!deskState) return;
    const active = activeDesk(deskState);
    $('desks-current').textContent = active.name;
    if (desksMenu.hidden) return;
    const full = deskState.desks.length >= MAX_DESKS;
    $('desks-count').textContent = `${deskState.desks.length} de ${MAX_DESKS}`;
    $('desks-list').replaceChildren(
      ...deskState.desks.map((desk) => {
        const on = desk.id === deskState.active;
        const row = document.createElement('li');
        row.className = `desks-row${on ? ' is-active' : ''}`;
        row.dataset.desk = desk.id;
        const pick = document.createElement('button');
        pick.type = 'button';
        pick.className = 'desks-pick';
        if (on) pick.setAttribute('aria-current', 'true');
        pick.innerHTML = `<span class="desks-check" aria-hidden="true">${icon(ICON.check, 13)}</span><span class="desks-name"></span><span class="desks-meta"></span>`;
        pick.children[1].textContent = desk.name;
        pick.children[2].textContent = `${cardCount(on ? cards.size : desk.cards.length)}${on ? ' · abierto' : ''}`;
        pick.addEventListener('click', () => {
          if (on) return closeManager(true);
          deskAction((state) => switchDesk(state, desk.id)).then(() => focusRow(desk.id));
        });
        row.append(
          pick,
          deskButton('desks-act', ICON.pencil, `Renombrar ${desk.name}`, () => renameDeskRow(row, desk)),
          deskButton('desks-act', ICON.archive, `Archivar ${desk.name}`, () =>
            deskAction((state) => archiveDesk(state, desk.id, new Date().toISOString(), newId())).then(() => focusRow(deskState.active)),
          ),
        );
        return row;
      }),
    );
    const add = $('desks-new');
    add.setAttribute('aria-disabled', String(full));
    $('desks-new-hint').textContent = full ? `Ya hay ${MAX_DESKS}: archiva uno para abrir otro` : 'Un lienzo vacío para otras notas';
    $('desks-archived-empty').hidden = deskState.archived.length > 0;
    $('desks-archived').replaceChildren(
      ...deskState.archived.map((desk) => {
        const row = document.createElement('li');
        row.className = 'desks-row';
        row.dataset.desk = desk.id;
        const info = document.createElement('div');
        info.className = 'desks-pick';
        info.innerHTML = '<span></span><span class="desks-name"></span><span class="desks-meta"></span>';
        info.children[1].textContent = desk.name;
        const when = desk.archivedAt ? new Date(desk.archivedAt) : null;
        const day = when && !Number.isNaN(when.getTime()) ? ` · ${when.toLocaleDateString('es', { day: 'numeric', month: 'short' })}` : '';
        info.children[2].textContent = `${cardCount(desk.cards.length)}${day}`;
        const load = deskButton('desks-act desks-load', null, full ? `Cargar ${desk.name}: antes archiva uno de la lista` : `Cargar ${desk.name}`, () =>
          deskAction((state) => restoreDesk(state, desk.id)).then(() => focusRow(desk.id)),
        );
        load.textContent = 'Cargar';
        load.disabled = full;
        row.append(
          info,
          load,
          deskButton('desks-act', ICON.trash, `Borrar ${desk.name}`, async () => {
            const sure = await ask(`¿Borrar el escritorio archivado «${desk.name}»? Sus notas no se borran: solo se pierde cómo estaban colocadas.`, { title: 'Borrar escritorio', kind: 'warning' });
            if (sure) deskAction((state) => removeArchived(state, desk.id)).then(() => $('desks-new').focus());
          }),
        );
        return row;
      }),
    );
  }

  function openManager() {
    desksMenu.hidden = false;
    desksButton.setAttribute('aria-expanded', 'true');
    renderManager();
    focusRow(deskState?.active);
  }
  function closeManager(refocus = false) {
    if (desksMenu.hidden) return;
    desksMenu.hidden = true;
    desksButton.setAttribute('aria-expanded', 'false');
    if (refocus) desksButton.focus();
  }
  desksButton.addEventListener('click', () => (desksMenu.hidden ? openManager() : closeManager(true)));
  $('desks-new').addEventListener('click', () => {
    if (!deskState || deskState.desks.length >= MAX_DESKS) return;
    deskAction((state) => createDesk(state, newId())).then(() => focusRow(deskState.active));
  });
  $('desk-manager').addEventListener('keydown', (event) => {
    if (event.key !== 'Escape' || desksMenu.hidden) return;
    event.stopPropagation();
    closeManager(true);
  });
  document.addEventListener('pointerdown', (event) => {
    if (!event.target.closest?.('#desk-manager')) closeManager();
  });

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
        deskState = null;
        useView(null);
        lastOpened = null;
        closeManager();
      }
      await loadTree();
      renderZoom();
      if (!deskState) {
        let raw = null;
        try {
          raw = await invoke('desks_load');
        } catch {}
        deskState = normalize(raw, newId());
        await applyDesk({ initial: true });
      }
      for (const card of cards.values()) place(card);
      applyCamera();
      renderDeskChrome();
    },
    // Se sale de la vista Notas: el lienzo se oculta y su desplazamiento deja de valer.
    pause() {
      camReady = false;
      closeManager();
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
      if (loaded && deskState) deskWork = deskWork.then(() => applyDesk()).catch(fail);
    },
    openNote,
    // Abre un tema (ruta relativa a wiki/notas/) en la carta de la izquierda.
    async openTopic(path) {
      if (!loaded) await loadTree();
      goTo(findTopic(path) ? path : '');
    },
    // La vista Notas vuelve a verse: cada carta en su sitio y la cámara donde estaba.
    relayout() {
      for (const card of cards.values()) place(card);
      applyCamera();
    },
  };
}
