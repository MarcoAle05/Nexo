// Mientras las cartas laterales cambian de ancho (minimizar, restaurar, expandir: transiciones
// de flex de 520 ms en styles.css), lo que llevan dentro y lo que llena el centro (las ventanas
// de un workspace, el registro de agentes) no se vuelve a maquetar ni a pintar en cada
// fotograma: toma de una vez su medida final y la carta o el centro solo lo recortan mientras
// se mueven. Con cuatro chats largos en el centro, cada fotograma de la transición costaba
// entre 60 y 350 ms en WebKitGTK (medido el 18-10-2026 sin GPU, con 40 mensajes por chat: de 3
// a 15 fps); así se maqueta una vez y va a 20-40 fps en ese mismo banco.

// El borde que queda quieto (el que menos se mueve) es al que se ancla lo de dentro.
export function anchorSide(before, after) {
  return Math.abs(after.left - before.left) <= Math.abs(after.right - before.right) ? 'left' : 'right';
}

const visible = (el) => getComputedStyle(el).display !== 'none';

// `onSettle` corre cuando acaba la transición (o enseguida, si no hay ninguna): lo que cuesta
// mucho y no tiene por qué verse a mitad, como `inert` en el escenario, va ahí.
export function createCardHold({ layout, stage, center, cores, onSettle = () => {} }) {
  let generation = 0;
  let queued = false;
  const pinned = new Set();

  function release(el) {
    el.style.removeProperty('width');
    el.style.removeProperty('left');
    el.style.removeProperty('right');
    el.style.removeProperty('flex');
    pinned.delete(el);
  }

  // Sin transición en marcha: fuera lo fijado, salvo el centro con el escenario plegado (una
  // carta expandida), que no se ve y así no se maqueta a ancho cero.
  function settle() {
    layout.classList.remove('is-holding');
    stage.classList.remove('is-holding');
    const folded = layout.classList.contains('expanded');
    for (const el of [...pinned]) if (!(folded && center.includes(el))) release(el);
    onSettle();
  }

  function hold() {
    queued = false;
    const gen = ++generation;
    const moving = document
      .getAnimations()
      .filter((a) => a instanceof CSSTransition && a.effect?.target?.parentElement === layout && /^(flex|margin)/.test(a.transitionProperty));
    const centerItems = center.filter(visible);
    const coreItems = cores().filter(visible);
    if (!moving.length || !(centerItems.length + coreItems.length)) return settle();

    // Si el escenario se pliega, el centro se queda con la medida que tiene y se apaga con él.
    const folding = layout.classList.contains('expanded');
    if (folding) for (const el of centerItems) if (!pinned.has(el)) pin(el, el.getBoundingClientRect().width);

    // El estado final: cada transición en su final, se mide y se vuelve a donde estaba (en el
    // mismo momento, sin que se pinte nada en medio).
    const times = moving.map((a) => a.currentTime);
    for (const a of moving) a.currentTime = a.effect.getComputedTiming().endTime;
    const after = stage.getBoundingClientRect();
    const coreWidths = coreItems.map((el) => el.parentElement.clientWidth);
    if (!folding) for (const el of centerItems) pin(el, after.width);
    coreItems.forEach((el, i) => {
      el.style.flex = `0 0 ${coreWidths[i]}px`;
      pinned.add(el);
    });
    moving.forEach((a, i) => (a.currentTime = times[i]));

    const side = anchorSide(stage.getBoundingClientRect(), after);
    for (const el of centerItems) {
      el.style.left = side === 'left' ? '0px' : 'auto';
      el.style.right = side === 'right' ? '0px' : 'auto';
    }
    layout.classList.add('is-holding');
    stage.classList.toggle('is-holding', centerItems.length > 0);
    Promise.allSettled(moving.map((a) => a.finished)).then(() => gen === generation && settle());
  }

  function pin(el, width) {
    el.style.width = `${width}px`;
    pinned.add(el);
  }

  // Se llama al cambiar las clases de `.layout`; se mide al acabar la tarea, cuando ya cambió
  // todo lo demás (la escena, qué núcleo se ve).
  return function schedule() {
    if (queued) return;
    queued = true;
    queueMicrotask(hold);
  };
}
