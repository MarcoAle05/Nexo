// Temas: la lista de themes.js casa con los bloques de styles.css, y cada tema pasa los
// contrastes de WCAG 2 con los tokens que declara (texto 4,5:1; marcas y foco 3:1).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { THEMES, DEFAULT_THEME, themeById } from '../src/themes.js';
import { TINTS, makePoints, recolor } from '../src/galaxy.js';

const css = readFileSync(new URL('../src/styles.css', import.meta.url), 'utf8');

// Bloques de primer nivel de la hoja: selector y declaraciones `--token: valor;`, en orden.
const BLOCKS = [...`\n${css}`.matchAll(/\n([^\n{}]+?) \{\n([\s\S]*?)\n\}/g)].map(([, selector, body]) => {
  const tokens = {};
  for (const [, name, value] of body.replace(/\/\*[\s\S]*?\*\//g, '').matchAll(/(--[\w-]+):\s*([^;]+);/g)) tokens[name] = value.trim();
  return { selector: selector.trim(), tokens };
});
const block = (selector) => BLOCKS.find((b) => b.selector === selector)?.tokens ?? null;
const LIGHT = THEMES.filter((t) => t.scheme === 'light').map((t) => t.id);
const lightSelector = `:is(${LIGHT.map((id) => `[data-theme="${id}"]`).join(', ')})`;
const primitives = block(':root');
const base = block(':root, [data-theme]');
const light = block(lightSelector);
// Las fórmulas (valen en la raíz y en cada tarjeta) y el bloque que da a las tarjetas sus canales.
const FORMULAS = BLOCKS.find((b) => b.selector.startsWith(':root, [data-theme], ') && '--dim' in b.tokens);
const CARDS = BLOCKS.find((b) => b.tokens['--fg-rgb'] === 'var(--s-fg-rgb)');

// Cascada de un elemento: los bloques que le valen, en el orden de la hoja (manda el último),
// y cada var() resuelto con sus propias declaraciones o, si no las tiene, con lo heredado.
const appliesToRoot = (b, id) => b === FORMULAS || b.selector === ':root' || b.selector === ':root, [data-theme]' || b.selector.includes(`[data-theme="${id}"]`);
function compute(raw, inherited) {
  const out = {};
  const get = (name, depth) => {
    assert.ok(depth < 12, `var() sin fin en ${name}`);
    if (name in out) return out[name];
    if (!(name in raw)) {
      assert.ok(name in inherited, `falta ${name}`);
      return inherited[name];
    }
    const v = raw[name].replace(/var\((--[\w-]+)(?:,\s*([^)]+))?\)/g, (_, n, fallback) => (n in raw || n in inherited ? get(n, depth + 1) : fallback));
    return (out[name] = v);
  };
  for (const name of Object.keys(raw)) get(name, 0);
  return { ...inherited, ...out };
}
function tokensOf(theme) {
  const raw = {};
  for (const b of BLOCKS) if (appliesToRoot(b, theme.id)) Object.assign(raw, b.tokens);
  return compute(raw, {});
}
const cardTokensOf = (theme) => compute({ ...FORMULAS.tokens, ...CARDS.tokens }, tokensOf(theme));
// La carta del chat (`#terminal-panel`): una tarjeta con el tono más hondo de las tarjetas.
const CHAT = BLOCKS.find((b) => b.selector === '#terminal-panel');
const chatTokensOf = (theme) => compute({ ...FORMULAS.tokens, ...CARDS.tokens, ...CHAT.tokens }, tokensOf(theme));

// Un color ya resuelto (hex o rgb/rgba) a [r, g, b, a].
function color(value) {
  const v = value.trim();
  let m = v.match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (m) {
    const h = m[1].length === 3 ? [...m[1]].map((c) => c + c).join('') : m[1];
    return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)).concat(1);
  }
  m = v.match(/^rgba?\(([^)]+)\)$/);
  assert.ok(m, `color que no se entiende: ${value}`);
  const parts = m[1].split(',').map((x) => Number(x.trim()));
  return parts.length === 3 ? [...parts, 1] : parts;
}
const over = ([r, g, b, a], [R, G, B]) => [r * a + R * (1 - a), g * a + G * (1 - a), b * a + B * (1 - a), 1];
const lum = (rgb) => {
  const lin = (c) => ((c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2]);
};
const ratio = (a, b) => {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};

test('cada tema de la lista tiene su bloque en styles.css, y al revés', () => {
  assert.ok(primitives && base && light, 'faltan los bloques comunes de temas');
  assert.equal(THEMES.filter((t) => t.id === DEFAULT_THEME).length, 1);
  assert.equal(new Set(THEMES.map((t) => t.id)).size, THEMES.length, 'ids repetidos');
  for (const t of THEMES) {
    assert.match(t.id, /^[a-z-]+$/, 'public/tema.js solo acepta letras y guiones');
    if (t.id !== DEFAULT_THEME) assert.ok(block(`[data-theme="${t.id}"]`), `sin bloque para ${t.id}`);
  }
  const declared = [...css.matchAll(/\n\[data-theme="([\w-]+)"\] \{/g)].map((m) => m[1]);
  assert.deepEqual(declared.sort(), THEMES.filter((t) => t.id !== DEFAULT_THEME).map((t) => t.id).sort());
  assert.equal(themeById('no-existe').id, DEFAULT_THEME);
});

test('los temas pasan los contrastes de WCAG 2 en el fondo y en las tarjetas', () => {
  assert.ok(FORMULAS && CARDS && CHAT, 'faltan las fórmulas, el bloque de las tarjetas o el de la carta del chat');
  for (const theme of THEMES) {
    const g = tokensOf(theme);
    const t = cardTokensOf(theme);
    const bg = color(g['--bg']);
    const text = (tokens, name, on, label) => {
      const r = ratio(over(color(tokens[name]), on), on);
      assert.ok(r >= 4.5, `${theme.id}: ${name} sobre ${label} da ${r.toFixed(2)}:1 (hace falta 4,5)`);
    };
    const mark = (fg, on, label) => {
      const r = ratio(over(fg, on), on);
      assert.ok(r >= 3, `${theme.id}: ${label} da ${r.toFixed(2)}:1 (hace falta 3)`);
    };
    // Sobre el fondo: la barra superior, el registro, el lienzo de notas.
    for (const name of ['--fg', '--dim', '--faint']) text(g, name, bg, 'el fondo');
    mark(color(g['--accent-soft']), bg, '--accent-soft (logo, puntos de estado) sobre el fondo');
    // En las tarjetas: cartas y menús (sobre el fondo) y lo que flota.
    const card = over(color(t['--glass']), bg);
    const menu = over(color(t['--menu']), bg);
    const raised = color(t['--raised']);
    const c = chatTokensOf(theme);
    const chat = over(color(c['--glass']), bg);
    for (const name of ['--fg', '--dim', '--faint']) {
      text(t, name, card, 'la tarjeta');
      text(t, name, menu, 'el menú');
      text(t, name, raised, 'una nota');
      text(c, name, chat, 'la carta del chat');
    }
    mark(color(c['--accent-line']), chat, '--accent-line (foco) sobre la carta del chat');
    const onAccent = ratio(color(t['--on-accent']), color(t['--accent']));
    assert.ok(onAccent >= 4.5, `${theme.id}: --on-accent sobre --accent da ${onAccent.toFixed(2)}:1`);
    mark(color(t['--accent-soft']), card, '--accent-soft (marcas) sobre la tarjeta');
    mark(color(t['--accent-line']), card, '--accent-line (foco) sobre la tarjeta');
    const stroke = t['--check-mark'].match(/stroke='%23([0-9a-f]{6})'/i)[1];
    mark(color(`#${stroke}`), color(t['--accent-soft']), 'la palomita sobre --accent-soft');
  }
});

test('las paletas de los lienzos tienen lo que esperan galaxy.js y graph.js', () => {
  const points = makePoints();
  for (const t of THEMES) {
    assert.equal(t.galaxy.tints.length, TINTS.length, `${t.id}: un tinte por cada uno de TINTS`);
    assert.ok(['add', 'over'].includes(t.galaxy.blend));
    assert.equal(t.galaxy.blend, t.scheme === 'light' ? 'over' : 'add', `${t.id}: sobre un fondo claro la luz sumada no se ve`);
    assert.ok(t.galaxy.gain > 0 && t.galaxy.gain <= 1);
    assert.equal(recolor(points, t.galaxy.tints).length, points.count * 3);
    assert.equal(t.graph.nebula.length, 8);
    for (const k of ['star', 'nova', 'core', 'ghost']) assert.match(t.graph[k], /^#[0-9a-f]{6}$/i);
    assert.equal(t.graph.ink.length, 3);
  }
  // Órbita se recolorea igual que se generó.
  assert.deepEqual([...recolor(points, themeById('orbita').galaxy.tints)], [...points.color]);
});
