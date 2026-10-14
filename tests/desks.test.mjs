import test from 'node:test';
import assert from 'node:assert/strict';
import { MAX_DESKS, normalize, nextName, activeDesk, storeActive, switchDesk, createDesk, renameDesk, archiveDesk, restoreDesk, removeArchived } from '../src/desks.js';

const names = (state) => state.desks.map((d) => d.name);
const cardA = { path: 'a.md', x: 0, y: 0, w: 380, h: 440, z: 1 };

test('sin nada guardado hay un escritorio, y es el activo', () => {
  const state = normalize(null, 'd1');
  assert.deepEqual(state, { active: 'd1', desks: [{ id: 'd1', name: 'Escritorio 1', cards: [], view: null }], archived: [] });
});

test('normalize arregla lo que venga mal del disco', () => {
  const state = normalize({
    active: 'no-existe',
    desks: [{ id: 'a', name: '  Uno  ', cards: [cardA, null, { x: 3 }], view: { x: 'mal' } }, { id: 'a', name: 'repetido' }, { name: 'sin id' }, { id: 'b', name: '', view: { x: 10, y: -20, extra: 1 } }],
    archived: 'mal',
  }, 'nuevo');
  assert.equal(state.active, 'a');
  assert.deepEqual(state.desks, [{ id: 'a', name: 'Uno', cards: [cardA], view: null }, { id: 'b', name: 'Escritorio', cards: [], view: { x: 10, y: -20 } }]);
  assert.deepEqual(state.archived, []);
});

test('como mucho cinco en la lista: los que sobran quedan archivados', () => {
  const desks = Array.from({ length: 7 }, (_, i) => ({ id: `d${i}`, name: `E${i}` }));
  const state = normalize({ active: 'd6', desks }, 'x');
  assert.equal(state.desks.length, MAX_DESKS);
  assert.deepEqual(state.archived.map((d) => d.id), ['d5', 'd6']);
  assert.equal(state.active, 'd0', 'el activo no puede ser uno archivado');
});

test('crear añade un escritorio vacío y lo deja activo, hasta cinco', () => {
  let state = normalize(null, 'd1');
  for (const id of ['d2', 'd3', 'd4', 'd5']) state = createDesk(state, id);
  assert.deepEqual(names(state), ['Escritorio 1', 'Escritorio 2', 'Escritorio 3', 'Escritorio 4', 'Escritorio 5']);
  assert.equal(state.active, 'd5');
  const full = createDesk(state, 'd6');
  assert.equal(full, state, 'con cinco no se crea otro');
});

test('los nombres nuevos no repiten los de la lista ni los archivados', () => {
  let state = normalize(null, 'd1');
  state = createDesk(state, 'd2');
  state = archiveDesk(state, 'd1', '2026-10-13', 'x');
  assert.equal(nextName(state), 'Escritorio 3');
  state = renameDesk(state, 'd2', 'Semana');
  assert.equal(nextName(state), 'Escritorio 2');
});

test('renombrar limpia el nombre y no acepta uno vacío', () => {
  const state = normalize(null, 'd1');
  assert.equal(names(renameDesk(state, 'd1', '  Proyecto   nexo \n'))[0], 'Proyecto nexo');
  assert.equal(renameDesk(state, 'd1', '   '), state);
  assert.equal(names(renameDesk(state, 'd1', 'x'.repeat(200)))[0].length, 60);
});

test('lo que hay en pantalla se apunta en el escritorio activo', () => {
  let state = createDesk(normalize(null, 'd1'), 'd2');
  state = storeActive(state, [cardA], { x: -120, y: 40 });
  assert.deepEqual(activeDesk(state), { id: 'd2', name: 'Escritorio 2', cards: [cardA], view: { x: -120, y: 40 } });
  assert.deepEqual(state.desks[0].cards, [], 'los demás no cambian');
  state = switchDesk(state, 'd1');
  assert.equal(state.active, 'd1');
  assert.equal(switchDesk(state, 'no-existe'), state);
});

test('archivar saca el escritorio de la lista con sus cartas y activa al vecino', () => {
  let state = normalize(null, 'd1');
  state = createDesk(state, 'd2');
  state = createDesk(state, 'd3');
  state = storeActive(switchDesk(state, 'd2'), [cardA], null);
  state = archiveDesk(state, 'd2', '2026-10-13T10:00', 'x');
  assert.deepEqual(state.desks.map((d) => d.id), ['d1', 'd3']);
  assert.equal(state.active, 'd3', 'el que ocupa su sitio');
  assert.deepEqual(state.archived, [{ id: 'd2', name: 'Escritorio 2', cards: [cardA], view: null, archivedAt: '2026-10-13T10:00' }]);
  // Archivar uno que no es el activo no cambia el activo.
  assert.equal(archiveDesk(state, 'd1', 'hoy', 'x').active, 'd3');
  // Archivar el último de la lista activa al anterior.
  assert.equal(archiveDesk(state, 'd3', 'hoy', 'x').active, 'd1');
});

test('archivar el único deja uno nuevo vacío', () => {
  const state = archiveDesk(normalize(null, 'd1'), 'd1', 'hoy', 'd2');
  assert.deepEqual(state.desks, [{ id: 'd2', name: 'Escritorio 2', cards: [], view: null }]);
  assert.equal(state.active, 'd2');
  assert.equal(state.archived.length, 1);
});

test('cargar un archivado lo devuelve a la lista y lo activa, si hay sitio', () => {
  let state = normalize(null, 'd1');
  state = storeActive(state, [cardA], { x: 5, y: 6 });
  state = archiveDesk(state, 'd1', 'hoy', 'd2');
  state = restoreDesk(state, 'd1');
  assert.deepEqual(state.desks.at(-1), { id: 'd1', name: 'Escritorio 1', cards: [cardA], view: { x: 5, y: 6 } }, 'vuelve sin la fecha de archivo');
  assert.equal(state.active, 'd1');
  assert.deepEqual(state.archived, []);

  let full = normalize(null, 'a');
  for (const id of ['b', 'c', 'd', 'e']) full = createDesk(full, id);
  full = { ...full, archived: [{ id: 'z', name: 'Viejo', cards: [], view: null, archivedAt: 'ayer' }] };
  assert.equal(restoreDesk(full, 'z'), full, 'con cinco en la lista no se carga');
  assert.equal(restoreDesk(full, 'no-existe'), full);
});

test('borrar un archivado no toca la lista', () => {
  let state = createDesk(normalize(null, 'd1'), 'd2');
  state = archiveDesk(state, 'd1', 'hoy', 'x');
  const after = removeArchived(state, 'd1');
  assert.deepEqual(after.archived, []);
  assert.deepEqual(after.desks, state.desks);
  assert.equal(after.active, state.active);
});

test('la vista de un escritorio guarda también su zoom, dentro de su rango', () => {
  const state = normalize({ active: 'a', desks: [{ id: 'a', name: 'A', view: { x: 1, y: 2, zoom: -75 } }, { id: 'b', name: 'B', view: { x: 0, y: 0, zoom: -900 } }, { id: 'c', name: 'C', view: { x: 0, y: 0, zoom: 30 } }] }, 'x');
  assert.deepEqual(state.desks.map((d) => d.view), [{ x: 1, y: 2, zoom: -75 }, { x: 0, y: 0, zoom: -200 }, { x: 0, y: 0 }]);
  assert.deepEqual(activeDesk(storeActive(state, [], { x: 5, y: 6, zoom: -50 })).view, { x: 5, y: 6, zoom: -50 });
});
