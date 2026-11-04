// Lo de alrededor de los elementos del lienzo: «Añadir» y «Capas» en la barra superior, la barra
// pequeña de lo seleccionado y la ventana para editar lo que no se edita en su sitio (tablas
// relacionales, secuencias, líneas de tiempo, referencias e imágenes).

import { CATALOG, COLORS, FORMS, FORM_LABELS, ICONS } from './canvas-catalog.js';
import { createModal } from './modal.js';
import { layoutSequence, layoutTimeline } from './canvas-board.js';

const $ = (id) => document.getElementById(id);
const ICON = {
  chevron: '<svg width="12" height="12" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="m5 8 5 5 5-5"/></svg>',
  eye: '<svg width="14" height="14" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M2.5 10s2.8-5 7.5-5 7.5 5 7.5 5-2.8 5-7.5 5-7.5-5-7.5-5z"/><circle cx="10" cy="10" r="2.2"/></svg>',
  eyeOff: '<svg width="14" height="14" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3l14 14M8.3 5.2A7 7 0 0 1 10 5c4.7 0 7.5 5 7.5 5a13 13 0 0 1-2.4 2.9M5.4 6.6A13 13 0 0 0 2.5 10s2.8 5 7.5 5a7 7 0 0 0 3-.7"/></svg>',
  target: '<svg width="14" height="14" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><circle cx="10" cy="10" r="6"/><path d="M10 2v4M10 14v4M2 10h4M14 10h4"/></svg>',
  merge: '<svg width="14" height="14" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M10 3v9M6.5 8.5 10 12l3.5-3.5M4 16.5h12"/></svg>',
  trash: '<svg width="14" height="14" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4 6h12M8 6V4h4v2M6 6l1 10h6l1-10"/></svg>',
  undo: '<path d="M7.5 4 4 7.5 7.5 11"/><path d="M4 7.5h7.5a4.5 4.5 0 0 1 0 9H8"/>',
  redo: '<path d="M12.5 4 16 7.5 12.5 11"/><path d="M16 7.5H8.5a4.5 4.5 0 0 0 0 9H12"/>',
  duplicate: '<rect x="7" y="7" width="10" height="10" rx="2"/><path d="M13 7V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2h2"/>',
  front: '<rect x="7" y="7" width="10" height="10" rx="2"/><path d="M4 13V5a1 1 0 0 1 1-1h8"/>',
  back: '<rect x="3" y="3" width="10" height="10" rx="2"/><path d="M16 7v8a1 1 0 0 1-1 1H7" stroke-dasharray="2 2"/>',
  note: '<path d="M5 2.5h7l3 3v12H5z"/><path d="M7.5 9h5M7.5 12h5"/>',
  edit: '<path d="M13.5 3.5l3 3L7 16H4v-3z"/>',
  trashPath: '<path d="M4 6h12M8 6V4h4v2M6 6l1 10h6l1-10"/>',
  alignLeft: '<path d="M4 3v14M7 6h9M7 10h5M7 14h7"/>',
  alignCenter: '<path d="M10 3v14M5 6h10M7 10h6M6 14h8"/>',
  alignRight: '<path d="M16 3v14M4 6h9M8 10h5M6 14h7"/>',
  alignTop: '<path d="M3 4h14M6 7v9M10 7v5M14 7v7"/>',
  alignMiddle: '<path d="M3 10h14M6 5v10M10 7v6M14 6v8"/>',
  alignBottom: '<path d="M3 16h14M6 4v9M10 8v5M14 6v7"/>',
  spreadX: '<path d="M4 3v14M16 3v14M8 7h4v6H8z"/>',
  spreadY: '<path d="M3 4h14M3 16h14M7 8h6v4H7z"/>',
  addRow: '<rect x="3" y="4" width="14" height="6" rx="1"/><path d="M10 12.5v5M7.5 15h5"/>',
  addCol: '<rect x="4" y="3" width="6" height="14" rx="1"/><path d="M12.5 10h5M15 7.5v5"/>',
  delRow: '<rect x="3" y="4" width="14" height="6" rx="1"/><path d="M7.5 15h5"/>',
  delCol: '<rect x="4" y="3" width="6" height="14" rx="1"/><path d="M12.5 10h5"/>',
  sortUp: '<path d="M6 16V5M3 8l3-3 3 3M12 6h5M12 10h3.5M12 14h2"/>',
  sortDown: '<path d="M6 4v11M3 12l3 3 3-3M12 6h2M12 10h3.5M12 14h5"/>',
  header: '<rect x="3" y="4" width="14" height="12" rx="1.5"/><path d="M3 8.5h14" stroke-width="2.4"/>',
};
const svg = (paths, size = 16) => `<svg width="${size}" height="${size}" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;

const ROUTES = [['angulada', 'En ángulo'], ['recta', 'Recta'], ['curva', 'Curva']];
const ENDS = [['nada', 'Nada'], ['flecha', 'Flecha'], ['triangulo', 'Triángulo'], ['rombo', 'Rombo'], ['uno', 'Uno'], ['muchos', 'Muchos (pata de gallo)'], ['cero-muchos', 'Cero o muchos']];
const LAYER_STATE = { dibujando: 'Dibujando', terminada: 'Terminada', sin_terminar: 'Sin terminar' };
const LANGS = ['', 'bash', 'c', 'cpp', 'css', 'go', 'java', 'javascript', 'json', 'python', 'rust', 'sql', 'typescript', 'yaml'];

export function createChrome(api) {
  const tools = $('desk-tools');
  const addBtn = $('canvas-add');
  const addMenu = $('canvas-add-menu');
  const layersBtn = $('canvas-layers');
  const layersMenu = $('canvas-layers-menu');
  const bar = $('canvas-bar');
  const editor = $('canvas-editor');
  const fileInput = $('canvas-file');
  let visible = false;
  let pendingAt = null; // dónde poner la imagen que se está eligiendo

  // ------------------------------------------------------------ añadir

  const itemByKey = new Map();
  function buildMenu() {
    addMenu.replaceChildren();
    for (const g of CATALOG) {
      const title = document.createElement('div');
      title.className = 'vault-menu-title';
      title.textContent = g.group;
      title.setAttribute('role', 'presentation');
      addMenu.append(title);
      for (const it of g.items) {
        itemByKey.set(it.key, it);
        const b = document.createElement('button');
        b.type = 'button';
        b.className = 'cv-menu-item';
        b.setAttribute('role', 'menuitem');
        b.dataset.key = it.key;
        b.tabIndex = -1;
        b.innerHTML = `<span class="cv-menu-icon">${it.icon}</span><span class="cv-menu-text"><span class="cv-menu-label"></span><small></small></span>${it.shortcut ? `<kbd>${it.shortcut}</kbd>` : ''}`;
        b.querySelector('.cv-menu-label').textContent = it.label;
        b.querySelector('small').textContent = it.hint;
        addMenu.append(b);
      }
      if (g.strip) {
        const row = document.createElement('div');
        row.className = 'cv-menu-strip';
        row.setAttribute('role', 'group');
        row.setAttribute('aria-label', g.stripLabel);
        for (const it of g.strip) {
          itemByKey.set(it.key, it);
          const b = document.createElement('button');
          b.type = 'button';
          b.className = 'cv-menu-form';
          b.setAttribute('role', 'menuitem');
          b.dataset.key = it.key;
          b.tabIndex = -1;
          b.title = it.label;
          b.setAttribute('aria-label', it.label);
          b.innerHTML = it.icon;
          row.append(b);
        }
        addMenu.append(row);
      }
    }
    // En las ventanas más estrechas «Capas» no cabe en la barra: va aquí.
    const layers = document.createElement('button');
    layers.type = 'button';
    layers.id = 'canvas-add-layers';
    layers.className = 'cv-menu-item cv-menu-layers';
    layers.setAttribute('role', 'menuitem');
    layers.tabIndex = -1;
    layers.innerHTML = '<span class="cv-menu-icon"><svg width="18" height="18" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 3.5 3.5 7 10 10.5 16.5 7z"/><path d="m3.5 10.5 6.5 3.5 6.5-3.5"/><path d="m3.5 14 6.5 3.5 6.5-3.5"/></svg></span><span class="cv-menu-text"><span class="cv-menu-label">Capas</span><small>Ver, ocultar, integrar o borrar</small></span>';
    addMenu.append(layers);
  }
  buildMenu();

  const menuItems = () => [...addMenu.querySelectorAll('[role="menuitem"]')].filter((b) => b.offsetParent !== null);
  function openAdd() {
    closeLayers();
    addMenu.hidden = false;
    addBtn.setAttribute('aria-expanded', 'true');
    menuItems()[0]?.focus();
  }
  function closeAdd(refocus = false) {
    if (addMenu.hidden) return;
    addMenu.hidden = true;
    addBtn.setAttribute('aria-expanded', 'false');
    if (refocus) addBtn.focus();
  }
  addBtn.addEventListener('click', () => (addMenu.hidden ? openAdd() : closeAdd(true)));
  addBtn.addEventListener('keydown', (event) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      openAdd();
    }
  });
  addMenu.addEventListener('keydown', (event) => {
    const list = menuItems();
    const at = list.indexOf(document.activeElement);
    const go = (i) => list[(i + list.length) % list.length]?.focus();
    if (event.key === 'ArrowDown' || event.key === 'ArrowRight') {
      event.preventDefault();
      go(at + 1);
    } else if (event.key === 'ArrowUp' || event.key === 'ArrowLeft') {
      event.preventDefault();
      go(at - 1);
    } else if (event.key === 'Home') {
      event.preventDefault();
      go(0);
    } else if (event.key === 'End') {
      event.preventDefault();
      go(list.length - 1);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      closeAdd(true);
    } else if (event.key === 'Tab') {
      closeAdd();
    }
  });

  // Con el ratón, se elige dónde ponerlo; con el teclado (`detail` 0), va al centro del hueco libre.
  function choose(item, byKeyboard) {
    if (item.tipo === 'imagen') {
      pendingAt = byKeyboard ? null : null;
      fileInput.click();
      return;
    }
    if (!byKeyboard) return api.arm(item);
    if (item.tipo === 'flecha') return api.arm(item);
    const [w, h] = item.template ? [660, 264] : [264, 88];
    const at = api.centerOfFree(w, h);
    api.createFromItem(item, at.x, at.y);
    return undefined;
  }
  addMenu.addEventListener('click', (event) => {
    const b = event.target.closest('[data-key]');
    if (!b) return;
    const item = itemByKey.get(b.dataset.key);
    if (!item) return;
    closeAdd(event.detail === 0 && item.tipo === 'flecha');
    choose(item, event.detail === 0);
  });
  tools.addEventListener('click', (event) => {
    const quick = event.target.closest('[data-quick]');
    if (quick) choose(itemByKey.get(quick.dataset.quick), event.detail === 0);
  });

  // Las imágenes: de un archivo o del portapapeles. Nunca de una dirección de fuera.
  async function placeImage(file, at) {
    const data = await new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result).split(',')[1] ?? '');
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(file);
    });
    let recurso;
    try {
      recurso = await api.invoke('canvas_image_save', { data });
    } catch (error) {
      return api.fail(error);
    }
    const url = URL.createObjectURL(file);
    const size = await new Promise((resolve) => {
      const img = new Image();
      img.onload = () => {
        URL.revokeObjectURL(url);
        resolve([img.naturalWidth, img.naturalHeight]);
      };
      img.onerror = () => {
        URL.revokeObjectURL(url);
        resolve([330, 220]);
      };
      img.src = url;
    });
    const k = Math.min(1, 660 / size[0], 440 / size[1]);
    const [w, h] = [Math.max(44, Math.round(size[0] * k)), Math.max(44, Math.round(size[1] * k))];
    const pos = at ?? api.centerOfFree(w, h);
    return api.createAndSelect([{ op: 'crear', tipo: 'imagen', x: pos.x, y: pos.y, w, h, datos: { recurso, alt: '' } }]);
  }
  fileInput.addEventListener('change', () => {
    const file = fileInput.files?.[0];
    fileInput.value = '';
    if (file) placeImage(file, pendingAt).catch(api.fail);
    pendingAt = null;
  });

  // ------------------------------------------------------------ capas

  function renderLayers() {
    const list = $('canvas-layers-list');
    const capas = api.capas();
    const all = api.all();
    const live = capas.filter((c) => c.estado === 'dibujando');
    layersBtn.classList.toggle('is-live', live.length > 0);
    layersBtn.setAttribute('aria-label', live.length ? `Capas: Nexo está dibujando ${live.map((c) => `«${c.nombre}»`).join(' y ')}` : 'Capas');
    if (layersMenu.hidden) return;
    list.replaceChildren(
      ...capas.map((capa) => {
        const n = all.filter((e) => e.capa === capa.id).length;
        const row = document.createElement('li');
        row.className = 'cv-layer-row';
        const info = document.createElement('div');
        info.className = 'cv-layer-info';
        const name = document.createElement('span');
        name.className = 'cv-layer-name';
        name.textContent = capa.nombre;
        const meta = document.createElement('span');
        meta.className = 'cv-layer-meta';
        const who = capa.autor === 'nexo' ? 'Nexo' : 'Tuya';
        const count = `${n} ${n === 1 ? 'elemento' : 'elementos'}`;
        meta.textContent = [who, count, LAYER_STATE[capa.estado]].filter(Boolean).join(' · ');
        info.append(name, meta);
        const act = (icon, label, handler, disabled = false) => {
          const b = document.createElement('button');
          b.type = 'button';
          b.className = 'cv-layer-act';
          b.innerHTML = icon;
          b.title = label;
          b.setAttribute('aria-label', `${label}: ${capa.nombre}`);
          b.disabled = disabled;
          b.addEventListener('click', handler);
          return b;
        };
        row.append(
          info,
          act(capa.visible === false ? ICON.eyeOff : ICON.eye, capa.visible === false ? 'Mostrar' : 'Ocultar', () => api.apply([{ op: 'capa-ver', capa: capa.id, visible: capa.visible === false }])),
          act(ICON.target, 'Ir a ella', () => {
            closeLayers();
            api.goTo(capa.id);
          }, n === 0),
        );
        if (capa.autor === 'nexo') {
          row.append(
            act(ICON.merge, 'Integrar en la tuya', () => api.apply([{ op: 'capa-integrar', capa: capa.id }])),
            act(ICON.trash, 'Borrar', async () => {
              const sure = await api.ask(`¿Borrar la capa «${capa.nombre}» con sus ${count}? Ctrl+Z la devuelve.`, { title: 'Borrar capa', kind: 'warning', okLabel: 'Borrar', cancelLabel: 'Cancelar' });
              if (sure) api.apply([{ op: 'capa-borrar', capa: capa.id }]);
            }),
          );
        }
        return row;
      }),
    );
  }
  function openLayers() {
    closeAdd();
    layersMenu.hidden = false;
    layersBtn.setAttribute('aria-expanded', 'true');
    renderLayers();
    layersMenu.querySelector('button')?.focus();
  }
  function closeLayers(refocus = false) {
    if (layersMenu.hidden) return;
    layersMenu.hidden = true;
    layersBtn.setAttribute('aria-expanded', 'false');
    if (refocus) layersBtn.focus();
  }
  layersBtn.addEventListener('click', () => (layersMenu.hidden ? openLayers() : closeLayers(true)));
  $('canvas-add-layers').addEventListener('click', () => {
    closeAdd();
    openLayers();
  });
  layersMenu.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') {
      event.stopPropagation();
      closeLayers(true);
    }
  });
  document.addEventListener('pointerdown', (event) => {
    if (!event.target.closest?.('#desk-tools')) {
      closeAdd();
      closeLayers();
    }
  });

  // ------------------------------------------------------------ barra de lo seleccionado

  const button = (paths, label, handler, { on = false } = {}) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'cv-bar-btn';
    b.innerHTML = svg(paths);
    b.title = label;
    b.setAttribute('aria-label', label);
    if (on) b.setAttribute('aria-pressed', 'true');
    b.addEventListener('click', handler);
    return b;
  };
  const choice = (label, options, value, handler) => {
    const s = document.createElement('select');
    s.className = 'cv-bar-select';
    s.setAttribute('aria-label', label);
    s.title = label;
    for (const [v, text] of options) {
      const o = document.createElement('option');
      o.value = v;
      o.textContent = text;
      s.append(o);
    }
    s.value = value;
    s.addEventListener('change', () => handler(s.value));
    return s;
  };
  const sep = () => {
    const s = document.createElement('span');
    s.className = 'cv-bar-sep';
    s.setAttribute('aria-hidden', 'true');
    return s;
  };
  const patch = (el, datos) => api.apply([{ op: 'cambiar', id: el.id, datos }]);

  function colorPicker(els) {
    const group = document.createElement('div');
    group.className = 'cv-swatches';
    group.setAttribute('role', 'group');
    group.setAttribute('aria-label', 'Color');
    const current = els[0].datos?.color;
    for (const c of COLORS) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = `cv-swatch cv-c-${c.id}`;
      b.title = c.label;
      b.setAttribute('aria-label', c.label);
      b.setAttribute('aria-pressed', String(c.id === current));
      b.addEventListener('click', () => api.apply(els.map((e) => ({ op: 'cambiar', id: e.id, datos: { color: c.id } }))));
      group.append(b);
    }
    return group;
  }

  function renderBar() {
    const sel = api.selected();
    if (!sel.length || !visible) {
      bar.hidden = true;
      return;
    }
    const parts = [];
    const el = sel[0];
    const single = sel.length === 1;
    const colored = sel.filter((e) => e.datos?.color !== undefined);
    if (single) {
      if (el.tipo === 'figura') parts.push(choice('Forma', FORMS.map((f) => [f, FORM_LABELS[f]]), el.datos.forma, (forma) => patch(el, { forma })));
      if (el.tipo === 'flecha') {
        parts.push(
          choice('Ruta', ROUTES, el.datos.ruta, (ruta) => patch(el, { ruta })),
          choice('Inicio', ENDS, el.datos.inicio, (inicio) => patch(el, { inicio })),
          choice('Fin', ENDS, el.datos.fin, (fin) => patch(el, { fin })),
          choice('Trazo', [['continuo', 'Continuo'], ['discontinuo', 'Discontinuo']], el.datos.trazo, (trazo) => patch(el, { trazo })),
        );
      }
      if (el.tipo === 'texto') {
        parts.push(
          choice('Tamaño', [['s', 'Pequeño'], ['m', 'Normal'], ['l', 'Grande'], ['xl', 'Título']], el.datos.tamano, (tamano) => patch(el, { tamano })),
          choice('Alineado', [['izq', 'Izquierda'], ['centro', 'Centro'], ['der', 'Derecha']], el.datos.alineado, (alineado) => patch(el, { alineado })),
        );
      }
      if (el.tipo === 'codigo') parts.push(choice('Lenguaje', LANGS.map((l) => [l, l || 'Sin lenguaje']), el.datos.lenguaje, (lenguaje) => patch(el, { lenguaje })));
      if (el.tipo === 'tabla') {
        const cell = api.grid.activeCell(el);
        if (cell) {
          const name = document.createElement('span');
          name.className = 'cv-cell-name';
          name.textContent = cell.name;
          name.title = cell.raw;
          parts.push(name);
        }
        const t = (paths, label, action) => button(paths, label, () => api.grid.tableAction(api.get(el.id), action));
        parts.push(t(ICON.addRow, 'Añadir fila', 'fila-nueva'), t(ICON.addCol, 'Añadir columna', 'columna-nueva'), t(ICON.delRow, 'Quitar fila', 'fila-borrar'), t(ICON.delCol, 'Quitar columna', 'columna-borrar'), t(ICON.sortUp, 'Ordenar de menor a mayor', 'orden-asc'), t(ICON.sortDown, 'Ordenar de mayor a menor', 'orden-desc'));
        parts.push(button(ICON.header, el.datos.cabecera === false ? 'Poner cabecera' : 'Quitar cabecera', () => api.grid.tableAction(api.get(el.id), 'cabecera'), { on: el.datos.cabecera !== false }));
      }
      if (el.tipo === 'tablero') parts.push(button(ICON.addCol, 'Añadir columna', () => api.grid.addColumn(api.get(el.id))));
      if (['entidad', 'secuencia', 'linea-tiempo', 'referencia', 'imagen'].includes(el.tipo)) parts.push(button(ICON.edit, 'Editar', () => openEditor(el)));
    } else {
      const solid = sel.filter((e) => e.tipo !== 'flecha');
      if (solid.length > 1) {
        parts.push(
          button(ICON.alignLeft, 'Alinear a la izquierda', () => api.align('izquierda')),
          button(ICON.alignCenter, 'Centrar en horizontal', () => api.align('centro')),
          button(ICON.alignRight, 'Alinear a la derecha', () => api.align('derecha')),
          button(ICON.alignTop, 'Alinear arriba', () => api.align('arriba')),
          button(ICON.alignMiddle, 'Centrar en vertical', () => api.align('medio')),
          button(ICON.alignBottom, 'Alinear abajo', () => api.align('abajo')),
        );
        if (solid.length > 2) parts.push(button(ICON.spreadX, 'Repartir en horizontal', () => api.distribute('x')), button(ICON.spreadY, 'Repartir en vertical', () => api.distribute('y')));
      }
    }
    if (colored.length) {
      if (parts.length) parts.push(sep());
      parts.push(colorPicker(colored));
    }
    if (parts.length) parts.push(sep());
    if (single && (el.tipo === 'texto' || el.tipo === 'adhesiva') && api.convertToNote) {
      parts.push(button(ICON.note, 'Convertir en una nota', () => api.convertToNote(el)));
    }
    parts.push(
      button(ICON.front, 'Traer al frente', () => api.reorder('frente')),
      button(ICON.back, 'Enviar al fondo', () => api.reorder('fondo')),
      button(ICON.duplicate, 'Duplicar', () => api.duplicate()),
      button(ICON.trashPath, 'Borrar', () => api.deleteSelection()),
    );
    bar.replaceChildren(...parts);
    bar.hidden = false;
    placeBar();
  }

  // Encima de lo seleccionado o, si no cabe, debajo; siempre dentro del hueco libre.
  function placeBar() {
    if (bar.hidden) return;
    const sel = api.selected();
    if (!sel.length) return;
    const screens = sel.map((e) => api.screenOf(api.rectOf(e)));
    const left = Math.min(...screens.map((s) => s.x));
    const top = Math.min(...screens.map((s) => s.y));
    const right = Math.max(...screens.map((s) => s.x + s.w));
    const bottom = Math.max(...screens.map((s) => s.y + s.h));
    const stage = $('stage').getBoundingClientRect();
    const b = bar.getBoundingClientRect();
    let y = top - b.height - 12;
    if (y < stage.top + 4) y = bottom + 12;
    y = Math.min(y, stage.bottom - b.height - 4);
    y = Math.max(y, stage.top + 4);
    const x = Math.max(stage.left + 4, Math.min((left + right) / 2 - b.width / 2, stage.right - b.width - 4));
    bar.style.left = `${Math.round(x)}px`;
    bar.style.top = `${Math.round(y)}px`;
  }

  // ------------------------------------------------------------ la ventana de editar

  let closeEditorDone = () => {};
  const modal = createModal({
    dialog: editor,
    closeButton: $('canvas-editor-close'),
    onClose: () => closeEditorDone(),
  });

  const row = (...children) => {
    const r = document.createElement('div');
    r.className = 'cv-form-row';
    r.append(...children);
    return r;
  };
  const input = (label, value = '', attrs = {}) => {
    const i = document.createElement('input');
    i.className = 'cv-form-input';
    i.value = value ?? '';
    i.placeholder = label;
    i.setAttribute('aria-label', label);
    Object.assign(i, attrs);
    return i;
  };
  const check = (label, on) => {
    const wrap = document.createElement('label');
    wrap.className = 'cv-form-check';
    const i = document.createElement('input');
    i.type = 'checkbox';
    i.checked = Boolean(on);
    wrap.append(i, label);
    return { wrap, input: i };
  };
  const removeBtn = (label, target) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'cv-form-remove';
    b.innerHTML = svg(ICON.trashPath, 14);
    b.title = label;
    b.setAttribute('aria-label', label);
    b.addEventListener('click', () => target.remove());
    return b;
  };
  const addBtnFor = (label, handler) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'cv-form-add';
    b.textContent = label;
    b.addEventListener('click', handler);
    return b;
  };
  const section = (title) => {
    const s = document.createElement('h3');
    s.className = 'cv-form-title';
    s.textContent = title;
    return s;
  };

  // Cada tipo: su formulario y lo que se guarda.
  const FORMS_BY_TYPE = {
    entidad(el, box) {
      const d = el.datos;
      const nombre = input('Nombre de la tabla', d.nombre);
      const list = document.createElement('div');
      const add = (c = {}) => {
        const nom = input('Columna', c.nombre);
        const tipo = input('Tipo', c.tipo);
        const flags = { pk: check('PK', c.pk), fk: check('FK', c.fk), nulo: check('Nulo', c.nulo), unico: check('Único', c.unico) };
        const r = row(nom, tipo, ...Object.values(flags).map((f) => f.wrap));
        r.dataset.id = c.id ?? '';
        r.read = () => ({ ...(c.id ? { id: c.id } : {}), nombre: nom.value, tipo: tipo.value, pk: flags.pk.input.checked, fk: flags.fk.input.checked, nulo: flags.nulo.input.checked, unico: flags.unico.input.checked });
        r.append(removeBtn('Quitar columna', r));
        list.append(r);
      };
      (d.columnas ?? []).forEach(add);
      box.append(row(nombre), section('Columnas'), list, addBtnFor('Añadir columna', () => add()));
      return () => {
        const columnas = [...list.children].map((r) => r.read()).filter((c) => c.nombre.trim());
        return { datos: { nombre: nombre.value, columnas }, h: 44 + 30 * Math.max(columnas.length, 1) };
      };
    },
    secuencia(el, box) {
      const d = el.datos;
      const titulo = input('Título', d.titulo);
      const people = document.createElement('div');
      const messages = document.createElement('div');
      const addP = (p = {}) => {
        const n = input('Participante', p.nombre);
        const r = row(n);
        r.read = () => ({ ...(p.id ? { id: p.id } : {}), nombre: n.value });
        r.append(removeBtn('Quitar participante', r));
        people.append(r);
      };
      const nameOf = (id) => (d.participantes ?? []).find((p) => p.id === id)?.nombre ?? id ?? '';
      const addM = (m = {}) => {
        const de = input('De', nameOf(m.de));
        const a = input('A', nameOf(m.a));
        const texto = input('Mensaje', m.texto);
        const tipo = choice('Tipo', [['sincrono', 'Llamada'], ['asincrono', 'Asíncrono'], ['respuesta', 'Respuesta']], m.tipo ?? 'sincrono', () => {});
        const r = row(de, a, texto, tipo);
        r.read = () => ({ ...(m.id ? { id: m.id } : {}), de: de.value.trim(), a: a.value.trim(), texto: texto.value, tipo: tipo.value });
        r.append(removeBtn('Quitar mensaje', r));
        messages.append(r);
      };
      (d.participantes ?? []).forEach(addP);
      (d.mensajes ?? []).forEach(addM);
      box.append(row(titulo), section('Participantes'), people, addBtnFor('Añadir participante', () => addP()), section('Mensajes, en orden'), messages, addBtnFor('Añadir mensaje', () => addM()));
      return () => {
        const participantes = [...people.children].map((r) => r.read()).filter((p) => p.nombre.trim());
        const names = new Set(participantes.map((p) => p.nombre));
        const mensajes = [...messages.children].map((r) => r.read()).filter((m) => names.has(m.de) && names.has(m.a));
        const w = Math.max(el.w, participantes.length * 176 + 44);
        const h = layoutSequence({ titulo: titulo.value, participantes: participantes.map((p, i) => ({ id: i, ...p })), mensajes: mensajes.map((m) => ({ ...m })) }, w).height;
        return { datos: { titulo: titulo.value, participantes, mensajes }, w, h };
      };
    },
    'linea-tiempo': (el, box) => {
      const d = el.datos;
      const titulo = input('Título', d.titulo);
      const tasks = document.createElement('div');
      const marks = document.createElement('div');
      const addT = (t = {}) => {
        const name = input('Tarea', t.titulo);
        const ini = input('Inicio', t.inicio, { type: 'date' });
        const fin = input('Fin', t.fin, { type: 'date' });
        const av = input('Avance %', t.avance ?? '', { inputMode: 'numeric' });
        const r = row(name, ini, fin, av);
        r.read = () => ({ ...(t.id ? { id: t.id } : {}), titulo: name.value, inicio: ini.value, fin: fin.value, dep: t.dep ?? [], avance: Number(av.value) || 0 });
        r.append(removeBtn('Quitar tarea', r));
        tasks.append(r);
      };
      const addH = (h = {}) => {
        const name = input('Hito', h.titulo);
        const when = input('Fecha', h.fecha, { type: 'date' });
        const r = row(name, when);
        r.read = () => ({ ...(h.id ? { id: h.id } : {}), titulo: name.value, fecha: when.value });
        r.append(removeBtn('Quitar hito', r));
        marks.append(r);
      };
      (d.tareas ?? []).forEach(addT);
      (d.hitos ?? []).forEach(addH);
      box.append(row(titulo), section('Tareas'), tasks, addBtnFor('Añadir tarea', () => addT()), section('Hitos'), marks, addBtnFor('Añadir hito', () => addH()));
      return () => {
        const tareas = [...tasks.children].map((r) => r.read()).filter((t) => t.titulo.trim());
        const hitos = [...marks.children].map((r) => r.read()).filter((h) => h.titulo.trim());
        const h = layoutTimeline({ titulo: titulo.value, tareas, hitos }, el.w).height;
        return { datos: { titulo: titulo.value, tareas, hitos }, h };
      };
    },
    referencia(el, box) {
      const d = el.datos;
      const tipo = choice('A qué apunta', [['nota', 'Una nota'], ['nodo', 'Un nodo del grafo'], ['fuente', 'Una fuente']], d.tipo, () => {});
      const ruta = input('Ruta', d.ruta, { type: 'text' });
      const options = document.createElement('datalist');
      options.id = 'canvas-note-paths';
      for (const p of api.notePaths?.() ?? []) {
        const o = document.createElement('option');
        o.value = p;
        options.append(o);
      }
      ruta.setAttribute('list', options.id);
      const titulo = input('Título visible', d.titulo);
      box.append(row(tipo), row(ruta), options, row(titulo));
      return () => ({ datos: { tipo: tipo.value, ruta: ruta.value.trim(), titulo: titulo.value } });
    },
    imagen(el, box) {
      const alt = input('Descripción de la imagen', el.datos.alt);
      box.append(row(alt));
      return () => ({ datos: { alt: alt.value } });
    },
  };

  const EDITOR_TITLE = { entidad: 'Tabla relacional', secuencia: 'Diagrama de secuencia', 'linea-tiempo': 'Línea de tiempo', referencia: 'Referencia', imagen: 'Imagen' };

  function openEditor(el) {
    const build = FORMS_BY_TYPE[el.tipo];
    if (!build) return;
    const box = $('canvas-editor-body');
    box.replaceChildren();
    $('canvas-editor-title').textContent = EDITOR_TITLE[el.tipo];
    const read = build(el, box);
    api.use(el.id, true);
    closeEditorDone = () => {
      closeEditorDone = () => {};
      api.use(el.id, false);
      api.focusEl(el.id);
    };
    $('canvas-editor-form').onsubmit = (event) => {
      event.preventDefault();
      const { datos, w, h } = read();
      api.apply([{ op: 'cambiar', id: el.id, datos, ...(w ? { w } : {}), ...(h ? { h } : {}) }]);
      modal.close();
    };
    modal.open();
    box.querySelector('input, select')?.focus();
  }

  return {
    changed: () => {
      renderLayers();
      renderBar();
    },
    selectionChanged: () => renderBar(),
    place: () => placeBar(),
    hideBar: () => {
      bar.hidden = true;
    },
    armed: (item) => {
      for (const b of tools.querySelectorAll('[data-quick]')) b.setAttribute('aria-pressed', String(b.dataset.quick === item?.key));
    },
    openEditor,
    pickImage(at) {
      pendingAt = at;
      fileInput.click();
    },
    placeImage,
    setVisible(on) {
      visible = on;
      tools.hidden = !on;
      if (!on) {
        closeAdd();
        closeLayers();
        bar.hidden = true;
      } else renderBar();
    },
  };
}

export { ICONS };
