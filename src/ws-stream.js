// Reparto de la salida del orquestador de un workspace entre sus ventanas. Todo llega por el
// mismo protocolo de Claude Code (workspace_run.rs): lo que hace un subagente viene con
// `parent_tool_use_id` (la llamada a Agent que lo desplegó o lo retomó) y su ciclo de vida
// con las líneas `system` de tareas (`task_started`, `task_progress`, `task_notification`,
// `task_updated`). Aquí se lleva la cuenta de cada agente (nombre, tipo, encargo, estado y
// tokens, que no se guardan en ningún sitio) y se decide a qué chat va cada línea: al del
// orquestador (`main`) o al del agente, que al acabar mal recibe una nota de cómo acabó. Sin
// DOM: se prueba con node.

export const AGENT_TOOLS = new Set(['Agent', 'Task']);
// Líneas que se guardan como mucho para un agente cuya tarea aún no ha empezado.
const PENDING_MAX = 400;

const RUNNING = new Set(['running', 'pending']);
// Cómo acabó un agente (su `status`) en el estado que se enseña.
const ENDED = { completed: 'done', failed: 'failed', stopped: 'stopped' };
// Lo que dice Claude Code, al retomar una sesión, de una tarea que cortó el final de la anterior.
const CUT = /didn't finish before the previous session ended/i;

// Cómo acabó una tarea según Claude Code: terminó, falló, se detuvo (alguien la paró) o no se
// sabe (`unknown`: un estado que nexo no conoce o una tarea que cortó el final de la sesión).
// Lo que no se sabe nunca se da por terminado.
export function endStatus(status, summary = '') {
  if (status === 'completed' || status === 'failed') return status;
  if (status === 'stopped' || status === 'killed') return CUT.test(summary) ? 'unknown' : 'stopped';
  return 'unknown';
}

// Cómo acabó un agente según el resultado de su llamada a Agent (uno en primer plano, o sin
// líneas de tarea). Uno en segundo plano solo dice que arrancó: de ahí no se sabe cómo acabó.
function resultEnd(block) {
  const text = JSON.stringify(block.content ?? '');
  if (text.includes('Async agent launched')) return 'unknown';
  if (/interrupted by user/i.test(text)) return 'stopped';
  return block.is_error ? 'failed' : 'completed';
}

// Tokens de una llamada al modelo: todo lo que leyó y escribió (como el registro de agentes).
export function usageTokens(usage) {
  if (!usage || typeof usage !== 'object') return 0;
  const n = (k) => (Number.isFinite(usage[k]) ? usage[k] : 0);
  return n('input_tokens') + n('output_tokens') + n('cache_read_input_tokens') + n('cache_creation_input_tokens');
}

// Estado que se enseña de un agente: esperando respuesta (tiene un permiso o una pregunta sin
// contestar), activo (su tarea está en marcha) o cómo acabó: terminado, falló, detenido o sin
// terminar (no se sabe: también el que nunca dio noticias). «Inactivo» es solo del orquestador
// y de Nexo, que son conversaciones y no tareas que acaben.
export function agentState(agent, waiting = false) {
  if (waiting) return 'waiting';
  if (!agent) return 'unknown';
  if (RUNNING.has(agent.status)) return 'active';
  return ENDED[agent.status] ?? 'unknown';
}

export const STATE_LABEL = { active: 'Activo', waiting: 'Esperando respuesta', idle: 'Inactivo', done: 'Terminado', failed: 'Falló', stopped: 'Detenido', unknown: 'Sin terminar' };

// Lo que se anuncia de un agente que acaba de terminar mal (el que terminó bien, no: lo cuenta
// el orquestador).
export const ENDED_TEXT = { failed: 'falló', stopped: 'se detuvo', unknown: 'quedó sin terminar' };

// La nota que cierra la conversación de un agente que no acabó bien, como un aviso del chat.
export function endNotice(agent) {
  const state = agentState(agent);
  if (state === 'failed') {
    // El resumen del aviso del historial empieza por «Agent "‹nombre›" failed:»; en directo
    // puede ser lo último que escribió el agente: solo su primera línea.
    const why = (agent.ending ?? '')
      .replace(/^Agent "[^"]*" failed:?\s*/i, '')
      .split('\n')
      .find((l) => l.trim())
      ?.trim()
      .slice(0, 200);
    return notice('error', why ? `El agente falló: ${why}` : 'El agente falló.');
  }
  if (state === 'stopped') return notice('info', 'El agente se detuvo.');
  if (state === 'unknown') return notice('info', 'El agente no terminó: Claude Code se cerró antes de que acabara, o no quedó constancia de cómo acabó.');
  return null;
}

const notice = (level, content) => ({ type: 'system', subtype: 'informational', level, content });

export function createWorkspaceStream() {
  const agents = new Map(); // id de la tarea → agente
  const byTool = new Map(); // tool_use_id de la llamada que lo desplegó o retomó → id
  const ownerOfTool = new Map(); // tool_use_id de una herramienta que usó un agente → id
  const owners = new Map(); // request_id de un permiso → id del agente o 'main'
  const spawns = new Map(); // tool_use_id de una llamada a Agent → su entrada
  const pending = new Map(); // tool_use_id aún sin agente → líneas en espera
  const mainUsage = new Map(); // id de respuesta del orquestador → tokens

  const strip = (msg) => {
    const { parent_tool_use_id: _, ...rest } = msg;
    return rest;
  };

  function upsert(id, fields) {
    let agent = agents.get(id);
    const created = !agent;
    if (!agent) {
      agent = { id, toolUse: null, name: '', type: 'general-purpose', prompt: '', status: 'running', background: false, tokens: 0, toolUses: 0, durationMs: 0, summary: '', ending: '', noted: false, lastTool: '', parent: null, history: false, usage: new Map(), started: Date.now() };
      agents.set(id, agent);
    }
    for (const [k, v] of Object.entries(fields)) if (v != null && v !== '') agent[k] = v;
    return { agent, created };
  }

  // Lo que escribió el modelo de un agente: sus herramientas (para sus permisos) y sus tokens.
  function noteAssistant(agent, msg) {
    const message = msg.message ?? {};
    for (const block of message.content ?? []) if (block?.type === 'tool_use' && block.id) ownerOfTool.set(block.id, agent.id);
    if (message.id && message.usage) agent.usage.set(message.id, Math.max(agent.usage.get(message.id) ?? 0, usageTokens(message.usage)));
  }

  // Un agente acaba: su estado, su nota (si no acabó bien) y la cuenta de los que acabaron en
  // esta línea, para anunciarlos. El mismo final dos veces (el resultado de su llamada y su
  // aviso) no deja dos notas.
  function finish(agent, status, out, summary = '') {
    out.changed.add(agent.id);
    if (agent.noted && agent.status === status) return;
    agent.status = status;
    agent.ending = summary;
    agent.noted = true;
    out.ended.push(agent.id);
    const note = endNotice(agent);
    if (note) out.targets.push({ to: agent.id, msg: note });
  }

  function link(toolUse, id) {
    byTool.set(toolUse, id);
    const waiting = pending.get(toolUse) ?? [];
    pending.delete(toolUse);
    return waiting;
  }

  // Un agente sin línea de tarea (versiones de Claude Code que no las mandan): sale de la
  // llamada a Agent que lo desplegó.
  function provisional(toolUse) {
    const input = spawns.get(toolUse) ?? {};
    const id = `tool:${toolUse}`;
    const { agent, created } = upsert(id, { toolUse, name: input.description || input.subagent_type || 'Agente', type: input.subagent_type || 'general-purpose', prompt: input.prompt || '' });
    return { agent, created, lines: link(toolUse, id) };
  }

  const toAgent = (id, msg) => {
    const agent = agents.get(id);
    if (msg.type === 'assistant' && agent) noteAssistant(agent, msg);
    return { to: id, msg: strip(msg) };
  };

  function route(msg) {
    const out = { targets: [], changed: new Set(), created: [], ended: [] };
    if (!msg || typeof msg !== 'object') return out;

    if (msg.type === 'system') {
      if (msg.subtype === 'task_started') {
        const isAgent = msg.task_type ? msg.task_type === 'local_agent' : Boolean(msg.subagent_type);
        if (!isAgent || !msg.task_id) return out;
        const spawn = spawns.get(msg.tool_use_id) ?? {};
        const { agent, created } = upsert(msg.task_id, {
          name: msg.description || spawn.description || msg.subagent_type,
          type: msg.subagent_type || spawn.subagent_type,
          prompt: msg.prompt || spawn.prompt,
          parent: msg.parent_task_id,
          background: msg.is_backgrounded,
          // Del historial: hasta su aviso (que va detrás) no se sabe cómo acabó.
          status: msg.nexo_history ? 'unknown' : 'running',
          history: msg.nexo_history ? true : null,
        });
        agent.noted = false;
        if (!agent.toolUse && msg.tool_use_id) agent.toolUse = msg.tool_use_id;
        if (!msg.nexo_history && !created) agent.status = 'running';
        if (created) out.created.push(agent.id);
        out.changed.add(agent.id);
        if (msg.tool_use_id) for (const line of link(msg.tool_use_id, agent.id)) out.targets.push(toAgent(agent.id, line));
        return out;
      }
      if (msg.subtype === 'task_progress' || msg.subtype === 'task_notification' || msg.subtype === 'task_updated') {
        const agent = agents.get(msg.task_id);
        if (!agent) return out;
        const usage = msg.usage;
        if (usage) {
          if (Number.isFinite(usage.total_tokens)) agent.tokens = usage.total_tokens;
          if (Number.isFinite(usage.tool_uses)) agent.toolUses = usage.tool_uses;
          if (Number.isFinite(usage.duration_ms)) agent.durationMs = usage.duration_ms;
        }
        if (msg.subtype === 'task_progress') {
          agent.status = 'running';
          agent.noted = false;
          if (msg.summary) agent.summary = msg.summary;
          if (msg.last_tool_name) agent.lastTool = msg.last_tool_name;
        } else if (msg.subtype === 'task_notification') {
          if (msg.summary) agent.summary = msg.summary;
          finish(agent, endStatus(msg.status, msg.summary ?? ''), out, msg.summary ?? '');
        } else {
          const patch = msg.patch ?? {};
          if (patch.status) agent.status = RUNNING.has(patch.status) ? patch.status : endStatus(patch.status);
          if (patch.is_backgrounded != null) agent.background = patch.is_backgrounded;
        }
        out.changed.add(agent.id);
        return out;
      }
      if (msg.subtype === 'background_tasks_changed') {
        const live = new Set((msg.tasks ?? []).map((t) => t.task_id));
        for (const agent of agents.values()) {
          const now = live.has(agent.id);
          if (now !== agent.background) {
            agent.background = now;
            out.changed.add(agent.id);
          }
        }
        return out;
      }
    }

    const parent = msg.parent_tool_use_id;
    if (parent) {
      const id = byTool.get(parent);
      if (id) {
        out.targets.push(toAgent(id, msg));
        return out;
      }
      // La llamada a Agent ya se vio pero su tarea aún no empezó: se espera.
      if (spawns.has(parent)) {
        const list = pending.get(parent) ?? pending.set(parent, []).get(parent);
        if (list.length < PENDING_MAX) list.push(msg);
        return out;
      }
      out.targets.push({ to: 'main', msg });
      return out;
    }

    if (msg.type === 'control_request' && msg.request?.subtype === 'can_use_tool') {
      const request = msg.request;
      const id = (request.agent_id && agents.has(request.agent_id) ? request.agent_id : null) ?? ownerOfTool.get(request.tool_use_id) ?? null;
      owners.set(msg.request_id, id ?? 'main');
      out.targets.push({ to: id ?? 'main', msg });
      if (id) out.changed.add(id);
      return out;
    }
    if (msg.type === 'control_cancel_request' || msg.type === 'nexo_permission') {
      const to = owners.get(msg.request_id) ?? 'main';
      out.targets.push({ to, msg });
      if (to !== 'main') out.changed.add(to);
      return out;
    }
    if (msg.type === 'nexo_user' && msg.to) {
      const agent = agents.get(msg.to);
      out.targets.push({ to: 'main', msg: { ...msg, toName: msg.toName ?? agent?.name ?? null } });
      if (agent) out.targets.push({ to: agent.id, msg: { type: 'nexo_user', text: msg.text, files: msg.files ?? [], local: true } });
      return out;
    }

    if (msg.type === 'assistant') {
      const message = msg.message ?? {};
      for (const block of message.content ?? []) {
        if (block?.type === 'tool_use' && AGENT_TOOLS.has(block.name)) spawns.set(block.id, block.input ?? {});
      }
      if (message.id && message.usage) mainUsage.set(message.id, Math.max(mainUsage.get(message.id) ?? 0, usageTokens(message.usage)));
    } else if (msg.type === 'user' && Array.isArray(msg.message?.content)) {
      for (const block of msg.message.content) {
        if (block?.type !== 'tool_result' || !spawns.has(block.tool_use_id)) continue;
        const id = byTool.get(block.tool_use_id);
        if (id) {
          // Un agente en primer plano acaba con su llamada (si no llegó antes su aviso).
          const agent = agents.get(id);
          const end = resultEnd(block);
          if (agent && RUNNING.has(agent.status) && end !== 'unknown') finish(agent, end, out);
          continue;
        }
        // La llamada a Agent terminó sin que llegara su tarea: el agente sale de la llamada.
        if (!pending.has(block.tool_use_id)) continue;
        const { agent, created, lines } = provisional(block.tool_use_id);
        if (created) out.created.push(agent.id);
        for (const line of lines) out.targets.push(toAgent(agent.id, line));
        const end = resultEnd(block);
        // Uno en segundo plano sigue trabajando: no se sabe cómo acabará, pero no se cortó.
        if (end === 'unknown') {
          agent.status = 'unknown';
          out.changed.add(agent.id);
        } else finish(agent, end, out);
      }
    }
    out.targets.push({ to: 'main', msg });
    return out;
  }

  return {
    route,
    agents,
    get(id) {
      return agents.get(id) ?? null;
    },
    // El agente que desplegó una llamada a Agent del orquestador (para el botón de su proceso).
    byToolUse(toolUse) {
      const id = byTool.get(toolUse);
      return id ? agents.get(id) : null;
    },
    tokensOf(agent) {
      if (!agent) return 0;
      if (agent.tokens) return agent.tokens;
      let sum = 0;
      for (const v of agent.usage.values()) sum += v;
      return sum;
    },
    mainTokens() {
      let sum = 0;
      for (const v of mainUsage.values()) sum += v;
      return sum;
    },
    // Claude Code se cerró: ningún agente sigue en marcha, y de los que lo estaban no se sabe
    // cómo acabaron (aunque lo cerrara nexo). Su nota la pide quien lo cierra (`endNotice`).
    exit() {
      const changed = [];
      for (const agent of agents.values()) {
        if (RUNNING.has(agent.status)) {
          agent.status = 'unknown';
          agent.ending = '';
          agent.noted = true;
          changed.push(agent.id);
        }
      }
      return changed;
    },
    forget(id) {
      agents.delete(id);
      for (const [tool, owner] of byTool) if (owner === id) byTool.delete(tool);
    },
    reset() {
      for (const map of [agents, byTool, ownerOfTool, owners, spawns, pending, mainUsage]) map.clear();
    },
  };
}
