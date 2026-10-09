// Skills del usuario (las de Claude Code del vault, en .claude/skills/): recuadros alrededor
// de la galaxia, un resumen al pulsar uno y la carta Agentes → Skills, desde donde se piden
// nuevas al agente creador-de-skills.
import { reconcile } from './dom.js';

// Órbita de los recuadros, en unidades de la galaxia (como LOOK en galaxy.js): elipse un
// poco girada, por fuera del aro para no tapar la galaxia.
export const ORBIT = {
  rx: 1.34, // semieje horizontal
  ry: 0.86, // semieje vertical
  angle: -10, // grados
  start: -2.25, // ángulo (radianes) del primer recuadro: arriba a la izquierda
  gap: 10, // px mínimos entre recuadros
  margin: 14, // px mínimos hasta el borde del escenario
  max: 10, // recuadros visibles; el resto se resume en «+N»
};

// Centros (px dentro del escenario) para recuadros de los tamaños dados, repartidos por la
// órbita sin pisarse ni salirse del escenario. `frame` es { cx, cy, scale } ya relativo al
// escenario y `bounds` su { width, height }. Función pura: se prueba sin DOM.
export function orbitLayout(sizes, frame, bounds, orbit = ORBIT) {
  const n = sizes.length;
  if (!n) return [];
  const a = orbit.rx * frame.scale;
  const b = orbit.ry * frame.scale;
  const turn = (orbit.angle * Math.PI) / 180;
  const at = (theta) => {
    const x = a * Math.cos(theta);
    const y = b * Math.sin(theta);
    return [frame.cx + x * Math.cos(turn) - y * Math.sin(turn), frame.cy + x * Math.sin(turn) + y * Math.cos(turn)];
  };
  const angles = sizes.map((_, i) => orbit.start + (i * Math.PI * 2) / n);
  const clamp = ([x, y], [w, h]) => [
    Math.min(Math.max(x, orbit.margin + w / 2), Math.max(orbit.margin + w / 2, bounds.width - orbit.margin - w / 2)),
    Math.min(Math.max(y, orbit.margin + h / 2), Math.max(orbit.margin + h / 2, bounds.height - orbit.margin - h / 2)),
  ];
  const overlap = (p, q, sp, sq) =>
    Math.abs(p[0] - q[0]) < (sp[0] + sq[0]) / 2 + orbit.gap && Math.abs(p[1] - q[1]) < (sp[1] + sq[1]) / 2 + orbit.gap;
  // Si dos se pisan, se separan un poco a lo largo de la órbita, en el sentido en que ya
  // están ordenados; unas decenas de pasadas bastan para los tamaños habituales.
  for (let pass = 0; pass < 80; pass++) {
    const centers = angles.map((theta, i) => clamp(at(theta), sizes[i]));
    let moved = false;
    for (let i = 0; i < n; i++) {
      for (let j = i + 1; j < n; j++) {
        if (!overlap(centers[i], centers[j], sizes[i], sizes[j])) continue;
        let d = angles[j] - angles[i];
        d = Math.atan2(Math.sin(d), Math.cos(d));
        const push = d >= 0 ? 0.02 : -0.02;
        angles[i] -= push;
        angles[j] += push;
        moved = true;
      }
    }
    if (!moved) break;
  }
  return angles.map((theta, i) => clamp(at(theta), sizes[i]));
}

const SKILL_ICON =
  '<svg width="15" height="15" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="14" height="14" rx="4.5"/><path d="M11.5 6.5l-3 7"/></svg>';
const USE_ICON =
  '<svg width="14" height="14" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M7 5.5v9l7-4.5z"/></svg>';
const TRASH_ICON =
  '<svg width="14" height="14" viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4 6h12M8 6V4h4v2M6 6l1 10h6l1-10"/></svg>';

const PENDING_MS = 20 * 60 * 1000; // una petición sin skill nueva en 20 min deja de mostrarse

// Nombre con el que se invoca en Claude Code (/<nombre>).
export const commandFor = (skill) => (/^[a-z0-9][a-z0-9-]{0,63}$/.test(skill.name) ? skill.name : skill.id);

export function createSkills({ invoke, ask, fail, galaxy, stage, onUse, onCreate, onShowAgents }) {
  const $ = (id) => document.getElementById(id);
  const orbitEl = $('skill-orbit');
  const pop = $('skill-pop');
  const list = $('skills-list');
  const form = $('skill-create');
  const input = $('skill-create-input');
  let skills = [];
  let pending = []; // { key, text, since }
  let openId = null;
  let fresh = new Set(); // recién creadas: su recuadro entra con un destello

  async function load() {
    try {
      skills = await invoke('skills_list');
    } catch {
      skills = [];
    }
    render();
  }

  // Skills nuevas que llegan del vigilante: cada una cierra una petición pendiente.
  function announce(ids) {
    for (const id of ids) fresh.add(id);
    pending = pending.slice(ids.length);
    load();
  }

  function render() {
    const now = Date.now();
    pending = pending.filter((p) => now - p.since < PENDING_MS);
    if (openId && !skills.some((s) => s.id === openId)) closePop();
    renderOrbit();
    renderList();
  }

  // ---------- recuadros alrededor de la galaxia ----------
  function renderOrbit() {
    const shown = skills.length > ORBIT.max ? skills.slice(0, ORBIT.max - 1) : skills;
    const items = shown.map((s) => ({ kind: 'skill', skill: s }));
    if (skills.length > shown.length) items.push({ kind: 'more', count: skills.length - shown.length });
    if (!skills.length) items.push({ kind: 'new' });
    reconcile(orbitEl, items, {
      key: (item) => (item.kind === 'skill' ? `s:${item.skill.id}` : item.kind),
      sig: (item) => (item.kind === 'skill' ? item.skill.name : item.kind === 'more' ? item.count : ''),
      build(item) {
        const pill = document.createElement('button');
        pill.type = 'button';
        pill.className = 'skill-pill';
        if (item.kind === 'skill') {
          pill.dataset.id = item.skill.id;
          pill.setAttribute('aria-haspopup', 'dialog');
          pill.innerHTML = '<span class="skill-pill-dot" aria-hidden="true"></span><span class="skill-pill-name"></span>';
          pill.querySelector('.skill-pill-name').textContent = item.skill.name;
          pill.addEventListener('click', (event) => {
            event.stopPropagation();
            if (openId === item.skill.id) closePop();
            else openPop(item.skill.id);
          });
          if (fresh.delete(item.skill.id)) pill.classList.add('born');
        } else if (item.kind === 'more') {
          pill.classList.add('skill-pill-more');
          pill.textContent = `+${item.count}`;
          pill.title = 'Ver todas en Agentes';
          pill.addEventListener('click', () => onShowAgents());
        } else {
          pill.classList.add('skill-pill-new');
          pill.innerHTML = '<span class="skill-pill-plus" aria-hidden="true">+</span><span class="skill-pill-name">Crear una skill</span>';
          pill.addEventListener('click', () => onShowAgents({ add: true }));
        }
        return pill;
      },
    });
    for (const pill of orbitEl.children) pill.setAttribute('aria-expanded', String(pill.dataset.id === openId));
    relayout();
  }

  function relayout() {
    const pills = [...orbitEl.children];
    if (!pills.length) return;
    const rect = stage.getBoundingClientRect();
    if (!rect.width) return;
    const frame = galaxy.frame;
    const local = { cx: frame.cx - rect.left, cy: frame.cy - rect.top, scale: frame.scale };
    // Primero se mide todo y después se coloca: una sola pasada de maquetación.
    const sizes = pills.map((p) => [p.offsetWidth, p.offsetHeight]);
    const centers = orbitLayout(sizes, local, rect);
    pills.forEach((p, i) => {
      p.style.left = `${Math.round(centers[i][0])}px`;
      p.style.top = `${Math.round(centers[i][1])}px`;
    });
    if (openId) placePop();
  }

  // ---------- resumen de una skill ----------
  function openPop(id) {
    const skill = skills.find((s) => s.id === id);
    if (!skill) return;
    openId = id;
    $('skill-pop-name').textContent = skill.name;
    $('skill-pop-text').textContent = skill.description || 'Esta skill aún no tiene descripción.';
    $('skill-pop-command').textContent = `/${commandFor(skill)}`;
    pop.hidden = false;
    for (const pill of orbitEl.children) pill.setAttribute('aria-expanded', String(pill.dataset.id === id));
    placePop();
  }

  function closePop() {
    openId = null;
    pop.hidden = true;
    for (const pill of orbitEl.children) pill.setAttribute('aria-expanded', 'false');
  }

  // Junto a su recuadro, del lado contrario a la galaxia y sin salirse del escenario.
  function placePop() {
    const pill = orbitEl.querySelector(`[data-id="${CSS.escape(openId)}"]`);
    const rect = stage.getBoundingClientRect();
    const frame = galaxy.frame;
    const px = pill ? pill.offsetLeft : frame.cx - rect.left;
    const py = pill ? pill.offsetTop : frame.cy - rect.top;
    const ph = pill ? pill.offsetHeight / 2 : 0;
    const w = pop.offsetWidth;
    const h = pop.offsetHeight;
    const below = py < frame.cy - rect.top;
    const margin = ORBIT.margin;
    const x = Math.min(Math.max(px - w / 2, margin), rect.width - margin - w);
    let y = below ? py + ph + 10 : py - ph - 10 - h;
    y = Math.min(Math.max(y, margin), rect.height - margin - h);
    pop.style.left = `${Math.round(x)}px`;
    pop.style.top = `${Math.round(y)}px`;
    pop.dataset.side = below ? 'below' : 'above';
  }

  $('skill-pop-use').addEventListener('click', () => {
    const skill = skills.find((s) => s.id === openId);
    closePop();
    if (skill) onUse(skill);
  });
  document.addEventListener('click', (event) => {
    if (!pop.hidden && !pop.contains(event.target) && !event.target.closest?.('.skill-pill')) closePop();
  });
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && !pop.hidden) {
      const pill = orbitEl.querySelector(`[data-id="${CSS.escape(openId)}"]`);
      closePop();
      pill?.focus();
    }
  });

  galaxy.onFrame(() => relayout());
  document.fonts?.ready.then(() => relayout());

  // ---------- carta Agentes → Skills ----------
  function renderList() {
    $('skills-count').textContent = String(skills.length);
    const items = [
      ...pending.map((p) => ({ kind: 'pending', ...p })),
      ...skills.map((s) => ({ kind: 'skill', skill: s })),
    ];
    $('skills-empty').hidden = items.length > 0;
    $('skills-label').hidden = items.length === 0;
    reconcile(list, items, {
      key: (item) => (item.kind === 'skill' ? `s:${item.skill.id}` : `p:${item.key}`),
      sig: (item) => (item.kind === 'skill' ? `${item.skill.name}|${item.skill.description}|${item.skill.modified}` : item.text),
      build: (item) => (item.kind === 'skill' ? skillRow(item.skill) : pendingRow(item)),
    });
  }

  function skillRow(skill) {
    const li = document.createElement('li');
    li.className = 'notes-row skill-row';
    li.innerHTML = `
      <button type="button" class="row-main">
        <span class="row-icon">${SKILL_ICON}</span>
        <span class="row-text">
          <span class="row-title"></span>
          <span class="row-meta"><span class="row-meta-text"></span></span>
        </span>
      </button>
      <div class="row-actions">
        <button type="button" class="row-action" data-act="use">${USE_ICON}</button>
        <button type="button" class="row-action" data-act="remove">${TRASH_ICON}</button>
      </div>`;
    li.querySelector('.row-title').textContent = skill.name;
    li.querySelector('.row-meta-text').textContent = skill.description || `/${commandFor(skill)}`;
    li.querySelector('.row-main').title = skill.description;
    const use = li.querySelector('[data-act="use"]');
    use.setAttribute('aria-label', `Usar ${skill.name} en Claude Code`);
    use.title = 'Usar en Claude Code';
    const remove = li.querySelector('[data-act="remove"]');
    remove.setAttribute('aria-label', `Mover ${skill.name} a la papelera`);
    remove.title = 'Mover a la papelera';
    li.querySelector('.row-main').addEventListener('click', (event) => {
      event.stopPropagation();
      openPop(skill.id);
    });
    use.addEventListener('click', () => onUse(skill));
    remove.addEventListener('click', () => removeSkill(skill));
    return li;
  }

  function pendingRow(item) {
    const li = document.createElement('li');
    li.className = 'notes-row skill-row skill-pending';
    li.innerHTML = `
      <div class="row-main">
        <span class="row-icon"><span class="skill-pending-dot" aria-hidden="true"></span></span>
        <span class="row-text">
          <span class="row-title">Creando skill…</span>
          <span class="row-meta"><span class="row-meta-text"></span></span>
        </span>
      </div>`;
    li.querySelector('.row-meta-text').textContent = item.text;
    li.title = 'El agente creador-de-skills está trabajando en la terminal de Claude Code';
    return li;
  }

  async function removeSkill(skill) {
    const ok = await ask(`¿Mover la skill «${skill.name}» a la papelera? Claude Code dejará de tenerla.`, {
      title: 'Borrar skill',
      kind: 'warning',
      okLabel: 'Mover a la papelera',
      cancelLabel: 'Cancelar',
    });
    if (!ok) return;
    try {
      await invoke('skills_remove', { id: skill.id });
    } catch (error) {
      fail(error);
    }
    load();
  }

  // Formulario «Agregar skill»: la descripción va al agente por la terminal de Claude Code.
  function openAdd() {
    form.hidden = false;
    $('skill-add').hidden = true;
    requestAnimationFrame(() => input.focus());
  }
  function closeAdd() {
    form.hidden = true;
    $('skill-add').hidden = false;
    input.value = '';
  }
  $('skill-add').addEventListener('click', openAdd);
  $('skill-create-cancel').addEventListener('click', closeAdd);
  input.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') closeAdd();
    // Intro envía; Mayús+Intro hace un salto de línea.
    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      form.requestSubmit();
    }
  });
  form.addEventListener('submit', async (event) => {
    event.preventDefault();
    const text = input.value.replace(/\s+/g, ' ').trim();
    if (!text) return input.focus();
    const ok = $('skill-create-ok');
    ok.disabled = true;
    try {
      await onCreate(text);
      pending.push({ key: String(Date.now()), text, since: Date.now() });
      closeAdd();
      render();
    } catch (error) {
      fail(error);
    } finally {
      ok.disabled = false;
    }
  });

  return {
    load,
    announce,
    relayout,
    openAdd,
    closePop,
    get count() {
      return skills.length;
    },
  };
}
