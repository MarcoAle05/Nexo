// Galaxia inicial: el plano principal antes de abrir el grafo. Miles de puntos blancos muy
// pequeños forman un núcleo grande y brillante y un aro que gira despacio a su alrededor,
// inclinado como la elipse del logo. Se dibuja con WebGL (un solo dibujado por fotograma:
// la GPU calcula la órbita de cada punto) y, si no hay WebGL, con un canvas 2D con menos puntos.

// Aspecto de la galaxia. Las distancias van en unidades de la galaxia (el aro mide ~0.9) y
// los tamaños de los puntos en píxeles CSS. Los colores siempre son blancos: cambia el brillo.
export const LOOK = {
  seed: 7,
  tilt: 0.36, // aplastamiento del disco visto en perspectiva (cos de la inclinación)
  angle: -24, // grados: el mismo giro que la elipse del logo
  speed: 1, // multiplica todas las velocidades de giro
  twinkle: 0.16, // cuánto titilan los puntos (0 = nada)
  core: { count: 11200, radius: 0.5, flatten: 0.8, size: [0.7, 1.4], alpha: [0.2, 0.62], spin: 0.05 },
  disk: { count: 1800, radius: 0.68, arms: 2, pitch: 0.32, scatter: 0.08, size: [0.6, 1.1], alpha: [0.08, 0.24] },
  ring: { count: 6200, radius: 0.9, width: 0.03, thickness: 0.008, size: [0.7, 1.5], alpha: [0.34, 0.9], clumps: 4, spin: 0.07 },
  halo: { count: 700, radius: 1.3, size: [0.6, 1.0], alpha: [0.05, 0.16], spin: 0.02 },
};

// Tipos de punto (el último dato de cada uno): el núcleo no se tapa a sí mismo; el disco y
// el aro sí quedan tapados por el núcleo cuando pasan por detrás.
const CORE = 0;
const DISC = 1;
const HALO = 2;

function random(seed) {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Puntos de la galaxia: por cada uno, su órbita (radio, ángulo inicial, altura sobre el
// disco, velocidad angular) y su aspecto (tamaño, brillo, fase del titileo, tipo).
// Determinista: la misma semilla da siempre la misma galaxia.
export function makePoints(look = LOOK) {
  const rand = random(look.seed);
  const gauss = () => Math.sqrt(-2 * Math.log(1 - rand())) * Math.cos(2 * Math.PI * rand());
  const between = ([a, b], t = rand()) => a + (b - a) * t;
  const total = look.core.count + look.disk.count + look.ring.count + look.halo.count;
  const orbit = new Float32Array(total * 4);
  const style = new Float32Array(total * 4);
  let n = 0;
  const push = (r, theta, z, omega, size, alpha, kind) => {
    orbit.set([r, theta, z, omega * look.speed], n * 4);
    style.set([size, alpha, rand(), kind], n * 4);
    n++;
  };
  // Curva de rotación plana: el aro gira a `ring.spin` y lo de dentro, más deprisa.
  const keplerish = (r) => look.ring.spin * (look.ring.radius / Math.max(r, 0.22));

  // Núcleo: un esferoide muy concentrado hacia el centro.
  const { core } = look;
  for (let i = 0; i < core.count; i++) {
    // Perfil de Plummer: muy denso en el centro, con una cola larga que se funde con el disco.
    const u = Math.min(rand(), 0.985);
    const s = Math.min(core.radius * 1.25, core.radius * 0.4 * Math.sqrt(Math.pow(u, -2 / 3) - 1));
    const cos = 2 * rand() - 1;
    const sin = Math.sqrt(1 - cos * cos);
    const near = 1 - s / (core.radius * 1.25); // 1 en el centro, 0 en el borde
    push(s * sin, rand() * Math.PI * 2, s * cos * core.flatten, core.spin * (0.85 + rand() * 0.3), between(core.size, near * 0.7 + rand() * 0.3), between(core.alpha, near * 0.8 + rand() * 0.2), CORE);
  }

  // Disco interior: polvo tenue en dos brazos espirales entre el núcleo y el aro.
  const { disk } = look;
  for (let i = 0; i < disk.count; i++) {
    const r = core.radius * 0.7 + (disk.radius - core.radius * 0.7) * Math.sqrt(rand());
    const arm = Math.floor(rand() * disk.arms);
    const theta = arm * ((Math.PI * 2) / disk.arms) + Math.log(r / (core.radius * 0.7)) / Math.tan(disk.pitch) + gauss() * disk.scatter * 3;
    push(r + gauss() * disk.scatter * 0.3, theta, gauss() * 0.012, keplerish(r), between(disk.size), between(disk.alpha), DISC);
  }

  // Aro: un anillo fino con grumos, la parte que más se ve girar.
  const { ring } = look;
  for (let i = 0; i < ring.count; i++) {
    let theta = rand() * Math.PI * 2;
    // Más puntos en unos tramos que en otros: el aro no es una línea perfecta.
    while (rand() > 0.7 + 0.3 * Math.sin(theta * ring.clumps + Math.sin(theta * 2) * 1.7) ** 2) theta = rand() * Math.PI * 2;
    const r = ring.radius + gauss() * ring.width;
    push(r, theta, gauss() * ring.thickness, keplerish(r), between(ring.size), between(ring.alpha), DISC);
  }

  // Halo: estrellas sueltas y muy tenues alrededor.
  const { halo } = look;
  for (let i = 0; i < halo.count; i++) {
    const r = core.radius + (halo.radius - core.radius) * Math.sqrt(rand());
    push(r, rand() * Math.PI * 2, gauss() * 0.12, halo.spin * (0.6 + rand() * 0.8), between(halo.size), between(halo.alpha), HALO);
  }
  return { orbit, style, count: n };
}

// Semiejes (en unidades de la galaxia) del rectángulo que ocupa la galaxia en pantalla,
// sea cual sea el giro de cada punto: la órbita proyectada de un punto es una elipse cuya
// caja se calcula directamente.
export function galaxyExtent(points, look = LOOK) {
  const ci = look.tilt;
  const si = Math.sqrt(1 - ci * ci);
  const turn = (look.angle * Math.PI) / 180;
  const tc = Math.cos(turn);
  const ts = Math.sin(turn);
  const au = Math.hypot(tc, ci * ts);
  const av = Math.hypot(ts, ci * tc);
  let x = 0;
  let y = 0;
  for (let i = 0; i < points.count; i++) {
    const r = points.orbit[i * 4];
    const z = points.orbit[i * 4 + 2];
    x = Math.max(x, r * au + Math.abs(z * si * ts));
    y = Math.max(y, r * av + Math.abs(z * si * tc));
  }
  return { x, y };
}

// Tamaño y centro de la galaxia dentro del hueco del escenario (px CSS de la ventana). El ancho
// manda casi siempre: deja sitio a los lados para la órbita de las skills (`ORBIT` en skills.js).
// En huecos estrechos (< ~600 px) los recuadros de los lados chocan con el borde del escenario,
// así que la galaxia encoge lo justo para que no pisen el anillo.
export function frameFor(rect) {
  const fit = Math.min(rect.width / 2.85, rect.height / 2.3, (rect.width - 300) / 1.45);
  const scale = Math.min(420, Math.max(80, fit));
  return { cx: rect.left + rect.width / 2, cy: rect.top + rect.height / 2 + 6, scale, rect };
}

const VERTEX = `
attribute vec4 a_orbit; // radio, ángulo inicial, altura, velocidad angular
attribute vec4 a_style; // tamaño, brillo, fase, tipo
uniform vec2 u_res;
uniform vec2 u_center;
uniform float u_scale;
uniform float u_time;
uniform float u_spread;
uniform float u_glow;
uniform vec2 u_tilt;
uniform vec2 u_turn;
uniform float u_dpr;
uniform float u_core;
uniform float u_twinkle;
varying float v_alpha;
void main() {
  float ang = a_orbit.y + a_orbit.w * u_time;
  float x = cos(ang) * a_orbit.x;
  float y = sin(ang) * a_orbit.x;
  float sy = y * u_tilt.x - a_orbit.z * u_tilt.y;
  float depth = y * u_tilt.y + a_orbit.z * u_tilt.x;
  vec2 p = vec2(x, sy);
  float spread = u_spread * u_spread;
  p *= 1.0 + spread * (1.2 + 2.4 * fract(a_style.z * 7.13));
  p = vec2(p.x * u_turn.x - p.y * u_turn.y, p.x * u_turn.y + p.y * u_turn.x);
  vec2 px = u_center + p * u_scale;
  gl_Position = vec4(px.x / u_res.x * 2.0 - 1.0, 1.0 - px.y / u_res.y * 2.0, 0.0, 1.0);
  float size = a_style.x * u_dpr;
  gl_PointSize = max(size, 1.0);
  float a = a_style.y * min(size, 1.0);
  a *= 1.0 - u_twinkle + u_twinkle * sin(u_time * (0.5 + fract(a_style.z * 3.7) * 1.8) + a_style.z * 6.2832);
  if (a_style.w > 0.5 && a_style.w < 1.5 && depth < 0.0) {
    a *= mix(0.08, 1.0, smoothstep(u_core * 0.7, u_core * 1.2, length(vec2(x, sy))));
  }
  a *= 1.0 + u_glow * 0.3;
  // En una galaxia pequeña los puntos (1 px como mínimo) se amontonan y saturan: se apagan un poco.
  a *= clamp(u_scale / (300.0 * u_dpr), 0.55, 1.0);
  v_alpha = a * pow(1.0 - u_spread, 1.5);
}`;

const FRAGMENT = `
precision mediump float;
varying float v_alpha;
void main() {
  vec2 c = gl_PointCoord - 0.5;
  float a = v_alpha * (1.0 - smoothstep(0.1, 0.25, dot(c, c)));
  gl_FragColor = vec4(a, a, a, a);
}`;

function webglRenderer(canvas, points) {
  const gl = canvas.getContext('webgl', { alpha: true, antialias: false, premultipliedAlpha: true, preserveDrawingBuffer: false });
  if (!gl) return null;
  const shader = (type, source) => {
    const s = gl.createShader(type);
    gl.shaderSource(s, source);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
    return s;
  };
  let program;
  try {
    program = gl.createProgram();
    gl.attachShader(program, shader(gl.VERTEX_SHADER, VERTEX));
    gl.attachShader(program, shader(gl.FRAGMENT_SHADER, FRAGMENT));
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program));
  } catch (error) {
    console.warn('galaxia: sin WebGL', error);
    return null;
  }
  gl.useProgram(program);
  const attribute = (name, data) => {
    const buffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
    const at = gl.getAttribLocation(program, name);
    gl.enableVertexAttribArray(at);
    gl.vertexAttribPointer(at, 4, gl.FLOAT, false, 0, 0);
  };
  attribute('a_orbit', points.orbit);
  attribute('a_style', points.style);
  const u = {};
  for (const name of ['res', 'center', 'scale', 'time', 'spread', 'glow', 'tilt', 'turn', 'dpr', 'core', 'twinkle']) u[name] = gl.getUniformLocation(program, `u_${name}`);
  gl.enable(gl.BLEND);
  gl.blendFunc(gl.ONE, gl.ONE); // luz que se suma: el núcleo denso brilla solo
  gl.clearColor(0, 0, 0, 0);
  return {
    kind: 'webgl',
    draw(state) {
      gl.viewport(0, 0, canvas.width, canvas.height);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.uniform2f(u.res, canvas.width, canvas.height);
      gl.uniform2f(u.center, state.cx * state.dpr, state.cy * state.dpr);
      gl.uniform1f(u.scale, state.scale * state.dpr);
      gl.uniform1f(u.time, state.time);
      gl.uniform1f(u.spread, state.spread);
      gl.uniform1f(u.glow, state.glow);
      gl.uniform2f(u.tilt, state.look.tilt, Math.sqrt(1 - state.look.tilt ** 2));
      const turn = (state.look.angle * Math.PI) / 180;
      gl.uniform2f(u.turn, Math.cos(turn), Math.sin(turn));
      gl.uniform1f(u.dpr, state.dpr);
      gl.uniform1f(u.core, state.look.core.radius);
      gl.uniform1f(u.twinkle, state.look.twinkle);
      gl.drawArrays(gl.POINTS, 0, points.count);
    },
  };
}

// Sin WebGL (pintado por CPU): uno de cada tres puntos, agrupados por brillo.
function canvasRenderer(canvas, points) {
  const ctx = canvas.getContext('2d');
  const step = 3;
  const BUCKETS = 8;
  const ids = [];
  for (let i = 0; i < points.count; i += step) ids.push(i);
  const bucket = new Uint8Array(points.count);
  for (const i of ids) bucket[i] = Math.min(BUCKETS - 1, Math.floor(points.style[i * 4 + 1] * BUCKETS));
  return {
    kind: '2d',
    draw(state) {
      const { orbit, style } = points;
      const { look, dpr } = state;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      ctx.globalCompositeOperation = 'lighter';
      ctx.fillStyle = '#fff';
      const ci = look.tilt;
      const si = Math.sqrt(1 - ci * ci);
      const turn = (look.angle * Math.PI) / 180;
      const tc = Math.cos(turn);
      const ts = Math.sin(turn);
      const fade = Math.pow(1 - state.spread, 1.5);
      const spread = state.spread * state.spread;
      const paths = Array.from({ length: BUCKETS }, () => new Path2D());
      for (const i of ids) {
        const o = i * 4;
        const ang = orbit[o + 1] + orbit[o + 3] * state.time;
        const x = Math.cos(ang) * orbit[o];
        const y = Math.sin(ang) * orbit[o];
        const sy = y * ci - orbit[o + 2] * si;
        const depth = y * si + orbit[o + 2] * ci;
        if (style[o + 3] === DISC && depth < 0 && Math.hypot(x, sy) < look.core.radius) continue;
        const k = 1 + spread * (1.2 + 2.4 * ((style[o + 2] * 7.13) % 1));
        const px = (state.cx + (x * tc - sy * ts) * k * state.scale) * dpr;
        const py = (state.cy + (x * ts + sy * tc) * k * state.scale) * dpr;
        const size = Math.max(1, style[o] * dpr);
        paths[bucket[i]].rect(px - size / 2, py - size / 2, size, size);
      }
      for (let b = 0; b < BUCKETS; b++) {
        const small = Math.min(1, Math.max(0.55, state.scale / 300));
        ctx.globalAlpha = Math.min(1, ((b + 0.5) / BUCKETS) * 1.6 * fade * small * (1 + state.glow * 0.3));
        ctx.fill(paths[b]);
      }
      ctx.globalAlpha = 1;
    },
  };
}

const ease = (t) => 1 - Math.pow(1 - t, 3);

// En reposo la galaxia se repinta como mucho a esta frecuencia: gira tan despacio (menos de
// medio píxel por fotograma) que a 120 Hz no se ve distinta, y cada fotograma de un lienzo
// animado le cuesta a WebKit copiarlo entero a la ventana. Las transiciones y el paso del
// ratón van a la frecuencia del monitor.
const IDLE_FPS = 60;

export function createGalaxy(canvas, { stage, onOpen }) {
  const points = makePoints(LOOK);
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)');
  let renderer = webglRenderer(canvas, points);
  if (!renderer) renderer = canvasRenderer(canvas, points);
  canvas.dataset.renderer = renderer.kind;

  const extent = galaxyExtent(points, LOOK);
  let dpr = 1;
  let frame = frameFor({ left: 0, top: 0, width: 800, height: 600 });
  let parent = { left: 0, top: 0, width: window.innerWidth, height: window.innerHeight };
  let box = null; // { x, y, w, h, dpr }: el lienzo dentro de su contenedor (px CSS)
  const listeners = new Set();
  let active = false;
  let raf = 0;
  let spread = 0; // 0 = galaxia en reposo, 1 = dispersa (grafo abierto)
  let tween = null; // { from, to, start, ms, done }
  let glow = 0;
  let hover = false;
  let frozenAt = 7.5; // con movimiento reducido, la galaxia queda quieta en este instante

  function measure() {
    const r = stage.getBoundingClientRect();
    if (!r.width || !r.height) return;
    const next = frameFor(r);
    if (next.cx === frame.cx && next.cy === frame.cy && next.scale === frame.scale) return;
    frame = next;
    for (const fn of listeners) fn(frame);
    request();
  }

  function resize() {
    dpr = window.devicePixelRatio || 1;
    const host = canvas.offsetParent ?? canvas.parentElement;
    if (host) parent = host.getBoundingClientRect();
    measure();
    request();
  }

  // En reposo el lienzo cubre solo la galaxia (con un margen), no la ventana: WebKit copia
  // cada fotograma del lienzo entero aunque apenas cambie, y así copia unas cuatro veces
  // menos. Al dispersarse (abrir o cerrar el grafo) los puntos salen de ese rectángulo y el
  // lienzo pasa a cubrir todo el contenedor hasta que vuelven a su sitio.
  function layoutBox() {
    let x = 0;
    let y = 0;
    let w = Math.round(parent.width);
    let h = Math.round(parent.height);
    if (spread < 0.001 && !tween) {
      const pad = 12;
      const cx = frame.cx - parent.left;
      const cy = frame.cy - parent.top;
      const hw = extent.x * frame.scale + pad;
      const hh = extent.y * frame.scale + pad;
      x = Math.max(0, Math.floor(cx - hw));
      y = Math.max(0, Math.floor(cy - hh));
      w = Math.min(w, Math.ceil(cx + hw)) - x;
      h = Math.min(h, Math.ceil(cy + hh)) - y;
    }
    if (box && box.x === x && box.y === y && box.w === w && box.h === h && box.dpr === dpr) return;
    box = { x, y, w, h, dpr };
    Object.assign(canvas.style, { left: `${x}px`, top: `${y}px`, width: `${w}px`, height: `${h}px` });
    canvas.width = Math.max(1, Math.round(w * dpr));
    canvas.height = Math.max(1, Math.round(h * dpr));
  }

  function request() {
    if (!raf && (active || tween)) raf = requestAnimationFrame(draw);
  }

  let drawnAt = 0;

  function draw(now) {
    raf = 0;
    const settled = !tween && glow === (hover ? 1 : 0);
    if (settled && now - drawnAt < 1000 / IDLE_FPS - 2) {
      request();
      return;
    }
    drawnAt = now;
    if (tween) {
      const t = Math.min(1, (now - tween.start) / tween.ms);
      spread = tween.from + (tween.to - tween.from) * ease(t);
      if (t >= 1) {
        const done = tween.done;
        tween = null;
        done?.();
      }
    }
    const target = hover ? 1 : 0;
    glow += (target - glow) * 0.12;
    if (Math.abs(target - glow) < 0.01) glow = target;
    const time = reduced.matches ? frozenAt : now / 1000;
    if (!reduced.matches) frozenAt = time;
    layoutBox();
    const cx = frame.cx - parent.left - box.x;
    const cy = frame.cy - parent.top - box.y;
    renderer.draw({ ...frame, cx, cy, time, spread, glow, dpr, look: LOOK });
    // Quieta (movimiento reducido) solo se repinta mientras algo cambia.
    if (tween || (active && (!reduced.matches || glow !== target))) request();
  }

  // La galaxia se centra en el hueco del escenario, que cambia al expandir, contraer o
  // minimizar las cartas, y el lienzo la sigue.
  window.addEventListener('resize', resize);
  new ResizeObserver(resize).observe(stage);
  resize();

  // ¿El punto (px CSS de la ventana) cae sobre la galaxia?
  function contains(x, y) {
    const turn = (-LOOK.angle * Math.PI) / 180;
    const dx = x - frame.cx;
    const dy = y - frame.cy;
    const u = (dx * Math.cos(turn) - dy * Math.sin(turn)) / frame.scale;
    const v = (dx * Math.sin(turn) + dy * Math.cos(turn)) / frame.scale;
    const rx = LOOK.ring.radius + LOOK.ring.width * 2;
    const ry = Math.max(rx * LOOK.tilt, LOOK.core.radius * 1.1);
    return (u / rx) ** 2 + (v / ry) ** 2 <= 1;
  }

  canvas.addEventListener('pointermove', (event) => {
    const inside = active && spread < 0.05 && contains(event.clientX, event.clientY);
    if (inside === hover) return;
    hover = inside;
    canvas.style.cursor = inside ? 'pointer' : '';
    request();
  });
  canvas.addEventListener('pointerleave', () => {
    hover = false;
    canvas.style.cursor = '';
    request();
  });
  canvas.addEventListener('click', (event) => {
    if (active && spread < 0.05 && contains(event.clientX, event.clientY)) onOpen();
  });

  function animate(to, ms) {
    return new Promise((resolve) => {
      const quick = reduced.matches ? Math.min(ms, 200) : ms;
      tween = { from: spread, to, start: performance.now(), ms: quick, done: resolve };
      request();
    });
  }

  return {
    // Dibujar o no (tapada por otra vista o por las cartas expandidas, no gasta nada).
    setActive(value) {
      if (value === active) return;
      active = value;
      if (active) {
        measure();
        request();
      } else {
        hover = false;
        canvas.style.cursor = '';
      }
    },
    // Al abrir el grafo los puntos se dispersan y se apagan; al volver se recogen.
    open: (ms = 900) => animate(1, ms),
    close: (ms = 1100) => animate(0, ms),
    // Sin animación (p. ej. al arrancar con el grafo abierto).
    set spread(value) {
      tween = null;
      spread = value;
      request();
    },
    get spread() {
      return spread;
    },
    contains,
    measure,
    get frame() {
      return frame;
    },
    // Avisa cuando cambian el centro o el tamaño (para colocar las skills alrededor).
    onFrame(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    get renderer() {
      return renderer.kind;
    },
  };
}
