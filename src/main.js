import '@fontsource-variable/geist';
import '@fontsource/geist-mono/400.css';
import '@fontsource/geist-mono/500.css';
import '@fontsource/geist-mono/700.css';
import '@fontsource-variable/unbounded';
import './styles.css';

import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { ask, open } from '@tauri-apps/plugin-dialog';
import { openUrl } from '@tauri-apps/plugin-opener';

import { createClaudeCode } from './claude-code.js';
import { colorFor, createGraph } from './graph.js';
import { createNotes } from './notes.js';
import { renderMarkdown, resolveLink } from './note-view.js';

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

// ---------------------------------------------------------------- reloj
// Hora del sistema; cada actualización se programa al inicio del segundo siguiente
// para que no se desfase.
const clockDate = new Intl.DateTimeFormat('es', { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
function tickClock() {
  const now = new Date();
  const pad = (n) => String(n).padStart(2, '0');
  $('clock-hm').textContent = `${pad(now.getHours())}:${pad(now.getMinutes())}`;
  $('clock-sec').textContent = `:${pad(now.getSeconds())}`;
  $('clock-time').dateTime = now.toISOString();
  $('clock-date').textContent = clockDate.format(now);
  setTimeout(tickClock, 1000 - now.getMilliseconds() + 5);
}
tickClock();

// ---------------------------------------------------------------- cartas expandibles
// Cada carta puede expandirse hasta el borde de la otra; con las dos expandidas se
// reparten el ancho. Contraer una deja a la otra como esté. Se recuerda entre aperturas.
const expanded = { left: false, right: false };
// La carta derecha (Nexo / Claude Code) se puede minimizar a un riel estrecho.
let minimized = false;
try {
  minimized = localStorage.getItem('nexo.minimized') === 'true';
} catch {}
try {
  Object.assign(expanded, JSON.parse(localStorage.getItem('nexo.expanded') ?? '{}'));
} catch {}

function renderExpanded() {
  const layout = $('layout');
  // Minimizada, la terminal no puede estar expandida.
  if (minimized) expanded.right = false;
  layout.classList.toggle('expand-left', expanded.left);
  layout.classList.toggle('expand-right', expanded.right);
  layout.classList.toggle('expanded', expanded.left || expanded.right);
  layout.classList.toggle('min-right', minimized);
  $('term-rail').hidden = !minimized;
  document.querySelector('#terminal-panel > .panel-core').hidden = minimized;
  document.querySelectorAll('[data-expand]').forEach((b) => {
    const on = expanded[b.dataset.expand];
    const what = b.dataset.expand === 'right' ? 'la terminal' : b.closest('#notes-core') ? 'la carta de notas' : 'la carta de fuentes';
    b.setAttribute('aria-pressed', String(on));
    b.setAttribute('aria-label', `${on ? 'Contraer' : 'Expandir'} ${what}`);
    b.title = `${on ? 'Contraer' : 'Expandir'} ${what}`;
  });
  // Con el grafo plegado no se puede usar su barra ni el enfoque.
  $('stage').inert = expanded.left || expanded.right;
}

document.querySelectorAll('[data-expand]').forEach((b) =>
  b.addEventListener('click', () => {
    expanded[b.dataset.expand] = !expanded[b.dataset.expand];
    renderExpanded();
    try {
      localStorage.setItem('nexo.expanded', JSON.stringify(expanded));
    } catch {}
  }),
);
renderExpanded();

function setMinimized(value) {
  minimized = value;
  renderExpanded();
  try {
    localStorage.setItem('nexo.minimized', String(minimized));
    localStorage.setItem('nexo.expanded', JSON.stringify(expanded));
  } catch {}
  // xterm deja de dibujar mientras está oculto: al volver se reajusta y enfoca.
  if (!minimized && mode === 'claude-code' && vault && desktop) requestAnimationFrame(() => claudeCode.show());
}
$('term-minimize').addEventListener('click', () => setMinimized(true));
$('term-restore').addEventListener('click', () => setMinimized(false));
document.querySelectorAll('[data-rail-mode]').forEach((b) =>
  b.addEventListener('click', () => {
    setMinimized(false);
    document.querySelector(`[data-mode="${b.dataset.railMode}"]`)?.click();
  }),
);

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
  web <enlace>        lee la página (con Antigravity o Playwright, según el selector) y la guarda en raw/web/
  resumir             Antigravity resume las fuentes que aún no tienen resumen
  nueva               empieza una conversación nueva con Nexo
  clave <sk-ant-…>    guarda tu clave de la API de Anthropic
  limpiar             vacía la terminal
La pestaña Claude Code abre Claude Code dentro de tu vault.`;
const COMMANDS = new Set(['ayuda', 'help', 'limpiar', 'clear', 'nueva', 'clave', 'key', 'compile', 'compilar', 'ask', 'pregunta', 'convertir', 'convert', 'web', 'url', 'resumir']);

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
    case 'resumir': {
      if (!vault) return printLine('Primero elige un vault.', 'error');
      const pending = sources.filter((s) => !s.summarized && hasText(s));
      if (!pending.length) return printLine('Todas las fuentes con texto ya tienen resumen.', 'info');
      printLine(`${pending.length} fuentes en la cola de Antigravity`, 'info');
      pending.forEach((s) => summarizeSource(s.path));
      return;
    }
    case 'web':
    case 'url':
      if (!arg) return printLine('Uso: web https://…', 'error');
      await addWebSource(arg);
      return;
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
  showActivity(false);
  const cc = mode === 'claude-code';
  $('output').hidden = cc;
  $('prompt').hidden = cc;
  $('cc-view').hidden = !cc;
  $('terminal-title').textContent = cc ? 'Claude Code' : 'Nexo';
  $('rail-label').textContent = cc ? 'Claude Code' : 'Nexo';
  document.querySelectorAll('[data-rail-mode]').forEach((b) => b.classList.toggle('active', b.dataset.railMode === mode));
  $('cc-save').hidden = !cc;
  if (!cc) return $('prompt-input').focus();
  if (!desktop) return printLine('Claude Code solo funciona en la app de escritorio.', 'error');
  if (!vault) {
    $('cc-exit-text').textContent = 'Primero elige un vault: Claude Code se abrirá dentro de él.';
    $('cc-exit').hidden = false;
    return;
  }
  claudeCode.show();
}

// Guardar la conversación: Claude Code escribe sus conclusiones en wiki/conversaciones/ y la
// nota aparece como nodo del grafo. Se revisa el grafo hasta que llegue (o pasen 3 minutos).
$('cc-save').addEventListener('click', () => {
  if (activityOpen) showActivity(false);
  if (!claudeCode.say('Guarda la conversación en el grafo.')) {
    return printLine('Abre primero la sesión de Claude Code.', 'error');
  }
  const before = new Set(graphData.nodes.filter((n) => n.kind === 'conversation').map((n) => n.id));
  const started = Date.now();
  $('cc-save').classList.add('working');
  const timer = setInterval(async () => {
    await loadGraph();
    const fresh = graphData.nodes.find((n) => n.kind === 'conversation' && !before.has(n.id));
    const updated = Date.now() - started > 180_000;
    if (fresh || updated) {
      clearInterval(timer);
      $('cc-save').classList.remove('working');
      if (fresh) graph.focus(fresh.id);
    }
  }, 4000);
});

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
    notesStale = true;
    if (view === 'notas') activateNotes();
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
  openButton.title = 'Ver resumen de la fuente';
  const name = document.createElement('span');
  name.className = 'source-name';
  name.textContent = source.name;
  const meta = document.createElement('span');
  meta.className = 'source-meta';
  meta.textContent = `${KIND_LABEL[source.kind]} · ${formatSize(source.size)}${source.summarized ? ' · ✦ resumen' : ''}`;
  openButton.append(name, meta);
  openButton.addEventListener('click', () => openDetail(source.path));

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
  const inDetail = Boolean(detailPath || noteNode);
  $('source-list').hidden = inDetail || visible.length === 0;
  $('sources-empty').hidden = inDetail || visible.length > 0;
  document.querySelector('.chips').hidden = inDetail;
  $('source-detail').hidden = !detailPath;
  $('note-detail').hidden = !noteNode;

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
  if (detailPath && !sources.some((s) => s.path === detailPath)) {
    detailPath = null;
    detailView = null;
  }
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
    loadGraph(); // cada fuente nueva ya es un nodo (su ficha)
    if (toConvert.length) await convertSources(toConvert);
    // Antigravity resume lo que tiene texto; las tareas esperan en su cola.
    const addedPaths = new Set(added.map((s) => s.path));
    sources.filter((s) => addedPaths.has(s.path) && hasText(s)).forEach((s) => summarizeSource(s.path));
  } catch (error) {
    fail(error);
  }
}

const hasText = (s) => s.converted || s.kind === 'web' || s.kind === 'nota' || /\.(md|markdown|txt|html?)$/i.test(s.path);

// Resumen con Antigravity: escribe la ficha de la fuente (resumen, ideas clave y conceptos).
const summarizing = new Set();
async function summarizeSource(path) {
  if (!desktop || !vault) return;
  if (!agyReady) return printLine('Antigravity (agy) no está disponible para resumir. Revisa Conexiones.', 'error');
  if (summarizing.has(path)) return;
  summarizing.add(path);
  if (detailPath === path) renderDetail();
  try {
    const message = await invoke('summarize_source', { path });
    printLine(`✦ ${message}`, 'info');
  } catch (error) {
    fail(`resumen de ${path}: ${error}`);
  } finally {
    summarizing.delete(path);
    await Promise.all([loadSources(), loadGraph()]);
    if (detailPath === path) await refreshDetail();
  }
}

// ---------------------------------------------------------------- conexiones (MCP, API, CLI)
let markitdownReady = false;
let agyReady = false;
let playwrightReady = false;

// Cómo se leen los enlaces: Antigravity directamente, o Playwright (Chrome) + MarkItDown y
// Antigravity solo para resumir. La elección se recuerda en este equipo.
let readMode = (() => {
  try {
    return localStorage.getItem('nexo.readMode') === 'playwright' ? 'playwright' : 'agy';
  } catch {
    return 'agy';
  }
})();
function renderReadMode() {
  document.querySelectorAll('[data-read]').forEach((b) => {
    b.setAttribute('aria-pressed', String(b.dataset.read === readMode));
    if (b.dataset.read === 'playwright') b.disabled = !playwrightReady;
  });
}
document.querySelectorAll('[data-read]').forEach((b) =>
  b.addEventListener('click', () => {
    readMode = b.dataset.read;
    try {
      localStorage.setItem('nexo.readMode', readMode);
    } catch {}
    renderReadMode();
  }),
);
renderReadMode();
let fetchingWeb = 0;

// Páginas web como fuente: Antigravity (agy) abre la URL y devuelve su contenido en Markdown.
async function addWebSource(url) {
  if (!desktop) return printLine('Añadir páginas web solo funciona en la app de escritorio.', 'error');
  if (!vault) return printLine('Primero elige un vault.', 'error');
  if (!agyReady) return printLine('Antigravity (agy) no está disponible. Revisa Conexiones.', 'error');
  const playwright = readMode === 'playwright' && playwrightReady;
  if (readMode === 'playwright' && !playwrightReady) {
    printLine('Playwright no está disponible (revisa Conexiones): se usará Antigravity.', 'info');
  }
  fetchingWeb += 1;
  $('link-form').classList.add('busy');
  printLine(
    playwright
      ? `Playwright abre ${url}; luego Antigravity lo resume (míralo en Actividad Antigravity)`
      : `Antigravity en cola: ${url} (míralo en Actividad Antigravity)`,
    'info',
  );
  try {
    const path = await invoke('add_web_source', { url, playwright });
    printLine(`+ raw/${path} · con ficha y resumen`, 'system');
    await Promise.all([loadSources(), loadGraph()]);
    return path;
  } catch (error) {
    fail(error);
  } finally {
    fetchingWeb -= 1;
    $('link-form').classList.toggle('busy', fetchingWeb > 0);
  }
}

$('link-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  const input = $('link-input');
  const url = input.value.trim();
  if (!url) return input.focus();
  if (!/^https?:\/\/\S+\.\S+/i.test(url)) return printLine('Pega un enlace completo (https://…).', 'error');
  input.value = '';
  if (!(await addWebSource(url))) input.value = input.value || url;
});
const STATE_LABEL = { ok: 'Activo', missing: 'No disponible', error: 'Error' };
const connMenu = $('conn-menu');
const connButton = $('conn-button');

// Límites de uso de las suscripciones (Claude Code y Antigravity): una mini pestaña junto
// al nombre con el porcentaje más alto, que despliega cada límite con su barra.
const USAGE_OF = { 'claude-code': 'claude', antigravity: 'agy' };
const usage = { claude: null, agy: null }; // { loading } | { data } | { error }
const usageOpen = new Set();
let lastConnections = [];

function resetText(iso) {
  if (!iso) return 'sin consumo pendiente';
  const minutes = Math.max(0, Math.round((new Date(iso) - Date.now()) / 60000));
  if (minutes < 60) return `se renueva en ${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `se renueva en ${hours} h ${minutes % 60} min`;
  return `se renueva en ${Math.floor(hours / 24)} d ${hours % 24} h`;
}

const pct = (n) => `${n < 10 && n % 1 ? n.toFixed(1) : Math.round(n)} %`;

function usageBar(used, className) {
  const bar = document.createElement('span');
  bar.className = className;
  const fill = document.createElement('span');
  fill.style.width = `${Math.min(100, Math.max(used > 0 ? 2 : 0, used))}%`;
  bar.dataset.level = used >= 90 ? 'high' : used >= 70 ? 'mid' : 'low';
  bar.append(fill);
  return bar;
}

function addUsage(li, name, service) {
  const info = usage[service];
  const tab = document.createElement('button');
  tab.type = 'button';
  tab.className = 'usage-tab';
  const open = usageOpen.has(service);
  tab.setAttribute('aria-expanded', String(open));
  const limits = info?.data?.groups.flatMap((g) => g.limits) ?? [];
  const top = limits.reduce((max, l) => Math.max(max, l.used), 0);
  if (info?.data) {
    tab.append(usageBar(top, 'usage-mini'), Object.assign(document.createElement('span'), { textContent: pct(top) }));
    tab.title = 'Límites de uso: el más consumido. Pulsa para ver el detalle.';
  } else {
    tab.textContent = info?.error ? 'Uso ?' : 'Uso…';
    tab.title = info?.error ?? 'Consultando los límites de uso…';
    tab.classList.toggle('loading', !info?.error);
  }
  tab.addEventListener('click', (event) => {
    event.stopPropagation();
    if (usageOpen.has(service)) usageOpen.delete(service);
    else usageOpen.add(service);
    renderConnections(lastConnections);
  });
  name.append(tab);
  if (!open) return;

  const panel = document.createElement('div');
  panel.className = 'usage-panel';
  if (info?.error) {
    panel.append(Object.assign(document.createElement('p'), { className: 'usage-note', textContent: info.error }));
  } else if (!info?.data) {
    panel.append(Object.assign(document.createElement('p'), { className: 'usage-note', textContent: 'Consultando…' }));
  } else {
    for (const group of info.data.groups) {
      const head = document.createElement('div');
      head.className = 'usage-group';
      head.textContent = group.name;
      if (group.models) {
        const models = document.createElement('small');
        models.textContent = group.models;
        head.append(models);
      }
      panel.append(head);
      for (const limit of group.limits) {
        const row = document.createElement('div');
        row.className = 'usage-row';
        const label = document.createElement('span');
        label.className = 'usage-label';
        label.textContent = limit.label;
        const value = document.createElement('span');
        value.className = 'usage-value';
        value.textContent = pct(limit.used);
        const reset = document.createElement('span');
        reset.className = 'usage-reset';
        reset.textContent = resetText(limit.resets_at);
        row.append(label, value, usageBar(limit.used, 'usage-bar'), reset);
        panel.append(row);
      }
    }
  }
  li.append(panel);
}

async function loadUsage(service, force) {
  usage[service] = { ...(usage[service]?.data ? { data: usage[service].data } : {}), loading: true };
  try {
    usage[service] = { data: await invoke(service === 'claude' ? 'usage_claude' : 'usage_agy', { force }) };
  } catch (error) {
    usage[service] = { error: String(error) };
  }
  if (!connMenu.hidden) renderConnections(lastConnections);
}

function renderConnections(list) {
  lastConnections = list;
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
      const service = USAGE_OF[c.id];
      if (service && c.state === 'ok') addUsage(li, name, service);
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
  refreshBrowser();
  $('conn-dot').dataset.state = 'checking';
  try {
    const list = await invoke('connections_status');
    markitdownReady = list.some((c) => c.id === 'markitdown' && c.state === 'ok');
    agyReady = list.some((c) => c.id === 'antigravity' && c.state === 'ok');
    playwrightReady = list.some((c) => c.id === 'playwright' && c.state === 'ok');
    renderReadMode();
    $('link-form').classList.toggle('unavailable', !agyReady);
    $('link-input').title = agyReady ? '' : 'Antigravity (agy) no está disponible: revisa Conexiones';
    renderConnections(list);
    renderSources();
    for (const [id, service] of Object.entries(USAGE_OF)) {
      if (list.some((c) => c.id === id && c.state === 'ok')) loadUsage(service, usageForce);
    }
    usageForce = false;
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
let usageForce = false;
$('conn-refresh').addEventListener('click', () => {
  usageForce = true;
  refreshConnections();
});

// Navegador: copia del perfil del navegador predeterminado (sesiones, cookies, contraseñas)
// que usan Playwright al leer enlaces y Claude Code.
let browserBusy = false;

function copyDate(copy) {
  const date = new Date(copy.copied_at.replace(' ', 'T'));
  if (Number.isNaN(date.getTime())) return copy.copied_at;
  return date.toLocaleString('es', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
}

function renderBrowser(status) {
  const { copy } = status;
  const source = status.source.Ok;
  $('browser-dot').dataset.state = copy ? 'ok' : 'warn';
  $('browser-state').textContent = copy ? 'Cargado' : 'Sin cargar';
  $('browser-detail').textContent = copy
    ? `Copia de ${copy.browser} · ${copyDate(copy)} · ${copy.cookies} cookies de ${copy.sites} sitios`
    : source
      ? `Playwright y Claude Code abren un navegador sin tus sesiones. Puedes copiar las de ${source}.`
      : status.source.Err;
  $('browser-load-label').textContent = copy ? 'Actualizar navegador' : 'Cargar navegador';
  $('browser-load-hint').textContent = copy
    ? `Vuelve a copiar ${source ?? copy.browser} y borra la copia del ${copyDate(copy)}`
    : source
      ? `Copia las sesiones y contraseñas de ${source}`
      : 'Necesita Chrome, Chromium, Brave, Edge o Vivaldi';
  $('browser-load').disabled = browserBusy || !source;
}

async function refreshBrowser() {
  if (!desktop) return;
  try {
    renderBrowser(await invoke('browser_status'));
  } catch (error) {
    fail(error);
  }
}

function browserNote(text, kind) {
  const note = $('browser-note');
  note.textContent = text;
  note.dataset.kind = kind;
  note.hidden = false;
}

$('browser-load').addEventListener('click', async (event) => {
  event.stopPropagation();
  if (!desktop) return printLine('Cargar el navegador solo funciona en la app de escritorio.', 'error');
  if (browserBusy) return;
  const updating = $('browser-load-label').textContent.startsWith('Actualizar');
  browserBusy = true;
  $('browser-load').disabled = true;
  $('browser-load-label').textContent = updating ? 'Actualizando navegador…' : 'Cargando navegador…';
  $('browser-load-hint').textContent = 'Copiando el perfil y comprobando que se abre';
  $('browser-dot').dataset.state = 'checking';
  $('browser-note').hidden = true;
  try {
    const copy = await invoke('browser_import');
    const message = `${updating ? 'Navegador actualizado' : 'Navegador cargado'}: se copió ${copy.browser} correctamente, con ${copy.cookies} cookies de ${copy.sites} sitios (sesiones iniciadas) y sus contraseñas guardadas.${updating ? ' La copia anterior se borró.' : ''}`;
    browserNote(message, 'ok');
    printLine(message, 'info');
  } catch (error) {
    browserNote(String(error), 'error');
    printLine(String(error), 'error');
  } finally {
    browserBusy = false;
    refreshBrowser();
  }
});
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

// ---------------------------------------------------------------- pestaña de resumen de una fuente
let detailPath = null;
let noteNode = null; // nodo de la wiki abierto en la carta (ver «contenido de un nodo»)
let noteView = null; // { id, body, meta, modified }
let detailView = null; // ficha: resumen, ideas clave, conceptos

async function openDetail(path, { fromGraph = false } = {}) {
  noteNode = null;
  noteView = null;
  detailPath = path;
  detailView = null;
  closeRename();
  renderSources();
  renderDetail();
  try {
    detailView = await invoke('source_detail', { path });
  } catch (error) {
    fail(error);
  }
  if (detailPath !== path) return;
  renderDetail();
  if (!fromGraph && detailView) graph.focus(detailView.id);
}

// Vuelve a leer la ficha de la fuente abierta (p. ej. cuando Antigravity termina su resumen).
async function refreshDetail() {
  if (!detailPath) return;
  const path = detailPath;
  try {
    const view = await invoke('source_detail', { path });
    if (detailPath === path) detailView = view;
  } catch (error) {
    fail(error);
  }
  if (detailPath === path) renderDetail();
}

function closeDetail() {
  detailPath = null;
  detailView = null;
  closeRename();
  renderSources();
}

function renderDetail() {
  const source = sources.find((s) => s.path === detailPath);
  if (!source) return closeDetail();
  $('detail-icon').innerHTML = svg(ICONS[source.kind]);
  $('detail-name').textContent = detailView?.name ?? source.name;
  $('detail-meta').textContent = `${KIND_LABEL[source.kind]} · ${formatSize(source.size)} · raw/${source.path}`;
  const working = summarizing.has(source.path);
  const summary = $('detail-summary');
  summary.classList.toggle('pending', !detailView?.summarized);
  summary.textContent = working
    ? 'Antigravity está leyendo la fuente…'
    : detailView?.summarized
      ? detailView.summary
      : hasText(source)
        ? 'Aún sin resumen. Pídeselo a Antigravity.'
        : 'Esta fuente no tiene texto que resumir (convierte el PDF a Markdown o añade una fuente de texto).';
  const points = detailView?.key_points ?? [];
  $('detail-points').replaceChildren(...points.map((p) => Object.assign(document.createElement('li'), { textContent: p })));
  $('detail-points').hidden = $('detail-points-title').hidden = !points.length;
  const concepts = detailView?.concepts ?? [];
  $('detail-concepts').replaceChildren(
    ...concepts.map((c) => {
      const chip = document.createElement('button');
      chip.type = 'button';
      chip.className = 'concept-chip';
      chip.textContent = c;
      chip.title = 'Ver en el grafo';
      chip.addEventListener('click', () => focusConcept(c));
      return chip;
    }),
  );
  $('detail-concepts').hidden = $('detail-concepts-title').hidden = !concepts.length;
  const button = $('detail-summarize');
  $('detail-eyebrow').textContent = `Fuente · ${KIND_LABEL[source.kind]}`;
  $('detail-summarize-label').textContent = working
    ? 'Antigravity trabajando…'
    : detailView?.summarized
      ? 'Actualizar resumen'
      : 'Resumir con Antigravity';
  button.disabled = working || !hasText(source) || !agyReady;
  button.classList.toggle('working', working);
  button.title = agyReady ? '' : 'Antigravity (agy) no está disponible: revisa Conexiones';
  $('detail-open-md').hidden = !source.converted;
  $('detail-graph').disabled = !detailView;
}

// ---------------------------------------------------------------- contenido de un nodo
// Al elegir un nodo del grafo (que no sea una fuente), la carta de la izquierda muestra
// lo que hay dentro de su nota: el Markdown, sus propiedades y sus conexiones.
async function showNode(node) {
  if (detailPath) closeDetail();
  noteNode = node;
  noteView = null;
  renderSources();
  renderNode();
  if (node.kind === 'missing') return;
  try {
    const view = await invoke('note_view', { id: node.id });
    if (noteNode?.id === node.id) noteView = view;
  } catch (error) {
    fail(error);
  }
  if (noteNode?.id === node.id) renderNode();
}

// Vuelve a leer el nodo abierto (cuando cambia la wiki); si ya no existe, se cierra.
async function refreshNode() {
  if (!noteNode) return;
  const fresh = graphData.nodes.find((n) => n.id === noteNode.id);
  if (!fresh) return closeNode();
  const id = fresh.id;
  if (fresh.kind !== 'missing') {
    try {
      const view = await invoke('note_view', { id });
      if (noteNode?.id === id) noteView = view;
    } catch {}
  }
  if (noteNode?.id === id) {
    noteNode = fresh;
    renderNode();
  }
}

function closeNode() {
  noteNode = null;
  noteView = null;
  renderSources();
}

function neighborsOf(id) {
  const ids = new Set();
  for (const [a, b] of graphData.edges) {
    if (a === id) ids.add(b);
    else if (b === id) ids.add(a);
  }
  return graphData.nodes.filter((n) => ids.has(n.id)).sort((a, b) => a.label.localeCompare(b.label, 'es'));
}

const NODE_STAR = '<svg width="16" height="16" viewBox="0 0 20 20" fill="currentColor" stroke="none"><path d="M10 1.5c.6 4.6 3.9 7.9 8.5 8.5-4.6.6-7.9 3.9-8.5 8.5-.6-4.6-3.9-7.9-8.5-8.5 4.6-.6 7.9-3.9 8.5-8.5z"/></svg>';

function renderNode() {
  const node = noteNode;
  if (!node) return;
  const missing = node.kind === 'missing';
  const linked = neighborsOf(node.id);
  $('note-icon').innerHTML = NODE_STAR;
  $('note-icon').style.color = colorFor(node);
  $('note-eyebrow').textContent = node.group && !missing ? `${SEARCH_TYPE[node.kind]} · ${node.group}` : SEARCH_TYPE[node.kind];
  $('note-name').textContent = node.label;
  $('note-meta').textContent = missing
    ? `Sin nota · ${linked.length} ${linked.length === 1 ? 'mención' : 'menciones'}`
    : [`wiki/${node.id}`, noteView?.modified && `editada ${noteView.modified}`].filter(Boolean).join(' · ');

  const props = noteView?.meta ?? [];
  $('note-props').replaceChildren(
    ...props.map(([key, value]) => {
      const row = document.createElement('div');
      row.className = 'note-prop';
      const k = document.createElement('span');
      k.textContent = key;
      const v = document.createElement('span');
      v.textContent = value;
      row.append(k, v);
      return row;
    }),
  );
  $('note-props').hidden = !props.length;

  const body = $('note-body');
  body.classList.toggle('pending', missing || !noteView);
  if (missing) body.textContent = 'Esta palabra clave todavía no tiene nota propia. Aparece en el grafo porque otras notas la enlazan con [[…]].';
  else if (!noteView) body.textContent = 'Leyendo la nota…';
  else if (!noteView.body.trim()) body.textContent = 'La nota está vacía.';
  else body.innerHTML = renderMarkdown(noteView.body);

  $('note-links-title').textContent = missing ? 'La enlazan' : `Conexiones · ${linked.length}`;
  $('note-links').replaceChildren(
    ...linked.map((n) => {
      const chip = document.createElement('button');
      chip.type = 'button';
      chip.className = 'node-chip';
      chip.title = `${SEARCH_TYPE[n.kind]} · ver en el grafo`;
      const dot = document.createElement('span');
      dot.className = 'node-chip-dot';
      dot.style.background = colorFor(n);
      chip.append(dot, n.label);
      chip.addEventListener('click', () => graph.focus(n.id));
      return chip;
    }),
  );
  $('note-links').hidden = $('note-links-title').hidden = !linked.length;
  $('note-open').hidden = missing;
  $('note-notes').hidden = !isNotesNode(node);
  $('note-notes').textContent = node.kind === 'index' ? 'Abrir tema en Notas' : 'Abrir en el escritorio';
}

$('note-back').addEventListener('click', closeNode);
$('note-open').addEventListener('click', () => noteNode && openNote(noteNode));
$('note-graph').addEventListener('click', () => noteNode && graph.focus(noteNode.id));
$('note-notes').addEventListener('click', () => noteNode && openInNotes(noteNode.id));
// Enlaces dentro de la nota: [[wiki]] y notas .md llevan a su nodo; las webs se abren fuera.
$('note-body').addEventListener('click', (event) => {
  const link = event.target.closest('a');
  if (!link || !noteNode) return;
  event.preventDefault();
  followLink(link, noteNode.id);
});

// Un concepto de la ficha → su nodo (nota existente o pendiente) en el grafo.
function focusConcept(name) {
  const key = name.toLowerCase();
  const node = graphData.nodes.find(
    (n) =>
      n.id.toLowerCase() === `?${key}` ||
      n.id.toLowerCase() === `${key}.md` ||
      n.id.toLowerCase().endsWith(`/${key}.md`) ||
      n.label.toLowerCase() === key,
  );
  if (!node || !graph.focus(node.id)) printLine(`«${name}» aún no está en el grafo`, 'info');
}

function closeRename() {
  $('rename-form').hidden = true;
  $('detail-name').hidden = false;
  $('detail-rename').hidden = false;
}

$('detail-back').addEventListener('click', closeDetail);
$('detail-rename').addEventListener('click', () => {
  const source = sources.find((s) => s.path === detailPath);
  if (!source) return;
  $('rename-input').value = source.name.replace(/\.[^.]+$/, '');
  $('rename-form').hidden = false;
  $('detail-name').hidden = true;
  $('detail-rename').hidden = true;
  $('rename-input').select();
});
$('rename-cancel').addEventListener('click', closeRename);
$('rename-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  const name = $('rename-input').value.trim();
  if (!name || !detailPath) return;
  const old = detailPath;
  try {
    const renamed = await invoke('rename_source', { path: old, name });
    printLine(`raw/${old} → raw/${renamed.path}`, 'system');
    if (excluded.delete(old)) excluded.add(renamed.path);
    closeRename();
    await Promise.all([loadSources(), loadGraph()]);
    openDetail(renamed.path);
  } catch (error) {
    fail(error);
  }
});
$('rename-input').addEventListener('keydown', (event) => {
  if (event.key === 'Escape') {
    event.stopPropagation();
    closeRename();
  }
});
$('detail-summarize').addEventListener('click', () => detailPath && summarizeSource(detailPath));
$('detail-open').addEventListener('click', () => detailPath && invoke('open_source', { path: detailPath }).catch(fail));
$('detail-open-md').addEventListener('click', () =>
  detailPath && invoke('open_source', { path: `markdown/${detailPath.replace(/\.[^./]+$/, '')}.md` }).catch(fail),
);
$('detail-graph').addEventListener('click', () => detailView && graph.focus(detailView.id));

// ---------------------------------------------------------------- actividad de Antigravity
// Cada tarea de agy (leer una web, resumir una fuente) llega como evento `agy-activity`.
const jobs = new Map();
let activityOpen = false;
const JOB_KIND = { web: 'Página web', resumen: 'Resumen de fuente' };

function jobStatus(job) {
  const seconds = Math.round(((job.finished ?? Date.now()) - job.started) / 1000);
  const tokens = job.tokens ? ` · ${(job.tokens / 1000).toFixed(1)}k tokens` : '';
  return {
    queued: 'En cola',
    running: `En curso · ${seconds} s`,
    done: `Hecho · ${seconds} s${tokens}`,
    error: 'Error',
  }[job.status];
}

function renderActivity() {
  const list = [...jobs.values()].sort((a, b) => b.id - a.id);
  const active = list.filter((j) => j.status === 'running' || j.status === 'queued').length;
  $('activity-count').hidden = !active;
  $('activity-count').textContent = `${active} en curso`;
  $('activity-dot').classList.toggle('busy', active > 0);
  $('rail-activity').hidden = active === 0;
  if (!activityOpen) return;
  $('activity-empty').hidden = list.length > 0;
  $('activity-list').replaceChildren(
    ...list.map((job) => {
      const li = document.createElement('li');
      li.className = `job ${job.status}`;
      const core = document.createElement('div');
      core.className = 'job-core';
      li.append(core);
      const head = document.createElement('div');
      head.className = 'job-head';
      const kind = Object.assign(document.createElement('span'), { className: 'job-kind', textContent: JOB_KIND[job.kind] ?? job.kind });
      const status = Object.assign(document.createElement('span'), { className: 'job-status', textContent: jobStatus(job) });
      const title = Object.assign(document.createElement('span'), { className: 'job-title', textContent: job.title, title: job.title });
      head.append(kind, status, title);
      core.append(head);
      if (job.steps.length) {
        const steps = document.createElement('ol');
        steps.className = 'job-steps';
        for (const step of job.steps) {
          const row = document.createElement('li');
          const dot = Object.assign(document.createElement('span'), { className: `step-dot ${step.state}` });
          const label = Object.assign(document.createElement('span'), { textContent: step.label, title: step.label });
          const time = Object.assign(document.createElement('span'), {
            className: 'step-time',
            textContent: step.seconds != null ? `${step.seconds.toFixed(1)} s` : step.state === 'active' ? '…' : '',
          });
          row.append(dot, label, time);
          steps.append(row);
        }
        core.append(steps);
      }
      if (job.result || job.error) {
        core.append(Object.assign(document.createElement('p'), { className: 'job-foot', textContent: job.error ?? `→ ${job.result}` }));
      }
      return li;
    }),
  );
}

function showActivity(open) {
  activityOpen = open;
  $('activity-button').setAttribute('aria-pressed', String(open));
  $('activity-view').hidden = !open;
  const cc = mode === 'claude-code';
  $('output').hidden = open || cc;
  $('prompt').hidden = open || cc;
  $('cc-view').hidden = open || !cc;
  renderActivity();
  // Al cerrar la actividad se vuelve a la vista anterior con el foco donde corresponde.
  if (!open && cc && vault && desktop) claudeCode.show();
  else if (!open && !cc) $('prompt-input').focus();
}

$('activity-button').addEventListener('click', () => showActivity(!activityOpen));
$('activity-clear').addEventListener('click', async () => {
  await invoke('agy_clear_finished').catch(fail);
  for (const [id, job] of jobs) if (job.finished) jobs.delete(id);
  renderActivity();
});
// Los segundos de las tareas en curso avanzan aunque no llegue ningún evento.
setInterval(() => {
  if (activityOpen && [...jobs.values()].some((j) => j.status === 'running')) renderActivity();
}, 1000);

if (desktop) {
  listen('agy-activity', ({ payload }) => {
    const before = jobs.get(payload.id);
    jobs.set(payload.id, payload);
    renderActivity();
    // Al terminar una tarea de Antigravity las cartas se actualizan solas.
    if (payload.finished && !before?.finished) {
      Promise.all([loadSources(), loadGraph()]).then(refreshDetail);
    }
  });
  // Cuando algo escribe en wiki/ (Claude Code, Obsidian…), el grafo se actualiza solo
  // y los nodos nuevos se señalan.
  let wikiTimer = 0;
  let wikiAdded = [];
  listen('wiki-changed', ({ payload }) => {
    notes.onWikiChanged(payload.changed ?? payload.added);
    wikiAdded.push(...payload.added);
    clearTimeout(wikiTimer);
    wikiTimer = setTimeout(async () => {
      const added = wikiAdded;
      wikiAdded = [];
      await loadGraph();
      refreshDetail();
      refreshNode();
      if (added.length) graph.announce(added);
    }, 400);
  });
  listen('desk-changed', () => notes.onDeskChanged());
  invoke('agy_jobs')
    .then((list) => {
      list.forEach((job) => jobs.set(job.id, job));
      renderActivity();
    })
    .catch(() => {});
}

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
    if (node.kind === 'source' && node.source) {
      if (node.source !== detailPath) openDetail(node.source, { fromGraph: true });
    } else if (node.id !== noteNode?.id) showNode(node);
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
  graphData = data ?? { nodes: [], edges: [] };
  graph.setData(graphData);
  if (!searchMenu.hidden && searchInput.value.trim()) {
    const current = searchHits[searchAt]?.id;
    searchHits = findNodes(searchInput.value);
    searchAt = Math.max(0, searchHits.findIndex((n) => n.id === current));
    renderSearch();
  }
}
let graphData = { nodes: [], edges: [] };

// ---------------------------------------------------------------- vistas: Grafo / Notas
// Notas cambia la carta de la izquierda por temas y notas y el grafo por el escritorio
// de notas; la terminal de la derecha no cambia.
const notes = createNotes({
  invoke: (cmd, args) => (desktop ? invoke(cmd, args) : Promise.reject('Las notas solo funcionan en la app de escritorio.')),
  ask: (message, options) => (desktop ? ask(message, options) : Promise.resolve(window.confirm(message))),
  renderMarkdown,
  fail,
  onLink: (link, from) => followLink(link, `notas/${from}`),
});
let view = 'grafo';
let notesStale = true;
// Activación en curso del apartado Notas: quien vaya a abrir algo en él la espera
// (si no, la activación cerraría la carta recién abierta al aplicar el escritorio guardado).
let notesActivation = Promise.resolve();

function activateNotes() {
  if (!desktop || !vault) return notesActivation;
  const reset = notesStale;
  notesStale = false;
  notesActivation = notesActivation.then(() => notes.activate({ reset })).catch(fail);
  return notesActivation;
}

function setView(next) {
  view = next === 'notas' ? 'notas' : 'grafo';
  document.body.dataset.view = view;
  document.querySelectorAll('[data-view]').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.view === view)));
  const inNotes = view === 'notas';
  document.querySelector('#sources-panel > .panel-core.sources').hidden = inNotes;
  $('notes-core').hidden = !inNotes;
  $('desk').hidden = !inNotes;
  $('desk-toolbar').hidden = !inNotes;
  $('stage').setAttribute('aria-label', inNotes ? 'Escritorio de notas' : 'Grafo de conocimiento');
  try {
    localStorage.setItem('nexo.view', view);
  } catch {}
  if (inNotes) {
    activateNotes();
    requestAnimationFrame(() => notes.relayout());
  }
}
document.querySelectorAll('[data-view]').forEach((b) => b.addEventListener('click', () => setView(b.dataset.view)));
try {
  if (localStorage.getItem('nexo.view') === 'notas') setView('notas');
} catch {}

// Un nodo de wiki/notas/ → su nota en el escritorio (o su tema en la carta de notas).
async function openInNotes(id) {
  const rel = id.replace(/^notas\//, '');
  if (view !== 'notas') setView('notas');
  await notesActivation;
  if (/(^|\/)_index\.md$/.test(rel)) return notes.openTopic(rel.replace(/\/?_index\.md$/, ''));
  return notes.openNote(rel);
}
const isNotesNode = (node) => node?.id?.startsWith('notas/') && node.kind !== 'missing';

// Enlaces dentro de notas y nodos: webs fuera; [[notas]] a su carta; el resto al grafo.
function followLink(link, fromId) {
  const href = link.getAttribute('href') ?? '';
  if (/^https?:\/\//i.test(href)) {
    if (desktop) openUrl(href).catch(fail);
    else window.open(href, '_blank', 'noopener');
    return;
  }
  const target = link.dataset.target ?? decodeURIComponent(href.split('#')[0]);
  if (!target) return;
  const node = resolveLink(graphData.nodes, fromId, target);
  if (!node) return printLine(`«${target}» aún no está en el grafo`, 'info');
  if (isNotesNode(node)) return openInNotes(node.id);
  setView('grafo');
  graph.focus(node.id);
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

// ---------------------------------------------------------------- búsqueda en el grafo
// Busca solo en los nombres de los nodos (fuentes, notas, temas, palabras clave), nunca en el
// contenido de las notas. Las flechas recorren las coincidencias y centran cada una.
const SEARCH_TYPE = {
  source: 'Fuente',
  note: 'Nodo',
  index: 'Tema',
  conversation: 'Conversación',
  missing: 'Palabra clave',
};
const SEARCH_ORDER = ['source', 'note', 'index', 'conversation', 'missing'];
const searchMenu = $('search-menu');
const searchButton = $('search-button');
const searchInput = $('search-input');
let searchHits = [];
let searchAt = -1;

const fold = (text) => text.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();

function findNodes(query) {
  const q = fold(query.trim());
  if (!q) return [];
  return graphData.nodes
    .map((node) => {
      // Nombre visible y, para notas, el nombre de archivo (p. ej. «busqueda-hibrida»).
      const names = [node.label, node.id.replace(/^\?/, '').split('/').pop().replace(/\.md$/, '')];
      const found = names.map(fold).find((n) => n.includes(q));
      return found ? { node, starts: found.startsWith(q) } : null;
    })
    .filter(Boolean)
    .sort(
      (a, b) =>
        Number(b.starts) - Number(a.starts) ||
        SEARCH_ORDER.indexOf(a.node.kind) - SEARCH_ORDER.indexOf(b.node.kind) ||
        a.node.label.localeCompare(b.node.label),
    )
    .map((hit) => hit.node);
}

// Resalta lo buscado dentro del nombre sin usar innerHTML con texto del usuario.
function highlighted(label, query) {
  const span = document.createElement('span');
  span.className = 'search-result-label';
  const at = fold(label).indexOf(fold(query.trim()));
  if (at < 0) {
    span.textContent = label;
    return span;
  }
  const end = at + query.trim().length;
  const mark = document.createElement('mark');
  mark.textContent = label.slice(at, end);
  span.append(label.slice(0, at), mark, label.slice(end));
  return span;
}

function renderSearch() {
  const query = searchInput.value;
  const has = searchHits.length > 0;
  $('search-count').textContent = !query.trim() ? '' : has ? `${searchAt + 1} de ${searchHits.length}` : '0';
  $('search-prev').disabled = $('search-next').disabled = searchHits.length < 2;
  $('search-hint').hidden = Boolean(query.trim()) && has;
  $('search-hint').textContent = !query.trim()
    ? 'Busca por nombre en el grafo, no dentro del contenido de las notas.'
    : !graphData.nodes.length
      ? 'El grafo está vacío.'
      : `Nada en el grafo se llama «${query.trim()}».`;
  $('search-results').replaceChildren(
    ...searchHits.map((node, i) => {
      const li = document.createElement('li');
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'search-result';
      if (i === searchAt) button.setAttribute('aria-current', 'true');
      const dot = document.createElement('span');
      dot.className = `search-result-dot${node.kind === 'missing' ? ' missing' : ''}`;
      dot.style.color = colorFor(node);
      const type = Object.assign(document.createElement('span'), {
        className: 'search-type',
        textContent: SEARCH_TYPE[node.kind] ?? 'Nodo',
      });
      button.append(dot, highlighted(node.label, query), type);
      button.addEventListener('click', () => goToHit(i));
      li.append(button);
      return li;
    }),
  );
  $('search-results').querySelector('[aria-current="true"]')?.scrollIntoView({ block: 'nearest' });
}

function goToHit(index) {
  if (!searchHits.length) return;
  searchAt = (index + searchHits.length) % searchHits.length;
  const hit = searchHits[searchAt];
  // En la vista Notas, las notas se abren en el escritorio; lo demás lleva al grafo.
  if (view === 'notas' && isNotesNode(hit)) openInNotes(hit.id);
  else {
    if (view === 'notas') setView('grafo');
    graph.focus(hit.id);
  }
  renderSearch();
}

function openSearch() {
  searchMenu.hidden = false;
  searchButton.setAttribute('aria-expanded', 'true');
  searchInput.focus();
  searchInput.select();
  renderSearch();
}
function closeSearch() {
  searchMenu.hidden = true;
  searchButton.setAttribute('aria-expanded', 'false');
}

searchInput.addEventListener('input', () => {
  searchHits = findNodes(searchInput.value);
  searchAt = searchHits.length ? 0 : -1;
  if (searchHits.length) graph.focus(searchHits[0].id);
  renderSearch();
});
searchInput.addEventListener('keydown', (event) => {
  if (event.key === 'Enter' || event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    const back = event.key === 'ArrowUp' || (event.key === 'Enter' && event.shiftKey);
    goToHit(searchAt + (back ? -1 : 1));
  } else if (event.key === 'Escape') {
    event.stopPropagation();
    closeSearch();
    searchButton.focus();
  }
});
$('search-prev').addEventListener('click', () => goToHit(searchAt - 1));
$('search-next').addEventListener('click', () => goToHit(searchAt + 1));
searchButton.addEventListener('click', (event) => {
  event.stopPropagation();
  if (searchMenu.hidden) openSearch();
  else closeSearch();
});
document.addEventListener('click', (event) => {
  if (!searchMenu.hidden && !searchMenu.contains(event.target) && !searchButton.contains(event.target)) closeSearch();
});
// ⌘K / Ctrl+K abre la búsqueda (salvo dentro de la terminal de Claude Code, donde Ctrl+K es suyo).
const isMac = /Mac/i.test(navigator.platform);
$('search-kbd').textContent = isMac ? '⌘K' : 'Ctrl K';
document.addEventListener('keydown', (event) => {
  if ((isMac ? event.metaKey : event.ctrlKey) && event.key.toLowerCase() === 'k' && !event.target.closest?.('.xterm')) {
    event.preventDefault();
    openSearch();
  }
});

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
      if (view === 'notas') activateNotes();
    } catch (error) {
      fail(error);
    }
  })();
} else {
  renderSources();
  loadGraph();
}
