// Lector de archivos del workspace: el texto con sus números de línea y coloreado como en un
// editor (highlight.js, solo los lenguajes de ws-highlight.js para no cargar los ~190). Es de
// solo lectura. highlight.js escapa el texto y solo añade <span class="hljs-…">, así que su HTML
// es seguro; el Markdown, en vista previa, pasa por DOMPurify (renderMarkdown).
// highlight.js va aparte (ws-highlight.js) y se carga al abrir el primer archivo.
let highlighter = null;
const loadHighlighter = () => (highlighter ??= import('./ws-highlight.js'));

// Extensión (o nombre de archivo) → [lenguaje de highlight.js, nombre para el usuario].
const BY_EXT = {
  js: ['javascript', 'JavaScript'], mjs: ['javascript', 'JavaScript'], cjs: ['javascript', 'JavaScript'], jsx: ['javascript', 'JSX'],
  ts: ['typescript', 'TypeScript'], tsx: ['typescript', 'TSX'], mts: ['typescript', 'TypeScript'],
  json: ['json', 'JSON'], jsonc: ['json', 'JSON'], jsonl: ['json', 'JSON Lines'], webmanifest: ['json', 'JSON'],
  rs: ['rust', 'Rust'], py: ['python', 'Python'], pyi: ['python', 'Python'], rb: ['ruby', 'Ruby'], go: ['go', 'Go'],
  java: ['java', 'Java'], kt: ['kotlin', 'Kotlin'], kts: ['kotlin', 'Kotlin'], swift: ['swift', 'Swift'],
  c: ['c', 'C'], h: ['c', 'C'], cc: ['cpp', 'C++'], cpp: ['cpp', 'C++'], hpp: ['cpp', 'C++'], cxx: ['cpp', 'C++'], ino: ['cpp', 'Arduino'],
  cs: ['csharp', 'C#'], php: ['php', 'PHP'], lua: ['lua', 'Lua'], sql: ['sql', 'SQL'],
  sh: ['bash', 'Shell'], bash: ['bash', 'Shell'], zsh: ['bash', 'Shell'], fish: ['bash', 'Shell'],
  css: ['css', 'CSS'], scss: ['scss', 'SCSS'], sass: ['scss', 'Sass'], less: ['scss', 'Less'],
  html: ['xml', 'HTML'], htm: ['xml', 'HTML'], xml: ['xml', 'XML'], svg: ['xml', 'SVG'], vue: ['xml', 'Vue'], svelte: ['xml', 'Svelte'],
  md: ['markdown', 'Markdown'], markdown: ['markdown', 'Markdown'], mdx: ['markdown', 'MDX'],
  yml: ['yaml', 'YAML'], yaml: ['yaml', 'YAML'], toml: ['ini', 'TOML'], ini: ['ini', 'INI'], cfg: ['ini', 'Config'], conf: ['ini', 'Config'], env: ['bash', 'Entorno'],
  diff: ['diff', 'Diff'], patch: ['diff', 'Diff'], txt: [null, 'Texto'], log: [null, 'Registro'], csv: [null, 'CSV'],
};
const BY_NAME = { dockerfile: ['dockerfile', 'Dockerfile'], makefile: ['makefile', 'Makefile'], '.gitignore': ['bash', 'Git'], '.env': ['bash', 'Entorno'], 'cargo.lock': ['ini', 'TOML'] };

export function languageOf(path) {
  const name = String(path).split('/').pop().toLowerCase();
  if (BY_NAME[name]) return BY_NAME[name];
  if (name.startsWith('.env')) return BY_NAME['.env'];
  const ext = name.includes('.') ? name.split('.').pop() : '';
  return BY_EXT[ext] ?? [null, ext ? ext.toUpperCase() : 'Texto'];
}

export const isMarkdown = (path) => languageOf(path)[0] === 'markdown';

// Colorear tiene un coste: lo muy grande se enseña tal cual.
const HIGHLIGHT_MAX = 400_000;

export function lineCount(text) {
  if (!text) return 0;
  let n = 1;
  for (let i = text.indexOf('\n'); i !== -1; i = text.indexOf('\n', i + 1)) n++;
  return text.endsWith('\n') ? n - 1 : n;
}

// «1\n2\n3…» para la columna de los números: un solo nodo de texto, alineado con el código
// porque los dos comparten fuente y alto de línea.
export function gutterText(lines) {
  const out = new Array(Math.max(1, lines));
  for (let i = 0; i < out.length; i++) out[i] = i + 1;
  return out.join('\n');
}

const escapeHtml = (text) => text.replace(/[&<>"']/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[ch]);

// El HTML coloreado de un archivo (o su texto escapado, si es muy grande o no hay lenguaje).
export async function highlight(text, path) {
  const [language] = languageOf(path);
  if (!language || text.length > HIGHLIGHT_MAX) return escapeHtml(text);
  try {
    const { highlightCode, hasLanguage } = await loadHighlighter();
    return hasLanguage(language) ? highlightCode(text, language) : escapeHtml(text);
  } catch {
    return escapeHtml(text);
  }
}

const formatSize = (bytes) => (bytes < 1024 ? `${bytes} B` : bytes < 1024 ** 2 ? `${Math.round(bytes / 1024)} KB` : `${(bytes / 1024 ** 2).toFixed(1)} MB`);

// Pinta un archivo en `box`. `view` es lo que devuelve workspace_read; `preview` enseña el
// Markdown ya renderizado.
export function renderFile(box, view, { preview = false, renderMarkdown = null } = {}) {
  box.replaceChildren();
  if (view.binary) {
    const note = document.createElement('div');
    note.className = 'ws-code-note';
    const title = document.createElement('p');
    title.className = 'ws-code-note-title';
    title.textContent = 'No es un archivo de texto';
    const text = document.createElement('p');
    text.textContent = `${formatSize(view.size)}. El lector solo enseña texto y código.`;
    note.append(title, text);
    box.append(note);
    return;
  }
  const text = view.text ?? '';
  if (preview && renderMarkdown) {
    const article = document.createElement('article');
    article.className = 'note-body ws-preview';
    article.innerHTML = renderMarkdown(text);
    box.append(article);
    return;
  }
  const scroll = document.createElement('div');
  scroll.className = 'ws-code';
  scroll.tabIndex = 0;
  scroll.setAttribute('role', 'region');
  scroll.setAttribute('aria-label', `Contenido de ${view.path}`);
  const gutter = document.createElement('pre');
  gutter.className = 'ws-code-gutter';
  gutter.setAttribute('aria-hidden', 'true');
  gutter.textContent = gutterText(lineCount(text));
  const pre = document.createElement('pre');
  pre.className = 'ws-code-text';
  const code = document.createElement('code');
  code.className = 'hljs';
  // Primero el texto tal cual y, en cuanto está, coloreado (sin mover el desplazamiento).
  code.textContent = text;
  highlight(text, view.path).then((html) => {
    if (box.contains(code)) code.innerHTML = html;
  });
  pre.append(code);
  scroll.append(gutter, pre);
  box.append(scroll);
  if (view.truncated) {
    const cut = document.createElement('p');
    cut.className = 'ws-code-cut';
    cut.textContent = `Se enseña el primer mega de ${formatSize(view.size)}.`;
    box.append(cut);
  }
}

export function fileMeta(view) {
  const [, name] = languageOf(view.path);
  if (view.binary) return `${formatSize(view.size)} · binario`;
  const lines = lineCount(view.text ?? '');
  return `${name} · ${lines.toLocaleString('es')} ${lines === 1 ? 'línea' : 'líneas'} · solo lectura`;
}
