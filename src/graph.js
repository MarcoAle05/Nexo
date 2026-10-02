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
const GALAXIES = new Set(['fuentes/_index.md', 'conversaciones/_index.md']);
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
  let width = 0;
  let height = 0;
  let dpr = 1;
  let nodes = [];
  let links = [];
  let adjacency = new Map();
  let transform = zoomIdentity;
  let selected = null;
  let hovered = null;
  let depth = 2;
  let showLabels = true;
  let scope = 'global';
  let highlight = null; // Set de ids a la profundidad elegida, o null sin selección
  let pulseStart = 0;
  let frame = 0;

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

  // ---------- dibujo ----------
  // Cada nodo es una estrella: resplandor suave, núcleo casi blanco teñido y destello de cuatro puntas.
  // Los índices de fuentes y de conversaciones (los centros a los que se unen) son minigalaxias
  // espirales: disco inclinado, dos brazos de polvo estelar y un núcleo brillante.
  function drawGalaxy(d, r, color, dim) {
    const a = dim ? 0.35 : 1;
    const extent = r * 3.4;
    const tilt = ((hashString(d.id) % 360) * Math.PI) / 180;

    const glow = ctx.createRadialGradient(d.x, d.y, 0, d.x, d.y, extent * 1.25);
    glow.addColorStop(0, rgba(color, 0.32 * a));
    glow.addColorStop(0.4, rgba(color, 0.1 * a));
    glow.addColorStop(1, rgba(color, 0));
    ctx.fillStyle = glow;
    ctx.beginPath();
    ctx.arc(d.x, d.y, extent * 1.25, 0, Math.PI * 2);
    ctx.fill();

    ctx.save();
    ctx.translate(d.x, d.y);
    ctx.rotate(tilt);
    ctx.scale(1, 0.62); // disco visto en perspectiva
    for (let arm = 0; arm < 2; arm++) {
      const start = arm * Math.PI;
      for (let i = 0; i < 22; i++) {
        const t = i / 21;
        const angle = start + t * Math.PI * 2.4;
        const dist = r * 0.55 + t * (extent - r * 0.55);
        // Pequeña dispersión fija para que el brazo parezca polvo y no una línea.
        const jitter = (((hashString(`${d.id}:${arm}:${i}`) % 100) / 100) - 0.5) * r * 0.45;
        const x = Math.cos(angle) * (dist + jitter);
        const y = Math.sin(angle) * (dist + jitter);
        ctx.beginPath();
        ctx.arc(x, y, Math.max(0.35, r * 0.2 * (1 - t * 0.7)), 0, Math.PI * 2);
        ctx.fillStyle = rgba(i % 5 === 0 ? '#ffffff' : color, (0.85 - t * 0.7) * a);
        ctx.fill();
      }
    }
    ctx.restore();

    const core = ctx.createRadialGradient(d.x, d.y, 0, d.x, d.y, r * 0.95);
    core.addColorStop(0, `rgba(255,255,255,${0.97 * a})`);
    core.addColorStop(0.5, rgba(color, 0.9 * a));
    core.addColorStop(1, rgba(color, 0));
    ctx.fillStyle = core;
    ctx.beginPath();
    ctx.arc(d.x, d.y, r * 0.95, 0, Math.PI * 2);
    ctx.fill();
  }

  function drawStar(d, r, color, dim, k) {
    const a = dim ? 0.35 : 1;
    const glow = ctx.createRadialGradient(d.x, d.y, 0, d.x, d.y, r * 3.2);
    glow.addColorStop(0, rgba(color, 0.45 * a));
    glow.addColorStop(0.35, rgba(color, 0.12 * a));
    glow.addColorStop(1, rgba(color, 0));
    ctx.fillStyle = glow;
    ctx.beginPath();
    ctx.arc(d.x, d.y, r * 3.2, 0, Math.PI * 2);
    ctx.fill();

    const spike = r * (d.kind === 'source' ? 3.2 : 2.3);
    const width = r * 0.32;
    ctx.fillStyle = rgba(color, 0.7 * a);
    ctx.beginPath();
    for (const [dx, dy] of [[1, 0], [0, 1], [-1, 0], [0, -1]]) {
      // Punta en rombo muy fino que se afila hacia fuera.
      ctx.moveTo(d.x + dy * width, d.y - dx * width);
      ctx.lineTo(d.x + dx * spike, d.y + dy * spike);
      ctx.lineTo(d.x - dy * width, d.y + dx * width);
    }
    ctx.fill();

    const core = ctx.createRadialGradient(d.x, d.y, 0, d.x, d.y, r);
    core.addColorStop(0, `rgba(255,255,255,${0.95 * a})`);
    core.addColorStop(0.55, rgba(color, 0.95 * a));
    core.addColorStop(1, rgba(color, 0.55 * a));
    ctx.fillStyle = core;
    ctx.beginPath();
    ctx.arc(d.x, d.y, r, 0, Math.PI * 2);
    ctx.fill();

    if (d.kind === 'conversation') {
      // Las conversaciones guardadas llevan un anillo, como una nova.
      ctx.beginPath();
      ctx.arc(d.x, d.y, r * 2, 0, Math.PI * 2);
      ctx.strokeStyle = rgba(color, 0.55 * a);
      ctx.lineWidth = 1 / k;
      ctx.stroke();
    }
  }

  const BIRTH_MS = 6000;
  const births = new Map();

  function requestDraw() {
    if (!frame) frame = requestAnimationFrame(draw);
  }

  function draw(now = performance.now()) {
    frame = 0;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    ctx.translate(transform.x, transform.y);
    ctx.scale(transform.k, transform.k);
    const k = transform.k;

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

    // aristas curvas
    for (const l of links) {
      if (!isVisible(l.source) || !isVisible(l.target)) continue;
      const hot = selected && (l.source === selected || l.target === selected);
      const lit = highlight && highlight.has(l.source.id) && highlight.has(l.target.id);
      const { x: x1, y: y1 } = l.source;
      const { x: x2, y: y2 } = l.target;
      const s = l.index % 2 ? 0.12 : -0.12;
      ctx.beginPath();
      ctx.moveTo(x1, y1);
      ctx.quadraticCurveTo((x1 + x2) / 2 - (y2 - y1) * s, (y1 + y2) / 2 + (x2 - x1) * s, x2, y2);
      const tint = l.source.kind === 'source' ? colorFor(l.target) : colorFor(l.source);
      ctx.strokeStyle = rgba(tint, hot ? 0.75 : lit ? 0.38 : highlight ? 0.07 : 0.2);
      ctx.lineWidth = (hot ? 1.2 : 0.7) / k;
      ctx.stroke();
    }

    // halos
    for (const d of nodes) {
      if (!isVisible(d) || isGalaxy(d) || (d.kind !== 'index' && d.kind !== 'source' && d.kind !== 'conversation' && d !== selected)) continue;
      const r = d === selected ? 90 : radius(d) * (d.kind === 'source' ? 4.5 : 6);
      const color = colorFor(d);
      const g = ctx.createRadialGradient(d.x, d.y, 0, d.x, d.y, r);
      g.addColorStop(0, rgba(color, d.kind === 'source' ? 0.4 : 0.5));
      g.addColorStop(0.28, rgba(color, 0.12));
      g.addColorStop(1, rgba(color, 0));
      ctx.fillStyle = g;
      ctx.globalAlpha = highlight && !highlight.has(d.id) ? 0.35 : 1;
      ctx.fillRect(d.x - r, d.y - r, r * 2, r * 2);
      ctx.globalAlpha = 1;
    }

    // nodos
    for (const d of nodes) {
      if (!isVisible(d)) continue;
      const dim = highlight && !highlight.has(d.id);
      const r = radius(d);
      ctx.beginPath();
      ctx.arc(d.x, d.y, d === hovered ? r + 1.5 : r, 0, Math.PI * 2);
      const color = colorFor(d);
      if (d.kind === 'missing') {
        ctx.setLineDash([1.5 / k, 1.5 / k]);
        ctx.strokeStyle = rgba(color, dim ? 0.25 : 0.6);
        ctx.lineWidth = 1 / k;
        ctx.stroke();
        ctx.setLineDash([]);
      } else {
        const size = d === hovered ? r + 1.5 : r;
        if (isGalaxy(d)) drawGalaxy(d, size, color, dim);
        else drawStar(d, size, color, dim, k);
      }
    }

    // nodos recién creados: ondas que se expanden durante unos segundos
    for (const [id, start] of births) {
      const d = nodes.find((n) => n.id === id);
      const age = (now - start) / BIRTH_MS;
      if (!d || age >= 1) {
        births.delete(id);
        continue;
      }
      if (!isVisible(d) || d.x == null) continue;
      const color = colorFor(d);
      for (const lag of [0, 0.33, 0.66]) {
        const t = (age * 3 + lag) % 1;
        ctx.beginPath();
        ctx.arc(d.x, d.y, radius(d) + 46 * (1 - Math.pow(1 - t, 3)), 0, Math.PI * 2);
        ctx.strokeStyle = rgba(color, 0.55 * (1 - t) * (1 - age));
        ctx.lineWidth = 1 / k;
        ctx.stroke();
      }
    }
    if (births.size) requestDraw();

    // nodo en foco con pulso
    if (selected) {
      const t = ((now - pulseStart) % 3200) / 3200;
      const eased = 1 - Math.pow(1 - t, 3);
      ctx.beginPath();
      ctx.arc(selected.x, selected.y, 14 * (0.6 + eased * 1.6), 0, Math.PI * 2);
      const focus = colorFor(selected);
      ctx.strokeStyle = rgba(focus, 0.7 * (1 - t));
      ctx.lineWidth = 1 / k;
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(selected.x, selected.y, 17, 0, Math.PI * 2);
      ctx.strokeStyle = rgba(focus, 0.6);
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(selected.x, selected.y, 7.5, 0, Math.PI * 2);
      ctx.fillStyle = rgba(focus, 1);
      ctx.fill();
      requestDraw();
    }

    // etiquetas: índices y vecindario siempre; el resto al acercar
    if (showLabels) {
      ctx.textBaseline = 'middle';
      for (const d of nodes) {
        if (!isVisible(d)) continue;
        const inFocus = highlight?.has(d.id);
        const always = d.kind === 'index' || d.kind === 'source' || d.kind === 'conversation' || d === hovered || births.has(d.id) || (selected && inFocus);
        if (!always && k < 1.3) continue;
        const size = (d === selected ? 15 : d.kind === 'index' ? 12.5 : 11) / k;
        ctx.font = `${d.kind === 'index' || d === selected ? 500 : 400} ${size}px ${FONT}`;
        const strong = d.kind === 'index' || d.kind === 'source' || d.kind === 'conversation' || inFocus;
        ctx.fillStyle = d.kind === 'note' || d.kind === 'missing'
          ? WHITE(highlight && !inFocus ? 0.35 : strong ? 0.92 : 0.6)
          : rgba(colorFor(d), highlight && !inFocus ? 0.4 : 0.95);
        ctx.fillText(d.label, d.x + radius(d) + (d === selected ? 14 : 5), d.y);
      }
    }
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
    const rect = canvas.getBoundingClientRect();
    dpr = window.devicePixelRatio || 1;
    width = rect.width;
    height = rect.height;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    requestDraw();
  }
  new ResizeObserver(resize).observe(canvas);
  // El canvas no hereda el CSS: se repinta cuando Geist termina de cargar para que las
  // etiquetas no se queden con la fuente del sistema.
  document.fonts.load(`500 12px ${FONT}`).finally(requestDraw);
  document.fonts.addEventListener('loadingdone', requestDraw);
  resize();

  // Encaja los nodos visibles dentro del hueco central (entre los paneles).
  function fit(duration = 600) {
    const view = getViewport();
    const visible = nodes.filter(isVisible);
    if (!visible.length) {
      select(canvas).transition().duration(duration).call(zoomBehavior.transform, zoomIdentity.translate(view.cx, view.cy));
      return;
    }
    const xs = visible.map((d) => d.x);
    const ys = visible.map((d) => d.y);
    const [minX, maxX, minY, maxY] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
    const k = Math.min(2, 0.85 * Math.min(view.w / Math.max(maxX - minX, 1), view.h / Math.max(maxY - minY, 1)));
    const next = zoomIdentity.translate(view.cx, view.cy).scale(k).translate(-(minX + maxX) / 2, -(minY + maxY) / 2);
    select(canvas).transition().duration(duration).call(zoomBehavior.transform, next);
  }

  return {
    setData({ nodes: rawNodes, edges }) {
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
    get size() {
      return nodes.length;
    },
  };
}
