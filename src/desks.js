// Escritorios de notas: hasta cinco en la lista y los que se quieran archivados. Aquí solo
// está el modelo (funciones puras sobre el estado, se prueban con node); la interfaz y el
// guardado están en notes.js.
//
// Estado: { active, desks: [{ id, name, cards, view }], archived: [{ …, archivedAt }] }.
// Las cartas del escritorio activo viven además en `.nexo/escritorio.json`, que es el que
// manda para él (lo escribe la interfaz al mover una carta y también Claude Code); el resto
// se guarda en `.nexo/escritorios.json`.

export const MAX_DESKS = 5;

const clean = (name) => String(name ?? '').replace(/\s+/g, ' ').trim().slice(0, 60);
const cardsOf = (value) => (Array.isArray(value) ? value.filter((c) => c && typeof c.path === 'string') : []);
// La vista de un escritorio: dónde se miraba y, si se alejó, con qué zoom (de 0 a −200 %).
const viewOf = (value) => {
  if (!value || !Number.isFinite(value.x) || !Number.isFinite(value.y)) return null;
  const zoom = Number.isFinite(value.zoom) && value.zoom < 0 ? Math.max(-200, Math.round(value.zoom)) : 0;
  return zoom ? { x: value.x, y: value.y, zoom } : { x: value.x, y: value.y };
};

// «Escritorio N» con el primer número que no esté en uso (ni en la lista ni archivado).
export function nextName(state) {
  const used = new Set([...state.desks, ...state.archived].map((d) => d.name));
  for (let n = 1; ; n++) if (!used.has(`Escritorio ${n}`)) return `Escritorio ${n}`;
}

// Deja lo leído del disco en un estado válido: ids únicos, al menos un escritorio, el
// activo existente y como mucho MAX_DESKS en la lista (los que sobren pasan a archivados).
export function normalize(raw, newId) {
  const seen = new Set();
  const entry = (d) => {
    if (!d || typeof d.id !== 'string' || !d.id || seen.has(d.id)) return null;
    seen.add(d.id);
    return { id: d.id, name: clean(d.name) || 'Escritorio', cards: cardsOf(d.cards), view: viewOf(d.view), ...(d.archivedAt ? { archivedAt: String(d.archivedAt) } : {}) };
  };
  const desks = (Array.isArray(raw?.desks) ? raw.desks : []).map(entry).filter(Boolean);
  const archived = (Array.isArray(raw?.archived) ? raw.archived : []).map(entry).filter(Boolean);
  const state = { active: raw?.active, desks: desks.slice(0, MAX_DESKS), archived: [...desks.slice(MAX_DESKS), ...archived] };
  if (!state.desks.length) state.desks.push({ id: newId, name: nextName(state), cards: [], view: null });
  if (!state.desks.some((d) => d.id === state.active)) state.active = state.desks[0].id;
  return state;
}

export const activeDesk = (state) => state.desks.find((d) => d.id === state.active);

// Apunta en el escritorio activo sus cartas y su vista (lo que hay ahora en pantalla).
export function storeActive(state, cards, view) {
  return { ...state, desks: state.desks.map((d) => (d.id === state.active ? { ...d, cards: cardsOf(cards), view: viewOf(view) } : d)) };
}

export function switchDesk(state, id) {
  return state.desks.some((d) => d.id === id) ? { ...state, active: id } : state;
}

// Escritorio nuevo, vacío, que pasa a ser el activo. Con la lista llena no hace nada.
export function createDesk(state, id) {
  if (state.desks.length >= MAX_DESKS) return state;
  return { ...state, active: id, desks: [...state.desks, { id, name: nextName(state), cards: [], view: null }] };
}

export function renameDesk(state, id, name) {
  const next = clean(name);
  if (!next) return state;
  const rename = (d) => (d.id === id ? { ...d, name: next } : d);
  return { ...state, desks: state.desks.map(rename), archived: state.archived.map(rename) };
}

// Archivar: sale de la lista y queda guardado. Si era el activo, pasa a serlo su vecino; si
// era el único, se abre uno nuevo vacío (`newId`) para que siempre haya dónde trabajar.
export function archiveDesk(state, id, when, newId) {
  const at = state.desks.findIndex((d) => d.id === id);
  if (at < 0) return state;
  const desks = state.desks.filter((d) => d.id !== id);
  const archived = [{ ...state.desks[at], archivedAt: when }, ...state.archived];
  let next = { ...state, desks, archived };
  if (!desks.length) next = { ...next, desks: [{ id: newId, name: nextName(next), cards: [], view: null }] };
  if (state.active === id) next.active = (next.desks[at] ?? next.desks[at - 1] ?? next.desks[0]).id;
  return next;
}

// Cargar un archivado: vuelve a la lista (al final) y pasa a ser el activo. Con la lista
// llena no hace nada: antes hay que archivar otro.
export function restoreDesk(state, id) {
  const desk = state.archived.find((d) => d.id === id);
  if (!desk || state.desks.length >= MAX_DESKS) return state;
  const { archivedAt, ...rest } = desk;
  return { active: id, desks: [...state.desks, rest], archived: state.archived.filter((d) => d.id !== id) };
}

export function removeArchived(state, id) {
  return { ...state, archived: state.archived.filter((d) => d.id !== id) };
}
