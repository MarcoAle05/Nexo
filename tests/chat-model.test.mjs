import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import {
  createChatModel, toolSummary, shortPath, modelLabel, effortLevels, filterCommands, composeMessage,
  userLine, allowLine, denyLine, controlLine, unsupportedLine,
} from '../src/chat-model.js';

// Conversación real con Claude Code 2.1.296 (Haiku), grabada con --include-partial-messages
// y saneada: un turno que crea un archivo pidiendo permiso y otro con el comando /context.
const lines = readFileSync(new URL('./fixtures/chat/turno.jsonl', import.meta.url), 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l));
const replay = (model, list, { answer = true } = {}) => {
  for (const line of list) {
    model.apply(line);
    if (answer && line.type === 'control_request') model.answer(line.request_id, 'allow');
  }
  return model;
};
const kinds = (model) => model.state.items.map((i) => i.kind);

test('la conversación grabada da las piezas en orden y sin duplicar', () => {
  const model = createChatModel();
  model.addUser('Crea el archivo hola.txt');
  replay(model, lines);
  assert.deepEqual(kinds(model), ['user', 'thinking', 'tool', 'text', 'text']);
  const [, thinking, tool, text, context] = model.state.items;
  assert.equal(thinking.streaming, false);
  assert.ok(thinking.tokens > 0, 'el razonamiento lleva su cuenta de tokens');
  assert.equal(tool.name, 'Write');
  assert.equal(tool.status, 'done');
  assert.equal(tool.input.file_path, '/vault/hola.txt');
  assert.match(tool.result, /File created successfully/);
  assert.match(text.text, /hola\.txt/);
  assert.equal(text.streaming, false);
  assert.equal(context.synthetic, true, 'la salida de /context viene como mensaje sintético');
  assert.match(context.text, /Context Usage/);
});

test('el texto se va escribiendo con el streaming y el mensaje completo lo cierra', () => {
  const model = createChatModel();
  const firstText = lines.findIndex((l) => l.type === 'stream_event' && l.event.delta?.type === 'text_delta');
  replay(model, lines.slice(0, firstText + 1));
  const item = model.state.items.at(-1);
  assert.equal(item.kind, 'text');
  assert.equal(item.streaming, true);
  assert.ok(item.text.length > 0);
  const partial = item.text;
  replay(model, lines.slice(firstText + 1));
  assert.equal(item.streaming, false);
  assert.ok(item.text.startsWith(partial), 'el texto final continúa el parcial, en la misma pieza');
});

test('sin streaming (al repetir la conversación) salen las mismas piezas', () => {
  const live = replay(createChatModel(), lines);
  const stored = replay(createChatModel(), lines.filter((l) => l.type !== 'stream_event' && l.subtype !== 'thinking_tokens' && l.type !== 'rate_limit_event'));
  assert.deepEqual(kinds(stored), kinds(live));
  assert.deepEqual(stored.state.items.map((i) => i.text ?? i.name), live.state.items.map((i) => i.text ?? i.name));
});

test('la inicialización da modelos, comandos y suscripción, sin datos de la cuenta', () => {
  const model = replay(createChatModel(), lines);
  assert.ok(model.state.models.some((m) => m.value === 'opus'));
  assert.ok(model.state.commands.some((c) => c.name === 'compact'));
  assert.deepEqual(model.state.account, { subscription: 'Claude Pro' });
  assert.equal(model.state.session.model, 'claude-haiku-5-5');
  assert.equal(model.state.session.cwd, '/vault');
  assert.equal(model.state.limits.fiveHour, 0.25);
});

test('un permiso deja el proceso en espera hasta que se contesta', () => {
  const model = createChatModel();
  const at = lines.findIndex((l) => l.type === 'control_request');
  replay(model, lines.slice(0, at + 1), { answer: false });
  const tool = model.state.items.find((i) => i.kind === 'tool');
  assert.equal(tool.status, 'waiting');
  assert.equal(tool.permission.description, '/vault/hola.txt');
  assert.deepEqual(tool.permission.suggestions.map((s) => s.type), ['setMode', 'addDirectories'], 'lo que Claude Code propone para no volver a preguntar');
  assert.deepEqual(model.pending(), [tool]);
  model.answer(lines[at].request_id, 'allow');
  assert.equal(tool.status, 'running');
  assert.deepEqual(model.pending(), []);
  assert.equal(model.answer(lines[at].request_id, 'deny'), null, 'un permiso se contesta una sola vez');
});

test('un permiso denegado se queda como denegado aunque llegue el resultado con error', () => {
  const model = createChatModel();
  const at = lines.findIndex((l) => l.type === 'control_request');
  replay(model, lines.slice(0, at + 1), { answer: false });
  model.apply({ type: 'nexo_permission', request_id: lines[at].request_id, decision: 'deny' });
  const tool = model.state.items.find((i) => i.kind === 'tool');
  assert.equal(tool.status, 'denied');
  model.apply({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: tool.id, is_error: true, content: 'El usuario no dio permiso.' }] }, parent_tool_use_id: null });
  assert.equal(tool.status, 'denied');
});

test('busy sigue el turno: empieza con el mensaje y termina con el resultado', () => {
  const model = createChatModel();
  assert.equal(model.state.busy, false);
  model.addUser('hola');
  assert.equal(model.state.busy, true);
  const end = lines.findIndex((l) => l.type === 'result');
  replay(model, lines.slice(0, end));
  assert.equal(model.state.busy, true);
  model.apply(lines[end]);
  assert.equal(model.state.busy, false);
  assert.equal(model.state.lastResult.error, false);
});

test('lo que hace un subagente queda dentro de su proceso', () => {
  const model = createChatModel();
  model.apply({ type: 'assistant', message: { id: 'm1', content: [{ type: 'tool_use', id: 'toolu_agente', name: 'Agent', input: { subagent_type: 'revisor', description: 'Revisar la wiki' } }] }, parent_tool_use_id: null });
  model.apply({ type: 'assistant', message: { id: 'm2', content: [{ type: 'text', text: 'Leo el índice.' }] }, parent_tool_use_id: 'toolu_agente' });
  model.apply({ type: 'assistant', message: { id: 'm2', content: [{ type: 'tool_use', id: 'toolu_leer', name: 'Read', input: { file_path: '/vault/wiki/_master-index.md' } }] }, parent_tool_use_id: 'toolu_agente' });
  model.flush();
  model.apply({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_leer', content: [{ type: 'text', text: '# Índice' }] }] }, parent_tool_use_id: 'toolu_agente' });
  assert.equal(model.state.items.length, 1);
  const agent = model.state.items[0];
  assert.deepEqual(agent.children.map((c) => c.kind), ['text', 'tool']);
  assert.equal(agent.children[1].status, 'done');
  assert.deepEqual(model.flush(), [agent], 'un cambio dentro repinta el proceso del agente');
});

test('si el turno termina, lo que estaba a medias se cierra', () => {
  const model = createChatModel();
  model.apply({ type: 'stream_event', event: { type: 'message_start', message: { id: 'm1' } }, parent_tool_use_id: null });
  model.apply({ type: 'stream_event', event: { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }, parent_tool_use_id: null });
  model.apply({ type: 'stream_event', event: { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text: 'Empiezo a' } }, parent_tool_use_id: null });
  model.apply({ type: 'assistant', message: { id: 'm0', content: [{ type: 'tool_use', id: 'toolu_x', name: 'Bash', input: { command: 'sleep 99' } }] }, parent_tool_use_id: null });
  model.apply({ type: 'control_request', request_id: 'r9', request: { subtype: 'can_use_tool', tool_name: 'Bash', input: { command: 'sleep 99' }, tool_use_id: 'toolu_x' } });
  model.apply({ type: 'result', subtype: 'error_during_execution', is_error: true, errors: ['interrumpido'] });
  const [text, tool, notice] = model.state.items;
  assert.equal(text.streaming, false);
  assert.equal(tool.status, 'stopped');
  assert.equal(tool.permission.answer, 'cancelled');
  assert.deepEqual([notice.kind, notice.level, notice.text], ['notice', 'error', 'interrumpido']);
  assert.equal(model.state.busy, false);
});

test('lo que no se sabe atender se devuelve para contestarlo con un error', () => {
  const model = createChatModel();
  assert.deepEqual(model.apply({ type: 'control_request', request_id: 'r1', request: { subtype: 'mcp_message' } }), { unsupported: 'r1', subtype: 'mcp_message' });
  assert.equal(model.apply({ type: 'algo_nuevo' }), null);
  assert.equal(model.apply(null), null);
  assert.equal(model.state.items.length, 0);
});

test('los ajustes vigentes y el modo de permisos se leen de las respuestas de control', () => {
  const model = createChatModel();
  model.apply({ type: 'control_response', response: { subtype: 'success', request_id: 'a', response: { effective: {}, applied: { model: 'claude-opus-5-5', effort: 'xhigh' } } } });
  assert.deepEqual(model.state.applied, { model: 'claude-opus-5-5', effort: 'xhigh' });
  model.apply({ type: 'control_response', response: { subtype: 'success', request_id: 'b', response: { mode: 'plan' } } });
  assert.equal(model.state.permissionMode, 'plan');
  model.apply({ type: 'system', subtype: 'status', status: null, permissionMode: 'acceptEdits' });
  assert.equal(model.state.permissionMode, 'acceptEdits');
});

test('las líneas propias de nexo repiten al usuario y sus respuestas', () => {
  const model = createChatModel();
  model.apply({ type: 'nexo_user', text: 'hola', files: ['/a/b.pdf'] });
  assert.deepEqual(model.state.items.map((i) => [i.kind, i.text, i.files]), [['user', 'hola', ['/a/b.pdf']]]);
  // Un «user» de texto que venga de Claude Code (órdenes locales, skills) no es un mensaje del usuario.
  model.apply({ type: 'user', message: { role: 'user', content: '<local-command-stdout>x</local-command-stdout>' }, parent_tool_use_id: null });
  assert.equal(model.state.items.length, 1);
});

test('toolSummary resume cada herramienta en español', () => {
  assert.deepEqual(toolSummary('Write', { file_path: '/vault/wiki/notas/compra.md' }, '/vault'), { label: 'Escribir', detail: 'wiki/notas/compra.md' });
  assert.deepEqual(toolSummary('Read', { file_path: '/home/u/docs/largo/camino/a.pdf' }, '/vault'), { label: 'Leer', detail: '…/camino/a.pdf' });
  assert.deepEqual(toolSummary('Bash', { command: 'git status\nls' }), { label: 'Ejecutar', detail: 'git status' });
  assert.deepEqual(toolSummary('Bash', { command: 'ls', description: 'Lista la carpeta' }), { label: 'Ejecutar', detail: 'Lista la carpeta' });
  assert.deepEqual(toolSummary('Agent', { subagent_type: 'revisor', description: 'Revisar la wiki' }), { label: 'Agente', detail: 'revisor · Revisar la wiki' });
  assert.deepEqual(toolSummary('mcp__playwright__browser_navigate', { url: 'https://example.com/a?b=1' }), { label: 'playwright', detail: 'browser navigate · example.com' });
  assert.deepEqual(toolSummary('Desconocida', { x: 3 }), { label: 'Desconocida', detail: '' });
  assert.deepEqual(toolSummary('Read', null), { label: 'Leer', detail: '' });
  assert.equal(toolSummary('Bash', { command: 'x'.repeat(300) }).detail.length, 96);
  assert.equal(shortPath('/vault', '/vault'), '.');
});

test('modelLabel y effortLevels salen de la lista de modelos de Claude Code', () => {
  const models = replay(createChatModel(), lines).state.models;
  assert.equal(modelLabel(models, 'claude-opus-5-5', null), 'Opus 5.5');
  assert.equal(modelLabel(models, 'claude-haiku-5-5', 'haiku'), 'Haiku 5.5');
  assert.equal(modelLabel(models, null, null), 'Opus 5.5', 'sin dato, el del modelo predeterminado');
  assert.equal(modelLabel(models, 'claude-raro-9', null), 'claude-raro-9');
  assert.equal(modelLabel([], null, null), 'Modelo');
  assert.deepEqual(effortLevels(models, 'claude-opus-5-5', null), ['low', 'medium', 'high', 'xhigh', 'max']);
  assert.deepEqual(effortLevels([{ value: 'viejo', resolvedModel: 'viejo', supportsEffort: false }], 'viejo', 'viejo'), []);
});

test('filterCommands pone primero los que empiezan por lo escrito', () => {
  const commands = [
    { name: 'compact', description: 'Free up context' },
    { name: 'context', description: 'Show context usage' },
    { name: 'data:analyze', description: 'Answer data questions', aliases: ['analyze'] },
    { name: 'model', description: 'Change the model' },
  ];
  assert.deepEqual(filterCommands(commands, '').map((c) => c.name), ['compact', 'context', 'data:analyze', 'model']);
  assert.deepEqual(filterCommands(commands, 'co').map((c) => c.name), ['compact', 'context']);
  assert.deepEqual(filterCommands(commands, 'ana').map((c) => c.name), ['data:analyze']);
  assert.deepEqual(filterCommands(commands, 'usage').map((c) => c.name), ['context'], 'también por la descripción');
  assert.deepEqual(filterCommands(commands, 'zzz'), []);
});

test('las líneas que se escriben a Claude Code son JSON de una sola línea', () => {
  const all = [
    userLine('hola\n«mundo»'),
    controlLine('r1', { subtype: 'interrupt' }),
    allowLine('r2', { file_path: '/a' }, [{ type: 'setMode', mode: 'acceptEdits', destination: 'session' }]),
    allowLine('r3', { a: 1 }),
    denyLine('r4', ''),
    unsupportedLine('r5', 'mcp_message'),
  ];
  for (const line of all) assert.ok(!line.includes('\n'), line);
  assert.deepEqual(JSON.parse(all[0]).message, { role: 'user', content: 'hola\n«mundo»' });
  assert.deepEqual(JSON.parse(all[2]).response.response, { behavior: 'allow', updatedInput: { file_path: '/a' }, updatedPermissions: [{ type: 'setMode', mode: 'acceptEdits', destination: 'session' }] });
  assert.equal('updatedPermissions' in JSON.parse(all[3]).response.response, false);
  assert.equal(JSON.parse(all[4]).response.response.behavior, 'deny');
  assert.equal(JSON.parse(all[5]).response.subtype, 'error');
});

test('composeMessage añade los adjuntos como rutas', () => {
  assert.equal(composeMessage('  hola  '), 'hola');
  assert.equal(composeMessage('Resume esto', ['/a/b.pdf', '/c d/e.png']), 'Resume esto\n\nArchivos adjuntos (léelos con Read):\n- /a/b.pdf\n- /c d/e.png');
  assert.equal(composeMessage('', ['/a.png']), 'Archivos adjuntos (léelos con Read):\n- /a.png');
});

test('lo que va por la API de nexo entra en la conversación sin empezar un turno de Claude', () => {
  const model = createChatModel();
  model.addUser('/preguntar ¿qué es PARA?', [], { local: true });
  assert.equal(model.state.busy, false, 'no es un turno de Claude Code');
  const reply = model.localReply('Nexo · API');
  reply.push('PARA es ');
  reply.push('un método.');
  const item = model.state.items.at(-1);
  assert.deepEqual([item.kind, item.speaker, item.text, item.streaming], ['text', 'Nexo · API', 'PARA es un método.', true]);
  assert.equal(reply.end(), 'PARA es un método.');
  assert.equal(item.streaming, false);
  assert.deepEqual(model.flush(), model.state.items);
});
