import '@fontsource-variable/inter';
import '@fontsource/geist-mono/400.css';
import '@fontsource/geist-mono/500.css';
import '@fontsource/geist-mono/700.css';
import '@fontsource/poppins/latin-500.css';
import '@fontsource/poppins/latin-ext-500.css';
import '@fontsource/poppins/latin-600.css';
import '@fontsource/poppins/latin-ext-600.css';
import '@fontsource/poppins/latin-700.css';
import '@fontsource/poppins/latin-ext-700.css';
import './styles.css';

import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { ask, open } from '@tauri-apps/plugin-dialog';
import { openUrl } from '@tauri-apps/plugin-opener';

import { createCardHold } from './card-resize.js';
import { createChat, mountChat } from './chat.js';
import { reconcile } from './dom.js';
import { createAgents } from './agents.js';
import { createGalaxy } from './galaxy.js';
import { createAgentLog } from './registro.js';
import { colorFor, createGraph } from './graph.js';
import { createNotes } from './notes.js';
import { renderMarkdown, resolveLink } from './note-view.js';
import { commandFor, createSkills } from './skills.js';
import { createSettings } from './settings.js';
import { currentTheme, onTheme } from './themes.js';
import { activity, createWorkspace } from './workspace.js';
import { agentsButtonLabel, createAttention, createWatch, watchedPath } from './ws-attention.js';
import { formatTokens } from './registro.js';
import { STATE_LABEL as AGENT_STATE } from './ws-stream.js';

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
// Las dos cartas se pueden minimizar a un riel estrecho: la derecha (el chat) y la izquierda
// (fuentes, notas o agentes, según la vista).
let minimized = false;
let minimizedLeft = false;
try {
  minimized = localStorage.getItem('nexo.minimized') === 'true';
  minimizedLeft = localStorage.getItem('nexo.minimizedLeft') === 'true';
} catch {}
// Al entrar en Notas el chat se minimiza solo para dejar sitio al lienzo; al salir vuelve,
// salvo que el usuario lo haya tocado entretanto.
let chatAutoMin = false;
try {
  Object.assign(expanded, JSON.parse(localStorage.getItem('nexo.expanded') ?? '{}'));
} catch {}

// El grafo deja de dibujarse mientras está tapado (se conecta más abajo, al crearlo).
let syncGraphActive = () => {};

// Cómo se llama la carta izquierda en cada vista (para su riel y sus botones).
const LEFT_CARD = {
  grafo: { label: 'Fuentes', card: 'la carta de fuentes' },
  notas: { label: 'Notas', card: 'la carta de notas' },
  agentes: { label: 'Agentes', card: 'la carta de agentes' },
};
const WS_CARD = { label: 'Workspace', card: 'la carta del workspace' };
// La vista Agentes tiene dos paneles: el de nexo (skills, agentes del vault y el registro) y el
// de un workspace (su carpeta, sus agentes y sus ventanas). Se recuerda cuál se dejó.
let agentsMode = 'nexo';
try {
  if (localStorage.getItem('nexo.agentsMode') === 'workspace') agentsMode = 'workspace';
} catch {}

// Lo de dentro de las cartas y del centro no se maqueta en cada fotograma de la transición:
// toma su medida final y se recorta (card-resize.js).
const holdCards = createCardHold({
  layout: $('layout'),
  stage: $('stage'),
  center: [$('ws-center'), $('agent-log')],
  cores: () => [...document.querySelectorAll('#layout > .panel > .panel-core')],
  // Con el grafo plegado no se puede usar su barra ni el enfoque. `inert` vuelve a resolver el
  // estilo de todo lo que lleva el centro (con un workspace abierto, unos 60 ms): se pone y se
  // quita al acabar la transición, cuando ya no se mueve nada.
  onSettle: () => ($('stage').inert = expanded.left || expanded.right),
});

function renderExpanded() {
  const layout = $('layout');
  // Minimizada, una carta no puede estar expandida.
  if (minimized) expanded.right = false;
  if (minimizedLeft) expanded.left = false;
  syncGraphActive();
  layout.classList.toggle('expand-left', expanded.left);
  layout.classList.toggle('expand-right', expanded.right);
  layout.classList.toggle('expanded', expanded.left || expanded.right);
  layout.classList.toggle('min-right', minimized);
  layout.classList.toggle('min-left', minimizedLeft);
  $('term-rail').hidden = !minimized;
  document.querySelector('#terminal-panel > .panel-core').hidden = minimized;
  // La izquierda tiene un núcleo por vista, cada uno con su `hidden`: minimizada se ocultan
  // todos con una clase (ver styles.css), sin perder cuál toca enseñar al volver.
  $('sources-panel').classList.toggle('is-min', minimizedLeft);
  $('left-rail').hidden = !minimizedLeft;
  const leftName = document.body.dataset.view === 'agentes' && agentsMode === 'workspace' ? WS_CARD : (LEFT_CARD[document.body.dataset.view] ?? LEFT_CARD.grafo);
  $('left-rail-label').textContent = leftName.label;
  for (const [button, verb] of [[$('left-restore'), 'Restaurar'], ...[...document.querySelectorAll('.left-minimize')].map((b) => [b, 'Minimizar'])]) {
    button.setAttribute('aria-label', `${verb} ${leftName.card}`);
    button.title = `${verb} ${leftName.card}`;
  }
  document.querySelectorAll('[data-expand]').forEach((b) => {
    const on = expanded[b.dataset.expand];
    const what =
      b.dataset.expand === 'right'
        ? 'el chat'
        : b.closest('#notes-core')
          ? 'la carta de notas'
          : b.closest('#agents-core')
            ? 'la carta de agentes'
            : b.closest('#ws-core')
              ? 'la carta del workspace'
              : 'la carta de fuentes';
    b.setAttribute('aria-pressed', String(on));
    b.setAttribute('aria-label', `${on ? 'Contraer' : 'Expandir'} ${what}`);
    b.title = `${on ? 'Contraer' : 'Expandir'} ${what}`;
  });
  holdCards();
}

// WebKitGTK deja rastros de 1 px (el borde de la carta en cada fotograma de la transición) en
// la franja que una carta deja libre al minimizarse, expandirse o contraerse. Al acabar la
// transición se repinta todo `.layout` cambiando su fondo, que no se ve (alfa casi cero).
let repaintFrame = 0;
let repaintFlip = false;
$('layout').addEventListener('transitionend', (event) => {
  if (!event.propertyName.startsWith('flex') || !event.target.matches('.panel, .stage') || repaintFrame) return;
  // Varias cartas terminan a la vez: un solo cambio por fotograma, o se anularían entre sí.
  repaintFrame = requestAnimationFrame(() => {
    repaintFrame = 0;
    repaintFlip = !repaintFlip;
    $('layout').style.backgroundColor = repaintFlip ? 'rgba(0, 0, 0, 0.002)' : '';
  });
});

document.querySelectorAll('[data-expand]').forEach((b) =>
  b.addEventListener('click', () => {
    expanded[b.dataset.expand] = !expanded[b.dataset.expand];
    renderExpanded();
    try {
      localStorage.setItem('nexo.expanded', JSON.stringify(expanded));
    } catch {}
    // Expandir o contraer el chat es para seguir escribiendo en él.
    if (b.dataset.expand === 'right') focusChat();
  }),
);
renderExpanded();

// Lleva el teclado al chat de la carta derecha.
function focusChat() {
  if (minimized || activityOpen) return;
  chat.focus();
}

// `auto` es la minimización que hace la app al entrar y salir de Notas: no se guarda como
// preferencia ni le quita el foco a lo que el usuario esté haciendo.
function setMinimized(value, { auto = false } = {}) {
  minimized = value;
  if (!auto) chatAutoMin = false;
  renderExpanded();
  if (!auto) {
    try {
      localStorage.setItem('nexo.minimized', String(minimized));
      localStorage.setItem('nexo.expanded', JSON.stringify(expanded));
    } catch {}
  }
  if (minimized) {
    // El botón que se pulsó desaparece con la carta: el foco pasa al que la devuelve.
    if (!auto) $('term-restore').focus();
    return;
  }
  // Al volver, la conversación baja hasta el final y el teclado va al campo.
  if (!auto && !activityOpen) requestAnimationFrame(() => chat.show());
}
$('term-minimize').addEventListener('click', () => setMinimized(true));
$('term-restore').addEventListener('click', () => setMinimized(false));
$('rail-chat').addEventListener('click', () => setMinimized(false));

function setMinimizedLeft(value, { focus = true } = {}) {
  if (minimizedLeft === value) return;
  minimizedLeft = value;
  renderExpanded();
  try {
    localStorage.setItem('nexo.minimizedLeft', String(minimizedLeft));
    localStorage.setItem('nexo.expanded', JSON.stringify(expanded));
  } catch {}
  if (!focus) return;
  // El foco sigue al control equivalente: del botón de minimizar al de restaurar y al revés.
  if (minimizedLeft) $('left-restore').focus();
  else document.querySelector('#sources-panel > .panel-core:not([hidden]) .left-minimize')?.focus();
}
document.querySelectorAll('.left-minimize').forEach((b) => b.addEventListener('click', () => setMinimizedLeft(true)));
$('left-restore').addEventListener('click', () => setMinimizedLeft(false));
// Lo que se va a enseñar en la carta izquierda (un nodo, una fuente, un tema, las skills) no
// se vería con ella minimizada: se restaura, sin mover el foco de donde esté.
const revealLeft = () => setMinimizedLeft(false, { focus: false });

// ---------------------------------------------------------------- chat con Claude
// La carta derecha: Claude Code con la suscripción del usuario, pintado como un chat
// (chat.js). Lo que la app tiene que decir (avisos y errores de las fuentes, de las
// conversiones, de Antigravity…) sale como notas dentro de la conversación.
function printLine(text, kind) {
  chat.notice(text, kind === 'error' ? 'error' : 'info');
}

const fail = (error) => printLine(String(error), 'error');

// Órdenes de nexo que salen en el menú «/» del chat, junto a las de Claude Code. Las tres
// primeras van por la API de Anthropic (nexo.rs), no por Claude Code.
const LOCAL_COMMANDS = [
  { name: 'compilar', description: 'Integra en wiki/ las fuentes marcadas que sean nuevas (API de Anthropic)', run: compileSources },
  { name: 'preguntar', argumentHint: '<pregunta>', description: 'Responde con tu wiki por la API y lo guarda en output/query-results.md', run: askWiki },
  { name: 'clave', argumentHint: '<sk-ant-…>', description: 'Guarda tu clave de la API de Anthropic', run: saveKey },
  { name: 'convertir', description: 'Pasa a Markdown los PDF y documentos pendientes (MarkItDown, sin IA)', run: convertPending },
  { name: 'resumir', description: 'Antigravity resume las fuentes que aún no tienen resumen', run: summarizePending },
  { name: 'guardar', description: 'Claude guarda las conclusiones de la conversación en el grafo', run: saveConversation },
  { name: 'nueva', description: 'Empieza una conversación nueva con Claude', run: () => chat.reset() },
];

// El workspace de la vista Agentes (se crea más abajo; el chat de Nexo le avisa de su estado).
let workspace = null;

async function pickFiles() {
  const picked = await open({ multiple: true, title: 'Adjuntar archivos al mensaje' });
  return picked ? [picked].flat() : [];
}

mountChat($('chat-view'), 'chat');
const chat = createChat({
  transport: {
    start({ options, replay, onEvent }) {
      const channel = new Channel();
      channel.onmessage = onEvent;
      return invoke('claude_code_start', { options, replay, channel });
    },
    send: (line, record) => invoke('claude_code_send', { line, record }),
    reset: () => invoke('claude_code_reset'),
  },
  desktop,
  blocked: () => (vault ? null : 'no-vault'),
  getCwd: () => vault?.path ?? '',
  pickFiles,
  onLink: (link) => followLink(link, ''),
  localCommands: () => LOCAL_COMMANDS,
  onState: ({ busy }) => {
    $('rail-working').hidden = !busy;
    workspace?.nexoChanged();
  },
});

// ---------------------------------------------------------------- Nexo por la API
// Lo que va por la API de Anthropic no ha cambiado (nexo.rs): compilar las fuentes marcadas
// (con los commits automáticos del vault), preguntar a la wiki y guardar la clave. Ya no tiene
// pestaña propia: se lanza desde el menú «/» del chat y su progreso sale en la conversación.
let apiBusy = false;

async function runNexo(command, args, reply) {
  if (!desktop) return printLine('Nexo por la API solo funciona en la app de escritorio.', 'error');
  if (!vault) return printLine('Primero elige un vault.', 'error');
  if (apiBusy) return printLine('Nexo sigue con lo anterior: espera a que termine.', 'info');
  apiBusy = true;
  const channel = new Channel();
  channel.onmessage = ({ kind, text }) => (kind === 'text' ? reply?.push(text) : printLine(text, kind === 'error' ? 'error' : 'info'));
  try {
    return await invoke(command, { ...args, channel });
  } catch (error) {
    fail(error);
  } finally {
    reply?.end();
    apiBusy = false;
  }
}

async function compileSources() {
  const selected = sources.filter((s) => !excluded.has(s.path)).map((s) => s.path);
  if (!selected.length) return printLine('No hay fuentes marcadas en contexto.', 'error');
  const count = await runNexo('compile', { sources: selected });
  if (count) {
    printLine(`${count} ${count === 1 ? 'fuente integrada' : 'fuentes integradas'} en wiki/`, 'info');
    await loadGraph();
  }
}

async function askWiki(question) {
  if (!question) return printLine('Uso: /preguntar ¿tu pregunta?', 'error');
  if (apiBusy || !desktop || !vault) return runNexo('ask', { question });
  chat.userNote(`/preguntar ${question}`);
  const saved = await runNexo('ask', { question }, chat.reply('Nexo · API'));
  if (saved) printLine(`guardado en ${saved}`, 'info');
}

// La clave no se pinta nunca: ni en la conversación ni en el aviso.
async function saveKey(key) {
  if (!key) return printLine('Uso: /clave sk-ant-…', 'error');
  if (!desktop) return printLine('Guardar la clave solo funciona en la app de escritorio.', 'error');
  try {
    await invoke('set_api_key', { key });
    printLine('Clave guardada en la configuración de nexo.', 'info');
    refreshConnections();
  } catch (error) {
    fail(error);
  }
}

function convertPending() {
  if (!vault) return printLine('Primero elige un vault.', 'error');
  if (!sources.some((s) => s.convertible && !s.converted)) return printLine('No hay nada pendiente de convertir.', 'info');
  convertSources([]);
}

function summarizePending() {
  if (!vault) return printLine('Primero elige un vault.', 'error');
  const pending = sources.filter((s) => !s.summarized && hasText(s));
  if (!pending.length) return printLine('Todas las fuentes con texto ya tienen resumen.', 'info');
  printLine(`${pending.length} fuentes en la cola de Antigravity`, 'info');
  pending.forEach((s) => summarizeSource(s.path));
}

// Guardar la conversación: Claude escribe sus conclusiones en wiki/conversaciones/ y la nota
// aparece como nodo del grafo. Se revisa el grafo hasta que llegue (o pasen 3 minutos).
async function saveConversation() {
  if (!chat.hasConversation) return printLine('Todavía no hay conversación que guardar.', 'info');
  if (activityOpen) showActivity(false);
  if (!(await chat.send('Guarda la conversación en el grafo.'))) return;
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
}
$('cc-save').addEventListener('click', saveConversation);

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
    // Claude trabaja dentro del vault: al cambiarlo se cierra la conversación anterior y se
    // abre otra en el nuevo.
    await chat.stop();
    if (vault.created.length) printLine(`En el vault se creó: ${vault.created.join(', ')}`, 'info');
    chat.start();
    await refresh();
    notesStale = true;
    if (view === 'notas') activateNotes();
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
  check.className = 'source-check';
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
  document.querySelectorAll('.chips [data-count]').forEach((el) => {
    const kind = el.dataset.count;
    el.textContent = String(kind === 'all' ? sources.length : sources.filter((s) => s.kind === kind).length);
  });

  const visible = filter === 'all' ? sources : sources.filter((s) => s.kind === filter);
  reconcile($('source-list'), visible, {
    key: (s) => s.path,
    sig: (s) => JSON.stringify([s, converting, excluded.has(s.path)]),
    build: sourceRow,
  });
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
    if (info.data.note) {
      panel.append(Object.assign(document.createElement('p'), { className: 'usage-note', textContent: info.data.note }));
    }
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

// Gráficos: cómo pinta WebKit (render.rs). De serie la GPU compone y la CPU dibuja; si WebKit se
// cae, nexo baja solo a sin WebGL y, como último recurso, a solo CPU.
const RENDER_MODE = {
  hibrido: ['ok', 'GPU + CPU', 'La GPU compone la página y el WebGL de la galaxia; la CPU la dibuja, porque dibujar por GPU hacía caer el driver de NVIDIA.'],
  'sin-webgl': ['ok', 'GPU + CPU', 'La GPU compone la página y la CPU la dibuja, sin WebGL: la galaxia usa su versión 2D.'],
  cpu: ['warn', 'CPU', 'nexo pinta solo por CPU. Volverá a probar la GPU cuando se actualice WebKit o el driver.'],
  'forzado-gpu': ['ok', 'GPU', 'Pintado por GPU, fijado al arrancar con una variable de entorno.'],
  'forzado-cpu': ['warn', 'CPU', 'Pintado por CPU, fijado al arrancar con una variable de entorno.'],
};
async function refreshRender() {
  try {
    const info = await invoke('render_info');
    const [dot, state, detail] = RENDER_MODE[info.modo] ?? RENDER_MODE.hibrido;
    $('render-dot').dataset.state = dot;
    $('render-state').textContent = state;
    const rate = info.hz && info.modo !== 'cpu' && info.modo !== 'forzado-cpu' ? ` A ${info.hz} Hz, como tu monitor.` : '';
    $('render-detail').textContent = [info.motivo, `${detail}${rate}`].filter(Boolean).join(' ');
  } catch (error) {
    $('render-state').textContent = 'Error';
    $('render-detail').textContent = String(error);
  }
}

async function refreshConnections() {
  if (!desktop) return;
  refreshBrowser();
  refreshRender();
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
  revealLeft();
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
  revealLeft();
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

let nodeShown = '';
function renderNode() {
  const node = noteNode;
  if (!node) return (nodeShown = '');
  const missing = node.kind === 'missing';
  const linked = neighborsOf(node.id);
  // La wiki avisa de cambios a menudo: si la nota y sus conexiones siguen igual, no se
  // repinta (repintar el cuerpo entero hacía parpadear la carta).
  const shown = JSON.stringify([node, noteView, linked.map((n) => [n.id, n.label, n.kind])]);
  if (shown === nodeShown) return;
  nodeShown = shown;
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
      chip.addEventListener('click', () => showInGraph(n.id));
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
$('note-graph').addEventListener('click', () => noteNode && showInGraph(noteNode.id));
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
  if (!node || !showInGraph(node.id)) printLine(`«${name}» aún no está en el grafo`, 'info');
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
$('detail-graph').addEventListener('click', () => detailView && showInGraph(detailView.id));

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
  reconcile($('activity-list'), list, {
    key: (job) => job.id,
    sig: (job) => JSON.stringify(job) + jobStatus(job),
    build: (job) => {
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
    },
  });
}

function showActivity(open) {
  activityOpen = open;
  $('activity-button').setAttribute('aria-pressed', String(open));
  $('activity-view').hidden = !open;
  $('chat-view').hidden = open;
  renderActivity();
  // Al cerrar la actividad se vuelve a la conversación, con el foco en su campo.
  if (!open) chat.show();
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
  // El agente creador-de-skills (u otro) escribió en .claude/skills/.
  listen('skills-changed', ({ payload }) => skills.announce(payload.added ?? []));
  listen('agents-changed', () => agents.load());
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
  const count = graphData.nodes.length;
  $('plane-open-count').textContent = count ? `${count} ${count === 1 ? 'nodo' : 'nodos'}` : '';
  if (!searchMenu.hidden && searchInput.value.trim()) {
    const current = searchHits[searchAt]?.id;
    searchHits = findNodes(searchInput.value);
    searchAt = Math.max(0, searchHits.findIndex((n) => n.id === current));
    renderSearch();
  }
}
let graphData = { nodes: [], edges: [] };

// ---------------------------------------------------------------- galaxia y skills
// En la vista Grafo el plano principal es la galaxia (con las skills alrededor) o el grafo,
// y se recuerda el último que eligió el usuario. La vista Agentes muestra el registro de agentes.
let plane = 'galaxia';
try {
  if (localStorage.getItem('nexo.plano') === 'grafo') plane = 'grafo';
} catch {}
let scene = ''; // lo que se ve en el centro: 'galaxia', 'grafo', 'escritorio' o 'registro'
let graphFramed = false;
const galaxy = createGalaxy($('galaxy-canvas'), { stage, onOpen: () => openGraph() });
// La luz de fondo sale del centro de la galaxia (el hueco entre las cartas), no del de la ventana.
const backdrop = document.querySelector('.backdrop');
galaxy.onFrame(({ cx, cy }) => {
  backdrop.style.setProperty('--glow-x', `${Math.round(cx)}px`);
  backdrop.style.setProperty('--glow-y', `${Math.round(cy)}px`);
});

// ---------------------------------------------------------------- configuración y tema
// Los lienzos no heredan el CSS: toman la paleta del tema al arrancar y en cada cambio. Lo que
// pinta la carta con el color de un nodo (su estrella, sus conexiones) se repinta también.
function paintTheme(theme) {
  graph.setPalette(theme.graph);
  galaxy.setPalette(theme.galaxy);
}
paintTheme(currentTheme());
onTheme((theme) => {
  paintTheme(theme);
  nodeShown = '';
  renderNode();
  if (!$('search-menu').hidden) renderSearch();
});
createSettings({ button: $('settings-button'), dialog: $('settings'), groups: $('theme-groups'), closeButton: $('settings-close') });

function renderScene() {
  const next = view === 'notas' ? 'escritorio' : view === 'agentes' ? (agentsMode === 'workspace' ? 'workspace' : 'registro') : plane;
  const before = scene;
  scene = next;
  document.body.dataset.scene = scene;
  $('stage').setAttribute(
    'aria-label',
    { escritorio: 'Escritorio de notas', grafo: 'Grafo de conocimiento', registro: 'Registro de agentes', workspace: 'Ventanas del workspace' }[scene] ?? 'Galaxia y skills',
  );
  syncGraphActive();
  if (before === next) return;
  if (next !== 'galaxia') skills.closePop();
  if (next === 'grafo') {
    if (before === 'galaxia') galaxy.open();
    else galaxy.spread = 1;
    // La galaxia se dispersa y el grafo crece desde el centro; la primera vez, ya encuadrado
    // (mientras estaba cerrado nadie lo encuadró).
    if (before) graph.reveal({ refit: !graphFramed, duration: before === 'galaxia' ? 1000 : 0 });
    graphFramed = true;
  } else if (next === 'galaxia') {
    // Al arrancar y al volver del grafo los puntos se recogen hasta formar la galaxia.
    if (before === 'grafo' || !before) {
      galaxy.spread = 1;
      galaxy.close(before ? 1100 : 1600);
    } else galaxy.spread = 0;
    requestAnimationFrame(() => skills.relayout());
  }
}

function setPlane(next) {
  plane = next === 'grafo' ? 'grafo' : 'galaxia';
  try {
    localStorage.setItem('nexo.plano', plane);
  } catch {}
  if (view === 'grafo') renderScene();
}

function openGraph() {
  setPlane('grafo');
  if (view !== 'grafo') setView('grafo');
}

// Navegar a un nodo concreto (búsqueda, «Ver en el grafo», enlaces) abre el grafo.
function showInGraph(id) {
  openGraph();
  return graph.focus(id);
}

$('plane-open').addEventListener('click', openGraph);
$('plane-galaxy').addEventListener('click', () => setPlane('galaxia'));

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// Le manda un mensaje a Claude como si lo escribiera el usuario: abre el chat (y a Claude, si
// hace falta) y lo envía.
async function sendToClaude(text) {
  if (!desktop) throw 'Claude solo funciona en la app de escritorio.';
  if (!vault) throw 'Primero elige un vault: Claude trabaja dentro de él.';
  if (minimized) setMinimized(false);
  if (activityOpen) showActivity(false);
  if (!(await chat.send(text))) throw 'No se pudo enviar el mensaje a Claude.';
}

const skills = createSkills({
  invoke: (cmd, args) => (desktop ? invoke(cmd, args) : Promise.reject('Las skills solo funcionan en la app de escritorio.')),
  ask: (message, options) => (desktop ? ask(message, options) : Promise.resolve(window.confirm(message))),
  fail,
  galaxy,
  stage,
  onUse: (skill) => sendToClaude(`/${commandFor(skill)}`).catch(fail),
  async onCreate(text) {
    if (!desktop) throw 'Las skills solo funcionan en la app de escritorio.';
    await invoke('skills_prepare');
    await sendToClaude(`Usa el agente creador-de-skills para crear una skill. Lo que pide el usuario: «${text}»`);
  },
  onShowAgents({ add = false } = {}) {
    setView('agentes');
    revealLeft();
    agents.showTab('skills');
    if (add) skills.openAdd();
  },
});

// Agentes de Claude Code del vault (.claude/agents/): resumen, modelo y esfuerzo de cada uno.
const agents = createAgents({
  invoke: (cmd, args) => (desktop ? invoke(cmd, args) : Promise.reject('Los agentes solo funcionan en la app de escritorio.')),
  ask: (message, options) => (desktop ? ask(message, options) : Promise.resolve(window.confirm(message))),
  fail,
  async onCreate({ text, model, effort }) {
    if (!desktop) throw 'Los agentes solo funcionan en la app de escritorio.';
    await invoke('skills_prepare');
    await sendToClaude(
      `Usa el agente creador-de-skills para crear un agente en .claude/agents/ con model: ${model} y effort: ${effort}. Lo que pide el usuario: «${text}»`,
    );
  },
});

// Registro de agentes (centro de la vista Agentes): tokens y parte de la sesión de 5 h de
// cada agente del vault, y su contexto como /context.
const agentLog = createAgentLog({
  invoke: (cmd, args) => (desktop ? invoke(cmd, args) : Promise.reject('El registro solo funciona en la app de escritorio.')),
  fail,
});

// ---------------------------------------------------------------- atención de los workspaces
// Un workspace cuyo orquestador o uno de sus agentes espera una respuesta (un permiso, una
// pregunta o un plan) lleva un punto aunque no se esté mirando: en el botón Agentes, en
// Recientes y en la lista de worktrees (ws-attention.js). El backend emite el evento global
// `workspace-attention` y, para no avisar al escritorio de lo que ya se ve, se le dice qué
// workspace hay en pantalla y si la ventana tiene el foco, solo cuando cambia.
const attention = createAttention();
const agentsButton = document.querySelector('.segmented.views [data-view="agentes"]');
const agentsDot = document.createElement('span');
agentsDot.className = 'views-attn';
agentsDot.setAttribute('aria-hidden', 'true');
agentsDot.hidden = true;
agentsButton.append(agentsDot);
let windowFocused = document.hasFocus();
const watch = createWatch((args) => invoke('workspace_watch', args).catch(() => {}));

function renderAttention() {
  const label = agentsButtonLabel(attention.count());
  agentsDot.hidden = !label;
  if (label) agentsButton.setAttribute('aria-label', label);
  else agentsButton.removeAttribute('aria-label');
  workspace?.attentionChanged();
}

function syncWatch() {
  if (!desktop) return;
  watch({ path: watchedPath({ view, mode: agentsMode, path: workspace?.current?.path }), focused: windowFocused });
}

if (desktop) {
  // El estado de todos se pide con el oyente ya puesto: así no se pierde un cambio entre los dos.
  listen('workspace-attention', ({ payload }) => {
    if (attention.set(payload)) renderAttention();
  })
    .then(() => invoke('workspace_attention'))
    .then((list) => attention.load(list) && renderAttention())
    .catch(() => {});
  // El perfil, el repertorio o los extras de un workspace cambiaron fuera de Extras (el agente
  // de nexo con la skill nexo:repertorio): lo vigila workspace.rs.
  listen('repertoire-changed', () => workspace?.repertoireChanged()).catch(() => {});
  listen('workspace-extras-changed', ({ payload }) => workspace?.extrasChanged(payload)).catch(() => {});
  window.addEventListener('focus', () => {
    windowFocused = true;
    syncWatch();
  });
  window.addEventListener('blur', () => {
    windowFocused = false;
    syncWatch();
  });
}

// ---------------------------------------------------------------- workspace
// La vista Agentes en modo workspace: una carpeta de proyecto con su orquestador y sus agentes
// (workspace.js). Nexo sigue en la carta derecha y también sale en la lista de agentes.
workspace = createWorkspace({
  invoke: (cmd, args) => (desktop ? invoke(cmd, args) : Promise.reject('Los workspaces solo funcionan en la app de escritorio.')),
  Channel,
  desktop,
  pickFolder: async (title) => (desktop ? open({ directory: true, multiple: false, title }) : null),
  pickFiles,
  ask: (message, options) => (desktop ? ask(message, options) : Promise.resolve(window.confirm(message))),
  fail,
  renderMarkdown,
  onLink: (link) => followLink(link, ''),
  nexo: {
    info: () => ({ state: chat.waiting ? 'waiting' : chat.busy ? 'active' : 'idle' }),
    focus() {
      if (minimized) setMinimized(false);
      if (activityOpen) showActivity(false);
      chat.focus();
    },
  },
  onMode: setAgentsMode,
  onChange: renderWorkspaceSummary,
  waiting: (path) => attention.waiting(path),
  attention: (path) => attention.get(path),
});

function setAgentsMode(mode) {
  const next = mode === 'workspace' ? 'workspace' : 'nexo';
  const changed = next !== agentsMode;
  agentsMode = next;
  try {
    localStorage.setItem('nexo.agentsMode', agentsMode);
  } catch {}
  if (view !== 'agentes') setView('agentes');
  else if (changed) setView('agentes');
  if (changed) {
    revealLeft();
    // El foco sigue al panel que aparece: el botón para volver o la entrada al workspace.
    requestAnimationFrame(() => (agentsMode === 'workspace' ? $('ws-back') : entryToFocus()).focus());
  }
}

// Sin ninguno en el panel, la entrada abre el menú de carpetas; con alguno, lo abren el «+» de
// la cabecera y la fila del final.
for (const id of ['ws-entry', 'ws-add', 'ws-add-row']) $(id).addEventListener('click', (event) => workspace.openDialog(event.currentTarget));

// Al volver al panel de nexo, el foco va al workspace actual (o a la entrada, sin ninguno).
const entryToFocus = () => $('ws-list').querySelector('.ws-entry[aria-current="true"]') ?? $('ws-list').querySelector('.ws-entry') ?? $('ws-entry');

const agentCount = (n) => `${n + 1} ${n ? 'agentes' : 'agente'}`;

// Los workspaces del panel (`open` de workspace.js, en su orden): cada uno con su × para quitarlo
// del panel, y el actual con su actividad. Sin el punto de «espera tu respuesta» (la entrada del
// panel no lo lleva, lo decidió el usuario). La firma no lleva lo que cambia a menudo: eso se
// pone en su sitio y la fila no se rehace (el foco no se pierde).
function renderWorkspaceEntries(ws, summary) {
  const panel = workspace?.list.open ?? [];
  const box = $('ws-list');
  $('ws-entry').hidden = panel.length > 0;
  $('ws-list-head').hidden = $('ws-add-row').hidden = panel.length === 0;
  reconcile(box, panel, {
    key: (w) => w.path,
    sig: (w) => [w.name, w.exists].join('|'),
    build: (w) => {
      const li = document.createElement('li');
      li.className = 'ws-entry-row';
      li.innerHTML = `<button type="button" class="ws-entry"><span class="ws-entry-icon" aria-hidden="true">${$('ws-entry').querySelector('.ws-entry-icon').innerHTML}</span><span class="ws-entry-text"><span class="ws-entry-name"><span class="ws-entry-name-text"></span></span><span class="ws-entry-meta"></span></span></button><button type="button" class="icon-btn ws-entry-remove" title="Quitar del panel (la carpeta no se toca)"><svg width="13" height="13" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="m5.5 5.5 9 9M14.5 5.5l-9 9"/></svg></button>`;
      li.querySelector('.ws-entry-name-text').textContent = w.name;
      const main = li.querySelector('.ws-entry');
      main.disabled = !w.exists;
      main.addEventListener('click', () => (workspace.current?.path === w.path ? setAgentsMode('workspace') : workspace.enter(w.path)));
      const remove = li.querySelector('.ws-entry-remove');
      remove.setAttribute('aria-label', `Quitar ${w.name} del panel`);
      remove.addEventListener('click', async () => {
        const at = [...box.children].indexOf(li);
        if (!(await workspace.remove(w.path))) return;
        const rows = [...box.children];
        (rows[Math.min(at, rows.length - 1)]?.querySelector('.ws-entry') ?? $('ws-entry')).focus();
      });
      return li;
    },
  });
  const agentsNow = summary?.agents ?? [];
  const now = summary?.main ? activity([summary.main, ...agentsNow]) : '';
  for (const li of box.children) {
    const w = panel.find((o) => o.path === li.dataset.key);
    if (!w) continue;
    const current = ws?.path === w.path;
    const main = li.querySelector('.ws-entry');
    const meta = !w.exists ? 'La carpeta ya no está' : current ? `${agentCount(agentsNow.length)}${now ? ` · ${now}` : ''}` : agentCount(w.agents ?? 0);
    li.querySelector('.ws-entry-meta').textContent = meta;
    const name = li.querySelector('.ws-entry-name');
    let badge = name.querySelector('.ws-badge');
    if (current && !badge) name.append(Object.assign(document.createElement('span'), { className: 'ws-badge', textContent: 'Actual' }));
    else if (!current) badge?.remove();
    if (current) main.setAttribute('aria-current', 'true');
    else main.removeAttribute('aria-current');
    main.title = w.path;
    main.setAttribute('aria-label', `Workspace ${w.name}${current ? ', el actual' : ''}: abrirlo. ${meta}`);
  }
}

// El panel de Agentes de nexo enseña también los workspaces: su lista y, con el nombre del
// actual, los subapartados de sus agentes (los que desplegó el orquestador) y de sus skills.
function renderWorkspaceSummary(ws, summary = workspace?.summary()) {
  // Abrir, cambiar o dejar un workspace cambia lo que hay en pantalla.
  syncWatch();
  renderWorkspaceEntries(ws, summary);
  const agentsNow = summary?.agents ?? [];

  const label = (id, text, count) => {
    const h = $(id);
    h.hidden = !ws;
    h.replaceChildren(document.createTextNode(text), Object.assign(document.createElement('span'), { className: 'ws-sub-count', textContent: ` · ${count}` }));
  };
  const agentsList = $('agents-ws-list');
  const skillsList = $('skills-ws-list');
  agentsList.hidden = skillsList.hidden = !ws;
  $('agents-label').textContent = ws ? 'Del vault' : 'Agentes';
  $('skills-label').textContent = ws ? 'Del vault' : 'Skills';
  if (!ws) {
    $('agents-ws-label').hidden = $('skills-ws-label').hidden = true;
    return;
  }
  const rows = [{ ...summary.main, key: 'main', badge: 'Principal' }, ...agentsNow];
  label('agents-ws-label', ws.name, rows.length);
  reconcile(agentsList, rows, {
    key: (a) => a.key,
    sig: (a) => [a.name, a.role, a.state, a.type, a.tokens ? formatTokens(a.tokens) : ''].join('|'),
    build: (a) => {
      const li = document.createElement('li');
      li.className = 'agent-row ws-sub-agent';
      li.innerHTML = '<button type="button" class="agent-main"><span class="row-icon"><span class="ws-state"></span></span><span class="agent-text"><span class="agent-name"><span class="agent-name-text"></span></span><span class="agent-specs"></span><span class="agent-desc"></span></span></button>';
      li.querySelector('.ws-state').dataset.state = a.state;
      li.querySelector('.agent-name-text').textContent = a.name;
      if (a.badge) li.querySelector('.agent-name').append(Object.assign(document.createElement('span'), { className: 'agent-badge', textContent: a.badge }));
      const specs = li.querySelector('.agent-specs');
      for (const text of [AGENT_STATE[a.state], a.type && a.key !== 'main' ? a.type : '', a.tokens ? `${formatTokens(a.tokens)} tokens` : ''].filter(Boolean)) {
        specs.append(Object.assign(document.createElement('span'), { className: 'agent-spec', textContent: text }));
      }
      li.querySelector('.agent-desc').textContent = a.role || '';
      const button = li.querySelector('.agent-main');
      button.title = `Abrir su ventana en el workspace ${ws.name}`;
      button.addEventListener('click', () => {
        setAgentsMode('workspace');
        if (a.key === 'main') workspace.openAgent(null);
        else workspace.openAgent(a.task);
      });
      return li;
    },
  });
  const wsSkills = summary.skills ?? [];
  label('skills-ws-label', ws.name, wsSkills.length);
  reconcile(skillsList, wsSkills.length ? wsSkills : [{ id: '', name: '', description: '' }], {
    key: (sk) => sk.id || 'vacío',
    sig: (sk) => [sk.name, sk.description].join('|'),
    build: (sk) => {
      const li = document.createElement('li');
      if (!sk.id) {
        li.className = 'ws-sub-empty';
        li.textContent = 'Sin skills propias: las de su carpeta .claude/skills/ saldrán aquí.';
        return li;
      }
      li.className = 'notes-row skill-row';
      li.innerHTML = '<div class="row-main"><span class="row-icon"><svg width="15" height="15" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="14" height="14" rx="4.5"/><path d="M10 6.5v7M6.5 10h7"/></svg></span><span class="row-text"><span class="row-title"></span><span class="row-meta"><span class="row-meta-text"></span></span></span></div>';
      li.querySelector('.row-title').textContent = sk.name;
      li.querySelector('.row-meta-text').textContent = sk.description;
      li.title = sk.description;
      return li;
    },
  });
}

// ---------------------------------------------------------------- vistas: Grafo / Notas
// Notas cambia la carta de la izquierda por temas y notas y el grafo por el escritorio
// de notas; el chat de la derecha no cambia.
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
  const wasNotes = view === 'notas';
  view = next === 'notas' || next === 'agentes' ? next : 'grafo';
  document.body.dataset.view = view;
  document.querySelectorAll('[data-view]').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.view === view)));
  const inNotes = view === 'notas';
  const inAgents = view === 'agentes';
  document.querySelector('#sources-panel > .panel-core.sources').hidden = inNotes || inAgents;
  $('notes-core').hidden = !inNotes;
  $('agents-core').hidden = !inAgents || agentsMode === 'workspace';
  $('ws-core').hidden = !inAgents || agentsMode !== 'workspace';
  // El lienzo de notas, su minimapa, el gestor de escritorios y el zoom solo existen en Notas.
  $('desk').hidden = !inNotes;
  $('desk-map').hidden = !inNotes;
  $('desk-manager').hidden = !inNotes;
  $('desk-zoom').hidden = !inNotes;
  try {
    localStorage.setItem('nexo.view', view);
  } catch {}
  // En Notas el lienzo es toda la app: el chat se minimiza solo y vuelve al salir, si era
  // la app quien lo había minimizado. Lo que el usuario elija a mano se respeta.
  if (inNotes && !wasNotes && !minimized) {
    setMinimized(true, { auto: true });
    chatAutoMin = true;
  } else if (!inNotes && wasNotes) {
    notes.pause();
    if (chatAutoMin && minimized) setMinimized(false, { auto: true });
    chatAutoMin = false;
  }
  // El riel de la izquierda lleva el nombre de la carta de esta vista.
  renderExpanded();
  renderScene();
  if (inNotes) {
    activateNotes();
    requestAnimationFrame(() => notes.relayout());
  }
}
// Solo se dibuja lo que se ve: con las cartas expandidas no se ve ninguno de los dos.
syncGraphActive = () => {
  const open = !expanded.left && !expanded.right;
  graph.setActive(scene === 'grafo' && open);
  galaxy.setActive(scene === 'galaxia' && open);
  agentLog.activate(scene === 'registro' && open);
  workspace?.setVisible(scene === 'workspace');
  syncWatch();
};
document.querySelectorAll('[data-view]').forEach((b) => b.addEventListener('click', () => setView(b.dataset.view)));
{
  let saved = 'grafo';
  try {
    saved = localStorage.getItem('nexo.view') ?? 'grafo';
  } catch {}
  setView(saved);
}

// Un nodo de wiki/notas/ → su nota en el escritorio (o su tema en la carta de notas).
async function openInNotes(id) {
  const rel = id.replace(/^notas\//, '');
  if (view !== 'notas') setView('notas');
  await notesActivation;
  if (/(^|\/)_index\.md$/.test(rel)) {
    revealLeft();
    return notes.openTopic(rel.replace(/\/?_index\.md$/, ''));
  }
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
  showInGraph(node.id);
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
    showInGraph(hit.id);
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
  // Mientras se escribe no se cambia de vista (solo al elegir un resultado); en la vista
  // Grafo, la galaxia deja paso al grafo para ver el nodo.
  if (searchHits.length) {
    if (view === 'grafo') setPlane('grafo');
    graph.focus(searchHits[0].id);
  }
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
// ⌘K / Ctrl+K abre la búsqueda.
const isMac = /Mac/i.test(navigator.platform);
$('search-kbd').textContent = isMac ? '⌘K' : 'Ctrl K';
document.addEventListener('keydown', (event) => {
  if ((isMac ? event.metaKey : event.ctrlKey) && event.key.toLowerCase() === 'k') {
    event.preventDefault();
    openSearch();
  }
});

// ---------------------------------------------------------------- arranque
async function refresh() {
  await Promise.all([loadSources(), loadGraph(), skills.load(), agents.load()]);
}

if (desktop) {
  getCurrentWebview().onDragDropEvent(({ payload }) => {
    const panel = $('sources-panel');
    if (payload.type === 'enter' || payload.type === 'over') {
      // Soltar archivos añade fuentes: con la carta minimizada no se vería dónde.
      if (payload.type === 'enter' && vault && view === 'grafo') revealLeft();
      panel.classList.toggle('dragging', Boolean(vault));
    }
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
      // El workspace que estaba abierto vuelve (no depende del vault).
      workspace.init();
      detected = await invoke('detect_vaults');
      // Primer arranque con un único vault de Obsidian: se usa directamente.
      if (!vault && detected.length === 1) return useVault(detected[0].path);
      showVault();
      // Cada vez que se abre nexo, el chat con Claude queda abierto con la suscripción.
      chat.start();
      await refresh();
      if (view === 'notas') activateNotes();
    } catch (error) {
      fail(error);
    }
  })();
} else {
  renderSources();
  loadGraph();
  skills.load();
  agents.load();
}
