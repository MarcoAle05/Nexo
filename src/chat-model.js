// Modelo del chat con Claude Code. Convierte las líneas del protocolo `stream-json` (lo que
// `claude -p` escribe por su salida) en una lista de piezas para pintar: mensajes del
// usuario, texto de Claude, sus razonamientos y sus procesos (herramientas, con los
// subagentes dentro y los permisos que piden). Sin DOM: se prueba con node.
//
// Además de las líneas de Claude Code entiende dos propias, que nexo guarda para poder
// repetir la conversación al recargar la interfaz: `nexo_user` (lo que escribió el usuario)
// y `nexo_permission` (lo que contestó a un permiso).

// Nombre legible de cada herramienta y el dato de su entrada que mejor la resume.
const TOOLS = {
  Read: ['Leer', (i) => i.file_path],
  Write: ['Escribir', (i) => i.file_path],
  Edit: ['Editar', (i) => i.file_path],
  MultiEdit: ['Editar', (i) => i.file_path],
  NotebookEdit: ['Editar', (i) => i.notebook_path],
  Bash: ['Ejecutar', (i) => i.description || i.command],
  BashOutput: ['Salida del proceso', (i) => i.bash_id],
  KillShell: ['Detener proceso', (i) => i.shell_id],
  Glob: ['Buscar archivos', (i) => i.pattern],
  Grep: ['Buscar texto', (i) => i.pattern],
  WebFetch: ['Leer página', (i) => host(i.url)],
  WebSearch: ['Buscar en la web', (i) => i.query],
  Task: ['Agente', (i) => [i.subagent_type, i.description].filter(Boolean).join(' · ')],
  Agent: ['Agente', (i) => [i.subagent_type, i.description].filter(Boolean).join(' · ')],
  Skill: ['Skill', (i) => i.skill || i.command],
  TodoWrite: ['Tareas', (i) => (Array.isArray(i.todos) ? `${i.todos.length} en la lista` : '')],
  TaskCreate: ['Tarea nueva', (i) => i.subject || i.description],
  TaskUpdate: ['Tarea', (i) => [i.status, i.subject].filter(Boolean).join(' · ')],
  AskUserQuestion: ['Pregunta', (i) => i.questions?.[0]?.question],
  ExitPlanMode: ['Plan', () => 'listo para revisar'],
  ToolSearch: ['Buscar herramientas', (i) => i.query],
};

function host(url) {
  try {
    return new URL(url).hostname;
  } catch {
    return url ?? '';
  }
}

// Ruta corta: relativa a la carpeta de trabajo si cae dentro; si no, sus dos últimos tramos.
export function shortPath(path, cwd) {
  if (typeof path !== 'string' || !path) return '';
  if (cwd && (path === cwd || path.startsWith(`${cwd}/`))) return path.slice(cwd.length + 1) || '.';
  const parts = path.split('/').filter(Boolean);
  return parts.length > 2 ? `…/${parts.slice(-2).join('/')}` : path;
}

const oneLine = (text, max = 96) => {
  const line = String(text ?? '').split('\n').find((l) => l.trim()) ?? '';
  const clean = line.trim();
  return clean.length > max ? `${clean.slice(0, max - 1)}…` : clean;
};

// { label, detail } de una herramienta: «Editar · wiki/notas/compra.md».
export function toolSummary(name, input = {}, cwd = '') {
  const safe = input && typeof input === 'object' ? input : {};
  const known = TOOLS[name];
  if (known) {
    const [label, pick] = known;
    const value = pick(safe);
    const isPath = ['file_path', 'notebook_path'].some((k) => safe[k] === value);
    return { label, detail: isPath ? shortPath(value, cwd) : oneLine(value) };
  }
  // Herramientas MCP: mcp__servidor__herramienta.
  const mcp = /^mcp__(.+?)__(.+)$/.exec(name ?? '');
  const first = Object.values(safe).find((v) => typeof v === 'string' && v.trim());
  if (mcp) return { label: mcp[1].replace(/^claude_ai_/, '').replace(/_/g, ' '), detail: oneLine([mcp[2].replace(/_/g, ' '), first && host(first)].filter(Boolean).join(' · ')) };
  return { label: name || 'Herramienta', detail: oneLine(first) };
}

// Texto de un resultado de herramienta (cadena o bloques), acotado para no pintar megas.
const RESULT_MAX = 12_000;
function resultText(content) {
  let text = '';
  if (typeof content === 'string') text = content;
  else if (Array.isArray(content)) {
    text = content
      .map((b) => (b?.type === 'text' ? b.text : b?.type === 'image' ? '[imagen]' : ''))
      .filter(Boolean)
      .join('\n');
  }
  return text.length > RESULT_MAX ? `${text.slice(0, RESULT_MAX)}\n… (${text.length - RESULT_MAX} caracteres más)` : text;
}

// Modelo vigente → nombre para el selector («Opus 5.5»). `applied` es el id resuelto.
export function modelLabel(models, applied, chosen) {
  const list = Array.isArray(models) ? models : [];
  const named = (m) => m && m.value !== 'default';
  const byChoice = list.find((m) => named(m) && m.value === chosen);
  const byId = list.find((m) => named(m) && (m.resolvedModel === applied || m.value === applied));
  const fallback = list.find((m) => m.value === 'default');
  const viaDefault = fallback && list.find((m) => named(m) && m.resolvedModel === fallback.resolvedModel);
  return (byChoice ?? byId ?? (applied ? null : viaDefault))?.displayName ?? applied ?? 'Modelo';
}

// En el botón van cortas (la barra es estrecha); en el menú, con su explicación.
export const EFFORT_LABELS = { low: 'Bajo', medium: 'Medio', high: 'Alto', xhigh: 'Extra', max: 'Máximo' };
export const EFFORT_HINTS = {
  low: 'Rápido, para tareas sencillas',
  medium: 'Equilibrado',
  high: 'Más cuidado en cada paso',
  xhigh: 'Extra alto, para trabajo difícil',
  max: 'El más lento y el que más gasta',
};
export const MODE_LABELS = { default: 'Preguntar', acceptEdits: 'Ediciones', plan: 'Plan', auto: 'Automático' };
export const MODE_HINTS = {
  default: 'Pide permiso antes de editar archivos o ejecutar órdenes',
  acceptEdits: 'Edita archivos sin preguntar; pide permiso para lo demás',
  plan: 'Solo investiga y propone un plan: no cambia nada',
  auto: 'Decide solo cuándo hace falta preguntar',
};

// Niveles de esfuerzo que admite el modelo vigente.
export function effortLevels(models, applied, chosen) {
  const list = Array.isArray(models) ? models : [];
  const model = list.find((m) => m.value === chosen && m.value !== 'default') ?? list.find((m) => m.resolvedModel === applied) ?? list.find((m) => m.value === 'default');
  return model?.supportedEffortLevels ?? (model && !model.supportsEffort ? [] : Object.keys(EFFORT_LABELS));
}

export function createChatModel() {
  const state = {
    items: [],
    busy: false,
    status: null, // 'requesting' | 'compacting' | 'retry' | null
    session: null, // { id, model, cwd, version, mcp }
    commands: [],
    models: [],
    account: null, // { subscription }
    applied: null, // { model, effort }: lo que Claude Code tiene vigente
    permissionMode: 'default',
    limits: null, // { fiveHour, sevenDay, resetsAt }
    lastResult: null,
  };
  const dirty = new Set();
  const tools = new Map(); // tool_use_id → pieza
  const streams = new Map(); // id del mensaje → piezas que se están escribiendo
  const current = new Map(); // subagente (o '') → id del mensaje que se está escribiendo
  let seq = 0;

  const touch = (item) => {
    // Lo que pasa dentro de un subagente se pinta con su proceso.
    dirty.add(item.owner ?? item);
    return item;
  };

  function add(item, parent) {
    item.id ??= `n${++seq}`;
    const owner = parent ? tools.get(parent) : null;
    if (owner) {
      item.owner = owner.owner ?? owner;
      owner.children.push(item);
    } else state.items.push(item);
    if (item.kind === 'tool') tools.set(item.id, item);
    return touch(item);
  }

  const toolItem = (id, name, input, parent) =>
    tools.get(id) ?? add({ id, kind: 'tool', name: name ?? '', input: input ?? {}, partial: '', status: 'preparing', result: null, children: [], permission: null }, parent);

  // Pieza que el streaming ya empezó para este mensaje y este tipo, y aún no se ha cerrado.
  function openStream(messageId, kind) {
    return (streams.get(messageId) ?? []).find((item) => item.kind === kind && !item.final);
  }

  function onStream(msg) {
    const event = msg.event ?? {};
    const parent = msg.parent_tool_use_id ?? '';
    if (event.type === 'message_start') {
      current.set(parent, event.message?.id);
      return;
    }
    const messageId = current.get(parent);
    if (!messageId) return;
    const list = streams.get(messageId) ?? streams.set(messageId, []).get(messageId);
    const at = (index) => list.find((item) => item.index === index);
    if (event.type === 'content_block_start') {
      const block = event.content_block ?? {};
      let item = null;
      if (block.type === 'text') item = add({ kind: 'text', text: block.text ?? '', streaming: true }, parent);
      else if (block.type === 'thinking') item = add({ kind: 'thinking', text: block.thinking ?? '', tokens: 0, streaming: true }, parent);
      else if (block.type === 'tool_use') item = toolItem(block.id, block.name, {}, parent);
      if (item) {
        item.index = event.index;
        item.final = false;
        list.push(item);
      }
    } else if (event.type === 'content_block_delta') {
      const item = at(event.index);
      const delta = event.delta ?? {};
      if (!item || item.final) return;
      if (delta.type === 'text_delta') item.text += delta.text ?? '';
      else if (delta.type === 'thinking_delta') item.text += delta.thinking ?? '';
      else if (delta.type === 'input_json_delta') item.partial += delta.partial_json ?? '';
      else return;
      touch(item);
    } else if (event.type === 'content_block_stop') {
      const item = at(event.index);
      if (item && item.streaming) {
        item.streaming = false;
        touch(item);
      }
    }
  }

  function onAssistant(msg) {
    const message = msg.message ?? {};
    const parent = msg.parent_tool_use_id ?? '';
    for (const block of message.content ?? []) {
      if (block.type === 'text') {
        const item = openStream(message.id, 'text') ?? add({ kind: 'text', text: '' }, parent);
        Object.assign(item, { text: block.text ?? '', streaming: false, final: true, synthetic: message.model === '<synthetic>' });
        touch(item);
      } else if (block.type === 'thinking' || block.type === 'redacted_thinking') {
        const item = openStream(message.id, 'thinking') ?? add({ kind: 'thinking', text: '', tokens: 0 }, parent);
        Object.assign(item, { text: block.thinking || item.text || '', streaming: false, final: true });
        touch(item);
      } else if (block.type === 'tool_use') {
        const item = toolItem(block.id, block.name, block.input, parent);
        Object.assign(item, { name: block.name, input: block.input ?? {}, final: true });
        if (item.status === 'preparing') item.status = 'running';
        touch(item);
      }
    }
  }

  function onUser(msg) {
    const content = msg.message?.content;
    if (!Array.isArray(content)) return;
    for (const block of content) {
      if (block?.type !== 'tool_result') continue;
      const item = tools.get(block.tool_use_id);
      if (!item) continue;
      item.result = resultText(block.content);
      const denied = item.permission?.answer === 'deny';
      item.status = denied ? 'denied' : block.is_error ? 'error' : 'done';
      touch(item);
    }
  }

  function onPermission(msg) {
    const request = msg.request ?? {};
    const item = toolItem(request.tool_use_id ?? `perm-${msg.request_id}`, request.tool_name, request.input, null);
    if (!item.final) Object.assign(item, { name: request.tool_name ?? item.name, input: request.input ?? item.input });
    item.permission = {
      requestId: msg.request_id,
      title: request.display_name ?? request.tool_name ?? '',
      description: request.description ?? '',
      suggestions: request.permission_suggestions ?? [],
      answer: null,
    };
    item.status = 'waiting';
    touch(item);
  }

  function answer(requestId, decision) {
    const item = [...tools.values()].find((t) => t.permission?.requestId === requestId);
    if (!item || item.permission.answer) return null;
    item.permission.answer = decision;
    if (item.status === 'waiting') item.status = decision === 'deny' ? 'denied' : 'running';
    return touch(item);
  }

  function onControlResponse(msg) {
    const response = msg.response ?? {};
    const data = response.response;
    if (response.subtype === 'success' && data && typeof data === 'object') {
      if (Array.isArray(data.models)) {
        state.models = data.models;
        state.commands = Array.isArray(data.commands) ? data.commands : state.commands;
        state.account = data.account ? { subscription: data.account.subscriptionType ?? '' } : state.account;
        if (data.current_permission_mode) state.permissionMode = data.current_permission_mode;
      }
      if (data.applied && typeof data.applied === 'object') state.applied = { model: data.applied.model ?? null, effort: data.applied.effort ?? null };
      if (typeof data.mode === 'string') state.permissionMode = data.mode;
    }
  }

  function onSystem(msg) {
    switch (msg.subtype) {
      case 'init':
        state.session = { id: msg.session_id, model: msg.model, cwd: msg.cwd ?? '', version: msg.claude_code_version ?? '', mcp: msg.mcp_servers ?? [], terminalOnly: msg.terminal_slash_commands ?? [] };
        if (msg.permissionMode) state.permissionMode = msg.permissionMode;
        state.busy = true;
        break;
      case 'status':
        state.status = msg.status ?? null;
        if (msg.permissionMode) state.permissionMode = msg.permissionMode;
        break;
      case 'session_state_changed':
        if (msg.state === 'idle') state.busy = false;
        else if (msg.state === 'running') state.busy = true;
        break;
      case 'compact_boundary':
        add({ kind: 'notice', level: 'info', text: 'Contexto compactado: Claude resumió lo anterior para seguir.' });
        break;
      case 'api_retry':
        state.status = 'retry';
        break;
      case 'thinking_tokens': {
        const thinking = [...streams.values()].flat().findLast((item) => item.kind === 'thinking');
        if (thinking && Number.isFinite(msg.estimated_tokens)) {
          thinking.tokens = msg.estimated_tokens;
          touch(thinking);
        }
        break;
      }
      case 'informational':
      case 'notification':
      case 'local_command': {
        const text = [msg.content, msg.message, msg.text].find((v) => typeof v === 'string' && v.trim());
        if (text) add({ kind: 'notice', level: msg.level === 'error' ? 'error' : 'info', text: text.trim() });
        break;
      }
      default:
    }
  }

  function onResult(msg) {
    state.busy = false;
    state.status = null;
    state.lastResult = { error: Boolean(msg.is_error), durationMs: msg.duration_ms ?? 0, cost: msg.total_cost_usd ?? 0, turns: msg.num_turns ?? 0 };
    settle('Interrumpido');
    if (msg.is_error) {
      const text = typeof msg.result === 'string' && msg.result.trim() ? msg.result.trim() : Array.isArray(msg.errors) && msg.errors.length ? msg.errors.join(' · ') : 'El turno terminó con un error.';
      add({ kind: 'notice', level: 'error', text });
    }
  }

  // Lo que se quedó a medias cuando el turno (o el proceso) termina.
  function settle(why) {
    for (const item of streams.values()) for (const piece of item) if (piece.streaming) touch(Object.assign(piece, { streaming: false }));
    streams.clear();
    current.clear();
    for (const item of tools.values()) {
      if (item.status === 'running' || item.status === 'preparing' || item.status === 'waiting') {
        if (item.permission && !item.permission.answer) item.permission.answer = 'cancelled';
        item.status = 'stopped';
        item.result ??= why;
        touch(item);
      }
    }
  }

  return {
    state,

    // Aplica una línea del protocolo (objeto ya interpretado). Devuelve lo que la interfaz
    // tiene que hacer además de pintar: contestar a una petición que no sabemos atender.
    apply(msg) {
      if (!msg || typeof msg !== 'object') return null;
      switch (msg.type) {
        case 'stream_event':
          onStream(msg);
          break;
        case 'assistant':
          onAssistant(msg);
          break;
        case 'user':
          onUser(msg);
          break;
        case 'system':
          onSystem(msg);
          break;
        case 'result':
          onResult(msg);
          break;
        case 'control_request':
          if (msg.request?.subtype === 'can_use_tool') onPermission(msg);
          else return { unsupported: msg.request_id, subtype: msg.request?.subtype ?? '' };
          break;
        case 'control_cancel_request':
          answer(msg.request_id, 'cancelled');
          break;
        case 'control_response':
          onControlResponse(msg);
          break;
        case 'rate_limit_event': {
          const info = msg.rate_limit_info ?? {};
          const windows = info.unifiedWindows ?? {};
          state.limits = { status: info.status ?? '', fiveHour: windows.five_hour?.utilization ?? null, sevenDay: windows.seven_day?.utilization ?? null, resetsAt: info.resetsAt ?? null };
          break;
        }
        case 'nexo_user':
          add({ kind: 'user', text: String(msg.text ?? ''), files: Array.isArray(msg.files) ? msg.files : [] });
          if (!msg.local) state.busy = true;
          break;
        case 'nexo_permission':
          answer(msg.request_id, msg.decision);
          break;
        default:
      }
      return null;
    },

    // Mensaje del usuario recién enviado. Con `local`, no va a Claude Code (es para la API de
    // nexo): se pinta, pero no empieza ningún turno suyo.
    addUser(text, files = [], { local = false } = {}) {
      if (!local) state.busy = true;
      return add({ kind: 'user', text, files });
    },

    // Respuesta que no viene de Claude Code sino de la API de nexo (`/preguntar`): una pieza de
    // texto con el nombre de quien habla, que se va llenando.
    localReply(speaker) {
      const item = add({ kind: 'text', text: '', streaming: true, speaker });
      return {
        push(text) {
          item.text += text;
          touch(item);
        },
        end() {
          item.streaming = false;
          touch(item);
          return item.text;
        },
      };
    },

    // Aviso de nexo (no viene de Claude): errores de la app, cambios de vault…
    notice(text, level = 'info') {
      return add({ kind: 'notice', level, text: String(text) });
    },

    answer,

    // Claude Code terminó (o se detuvo).
    exit() {
      state.busy = false;
      state.status = null;
      settle('Claude Code se cerró');
    },

    // Piezas que cambiaron desde la última vez que se pidió.
    flush() {
      const changed = [...dirty];
      dirty.clear();
      return changed;
    },

    clear() {
      state.items.length = 0;
      state.busy = false;
      state.status = null;
      state.lastResult = null;
      tools.clear();
      streams.clear();
      current.clear();
      dirty.clear();
    },

    // Permisos sin contestar, del más antiguo al más nuevo.
    pending() {
      return [...tools.values()].filter((t) => t.permission && !t.permission.answer);
    },
  };
}

// Líneas que nexo le escribe a Claude Code.
export const userLine = (text) => JSON.stringify({ type: 'user', message: { role: 'user', content: text }, parent_tool_use_id: null, session_id: '' });
export const controlLine = (requestId, request) => JSON.stringify({ type: 'control_request', request_id: requestId, request });
export const allowLine = (requestId, input, permissions) =>
  JSON.stringify({ type: 'control_response', response: { subtype: 'success', request_id: requestId, response: { behavior: 'allow', updatedInput: input ?? {}, ...(permissions?.length ? { updatedPermissions: permissions } : {}) } } });
export const denyLine = (requestId, message) =>
  JSON.stringify({ type: 'control_response', response: { subtype: 'success', request_id: requestId, response: { behavior: 'deny', message: message || 'El usuario no dio permiso.' } } });
export const unsupportedLine = (requestId, subtype) =>
  JSON.stringify({ type: 'control_response', response: { subtype: 'error', request_id: requestId, error: `nexo no atiende peticiones «${subtype}»` } });

// Mensaje que se le manda a Claude: el texto y, debajo, los archivos adjuntos como rutas.
export function composeMessage(text, files = []) {
  const body = text.trim();
  if (!files.length) return body;
  const list = files.map((f) => `- ${f}`).join('\n');
  return `${body}${body ? '\n\n' : ''}Archivos adjuntos (léelos con Read):\n${list}`;
}

// Comandos «/» que encajan con lo escrito, primero los que empiezan igual.
export function filterCommands(commands, query) {
  const q = query.trim().toLowerCase();
  const scored = [];
  for (const command of commands) {
    const names = [command.name, ...(command.aliases ?? [])].map((n) => n.toLowerCase());
    const starts = names.some((n) => n.startsWith(q));
    const inside = !starts && (names.some((n) => n.includes(q)) || (q.length > 2 && (command.description ?? '').toLowerCase().includes(q)));
    if (!q || starts || inside) scored.push([starts ? 0 : 1, command]);
  }
  return scored.sort((a, b) => a[0] - b[0]).map(([, command]) => command);
}
