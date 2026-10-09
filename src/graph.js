// Vista de grafo de la wiki, dibujada en canvas con d3-force: nodos como estrellas de neón suave.
import { forceCenter, forceCollide, forceLink, forceManyBody, forceSimulation, forceX, forceY } from 'd3-force';
import { select } from 'd3-selection';
import 'd3-transition';
import { zoom, zoomIdentity } from 'd3-zoom';

const FONT = "'Geist Variable', 'Geist', system-ui, sans-serif";
const WHITE = (a) => `rgba(255,255,255,${a})`;

// Paleta galaxia: neones desaturados, luminosos pero suaves sobre el negro.
const NEBULA = ['#b69cff', '#7fe7ff', '#ff8ad8', '#7dffcf', '#8fa8ff', '#ffb0c8', '#c9a8ff', '#9ff0ff'];
const STAR = '#ffd89a'; // fuentes: ámbar estelar
const NOVA = '#d6f4ff'; // conversaciones guardadas: blanco azulado con anillo
const CORE = '#ece6ff'; // notas de la raíz e índice maestro
const GHOST = '#a99bd6'; // enlaces a notas que aún no existen

function hexToRgb(hex) {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}
const rgba = (hex, a) => {
  const [r, g, b] = hexToRgb(hex);
  return `rgba(${r},${g},${b},${a})`;
};
function hashString(text) {
  let h = 2166136261;
  for (const c of text) h = Math.imul(h ^ c.charCodeAt(0), 16777619);
  return h >>> 0;
}
// Los índices de fuentes y de conversaciones se dibujan como minigalaxias.
const GALAXIES = new Set(['fuentes/_index.md', 'conversaciones/_index.md', 'notas/_index.md']);
const isGalaxy = (d) => GALAXIES.has(d.id);

export function colorFor(d) {
  if (d.kind === 'source') return STAR;
  if (d.kind === 'conversation') return NOVA;
  if (d.kind === 'missing') return GHOST;
  if (!d.group) return CORE;
  return NEBULA[hashString(d.group) % NEBULA.length];
}


export function createGraph(canvas, { getViewport, onSelect, onOpen }) {
  const ctx = canvas.getContext('2d');
  // Capa de efectos (pulso del nodo en foco y ondas de los nodos nuevos): se anima en cada
  // fotograma sin redibujar el grafo entero, que solo se repinta cuando algo cambia.
  const fx = document.createElement('canvas');
  fx.className = 'graph-canvas graph-fx';
  fx.setAttribute('aria-hidden', 'true');
  canvas.after(fx);
  const fxCtx = fx.getContext('2d');
  let width = 0;
  let height = 0;
  let dpr = 1;
  let nodes = [];
  let links = [];
  let adjacency = new Map();
  let signature = '';
  let transform = zoomIdentity;
  let selected = null;
  let hovered = null;
  let depth = 2;
  let showLabels = true;
  let scope = 'global';
  let highlight = null; // Set de ids a la profundidad elegida, o null sin selección
  let pulseStart = 0;
  let frame = 0;
  let fxFrame = 0;
  let fxDirty = null; // rectángulo (px del dispositivo) pintado en la capa de efectos
  let active = true; // con el grafo tapado (vista Notas, cartas expandidas) no se dibuja

  const simulation = forceSimulation()
    .force('link', forceLink().id((d) => d.id).distance(60).strength(0.5))
    .force('charge', forceManyBody().strength(-160).distanceMax(500))
    .force('center', forceCenter(0, 0))
    .force('x', forceX(0).strength(0.04))
    .force('y', forceY(0).strength(0.04))
    .force('collide', forceCollide((d) => radius(d) * (isGalaxy(d) ? 3 : 1) + 6))
    .on('tick', requestDraw);

  function radius(d) {
    const degree = adjacency.get(d.id)?.size ?? 0;
    if (d.kind === 'index') return 5 + Math.sqrt(degree) * 1.1;
    if (d.kind === 'missing') return 2.5;
    if (d.kind === 'source') return 3.2 + Math.sqrt(degree) * 0.9;
    if (d.kind === 'conversation') return 3.6 + Math.sqrt(degree) * 0.9;
    return 2 + Math.sqrt(degree) * 0.9;
  }

  function computeHighlight() {
    if (!selected) return null;
    const seen = new Set([selected.id]);
    let frontier = [selected.id];
    for (let level = 0; level < depth; level++) {
      const next = [];
      for (const id of frontier) {
        for (const other of adjacency.get(id) ?? []) {
          if (!seen.has(other)) {
            seen.add(other);
            next.push(other);
          }
        }
      }
      frontier = next;
    }
    return seen;
  }

  const isVisible = (d) => scope === 'global' || !highlight || highlight.has(d.id);

  // ---------- sprites ----------
  // Estrellas, halos y galaxias se pintan una vez en un lienzo pequeño (a la escala de
  // pantalla, en cuartos de píxel) y después solo se copian: los degradados radiales son
  // lo más caro del dibujo y el grafo se repinta en cada paso de la simulación.
  const sprites = new Map();
  function sprite(key, extent, paint) {
    let s = sprites.get(key);
    if (s) {
      sprites.delete(key); // el más usado pasa al final: se descartan los viejos
      sprites.set(key, s);
      return s;
    }
    const size = Math.max(2, Math.ceil(extent * 2) + 2);
    s = document.createElement('canvas');
    s.width = s.height = size;
    const c = s.getContext('2d');
    c.translate(size / 2, size / 2);
    paint(c);
    sprites.set(key, s);
    if (sprites.size > 800) sprites.delete(sprites.keys().next().value);
    return s;
  }
  const quarter = (v) => Math.max(0.25, Math.round(v * 4) / 4);

  // Cada nodo es una estrella: resplandor suave, núcleo casi blanco teñido y destello de cuatro puntas.
  function paintStar(c, r, color, a, kind) {
    const glow = c.createRadialGradient(0, 0, 0, 0, 0, r * 3.2);
    glow.addColorStop(0, rgba(color, 0.45 * a));
    glow.addColorStop(0.35, rgba(color, 0.12 * a));
    glow.addColorStop(1, rgba(color, 0));
    c.fillStyle = glow;
    c.beginPath();
    c.arc(0, 0, r * 3.2, 0, Math.PI * 2);
    c.fill();

    const spike = r * (kind === 'source' ? 3.2 : 2.3);
    const w = r * 0.32;
    c.fillStyle = rgba(color, 0.7 * a);
    c.beginPath();
    for (const [dx, dy] of [[1, 0], [0, 1], [-1, 0], [0, -1]]) {
      // Punta en rombo muy fino que se afila hacia fuera.
      c.moveTo(dy * w, -dx * w);
      c.lineTo(dx * spike, dy * spike);
      c.lineTo(-dy * w, dx * w);
    }
    c.fill();

    const core = c.createRadialGradient(0, 0, 0, 0, 0, r);
    core.addColorStop(0, `rgba(255,255,255,${0.95 * a})`);
    core.addColorStop(0.55, rgba(color, 0.95 * a));
    core.addColorStop(1, rgba(color, 0.55 * a));
    c.fillStyle = core;
    c.beginPath();
    c.arc(0, 0, r, 0, Math.PI * 2);
    c.fill();

    if (kind === 'conversation') {
      // Las conversaciones guardadas llevan un anillo, como una nova.
      c.beginPath();
      c.arc(0, 0, r * 2, 0, Math.PI * 2);
      c.strokeStyle = rgba(color, 0.55 * a);
      c.lineWidth = Math.max(1, r * 0.12);
      c.stroke();
    }
  }

  // Los índices de fuentes, conversaciones y notas (los centros a los que se unen) son
  // minigalaxias espirales: disco inclinado, dos brazos de polvo estelar y un núcleo brillante.
  function paintGalaxy(c, id, r, color, a) {
    const extent = r * 3.4;
    const tilt = ((hashString(id) % 360) * Math.PI) / 180;

    const glow = c.createRadialGradient(0, 0, 0, 0, 0, extent * 1.25);
    glow.addColorStop(0, rgba(color, 0.32 * a));
    glow.addColorStop(0.4, rgba(color, 0.1 * a));
    glow.addColorStop(1, rgba(color, 0));
    c.fillStyle = glow;
    c.beginPath();
    c.arc(0, 0, extent * 1.25, 0, Math.PI * 2);
    c.fill();

    c.save();
    c.rotate(tilt);
    c.scale(1, 0.62); // disco visto en perspectiva
    for (let arm = 0; arm < 2; arm++) {
      const start = arm * Math.PI;
      for (let i = 0; i < 22; i++) {
        const t = i / 21;
        const angle = start + t * Math.PI * 2.4;
        const dist = r * 0.55 + t * (extent - r * 0.55);
        // Pequeña dispersión fija para que el brazo parezca polvo y no una línea.
        const jitter = (((hashString(`${id}:${arm}:${i}`) % 100) / 100) - 0.5) * r * 0.45;
        c.beginPath();
        c.arc(Math.cos(angle) * (dist + jitter), Math.sin(angle) * (dist + jitter), Math.max(0.35, r * 0.2 * (1 - t * 0.7)), 0, Math.PI * 2);
        c.fillStyle = rgba(i % 5 === 0 ? '#ffffff' : color, (0.85 - t * 0.7) * a);
        c.fill();
      }
    }
    c.restore();

    const core = c.createRadialGradient(0, 0, 0, 0, 0, r * 0.95);
    core.addColorStop(0, `rgba(255,255,255,${0.97 * a})`);
    core.addColorStop(0.5, rgba(color, 0.9 * a));
    core.addColorStop(1, rgba(color, 0));
    c.fillStyle = core;
    c.beginPath();
    c.arc(0, 0, r * 0.95, 0, Math.PI * 2);
    c.fill();
  }

  function paintHalo(c, r, color, inner) {
    const g = c.createRadialGradient(0, 0, 0, 0, 0, r);
    g.addColorStop(0, rgba(color, inner));
    g.addColorStop(0.28, rgba(color, 0.12));
    g.addColorStop(1, rgba(color, 0));
    c.fillStyle = g;
    c.fillRect(-r, -r, r * 2, r * 2);
  }

  const BIRTH_MS = 6000;
  const births = new Map();

  function requestDraw() {
    if (!frame && active) frame = requestAnimationFrame(draw);
  }

  function requestFx() {
    if (!fxFrame && active && (selected || births.size || fxDirty)) fxFrame = requestAnimationFrame(drawFx);
  }

  // Coordenadas del grafo → píxeles CSS del lienzo.
  const screenX = (x) => x * transform.k + transform.x;
  const screenY = (y) => y * transform.k + transform.y;

  function draw() {
    frame = 0;
    if (!active) return;
    const k = transform.k;
    const scale = k * dpr; // píxeles del dispositivo por unidad del grafo
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    // Lo que queda fuera de la pantalla no se dibuja (con margen para resplandores y etiquetas).
    const margin = 120;
    const onScreen = (d) => {
      const x = screenX(d.x);
      const y = screenY(d.y);
      return x > -margin && y > -margin && x < width + margin && y < height + margin;
    };

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.translate(transform.x, transform.y);
    ctx.scale(k, k);

    if (selected) {
      for (const [r, a, dash] of [[70, 0.16, []], [140, 0.1, [1, 6]], [230, 0.06, []]]) {
        ctx.beginPath();
        ctx.setLineDash(dash.map((v) => v / k));
        ctx.arc(selected.x, selected.y, r, 0, Math.PI * 2);
        ctx.strokeStyle = rgba(colorFor(selected), a * 1.3);
        ctx.lineWidth = 1 / k;
        ctx.stroke();
      }
      ctx.setLineDash([]);
    }

    // aristas curvas, agrupadas por estilo: un solo trazo por grupo
    const groups = new Map();
    for (const l of links) {
      if (!isVisible(l.source) || !isVisible(l.target)) continue;
      const hot = selected && (l.source === selected || l.target === selected);
      const lit = highlight && highlight.has(l.source.id) && highlight.has(l.target.id);
      const tint = l.source.kind === 'source' ? colorFor(l.target) : colorFor(l.source);
      const alpha = hot ? 0.75 : lit ? 0.38 : highlight ? 0.07 : 0.2;
      const key = `${tint}|${alpha}|${hot ? 1 : 0}`;
      let group = groups.get(key);
      if (!group) groups.set(key, (group = { style: rgba(tint, alpha), width: (hot ? 1.2 : 0.7) / k, links: [] }));
      group.links.push(l);
    }
    for (const group of groups.values()) {
      ctx.beginPath();
      for (const l of group.links) {
        const { x: x1, y: y1 } = l.source;
        const { x: x2, y: y2 } = l.target;
        const s = l.index % 2 ? 0.12 : -0.12;
        ctx.moveTo(x1, y1);
        ctx.quadraticCurveTo((x1 + x2) / 2 - (y2 - y1) * s, (y1 + y2) / 2 + (x2 - x1) * s, x2, y2);
      }
      ctx.strokeStyle = group.style;
      ctx.lineWidth = group.width;
      ctx.stroke();
    }

    // De aquí en adelante, en píxeles del dispositivo: los sprites se copian sin escalar.
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    const blit = (img, d) => ctx.drawImage(img, screenX(d.x) * dpr - img.width / 2, screenY(d.y) * dpr - img.height / 2);

    // halos
    for (const d of nodes) {
      if (!isVisible(d) || isGalaxy(d) || (d.kind !== 'index' && d.kind !== 'source' && d.kind !== 'conversation' && d !== selected)) continue;
      if (!onScreen(d)) continue;
      const r = quarter((d === selected ? 90 : radius(d) * (d.kind === 'source' ? 4.5 : 6)) * scale);
      const color = colorFor(d);
      const inner = d.kind === 'source' ? 0.4 : 0.5;
      const img = sprite(`halo|${color}|${inner}|${r}`, r, (c) => paintHalo(c, r, color, inner));
      ctx.globalAlpha = highlight && !highlight.has(d.id) ? 0.35 : 1;
      blit(img, d);
      ctx.globalAlpha = 1;
    }

    // nodos
    for (const d of nodes) {
      if (!isVisible(d) || !onScreen(d)) continue;
      const dim = Boolean(highlight && !highlight.has(d.id));
      const size = d === hovered ? radius(d) + 1.5 : radius(d);
      const color = colorFor(d);
      if (d.kind === 'missing') {
        ctx.beginPath();
        ctx.arc(screenX(d.x) * dpr, screenY(d.y) * dpr, size * scale, 0, Math.PI * 2);
        ctx.setLineDash([1.5 * dpr, 1.5 * dpr]);
        ctx.strokeStyle = rgba(color, dim ? 0.25 : 0.6);
        ctx.lineWidth = dpr;
        ctx.stroke();
        ctx.setLineDash([]);
        continue;
      }
      const r = quarter(size * scale);
      const a = dim ? 0.35 : 1;
      const img = isGalaxy(d)
        ? sprite(`galaxy|${d.id}|${color}|${r}|${a}`, r * 3.4 * 1.25, (c) => paintGalaxy(c, d.id, r, color, a))
        : sprite(`star|${d.kind === 'source' || d.kind === 'conversation' ? d.kind : ''}|${color}|${r}|${a}`, r * 3.2, (c) => paintStar(c, r, color, a, d.kind));
      blit(img, d);
    }

    // nodo en foco: anillo y centro fijos (el pulso va en la capa de efectos)
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    if (selected) {
      const focus = colorFor(selected);
      const x = screenX(selected.x);
      const y = screenY(selected.y);
      ctx.beginPath();
      ctx.arc(x, y, 17 * k, 0, Math.PI * 2);
      ctx.strokeStyle = rgba(focus, 0.6);
      ctx.lineWidth = 1;
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(x, y, 7.5 * k, 0, Math.PI * 2);
      ctx.fillStyle = rgba(focus, 1);
      ctx.fill();
    }

    // etiquetas: índices y vecindario siempre; el resto al acercar
    if (showLabels) {
      ctx.textBaseline = 'middle';
      let font = '';
      for (const d of nodes) {
        if (!isVisible(d) || !onScreen(d)) continue;
        const inFocus = highlight?.has(d.id);
        const always = d.kind === 'index' || d.kind === 'source' || d.kind === 'conversation' || d === hovered || births.has(d.id) || (selected && inFocus);
        if (!always && k < 1.3) continue;
        const size = d === selected ? 15 : d.kind === 'index' ? 12.5 : 11;
        const next = `${d.kind === 'index' || d === selected ? 500 : 400} ${size}px ${FONT}`;
        if (next !== font) ctx.font = font = next;
        const strong = d.kind === 'index' || d.kind === 'source' || d.kind === 'conversation' || inFocus;
        ctx.fillStyle = d.kind === 'note' || d.kind === 'missing'
          ? WHITE(highlight && !inFocus ? 0.35 : strong ? 0.92 : 0.6)
          : rgba(colorFor(d), highlight && !inFocus ? 0.4 : 0.95);
        ctx.fillText(d.label, screenX(d.x) + (radius(d) + (d === selected ? 14 : 5)) * k, screenY(d.y));
      }
    }
    requestFx();
  }

  // Capa de efectos: solo se borra y repinta el rectángulo que ocupan los anillos.
  function drawFx(now) {
    fxFrame = 0;
    fxCtx.setTransform(1, 0, 0, 1, 0, 0);
    if (fxDirty) fxCtx.clearRect(...fxDirty);
    fxDirty = null;
    if (!active) return;
    const k = transform.k;
    let box = null;
    const ring = (d, r, style) => {
      const x = screenX(d.x);
      const y = screenY(d.y);
      fxCtx.beginPath();
      fxCtx.arc(x * dpr, y * dpr, r * dpr, 0, Math.PI * 2);
      fxCtx.strokeStyle = style;
      fxCtx.stroke();
      const pad = (r + 2) * dpr;
      const rect = [x * dpr - pad, y * dpr - pad, x * dpr + pad, y * dpr + pad];
      box = box ? [Math.min(box[0], rect[0]), Math.min(box[1], rect[1]), Math.max(box[2], rect[2]), Math.max(box[3], rect[3])] : rect;
    };
    fxCtx.lineWidth = dpr;

    // nodos recién creados: ondas que se expanden durante unos segundos
    for (const [id, start] of births) {
      const d = nodes.find((n) => n.id === id);
      const age = (now - start) / BIRTH_MS;
      if (!d || age >= 1) {
        births.delete(id);
        requestDraw(); // su etiqueta ya no tiene por qué seguir a la vista
        continue;
      }
      if (!isVisible(d) || d.x == null) continue;
      const color = colorFor(d);
      for (const lag of [0, 0.33, 0.66]) {
        const t = (age * 3 + lag) % 1;
        ring(d, (radius(d) + 46 * (1 - Math.pow(1 - t, 3))) * k, rgba(color, 0.55 * (1 - t) * (1 - age)));
      }
    }

    // nodo en foco con pulso
    if (selected) {
      const t = ((now - pulseStart) % 3200) / 3200;
      const eased = 1 - Math.pow(1 - t, 3);
      ring(selected, 14 * (0.6 + eased * 1.6) * k, rgba(colorFor(selected), 0.7 * (1 - t)));
    }

    if (box) fxDirty = [Math.floor(box[0]), Math.floor(box[1]), Math.ceil(box[2] - box[0]) + 1, Math.ceil(box[3] - box[1]) + 1];
    requestFx();
  }

  // ---------- interacción ----------
  function nodeAt(event) {
    const rect = canvas.getBoundingClientRect();
    const [x, y] = transform.invert([event.clientX - rect.left, event.clientY - rect.top]);
    let best = null;
    let bestDist = Infinity;
    for (const d of nodes) {
      if (!isVisible(d)) continue;
      const dist = Math.hypot(d.x - x, d.y - y);
      if (dist < Math.max(radius(d) + 4, 8 / transform.k) && dist < bestDist) {
        best = d;
        bestDist = dist;
      }
    }
    return best;
  }

  const zoomBehavior = zoom()
    .scaleExtent([0.2, 5])
    .filter((event) => !event.button && event.type !== 'dblclick')
    .on('zoom', (event) => {
      transform = event.transform;
      requestDraw();
    });
  select(canvas).call(zoomBehavior);

  canvas.addEventListener('pointermove', (event) => {
    const d = nodeAt(event);
    if (d !== hovered) {
      hovered = d;
      canvas.style.cursor = d ? 'pointer' : '';
      requestDraw();
    }
  });

  canvas.addEventListener('click', (event) => {
    if (event.defaultPrevented) return;
    const d = nodeAt(event);
    setSelected(d && d !== selected ? d : d ? selected : null);
  });

  canvas.addEventListener('dblclick', (event) => {
    const d = nodeAt(event);
    if (d && d.kind !== 'missing') onOpen(d);
  });

  function setSelected(d) {
    selected = d;
    highlight = computeHighlight();
    pulseStart = performance.now();
    onSelect(d ? { ...d, degree: adjacency.get(d.id)?.size ?? 0 } : null);
    requestDraw();
  }

  function resize() {
    const ratio = window.devicePixelRatio || 1;
    if (ratio !== dpr) sprites.clear();
    dpr = ratio;
    // Tamaño de maquetación y no getBoundingClientRect, que incluye las transformaciones CSS:
    // medido con el lienzo escalado, el mapa de bits se quedaba pequeño y el grafo se veía
    // estirado y sin responder al ratón donde estaban los nodos.
    width = canvas.clientWidth;
    height = canvas.clientHeight;
    canvas.width = fx.width = Math.round(width * dpr);
    canvas.height = fx.height = Math.round(height * dpr);
    fxDirty = null;
    requestDraw();
  }
  new ResizeObserver(resize).observe(canvas);
  // El canvas no hereda el CSS: se repinta cuando Geist termina de cargar para que las
  // etiquetas no se queden con la fuente del sistema.
  document.fonts.load(`500 12px ${FONT}`).finally(requestDraw);
  document.fonts.addEventListener('loadingdone', requestDraw);
  resize();

  // Vista que encaja los nodos visibles dentro del hueco central (entre los paneles).
  function fitTransform() {
    const view = getViewport();
    const visible = nodes.filter(isVisible);
    if (!visible.length) return zoomIdentity.translate(view.cx, view.cy);
    const xs = visible.map((d) => d.x);
    const ys = visible.map((d) => d.y);
    const [minX, maxX, minY, maxY] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
    const k = Math.min(2, 0.85 * Math.min(view.w / Math.max(maxX - minX, 1), view.h / Math.max(maxY - minY, 1)));
    return zoomIdentity.translate(view.cx, view.cy).scale(k).translate(-(minX + maxX) / 2, -(minY + maxY) / 2);
  }

  function fit(duration = 600) {
    select(canvas).transition().duration(duration).call(zoomBehavior.transform, fitTransform());
  }

  return {
    setData({ nodes: rawNodes, edges }) {
      // La wiki cambia a menudo sin tocar el grafo (p. ej. al guardar una nota sin enlaces
      // nuevos): si nodos y aristas son los mismos, no se reinicia la simulación.
      const next = JSON.stringify([rawNodes.map((n) => [n.id, n.label, n.kind, n.group, n.source]), edges]);
      if (next === signature) return;
      signature = next;
      const previous = new Map(nodes.map((d) => [d.id, d]));
      nodes = rawNodes.map((n) => Object.assign(previous.get(n.id) ?? {}, n));
      const ids = new Set(nodes.map((d) => d.id));
      links = edges.filter(([a, b]) => ids.has(a) && ids.has(b)).map(([source, target]) => ({ source, target }));
      adjacency = new Map(nodes.map((d) => [d.id, new Set()]));
      for (const { source, target } of links) {
        adjacency.get(source).add(target);
        adjacency.get(target).add(source);
      }
      const fresh = nodes.some((d) => !previous.has(d.id));
      simulation.nodes(nodes);
      simulation.force('link').links(links);
      if (selected && !ids.has(selected.id)) setSelected(null);
      highlight = computeHighlight();
      simulation.alpha(fresh ? 1 : 0.2).restart();
      if (fresh && !previous.size) {
        transform = zoomIdentity.translate(getViewport().cx, getViewport().cy);
        select(canvas).call(zoomBehavior.transform, transform);
        simulation.on('end.fit', () => {
          simulation.on('end.fit', null);
          fit();
        });
      }
      requestDraw();
    },
    setDepth(value) {
      depth = value;
      highlight = computeHighlight();
      requestDraw();
    },
    setLabels(value) {
      showLabels = value;
      requestDraw();
    },
    setScope(value) {
      scope = value;
      requestDraw();
      fit();
    },
    clearSelection: () => setSelected(null),
    // Señala nodos recién añadidos a la wiki (p. ej. por Claude Code) con unas ondas.
    announce(ids) {
      const now = performance.now();
      for (const id of ids) births.set(id, now);
      requestDraw();
    },
    // Con el grafo tapado (vista Notas o cartas expandidas) no se dibuja nada.
    setActive(value) {
      if (value === active) return;
      active = value;
      if (active) requestDraw();
      else {
        cancelAnimationFrame(frame);
        cancelAnimationFrame(fxFrame);
        frame = fxFrame = 0;
      }
    },
    // Enfoca el nodo con ese id (p. ej. al abrir una fuente) y lo centra.
    focus(id) {
      const d = nodes.find((n) => n.id === id);
      if (!d) return false;
      setSelected(d);
      const view = getViewport();
      const next = zoomIdentity.translate(view.cx, view.cy).scale(Math.max(transform.k, 1.4)).translate(-d.x, -d.y);
      select(canvas).transition().duration(700).call(zoomBehavior.transform, next);
      return true;
    },
    fit,
    // Al abrir el grafo desde la galaxia crece desde el centro del escenario hasta su vista
    // (la que tenía o, con `refit`, encajada). Se anima con el propio zoom: escalar el lienzo
    // con CSS obligaba a repintarlo entero en cada fotograma y descuadraba sus medidas.
    reveal({ refit = false, duration = 1000 } = {}) {
      const target = refit ? fitTransform() : transform;
      const { cx, cy } = getViewport();
      const s = 0.55;
      const from = zoomIdentity.translate(target.x * s + cx * (1 - s), target.y * s + cy * (1 - s)).scale(target.k * s);
      select(canvas).interrupt().call(zoomBehavior.transform, from);
      select(canvas)
        .transition()
        .duration(duration)
        .ease((t) => 1 - Math.pow(1 - t, 3))
        .call(zoomBehavior.transform, target);
    },
    get size() {
      return nodes.length;
    },
  };
}
