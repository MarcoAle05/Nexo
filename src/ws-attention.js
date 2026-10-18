// Qué workspaces esperan una respuesta del usuario (un permiso, una pregunta o un plan sin
// contestar), aunque la interfaz no los esté mirando. El backend (workspace_run.rs) emite el
// evento global `workspace-attention` con `{ path, waiting, busy }` cada vez que cambia el
// estado de uno, y `workspace_attention` da el de todos al arrancar. Aquí se juntan y salen los
// textos del punto; y `createWatch` le dice al backend qué workspace hay en pantalla solo cuando
// cambia. Sin DOM: se prueba con node.

export const WAITING_TEXT = 'Espera tu respuesta';

// Las rutas llegan como las da `ws_root`; una barra final de más no hace otro workspace.
const key = (path) => String(path ?? '').replace(/(.)\/+$/, '$1');

export function createAttention() {
  const states = new Map(); // ruta → { waiting, busy }
  const heard = new Set(); // rutas que ya llegaron por evento: la carga inicial no las pisa

  const put = (entry) => {
    if (!entry || typeof entry.path !== 'string' || !entry.path) return false;
    const path = key(entry.path);
    const waiting = Number.isFinite(entry.waiting) && entry.waiting > 0 ? Math.floor(entry.waiting) : 0;
    const busy = Boolean(entry.busy);
    // Un workspace libre y sin nada pendiente es lo mismo que uno sin proceso: no se guarda.
    const before = states.get(path) ?? { waiting: 0, busy: false };
    if (before.waiting === waiting && before.busy === busy) return false;
    if (!waiting && !busy) states.delete(path);
    else states.set(path, { waiting, busy });
    return true;
  };

  return {
    // Un evento `workspace-attention`. Devuelve si cambió algo.
    set(entry) {
      const changed = put(entry);
      if (entry?.path) heard.add(key(entry.path));
      return changed;
    },
    // Lo que da `workspace_attention` al arrancar (o tras una recarga). Un evento que llegó
    // antes que la respuesta es más nuevo que ella y se respeta.
    load(list) {
      let changed = false;
      for (const entry of Array.isArray(list) ? list : []) {
        if (entry?.path && heard.has(key(entry.path))) continue;
        changed = put(entry) || changed;
      }
      return changed;
    },
    waiting: (path) => (states.get(key(path))?.waiting ?? 0) > 0,
    // Cuántos workspaces esperan (no cuántas peticiones: el botón habla de workspaces).
    count: () => [...states.values()].filter((s) => s.waiting > 0).length,
    get: (path) => states.get(key(path)) ?? { waiting: 0, busy: false },
  };
}

// Nombre accesible del botón Agentes: con alguno esperando lo dice; sin ninguno, `null` (el
// botón se queda con su texto, sin nada de más).
export function agentsButtonLabel(count) {
  if (!count) return null;
  return count === 1 ? 'Agentes: 1 workspace espera tu respuesta' : `Agentes: ${count} workspaces esperan tu respuesta`;
}

// Qué workspace hay en pantalla: solo en la vista Agentes, con el panel de un workspace y uno
// abierto. En cualquier otro caso, ninguno.
export function watchedPath({ view, mode, path }) {
  return view === 'agentes' && mode === 'workspace' && path ? path : null;
}

// Llama a `send({ path, focused })` (workspace_watch) solo cuando cambia lo que hay en pantalla
// o el foco de la ventana. Sin noticias, la ventana se toma como sin foco.
export function createWatch(send) {
  let last = null;
  return (next) => {
    const target = { path: next?.path ?? null, focused: Boolean(next?.focused) };
    if (last && last.path === target.path && last.focused === target.focused) return false;
    last = target;
    send(target);
    return true;
  };
}
