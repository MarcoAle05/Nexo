// Vista de una nota de la wiki dentro de nexo: Markdown → HTML saneado, con los
// [[enlaces]] de Obsidian convertidos en enlaces que llevan a su nodo del grafo.
// El contenido puede venir de páginas web (fichas, notas de Claude), así que todo
// pasa por DOMPurify antes de entrar en la página.
import DOMPurify from 'dompurify';
import { Marked } from 'marked';

const escapeHtml = (text) =>
  text.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

const wikilink = {
  name: 'wikilink',
  level: 'inline',
  start: (src) => src.indexOf('[['),
  tokenizer(src) {
    const match = /^\[\[([^\]|]+?)(?:\|([^\]]+))?\]\]/.exec(src);
    if (!match) return undefined;
    const target = match[1].split(/[#^]/)[0].trim();
    return { type: 'wikilink', raw: match[0], target, text: (match[2] ?? match[1]).trim() };
  },
  renderer: (token) =>
    `<a href="#" class="wikilink" data-target="${escapeHtml(token.target)}">${escapeHtml(token.text)}</a>`,
};

const marked = new Marked({ gfm: true, breaks: false });
marked.use({ extensions: [wikilink] });

export function renderMarkdown(text) {
  const html = marked.parse(text ?? '');
  return DOMPurify.sanitize(html, { FORBID_TAGS: ['style', 'form'], FORBID_ATTR: ['style'] });
}

const strip = (s) => s.replace(/\.md$/i, '').toLowerCase();

// Resuelve un enlace como Obsidian (y como graph.rs): por ruta exacta, relativa a la
// carpeta de la nota o por nombre; con nombres repetidos gana la misma carpeta.
// Si no existe, el nodo de palabra clave `?destino`.
export function resolveLink(nodes, fromId, target) {
  const key = strip(target.replace(/^\.\//, '').replace(/%20/g, ' '));
  const dir = fromId.includes('/') ? fromId.slice(0, fromId.lastIndexOf('/')).toLowerCase() : '';
  const notes = nodes.filter((n) => n.kind !== 'missing');
  const byPath = (path) => notes.find((n) => strip(n.id) === path);
  const exact = byPath(key) ?? (dir && byPath(`${dir}/${key}`));
  if (exact) return exact;
  const stem = key.split('/').pop();
  const same = notes.filter((n) => strip(n.id).split('/').pop() === stem);
  const found = same.find((n) => n.id.toLowerCase().startsWith(`${dir}/`)) ?? same[0];
  if (found) return found;
  return nodes.find((n) => n.kind === 'missing' && n.id.slice(1).toLowerCase() === target.trim().toLowerCase()) ?? null;
}
