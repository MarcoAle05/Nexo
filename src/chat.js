// Chat con Claude Code: la carta derecha (Nexo) y cada ventana de agente de un workspace. Claude
// Code corre sin terminal (claude_code.rs, workspace_run.rs) y aquí se pinta su conversación
// con el diseño de la app: lo que escribe el usuario, lo que contesta Claude, sus procesos
// (herramientas y subagentes) y los permisos que pide. Debajo, el compositor con el modelo, el
// esfuerzo, el modo de permisos, los comandos «/» y Enviar. El protocolo lo interpreta
// chat-model.js (sin DOM); esto es la vista y el cableado.
//
// Todas las piezas de un chat salen de la misma plantilla (`#chat-template`, `mountChat`) con
// sus ids bajo un prefijo («chat» en la carta de Nexo): así cada ventana tiene las suyas y las
// relaciones de ARIA (aria-controls, for…) siguen apuntando dentro de ella. Lo que habla con el
// proceso es un transporte: el chat de Nexo usa claude_code_*, el orquestador de un workspace
// workspace_*. Un subagente no tiene proceso propio: su chat va «alimentado» (`feed`) con las
// líneas que le reparte el workspace, y lo que escribe el usuario se lo pasa el orquestador.
import {
  EFFORT_HINTS,
  EFFORT_LABELS,
  MODE_HINTS,
  MODE_LABELS,
  allowLine,
  composeMessage,
  controlLine,
  createChatModel,
  denyLine,
  effortLevels,
  filterCommands,
  modelLabel,
  toolSummary,
  unsupportedLine,
  userLine,
} from './chat-model.js';
import { renderMarkdown } from './note-view.js';

const svg = (paths, size = 13, width = 1.6) =>
  `<svg width="${size}" height="${size}" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="${width}" stroke-linecap="round" stroke-linejoin="round">${paths}</svg>`;
const ICON = {
  check: svg('<path d="m4.5 10.5 3.5 3.5 7.5-8"/>'),
  chevron: svg('<path d="m5 8 5 5 5-5"/>', 12),
  close: svg('<path d="m5.5 5.5 9 9M14.5 5.5l-9 9"/>', 12),
  open: svg('<path d="M8 4H4v12h12v-4M11 4h5v5M16 4l-7 7"/>', 12),
  // Estados de un proceso: hecho, error, denegado o detenido, y a la espera de permiso.
  states:
    svg('<path d="m4.5 10.5 3.5 3.5 7.5-8"/>').replace('<svg', '<svg class="is-done"') +
    svg('<path d="M10 5.5v5.5M10 14.2v.3"/><circle cx="10" cy="10" r="7.5"/>').replace('<svg', '<svg class="is-error"') +
    svg('<circle cx="10" cy="10" r="7.5"/><path d="m5 15 10-10"/>').replace('<svg', '<svg class="is-denied"') +
    svg('<rect x="4.5" y="9" width="11" height="8" rx="2"/><path d="M7 9V6.5a3 3 0 0 1 6 0V9"/>').replace('<svg', '<svg class="is-waiting"'),
};

const STATUS_TEXT = {
  preparing: 'preparando',
  running: 'en curso',
  waiting: 'espera tu permiso',
  done: 'hecho',
  error: 'con error',
  denied: 'sin permiso',
  stopped: 'detenido',
};
const MODES = ['default', 'acceptEdits', 'plan', 'auto'];
const CONTROL_TIMEOUT = 20_000;
const AGENT_TOOLS = new Set(['Agent', 'Task']);

const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
};
const baseName = (path) => String(path).split(/[\\/]/).filter(Boolean).pop() ?? String(path);
const pretty = (value) => {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
};

// Preferencias del compositor (modelo, esfuerzo, permisos) guardadas en este equipo.
export const localPrefs = (key) => ({
  load() {
    try {
      return JSON.parse(localStorage.getItem(key) ?? '{}') ?? {};
    } catch {
      return {};
    }
  },
  save(prefs) {
    try {
      localStorage.setItem(key, JSON.stringify(prefs));
    } catch {}
  },
});

// Copia la plantilla del chat en `container` con sus ids bajo `prefix` (`chat-log` →
// `<prefix>-log`), también en las referencias de ARIA, y pone los textos de quien habla.
export function mountChat(container, prefix, { label = 'Conversación con Claude', placeholder = 'Escríbele a Claude…', inputLabel = 'Mensaje para Claude' } = {}) {
  const template = document.getElementById('chat-template');
  const fragment = template.content.cloneNode(true);
  const rename = (value) => value.replace(/(^|\s)chat-/g, `$1${prefix}-`);
  for (const node of fragment.querySelectorAll('[id], [for], [aria-controls], [aria-describedby], [aria-labelledby]')) {
    for (const attr of ['id', 'for', 'aria-controls', 'aria-describedby', 'aria-labelledby']) {
      if (node.hasAttribute(attr)) node.setAttribute(attr, rename(node.getAttribute(attr)));
    }
  }
  container.replaceChildren(fragment);
  container.querySelector(`#${prefix}-list`)?.setAttribute('aria-label', label);
  const input = container.querySelector(`#${prefix}-input`);
  input?.setAttribute('placeholder', placeholder);
  const inputName = container.querySelector(`label[for="${prefix}-input"]`);
  if (inputName) inputName.textContent = inputLabel;
  return container;
}

// Lo que una herramienta manda, en claro: la orden, el archivo o su entrada tal cual.
function inputText(item) {
  const input = item.input ?? {};
  if (item.name === 'Bash') return input.command ?? '';
  if (item.name === 'Write') return input.content ?? '';
  if (item.name === 'Edit') return `− ${input.old_string ?? ''}\n+ ${input.new_string ?? ''}`;
  if (item.name === 'TodoWrite' || item.name === 'AskUserQuestion' || item.name === 'ExitPlanMode') return '';
  const keys = Object.keys(input);
  if (!keys.length) return item.partial ?? '';
  return pretty(input);
}

const EMPTY = {
  browser: ['Solo en la app de escritorio', 'El chat con Claude necesita la app de nexo: aquí solo se ve la interfaz.'],
  'no-vault': ['Elige un vault', 'Claude se abrirá dentro de él y trabajará con tus fuentes y tu wiki.'],
  idle: ['Claude', 'El chat se abre con tu suscripción de Claude Code.'],
  starting: ['Abriendo Claude…', 'Arrancando Claude Code con tu suscripción.'],
  ready: ['Claude está listo', 'Pregunta por tus fuentes y tu wiki, o escribe / para ver los comandos y las skills.'],
  exited: ['Claude Code se cerró', 'Puedes abrirlo de nuevo y seguir con la misma conversación.'],
  error: ['No se pudo abrir Claude', ''],
};

export function createChat({
  root = document,
  prefix = 'chat',
  transport,
  desktop,
  // Por qué no se puede abrir todavía (`'no-vault'`, `'browser'`…) o null si se puede.
  blocked = () => null,
  feed = false,
  // Un chat alimentado: qué hacer con lo que escribe el usuario (se lo pasa el orquestador).
  submitUser = null,
  onInterrupt = null,
  prefs: prefsStore = localPrefs('nexo.chat'),
  getCwd = () => '',
  pickFiles,
  onLink = () => {},
  localCommands = () => [],
  onState = () => {},
  onStarted = () => {},
  // Procesos de la herramienta Agent: el agente que desplegó (para abrir su ventana).
  agentFor = null,
  onOpenAgent = () => {},
  speaker = 'Claude',
  texts = {},
  model = createChatModel(),
}) {
  const $ = (name) => (root === document ? document.getElementById(`${prefix}-${name}`) : root.querySelector(`#${CSS.escape(prefix)}-${name}`));
  const log = $('log');
  const list = $('list');
  const input = $('input');
  const bar = $('bar');
  const commandsBox = $('commands');
  const form = $('form');
  const empty = { ...EMPTY, ...texts };
  if (feed) root.querySelector?.('.chat-view')?.setAttribute('data-feed', '');

  const nodes = new Map(); // pieza → su <li>
  // Las piezas lejos del final llevan `chat-far`: solo se maquetan y se pintan cuando se ven
  // (styles.css). Lo de cerca del final no (FAR_PX o tres alturas de la conversación, lo que
  // sea más): con la conversación pegada al final, nada de lo que se ve cambia de alto al
  // aparecer y bajar hasta el final cae siempre en su sitio. Una pieza se mira desde el
  // fotograma siguiente a pintarse, cuando ya recuerda su alto (si no, mediría 160 px). `far` es
  // la primera que no la lleva y `seen`, cuántas había en la pasada anterior.
  const FAR_PX = 2400;
  let far = 0;
  let seen = 0;
  let farFrame = 0;
  const waiting = new Map(); // id de petición de control → { resolve, reject, timer }
  let phase = feed ? 'ready' : desktop ? 'idle' : 'browser';
  let phaseDetail = '';
  let running = feed;
  let starting = null;
  let fresh = true; // esta página aún no ha pedido la conversación guardada
  let stick = true; // la conversación sigue al final mientras no se suba a mano
  let frame = 0;
  let files = [];
  let requestSeq = 0;
  let prefs = prefsStore.load() ?? {};
  const savePrefs = () => prefsStore.save(prefs);
  const send$ = (line, record) => transport.send(line, record);

  // ---------------------------------------------------------------- anuncios
  // La región de estado existe vacía desde el principio; se vacía y se rellena en dos pasos
  // para que el lector de pantalla repita un texto aunque sea el mismo.
  function announce(text, urgent = false) {
    const region = $(urgent ? 'alert' : 'live');
    region.textContent = '';
    requestAnimationFrame(() => (region.textContent = text));
  }
  const plain = (markdown) => {
    const box = el('div');
    box.innerHTML = renderMarkdown(markdown);
    return (box.textContent ?? '').replace(/\s+/g, ' ').trim();
  };

  // ---------------------------------------------------------------- pintar piezas
  function paintUser(node, item) {
    const who = item.from ? `${item.from}: ` : 'Tú: ';
    const parts = [el('span', 'visually-hidden', who)];
    if (item.from || item.toName) node.classList.add('chat-user-tagged');
    if (item.from) parts.push(el('span', 'chat-user-tag', item.from));
    else if (item.toName) parts.push(el('span', 'chat-user-tag', `Para ${item.toName}`));
    parts.push(el('div', 'chat-bubble', item.text || 'Archivos adjuntos'));
    node.replaceChildren(...parts);
    if (item.files?.length) {
      const attached = el('ul', 'chat-user-files');
      attached.setAttribute('aria-label', 'Archivos adjuntos');
      for (const file of item.files) {
        const li = el('li', 'chat-file');
        li.title = file;
        li.append(el('span', 'chat-file-name', baseName(file)));
        attached.append(li);
      }
      node.append(attached);
    }
  }

  function paintText(node, item) {
    node.classList.toggle('streaming', Boolean(item.streaming));
    let body = node.querySelector('.chat-md');
    if (!body) {
      body = el('div', 'note-body chat-md');
      node.replaceChildren(el('span', 'visually-hidden', `${item.speaker ?? speaker}: `), body);
    }
    body.innerHTML = renderMarkdown(item.text);
  }

  function paintThinking(node, item) {
    const tokens = item.tokens ? ` · ~${item.tokens} tokens` : '';
    const label = item.streaming ? `Pensando…${tokens}` : `Razonó${tokens}`;
    node.hidden = !item.streaming && !item.text && !item.tokens;
    if (!item.text) return node.replaceChildren(el('span', '', label));
    let details = node.querySelector('details');
    if (!details) {
      details = el('details');
      details.append(el('summary'), el('p', 'chat-think-text'));
      node.replaceChildren(details);
    }
    details.firstChild.textContent = label;
    details.lastChild.textContent = item.text;
  }

  function todoList(todos) {
    const ul = el('ul', 'chat-todos');
    for (const todo of todos) {
      const li = el('li');
      li.dataset.state = todo.status ?? 'pending';
      const mark = { completed: '✓', in_progress: '▸' }[todo.status] ?? '◻';
      li.append(el('span', '', mark), el('span', '', (todo.status === 'in_progress' && todo.activeForm) || todo.content || ''));
      const state = { completed: 'hecha', in_progress: 'en curso' }[todo.status] ?? 'pendiente';
      li.append(el('span', 'visually-hidden', `, ${state}`));
      ul.append(li);
    }
    return ul;
  }

  // Lo que hizo un subagente, dentro de su proceso: texto y una línea por herramienta.
  function childNode(child, cwd) {
    const li = el('li', `chat-child chat-child-${child.kind}`);
    if (child.kind === 'text') {
      const body = el('div', 'note-body chat-md');
      body.innerHTML = renderMarkdown(child.text);
      li.append(body);
    } else if (child.kind === 'tool') {
      const { label, detail } = toolSummary(child.name, child.input, cwd);
      li.className = 'chat-child chat-proc-line';
      li.append(el('span', 'chat-proc-label', label), document.createTextNode(' '), el('span', 'chat-proc-detail', detail), el('span', 'visually-hidden', `, ${STATUS_TEXT[child.status] ?? ''}`));
    } else if (child.kind === 'notice') li.textContent = child.text;
    else return null;
    return li;
  }

  function procBody(item, cwd) {
    const parts = [];
    if (item.name === 'TodoWrite' && Array.isArray(item.input?.todos)) parts.push(todoList(item.input.todos));
    if (item.children.length) {
      const children = el('ol', 'chat-children');
      children.setAttribute('aria-label', 'Lo que hizo el agente');
      for (const child of item.children) {
        const node = childNode(child, cwd);
        if (node) children.append(node);
      }
      if (children.children.length) parts.push(children);
    }
    const sent = inputText(item);
    if (sent) parts.push(el('p', 'chat-io-label', item.name === 'Bash' ? 'Orden' : 'Entrada'), el('pre', 'chat-io', sent));
    // El resultado de la lista de tareas es solo un «hecho»: no aporta nada a la vista.
    if (item.result && (item.name !== 'TodoWrite' || item.status === 'error')) parts.push(el('p', 'chat-io-label', 'Resultado'), el('pre', 'chat-io', item.result));
    return parts;
  }

  // Un proceso de la herramienta Agent lleva, debajo, el botón que abre la ventana del agente.
  function agentButton(node, item) {
    const agent = AGENT_TOOLS.has(item.name) && agentFor ? agentFor(item.id) : null;
    let button = node.querySelector(':scope > .chat-proc-open');
    if (!agent) return button?.remove();
    if (!button) {
      button = el('button', 'chat-proc-open');
      button.type = 'button';
      button.addEventListener('click', () => onOpenAgent(agentFor(item.id)));
      node.querySelector(':scope > details').after(button);
    }
    button.innerHTML = `${ICON.open}<span></span>`;
    button.lastChild.textContent = `Abrir la ventana de ${agent.name}`;
  }

  function paintTool(node, item) {
    const cwd = model.state.session?.cwd ?? getCwd() ?? '';
    const { label, detail } = toolSummary(item.name, item.input, cwd);
    node.dataset.status = item.status;
    let details = node.querySelector(':scope > details');
    if (!details) {
      details = el('details', 'chat-proc');
      const summary = el('summary');
      const state = el('span', 'chat-proc-state');
      state.setAttribute('aria-hidden', 'true');
      state.innerHTML = ICON.states;
      const chevron = el('span', 'chat-proc-chevron');
      chevron.setAttribute('aria-hidden', 'true');
      chevron.innerHTML = ICON.chevron;
      const text = el('span', 'chat-proc-detail');
      text.append(el('span'), el('span', 'visually-hidden'));
      summary.append(state, el('span', 'chat-proc-label'), text, chevron);
      details.append(summary, el('div', 'chat-proc-body'));
      node.replaceChildren(details);
    }
    const [, labelNode, detailNode] = details.firstChild.children;
    labelNode.textContent = label;
    detailNode.firstChild.textContent = detail;
    detailNode.lastChild.textContent = `, ${STATUS_TEXT[item.status] ?? item.status}`;
    // El cuerpo solo se rehace si cambió algo que se ve dentro.
    const sig = [item.status, item.result?.length ?? 0, item.children.length, item.children.map((c) => `${c.status ?? ''}${c.text?.length ?? ''}`).join(), item.final ? pretty(item.input).length : item.partial.length].join('|');
    const body = details.lastChild;
    if (body.dataset.sig !== sig) {
      body.dataset.sig = sig;
      body.replaceChildren(...procBody(item, cwd));
    }
    // Una lista de tareas se ve sin tener que abrirla.
    if (item.name === 'TodoWrite' && !details.dataset.opened) {
      details.open = true;
      details.dataset.opened = '1';
    }
    agentButton(node, item);
    const pending = item.permission && !item.permission.answer ? item.permission : null;
    const card = node.querySelector(':scope > .chat-perm');
    if (!pending) card?.remove();
    else if (card?.dataset.request !== pending.requestId) {
      card?.remove();
      node.append(permissionCard(item, label, detail));
    }
  }

  // ---------------------------------------------------------------- permisos
  async function answerPermission(item, decision, { input: updated, message } = {}) {
    const { requestId, suggestions } = item.permission;
    const line =
      decision === 'deny' ? denyLine(requestId, message) : allowLine(requestId, updated ?? item.input, decision === 'always' ? suggestions : null);
    const record = JSON.stringify({ type: 'nexo_permission', request_id: requestId, decision: decision === 'deny' ? 'deny' : 'allow' });
    try {
      await send$(line, record);
    } catch (error) {
      return notice(error, 'error');
    }
    model.answer(requestId, decision === 'deny' ? 'deny' : 'allow');
    queueRender();
    // Los botones desaparecen con la tarjeta: el foco vuelve al campo, no se pierde.
    input.focus();
  }

  const permButton = (label, className, onClick, hint) => {
    const button = el('button', `chat-perm-btn${className ? ` ${className}` : ''}`, label);
    button.type = 'button';
    if (hint) button.append(el('small', '', hint));
    button.addEventListener('click', onClick);
    return button;
  };

  // Qué pasa al pulsar «Permitir siempre», según lo que propone Claude Code.
  function alwaysHint(suggestions) {
    if (suggestions.some((s) => s.type === 'setMode' && s.mode === 'acceptEdits')) return 'y aceptar las ediciones en esta sesión';
    if (suggestions.some((s) => s.type === 'addDirectories')) return 'y dar acceso a esa carpeta';
    return 'y no volver a preguntar por esto';
  }

  function permissionCard(item, label, detail) {
    const { requestId, suggestions } = item.permission;
    const card = el('div', 'chat-perm');
    card.dataset.request = requestId;
    card.setAttribute('role', 'group');
    const actions = el('div', 'chat-perm-actions');

    if (item.name === 'AskUserQuestion' && Array.isArray(item.input?.questions)) {
      card.setAttribute('aria-label', `${speaker} te pregunta`);
      card.append(el('p', 'chat-perm-title', `${speaker} te pregunta`));
      const picked = new Map();
      const groups = item.input.questions.map((question) => {
        const group = el('fieldset', 'chat-question');
        group.append(el('legend', '', question.question ?? ''));
        const options = el('div', 'chat-perm-actions');
        for (const option of question.options ?? []) {
          const button = permButton(option.label ?? '', '', () => {
            const chosen = picked.get(question.question) ?? new Set();
            if (!question.multiSelect) {
              chosen.clear();
              for (const other of options.children) other.setAttribute('aria-pressed', 'false');
            }
            const on = !chosen.has(option.label) || !question.multiSelect;
            if (on) chosen.add(option.label);
            else chosen.delete(option.label);
            button.setAttribute('aria-pressed', String(on));
            picked.set(question.question, chosen);
          }, option.description);
          button.setAttribute('aria-pressed', 'false');
          options.append(button);
        }
        group.append(options);
        return group;
      });
      card.append(...groups);
      actions.append(
        permButton('Responder', 'primary', () => {
          const missing = item.input.questions.findIndex((q) => !(picked.get(q.question)?.size > 0));
          if (missing >= 0) {
            announce(`Falta elegir una opción en: ${item.input.questions[missing].question}`, true);
            return groups[missing].querySelector('button')?.focus();
          }
          const answers = Object.fromEntries(item.input.questions.map((q) => [q.question, [...picked.get(q.question)].join(', ')]));
          answerPermission(item, 'allow', { input: { ...item.input, answers } });
        }),
        permButton('No responder', '', () => answerPermission(item, 'deny', { message: 'El usuario prefirió no contestar.' })),
      );
    } else if (item.name === 'ExitPlanMode') {
      card.setAttribute('aria-label', `Plan de ${speaker}`);
      card.append(el('p', 'chat-perm-title', 'Plan para revisar'));
      const plan = el('div', 'note-body chat-md chat-perm-plan');
      plan.tabIndex = 0;
      plan.setAttribute('aria-label', 'Plan');
      plan.innerHTML = renderMarkdown(item.input?.plan ?? '');
      card.append(plan);
      actions.append(
        permButton('Aprobar el plan', 'primary', () => answerPermission(item, 'allow')),
        permButton('Seguir planeando', '', () => answerPermission(item, 'deny', { message: 'El usuario quiere seguir planeando antes de ejecutar.' })),
      );
    } else {
      card.setAttribute('aria-label', `Permiso para ${label} ${detail}`.trim());
      card.append(el('p', 'chat-perm-title', `${speaker} pide permiso`));
      const text = el('p', 'chat-perm-text', `${label} `);
      if (detail) text.append(el('code', '', detail));
      card.append(text);
      if (item.name === 'Bash' && item.input?.command) card.append(el('pre', 'chat-io', item.input.command));
      actions.append(permButton('Permitir', 'primary', () => answerPermission(item, 'allow')));
      if (suggestions.length) actions.append(permButton('Permitir siempre', '', () => answerPermission(item, 'always'), alwaysHint(suggestions)));
      actions.append(permButton('Rechazar', '', () => answerPermission(item, 'deny')));
    }
    card.append(actions);
    return card;
  }

  // ---------------------------------------------------------------- lista
  function paint(item) {
    let node = nodes.get(item);
    if (!node) {
      node = el('li', `chat-item chat-${item.kind === 'thinking' ? 'think' : item.kind} enter`);
      node.addEventListener('animationend', () => node.classList.remove('enter'), { once: true });
      nodes.set(item, node);
      list.append(node);
    }
    if (item.kind === 'user') paintUser(node, item);
    else if (item.kind === 'text') paintText(node, item);
    else if (item.kind === 'thinking') paintThinking(node, item);
    else if (item.kind === 'tool') paintTool(node, item);
    else if (item.kind === 'notice') {
      node.dataset.level = item.level;
      node.textContent = item.text;
    }
  }

  function renderChrome() {
    const { state } = model;
    const none = state.items.length === 0;
    $('empty').hidden = !none;
    const [title, text] = empty[phase] ?? empty.ready;
    $('empty-title').textContent = title;
    $('empty-text').textContent = phase === 'error' ? phaseDetail : text;
    $('retry').hidden = feed || !(phase === 'error' || phase === 'exited');
    $('banner').hidden = feed || none || !(phase === 'exited' || phase === 'error');
    $('banner-text').textContent = phase === 'error' ? `No se pudo abrir Claude: ${phaseDetail}` : `Claude Code se cerró.${phaseDetail ? ` ${phaseDetail}` : ''}`;

    const asking = model.pending().length > 0;
    const writing = state.items.at(-1)?.streaming && state.items.at(-1)?.kind === 'text';
    const working = state.busy && !asking && !writing;
    $('working').hidden = !working;
    $('working-text').textContent =
      state.status === 'compacting' ? 'Compactando el contexto…' : state.status === 'retry' ? 'Reintentando la conexión…' : `${speaker} está trabajando…`;
    list.setAttribute('aria-busy', String(state.busy));

    const stop = state.busy && !input.value.trim() && !files.length && (!feed || onInterrupt);
    $('send').dataset.state = stop ? 'stop' : 'send';
    $('send-label').textContent = stop ? 'Detener' : 'Enviar';

    const plan = $('plan');
    if (plan) {
      plan.hidden = feed || !state.account?.subscription;
      plan.textContent = state.account?.subscription ?? '';
    }

    if (!feed) {
      const applied = state.applied ?? {};
      $('model-label').textContent = modelLabel(state.models, applied.model ?? state.session?.model, prefs.model);
      const effort = applied.effort ?? prefs.effort;
      $('effort-label').textContent = EFFORT_LABELS[effort] ?? 'Esfuerzo';
      $('effort').disabled = effortLevels(state.models, applied.model, prefs.model).length === 0;
      $('access-label').textContent = MODE_LABELS[state.permissionMode] ?? state.permissionMode;
    }
    fitBar();
    onState({ busy: state.busy, running, phase, waiting: asking });
  }

  // «Enviar» pierde su texto (no su nombre) cuando la barra no cabe a lo ancho de la carta.
  function fitBar() {
    if (!bar.clientWidth) return;
    // Alimentado, la barra va en la fila del campo: Enviar pierde el texto si el compositor
    // es estrecho (el campo se queda con lo que sobra).
    if (feed) return bar.classList.toggle('compact', form.clientWidth < 340);
    bar.classList.remove('compact');
    // Con `measure` nada encoge: cada pieza da el ancho que querría.
    bar.classList.add('measure');
    const shown = [...bar.children].filter((child) => child.offsetParent !== null);
    const pieces = shown.filter((child) => !child.classList.contains('chat-bar-space'));
    const style = getComputedStyle(bar);
    const gaps = parseFloat(style.columnGap) * Math.max(0, shown.length - 1);
    const padding = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
    const needed = pieces.reduce((sum, child) => sum + child.getBoundingClientRect().width + parseFloat(getComputedStyle(child).marginLeft), 0) + gaps + padding;
    bar.classList.remove('measure');
    bar.classList.toggle('compact', needed > bar.clientWidth + 0.5);
  }

  // El nombre de quien habla (Claude, o Nexo por la API) encabeza lo primero que se ve de él
  // tras cada mensaje del usuario o cuando cambia.
  function markFirsts() {
    let last = null;
    for (const item of model.state.items) {
      const node = nodes.get(item);
      if (!node) continue;
      if (item.kind === 'user' || item.kind === 'notice') {
        last = null;
        continue;
      }
      if (node.hidden) continue;
      const who = item.speaker ?? speaker;
      node.dataset.speaker = who;
      node.classList.toggle('chat-first', who !== last);
      last = who;
    }
  }

  function markFar() {
    const { items } = model.state;
    const limit = Math.min(seen, items.length);
    const fresh = items.length > seen;
    seen = items.length;
    if (far < limit) {
      // Primero se mide y luego se marca: una pieza lejana deja de maquetarse y movería las demás.
      const end = list.offsetTop + list.offsetHeight - Math.max(FAR_PX, 3 * log.clientHeight);
      let next = far;
      while (next < limit) {
        const node = nodes.get(items[next]);
        if (node && node.offsetTop + node.offsetHeight > end) break;
        next++;
      }
      for (; far < next; far++) nodes.get(items[far])?.classList.add('chat-far');
    }
    if (fresh && !farFrame) {
      farFrame = requestAnimationFrame(() => {
        farFrame = 0;
        markFar();
        if (stick) log.scrollTop = log.scrollHeight;
      });
    }
  }

  function render() {
    frame = 0;
    for (const item of model.flush()) paint(item);
    markFirsts();
    markFar();
    renderChrome();
    if (stick) log.scrollTop = log.scrollHeight;
    $('jump').hidden = stick || log.scrollHeight - log.scrollTop - log.clientHeight < 80;
  }
  function queueRender() {
    if (!frame) frame = requestAnimationFrame(render);
  }

  function notice(text, level = 'info') {
    model.notice(String(text), level);
    if (level === 'error') announce(`Error: ${text}`, true);
    queueRender();
  }

  function setPhase(next, detail = '') {
    phase = next;
    phaseDetail = detail;
    queueRender();
  }

  // ---------------------------------------------------------------- proceso y protocolo
  function onEvent(event) {
    if (event.kind === 'exit') {
      running = false;
      model.exit();
      for (const [, pending] of waiting) pending.reject('Claude Code se cerró.');
      waiting.clear();
      const detail = (event.detail ?? '').split('\n').filter((l) => l.trim()).pop() ?? '';
      if (feed) {
        queueRender();
        return;
      }
      setPhase('exited', detail.slice(0, 300));
      announce(`Claude Code se cerró.${detail ? ` ${detail}` : ''}`, true);
      return;
    }
    let msg = event.msg;
    if (!msg) {
      try {
        msg = JSON.parse(event.text);
      } catch {
        return;
      }
    }
    const effect = model.apply(msg);
    if (effect?.unsupported && !feed) send$(unsupportedLine(effect.unsupported, effect.subtype), null).catch(() => {});
    if (msg.type === 'control_response') {
      const id = msg.response?.request_id;
      const pending = waiting.get(id);
      if (pending) {
        waiting.delete(id);
        clearTimeout(pending.timer);
        if (msg.response.subtype === 'success') pending.resolve(msg.response.response ?? null);
        else pending.reject(msg.response.error ?? 'Claude Code rechazó la petición.');
      }
    } else if (!running || event.history) {
      // Es la conversación guardada, que se repite al conectar: nada que anunciar.
    } else if (msg.type === 'control_request' && msg.request?.subtype === 'can_use_tool') {
      const { label, detail } = toolSummary(msg.request.tool_name, msg.request.input, model.state.session?.cwd ?? getCwd());
      const what = msg.request.tool_name === 'AskUserQuestion' ? `${speaker} te hace una pregunta` : `${speaker} pide permiso para ${label} ${detail}`.trim();
      announce(`${what}. Los botones están en la conversación, antes del campo de mensaje.`);
    } else if (msg.type === 'result') {
      const last = model.state.items.findLast((i) => i.kind === 'text');
      announce(msg.is_error ? 'El turno terminó con un error.' : `${speaker} terminó. ${last ? plain(last.text).slice(0, 280) : ''}`);
    }
    queueRender();
  }

  // Petición de control (inicializar, cambiar de modelo…): se resuelve con su respuesta.
  function control(request) {
    const id = `nexo-${Date.now().toString(36)}-${++requestSeq}`;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        waiting.delete(id);
        reject('Claude Code no contestó a tiempo.');
      }, CONTROL_TIMEOUT);
      waiting.set(id, { resolve, reject, timer });
      send$(controlLine(id, request), null).catch((error) => {
        waiting.delete(id);
        clearTimeout(timer);
        reject(error);
      });
    });
  }

  function start() {
    if (feed) return Promise.resolve(true);
    if (starting) return starting;
    if (running) return Promise.resolve(true);
    const why = !desktop ? 'browser' : blocked();
    if (why) {
      setPhase(why);
      return Promise.resolve(false);
    }
    setPhase('starting');
    $('alert').textContent = '';
    starting = (async () => {
      try {
        const options = { model: prefs.model ?? null, effort: prefs.effort ?? null, permissionMode: prefs.mode ?? null };
        const info = await transport.start({ options, replay: fresh, onEvent });
        fresh = false;
        running = true;
        setPhase('ready');
        onStarted(info);
        // Sin gastar nada: qué modelos y comandos hay y qué está vigente.
        if (!info.attached) await control({ subtype: 'initialize' }).catch((error) => notice(error, 'error'));
        await control({ subtype: 'get_settings' }).catch(() => {});
        queueRender();
        return true;
      } catch (error) {
        running = false;
        setPhase('error', String(error));
        announce(`No se pudo abrir Claude: ${error}`, true);
        return false;
      } finally {
        starting = null;
      }
    })();
    return starting;
  }

  async function send(text, attached = []) {
    const body = composeMessage(text ?? '', attached);
    if (!body) return false;
    const shown = String(text ?? '').trim();
    if (feed) {
      // Al agente se lo pasa el orquestador; aquí se ve como lo escribió el usuario.
      if (!(await submitUser(text ?? '', attached))) return false;
      model.addUser(shown, attached, { local: true });
      stick = true;
      queueRender();
      return true;
    }
    if (!running && !(await start())) return false;
    // El mensaje sale en la conversación antes de mandarlo: lo que conteste Claude va detrás.
    model.addUser(shown, attached);
    stick = true;
    queueRender();
    try {
      await send$(userLine(body), JSON.stringify({ type: 'nexo_user', text: shown, files: attached }));
    } catch (error) {
      model.state.busy = false;
      notice(`No se pudo enviar: ${error}`, 'error');
      return false;
    }
    return true;
  }

  // Un mensaje que no escribió el usuario en este chat (lo que escribe en la ventana de un
  // agente): va a Claude Code tal cual y se pinta con `shown` (y a quién iba).
  async function relay(body, record) {
    if (!running && !(await start())) return false;
    model.addUser(record.text, record.files ?? [], { to: record.to, toName: record.toName });
    stick = true;
    queueRender();
    try {
      await send$(userLine(body), JSON.stringify({ type: 'nexo_user', ...record }));
    } catch (error) {
      model.state.busy = false;
      notice(`No se pudo enviar: ${error}`, 'error');
      return false;
    }
    return true;
  }

  const interrupt = () => (feed ? onInterrupt?.() : control({ subtype: 'interrupt' }).catch((error) => notice(error, 'error')));

  function clearView() {
    model.clear();
    nodes.clear();
    far = 0;
    seen = 0;
    list.replaceChildren();
  }

  // Conversación nueva: sesión nueva de Claude Code y la lista vacía.
  async function reset() {
    if (feed) return;
    try {
      await transport.reset();
    } catch (error) {
      return notice(error, 'error');
    }
    running = false;
    clearView();
    setFiles([]);
    setPhase('idle');
    announce('Conversación nueva.');
    await start();
    input.focus();
  }

  // ---------------------------------------------------------------- selectores
  const menus = [];
  const closeMenus = (except) => menus.forEach((m) => m !== except && m.hide(false));

  function menuItem({ value, label, hint, tag, checked }) {
    const item = el('button', 'chat-menu-item');
    item.type = 'button';
    item.tabIndex = -1;
    item.dataset.value = value;
    item.setAttribute('role', 'menuitemradio');
    item.setAttribute('aria-checked', String(Boolean(checked)));
    const check = el('span', 'chat-menu-check');
    check.setAttribute('aria-hidden', 'true');
    check.innerHTML = ICON.check;
    item.append(check, el('span', 'chat-menu-name', label));
    if (tag) item.append(el('span', 'chat-menu-tag', tag));
    if (hint) item.append(el('span', 'chat-menu-hint', hint));
    return item;
  }

  // Botón con menú (patrón de la APG): Intro, Espacio o las flechas lo abren; las flechas
  // recorren las opciones; Escape lo cierra y devuelve el foco al botón.
  function createMenu(button, panel, items, pick) {
    let open = false;
    const options = () => [...panel.querySelectorAll('[role="menuitemradio"]')];
    const self = {
      hide(refocus) {
        if (!open) return;
        open = false;
        panel.hidden = true;
        button.setAttribute('aria-expanded', 'false');
        if (refocus) button.focus();
      },
    };
    function show(last = false) {
      closeMenus(self);
      closeCommands();
      const built = items();
      panel.replaceChildren(...(built.length ? built.map(menuItem) : [el('p', 'chat-menu-empty', 'Claude Code todavía no ha dicho qué opciones hay.')]));
      panel.hidden = false;
      button.setAttribute('aria-expanded', 'true');
      open = true;
      const all = options();
      (all.find((o) => o.getAttribute('aria-checked') === 'true') ?? (last ? all.at(-1) : all[0]))?.focus();
    }
    button.addEventListener('click', () => (open ? self.hide(true) : show()));
    button.addEventListener('keydown', (event) => {
      if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
      event.preventDefault();
      show(event.key === 'ArrowUp');
    });
    panel.addEventListener('keydown', (event) => {
      const all = options();
      const at = all.indexOf(document.activeElement);
      const go = (index) => {
        event.preventDefault();
        all[(index + all.length) % all.length]?.focus();
      };
      if (event.key === 'ArrowDown') go(at + 1);
      else if (event.key === 'ArrowUp') go(at - 1);
      else if (event.key === 'Home') go(0);
      else if (event.key === 'End') go(all.length - 1);
      else if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        self.hide(true);
      } else if (event.key === 'Tab') self.hide(false);
    });
    panel.addEventListener('click', (event) => {
      const item = event.target.closest('[data-value]');
      if (!item) return;
      self.hide(true);
      pick(item.dataset.value);
    });
    menus.push(self);
    return self;
  }

  // Un cambio se aplica a la sesión en marcha y se recuerda para las siguientes.
  async function apply(request, remember) {
    remember();
    savePrefs();
    if (running) {
      try {
        await control(request);
        await control({ subtype: 'get_settings' }).catch(() => {});
      } catch (error) {
        notice(error, 'error');
      }
    }
    queueRender();
  }

  if (!feed) {
    createMenu(
      $('model'),
      $('model-menu'),
      () => {
        const { models, applied, session } = model.state;
        const current = applied?.model ?? session?.model;
        // Marcado: lo elegido aquí si sigue vigente; si no, el modelo que Claude Code dice tener.
        const picked = models.find((m) => m.value === (prefs.model ?? 'default'));
        const chosen = picked && (!current || picked.resolvedModel === current) ? picked.value : (models.find((m) => m.value !== 'default' && m.resolvedModel === current)?.value ?? 'default');
        return models.map((m) => ({
          value: m.value,
          label: m.value === 'default' ? 'Predeterminado' : m.displayName,
          tag: m.value === 'default' ? modelLabel(models, m.resolvedModel, null) : m.resolvedModel,
          hint: m.value === 'default' ? 'El que tienes configurado en Claude Code' : '',
          checked: m.value === chosen,
        }));
      },
      (value) => {
        const name = modelLabel(model.state.models, null, value);
        apply({ subtype: 'set_model', model: value }, () => (prefs.model = value)).then(() => announce(`Modelo: ${value === 'default' ? 'predeterminado' : name}.`));
      },
    );

    createMenu(
      $('effort'),
      $('effort-menu'),
      () => {
        const { models, applied } = model.state;
        const current = applied?.effort ?? prefs.effort;
        return effortLevels(models, applied?.model, prefs.model).map((level) => ({ value: level, label: level === 'xhigh' ? 'Extra alto' : (EFFORT_LABELS[level] ?? level), hint: EFFORT_HINTS[level] ?? '', checked: level === current }));
      },
      (value) => apply({ subtype: 'apply_flag_settings', settings: { effortLevel: value } }, () => (prefs.effort = value)).then(() => announce(`Esfuerzo: ${EFFORT_LABELS[value] ?? value}.`)),
    );

    createMenu(
      $('access'),
      $('access-menu'),
      () => {
        const { models, applied, permissionMode } = model.state;
        const current = models.find((m) => m.resolvedModel === applied?.model);
        return MODES.filter((mode) => mode !== 'auto' || current?.supportsAutoMode !== false).map((mode) => ({ value: mode, label: MODE_LABELS[mode], hint: MODE_HINTS[mode], checked: mode === permissionMode }));
      },
      (value) =>
        apply({ subtype: 'set_permission_mode', mode: value }, () => {
          prefs.mode = value;
          if (!running) model.state.permissionMode = value;
        }).then(() => announce(`Permisos: ${MODE_LABELS[value]}.`)),
    );
  }

  // ---------------------------------------------------------------- comandos «/»
  // Lista desplegable ligada al campo: el foco se queda en él y la opción activa se señala
  // con aria-activedescendant.
  let commandList = [];
  let commandAt = -1;

  function allCommands() {
    const terminalOnly = new Set(model.state.session?.terminalOnly ?? ['doctor', 'color', 'focus', 'reload-plugins']);
    const local = localCommands().map((c) => ({ ...c, local: true }));
    const taken = new Set(local.map((c) => c.name));
    const remote = model.state.commands.filter((c) => !terminalOnly.has(c.name) && !taken.has(c.name));
    return [...local, ...remote];
  }

  function closeCommands() {
    if (commandsBox.hidden) return;
    commandsBox.hidden = true;
    commandsBox.replaceChildren();
    commandList = [];
    commandAt = -1;
    input.setAttribute('aria-expanded', 'false');
    input.removeAttribute('aria-activedescendant');
    $('slash').setAttribute('aria-expanded', 'false');
  }

  function markCommand(index) {
    commandAt = index;
    [...commandsBox.querySelectorAll('[role="option"]')].forEach((option, i) => {
      option.setAttribute('aria-selected', String(i === index));
      if (i === index) {
        input.setAttribute('aria-activedescendant', option.id);
        option.scrollIntoView({ block: 'nearest' });
      }
    });
  }

  function openCommands(query) {
    if (feed) return;
    closeMenus();
    commandList = filterCommands(allCommands(), query).slice(0, 60);
    const options = commandList.map((command, index) => {
      const option = el('div', 'chat-menu-item');
      option.id = `${prefix}-command-${index}`;
      option.setAttribute('role', 'option');
      option.dataset.index = index;
      const slash = el('span', 'chat-menu-check', '/');
      slash.setAttribute('aria-hidden', 'true');
      slash.style.visibility = 'visible';
      option.append(slash, el('span', 'chat-menu-name mono', command.name));
      option.append(el('span', 'chat-menu-tag', command.local ? 'nexo' : (command.argumentHint ?? '')));
      if (command.description) option.append(el('span', 'chat-menu-hint', command.description));
      return option;
    });
    commandsBox.replaceChildren(...(options.length ? options : [el('p', 'chat-menu-empty', 'Ningún comando empieza así.')]));
    commandsBox.hidden = false;
    input.setAttribute('aria-expanded', 'true');
    $('slash').setAttribute('aria-expanded', 'true');
    if (options.length) markCommand(0);
    else input.removeAttribute('aria-activedescendant');
  }

  function pickCommand(command) {
    closeCommands();
    if (!command) return;
    // Las órdenes de nexo sin argumentos se ejecutan al elegirlas; las demás quedan en el campo.
    if (command.local && !command.argumentHint) {
      input.value = '';
      grow();
      queueRender();
      command.run('');
    } else {
      input.value = `/${command.name} `;
      grow();
      queueRender();
    }
    input.focus();
  }

  // El menú acompaña mientras se escribe el nombre del comando: «/» y nada de espacios.
  function syncCommands() {
    const match = /^\/([^\s]*)$/.exec(input.value);
    if (match && !feed) openCommands(match[1]);
    else closeCommands();
  }

  // Sin el foco en el campo no hay a quién señalarle la opción activa.
  commandsBox.addEventListener('pointerdown', (event) => event.preventDefault());
  commandsBox.addEventListener('click', (event) => {
    const option = event.target.closest('[role="option"]');
    if (option) pickCommand(commandList[Number(option.dataset.index)]);
  });
  $('slash').addEventListener('click', () => {
    if (!commandsBox.hidden) {
      closeCommands();
      return input.focus();
    }
    if (!input.value.trim()) input.value = '/';
    input.focus();
    if (/^\/([^\s]*)$/.test(input.value)) syncCommands();
    else openCommands('');
  });

  // ---------------------------------------------------------------- compositor
  function grow() {
    input.style.height = 'auto';
    input.style.height = `${Math.min(input.scrollHeight, 184)}px`;
  }

  function setFiles(next) {
    files = next;
    const box = $('files');
    box.hidden = files.length === 0;
    box.replaceChildren(
      ...files.map((file) => {
        const li = el('li', 'chat-file');
        li.title = file;
        const remove = el('button');
        remove.type = 'button';
        remove.setAttribute('aria-label', `Quitar ${baseName(file)}`);
        remove.innerHTML = ICON.close;
        remove.addEventListener('click', () => {
          setFiles(files.filter((f) => f !== file));
          input.focus();
        });
        li.append(el('span', 'chat-file-name', baseName(file)), remove);
        return li;
      }),
    );
    queueRender();
  }

  $('attach').addEventListener('click', async () => {
    try {
      const picked = await pickFiles();
      if (picked?.length) {
        setFiles([...new Set([...files, ...picked])]);
        announce(`${picked.length === 1 ? 'Un archivo adjunto' : `${picked.length} archivos adjuntos`}.`);
      }
    } catch (error) {
      notice(error, 'error');
    }
    input.focus();
  });

  async function submit() {
    const text = input.value;
    if (!text.trim() && !files.length) {
      // Vacío y con Claude trabajando, el botón es «Detener».
      if (model.state.busy) interrupt();
      return input.focus();
    }
    // Estos dos empiezan la conversación de cero: lo hace nexo, que lleva la cuenta de la sesión.
    if (!feed && /^\/(clear|new)\s*$/.test(text.trim())) {
      input.value = '';
      grow();
      return reset();
    }
    // Las órdenes de nexo (compilar, preguntar, clave…) no van a Claude Code ni se pintan
    // como mensaje: una clave escrita aquí no sale del campo.
    const typed = /^\/(\S+)(?:\s+([\s\S]*))?$/.exec(text.trim());
    const local = !feed && typed && localCommands().find((c) => c.name === typed[1]);
    if (local) {
      input.value = '';
      grow();
      closeCommands();
      queueRender();
      local.run((typed[2] ?? '').trim());
      return input.focus();
    }
    const attached = files;
    input.value = '';
    grow();
    setFiles([]);
    closeCommands();
    if (!(await send(text, attached))) {
      // No salió: el texto vuelve al campo en vez de perderse.
      if (!input.value) input.value = text;
      setFiles(attached);
      grow();
    }
    input.focus();
  }

  form.addEventListener('submit', (event) => {
    event.preventDefault();
    submit();
  });

  input.addEventListener('input', () => {
    grow();
    syncCommands();
    queueRender();
  });
  input.addEventListener('keydown', (event) => {
    if (event.isComposing) return;
    if (!commandsBox.hidden) {
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        if (commandList.length) markCommand((commandAt + (event.key === 'ArrowDown' ? 1 : -1) + commandList.length) % commandList.length);
        return;
      }
      if ((event.key === 'Enter' || event.key === 'Tab') && commandList[commandAt]) {
        event.preventDefault();
        return pickCommand(commandList[commandAt]);
      }
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        return closeCommands();
      }
    }
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      submit();
    } else if (event.key === 'Escape' && model.state.busy && (!feed || onInterrupt)) {
      event.preventDefault();
      event.stopPropagation();
      interrupt();
    }
  });
  input.addEventListener('blur', () => setTimeout(() => document.activeElement !== input && closeCommands(), 120));

  const outside = (event) => {
    if (!form.isConnected) return document.removeEventListener('pointerdown', outside);
    if (!form.contains(event.target)) {
      closeMenus();
      closeCommands();
    }
  };
  document.addEventListener('pointerdown', outside);

  // ---------------------------------------------------------------- resto de la carta
  log.addEventListener('scroll', () => {
    stick = log.scrollHeight - log.scrollTop - log.clientHeight < 48;
    $('jump').hidden = stick;
  });
  $('jump').addEventListener('click', () => {
    stick = true;
    log.scrollTop = log.scrollHeight;
    $('jump').hidden = true;
    input.focus();
  });
  list.addEventListener('click', (event) => {
    const link = event.target.closest('a');
    if (!link || !list.contains(link)) return;
    event.preventDefault();
    onLink(link);
  });
  const restart = () => start().then((ok) => ok && input.focus());
  $('retry').addEventListener('click', restart);
  $('restart').addEventListener('click', restart);
  $('new')?.addEventListener('click', reset);
  const resizer = new ResizeObserver(() => queueRender());
  resizer.observe(bar);
  queueRender();

  return {
    start,
    send,
    relay,
    notice,
    reset,
    interrupt,
    // Las líneas que reparte el workspace (o el transporte del chat de Nexo).
    receive: onEvent,
    // Otro vault: la conversación era del anterior.
    async stop() {
      try {
        await transport.reset();
      } catch {}
      running = false;
      clearView();
      setPhase(blocked() ? blocked() : 'idle');
    },
    // Vuelve a abrir el proceso (Reiniciar espacio de Workspace): se retoma la sesión.
    async restart() {
      running = false;
      setPhase('starting');
      return start();
    },
    // Cuando cambia algo de fuera que se ve en la conversación (los agentes de los procesos).
    refresh(ids) {
      for (const id of ids ?? []) model.touchTool?.(id);
      queueRender();
    },
    setBusy(busy) {
      model.state.busy = Boolean(busy);
      queueRender();
    },
    focus: () => input.focus(),
    // Para la API de nexo: la pregunta del usuario y una respuesta que se va llenando, en la
    // misma conversación pero sin pasar por Claude Code.
    userNote(text) {
      model.addUser(text, [], { local: true });
      stick = true;
      queueRender();
    },
    reply(who) {
      const reply = model.localReply(who);
      queueRender();
      return {
        push(text) {
          reply.push(text);
          queueRender();
        },
        end() {
          const text = reply.end();
          queueRender();
          if (text) announce(`${who} terminó. ${plain(text).slice(0, 280)}`);
          return text;
        },
      };
    },
    // La carta vuelve a verse (se restauró o se cerró la actividad): al final y con el foco.
    show({ focus = true } = {}) {
      stick = true;
      // Ya, no en el fotograma siguiente: una ventana que vuelve a la rejilla empieza arriba y
      // maquetaría para nada las primeras piezas (las lejanas solo se maquetan si se ven).
      log.scrollTop = log.scrollHeight;
      queueRender();
      if (focus) input.focus();
    },
    dispose() {
      resizer.disconnect();
      document.removeEventListener('pointerdown', outside);
      for (const [, pending] of waiting) clearTimeout(pending.timer);
      waiting.clear();
      cancelAnimationFrame(frame);
      cancelAnimationFrame(farFrame);
    },
    model,
    get running() {
      return running;
    },
    get busy() {
      return model.state.busy;
    },
    get waiting() {
      return model.pending().length > 0;
    },
    get hasConversation() {
      return model.state.items.some((i) => i.kind === 'user');
    },
  };
}
