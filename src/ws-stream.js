// Reparto de la salida del orquestador de un workspace entre sus ventanas. Todo llega por el
// mismo protocolo de Claude Code (workspace_run.rs): lo que hace un subagente viene con
// `parent_tool_use_id` (la llamada a Agent que lo desplegó o lo retomó) y su ciclo de vida
// con las líneas `system` de tareas (`task_started`, `task_progress`, `task_notification`,
// `task_updated`). Aquí se lleva la cuenta de cada agente (nombre, tipo, encargo, estado y
// tokens, que no se guardan en ningún sitio) y se decide a qué chat va cada línea: al del
// orquestador (`main`) o al del agente. Sin DOM: se prueba con node.

export const AGENT_TOOLS = new Set(['Agent', 'Task']);
// Líneas que se guardan como mucho para un agente cuya tarea aún no ha empezado.
const PENDING_MAX = 400;

const RUNNING = new Set(['running', 'pending']);

// Tokens de una llamada al modelo: todo lo que leyó y escribió (como el registro de agentes).
export function usageTokens(usage) {
  if (!usage || typeof usage !== 'object') return 0;
  const n = (k) => (Number.isFinite(usage[k]) ? usage[k] : 0);
  return n('input_tokens') + n('output_tokens') + n('cache_read_input_tokens') + n('cache_creation_input_tokens');
}

// Estado que se enseña de un agente: esperando respuesta (tiene un permiso o una pregunta sin
// contestar), activo (su tarea está en marcha) o inactivo.
export function agentState(agent, waiting = false) {
  if (waiting) return 'waiting';
  return agent && RUNNING.has(agent.status) ? 'active' : 'idle';
}

export const STATE_LABEL = { active: 'Activo', waiting: 'Esperando respuesta', idle: 'Inactivo' };

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
      agent = { id, toolUse: null, name: '', type: 'general-purpose', prompt: '', status: 'running', background: false, tokens: 0, toolUses: 0, durationMs: 0, summary: '', lastTool: '', parent: null, history: false, usage: new Map(), started: Date.now() };
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
    const out = { targets: [], changed: new Set(), created: [] };
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
          status: msg.nexo_history ? 'completed' : 'running',
          history: msg.nexo_history ? true : null,
        });
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
          if (msg.summary) agent.summary = msg.summary;
          if (msg.last_tool_name) agent.lastTool = msg.last_tool_name;
        } else if (msg.subtype === 'task_notification') {
          agent.status = msg.status === 'failed' ? 'failed' : msg.status === 'stopped' ? 'stopped' : 'completed';
          if (msg.summary) agent.summary = msg.summary;
        } else {
          const patch = msg.patch ?? {};
          if (patch.status) agent.status = patch.status === 'killed' ? 'stopped' : patch.status;
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
      // La llamada a Agent terminó sin que llegara su tarea: el agente sale de la llamada.
      for (const block of msg.message.content) {
        if (block?.type !== 'tool_result' || !spawns.has(block.tool_use_id) || byTool.has(block.tool_use_id)) continue;
        if (!pending.has(block.tool_use_id)) continue;
        const { agent, created, lines } = provisional(block.tool_use_id);
        agent.status = 'completed';
        if (created) out.created.push(agent.id);
        out.changed.add(agent.id);
        for (const line of lines) out.targets.push(toAgent(agent.id, line));
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
    // Claude Code se cerró: ningún agente sigue en marcha.
    exit() {
      const changed = [];
      for (const agent of agents.values()) {
        if (RUNNING.has(agent.status)) {
          agent.status = 'stopped';
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
