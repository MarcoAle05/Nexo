import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effortLabel, modelLabel, toolsLabel } from '../src/agents.js';

test('modelLabel da nombres legibles a los ids completos', () => {
  assert.equal(modelLabel('claude-haiku-5-5'), 'Haiku 5.5');
  assert.equal(modelLabel('claude-sonnet-5-5'), 'Sonnet 5.5');
  assert.equal(modelLabel('claude-opus-5-5'), 'Opus 5.5');
  assert.equal(modelLabel('claude-fable-5-1'), 'Fable 5.1');
  assert.equal(modelLabel('claude-sonnet-4-5-20250929'), 'Sonnet 4.5');
});

test('modelLabel entiende alias, inherit y la ausencia de modelo', () => {
  assert.equal(modelLabel('sonnet'), 'Sonnet (el último)');
  assert.equal(modelLabel('opus[1m]'), 'Opus (el último)');
  assert.equal(modelLabel('inherit'), 'Modelo de la sesión');
  assert.equal(modelLabel(null), 'Modelo de la sesión');
  assert.equal(modelLabel(''), 'Modelo de la sesión');
  assert.equal(modelLabel('otro-modelo'), 'otro-modelo');
});

test('effortLabel traduce los niveles y respeta los desconocidos', () => {
  assert.equal(effortLabel('low'), 'Esfuerzo bajo');
  assert.equal(effortLabel('medium'), 'Esfuerzo medio');
  assert.equal(effortLabel('high'), 'Esfuerzo alto');
  assert.equal(effortLabel('xhigh'), 'Esfuerzo muy alto');
  assert.equal(effortLabel('max'), 'Esfuerzo máximo');
  assert.equal(effortLabel('32'), 'Esfuerzo 32');
  assert.equal(effortLabel(undefined), 'Esfuerzo de la sesión');
});

test('toolsLabel agrupa las herramientas MCP por servidor', () => {
  assert.equal(
    toolsLabel('Read, Write, Bash, mcp__playwright__browser_navigate, mcp__playwright__browser_click'),
    'Read, Write, Bash · playwright: navigate, click',
  );
  assert.equal(toolsLabel('mcp__claude_ai_Google_Drive__search'), 'claude_ai_Google_Drive: search');
  assert.equal(toolsLabel('Read Grep'), 'Read, Grep');
  assert.equal(toolsLabel(null), 'Todas las herramientas de la sesión');
});
