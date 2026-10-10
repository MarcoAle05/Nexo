// Carta Agentes: pestañas Skills / Agentes y la lista de agentes de Claude Code del vault
// (.claude/agents/), cada uno con su resumen, su modelo y su esfuerzo. Los nuevos se piden a
// Claude Code (los escribe creador-de-skills) con el modelo y el esfuerzo elegidos.
import { reconcile } from './dom.js';

const FAMILY = { opus: 'Opus', sonnet: 'Sonnet', haiku: 'Haiku', fable: 'Fable' };

// «claude-haiku-5-5» → «Haiku 5.5»; los alias («sonnet») son el último de esa familia y sin
// modelo (o «inherit») el agente usa el de la sesión de Claude Code.
export function modelLabel(model) {
  const m = String(model ?? '').trim();
  if (!m || m === 'inherit') return 'Modelo de la sesión';
  const id = /^claude-(opus|sonnet|haiku|fable)-(\d+)-(\d+)(?:-\d{8})?$/.exec(m);
  if (id) return `${FAMILY[id[1]]} ${id[2]}.${id[3]}`;
  const alias = /^(opus|sonnet|haiku|fable)(\[1m\])?$/.exec(m);
  if (alias) return `${FAMILY[alias[1]]} (el último)`;
  return m;
}

const EFFORT = { low: 'bajo', medium: 'medio', high: 'alto', xhigh: 'muy alto', max: 'máximo' };

export function effortLabel(effort) {
  const e = String(effort ?? '').trim();
  if (!e) return 'Esfuerzo de la sesión';
  return `Esfuerzo ${EFFORT[e] ?? e}`;
}

// «Read, Bash, mcp__playwright__browser_click, …» → «Read, Bash · playwright: click, …»:
// las herramientas de un servidor MCP se agrupan con su nombre corto.
export function toolsLabel(tools) {
  const list = String(tools ?? '')
    .split(/[,\s]+/)
    .map((t) => t.trim())
    .filter(Boolean);
  if (!list.length) return 'Todas las herramientas de la sesión';
  const plain = [];
  const servers = new Map();
  for (const tool of list) {
    const mcp = /^mcp__([^_]+(?:_[^_]+)*?)__(.+)$/.exec(tool);
    if (!mcp) {
      plain.push(tool);
      continue;
    }
    const [, server, name] = mcp;
    if (!servers.has(server)) servers.set(server, []);
    servers.get(server).push(name.replace(/^browser_/, ''));
  }
  return [plain.join(', '), ...[...servers].map(([server, names]) => `${server}: ${names.join(', ')}`)].filter(Boolean).join(' · ');
}

const AGENT_ICON =
  '<svg width="15" height="15" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><circle cx="10" cy="7.5" r="3"/><path d="M4.5 16.5c.9-2.8 3-4.3 5.5-4.3s4.6 1.5 5.5 4.3"/></svg>';
const TRASH_ICON =
  '<svg width="14" height="14" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4 6h12M8 6V4h4v2M6 6l1 10h6l1-10"/></svg>';

const PENDING_MS = 20 * 60 * 1000;

export function createAgents({ invoke, ask, fail, onCreate }) {
  const $ = (id) => document.getElementById(id);
  const list = $('agents-list');
  const form = $('agent-create');
  const input = $('agent-create-input');
  let agents = [];
  let loaded = false;
  let pending = []; // { key, text, model, effort, since }
  const open = new Set(); // agentes desplegados (resumen completo y herramientas)

  // ---------- pestañas ----------
  let tab = 'skills';
  try {
    if (localStorage.getItem('nexo.agentsTab') === 'agentes') tab = 'agentes';
  } catch {}
  function showTab(next) {
    tab = next === 'agentes' ? 'agentes' : 'skills';
    document.querySelectorAll('[data-agents-tab]').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.agentsTab === tab)));
    $('pane-skills').hidden = tab !== 'skills';
    $('pane-agents').hidden = tab !== 'agentes';
    try {
      localStorage.setItem('nexo.agentsTab', tab);
    } catch {}
  }
  document.querySelectorAll('[data-agents-tab]').forEach((b) => b.addEventListener('click', () => showTab(b.dataset.agentsTab)));
  showTab(tab);

  // ---------- lista ----------
  async function load() {
    const before = new Set(agents.map((a) => a.id));
    try {
      agents = await invoke('agents_list');
    } catch {
      agents = [];
    }
    // Cada agente nuevo cierra una petición pendiente.
    const fresh = loaded ? agents.filter((a) => !before.has(a.id)).length : 0;
    loaded = true;
    if (fresh) pending = pending.slice(fresh);
    render();
  }

  function render() {
    const now = Date.now();
    pending = pending.filter((p) => now - p.since < PENDING_MS);
    $('agents-count').textContent = String(agents.length);
    const items = [...pending.map((p) => ({ kind: 'pending', ...p })), ...agents.map((a) => ({ kind: 'agent', agent: a }))];
    $('agents-empty').hidden = items.length > 0;
    $('agents-label').hidden = items.length === 0;
    reconcile(list, items, {
      key: (item) => (item.kind === 'agent' ? `a:${item.agent.id}` : `p:${item.key}`),
      sig: (item) =>
        item.kind === 'agent'
          ? [item.agent.name, item.agent.description, item.agent.model, item.agent.effort, item.agent.tools, item.agent.managed].join('|')
          : item.text,
      build: (item) => (item.kind === 'agent' ? agentRow(item.agent) : pendingRow(item)),
    });
  }

  function agentRow(agent) {
    const li = document.createElement('li');
    li.className = 'agent-row';
    li.classList.toggle('open', open.has(agent.id));
    li.innerHTML = `
      <button type="button" class="agent-main" aria-expanded="false">
        <span class="row-icon">${AGENT_ICON}</span>
        <span class="agent-text">
          <span class="agent-name"><span class="agent-name-text"></span></span>
          <span class="agent-specs">
            <span class="agent-spec agent-model"></span>
            <span class="agent-spec agent-effort"></span>
          </span>
          <span class="agent-desc"></span>
          <span class="agent-tools"></span>
        </span>
      </button>`;
    li.querySelector('.agent-name-text').textContent = agent.name;
    if (agent.managed) {
      const badge = document.createElement('span');
      badge.className = 'agent-badge';
      badge.textContent = 'nexo';
      badge.title = 'Lo instala y mantiene nexo';
      li.querySelector('.agent-name').append(badge);
    }
    li.querySelector('.agent-model').textContent = modelLabel(agent.model);
    li.querySelector('.agent-effort').textContent = effortLabel(agent.effort);
    li.querySelector('.agent-desc').textContent = agent.description;
    const tools = li.querySelector('.agent-tools');
    tools.textContent = agent.tools ? `Herramientas: ${toolsLabel(agent.tools)}` : 'Todas las herramientas de la sesión';
    const main = li.querySelector('.agent-main');
    main.setAttribute('aria-expanded', String(open.has(agent.id)));
    main.title = open.has(agent.id) ? '' : 'Ver el resumen completo';
    main.addEventListener('click', () => {
      const now = !open.has(agent.id);
      if (now) open.add(agent.id);
      else open.delete(agent.id);
      li.classList.toggle('open', now);
      main.setAttribute('aria-expanded', String(now));
      main.title = now ? '' : 'Ver el resumen completo';
    });
    if (!agent.managed) {
      const actions = document.createElement('div');
      actions.className = 'row-actions';
      actions.innerHTML = `<button type="button" class="row-action">${TRASH_ICON}</button>`;
      const remove = actions.querySelector('button');
      remove.setAttribute('aria-label', `Mover ${agent.name} a la papelera`);
      remove.title = 'Mover a la papelera';
      remove.addEventListener('click', () => removeAgent(agent));
      li.append(actions);
    }
    return li;
  }

  function pendingRow(item) {
    const li = document.createElement('li');
    li.className = 'agent-row agent-pending';
    li.innerHTML = `
      <div class="agent-main">
        <span class="row-icon"><span class="skill-pending-dot" aria-hidden="true"></span></span>
        <span class="agent-text">
          <span class="agent-name">Creando agente…</span>
          <span class="agent-specs">
            <span class="agent-spec"></span>
            <span class="agent-spec"></span>
          </span>
          <span class="agent-desc"></span>
        </span>
      </div>`;
    const specs = li.querySelectorAll('.agent-spec');
    specs[0].textContent = modelLabel(item.model);
    specs[1].textContent = effortLabel(item.effort);
    li.querySelector('.agent-desc').textContent = item.text;
    li.title = 'creador-de-skills está trabajando en la terminal de Claude Code';
    return li;
  }

  async function removeAgent(agent) {
    const ok = await ask(`¿Mover el agente «${agent.name}» a la papelera? Claude Code dejará de tenerlo en la próxima sesión.`, {
      title: 'Borrar agente',
      kind: 'warning',
      okLabel: 'Mover a la papelera',
      cancelLabel: 'Cancelar',
    });
    if (!ok) return;
    try {
      await invoke('agents_remove', { id: agent.id });
    } catch (error) {
      fail(error);
    }
    load();
  }

  // ---------- «Agregar agente» ----------
  const picked = (attr) => document.querySelector(`[${attr}][aria-pressed="true"]`)?.getAttribute(attr);
  function openAdd() {
    showTab('agentes');
    form.hidden = false;
    $('agent-add').hidden = true;
    requestAnimationFrame(() => input.focus());
  }
  function closeAdd() {
    form.hidden = true;
    $('agent-add').hidden = false;
    input.value = '';
  }
  $('agent-add').addEventListener('click', openAdd);
  $('agent-create-cancel').addEventListener('click', closeAdd);
  input.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') closeAdd();
    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      form.requestSubmit();
    }
  });
  form.addEventListener('submit', async (event) => {
    event.preventDefault();
    const text = input.value.replace(/\s+/g, ' ').trim();
    if (!text) return input.focus();
    const model = picked('data-agent-model') ?? 'claude-sonnet-5-5';
    const effort = picked('data-agent-effort') ?? 'medium';
    const ok = $('agent-create-ok');
    ok.disabled = true;
    try {
      await onCreate({ text, model, effort });
      pending.push({ key: String(Date.now()), text, model, effort, since: Date.now() });
      closeAdd();
      render();
    } catch (error) {
      fail(error);
    } finally {
      ok.disabled = false;
    }
  });

  return {
    load,
    showTab,
    openAdd,
    get count() {
      return agents.length;
    },
  };
}
