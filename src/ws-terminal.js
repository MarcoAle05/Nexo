// Una terminal de un workspace en su ventana: xterm.js conectado al PTY del backend
// (terminales.rs). Al conectarse recibe lo que la terminal ya había escrito (también tras una
// recarga de Vite o al volver al workspace) y después lo que llega. Los colores salen de los
// tokens del tema de su ventana (ws-term-theme.js) y cambian con el tema.
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { WebLinksAddon } from '@xterm/addon-web-links';
import '@xterm/xterm/css/xterm.css';
import { THEME_TOKENS, terminalTheme } from './ws-term-theme.js';

// Lo mismo que el historial de la pantalla del backend (terminales.rs, SCROLLBACK).
const SCROLLBACK = 10000;

function readTheme(win) {
  const style = getComputedStyle(win);
  const tokens = Object.fromEntries(THEME_TOKENS.map((name) => [name, style.getPropertyValue(name)]));
  const page = getComputedStyle(document.documentElement).getPropertyValue('--bg');
  return terminalTheme((name) => tokens[name], page);
}

// `host` es el cuerpo de la ventana y `win`, la ventana (de ella salen los colores). `onExit`
// recibe el código con el que terminó su programa; `onLink`, los enlaces que se pulsan.
export function createTerminalView({ host, win, id, label, invoke, Channel, onExit = () => {}, onLink = () => {} }) {
  const box = document.createElement('div');
  box.className = 'ws-term';
  host.replaceChildren(box);
  const term = new Terminal({
    fontFamily: "'Geist Mono', ui-monospace, Menlo, monospace",
    fontSize: 13,
    lineHeight: 1.2,
    cursorBlink: true,
    scrollback: SCROLLBACK,
    // Lo que un programa pinta con sus propios colores (Claude Code los trae) no baja de 3:1.
    minimumContrastRatio: 3,
    theme: readTheme(win),
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.loadAddon(new WebLinksAddon((event, uri) => /^https?:\/\//i.test(uri) && onLink(uri)));
  box.style.backgroundColor = term.options.theme.background;
  term.open(box);
  // Con WebGL si WebKit lo da (modo «hibrido» de render.rs); si no, o si se pierde el contexto,
  // xterm pinta con el DOM.
  try {
    const gl = new WebglAddon();
    gl.onContextLoss(() => gl.dispose());
    term.loadAddon(gl);
  } catch {}
  term.textarea?.setAttribute('aria-label', label);

  // Copiar y pegar como en las terminales de Linux; F6 es de la app (pasa el foco a otra zona).
  term.attachCustomKeyEventHandler((event) => {
    if (event.type !== 'keydown') return true;
    if (event.key === 'F6') return false;
    if (event.ctrlKey && event.shiftKey && event.code === 'KeyC') {
      const text = term.getSelection();
      if (text) navigator.clipboard?.writeText(text).catch(() => {});
      return false;
    }
    if (event.ctrlKey && event.shiftKey && event.code === 'KeyV') {
      navigator.clipboard
        ?.readText()
        .then((text) => text && term.paste(text))
        .catch(() => {});
      return false;
    }
    return true;
  });

  term.onData((data) => invoke('terminal_input', { id, data }).catch(() => {}));
  // El tamaño se le dice al PTY al final de un cambio, no en cada paso.
  let sizeTimer = 0;
  term.onResize(({ cols, rows }) => {
    clearTimeout(sizeTimer);
    sizeTimer = setTimeout(() => invoke('terminal_resize', { id, cols, rows }).catch(() => {}), 80);
  });

  let disposed = false;
  let exited = false;
  const channel = new Channel();
  channel.onmessage = (event) => {
    if (disposed) return;
    if (event.kind === 'data') term.write(event.data);
    else if (event.kind === 'exit' && !exited) {
      exited = true;
      const code = event.code == null ? '' : ` (código ${event.code})`;
      term.write(`\r\n\x1b[2m— El proceso terminó${code} —\x1b[0m\r\n`);
      onExit(event.code);
    }
  };
  invoke('terminal_attach', { id, channel }).catch((error) => term.write(`\r\n\x1b[2m${String(error)}\x1b[0m\r\n`));

  // Se ajusta al tamaño de su ventana: un ResizeObserver con un fotograma de margen (durante las
  // transiciones de las cartas el centro ya tiene su medida final, ver card-resize.js).
  let fitFrame = 0;
  const refit = () => {
    if (disposed || fitFrame) return;
    fitFrame = requestAnimationFrame(() => {
      fitFrame = 0;
      if (disposed || !box.isConnected || !box.clientWidth || !box.clientHeight) return;
      try {
        fit.fit();
      } catch {}
    });
  };
  const observer = new ResizeObserver(refit);
  observer.observe(box);
  refit();

  return {
    term,
    get exited() {
      return exited;
    },
    focus: () => term.focus(),
    refit,
    // Cambió el tema: los colores se vuelven a leer de la ventana.
    applyTheme() {
      const theme = readTheme(win);
      term.options.theme = theme;
      box.style.backgroundColor = theme.background;
    },
    dispose() {
      disposed = true;
      observer.disconnect();
      cancelAnimationFrame(fitFrame);
      clearTimeout(sizeTimer);
      term.dispose();
    },
  };
}
