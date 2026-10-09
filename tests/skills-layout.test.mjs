// Pruebas de la órbita de los recuadros de skills y de los nombres de comando (funciones puras).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ORBIT, orbitLayout, commandFor } from '../src/skills.js';
import { frameFor } from '../src/galaxy.js';

// Escenarios: tamaños de la ventana del escenario (px) y su marco de la galaxia.
const ESCENARIOS = [
  { nombre: '1000×800', width: 1000, height: 800 },
  { nombre: '1280×720', width: 1280, height: 720 },
  { nombre: '700×900', width: 700, height: 900 },
];

// Recuadros de tamaño típico (ancho 70–200, alto 34), siempre los mismos para cada n.
const tamanos = (n) => Array.from({ length: n }, (_, i) => [70 + ((i * 37) % 131), 34]);

const marco = (e) => frameFor({ left: 0, top: 0, width: e.width, height: e.height });
const bounds = (e) => ({ width: e.width, height: e.height });

// Separación entre dos recuadros: el hueco del eje en el que más se separan (negativo = se pisan).
function separacion([x1, y1], [w1, h1], [x2, y2], [w2, h2]) {
  const dx = Math.abs(x1 - x2) - (w1 + w2) / 2;
  const dy = Math.abs(y1 - y2) - (h1 + h2) / 2;
  return Math.max(dx, dy);
}

test('orbitLayout devuelve un centro por cada tamaño', () => {
  for (let n = 1; n <= 10; n++) {
    const sizes = tamanos(n);
    const centros = orbitLayout(sizes, marco(ESCENARIOS[0]), bounds(ESCENARIOS[0]));
    assert.equal(centros.length, n, `n = ${n}`);
    for (const [x, y] of centros) {
      assert.ok(Number.isFinite(x) && Number.isFinite(y), `centro no finito con n = ${n}`);
    }
  }
});

test('orbitLayout sin tamaños devuelve una lista vacía', () => {
  assert.deepEqual(orbitLayout([], marco(ESCENARIOS[0]), bounds(ESCENARIOS[0])), []);
});

test('orbitLayout deja los recuadros dentro del escenario, respetando el margen', () => {
  for (const e of ESCENARIOS) {
    for (let n = 1; n <= 10; n++) {
      const sizes = tamanos(n);
      const centros = orbitLayout(sizes, marco(e), bounds(e));
      centros.forEach(([x, y], i) => {
        const [w, h] = sizes[i];
        const ctx = `${e.nombre}, n = ${n}, recuadro ${i}`;
        assert.ok(x - w / 2 >= ORBIT.margin - 1e-6, `se sale por la izquierda (${ctx})`);
        assert.ok(x + w / 2 <= e.width - ORBIT.margin + 1e-6, `se sale por la derecha (${ctx})`);
        assert.ok(y - h / 2 >= ORBIT.margin - 1e-6, `se sale por arriba (${ctx})`);
        assert.ok(y + h / 2 <= e.height - ORBIT.margin + 1e-6, `se sale por abajo (${ctx})`);
      });
    }
  }
});

test('orbitLayout no solapa los recuadros cuando caben', () => {
  for (const e of ESCENARIOS) {
    for (let n = 1; n <= 10; n++) {
      const sizes = tamanos(n);
      const centros = orbitLayout(sizes, marco(e), bounds(e));
      for (let i = 0; i < n; i++) {
        for (let j = i + 1; j < n; j++) {
          const s = separacion(centros[i], sizes[i], centros[j], sizes[j]);
          assert.ok(s >= 0, `recuadros ${i} y ${j} se pisan (${s.toFixed(1)} px) en ${e.nombre}, n = ${n}`);
        }
      }
    }
  }
});

test('orbitLayout es determinista', () => {
  for (const e of ESCENARIOS) {
    const sizes = tamanos(7);
    const a = orbitLayout(sizes, marco(e), bounds(e));
    const b = orbitLayout(sizes, marco(e), bounds(e));
    assert.deepEqual(a, b);
  }
});

test('commandFor usa el nombre si es kebab válido', () => {
  assert.equal(commandFor({ id: 'mi-carpeta', name: 'classroom' }), 'classroom');
  assert.equal(commandFor({ id: 'mi-carpeta', name: 'a1-b2' }), 'a1-b2');
});

test('commandFor cae al id si el nombre tiene espacios, mayúsculas o no es válido', () => {
  assert.equal(commandFor({ id: 'mi-skill', name: 'Mi Skill Nueva' }), 'mi-skill');
  assert.equal(commandFor({ id: 'mi-skill', name: 'Classroom' }), 'mi-skill');
  assert.equal(commandFor({ id: 'mi-skill', name: '-guion' }), 'mi-skill');
  assert.equal(commandFor({ id: 'mi-skill', name: 'a'.repeat(65) }), 'mi-skill');
});
