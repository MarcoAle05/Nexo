// Workspace de la vista Agentes: una carpeta de proyecto con su propio orquestador de Claude
// Code (workspace.rs, workspace_run.rs). En la carta de la izquierda, el explorador de
// archivos, los agentes (Nexo, el orquestador y los que este despliega) y los Extras del perfil
// global, apagados hasta que el usuario los enciende; en el centro, una barra de pestañas y
// hasta MAX_OPEN ventanas a la vez: el chat de cada agente (el mismo de la carta derecha) y el
// lector de los archivos. Lo que dejó el usuario (pestañas, ventanas, carpetas abiertas, el
// modelo del orquestador) se guarda en el propio workspace y vuelve al abrirlo; los tokens se
// miden en directo y no se guardan.
import { createChat, mountChat } from './chat.js';
import { createChatModel } from './chat-model.js';
import { createModal } from './modal.js';
import { formatTokens } from './registro.js';
import { fileMeta, isMarkdown, languageOf, renderFile } from './ws-code.js';
import { MAX_OPEN, addTab, closeTab, fits, focusWindow, minimizeWindow, neighbor, normalizeWindows, openWindow, setMode, tileLayout, wheelDelta } from './ws-layout.js';
import { STATE_LABEL, agentState, createWorkspaceStream } from './ws-stream.js';

const svg = (paths, size = 16, width = 1.4) =>
  `<svg width="${size}" height="${size}" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="${width}" stroke-linecap="round" stroke-linejoin="round">${paths}</svg>`;
export const ICON = {
  folder: svg('<path d="M2.5 5.5V15.5h15V7.5H9.5L8 5.5z"/>'),
  folderOpen: svg('<path d="M2.5 15.5V5.5H8l1.5 2h6.5v2"/><path d="M2.5 15.5 5 9.5h12.5l-2.5 6z"/>'),
  folderPlus: svg('<path d="M2.5 5.5V15.5h15V7.5H9.5L8 5.5z"/><path d="M10 9.5v4M8 11.5h4"/>'),
  file: svg('<path d="M5 2.5h6.5l3.5 3.5v11.5H5z"/><path d="M11.5 2.5V6H15"/>', 15),
  code: svg('<path d="M5 2.5h6.5l3.5 3.5v11.5H5z"/><path d="m8.5 10.5-1.5 1.5 1.5 1.5M11.5 10.5l1.5 1.5-1.5 1.5"/>', 15),
  doc: svg('<path d="M5 2.5h6.5l3.5 3.5v11.5H5z"/><path d="M7.5 10h5M7.5 13h3.5"/>', 15),
  data: svg('<path d="M5 2.5h6.5l3.5 3.5v11.5H5z"/><path d="M8 10.5c-1 0-1 .5-1 1.5s-.5 1-1 1c.5 0 1 0 1 1s0 1.5 1 1.5M12 10.5c1 0 1 .5 1 1.5s.5 1 1 1c-.5 0-1 0-1 1s0 1.5-1 1.5" transform="translate(0 -1)"/>', 15),
  image: svg('<rect x="3.5" y="3.5" width="13" height="13" rx="2"/><circle cx="8" cy="8" r="1.4"/><path d="m4 15 4.5-4.5 3 3 2-2 3 3"/>', 15),
  chevron: svg('<path d="m8 5 5 5-5 5"/>', 12, 1.6),
  back: svg('<path d="M12 4 6 10l6 6"/>', 14, 1.6),
  refresh: svg('<path d="M16 10a6 6 0 1 1-1.8-4.3M16 4v3.5h-3.5"/>', 14, 1.5),
  collapse: svg('<path d="m6 8 4-4 4 4M6 12l4 4 4-4"/>', 14, 1.5),
  minimize: svg('<path d="M5 10h10"/>', 14, 1.6),
  close: svg('<path d="m5.5 5.5 9 9M14.5 5.5l-9 9"/>', 13, 1.6),
  tabs: svg('<rect x="3" y="4.5" width="14" height="11" rx="2"/><path d="M3 8h14M8 4.5V8"/>', 15),
  eye: svg('<path d="M2.5 10s2.7-5 7.5-5 7.5 5 7.5 5-2.7 5-7.5 5-7.5-5-7.5-5z"/><circle cx="10" cy="10" r="2.2"/>', 14),
  nexo: '<svg width="15" height="15" viewBox="0 0 22 22" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4"><circle cx="11" cy="11" r="3" fill="currentColor" stroke="none"/><ellipse cx="11" cy="11" rx="9.5" ry="4" transform="rotate(-24 11 11)"/></svg>',
};

const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
};
const baseName = (path) => String(path).split('/').filter(Boolean).pop() ?? String(path);
const sameSet = (a = [], b = []) => a.length === b.length && a.every((x) => b.includes(x));
const sameExtras = (a, b) => ['plugins', 'mcp', 'connectors', 'skills'].every((k) => sameSet(a?.[k] ?? [], b?.[k] ?? []));
const EMPTY_EXTRAS = () => ({ plugins: [], mcp: [], connectors: [], skills: [] });

// Icono de un archivo según su tipo.
export function fileIcon(path) {
  const [language] = languageOf(path);
  const ext = baseName(path).split('.').pop().toLowerCase();
  if (/^(png|jpe?g|gif|webp|svg|ico|bmp|avif)$/.test(ext)) return ICON.image;
  if (language === 'markdown' || ext === 'txt') return ICON.doc;
  if (language === 'json' || language === 'yaml' || language === 'ini') return ICON.data;
  if (language) return ICON.code;
  return ICON.file;
}

// Cómo se llama una pestaña/ventana: `main` (el orquestador), `a:<tarea>` (un agente) o
// `f:<ruta>` (un archivo).
export function activity(list) {
  const active = list.filter((a) => a.state === 'active').length;
  const waiting = list.filter((a) => a.state === 'waiting').length;
  return [active ? `${active} ${active === 1 ? 'activo' : 'activos'}` : '', waiting ? `${waiting} esperando` : ''].filter(Boolean).join(' · ');
}

export const keyKind = (key) => (key === 'main' ? 'main' : key.startsWith('a:') ? 'agent' : 'file');

export function createWorkspace({ invoke, Channel, desktop, pickFolder, pickFiles, ask, fail, renderMarkdown, onLink = () => {}, nexo, onMode = () => {}, onChange = () => {} }) {
  const $ = (id) => document.getElementById(id);
  const core = $('ws-core');
  const center = $('ws-center');
  const winBox = $('ws-windows');
  const tabsBox = $('ws-tabs');
  const tabsScroll = $('ws-tabs-scroll');
  const tree = $('ws-tree');

  let ws = null; // { path, name }
  let list = { current: null, recent: [], parent: '' };
  let registry = []; // agentes registrados en .nexo/agentes/
  let fresh = new Set(); // agentes recién desplegados que aún no se han visto en el registro
  let removed = new Set(); // agentes que quitó el orquestador: lo que aún llegue de ellos se ignora
  let defined = []; // .claude/agents/ del workspace
  let skills = []; // .claude/skills/ del workspace
  let catalog = null; // { plugins, mcp, connectors, skills, error } | { loading } | { error }
  let extras = EMPTY_EXTRAS();
  let applied = null; // extras con los que corre el orquestador
  let ui = {};
  let windows = normalizeWindows(null);
  let visible = false;
  let loading = false;
  let seq = 0; // cada workspace abierto: lo que llega tarde de uno anterior se descarta
  let stream = createWorkspaceStream();
  let replaying = false;
  let mainState = { busy: false, running: false, waiting: false, phase: 'idle' };
  const wins = new Map(); // clave → ventana { key, kind, el, chat?, file?, body, title, sub, icon }

  // ---------------------------------------------------------------- anuncios y guardado
  function announce(text) {
    const region = $('ws-live');
    region.textContent = '';
    requestAnimationFrame(() => (region.textContent = text));
  }

  let saveTimer = 0;
  function saveUi() {
    if (!ws) return;
    const path = ws.path;
    ui.windows = windows;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => invoke('workspace_save_ui', { path, ui }).catch(() => {}), 500);
  }

  // ---------------------------------------------------------------- menú de carpetas
  const dialog = $('ws-dialog');
  const createForm = $('ws-create');
  const createName = $('ws-create-name');
  let createParent = '';
  const modal = createModal({
    dialog,
    button: null,
    closeButton: $('ws-dialog-close'),
    onOpen: () => {
      closeCreate();
      renderRecent();
    },
    initialFocus: () => $('ws-pick'),
    onClose: () => lastTrigger?.isConnected && lastTrigger.focus(),
  });
  let lastTrigger = null;
  function openDialog(trigger = null) {
    lastTrigger = trigger;
    refreshList().then(renderRecent);
    modal.open();
  }

  function renderRecent() {
    const box = $('ws-recent');
    const items = list.recent.filter((r) => r.exists);
    $('ws-recent-box').hidden = items.length === 0;
    box.replaceChildren(
      ...items.map((r) => {
        const li = el('li', 'ws-recent-item');
        const open = el('button', 'ws-recent-open');
        open.type = 'button';
        const icon = el('span', 'ws-recent-icon');
        icon.innerHTML = ICON.folder;
        const text = el('span', 'ws-recent-text');
        const name = el('span', 'ws-recent-name', r.name);
        if (ws?.path === r.path) name.append(el('span', 'ws-badge', 'Abierto'));
        text.append(name, el('span', 'ws-recent-path', r.path));
        open.append(icon, text);
        open.title = r.path;
        open.addEventListener('click', () => {
          modal.close();
          if (ws?.path !== r.path) openWorkspace(r.path);
          else onMode('workspace');
        });
        const forget = el('button', 'icon-btn ws-recent-forget');
        forget.type = 'button';
        forget.innerHTML = ICON.close;
        forget.setAttribute('aria-label', `Quitar ${r.name} de la lista`);
        forget.title = 'Quitar de la lista (la carpeta no se toca)';
        forget.addEventListener('click', async () => {
          try {
            await invoke('workspace_forget', { path: r.path });
          } catch (error) {
            return fail(error);
          }
          if (ws?.path === r.path) leave();
          await refreshList();
          renderRecent();
          $('ws-pick').focus();
        });
        li.append(open, forget);
        return li;
      }),
    );
  }

  async function refreshList() {
    if (!desktop) return list;
    try {
      list = await invoke('workspace_list');
    } catch (error) {
      fail(error);
    }
    return list;
  }

  $('ws-pick').addEventListener('click', async () => {
    if (!desktop) return fail('Los workspaces solo funcionan en la app de escritorio.');
    const path = await pickFolder('Elige la carpeta del workspace');
    if (!path) return;
    try {
      const info = await invoke('workspace_open', { path });
      modal.close();
      await openWorkspace(info.path);
    } catch (error) {
      fail(error);
    }
  });

  function openCreate() {
    createForm.hidden = false;
    $('ws-new').setAttribute('aria-expanded', 'true');
    createParent = createParent || list.parent || '';
    renderCreate();
    createName.value = '';
    $('ws-create-error').hidden = true;
    requestAnimationFrame(() => createName.focus());
  }
  function closeCreate() {
    createForm.hidden = true;
    $('ws-new').setAttribute('aria-expanded', 'false');
  }
  function renderCreate() {
    $('ws-create-parent').textContent = createParent || 'Elige dónde';
    $('ws-create-parent').title = createParent;
    const name = createName.value.trim();
    $('ws-create-preview').textContent = name && createParent ? `Se creará ${createParent.replace(/\/$/, '')}/${name}` : 'Escribe un nombre para la carpeta.';
  }
  $('ws-new').addEventListener('click', () => (createForm.hidden ? openCreate() : closeCreate()));
  $('ws-create-cancel').addEventListener('click', () => {
    closeCreate();
    $('ws-new').focus();
  });
  createName.addEventListener('input', renderCreate);
  $('ws-create-browse').addEventListener('click', async () => {
    const path = await pickFolder('Dónde crear la carpeta');
    if (path) createParent = path;
    renderCreate();
    createName.focus();
  });
  createForm.addEventListener('submit', async (event) => {
    event.preventDefault();
    const name = createName.value.trim();
    const error = $('ws-create-error');
    if (!name) return createName.focus();
    if (!desktop) return fail('Los workspaces solo funcionan en la app de escritorio.');
    try {
      const info = await invoke('workspace_create', { parent: createParent, name });
      modal.close();
      await openWorkspace(info.path);
    } catch (e) {
      error.textContent = String(e);
      error.hidden = false;
      createName.focus();
    }
  });

  // ---------------------------------------------------------------- abrir y dejar un workspace
  function loadingStep(text) {
    $('ws-loading-step').textContent = text;
  }
  let loadingTimer = 0;
  function setLoading(on, title = '') {
    loading = on;
    clearTimeout(loadingTimer);
    core.classList.toggle('is-loading', on);
    if (on) {
      $('ws-loading-title').textContent = title;
      // Lo que tarda menos que esto no merece pantalla de carga: se vería un parpadeo.
      loadingTimer = setTimeout(() => ($('ws-loading').hidden = false), 160);
    } else $('ws-loading').hidden = true;
  }

  function disposeWindows() {
    for (const win of wins.values()) {
      win.chat?.dispose();
      win.el.remove();
    }
    wins.clear();
    stream.reset();
    stream = createWorkspaceStream();
  }

  // Deja de mirar el workspace abierto: su orquestador se cierra cuando termine.
  function detach() {
    if (!ws) return;
    const path = ws.path;
    clearTimeout(saveTimer);
    invoke('workspace_save_ui', { path, ui: { ...ui, windows } }).catch(() => {});
    invoke('workspace_detach', { path }).catch(() => {});
    disposeWindows();
  }

  function leave() {
    detach();
    ws = null;
    registry = [];
    catalog = null;
    applied = null;
    render();
    onChange(null);
  }

  async function openWorkspace(path) {
    const mine = ++seq;
    if (ws && ws.path !== path) detach();
    onMode('workspace');
    setLoading(true, `Abriendo ${baseName(path)}…`);
    loadingStep('Leyendo la carpeta');
    try {
      const state = await invoke('workspace_state', { path });
      if (mine !== seq) return;
      ws = { path: state.info.path, name: state.info.name };
      list.current = state.info;
      registry = state.agents ?? [];
      defined = state.defined ?? [];
      skills = state.skills ?? [];
      extras = { ...EMPTY_EXTRAS(), ...(state.extras ?? {}) };
      ui = state.ui && typeof state.ui === 'object' ? state.ui : {};
      expanded = new Set(Array.isArray(ui.expanded) ? ui.expanded : []);
      sections = { files: true, agents: true, extras: true, ...(ui.sections ?? {}) };
      children.clear();
      applied = null;
      fresh = new Set();
      removed = new Set();
      const keys = new Set(['main', ...registry.map((a) => `a:${a.task}`), ...((ui.windows?.tabs ?? []).filter((k) => k.startsWith('f:')))]);
      windows = normalizeWindows(ui.windows, keys);
      windows = addTab(windows, 'main');
      // El orquestador siempre es la primera pestaña.
      windows.tabs = ['main', ...windows.tabs.filter((k) => k !== 'main')];
      if (!windows.open.length) windows = openWindow(windows, 'main').state;
      onChange(ws);
      render();
      loadingStep('Leyendo el explorador');
      await loadTree();
      if (mine !== seq) return;
      loadingStep('Abriendo el orquestador');
      ensureWindow('main');
      for (const key of windows.tabs) if (key !== 'main') ensureWindow(key);
      layoutWindows();
      // El catálogo de extras y el arranque van a la vez: el arranque también lo necesita.
      loadCatalog(false);
      const started = wins.get('main').chat.start();
      await Promise.race([started, new Promise((r) => setTimeout(r, 6000))]);
    } catch (error) {
      if (mine === seq) {
        fail(error);
        if (!ws) render();
      }
    } finally {
      if (mine === seq) setLoading(false);
      render();
    }
  }

  // ---------------------------------------------------------------- explorador
  let expanded = new Set(); // carpetas abiertas (rutas relativas; '' es la raíz)
  let sections = { files: true, agents: true, extras: true };
  const children = new Map(); // carpeta → { entries, truncated } | { error }
  let focusPath = null;

  async function listDir(dir) {
    if (!ws) return;
    const path = ws.path;
    try {
      const listing = await invoke('workspace_tree', { path, dir });
      if (ws?.path === path) children.set(dir, listing);
    } catch (error) {
      if (ws?.path === path) children.set(dir, { error: String(error), entries: [] });
    }
  }

  async function loadTree() {
    await listDir('');
    // Las carpetas que el usuario dejó abiertas, si siguen existiendo.
    for (const dir of [...expanded].sort()) {
      if (!dir) continue;
      const parent = dir.includes('/') ? dir.slice(0, dir.lastIndexOf('/')) : '';
      if (!children.get(parent)?.entries?.some((e) => e.path === dir && e.dir)) {
        expanded.delete(dir);
        continue;
      }
      await listDir(dir);
    }
    renderTree();
  }

  // Lo visible del árbol, en orden: cada fila con su nivel (para aria-level y la sangría).
  function visibleRows() {
    const rows = [];
    const walk = (dir, level) => {
      const listing = children.get(dir);
      if (!listing) return;
      if (listing.error) {
        rows.push({ kind: 'note', level, text: listing.error, key: `e:${dir}` });
        return;
      }
      listing.entries.forEach((entry, i) => {
        rows.push({ kind: 'entry', entry, level, pos: i + 1, size: listing.entries.length, key: entry.path });
        if (entry.dir && expanded.has(entry.path)) walk(entry.path, level + 1);
      });
      if (listing.truncated) rows.push({ kind: 'note', level, text: 'Hay más archivos de los que se enseñan aquí.', key: `t:${dir}` });
      if (!listing.entries.length && dir) rows.push({ kind: 'note', level, text: 'Carpeta vacía', key: `v:${dir}` });
    };
    walk('', 1);
    return rows;
  }

  function renderTree() {
    const rows = visibleRows();
    if (!rows.some((r) => r.kind === 'entry' && r.key === focusPath)) focusPath = rows.find((r) => r.kind === 'entry')?.key ?? null;
    const openFile = windows.active?.startsWith('f:') ? windows.active.slice(2) : null;
    const items = rows.map((row) => {
      const li = el('li', row.kind === 'note' ? 'ws-tree-note' : 'ws-tree-item');
      li.style.setProperty('--level', row.level);
      if (row.kind === 'note') {
        li.textContent = row.text;
        li.setAttribute('role', 'none');
        return li;
      }
      const { entry } = row;
      li.setAttribute('role', 'treeitem');
      li.dataset.path = entry.path;
      li.dataset.dir = entry.dir ? '1' : '';
      li.setAttribute('aria-level', row.level);
      li.setAttribute('aria-setsize', row.size);
      li.setAttribute('aria-posinset', row.pos);
      li.tabIndex = entry.path === focusPath ? 0 : -1;
      if (entry.dir) li.setAttribute('aria-expanded', String(expanded.has(entry.path)));
      li.setAttribute('aria-selected', String(entry.path === openFile));
      const twist = el('span', 'ws-tree-twist');
      twist.setAttribute('aria-hidden', 'true');
      if (entry.dir) twist.innerHTML = ICON.chevron;
      const icon = el('span', 'ws-tree-icon');
      icon.setAttribute('aria-hidden', 'true');
      icon.innerHTML = entry.dir ? (expanded.has(entry.path) ? ICON.folderOpen : ICON.folder) : fileIcon(entry.path);
      const name = el('span', 'ws-tree-name', entry.name);
      li.title = entry.path;
      li.append(twist, icon, name);
      if (entry.link) li.append(el('span', 'ws-tree-link', '↗'));
      return li;
    });
    tree.replaceChildren(...items);
  }

  async function toggleDir(dir, open = !expanded.has(dir)) {
    if (open) {
      expanded.add(dir);
      if (!children.has(dir)) await listDir(dir);
    } else {
      expanded.delete(dir);
      for (const d of [...expanded]) if (d.startsWith(`${dir}/`)) expanded.delete(d);
    }
    ui.expanded = [...expanded];
    saveUi();
    renderTree();
  }

  function focusTreeItem(path) {
    focusPath = path;
    for (const li of tree.querySelectorAll('[role="treeitem"]')) li.tabIndex = li.dataset.path === path ? 0 : -1;
    tree.querySelector(`[data-path="${CSS.escape(path)}"]`)?.focus();
  }

  tree.addEventListener('click', (event) => {
    const li = event.target.closest('[role="treeitem"]');
    if (!li) return;
    focusTreeItem(li.dataset.path);
    if (li.dataset.dir) toggleDir(li.dataset.path).then(() => focusTreeItem(li.dataset.path));
    else openFile(li.dataset.path, { focus: false });
  });
  tree.addEventListener('keydown', (event) => {
    const items = [...tree.querySelectorAll('[role="treeitem"]')];
    const at = items.findIndex((li) => li.dataset.path === focusPath);
    const li = items[at];
    if (!li) return;
    const go = (i) => {
      event.preventDefault();
      const target = items[Math.max(0, Math.min(items.length - 1, i))];
      if (target) focusTreeItem(target.dataset.path);
    };
    const path = li.dataset.path;
    if (event.key === 'ArrowDown') go(at + 1);
    else if (event.key === 'ArrowUp') go(at - 1);
    else if (event.key === 'Home') go(0);
    else if (event.key === 'End') go(items.length - 1);
    else if (event.key === 'ArrowRight') {
      event.preventDefault();
      if (li.dataset.dir && !expanded.has(path)) toggleDir(path, true).then(() => focusTreeItem(path));
      else if (li.dataset.dir) go(at + 1);
    } else if (event.key === 'ArrowLeft') {
      event.preventDefault();
      if (li.dataset.dir && expanded.has(path)) toggleDir(path, false).then(() => focusTreeItem(path));
      else if (path.includes('/')) focusTreeItem(path.slice(0, path.lastIndexOf('/')));
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      if (li.dataset.dir) toggleDir(path).then(() => focusTreeItem(path));
      else openFile(path, { focus: event.key === 'Enter' });
    }
  });
  $('ws-files-refresh').addEventListener('click', () => refreshTree(true));
  $('ws-files-collapse').addEventListener('click', () => {
    expanded.clear();
    ui.expanded = [];
    saveUi();
    renderTree();
  });

  // Vuelve a leer lo que está abierto (el orquestador o sus agentes escribieron algo).
  let refreshTimer = 0;
  async function refreshTree(now = false) {
    clearTimeout(refreshTimer);
    const run = async () => {
      if (!ws) return;
      await Promise.all(['', ...expanded].map(listDir));
      renderTree();
      for (const win of wins.values()) if (win.kind === 'file' && win.el.isConnected) reloadFile(win, true);
    };
    if (now) return run();
    refreshTimer = setTimeout(run, 700);
  }

  // ---------------------------------------------------------------- registro de agentes
  let agentsTimer = 0;
  function refreshAgents() {
    clearTimeout(agentsTimer);
    agentsTimer = setTimeout(async () => {
      if (!ws) return;
      const path = ws.path;
      let next;
      try {
        next = await invoke('workspace_agents', { path });
      } catch {
        return;
      }
      if (ws?.path !== path) return;
      const before = new Set(registry.map((a) => a.task));
      registry = next;
      const now = new Set(registry.map((a) => a.task));
      for (const task of fresh) if (now.has(task)) fresh.delete(task);
      // Lo que el orquestador quitó (borró su archivo) desaparece de la carta y del centro.
      for (const task of before) {
        if (now.has(task)) continue;
        const key = `a:${task}`;
        removed.add(task);
        dropWindow(key);
        stream.forget(task);
        announce('El orquestador quitó un agente del workspace.');
      }
      render();
    }, 300);
  }

  // Todo lo que se sabe de un agente: su registro (nombre y rol que puede cambiar el
  // orquestador) y lo que llega en directo (estado y tokens).
  function agentInfo(task) {
    const reg = registry.find((a) => a.task === task);
    const live = stream.get(task);
    const win = wins.get(`a:${task}`);
    const waiting = Boolean(win?.chat?.waiting);
    return {
      task,
      key: `a:${task}`,
      name: reg?.name || live?.name || 'Agente',
      role: reg?.role || live?.summary || '',
      type: reg?.type || live?.type || 'general-purpose',
      prompt: reg?.prompt || live?.prompt || '',
      created: reg?.created || '',
      state: agentState(live, waiting),
      tokens: stream.tokensOf(live),
      lastTool: live?.lastTool || '',
      registered: Boolean(reg),
    };
  }

  function agentList() {
    const tasks = [...registry.map((a) => a.task)];
    for (const task of fresh) if (!tasks.includes(task)) tasks.push(task);
    return tasks.map(agentInfo);
  }

  const mainInfo = () => ({
    key: 'main',
    name: 'Orquestador',
    role: 'El agente principal de la carpeta',
    state: mainState.waiting ? 'waiting' : mainState.busy ? 'active' : 'idle',
    tokens: stream.mainTokens(),
  });

  function infoOf(key) {
    if (key === 'main') return mainInfo();
    if (key.startsWith('a:')) return agentInfo(key.slice(2));
    return { key, name: baseName(key.slice(2)), path: key.slice(2) };
  }

  // ---------------------------------------------------------------- protocolo del orquestador
  function dispatch(event, mainEvent) {
    if (event.kind === 'exit') {
      mainEvent(event);
      for (const win of wins.values()) if (win.kind === 'agent') win.chat?.receive(event);
      const changed = stream.exit();
      if (changed.length) syncAgents(changed);
      return;
    }
    let msg;
    try {
      msg = JSON.parse(event.text);
    } catch {
      return;
    }
    const history = replaying || Boolean(msg.nexo_history);
    const out = stream.route(msg);
    for (const target of out.targets) {
      if (target.to === 'main') mainEvent({ kind: 'line', msg: target.msg, history });
      else if (!removed.has(target.to)) ensureWindow(`a:${target.to}`)?.chat?.receive({ kind: 'line', msg: target.msg, history });
    }
    for (const task of out.created) {
      // Si vuelve a empezar (retomado), vuelve al workspace: nexo lo registró otra vez.
      removed.delete(task);
      fresh.add(task);
      windows = addTab(windows, `a:${task}`);
      ensureWindow(`a:${task}`);
      if (!history) {
        // Un agente nuevo se ve en cuanto aparece si cabe en el mosaico con su medida; si no,
        // queda en su pestaña, con su punto de estado.
        const box = winBox.getBoundingClientRect();
        if (windows.mode === 'mosaico' && windows.open.length < MAX_OPEN && (!box.width || fits(windows.open.length + 1, box.width, box.height))) {
          windows = { ...windows, open: [...windows.open, `a:${task}`] };
          layoutWindows();
        }
        announce(`Nuevo agente en el workspace: ${agentInfo(task).name}.`);
        saveUi();
      }
    }
    if (out.created.length) refreshAgents();
    if (out.changed.size || out.created.length) syncAgents([...out.changed, ...out.created]);
    // Lo que escriben el orquestador y sus agentes puede cambiar archivos y el registro.
    if (!history && (msg.type === 'result' || (msg.type === 'user' && Array.isArray(msg.message?.content) && msg.message.content.some((b) => b?.type === 'tool_result')))) {
      refreshTree();
      refreshAgents();
    }
  }

  function syncAgents(tasks) {
    const main = wins.get('main')?.chat;
    for (const task of new Set(tasks)) {
      const agent = stream.get(task);
      const win = wins.get(`a:${task}`);
      if (win?.chat) win.chat.setBusy(agentState(agent) === 'active');
      if (agent?.toolUse) main?.refresh([agent.toolUse]);
    }
    scheduleRender();
  }

  // El transporte del orquestador. `owner` es el workspace abierto cuando se creó: lo que
  // llegue tarde de uno que ya se dejó no se mezcla con el de ahora.
  function orchestratorTransport(path, owner) {
    return {
      async start({ options, replay, onEvent }) {
        const channel = new Channel();
        // Lo que se repite al conectar llega antes de `nexo_replay_end`.
        replaying = replay;
        channel.onmessage = (event) => {
          if (owner !== seq) return;
          if (event.kind === 'line' && event.text === '{"type":"nexo_replay_end"}') {
            replaying = false;
            return;
          }
          dispatch(event, onEvent);
        };
        try {
          return await invoke('workspace_start', { path, options, replay, channel });
        } catch (error) {
          replaying = false;
          throw error;
        }
      },
      send: (line, record) => invoke('workspace_send', { path, line, record }),
      reset: () => invoke('workspace_reset', { path }),
    };
  }

  // Lo que el usuario escribe en la ventana de un agente: se lo pasa el orquestador.
  function relayToAgent(task, text, files) {
    const info = agentInfo(task);
    const main = ensureWindow('main').chat;
    const body = `[nexo → agente ${task}] ${info.name}\n${text.trim()}\n\n—\nEs un mensaje del usuario para ese agente, escrito en su ventana de nexo: pásaselo con SendMessage (destinatario: ${task}) sin contestarlo tú. Si ya terminó y no se puede continuar, vuelve a encargarle el trabajo con Agent y el mismo subagent_type (${info.type}).${files.length ? `\nArchivos adjuntos (léelos con Read):\n${files.map((f) => `- ${f}`).join('\n')}` : ''}`;
    return main.relay(body, { text: text.trim(), files, to: task, toName: info.name });
  }

  // ---------------------------------------------------------------- ventanas
  let winSeq = 0;
  function ensureWindow(key) {
    if (wins.has(key)) return wins.get(key);
    if (!ws) return null;
    const kind = keyKind(key);
    const id = `ws-${++winSeq}`;
    const section = el('section', 'ws-win');
    section.dataset.key = key;
    section.dataset.kind = kind;
    section.setAttribute('aria-labelledby', `${id}-title`);
    const head = el('header', 'ws-win-head');
    const icon = el('span', 'ws-win-icon');
    icon.setAttribute('aria-hidden', 'true');
    const heading = el('div', 'ws-win-heading');
    const title = el('h3', 'ws-win-title');
    title.id = `${id}-title`;
    const sub = el('p', 'ws-win-sub');
    heading.append(title, sub);
    const actions = el('div', 'ws-win-actions');
    const minimize = el('button', 'icon-btn ws-win-btn');
    minimize.type = 'button';
    minimize.innerHTML = ICON.minimize;
    minimize.addEventListener('click', () => minimizeKey(key));
    actions.append(minimize);
    let preview = null;
    if (kind === 'file') {
      if (isMarkdown(key.slice(2))) {
        preview = el('button', 'ws-win-chip');
        preview.type = 'button';
        preview.setAttribute('aria-pressed', String(Boolean(ui.preview?.[key.slice(2)])));
        preview.innerHTML = `${ICON.eye}<span>Vista previa</span>`;
        preview.addEventListener('click', () => {
          const on = preview.getAttribute('aria-pressed') !== 'true';
          preview.setAttribute('aria-pressed', String(on));
          ui.preview = { ...(ui.preview ?? {}), [key.slice(2)]: on || undefined };
          saveUi();
          paintFile(wins.get(key));
        });
        actions.prepend(preview);
      }
      const close = el('button', 'icon-btn ws-win-btn');
      close.type = 'button';
      close.innerHTML = ICON.close;
      close.addEventListener('click', () => closeKey(key));
      actions.append(close);
    }
    head.append(icon, heading, actions);
    const body = el('div', 'ws-win-body');
    section.append(head, body);
    section.addEventListener('focusin', () => {
      if (windows.active !== key && windows.open.includes(key)) {
        windows = focusWindow(windows, key);
        scheduleRender();
        saveUi();
      }
    });
    const win = { key, kind, el: section, body, title, sub, icon, minimize, preview, file: null, chat: null };
    wins.set(key, win);

    if (kind === 'file') reloadFile(win);
    else {
      const chatView = el('div', 'chat-view');
      body.append(chatView);
      const info = infoOf(key);
      mountChat(chatView, id, { label: `Conversación con ${info.name}`, placeholder: kind === 'main' ? 'Escríbele al orquestador…' : 'Escríbele a este agente…', inputLabel: `Mensaje para ${info.name}` });
      const path = ws.path;
      if (kind === 'main') {
        win.chat = createChat({
          root: section,
          prefix: id,
          transport: orchestratorTransport(path, seq),
          desktop,
          speaker: 'Orquestador',
          prefs: { load: () => ui.chat ?? {}, save: (p) => ((ui.chat = p), saveUi()) },
          getCwd: () => path,
          pickFiles,
          onLink,
          agentFor: (toolUse) => {
            const agent = stream.byToolUse(toolUse);
            return agent ? agentInfo(agent.id) : null;
          },
          onOpenAgent: (agent) => agent && openKey(agent.key, { focus: true }),
          onState: (s) => {
            mainState = s;
            scheduleRender();
          },
          onStarted: (info) => {
            applied = info?.applied ?? null;
            scheduleRender();
          },
          texts: {
            idle: ['Orquestador', 'El agente principal de esta carpeta: reparte el trabajo entre sus agentes.'],
            starting: ['Abriendo el orquestador…', 'Arrancando Claude Code en la carpeta del workspace.'],
            ready: ['El orquestador está listo', 'Pídele lo que quieras hacer en esta carpeta: lo hará él o lo repartirá entre agentes, que aparecerán aquí.'],
          },
        });
      } else {
        const task = key.slice(2);
        const agentModel = createChatModel({ prompts: true });
        win.chat = createChat({
          root: section,
          prefix: id,
          transport: { send: (line, record) => invoke('workspace_send', { path, line, record }), start: async () => ({}), reset: async () => {} },
          desktop,
          feed: true,
          model: agentModel,
          speaker: info.name,
          submitUser: (text, files) => relayToAgent(task, text, files),
          // «Detener» (o Escape) para la tarea del agente; quitarlo del workspace es cosa del
          // orquestador.
          onInterrupt: () => {
            if (agentInfo(task).state !== 'active') return;
            const line = JSON.stringify({ type: 'control_request', request_id: `nexo-stop-${task}-${Date.now().toString(36)}`, request: { subtype: 'stop_task', task_id: task } });
            invoke('workspace_send', { path, line, record: null }).then(() => announce(`Se pidió detener a ${agentInfo(task).name}.`), fail);
          },
          getCwd: () => path,
          pickFiles,
          onLink,
          onState: () => scheduleRender(),
          texts: { ready: [info.name, 'Aquí se ve lo que hace este agente. Lo que le escribas se lo pasa el orquestador.'] },
        });
        // Su encargo, lo primero de su conversación.
        if (info.prompt) agentModel.addUser(info.prompt, [], { local: true }).from = 'Encargo del orquestador';
        win.chat.setBusy(info.state === 'active');
      }
    }
    paintWindowHead(win);
    return win;
  }

  function paintWindowHead(win) {
    const info = infoOf(win.key);
    win.title.textContent = info.name;
    if (win.kind === 'file') {
      win.icon.innerHTML = fileIcon(info.path);
      win.sub.textContent = win.file?.view ? `${info.path} · ${fileMeta(win.file.view)}` : info.path;
      win.sub.title = info.path;
      win.minimize.setAttribute('aria-label', `Minimizar ${info.name}`);
      win.minimize.title = 'Minimizar';
      win.el.querySelector('.ws-win-btn:last-child')?.setAttribute('aria-label', `Cerrar ${info.name}`);
      win.el.querySelector('.ws-win-btn:last-child')?.setAttribute('title', 'Cerrar');
      return;
    }
    win.icon.replaceChildren(stateDot(info.state));
    const parts = [];
    if (win.kind === 'main') parts.push('Principal');
    else if (info.role) parts.push(info.role);
    if (info.tokens) parts.push(`${formatTokens(info.tokens)} tokens`);
    parts.push(STATE_LABEL[info.state]);
    win.sub.textContent = parts.join(' · ');
    win.sub.title = parts.join(' · ');
    win.el.dataset.state = info.state;
    win.minimize.setAttribute('aria-label', `Minimizar la ventana de ${info.name}`);
    win.minimize.title = 'Minimizar';
  }

  async function reloadFile(win, quiet = false) {
    if (!ws) return;
    const path = ws.path;
    const file = win.key.slice(2);
    if (!quiet) {
      win.body.replaceChildren(el('p', 'ws-code-loading', 'Leyendo…'));
    }
    try {
      const view = await invoke('workspace_read', { path, file });
      if (ws?.path !== path) return;
      if (quiet && win.file?.view && win.file.view.modified === view.modified && win.file.view.size === view.size) return;
      win.file = { view, error: null };
    } catch (error) {
      if (quiet && win.file?.view) {
        win.file = { view: null, error: String(error) };
      } else win.file = { view: null, error: String(error) };
    }
    paintFile(win);
    paintWindowHead(win);
  }

  function paintFile(win) {
    if (!win) return;
    const scrolled = win.body.querySelector('.ws-code, .ws-preview');
    const top = scrolled?.scrollTop ?? 0;
    const left = scrolled?.scrollLeft ?? 0;
    if (win.file?.error) {
      const note = el('div', 'ws-code-note');
      note.append(el('p', 'ws-code-note-title', 'No se pudo leer el archivo'), el('p', '', win.file.error));
      win.body.replaceChildren(note);
      return;
    }
    if (!win.file?.view) return;
    const file = win.key.slice(2);
    renderFile(win.body, win.file.view, { preview: Boolean(ui.preview?.[file]) && isMarkdown(file), renderMarkdown });
    const next = win.body.querySelector('.ws-code, .ws-preview');
    if (next) {
      next.scrollTop = top;
      next.scrollLeft = left;
    }
    win.body.querySelector('.ws-preview')?.addEventListener('click', (event) => {
      const link = event.target.closest('a');
      if (!link) return;
      event.preventDefault();
      onLink(link);
    });
  }

  function openFile(file, { focus = true } = {}) {
    openKey(`f:${file}`, { focus });
  }

  // Abre (o enseña) la ventana de una pestaña; en mosaico va junto a las demás.
  function openKey(key, { focus = true } = {}) {
    if (!ws) return;
    ensureWindow(key);
    const { state, evicted } = openWindow(windows, key);
    windows = state;
    if (evicted) announce(`${infoOf(evicted).name} se minimizó para dejar sitio.`);
    layoutWindows();
    render();
    saveUi();
    if (focus) requestAnimationFrame(() => focusWin(key));
  }

  function focusWin(key) {
    const win = wins.get(key);
    if (!win?.el.isConnected) return;
    if (win.chat) win.chat.focus();
    else (win.body.querySelector('.ws-code, .ws-preview') ?? win.minimize).focus();
  }

  function focusTab(key) {
    requestAnimationFrame(() => tabsBox.querySelector(`[data-key="${CSS.escape(key)}"] .ws-tab-main`)?.focus());
  }

  function minimizeKey(key) {
    windows = minimizeWindow(windows, key);
    layoutWindows();
    render();
    saveUi();
    focusTab(key);
  }

  function closeKey(key) {
    if (keyKind(key) !== 'file') return;
    const at = windows.tabs.indexOf(key);
    windows = closeTab(windows, key);
    dropWindow(key);
    render();
    saveUi();
    const next = windows.tabs[Math.min(at, windows.tabs.length - 1)];
    if (next) focusTab(next);
  }

  function dropWindow(key) {
    const win = wins.get(key);
    if (win) {
      win.chat?.dispose();
      win.el.remove();
      wins.delete(key);
    }
    if (windows.tabs.includes(key)) windows = closeTab(windows, key);
    layoutWindows();
  }

  // Coloca las ventanas que se ven en la rejilla del centro (y quita las demás).
  let layoutFrame = 0;
  function layoutWindows() {
    cancelAnimationFrame(layoutFrame);
    layoutFrame = requestAnimationFrame(() => {
      layoutFrame = 0;
      const open = windows.open.filter((k) => wins.has(k));
      for (const [key, win] of wins) {
        const shown = open.includes(key);
        if (!shown && win.el.isConnected) win.el.remove();
      }
      const rect = winBox.getBoundingClientRect();
      const layout = tileLayout(open.length, rect.width || 800, rect.height || 600);
      winBox.style.gridTemplateColumns = `repeat(${Math.max(1, layout.cols)}, minmax(0, 1fr))`;
      winBox.style.gridTemplateRows = `repeat(${Math.max(1, layout.rows)}, minmax(0, 1fr))`;
      open.forEach((key, i) => {
        const win = wins.get(key);
        const cell = layout.cells[i];
        win.el.style.gridColumn = `${cell.col} / span ${cell.colSpan}`;
        win.el.style.gridRow = `${cell.row} / span ${cell.rowSpan}`;
        if (!win.el.isConnected) {
          winBox.append(win.el);
          win.chat?.show({ focus: false });
        }
        win.el.classList.toggle('is-active', key === windows.active && open.length > 1);
      });
      $('ws-empty').hidden = open.length > 0 || !ws;
    });
  }
  new ResizeObserver(() => visible && layoutWindows()).observe(winBox);

  // ---------------------------------------------------------------- barra de pestañas
  function stateDot(state) {
    const dot = el('span', 'ws-state');
    dot.dataset.state = state;
    dot.title = STATE_LABEL[state];
    return dot;
  }

  let shownActive = null;
  function renderTabs() {
    const keys = windows.tabs.filter((k) => k === 'main' || wins.has(k) || k.startsWith('f:'));
    let roving = tabsBox.querySelector('.ws-tab-main[tabindex="0"]')?.closest('[data-key]')?.dataset.key;
    if (!keys.includes(roving)) roving = windows.active ?? keys[0];
    const existing = new Map([...tabsBox.children].map((li) => [li.dataset.key, li]));
    const items = keys.map((key) => {
      const info = infoOf(key);
      const kind = keyKind(key);
      let li = existing.get(key);
      if (!li) {
        li = el('li', 'ws-tab');
        li.dataset.key = key;
        li.dataset.kind = kind;
        const main = el('button', 'ws-tab-main');
        main.type = 'button';
        main.append(el('span', 'ws-tab-icon'), el('span', 'ws-tab-name'), el('span', 'visually-hidden ws-tab-state'));
        main.addEventListener('click', () => {
          if (windows.open.includes(key) && windows.active === key && windows.mode === 'mosaico') focusWin(key);
          else openKey(key, { focus: true });
        });
        li.append(main);
        if (kind === 'file') {
          main.setAttribute('aria-keyshortcuts', 'Delete');
          const close = el('button', 'ws-tab-close');
          close.type = 'button';
          close.tabIndex = -1;
          close.innerHTML = ICON.close;
          close.addEventListener('click', (event) => {
            event.stopPropagation();
            closeKey(key);
          });
          li.append(close);
        }
      }
      const main = li.firstChild;
      const shown = windows.open.includes(key);
      main.setAttribute('aria-pressed', String(shown));
      if (key === windows.active) main.setAttribute('aria-current', 'true');
      else main.removeAttribute('aria-current');
      main.tabIndex = key === roving ? 0 : -1;
      const icon = main.querySelector('.ws-tab-icon');
      if (kind === 'file') {
        if (!icon.firstChild) icon.innerHTML = fileIcon(info.path);
        main.title = `${info.path}${shown ? '' : ' · minimizada'}`;
        main.querySelector('.ws-tab-state').textContent = shown ? ', a la vista' : ', minimizada';
        li.lastChild.setAttribute('aria-label', `Cerrar ${info.name}`);
        li.lastChild.title = 'Cerrar';
      } else {
        icon.replaceChildren(stateDot(info.state));
        main.title = `${info.name} · ${STATE_LABEL[info.state]}${shown ? '' : ' · minimizada'}`;
        main.querySelector('.ws-tab-state').textContent = `, ${STATE_LABEL[info.state].toLowerCase()}${shown ? '' : ', minimizada'}`;
      }
      main.querySelector('.ws-tab-name').textContent = info.name;
      li.classList.toggle('is-shown', shown);
      li.classList.toggle('is-active', key === windows.active);
      return li;
    });
    for (const [key, li] of existing) if (!keys.includes(key)) li.remove();
    items.forEach((li, i) => {
      if (tabsBox.children[i] !== li) tabsBox.insertBefore(li, tabsBox.children[i] ?? null);
    });
    // La pestaña de la ventana activa, a la vista en el carrusel.
    if (windows.active !== shownActive) {
      shownActive = windows.active;
      requestAnimationFrame(() => tabsBox.querySelector(`[data-key="${CSS.escape(shownActive ?? '')}"]`)?.scrollIntoView({ block: 'nearest', inline: 'nearest' }));
    }
    $('ws-all-count').textContent = String(keys.length);
    $('ws-all').setAttribute('aria-label', `Ver las ${keys.length} pestañas`);
    syncTabEdges();
  }

  function syncTabEdges() {
    const max = tabsScroll.scrollWidth - tabsScroll.clientWidth;
    tabsScroll.classList.toggle('fade-start', tabsScroll.scrollLeft > 1);
    tabsScroll.classList.toggle('fade-end', tabsScroll.scrollLeft < max - 1);
  }
  tabsScroll.addEventListener('scroll', syncTabEdges, { passive: true });
  // La rueda del ratón desplaza las pestañas en horizontal (en carrusel, sin encogerlas).
  tabsScroll.addEventListener(
    'wheel',
    (event) => {
      if (tabsScroll.scrollWidth <= tabsScroll.clientWidth) return;
      event.preventDefault();
      tabsScroll.scrollLeft += wheelDelta(event, tabsScroll.clientWidth);
    },
    { passive: false },
  );
  tabsBox.addEventListener('keydown', (event) => {
    const current = event.target.closest('.ws-tab')?.dataset.key;
    if (!current) return;
    const keys = [...tabsBox.children].map((li) => li.dataset.key);
    let next = null;
    if (event.key === 'ArrowRight') next = neighbor(keys, current, 1);
    else if (event.key === 'ArrowLeft') next = neighbor(keys, current, -1);
    else if (event.key === 'Home') next = keys[0];
    else if (event.key === 'End') next = keys.at(-1);
    else if ((event.key === 'Delete' || event.key === 'Backspace') && keyKind(current) === 'file') {
      event.preventDefault();
      return closeKey(current);
    }
    if (!next) return;
    event.preventDefault();
    for (const button of tabsBox.querySelectorAll('.ws-tab-main')) button.tabIndex = -1;
    const button = tabsBox.querySelector(`[data-key="${CSS.escape(next)}"] .ws-tab-main`);
    button.tabIndex = 0;
    button.focus();
    button.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  });

  // «Todas»: un vistazo a todas las pestañas, por tipo, con cuáles se ven.
  const overview = $('ws-overview');
  const allButton = $('ws-all');
  function closeOverview(refocus = false) {
    if (overview.hidden) return;
    overview.hidden = true;
    allButton.setAttribute('aria-expanded', 'false');
    if (refocus) allButton.focus();
  }
  function renderOverview() {
    const keys = windows.tabs;
    const groups = [
      ['Agentes', keys.filter((k) => keyKind(k) !== 'file')],
      ['Archivos', keys.filter((k) => keyKind(k) === 'file')],
    ];
    const parts = [];
    const head = el('div', 'vault-menu-title ws-overview-title');
    head.append(el('span', '', 'Pestañas'), el('span', 'ws-overview-count', `${keys.length} · ${windows.open.length} a la vista`));
    parts.push(head);
    for (const [title, group] of groups) {
      if (!group.length) continue;
      parts.push(el('div', 'ws-overview-group', `${title} · ${group.length}`));
      const ul = el('ul', 'ws-overview-list');
      for (const key of group) {
        const info = infoOf(key);
        const li = el('li');
        const button = el('button', 'vault-option ws-overview-item');
        button.type = 'button';
        const icon = el('span', 'ws-tab-icon');
        if (keyKind(key) === 'file') icon.innerHTML = fileIcon(info.path);
        else icon.append(stateDot(info.state));
        const text = el('span', 'ws-overview-text');
        const file = keyKind(key) === 'file';
        text.append(el('span', 'ws-overview-name', info.name), el('small', file ? 'is-path' : '', file ? info.path : [info.role || (key === 'main' ? 'Principal' : ''), STATE_LABEL[info.state]].filter(Boolean).join(' · ')));
        const where = el('span', 'ws-overview-where', windows.open.includes(key) ? 'A la vista' : 'Minimizada');
        button.append(icon, text, where);
        button.addEventListener('click', () => {
          closeOverview();
          openKey(key, { focus: true });
        });
        li.append(button);
        ul.append(li);
      }
      parts.push(ul);
    }
    overview.replaceChildren(...parts);
  }
  allButton.addEventListener('click', (event) => {
    event.stopPropagation();
    if (!overview.hidden) return closeOverview(true);
    renderOverview();
    overview.hidden = false;
    allButton.setAttribute('aria-expanded', 'true');
    overview.querySelector('button')?.focus();
  });
  overview.addEventListener('keydown', (event) => {
    const items = [...overview.querySelectorAll('button')];
    const at = items.indexOf(document.activeElement);
    if (event.key === 'Escape') {
      event.stopPropagation();
      closeOverview(true);
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      items[(at + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length]?.focus();
    }
  });
  document.addEventListener('click', (event) => {
    if (!overview.hidden && !overview.contains(event.target) && !allButton.contains(event.target)) closeOverview();
  });

  // Mosaico (hasta MAX_OPEN juntas) o una sola ventana que ocupa todo el centro.
  const modeSwitch = $('ws-mode');
  modeSwitch.addEventListener('click', () => {
    windows = setMode(windows, windows.mode === 'mosaico' ? 'sola' : 'mosaico');
    layoutWindows();
    render();
    saveUi();
    announce(windows.mode === 'mosaico' ? `Mosaico: hasta ${MAX_OPEN} ventanas juntas.` : 'Cada ventana ocupa todo el centro.');
  });

  // ---------------------------------------------------------------- carta: agentes
  // «2 activos · 1 esperando»: lo que está pasando ahora, sin contar los inactivos.
  function renderAgents() {
    const box = $('ws-agents');
    const rows = [];
    const row = (info, { badge = '', onClick, current = false, extra = '' }) => {
      const li = el('li', 'ws-agent');
      li.dataset.state = info.state;
      const button = el('button', 'ws-agent-main');
      button.type = 'button';
      if (current) button.setAttribute('aria-current', 'true');
      const text = el('span', 'ws-agent-text');
      const name = el('span', 'ws-agent-name');
      name.append(el('span', 'ws-agent-name-text', info.name));
      if (badge) name.append(el('span', 'ws-badge', badge));
      text.append(name, el('span', 'ws-agent-role', info.role || extra));
      const num = el('span', 'ws-agent-num', info.tokens ? formatTokens(info.tokens) : '');
      button.append(stateDot(info.state), text, num);
      button.setAttribute('aria-label', `${info.name}${badge ? `, ${badge.toLowerCase()}` : ''}, ${STATE_LABEL[info.state].toLowerCase()}${info.tokens ? `, ${formatTokens(info.tokens)} tokens` : ''}${info.role ? `. ${info.role}` : ''}`);
      button.title = info.role || '';
      button.addEventListener('click', onClick);
      li.append(button);
      return li;
    };
    const n = nexo?.info?.() ?? { state: 'idle' };
    rows.push(row({ name: 'Nexo', role: 'El chat de la carta derecha: tu vault', state: n.state, tokens: 0 }, { badge: 'Vault', onClick: () => nexo?.focus?.() }));
    rows.push(row(mainInfo(), { badge: 'Principal', onClick: () => openKey('main', { focus: true }), current: windows.active === 'main' }));
    const agents = agentList();
    for (const info of agents) rows.push(row(info, { onClick: () => openKey(info.key, { focus: true }), current: windows.active === info.key }));
    box.replaceChildren(...rows);
    $('ws-agents-count').textContent = String(agents.length + 1);
    $('ws-agents-aside').textContent = activity([mainInfo(), ...agents]);
  }

  // ---------------------------------------------------------------- carta: extras
  async function loadCatalog(force) {
    if (!ws) return;
    const path = ws.path;
    catalog = { loading: true, ...(catalog && !catalog.loading ? { previous: catalog } : {}) };
    renderExtras();
    try {
      const next = await invoke('workspace_extras', { path, force });
      if (ws?.path === path) catalog = next;
    } catch (error) {
      if (ws?.path === path) catalog = { error: String(error) };
    }
    if (ws?.path === path) {
      // Al arrancar, el orquestador tomó estos extras si ya corría: qué tiene puestos.
      if (!applied && mainState.running) applied = await invoke('workspace_running', { path }).catch(() => null);
      renderExtras();
      renderFoot();
    }
  }

  const GROUPS = [
    { key: 'plugins', title: 'Plugins', id: (p) => p.id, detail: (p) => [p.skills ? `${p.skills} ${p.skills === 1 ? 'skill' : 'skills'}` : '', p.servers ? `${p.servers} MCP` : '', p.synced ? 'de claude.ai' : '', p.description].filter(Boolean).join(' · ') },
    { key: 'mcp', title: 'MCP', id: (s) => s.name, detail: (s) => [s.kind, s.detail].filter(Boolean).join(' · ') },
    { key: 'connectors', title: 'Conexiones', id: (s) => s.name, detail: (s) => (s.status === 'needs-auth' ? 'Sin iniciar sesión en claude.ai' : 'Conexión de claude.ai'), name: (s) => s.name.replace(/^claude\.ai /, '') },
    { key: 'skills', title: 'Skills sin plugin', id: (s) => s.name, detail: (s) => [s.source === 'claude.ai' ? 'de claude.ai' : 'de ~/.claude/skills', s.description].filter(Boolean).join(' · ') },
  ];

  function renderExtras() {
    const box = $('ws-extras');
    const data = catalog?.loading ? catalog.previous : catalog;
    const on = GROUPS.reduce((n, g) => n + (extras[g.key]?.length ?? 0), 0);
    $('ws-extras-aside').textContent = on ? `${on} ${on === 1 ? 'encendido' : 'encendidos'}` : 'Todo apagado';
    $('ws-extras-refresh').classList.toggle('loading', Boolean(catalog?.loading));
    if (!data) {
      const note = el('p', 'ws-sec-note');
      note.textContent = catalog?.error ? `No se pudo leer tu perfil de Claude Code: ${catalog.error}` : 'Leyendo tu perfil de Claude Code…';
      note.classList.toggle('is-waiting', !catalog?.error);
      return box.replaceChildren(note);
    }
    const parts = [];
    const intro = el('p', 'ws-sec-note', 'Lo instalado en tu perfil global. Apagado, el orquestador y sus agentes no lo ven.');
    parts.push(intro);
    for (const group of GROUPS) {
      const items = data[group.key] ?? [];
      if (!items.length) continue;
      const wrap = el('div', 'ws-extras-group');
      const count = items.filter((i) => extras[group.key].includes(group.id(i))).length;
      const h = el('h4', 'ws-extras-h');
      h.append(el('span', '', group.title), el('span', 'ws-extras-n', count ? `${count} de ${items.length}` : String(items.length)));
      const ul = el('ul', 'ws-extras-list');
      for (const item of items) {
        const id = group.id(item);
        const checked = extras[group.key].includes(id);
        const li = el('li');
        const button = el('button', 'ws-extra');
        button.type = 'button';
        button.setAttribute('role', 'switch');
        button.setAttribute('aria-checked', String(checked));
        const text = el('span', 'ws-extra-text');
        const nameId = `ws-x-${group.key}-${id.replace(/[^a-z0-9]/gi, '-')}`;
        const name = el('span', 'ws-extra-name', group.name ? group.name(item) : item.name);
        name.id = nameId;
        const detail = el('span', 'ws-extra-detail', group.detail(item));
        detail.id = `${nameId}-d`;
        text.append(name, detail);
        button.setAttribute('aria-labelledby', nameId);
        button.setAttribute('aria-describedby', detail.id);
        button.title = group.detail(item);
        const sw = el('span', 'switch');
        sw.setAttribute('aria-hidden', 'true');
        sw.append(el('span'));
        button.append(text, sw);
        button.addEventListener('click', () => toggleExtra(group.key, id));
        li.append(button);
        ul.append(li);
      }
      wrap.append(h, ul);
      parts.push(wrap);
    }
    if (parts.length === 1) parts.push(el('p', 'ws-sec-note', 'No tienes plugins, servidores MCP, conexiones ni skills propias en tu perfil global.'));
    if (catalog?.error && data) parts.push(el('p', 'ws-sec-note', `No se pudo volver a leer: ${catalog.error}`));
    // Rehacer la lista no debe hacer perder el foco del interruptor que se acaba de pulsar.
    const focused = document.activeElement?.closest?.('.ws-extra')?.getAttribute('aria-labelledby');
    const scroller = box.closest('.ws-sec-body');
    const top = scroller?.scrollTop ?? 0;
    box.replaceChildren(...parts);
    if (scroller) scroller.scrollTop = top;
    if (focused) {
      const button = $(focused)?.closest('.ws-extra');
      button?.focus({ preventScroll: true });
      // El pie puede aparecer y quitarle alto a la sección: que el interruptor siga a la vista.
      requestAnimationFrame(() => button?.scrollIntoView({ block: 'nearest' }));
    }
  }

  async function toggleExtra(group, id) {
    if (!ws) return;
    const path = ws.path;
    const list = extras[group];
    extras = { ...extras, [group]: list.includes(id) ? list.filter((x) => x !== id) : [...list, id] };
    renderExtras();
    renderFoot();
    try {
      extras = { ...EMPTY_EXTRAS(), ...(await invoke('workspace_set_extras', { path, extras })) };
    } catch (error) {
      fail(error);
    }
  }

  // «Reiniciar espacio de Workspace»: aparece cuando lo encendido no es con lo que corre.
  function renderFoot() {
    const dirty = Boolean(ws && mainState.running && applied && !sameExtras(applied, extras));
    $('ws-foot').hidden = !dirty;
  }

  $('ws-restart').addEventListener('click', async () => {
    if (!ws) return;
    const path = ws.path;
    const busy = mainState.busy || agentList().some((a) => a.state === 'active');
    if (busy) {
      const ok = await ask('El orquestador o alguno de sus agentes está trabajando. Si reinicias ahora, se corta lo que estén haciendo.', {
        title: 'Reiniciar espacio de Workspace',
        kind: 'warning',
        okLabel: 'Reiniciar',
        cancelLabel: 'Cancelar',
      });
      if (!ok) return;
    }
    const button = $('ws-restart');
    button.disabled = true;
    button.classList.add('working');
    try {
      await invoke('workspace_stop', { path });
      const changed = stream.exit();
      if (changed.length) syncAgents(changed);
      await wins.get('main')?.chat.restart();
      const on = GROUPS.reduce((n, g) => n + extras[g.key].length, 0);
      announce(`Workspace reiniciado con ${on ? `${on} ${on === 1 ? 'extra' : 'extras'}` : 'los extras apagados'}.`);
    } catch (error) {
      fail(error);
    } finally {
      button.disabled = false;
      button.classList.remove('working');
      renderFoot();
      // El botón desaparece con el pie: el foco va a la cabecera de los extras.
      if ($('ws-foot').hidden) $('ws-sec-extras-toggle').focus();
    }
  });
  $('ws-extras-refresh').addEventListener('click', () => loadCatalog(true));

  // ---------------------------------------------------------------- secciones plegables
  for (const [key, id] of [['files', 'ws-sec-files'], ['agents', 'ws-sec-agents'], ['extras', 'ws-sec-extras']]) {
    const toggle = $(`${id}-toggle`);
    toggle.addEventListener('click', () => {
      sections = { ...sections, [key]: !sections[key] };
      ui.sections = sections;
      saveUi();
      renderSections();
    });
  }
  function renderSections() {
    for (const [key, id] of [['files', 'ws-sec-files'], ['agents', 'ws-sec-agents'], ['extras', 'ws-sec-extras']]) {
      const open = sections[key] !== false;
      $(id).classList.toggle('is-folded', !open);
      $(`${id}-toggle`).setAttribute('aria-expanded', String(open));
    }
  }

  // ---------------------------------------------------------------- pintar
  let renderFrame = 0;
  function scheduleRender() {
    if (!renderFrame) renderFrame = requestAnimationFrame(render);
  }

  function render() {
    renderFrame = 0;
    const has = Boolean(ws);
    $('ws-title').textContent = has ? ws.name : 'Workspace';
    $('ws-title').title = has ? ws.path : '';
    $('ws-path').textContent = has ? ws.path : '';
    $('ws-none').hidden = has || loading;
    $('ws-sections').hidden = !has && !loading;
    $('ws-bar').hidden = !has;
    renderSections();
    if (has) {
      renderAgents();
      renderTabs();
      for (const win of wins.values()) if (win.kind !== 'file') paintWindowHead(win);
      if (!overview.hidden) renderOverview();
      modeSwitch.setAttribute('aria-checked', String(windows.mode === 'mosaico'));
      if (!catalog) renderExtras();
    }
    renderFoot();
    $('ws-empty').hidden = !has || windows.open.length > 0;
    onChange(ws, { agents: has ? agentList() : [], skills, defined, main: has ? mainInfo() : null });
  }

  $('ws-folder').addEventListener('click', (event) => openDialog(event.currentTarget));
  $('ws-none-open').addEventListener('click', (event) => openDialog(event.currentTarget));
  $('ws-back').addEventListener('click', () => onMode('nexo'));
  $('ws-empty-files').addEventListener('click', () => {
    sections = { ...sections, files: true };
    renderSections();
    tree.querySelector('[tabindex="0"]')?.focus();
  });

  // ---------------------------------------------------------------- arranque
  async function init() {
    await refreshList();
    if (list.current?.exists) await openWorkspace(list.current.path);
    else render();
  }

  return {
    init,
    openDialog,
    openWorkspace,
    openAgent: (task) => openKey(task ? `a:${task}` : 'main', { focus: true }),
    // El centro se ve (la vista Agentes en modo workspace): recolocar lo que haga falta.
    setVisible(on) {
      visible = on;
      center.hidden = !on;
      if (on) {
        layoutWindows();
        render();
      } else closeOverview();
    },
    nexoChanged: () => scheduleRender(),
    get current() {
      return ws;
    },
    get list() {
      return list;
    },
    summary: () => (ws ? { agents: agentList(), skills, defined, main: mainInfo() } : null),
  };
}
