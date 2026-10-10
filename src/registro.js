// Registro de agentes (centro de la vista Agentes): lo que gastó cada agente de Claude Code
// del vault en la sesión de 5 h actual del plan (tokens y su parte de la sesión) y, al abrir
// uno, su contexto como /context. Los datos vienen de `agent_log` y `agent_context`
// (registro.rs); cuando la sesión de 5 h se renueva, el registro empieza vacío.
import { reconcile } from './dom.js';
import { modelLabel, effortLabel } from './agents.js';

// ---------- formatos (puros, se prueban con node) ----------
const num = (value, digits) =>
  value.toLocaleString('es', { minimumFractionDigits: 0, maximumFractionDigits: digits });

// 812 → «812», 48 120 → «48,1 k», 2 533 170 → «2,53 M».
export function formatTokens(n) {
  const v = Math.max(0, Number(n) || 0);
  if (v < 1000) return String(Math.round(v));
  if (v < 1e6) return `${num(v / 1000, v < 100_000 ? 1 : 0)} k`;
  return `${num(v / 1e6, v < 10e6 ? 2 : 1)} M`;
}

// Parte de la sesión de 5 h, en puntos porcentuales: «≈ 6,1 %», «< 0,1 %», «—» sin dato.
export function formatShare(p) {
  if (p == null || !Number.isFinite(p)) return '—';
  if (p <= 0) return '0 %';
  if (p < 0.1) return '< 0,1 %';
  return `≈ ${num(p, p < 10 ? 1 : 0)} %`;
}

export function formatUsd(usd) {
  const v = Number(usd) || 0;
  return `${num(v, v < 1 ? 3 : 2)} US$`;
}

export const totalTokens = (t) => (t ? t.input + t.output + t.cache_read + t.cache_write : 0);

// Categorías de /context (los nombres que da Claude Code) → etiqueta y color de nexo.
// Los colores son neones suaves; la conversación, blanca.
export const CATEGORY = {
  'System prompt': { label: 'Instrucciones del sistema', color: '#bfa3fe' },
  'System tools': { label: 'Herramientas', color: '#60d9ff' },
  'MCP tools': { label: 'Herramientas MCP', color: '#8fa8ff' },
  'MCP server instructions': { label: 'Instrucciones MCP', color: '#9ff0ff' },
  'Custom agents': { label: 'Agentes', color: '#fd80c9' },
  'Memory files': { label: 'Memoria', color: '#ffdbe5' },
  Skills: { label: 'Skills', color: '#6ef8d0' },
  'Initial context': { label: 'Al empezar', color: '#c9a8ff' },
  Messages: { label: 'Conversación', color: '#fdfdfe' },
  'Autocompact buffer': { label: 'Reserva para compactar' },
  'Compact buffer': { label: 'Reserva para compactar' },
  'Free space': { label: 'Libre' },
};
const DEFERRED = {
  'System tools (deferred)': 'Herramientas que se cargan al usarlas',
  'MCP tools (deferred)': 'Herramientas MCP que se cargan al usarlas',
};

export function categoryLabel(name, kind = 'main') {
  if (name === 'Messages' && kind === 'sub') return 'Encargo y trabajo';
  return CATEGORY[name]?.label ?? DEFERRED[name] ?? name;
}

// Tramos de la barra del contexto, en % de la ventana: uno por categoría usada, a escala pero
// con un mínimo para que se vea aunque sea diminuta, y la reserva para compactar al final
// (solo lo que quede libre de ella). Si los mínimos no caben, los tramos usados se encogen
// para que todo sume como mucho 100.
export function contextSegments(categories, max, min = 1.2) {
  const list = categories ?? [];
  const pct = (tokens) => (max > 0 ? (tokens / max) * 100 : 0);
  const used = list.filter((c) => c.kind === 'used' && c.tokens > 0);
  const segments = used.map((c) => ({ name: c.name, kind: 'used', pct: Math.max(min, pct(c.tokens)) }));
  const buffer = list.find((c) => c.kind === 'buffer' && c.tokens > 0);
  const free = 100 - used.reduce((sum, c) => sum + pct(c.tokens), 0);
  const reserve = buffer ? Math.max(0, Math.min(pct(buffer.tokens), free)) : 0;
  const total = segments.reduce((sum, c) => sum + c.pct, 0);
  if (total > 100 - reserve) for (const c of segments) c.pct *= (100 - reserve) / total;
  if (reserve > 0) segments.push({ name: buffer.name, kind: 'buffer', pct: reserve });
  return segments;
}

// Parte de la ventana de contexto: «12 %», «0,7 %», «< 0,1 %».
export function percent(p) {
  if (!(p > 0)) return '0 %';
  if (p < 0.1) return '< 0,1 %';
  return `${num(p, p < 10 ? 1 : 0)} %`;
}

const clock = (ms) => new Date(ms).toLocaleTimeString('es', { hour: '2-digit', minute: '2-digit' });

// «en 2 h 05 min», «en 12 min».
export function untilLabel(ms, now = Date.now()) {
  const minutes = Math.max(0, Math.round((ms - now) / 60000));
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return h ? `en ${h} h ${String(m).padStart(2, '0')} min` : `en ${m} min`;
}

// ---------- interfaz ----------
const CHEVRON =
  '<svg width="14" height="14" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="m8 5 5 5-5 5"/></svg>';
const BACK =
  '<svg width="14" height="14" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="m12 5-5 5 5 5"/></svg>';
const DOWN =
  '<svg width="14" height="14" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="m5 8 5 5 5-5"/></svg>';

const POLL_MS = 5000;
const REMEASURE_MS = 45000;

export function createAgentLog({ invoke, fail }) {
  const $ = (id) => document.getElementById(id);
  const root = $('agent-log');
  const listView = $('log-list-view');
  const detail = $('log-detail');
  let log = null;
  let timer = 0;
  let active = false;
  let loading = false;
  let openId = null;
  let context = null; // { id, data } | { id, error } | { id, loading }

  async function load(force = false) {
    if (loading) return;
    loading = true;
    $('log-refresh').classList.add('loading');
    try {
      log = await invoke('agent_log', { force });
    } catch (error) {
      log = { window: null, used: null, error: String(error), agents: [], machine_cost: 0, nexo_cost: 0, nexo_share: null };
    } finally {
      loading = false;
      $('log-refresh').classList.remove('loading');
    }
    render();
  }

  function schedule() {
    clearTimeout(timer);
    if (!active) return;
    timer = setTimeout(async () => {
      if (document.visibilityState === 'visible') await load();
      schedule();
    }, POLL_MS);
  }

  // La vista Agentes está a la vista: se lee ya y cada pocos segundos; si no, nada.
  function activate(on) {
    if (on === active) return;
    active = on;
    root.hidden = !on;
    if (on) {
      load();
      schedule();
    } else clearTimeout(timer);
  }

  // ---------- resumen de la sesión ----------
  function renderSession() {
    const used = log?.used;
    const win = log?.window;
    const usedEl = $('log-used');
    usedEl.textContent = used == null ? '—' : num(used, used < 10 ? 1 : 0);
    // Claude da el porcentaje cada pocos minutos; entre lecturas se suma lo que gastaron los
    // agentes de este equipo.
    usedEl.title = log?.checked
      ? `Claude lo dio a las ${clock(log.checked)}; desde entonces se suma lo que gastaron los agentes de este equipo.`
      : '';
    $('log-used-unit').hidden = used == null;
    $('log-reset').textContent = win
      ? `Se renueva ${win.exact ? 'a' : 'hacia'} las ${clock(win.end)} · ${untilLabel(win.end)}`
      : log
        ? 'Sin sesión en curso'
        : '';
    const nexo = log?.nexo_share;
    const rest = used != null && nexo != null ? Math.max(0, used - nexo) : null;
    $('log-nexo').textContent = nexo == null ? '—' : percent(nexo);
    $('log-rest').textContent = rest == null ? '—' : percent(rest);
    $('log-bar-nexo').style.width = `${Math.min(100, nexo ?? 0)}%`;
    $('log-bar-rest').style.width = `${Math.min(100 - Math.min(100, nexo ?? 0), rest ?? 0)}%`;
    const note = $('log-note');
    note.textContent = log?.error
      ? `No se pudo leer el uso del plan: ${log.error} Mientras, la sesión de 5 h se deduce de tus mensajes en este equipo, sin porcentaje.`
      : log?.note
        ? log.note
        : win && !win.exact
          ? 'Sesión deducida de tus mensajes en este equipo: el porcentaje llegará con la próxima lectura de Claude.'
          : '';
    note.hidden = !note.textContent;
  }

  // ---------- filas ----------
  function timeLabel(agent) {
    if (agent.active) return `desde las ${clock(agent.started)}`;
    const a = clock(agent.started);
    const b = clock(agent.last);
    return a === b ? a : `${a} – ${b}`;
  }

  function row(agent) {
    const li = document.createElement('li');
    li.className = 'log-row';
    li.classList.toggle('active', agent.active);
    const share = agent.share ?? 0;
    const used = log?.used || 0;
    li.style.setProperty('--share', `${used > 0 ? Math.min(100, (share / used) * 100) : 0}%`);
    li.innerHTML = `
      <button type="button" class="log-main">
        <span class="log-status" aria-hidden="true"></span>
        <span class="log-text">
          <span class="log-name"><span class="log-name-text"></span><span class="agent-spec log-model"></span></span>
          <span class="log-sub"></span>
        </span>
        <span class="log-num">
          <span class="log-tokens"><b></b> tokens</span>
          <span class="log-share"><b></b> de la sesión</span>
        </span>
        <span class="log-chevron">${CHEVRON}</span>
      </button>`;
    li.querySelector('.log-name-text').textContent = agent.name;
    li.querySelector('.log-model').textContent = modelLabel(agent.model);
    const sub = [agent.title, timeLabel(agent)].filter(Boolean).join(' · ');
    li.querySelector('.log-sub').textContent = sub;
    li.querySelector('.log-sub').title = sub;
    li.querySelector('.log-tokens b').textContent = formatTokens(totalTokens(agent.tokens));
    li.querySelector('.log-share b').textContent = formatShare(agent.share);
    const main = li.querySelector('.log-main');
    main.setAttribute(
      'aria-label',
      `${agent.name}, ${agent.active ? 'activo' : 'terminado'}, ${formatTokens(totalTokens(agent.tokens))} tokens, ${formatShare(agent.share)} de la sesión. Ver detalles`,
    );
    main.addEventListener('click', () => openDetail(agent.id));
    return li;
  }

  function renderList() {
    const agents = log?.agents ?? [];
    const running = agents.filter((a) => a.active);
    const past = agents.filter((a) => !a.active);
    const opts = {
      key: (a) => a.id,
      sig: (a) =>
        [a.name, a.model, a.title, a.active, a.calls, a.share?.toFixed(2), totalTokens(a.tokens), a.last, log?.used].join('|'),
      build: row,
    };
    reconcile($('log-active'), running, opts);
    reconcile($('log-past'), past, opts);
    $('log-active-sec').hidden = running.length === 0;
    $('log-past-sec').hidden = past.length === 0;
    $('log-active-count').textContent = `· ${running.length}`;
    $('log-past-count').textContent = `· ${past.length}`;
    const empty = $('log-empty');
    empty.hidden = agents.length > 0;
    if (!agents.length) {
      $('log-empty-title').textContent = log?.window ? 'Sin agentes en esta sesión' : 'No hay una sesión de 5 h en curso';
      $('log-empty-text').textContent = log?.window
        ? 'Cuando Claude Code o uno de tus agentes trabaje en el vault aparecerá aquí, con sus tokens y su parte de la sesión.'
        : 'Empieza con tu próximo mensaje a Claude. Al renovarse la sesión, el registro se vacía.';
    }
  }

  function render() {
    renderSession();
    renderList();
    if (openId) renderDetail();
  }

  // ---------- detalle: /context y uso ----------
  async function openDetail(id) {
    openId = id;
    listView.hidden = true;
    detail.hidden = false;
    root.querySelector('.log-scroll').scrollTop = 0;
    renderDetail();
    measure(id);
    requestAnimationFrame(() => $('log-back')?.focus());
  }

  function closeDetail() {
    const id = openId;
    openId = null;
    detail.hidden = true;
    listView.hidden = false;
    detail.replaceChildren();
    root.querySelector(`.log-row[data-key="${CSS.escape(id ?? '')}"] .log-main`)?.focus();
  }

  // `quiet`: se vuelve a medir sin borrar lo que se ve (el agente avanzó o terminó).
  async function measure(id, quiet = false) {
    const agent = log?.agents?.find((a) => a.id === id);
    const seen = { calls: agent?.calls, active: agent?.active, at: Date.now() };
    if (quiet && context?.data) {
      context = { ...context, ...seen, refreshing: true };
      $('ld-remeasure')?.classList.add('loading');
    } else {
      context = { id, loading: true, ...seen };
      renderContext();
    }
    try {
      const data = await invoke('agent_context', { id });
      if (openId === id) context = { id, data, ...seen };
    } catch (error) {
      if (openId === id && !quiet) context = { id, error: String(error), ...seen };
      else if (openId === id) context = { ...context, refreshing: false };
    }
    if (openId === id) renderContext();
  }

  // El contexto que se ve es de cuando se midió: si el agente siguió (más llamadas) o
  // terminó, se vuelve a medir, como mucho cada REMEASURE_MS mientras trabaja.
  function stale(agent) {
    if (!context?.data || context.refreshing || context.id !== agent.id) return false;
    if (agent.calls === context.calls && agent.active === context.active) return false;
    return !agent.active || Date.now() - context.at > REMEASURE_MS;
  }

  function renderDetail() {
    const agent = log?.agents?.find((a) => a.id === openId);
    if (!detail.firstChild) {
      detail.innerHTML = `
        <header class="log-detail-head">
          <button type="button" class="log-back" id="log-back">${BACK}<span>Registro</span></button>
          <h2 class="log-detail-name" id="ld-name"></h2>
          <div class="log-chips" id="ld-specs"></div>
          <p class="log-detail-title" id="ld-title"></p>
        </header>
        <section class="log-sec" aria-labelledby="ld-ctx-h">
          <div class="log-sec-head">
            <h3 class="log-h" id="ld-ctx-h">Contexto</h3>
            <span class="log-sec-aside mono" id="ld-ctx-total"></span>
            <button type="button" class="icon-btn log-remeasure" id="ld-remeasure" aria-label="Volver a medir" title="Volver a medir"><svg width="14" height="14" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M16 10a6 6 0 1 1-1.8-4.3M16 4v3.5h-3.5"/></svg></button>
          </div>
          <div class="log-context" id="ld-context"></div>
        </section>
        <section class="log-sec" aria-labelledby="ld-use-h">
          <div class="log-sec-head">
            <h3 class="log-h" id="ld-use-h">En esta sesión de 5 h</h3>
            <span class="log-sec-aside" id="ld-share"></span>
          </div>
          <dl class="log-stats" id="ld-usage"></dl>
          <div class="log-models" id="ld-models"></div>
          <p class="log-foot" id="ld-cost"></p>
        </section>
        <div class="log-more" id="ld-more"></div>`;
      $('log-back').addEventListener('click', closeDetail);
      $('ld-remeasure').addEventListener('click', () => openId && measure(openId));
    }
    if (!agent) {
      // La sesión de 5 h se renovó (o el agente salió de la ventana): vuelta al registro.
      if (log) closeDetail();
      return;
    }
    $('ld-name').textContent = agent.name;
    $('ld-name').title = agent.kind === 'main' ? 'Agente principal' : 'Subagente';
    const chip = (text, extra) => {
      const s = document.createElement('span');
      s.className = `log-chip ${extra ?? ''}`;
      s.textContent = text;
      return s;
    };
    $('ld-specs').replaceChildren(
      chip(modelLabel(agent.model)),
      ...(agent.kind === 'sub' ? [chip(effortLabel(agent.effort))] : []),
      agent.active ? chip('Activo', 'status on') : chip('Terminado', 'status'),
    );
    $('ld-title').textContent = [agent.title, timeLabel(agent)].filter(Boolean).join(' · ');

    $('ld-share').textContent = `${formatShare(agent.share)} de la sesión`;
    const t = agent.tokens;
    const usage = [
      ['Total', totalTokens(t), 'Todo lo que leyó y escribió el modelo'],
      ['Salida', t.output, agent.kind === 'sub' ? 'Lo que escribió (estimado con su texto: Claude Code no guarda la cifra final de los subagentes)' : 'Lo que escribió, pensamiento incluido'],
      ['Caché leída', t.cache_read, 'Contexto que ya estaba en caché: lo más barato'],
      ['Caché escrita', t.cache_write, 'Contexto nuevo que se guardó en caché'],
      ['Entrada', t.input, 'Contexto enviado sin caché'],
    ];
    $('ld-usage').replaceChildren(
      ...usage.map(([label, value, hint]) => {
        const div = document.createElement('div');
        div.className = 'log-stat';
        div.title = hint;
        div.innerHTML = '<dt></dt><dd></dd>';
        div.querySelector('dt').textContent = label;
        div.querySelector('dd').textContent = formatTokens(value);
        return div;
      }),
    );
    const models = $('ld-models');
    models.hidden = (agent.models?.length ?? 0) < 2;
    if (!models.hidden) {
      models.replaceChildren(
        ...agent.models.map((m) => {
          const line = document.createElement('div');
          line.className = 'log-model-line';
          line.innerHTML = '<span></span><span class="mono"></span>';
          line.children[0].textContent = modelLabel(m.model);
          line.children[1].textContent = `${formatTokens(totalTokens(m.tokens))} · ${m.calls} ${m.calls === 1 ? 'llamada' : 'llamadas'}`;
          return line;
        }),
      );
    }
    const cost = $('ld-cost');
    cost.textContent = `${agent.calls} ${agent.calls === 1 ? 'llamada' : 'llamadas'} al modelo · en la API costaría ${formatUsd(agent.cost)}`;
    cost.title = 'Su parte de la sesión se calcula con este coste frente al de todo Claude Code de este equipo.';
    if (context?.id !== openId) renderContext();
    else if (stale(agent)) measure(agent.id, true);
  }

  function legendItem(c, kind, max) {
    const li = document.createElement('li');
    li.className = 'log-ctx-item';
    li.innerHTML = '<span class="log-swatch" aria-hidden="true"></span><span class="log-ctx-name"></span><span class="log-ctx-nums"><b></b><span class="mono"></span></span>';
    li.firstChild.style.setProperty('--c', CATEGORY[c.name]?.color ?? '#f5f5f5');
    const label = categoryLabel(c.name, kind);
    li.children[1].textContent = label;
    li.children[1].title = label;
    li.querySelector('b').textContent = formatTokens(c.tokens);
    li.querySelector('.mono').textContent = percent((c.tokens / max) * 100);
    return li;
  }

  function renderContext() {
    const box = $('ld-context');
    if (!box) return;
    const total = $('ld-ctx-total');
    const remeasure = $('ld-remeasure');
    remeasure.disabled = !!(context?.loading || context?.refreshing);
    remeasure.classList.toggle('loading', !!context?.refreshing);
    if (!context || context.id !== openId || context.loading) {
      box.innerHTML = '<p class="log-measuring">Midiendo el contexto…</p>';
      total.textContent = '';
      $('ld-more').replaceChildren();
      return;
    }
    if (context.error) {
      box.innerHTML = '<p class="log-measuring"></p>';
      box.firstChild.textContent = `No se pudo medir: ${context.error}`;
      total.textContent = '';
      return;
    }
    const data = context.data;
    const kind = data.kind;
    const max = data.maxTokens || 1;
    total.textContent = `${formatTokens(data.totalTokens)} / ${formatTokens(max)} · ${num(data.percentage ?? (data.totalTokens / max) * 100, 0)} %`;
    const categories = data.categories ?? [];

    const bar = document.createElement('div');
    bar.className = 'log-ctx-bar';
    bar.setAttribute('role', 'img');
    bar.setAttribute('aria-label', `Contexto: ${formatTokens(data.totalTokens)} de ${formatTokens(max)} tokens`);
    const segments = contextSegments(categories, max);
    for (const seg of segments) {
      const span = document.createElement('span');
      span.className = `log-seg ${seg.kind}`;
      span.style.width = `${seg.pct}%`;
      if (seg.kind === 'used') span.style.setProperty('--c', CATEGORY[seg.name]?.color ?? '#f5f5f5');
      span.title = categoryLabel(seg.name, kind);
      if (seg.kind === 'buffer') {
        const free = document.createElement('span');
        free.className = 'log-ctx-free';
        bar.append(free);
      }
      bar.append(span);
    }

    const legend = document.createElement('ul');
    legend.className = 'log-ctx-legend';
    for (const c of categories) if (c.kind === 'used' && c.tokens > 0) legend.append(legendItem(c, kind, max));

    // Lo que no ocupa tramo de color: el espacio libre, la reserva y lo que se carga al usarlo.
    const foot = document.createElement('p');
    foot.className = 'log-ctx-foot';
    const HINT = {
      buffer: 'Espacio que Claude Code deja libre para poder resumir la conversación cuando se llene',
      deferred: 'No ocupan contexto hasta que el modelo las pide',
    };
    for (const c of categories) {
      if (c.kind === 'used' || !c.tokens) continue;
      const span = document.createElement('span');
      span.innerHTML = '<span></span> <b></b>';
      span.firstChild.textContent = categoryLabel(c.name, kind);
      span.querySelector('b').textContent = formatTokens(c.tokens);
      if (HINT[c.kind]) span.title = HINT[c.kind];
      foot.append(span);
    }

    const parts = [bar, legend];
    if (foot.childElementCount) parts.push(foot);
    if (!data.exact) {
      const note = document.createElement('p');
      note.className = 'log-foot';
      note.textContent =
        data.categories?.[0]?.name === 'Initial context'
          ? 'El total es el de su última llamada. Sin su configuración no se puede repartir más.'
          : 'El total es exacto (su última llamada); el reparto entre categorías se calcula con la configuración actual del agente.';
      parts.push(note);
    }
    box.replaceChildren(...parts);
    renderMore(data);
  }

  // Debajo, como en /context: la conversación por tipo y listas plegables.
  function renderMore(data) {
    const more = $('ld-more');
    const parts = [];
    const groups = [];
    const breakdown = data.messageBreakdown;
    const messages = data.categories?.find((c) => c.name === 'Messages')?.tokens ?? 0;
    if (breakdown && messages > 0) {
      const rows = [
        ['Tus mensajes', breakdown.userMessageTokens],
        ['Respuestas', breakdown.assistantMessageTokens],
        ['Llamadas a herramientas', breakdown.toolCallTokens],
        ['Resultados de herramientas', breakdown.toolResultTokens],
        ['Adjuntos y avisos del sistema', breakdown.attachmentTokens],
      ].filter(([, v]) => v > 0);
      // Claude Code estima cada tipo por separado y la suma no coincide con el total real de la
      // conversación: se reparte ese total (exacto) en la misma proporción.
      const scale = messages / (rows.reduce((sum, [, v]) => sum + v, 0) || 1);
      const scaled = (list) => list.map(([name, v]) => [name, v * scale]);
      const section = document.createElement('section');
      section.className = 'log-sec';
      section.innerHTML = '<div class="log-sec-head"><h3 class="log-h">Conversación</h3></div>';
      section.querySelector('.log-h').title = 'Reparto estimado por tipo';
      section.append(list(scaled(rows)));
      parts.push(section);
      const attachments = (breakdown.attachmentsByType ?? []).map((a) => [a.name.replace(/_/g, ' '), a.tokens]);
      if (attachments.length) groups.push({ key: 'attachments', title: 'Adjuntos', note: 'Estimado', rows: scaled(attachments) });
      const tools = (breakdown.toolCallsByType ?? []).map((t) => [t.name, (t.callTokens ?? 0) + (t.resultTokens ?? 0)]);
      if (tools.length) groups.push({ key: 'tools', title: `Herramientas · ${tools.length}`, note: 'Estimado', rows: scaled(tools) });
    }
    const skills = data.skills?.skillFrontmatter ?? [];
    if (skills.length) groups.push({ key: 'skills', title: `Skills · ${data.skills.includedSkills ?? skills.length}`, rows: skills.map((s) => [s.name, s.tokens]) });
    const agents = data.agents ?? [];
    if (agents.length) groups.push({ key: 'agents', title: `Agentes · ${agents.length}`, rows: agents.map((a) => [a.agentType, a.tokens]) });
    const memory = data.memoryFiles ?? [];
    if (memory.length) groups.push({ key: 'memory', title: 'Memoria', rows: memory.map((m) => [m.path.replace(/^.*\/(\.claude|projects)\//, '…/'), m.tokens]) });
    const mcp = data.mcpTools ?? [];
    if (mcp.length) groups.push({ key: 'mcp', title: `MCP · ${mcp.length}`, rows: mcp.map((t) => [t.name, t.tokens]) });

    // Al volver a medir se conserva lo que el usuario abrió o cerró.
    const wasOpen = new Map([...more.querySelectorAll('details')].map((d) => [d.dataset.key, d.open]));
    if (groups.length) {
      const box = document.createElement('div');
      box.className = 'log-groups';
      box.append(
        ...groups.map((g) => {
          const det = document.createElement('details');
          det.className = 'log-group';
          det.dataset.key = g.key;
          det.open = wasOpen.get(g.key) ?? false;
          const summary = document.createElement('summary');
          summary.innerHTML = `<span></span><span class="log-group-total"></span>${DOWN}`;
          summary.children[0].textContent = g.title;
          if (g.note) summary.title = g.note;
          summary.children[1].textContent = `${g.note ? '≈ ' : ''}${formatTokens(g.rows.reduce((s, [, v]) => s + (v || 0), 0))}`;
          det.append(summary, list(g.rows));
          return det;
        }),
      );
      parts.push(box);
    }
    more.replaceChildren(...parts);
  }

  function list(rows) {
    const ul = document.createElement('ul');
    ul.className = 'log-rows';
    for (const [name, tokens] of [...rows].sort((a, b) => (b[1] || 0) - (a[1] || 0))) {
      const li = document.createElement('li');
      li.innerHTML = '<span></span><span></span>';
      li.children[0].textContent = name;
      li.children[0].title = name;
      li.children[1].textContent = formatTokens(tokens);
      ul.append(li);
    }
    return ul;
  }

  $('log-refresh').addEventListener('click', () => load(true));
  root.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && openId) {
      event.stopPropagation();
      closeDetail();
    }
  });

  return { activate, load, closeDetail };
}
