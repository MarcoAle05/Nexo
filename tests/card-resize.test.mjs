// Cartas que cambian de ancho: a qué borde se ancla lo de dentro mientras se mueven.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { anchorSide } from '../src/card-resize.js';

const box = (left, right) => ({ left, right });

test('lo de dentro se ancla al borde que no se mueve', () => {
  // Minimizar o restaurar el chat: el escenario solo cambia por la derecha.
  assert.equal(anchorSide(box(470, 1460), box(470, 1820)), 'left');
  assert.equal(anchorSide(box(470, 1820), box(470, 1460)), 'left');
  // Minimizar o restaurar la carta izquierda: solo por la izquierda.
  assert.equal(anchorSide(box(470, 1460), box(110, 1460)), 'right');
  assert.equal(anchorSide(box(110, 1460), box(470, 1460)), 'right');
});

test('si se mueven los dos bordes, gana el que menos se mueve', () => {
  // Expandir la izquierda: el escenario se pliega hacia la derecha y su margen negativo lo
  // corre 10 px por ese lado.
  assert.equal(anchorSide(box(470, 1460), box(1450, 1450)), 'right');
  // Contraer el chat expandido: vuelve desde la izquierda, también con esos 10 px.
  assert.equal(anchorSide(box(480, 480), box(470, 1460)), 'left');
});
