// Utilidades de DOM compartidas por las cartas.

// Deja en `list` un elemento por cada `item`, en orden, reutilizando los que no cambiaron
// (misma clave y misma firma) y sin tocar los que ya están en su sitio. Así una recarga
// sin cambios no altera la página y solo los elementos nuevos llevan la clase `enter`
// (su animación de entrada). Antes cada recarga reconstruía la lista entera y la
// animación se repetía: la carta parpadeaba.
export function reconcile(list, items, { key, sig = () => '', build }) {
  const before = new Map();
  for (const el of list.children) if (el.dataset.key != null) before.set(el.dataset.key, el);
  const next = items.map((item, index) => {
    const k = String(key(item));
    const s = String(sig(item));
    const old = before.get(k);
    if (old && old.dataset.sig === s) return old;
    const el = build(item, index);
    el.dataset.key = k;
    el.dataset.sig = s;
    if (!old) {
      el.classList.add('enter');
      el.addEventListener('animationend', () => el.classList.remove('enter'), { once: true });
    }
    return el;
  });
  let cursor = list.firstChild;
  for (const el of next) {
    if (el === cursor) cursor = cursor.nextSibling;
    else list.insertBefore(el, cursor);
  }
  while (cursor) {
    const stale = cursor;
    cursor = cursor.nextSibling;
    stale.remove();
  }
}
