// Reparto de la salida del orquestador entre las ventanas de un workspace.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { STATE_LABEL, agentState, createWorkspaceStream, endNotice, endStatus, usageTokens } from '../src/ws-stream.js';
import { createChatModel } from '../src/chat-model.js';

const spawn = (id, input) => ({ type: 'assistant', message: { id: `m-${id}`, model: 'claude-opus-5-5', content: [{ type: 'tool_use', id, name: 'Agent', input }], usage: { input_tokens: 10, output_tokens: 5, cache_read_input_tokens: 100, cache_creation_input_tokens: 0 } } });
const started = (task, tool, extra = {}) => ({ type: 'system', subtype: 'task_started', task_id: task, tool_use_id: tool, description: 'Revisor de la API', subagent_type: 'general-purpose', task_type: 'local_agent', prompt: 'Revisa src/api', ...extra });
const sub = (tool, content, id = 'ms1') => ({ type: 'assistant', parent_tool_use_id: tool, message: { id, model: 'claude-sonnet-5-5', content, usage: { input_tokens: 1, output_tokens: 2 } } });
const to = (out) => out.targets.map((t) => t.to);

test('lo de cada subagente va a su ventana y lo demás al orquestador', () => {
  const s = createWorkspaceStream();
  assert.deepEqual(to(s.route({ type: 'system', subtype: 'init', session_id: 'x' })), ['main']);
  assert.deepEqual(to(s.route(spawn('toolu_1', { description: 'Revisor de la API', subagent_type: 'general-purpose', prompt: 'Revisa src/api' }))), ['main']);
  const born = s.route(started('a1', 'toolu_1'));
  assert.deepEqual(born.created, ['a1']);
  assert.deepEqual(born.targets, []);
  const agent = s.get('a1');
  assert.equal(agent.name, 'Revisor de la API');
  assert.equal(agentState(agent), 'active');
  const line = s.route(sub('toolu_1', [{ type: 'tool_use', id: 'toolu_r', name: 'Read', input: { file_path: '/p/src/api.js' } }]));
  assert.deepEqual(to(line), ['a1']);
  assert.equal('parent_tool_use_id' in line.targets[0].msg, false, 'en su ventana es una línea de primer nivel');
  // Su permiso (por el agent_id o por la herramienta que lo pide) también va a su ventana.
  const perm = { type: 'control_request', request_id: 'r1', request: { subtype: 'can_use_tool', tool_name: 'Read', tool_use_id: 'toolu_r', input: {} } };
  assert.deepEqual(to(s.route(perm)), ['a1']);
  assert.deepEqual(to(s.route({ type: 'nexo_permission', request_id: 'r1', decision: 'allow' })), ['a1']);
  assert.deepEqual(to(s.route({ type: 'control_request', request_id: 'r2', request: { subtype: 'can_use_tool', tool_name: 'Bash', agent_id: 'a1', input: {} } })), ['a1']);
  assert.deepEqual(to(s.route({ type: 'control_request', request_id: 'r3', request: { subtype: 'can_use_tool', tool_name: 'Bash', tool_use_id: 'otro', input: {} } })), ['main']);
  assert.equal(s.byToolUse('toolu_1').id, 'a1');
});

test('el estado y los tokens salen de las líneas de tareas', () => {
  const s = createWorkspaceStream();
  s.route(spawn('toolu_1', {}));
  s.route(started('a1', 'toolu_1'));
  s.route({ type: 'system', subtype: 'task_progress', task_id: 'a1', usage: { total_tokens: 4820, tool_uses: 3, duration_ms: 9000 }, last_tool_name: 'Grep', summary: 'Buscando los endpoints' });
  const agent = s.get('a1');
  assert.equal(s.tokensOf(agent), 4820);
  assert.equal(agent.lastTool, 'Grep');
  assert.equal(agentState(agent, true), 'waiting');
  const done = s.route({ type: 'system', subtype: 'task_notification', task_id: 'a1', status: 'completed', summary: 'Listo', usage: { total_tokens: 9100, tool_uses: 7, duration_ms: 20000 } });
  assert.deepEqual([...done.changed], ['a1']);
  assert.equal(agent.status, 'completed');
  assert.equal(agentState(agent), 'done');
  assert.deepEqual(done.ended, ['a1']);
  assert.deepEqual(done.targets, [], 'el que terminó bien no lleva nota');
  assert.equal(s.tokensOf(agent), 9100);
  // Retomado con SendMessage: la misma tarea vuelve a estar activa y no se duplica.
  const again = s.route(started('a1', 'toolu_2'));
  assert.deepEqual(again.created, []);
  assert.equal(agentState(agent), 'active');
  assert.deepEqual(to(s.route(sub('toolu_2', [{ type: 'text', text: 'Sigo.' }], 'ms2'))), ['a1']);
  // Las tareas que no son agentes (órdenes en segundo plano) no crean ventana.
  assert.deepEqual(s.route({ type: 'system', subtype: 'task_started', task_id: 'b1', task_type: 'local_bash', description: 'npm test' }).created, []);
  assert.equal(s.mainTokens(), 115);
  assert.equal(usageTokens({ input_tokens: 1, output_tokens: 2, cache_read_input_tokens: 3, cache_creation_input_tokens: 4 }), 10);
  assert.deepEqual(s.exit(), ['a1']);
});

test('lo que llega antes de su tarea espera y, si la tarea no llega, sale de la llamada a Agent', () => {
  const s = createWorkspaceStream();
  s.route(spawn('toolu_9', { description: 'Buscador', subagent_type: 'Explore', prompt: 'Busca' }));
  assert.deepEqual(to(s.route(sub('toolu_9', [{ type: 'text', text: 'Busco.' }]))), [], 'espera');
  const result = s.route({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_9', content: 'hecho' }] } });
  assert.deepEqual(result.created, ['tool:toolu_9']);
  assert.deepEqual(to(result), ['tool:toolu_9', 'main']);
  assert.equal(s.get('tool:toolu_9').type, 'Explore');
  // Y si la tarea llega a tiempo, lo que esperaba va a su ventana.
  const t = createWorkspaceStream();
  t.route(spawn('toolu_5', {}));
  t.route(sub('toolu_5', [{ type: 'text', text: 'Hola' }]));
  assert.deepEqual(to(t.route(started('a5', 'toolu_5'))), ['a5']);
});

test('los mensajes al agente van a su ventana y al orquestador, que es quien se los pasa', () => {
  const s = createWorkspaceStream();
  s.route(spawn('toolu_1', {}));
  s.route(started('a1', 'toolu_1'));
  const out = s.route({ type: 'nexo_user', text: 'Mira los tests', files: [], to: 'a1' });
  assert.deepEqual(to(out), ['main', 'a1']);
  assert.equal(out.targets[0].msg.toName, 'Revisor de la API');
  assert.equal(out.targets[1].msg.local, true, 'en su ventana no empieza un turno');
});

test('la ventana de un agente pinta su encargo, su trabajo y sus permisos', () => {
  const s = createWorkspaceStream();
  const agentModel = createChatModel({ prompts: true });
  const main = createChatModel();
  const apply = (out) => out.targets.forEach((t) => (t.to === 'main' ? main : agentModel).apply(t.msg));
  apply(s.route(spawn('toolu_1', {})));
  apply(s.route(started('a1', 'toolu_1')));
  apply(s.route({ type: 'user', parent_tool_use_id: 'toolu_1', message: { role: 'user', content: 'Mira también los tests' } }));
  apply(s.route(sub('toolu_1', [{ type: 'tool_use', id: 'toolu_r', name: 'Read', input: { file_path: '/p/a.js' } }])));
  apply(s.route({ type: 'control_request', request_id: 'r1', request: { subtype: 'can_use_tool', tool_name: 'Read', tool_use_id: 'toolu_r', input: { file_path: '/p/a.js' } } }));
  const kinds = agentModel.state.items.map((i) => i.kind);
  assert.deepEqual(kinds, ['user', 'tool']);
  assert.equal(agentModel.state.items[0].from, 'Orquestador');
  assert.equal(agentModel.pending().length, 1);
  assert.equal(main.state.items.length, 1, 'en el orquestador queda la llamada a Agent');
  assert.equal(main.state.items[0].children.length, 0, 'sin repetir lo que hizo el agente');
});

// Una conversación real (Claude Code 2.1.301, Haiku): el orquestador despliega un agente
// Explore en segundo plano, termina su turno, el agente trabaja y avisa al acabar, y el
// orquestador responde en un turno nuevo.
test('una conversación real con un agente en segundo plano se reparte bien', async () => {
  const { readFileSync } = await import('node:fs');
  const lines = readFileSync(new URL('./fixtures/workspace/agente-en-segundo-plano.jsonl', import.meta.url), 'utf8').trim().split('\n').map((l) => JSON.parse(l));
  const s = createWorkspaceStream();
  const main = createChatModel();
  const sub = createChatModel({ prompts: true });
  const states = [];
  for (const line of lines) {
    const out = s.route(line);
    for (const t of out.targets) (t.to === 'main' ? main : sub).apply(t.msg);
    const agent = [...s.agents.values()][0];
    if (agent) states.push(agentState(agent));
  }
  assert.equal(s.agents.size, 1);
  const agent = [...s.agents.values()][0];
  assert.equal(agent.name, 'Lister de archivos');
  assert.equal(agent.type, 'Explore');
  assert.equal(agent.status, 'completed');
  assert.equal(s.tokensOf(agent), 8606);
  assert.ok(states.includes('active') && states.at(-1) === 'done', 'activo mientras trabaja, terminado al acabar');
  // En su ventana, lo suyo (razonamiento, herramienta y respuesta); en la del orquestador, la
  // llamada a Agent y sus dos respuestas, sin lo del agente dentro.
  assert.deepEqual(sub.state.items.map((i) => i.kind), ['thinking', 'tool', 'text']);
  const agentCall = main.state.items.find((i) => i.kind === 'tool' && i.name === 'Agent');
  assert.equal(agentCall.children.length, 0);
  assert.equal(main.state.items.filter((i) => i.kind === 'text').length, 2);
  assert.ok(s.mainTokens() > 0);
});

const notes = (out) => out.targets.filter((t) => t.msg.subtype === 'informational');

test('cómo acabó una tarea: lo que no se sabe nunca es un éxito', () => {
  assert.equal(endStatus('completed'), 'completed');
  assert.equal(endStatus('failed'), 'failed');
  assert.equal(endStatus('stopped'), 'stopped');
  assert.equal(endStatus('killed'), 'stopped');
  // Al retomar una sesión, Claude Code da por «stopped» lo que cortó el final de la anterior.
  assert.equal(endStatus('stopped', 'Agent "Revisor" didn\'t finish before the previous session ended'), 'unknown');
  assert.equal(endStatus('paused'), 'unknown');
  assert.equal(endStatus(undefined), 'unknown');
  assert.equal(agentState(undefined), 'unknown', 'sin noticias de un agente, no se sabe cómo acabó');
  assert.equal(agentState({ status: 'raro' }), 'unknown');
  assert.deepEqual(
    ['active', 'waiting', 'done', 'failed', 'stopped', 'unknown', 'idle'].map((k) => STATE_LABEL[k]),
    ['Activo', 'Esperando respuesta', 'Terminado', 'Falló', 'Detenido', 'Sin terminar', 'Inactivo'],
  );
});

test('un agente que falla, se detiene o se corta lo dice y deja una nota en su ventana', () => {
  const s = createWorkspaceStream();
  s.route(spawn('toolu_1', {}));
  s.route(started('a1', 'toolu_1'));
  const failed = s.route({ type: 'system', subtype: 'task_notification', task_id: 'a1', status: 'failed', summary: 'API Error: 529 overloaded\nmás detalle' });
  assert.equal(agentState(s.get('a1')), 'failed');
  assert.deepEqual(failed.ended, ['a1']);
  assert.deepEqual(notes(failed).map((t) => [t.to, t.msg.level, t.msg.content]), [['a1', 'error', 'El agente falló: API Error: 529 overloaded']]);
  // Retomado, vuelve a estar activo; Detener: `task_updated` killed y después su aviso, una nota.
  s.route(started('a1', 'toolu_2'));
  assert.equal(agentState(s.get('a1')), 'active');
  const killed = s.route({ type: 'system', subtype: 'task_updated', task_id: 'a1', patch: { status: 'killed' } });
  assert.equal(agentState(s.get('a1')), 'stopped');
  assert.deepEqual(notes(killed), []);
  const stopped = s.route({ type: 'system', subtype: 'task_notification', task_id: 'a1', status: 'stopped' });
  assert.deepEqual(notes(stopped).map((t) => t.msg.content), ['El agente se detuvo.']);
  assert.deepEqual(notes(s.route({ type: 'system', subtype: 'task_notification', task_id: 'a1', status: 'stopped' })), [], 'el mismo final no deja otra nota');
  // Claude Code se cierra con un agente en marcha: no se sabe cómo acabó.
  s.route(spawn('toolu_3', {}));
  s.route(started('a3', 'toolu_3'));
  assert.deepEqual(s.exit(), ['a3']);
  assert.equal(agentState(s.get('a3')), 'unknown');
  assert.equal(STATE_LABEL[agentState(s.get('a3'))], 'Sin terminar');
  assert.match(endNotice(s.get('a3')).content, /^El agente no terminó/);
  assert.equal(endNotice({ status: 'completed' }), null);
});

test('un agente en primer plano acaba con el resultado de su llamada a Agent', () => {
  const result = (id, content, is_error = false) => ({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: id, content, is_error }] } });
  const s = createWorkspaceStream();
  for (const [task, tool] of [['a1', 'toolu_1'], ['a2', 'toolu_2'], ['a3', 'toolu_3'], ['a4', 'toolu_4']]) {
    s.route(spawn(tool, {}));
    s.route(started(task, tool));
  }
  s.route(result('toolu_1', 'Hecho: tres endpoints'));
  s.route(result('toolu_2', 'Error: el agente no pudo seguir', true));
  s.route(result('toolu_3', [{ type: 'text', text: '[Request interrupted by user for tool use]' }], true));
  // Uno en segundo plano solo dice que arrancó: sigue activo hasta su aviso.
  s.route(result('toolu_4', [{ type: 'text', text: 'Async agent launched successfully.' }]));
  assert.deepEqual(['a1', 'a2', 'a3', 'a4'].map((id) => agentState(s.get(id))), ['done', 'failed', 'stopped', 'active']);
  // Su aviso después no deja otra nota.
  assert.deepEqual(notes(s.route({ type: 'system', subtype: 'task_notification', task_id: 'a2', status: 'failed' })), []);
});

test('en el historial cada agente acaba como dice su aviso, también si no se sabe', () => {
  const s = createWorkspaceStream();
  const model = createChatModel({ prompts: true });
  const replay = [
    started('a1', 'toolu_1', { nexo_history: true }),
    sub('toolu_1', [{ type: 'text', text: 'Reviso.' }]),
    { type: 'system', subtype: 'task_notification', task_id: 'a1', tool_use_id: 'toolu_1', status: 'unknown', summary: '', nexo_history: true },
  ];
  const ended = [];
  for (const line of replay) {
    const out = s.route(line);
    ended.push(...out.ended);
    for (const t of out.targets) if (t.to === 'a1') model.apply(t.msg);
  }
  assert.deepEqual(ended, ['a1']);
  assert.equal(agentState(s.get('a1')), 'unknown');
  assert.deepEqual(model.state.items.map((i) => [i.kind, i.level ?? '']), [['text', ''], ['notice', 'info']]);
});
