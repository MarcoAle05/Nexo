// Estado de atención de todos los workspaces y textos del punto de «espera tu respuesta».
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { WAITING_TEXT, agentsButtonLabel, createAttention, createWatch, watchedPath } from '../src/ws-attention.js';

test('el estado junta los eventos de todos los workspaces', () => {
  const a = createAttention();
  assert.equal(a.set({ path: '/p/uno', waiting: 1, busy: true }), true);
  assert.equal(a.set({ path: '/p/dos', waiting: 0, busy: true }), true);
  assert.equal(a.set({ path: '/p/tres', waiting: 2, busy: true }), true);
  assert.equal(a.waiting('/p/uno'), true);
  assert.equal(a.waiting('/p/dos'), false, 'trabajando no es esperar');
  assert.equal(a.waiting('/p/tres/'), true, 'la barra final no cambia el workspace');
  assert.equal(a.waiting('/p/otro'), false);
  assert.equal(a.count(), 2, 'cuenta workspaces, no peticiones');
  assert.deepEqual(a.get('/p/tres'), { waiting: 2, busy: true });
  // El mismo estado otra vez no es un cambio (no hace falta repintar).
  assert.equal(a.set({ path: '/p/uno', waiting: 1, busy: true }), false);
  assert.equal(a.set({ path: '/p/cuatro', waiting: 0, busy: false }), false);
  // Lo que no tiene forma de evento se ignora.
  assert.equal(a.set({ waiting: 3 }), false);
  assert.equal(a.set(null), false);
  assert.equal(a.count(), 2);
});

test('la carga inicial no pisa un evento que llegó antes', () => {
  const a = createAttention();
  a.set({ path: '/p/uno', waiting: 0, busy: false });
  assert.equal(a.load([{ path: '/p/uno', waiting: 1, busy: true }, { path: '/p/dos', waiting: 1, busy: true }]), true);
  assert.equal(a.waiting('/p/uno'), false, 'el evento es más nuevo que la respuesta');
  assert.equal(a.waiting('/p/dos'), true);
  assert.equal(a.load(null), false);
});

test('el botón Agentes dice cuántos esperan', () => {
  assert.equal(agentsButtonLabel(0), null, 'sin ninguno, nada de más');
  assert.equal(agentsButtonLabel(1), 'Agentes: 1 workspace espera tu respuesta');
  assert.equal(agentsButtonLabel(2), 'Agentes: 2 workspaces esperan tu respuesta');
  const a = createAttention();
  a.load([{ path: '/p/uno', waiting: 1, busy: true }, { path: '/p/dos', waiting: 4, busy: true }, { path: '/p/tres', waiting: 0, busy: true }]);
  assert.equal(agentsButtonLabel(a.count()), 'Agentes: 2 workspaces esperan tu respuesta');
  assert.equal(WAITING_TEXT, 'Espera tu respuesta');
});

test('un workspace que deja de esperar pierde el punto', () => {
  const a = createAttention();
  a.set({ path: '/p/uno', waiting: 1, busy: true });
  a.set({ path: '/p/dos', waiting: 1, busy: true });
  assert.equal(a.set({ path: '/p/uno', waiting: 0, busy: true }), true);
  assert.equal(a.waiting('/p/uno'), false);
  assert.equal(agentsButtonLabel(a.count()), 'Agentes: 1 workspace espera tu respuesta');
  // Al cerrarse el proceso (o terminar) queda libre y sin nada pendiente.
  assert.equal(a.set({ path: '/p/dos', waiting: 0, busy: false }), true);
  assert.equal(a.count(), 0);
  assert.equal(agentsButtonLabel(a.count()), null);
});

test('solo se mira un workspace en la vista Agentes con su panel y uno abierto', () => {
  assert.equal(watchedPath({ view: 'agentes', mode: 'workspace', path: '/p/uno' }), '/p/uno');
  assert.equal(watchedPath({ view: 'agentes', mode: 'nexo', path: '/p/uno' }), null);
  assert.equal(watchedPath({ view: 'grafo', mode: 'workspace', path: '/p/uno' }), null);
  assert.equal(watchedPath({ view: 'notas', mode: 'workspace', path: '/p/uno' }), null);
  assert.equal(watchedPath({ view: 'agentes', mode: 'workspace', path: null }), null);
});

test('workspace_watch solo se llama cuando cambia lo que hay en pantalla o el foco', () => {
  const sent = [];
  const watch = createWatch((args) => sent.push(args));
  assert.equal(watch({ path: null, focused: true }), true);
  assert.equal(watch({ path: null, focused: true }), false);
  watch({ path: '/p/uno', focused: true });
  watch({ path: '/p/uno', focused: true });
  watch({ path: '/p/uno', focused: false });
  watch({ path: null, focused: false });
  watch({});
  assert.deepEqual(sent, [
    { path: null, focused: true },
    { path: '/p/uno', focused: true },
    { path: '/p/uno', focused: false },
    { path: null, focused: false },
  ]);
});
