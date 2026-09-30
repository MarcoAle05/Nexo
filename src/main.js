import '@fontsource-variable/geist';
import '@fontsource/geist-mono/400.css';
import '@fontsource/geist-mono/500.css';
import '@fontsource-variable/unbounded';
import './styles.css';

import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open } from '@tauri-apps/plugin-dialog';

import { createClaudeCode } from './claude-code.js';
import { createGraph } from './graph.js';

const $ = (id) => document.getElementById(id);
const desktop = isTauri();

// ---------------------------------------------------------------- controles genéricos
document.querySelectorAll('[data-segmented]').forEach((group) => {
  group.addEventListener('click', (event) => {
    const button = event.target.closest('button');
    if (!button || !group.contains(button)) return;
    group.querySelectorAll('button').forEach((b) => b.setAttribute('aria-pressed', String(b === button)));
  });
});

document.querySelectorAll('[data-switch]').forEach((toggle) => {
  toggle.addEventListener('click', () => {
    toggle.setAttribute('aria-checked', String(toggle.getAttribute('aria-checked') !== 'true'));
  });
});

// ---------------------------------------------------------------- terminal
const output = $('output');

function printLine(text, kind) {
  const line = document.createElement('div');
  line.className = kind ? `line ${kind}` : 'line';
  if (!kind) {
    const caret = document.createElement('span');
    caret.className = 'caret';
    caret.textContent = '❯';
    line.append(caret);
  }
  const body = document.createElement('span');
  body.textContent = text;
  line.append(body);
  output.append(line);
  output.scrollTop = output.scrollHeight;
}

const fail = (error) => printLine(`error: ${error}`, 'error');

// ---------------------------------------------------------------- Nexo (Claude)
let mode = 'api';
let busy = false;

const HELP = `Escribe cualquier cosa para hablar con Nexo sobre tu wiki, o usa un comando
(el prefijo "nexo" es opcional):
  compile             integra en wiki/ las fuentes marcadas que sean nuevas
  ask <pregunta>      responde con tu wiki y lo guarda en output/query-results.md
  convertir           pasa a Markdown los PDF pendientes (MarkItDown, sin IA)
  nueva               empieza una conversación nueva con Nexo
  clave <sk-ant-…>    guarda tu clave de la API de Anthropic
  limpiar             vacía la terminal
La pestaña Claude Code abre Claude Code dentro de tu vault.`;
const COMMANDS = new Set(['ayuda', 'help', 'limpiar', 'clear', 'nueva', 'clave', 'key', 'compile', 'compilar', 'ask', 'pregunta', 'convertir', 'convert']);

function setBusy(value) {
  busy = value;
  $('prompt').classList.toggle('busy', value);
  $('prompt-input').disabled = value;
  if (!value) $('prompt-input').focus();
}

// Bloque de respuesta de Nexo que se va llenando con el streaming.
function startReply() {
  const reply = document.createElement('div');
  reply.className = 'reply pending';
  const name = document.createElement('span');
  name.className = 'reply-name';
  name.textContent = 'Nexo';
  const body = document.createElement('div');
  body.className = 'reply-body';
  reply.append(name, body);
  output.append(reply);
  output.scrollTop = output.scrollHeight;
  return {
    push(text) {
      reply.classList.remove('pending');
      reply.classList.add('streaming');
      body.textContent += text;
      output.scrollTop = output.scrollHeight;
    },
    end() {
      reply.classList.remove('pending', 'streaming');
      if (!body.textContent) reply.remove();
    },
  };
}

function nexoChannel(reply) {
  const channel = new Channel();
  channel.onmessage = ({ kind, text }) => {
    if (kind === 'text') reply?.push(text);
    else printLine(text, kind);
  };
  return channel;
}

async function runNexo(command, args, withReply) {
  if (!desktop) return printLine('Nexo solo funciona en la app de escritorio.', 'error');
  if (!vault) return printLine('Primero elige un vault.', 'error');
  setBusy(true);
  const reply = withReply ? startReply() : null;
  try {
    return await invoke(command, { ...args, channel: nexoChannel(reply) });
  } catch (error) {
    fail(error);
  } finally {
    reply?.end();
    setBusy(false);
  }
}

async function runCommand(text) {
  const [head, ...rest] = text.replace(/^nexo\s+/i, '').split(/\s+/);
  const arg = rest.join(' ').trim();
  switch (head.toLowerCase()) {
    case 'ayuda':
    case 'help':
      return printLine(HELP, 'help');
    case 'limpiar':
    case 'clear':
      return output.replaceChildren();
    case 'nueva':
      await invoke('chat_reset');
      return printLine('conversación nueva con Nexo', 'info');
    case 'clave':
    case 'key':
      if (!arg) return printLine('Uso: clave sk-ant-…', 'error');
      try {
        await invoke('set_api_key', { key: arg });
        printLine('clave guardada en la configuración de nexo', 'info');
      } catch (error) {
        fail(error);
      }
      return;
    case 'compile':
    case 'compilar': {
      const selected = sources.filter((s) => !excluded.has(s.path)).map((s) => s.path);
      if (!selected.length) return printLine('No hay fuentes marcadas en contexto.', 'error');
      const count = await runNexo('compile', { sources: selected }, false);
      if (count) {
        printLine(`${count} ${count === 1 ? 'fuente integrada' : 'fuentes integradas'} en wiki/`, 'info');
        await loadGraph();
      }
      return;
    }
    case 'convertir':
    case 'convert':
      if (!vault) return printLine('Primero elige un vault.', 'error');
      if (!sources.some((s) => s.convertible && !s.converted)) return printLine('No hay nada pendiente de convertir.', 'info');
      return convertSources([]);
    case 'ask':
    case 'pregunta': {
      if (!arg) return printLine('Uso: ask ¿tu pregunta?', 'error');
      const saved = await runNexo('ask', { question: arg }, true);
      if (saved) printLine(`guardado en ${saved}`, 'info');
      return;
    }
    default:
      printLine(`Comando desconocido: ${head}. Escribe ayuda.`, 'error');
  }
}

$('prompt').addEventListener('submit', async (event) => {
  event.preventDefault();
  const input = $('prompt-input');
  const text = input.value.trim();
  if (!text || busy) return;
  input.value = '';
  // Nunca se muestra la clave en pantalla.
  printLine(text.replace(/(sk-ant-[\w-]{4})[\w-]+/g, '$1…'));
  const command = text.replace(/^\//, '');
  const head = command.replace(/^nexo\s+/i, '').split(/\s+/)[0].toLowerCase();
  if (COMMANDS.has(head)) await runCommand(command);
  else await runNexo('chat_send', { message: text }, true);
});

// Pestañas del panel derecho: API (Nexo) y Claude Code (terminal real en el vault).
const claudeCode = createClaudeCode({
  container: $('cc-term'),
  exitBox: $('cc-exit'),
  exitText: $('cc-exit-text'),
  onError: fail,
});
$('cc-restart').addEventListener('click', () => claudeCode.restart());

function setMode(next) {
  mode = next;
  const cc = mode === 'claude-code';
  $('output').hidden = cc;
  $('prompt').hidden = cc;
  $('cc-view').hidden = !cc;
  $('terminal-title').textContent = cc ? 'Claude Code' : 'Nexo';
  if (!cc) return $('prompt-input').focus();
  if (!desktop) return printLine('Claude Code solo funciona en la app de escritorio.', 'error');
  if (!vault) {
    $('cc-exit-text').textContent = 'Primero elige un vault: Claude Code se abrirá dentro de él.';
    $('cc-exit').hidden = false;
    return;
  }
  claudeCode.show();
}

document.querySelectorAll('[data-mode]').forEach((button) => {
  button.addEventListener('click', () => setMode(button.dataset.mode));
});

// ---------------------------------------------------------------- vault
// Carpeta de Obsidian con raw/ → wiki/ → output/.
let vault = null;
let detected = [];

function showVault() {
  $('vault-name').textContent = vault ? vault.name : 'Sin vault';
  $('vault-path').textContent = vault ? vault.path : 'sin vault';
  $('vault-path').title = vault ? vault.path : '';
  $('vault-pick').hidden = Boolean(vault);
  renderDetected();
}

async function useVault(path) {
  try {
    vault = await invoke('set_vault', { path });
    excluded.clear();
    showVault();
    printLine(`vault → ${vault.path}`, 'system');
    printLine(
      vault.created.length ? `creado: ${vault.created.join(', ')}` : 'estructura raw/ · wiki/ · output/ lista',
      'system',
    );
    await refresh();
    // Claude Code trabaja dentro del vault: al cambiarlo se cierra la sesión anterior.
    await claudeCode.stop();
    if (mode === 'claude-code') setMode('claude-code');
  } catch (error) {
    fail(error);
  }
}

async function pickFolder() {
  if (!desktop) return printLine('Esto solo funciona en la app de escritorio (npm run tauri dev).', 'error');
  const path = await open({ directory: true, multiple: false, title: 'Elige tu vault de Obsidian' });
  if (path) await useVault(path);
}

function vaultOption(v) {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'vault-option';
  if (vault && vault.path === v.path) button.setAttribute('aria-current', 'true');
  const name = document.createElement('span');
  name.textContent = v.name;
  const path = document.createElement('small');
  path.textContent = v.path;
  button.append(name, path);
  button.addEventListener('click', () => {
    closeMenu();
    if (!vault || vault.path !== v.path) useVault(v.path);
  });
  return button;
}

// Sin vault elegido: los vaults que Obsidian conoce aparecen en el panel de fuentes.
function renderDetected() {
  const box = $('detected-vaults');
  box.hidden = Boolean(vault) || !detected.length;
  box.replaceChildren(...(vault ? [] : detected.map(vaultOption)));
}

const menu = $('vault-menu');
const menuButton = $('vault-button');

function closeMenu() {
  menu.hidden = true;
  menuButton.setAttribute('aria-expanded', 'false');
}

menuButton.addEventListener('click', async (event) => {
  event.stopPropagation();
  if (!menu.hidden) return closeMenu();
  if (desktop) detected = await invoke('detect_vaults').catch(() => detected);
  const items = [];
  if (detected.length) {
    const title = document.createElement('li');
    title.className = 'vault-menu-title';
    title.textContent = 'Vaults de Obsidian';
    items.push(title, ...detected.map((v) => {
      const li = document.createElement('li');
      li.append(vaultOption(v));
      return li;
    }));
    const sep = document.createElement('li');
    sep.className = 'vault-sep';
    sep.setAttribute('aria-hidden', 'true');
    items.push(sep);
  }
  const other = document.createElement('li');
  const otherButton = document.createElement('button');
  otherButton.type = 'button';
  otherButton.className = 'vault-option';
  otherButton.textContent = 'Elegir otra carpeta…';
  otherButton.addEventListener('click', () => {
    closeMenu();
    pickFolder();
  });
  other.append(otherButton);
  items.push(other);
  menu.replaceChildren(...items);
  menu.hidden = false;
  menuButton.setAttribute('aria-expanded', 'true');
});

document.addEventListener('click', (event) => {
  if (!menu.hidden && !menu.contains(event.target)) closeMenu();
});
document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') {
    closeMenu();
    connMenu.hidden = true;
    connButton.setAttribute('aria-expanded', 'false');
  }
});
$('vault-pick').addEventListener('click', pickFolder);

// ---------------------------------------------------------------- fuentes (raw/)
const ICONS = {
  pdf: '<path d="M5 2.5h6.5l3.5 3.5v11.5H5z"/><path d="M11.5 2.5V6H15"/>',
  web: '<circle cx="10" cy="10" r="7.5"/><path d="M2.5 10h15M10 2.5c2.4 2.4 2.4 12.6 0 15M10 2.5c-2.4 2.4-2.4 12.6 0 15"/>',
  nota: '<path d="M8 3 6.5 17M13.5 3 12 17M3.5 7.5h13M3 12.5h13"/>',
  media: '<path d="M3 10h1.5M6.5 6.5v7M10 3.5v13M13.5 7v6M17 10h-1.5"/>',
  otro: '<rect x="4" y="4" width="12" height="12" rx="2.5"/>',
};
const TRASH = '<path d="M4 6h12M8 6V4h4v2M6 6l1 10h6l1-10"/>';
const KIND_LABEL = { pdf: 'PDF', web: 'Web', nota: 'Nota', media: 'Media', otro: 'Archivo' };
const svg = (paths, size = 18) =>
  `<svg width="${size}" height="${size}" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;

let sources = [];
let filter = 'all';
const excluded = new Set(); // fuentes desmarcadas: todas entran en contexto salvo estas

function formatSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}

function renderContext() {
  const active = sources.filter((s) => !excluded.has(s.path)).length;
  $('context-count').textContent = `${active} de ${sources.length}`;
  $('context-meter').style.width = sources.length ? `${(active / sources.length) * 100}%` : '0%';
}

function sourceRow(source, index) {
  const li = document.createElement('li');
  const row = document.createElement('div');
  row.className = 'source';
  row.style.animationDelay = `${Math.min(index, 12) * 40}ms`;

  const icon = document.createElement('span');
  icon.className = 'source-icon';
  icon.innerHTML = svg(ICONS[source.kind]);

  const openButton = document.createElement('button');
  openButton.type = 'button';
  openButton.className = 'source-open';
  openButton.title = `Abrir raw/${source.path}`;
  const name = document.createElement('span');
  name.className = 'source-name';
  name.textContent = source.name;
  const meta = document.createElement('span');
  meta.className = 'source-meta';
  meta.textContent = `${KIND_LABEL[source.kind]} · ${formatSize(source.size)}`;
  openButton.append(name, meta);
  openButton.addEventListener('click', () => invoke('open_source', { path: source.path }).catch(fail));

  // Estado de la conversión a Markdown (MarkItDown, sin IA).
  const text = document.createElement('span');
  text.className = 'source-text';
  text.append(openButton);
  if (source.convertible) {
    const md = document.createElement('button');
    md.type = 'button';
    md.className = 'md-badge';
    if (source.converted) {
      md.textContent = 'MD ✓';
      md.title = 'Abrir la versión Markdown';
      md.addEventListener('click', () =>
        invoke('open_source', { path: `markdown/${source.path.replace(/\.[^./]+$/, '')}.md` }).catch(fail),
      );
    } else {
      md.textContent = converting ? 'MD…' : 'MD pendiente';
      md.disabled = converting;
      md.title = 'Convertir a Markdown con MarkItDown';
      md.addEventListener('click', () => convertSources([source.path]));
    }
    text.append(md);
  }

  const remove = document.createElement('button');
  remove.type = 'button';
  remove.className = 'icon-btn source-remove';
  remove.setAttribute('aria-label', `Quitar ${source.name}`);
  remove.title = 'Mover a la papelera';
  remove.innerHTML = svg(TRASH, 16);
  remove.addEventListener('click', () => removeSource(source));

  const check = document.createElement('input');
  check.type = 'checkbox';
  check.checked = !excluded.has(source.path);
  check.setAttribute('aria-label', `Usar ${source.name} en el contexto`);
  check.addEventListener('change', () => {
    if (check.checked) excluded.delete(source.path);
    else excluded.add(source.path);
    renderContext();
  });

  row.append(icon, text, remove, check);
  li.append(row);
  return li;
}

function renderSources() {
  $('source-count').textContent = String(sources.length);
  document.querySelectorAll('[data-count]').forEach((el) => {
    const kind = el.dataset.count;
    el.textContent = String(kind === 'all' ? sources.length : sources.filter((s) => s.kind === kind).length);
  });

  const visible = filter === 'all' ? sources : sources.filter((s) => s.kind === filter);
  $('source-list').replaceChildren(...visible.map(sourceRow));
  $('source-list').hidden = visible.length === 0;
  $('sources-empty').hidden = visible.length > 0;

  const [title, text] = !vault
    ? ['Elige tu vault', detected.length
        ? 'Encontramos estos vaults de Obsidian. Elige uno o busca otra carpeta.'
        : 'Selecciona la carpeta de Obsidian donde nexo guardará tus fuentes.']
    : sources.length
      ? ['Nada de este tipo', 'Prueba con otro filtro.']
      : ['Aún no hay fuentes', 'Añade PDF, enlaces, audio o notas, o arrástralos aquí: se guardarán en raw/.'];
  $('empty-title').textContent = title;
  $('empty-text').textContent = text;
  renderContext();
}

async function loadSources() {
  sources = vault ? await invoke('list_sources').catch((e) => (fail(e), [])) : [];
  for (const path of excluded) if (!sources.some((s) => s.path === path)) excluded.delete(path);
  renderSources();
  // Los PDF (y Office/EPUB) sin convertir se pasan a Markdown en segundo plano: es local y gratis.
  // Cada archivo se intenta una vez por sesión, para no repetir los que no tienen texto (PDF escaneados).
  const fresh = sources.filter((s) => s.convertible && !s.converted && !autoTried.has(s.path));
  if (fresh.length && !converting) {
    fresh.forEach((s) => autoTried.add(s.path));
    convertSources(fresh.map((s) => s.path));
  }
}
const autoTried = new Set();

// Conversión local con el servidor MCP de MarkItDown; no usa Claude ni la API.
let converting = false;
async function convertSources(paths) {
  if (converting || !desktop || !vault) return;
  converting = true;
  renderSources();
  const channel = new Channel();
  channel.onmessage = ({ kind, text }) => printLine(text, kind === 'error' ? 'error' : 'step');
  try {
    const count = await invoke('convert_sources', { paths, channel });
    if (count) printLine(`${count} ${count === 1 ? 'fuente convertida' : 'fuentes convertidas'} a Markdown en raw/markdown/`, 'info');
  } catch (error) {
    fail(error);
  } finally {
    converting = false;
  }
  sources = await invoke('list_sources').catch(() => sources);
  renderSources();
}

const CONVERTIBLE = /\.(pdf|docx|pptx|xlsx|xls|epub)$/i;

async function addSources(paths) {
  if (!vault) return printLine('Primero elige un vault.', 'error');
  // Sin MarkItDown activo no se aceptan PDF (ni Office/EPUB): deben convertirse al subirlos.
  if (!markitdownReady) {
    const blocked = paths.filter((p) => CONVERTIBLE.test(p));
    if (blocked.length) {
      printLine(
        `${blocked.length} PDF/documentos no se añadieron: MarkItDown (MCP) no está activo. Revisa Conexiones.`,
        'error',
      );
      paths = paths.filter((p) => !CONVERTIBLE.test(p));
    }
  }
  if (!paths.length) return;
  try {
    const { added, skipped, rejected } = await invoke('add_sources', { paths });
    added.forEach((s) => printLine(`+ raw/${s.path}`, 'system'));
    if (skipped.length) printLine(`omitidos (carpetas o ya en raw/): ${skipped.length}`, 'system');
    if (rejected.length) printLine(`rechazados sin MarkItDown: ${rejected.join(', ')}`, 'error');
    // Conversión inmediata a Markdown de lo que se acaba de subir.
    const toConvert = added.filter((s) => s.convertible).map((s) => s.path);
    toConvert.forEach((p) => autoTried.add(p));
    sources = await invoke('list_sources').catch(() => sources);
    renderSources();
    if (toConvert.length) await convertSources(toConvert);
  } catch (error) {
    fail(error);
  }
}

// ---------------------------------------------------------------- conexiones (MCP, API, CLI)
let markitdownReady = false;
const STATE_LABEL = { ok: 'Activo', missing: 'No disponible', error: 'Error' };
const connMenu = $('conn-menu');
const connButton = $('conn-button');

function renderConnections(list) {
  const dot = (state) => {
    const d = document.createElement('span');
    d.className = 'conn-dot';
    d.dataset.state = state === 'ok' ? 'ok' : state === 'checking' ? 'checking' : 'warn';
    d.setAttribute('aria-hidden', 'true');
    return d;
  };
  $('conn-list').replaceChildren(
    ...list.map((c) => {
      const li = document.createElement('li');
      li.className = 'conn-item';
      const name = document.createElement('span');
      name.className = 'conn-name';
      name.textContent = c.name;
      const kind = document.createElement('span');
      kind.className = 'conn-kind';
      kind.textContent = c.kind;
      name.append(kind);
      const state = document.createElement('span');
      state.className = 'conn-state';
      state.textContent = STATE_LABEL[c.state] ?? 'Comprobando…';
      const detail = document.createElement('span');
      detail.className = 'conn-detail';
      detail.textContent = c.detail;
      li.append(dot(c.state), name, state, detail);
      return li;
    }),
  );
  const states = list.map((c) => c.state);
  $('conn-dot').dataset.state = states.includes('checking')
    ? 'checking'
    : states.every((s) => s === 'ok')
      ? 'ok'
      : 'warn';
  connButton.title = list.map((c) => `${c.name}: ${STATE_LABEL[c.state] ?? '…'}`).join(' · ');
}

async function refreshConnections() {
  if (!desktop) return;
  $('conn-dot').dataset.state = 'checking';
  try {
    const list = await invoke('connections_status');
    markitdownReady = list.some((c) => c.id === 'markitdown' && c.state === 'ok');
    renderConnections(list);
    renderSources();
  } catch (error) {
    fail(error);
  }
}

connButton.addEventListener('click', (event) => {
  event.stopPropagation();
  const opening = connMenu.hidden;
  connMenu.hidden = !opening;
  connButton.setAttribute('aria-expanded', String(opening));
  if (opening) refreshConnections();
});
$('conn-refresh').addEventListener('click', refreshConnections);
document.addEventListener('click', (event) => {
  if (!connMenu.hidden && !connMenu.contains(event.target)) {
    connMenu.hidden = true;
    connButton.setAttribute('aria-expanded', 'false');
  }
});

async function removeSource(source) {
  try {
    await invoke('remove_source', { path: source.path });
    printLine(`− raw/${source.path} → papelera`, 'system');
    excluded.delete(source.path);
    await loadSources();
  } catch (error) {
    fail(error);
  }
}

document.querySelectorAll('[data-kind]').forEach((chip) => {
  chip.addEventListener('click', () => {
    filter = chip.dataset.kind;
    renderSources();
  });
});

$('add-source').addEventListener('click', async () => {
  if (!desktop) return printLine('Añadir fuentes solo funciona en la app de escritorio.', 'error');
  if (!vault) return pickFolder();
  const picked = await open({ multiple: true, title: 'Añadir fuentes a raw/' });
  if (picked) addSources(Array.isArray(picked) ? picked : [picked]);
});

// ---------------------------------------------------------------- grafo (wiki/)
const stage = $('stage');
const graph = createGraph($('graph-canvas'), {
  // Hueco central entre los paneles, en coordenadas del lienzo.
  getViewport() {
    const r = stage.getBoundingClientRect();
    return { cx: r.left + r.width / 2, cy: r.top + r.height / 2, w: r.width, h: Math.max(r.height - 140, 200) };
  },
  onSelect(node) {
    $('focus-pill').hidden = !node;
    if (!node) return;
    $('focus-name').textContent = node.label;
    $('focus-degree').textContent = `· ${node.degree} ${node.degree === 1 ? 'conexión' : 'conexiones'}`;
    $('focus-open').hidden = node.kind === 'missing';
    focused = node;
  },
  onOpen: (node) => openNote(node),
});
let focused = null;
let depth = 2;

function openNote(node) {
  invoke('open_note', { id: node.id }).catch(fail);
}

async function loadGraph() {
  const data = vault ? await invoke('read_graph').catch((e) => (fail(e), null)) : null;
  const empty = !data || !data.nodes.length;
  $('stage-empty').hidden = !empty;
  $('stage-empty-text').textContent = !vault
    ? 'Elige un vault para ver su grafo'
    : 'Tu wiki está vacía: aquí aparecerán las notas de wiki/ y sus [[enlaces]]';
  $('graph-toolbar').querySelectorAll('button').forEach((b) => (b.disabled = empty));
  graph.setData(data ?? { nodes: [], edges: [] });
}

document.querySelectorAll('[data-scope]').forEach((b) =>
  b.addEventListener('click', () => graph.setScope(b.dataset.scope)),
);
$('depth-down').addEventListener('click', () => setDepth(depth - 1));
$('depth-up').addEventListener('click', () => setDepth(depth + 1));
function setDepth(value) {
  depth = Math.min(4, Math.max(1, value));
  $('depth-value').textContent = String(depth);
  graph.setDepth(depth);
}
$('labels-switch').addEventListener('click', () =>
  graph.setLabels($('labels-switch').getAttribute('aria-checked') === 'true'),
);
$('graph-fit').addEventListener('click', () => graph.fit());
$('focus-close').addEventListener('click', () => graph.clearSelection());
$('focus-open').addEventListener('click', () => focused && openNote(focused));

// ---------------------------------------------------------------- arranque
async function refresh() {
  await Promise.all([loadSources(), loadGraph()]);
}

if (desktop) {
  getCurrentWebview().onDragDropEvent(({ payload }) => {
    const panel = $('sources-panel');
    if (payload.type === 'enter' || payload.type === 'over') panel.classList.toggle('dragging', Boolean(vault));
    else if (payload.type === 'leave') panel.classList.remove('dragging');
    else if (payload.type === 'drop') {
      panel.classList.remove('dragging');
      addSources(payload.paths);
    }
  });

  // Cambios hechos fuera (Obsidian, explorador) se ven al volver a la ventana.
  window.addEventListener('focus', () => vault && refresh());

  (async () => {
    try {
      vault = await invoke('get_vault');
      refreshConnections();
      detected = await invoke('detect_vaults');
      const key = await invoke('key_status');
      printLine(
        `Nexo · ${key.model} · ${
          !key.configured
            ? 'falta la clave: escribe clave sk-ant-…'
            : key.source === 'env'
              ? 'clave desde ANTHROPIC_API_KEY'
              : 'clave guardada'
        }`,
        'info',
      );
      // Primer arranque con un único vault de Obsidian: se usa directamente.
      if (!vault && detected.length === 1) {
        printLine(`vault de Obsidian detectado: ${detected[0].name}`, 'system');
        return useVault(detected[0].path);
      }
      showVault();
      if (vault) printLine(`vault → ${vault.path}`, 'system');
      else if (detected.length) printLine(`${detected.length} vaults de Obsidian detectados`, 'system');
      await refresh();
    } catch (error) {
      fail(error);
    }
  })();
} else {
  renderSources();
  loadGraph();
}
