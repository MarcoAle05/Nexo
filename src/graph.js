// Vista de grafo de la wiki, dibujada en canvas con d3-force (estilo "Órbita").
import { forceCenter, forceCollide, forceLink, forceManyBody, forceSimulation, forceX, forceY } from 'd3-force';
import { select } from 'd3-selection';
import 'd3-transition';
import { zoom, zoomIdentity } from 'd3-zoom';

const FONT = "'Geist Variable', 'Geist', system-ui, sans-serif";
const WHITE = (a) => `rgba(255,255,255,${a})`;

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
    .force('collide', forceCollide((d) => radius(d) + 6))
    .on('tick', requestDraw);

  function radius(d) {
    const degree = adjacency.get(d.id)?.size ?? 0;
    if (d.kind === 'index') return 5 + Math.sqrt(degree) * 1.1;
    if (d.kind === 'missing') return 2.5;
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
        ctx.strokeStyle = WHITE(a);
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
      ctx.strokeStyle = WHITE(hot ? 0.62 : lit ? 0.3 : highlight ? 0.06 : 0.14);
      ctx.lineWidth = (hot ? 1.1 : 0.7) / k;
      ctx.stroke();
    }

    // halos
    for (const d of nodes) {
      if (!isVisible(d) || (d.kind !== 'index' && d !== selected)) continue;
      const r = d === selected ? 90 : radius(d) * 6;
      const g = ctx.createRadialGradient(d.x, d.y, 0, d.x, d.y, r);
      g.addColorStop(0, WHITE(0.5));
      g.addColorStop(0.28, WHITE(0.12));
      g.addColorStop(1, WHITE(0));
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
      if (d.kind === 'missing') {
        ctx.setLineDash([1.5 / k, 1.5 / k]);
        ctx.strokeStyle = WHITE(dim ? 0.2 : 0.5);
        ctx.lineWidth = 1 / k;
        ctx.stroke();
        ctx.setLineDash([]);
      } else {
        ctx.fillStyle = WHITE(dim ? 0.35 : 0.95);
        ctx.fill();
      }
    }

    // nodo en foco con pulso
    if (selected) {
      const t = ((now - pulseStart) % 3200) / 3200;
      const eased = 1 - Math.pow(1 - t, 3);
      ctx.beginPath();
      ctx.arc(selected.x, selected.y, 14 * (0.6 + eased * 1.6), 0, Math.PI * 2);
      ctx.strokeStyle = WHITE(0.6 * (1 - t));
      ctx.lineWidth = 1 / k;
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(selected.x, selected.y, 17, 0, Math.PI * 2);
      ctx.strokeStyle = WHITE(0.55);
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(selected.x, selected.y, 7.5, 0, Math.PI * 2);
      ctx.fillStyle = WHITE(1);
      ctx.fill();
      requestDraw();
    }

    // etiquetas: índices y vecindario siempre; el resto al acercar
    if (showLabels) {
      ctx.textBaseline = 'middle';
      for (const d of nodes) {
        if (!isVisible(d)) continue;
        const inFocus = highlight?.has(d.id);
        const always = d.kind === 'index' || d === hovered || (selected && inFocus);
        if (!always && k < 1.3) continue;
        const size = (d === selected ? 15 : d.kind === 'index' ? 12.5 : 11) / k;
        ctx.font = `${d.kind === 'index' || d === selected ? 500 : 400} ${size}px ${FONT}`;
        ctx.fillStyle = WHITE(highlight && !inFocus ? 0.35 : d.kind === 'index' || inFocus ? 0.92 : 0.6);
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
    fit,
    get size() {
      return nodes.length;
    },
  };
}
