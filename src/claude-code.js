// Pestaña "Claude Code": una terminal real (xterm.js) conectada a un pseudo-terminal
// en Rust que ejecuta `claude` dentro del vault.
import '@xterm/xterm/css/xterm.css';
import { Channel, invoke } from '@tauri-apps/api/core';
import { FitAddon } from '@xterm/addon-fit';
import { Terminal } from '@xterm/xterm';

const FONT_FAMILY = "'Geist Mono', ui-monospace, Menlo, monospace";

export function createClaudeCode({ container, exitBox, exitText, onError }) {
  let term = null;
  let fit = null;
  let running = false;

  // Las pulsaciones se envían en orden y agrupadas: nunca hay dos escrituras a la vez.
  let pending = '';
  let flushing = false;
  async function send(data) {
    pending += data;
    if (flushing) return;
    flushing = true;
    while (pending) {
      const chunk = pending;
      pending = '';
      await invoke('claude_code_write', { data: chunk }).catch(onError);
    }
    flushing = false;
  }

  function ensureTerminal() {
    if (term) return;
    term = new Terminal({
      allowTransparency: true,
      cursorBlink: true,
      fontFamily: FONT_FAMILY,
      fontSize: 12.5,
      lineHeight: 1.25,
      scrollback: 5000,
      theme: {
        background: 'rgba(0,0,0,0)',
        foreground: '#f5f5f5',
        cursor: '#f5f5f5',
        cursorAccent: '#050505',
        selectionBackground: 'rgba(255,255,255,0.25)',
      },
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.open(container);
    // Si la fuente llega después de abrir, se reasigna para que xterm vuelva a medir.
    document.fonts.addEventListener('loadingdone', () => {
      term.options.fontFamily = FONT_FAMILY;
      if (container.offsetParent) fit.fit();
    });
    // Un clic en cualquier parte del panel devuelve el foco a la terminal.
    container.parentElement.addEventListener('mousedown', () => requestAnimationFrame(() => term.focus()));
    term.onData((data) => running && send(data));
    term.onResize(({ cols, rows }) => running && invoke('claude_code_resize', { cols, rows }).catch(() => {}));
    new ResizeObserver(() => {
      if (container.offsetParent) fit.fit();
    }).observe(container);
  }

  // xterm mide las celdas al abrirse: primero se asegura Geist Mono (normal y negrita).
  const fontsReady = Promise.all([
    document.fonts.load("400 12.5px 'Geist Mono'"),
    document.fonts.load("700 12.5px 'Geist Mono'"),
  ]).catch(() => {});

  async function start() {
    await fontsReady;
    ensureTerminal();
    fit.fit();
    exitBox.hidden = true;
    term.reset();
    const channel = new Channel();
    channel.onmessage = (event) => {
      if (event.kind === 'data') term.write(event.text);
      else {
        running = false;
        exitText.textContent = `Claude Code terminó${event.code ? ` (código ${event.code})` : ''}.`;
        exitBox.hidden = false;
      }
    };
    try {
      const session = await invoke('claude_code_start', { cols: term.cols, rows: term.rows, channel });
      term.writeln(`\x1b[2m${session}\x1b[0m`);
      running = true;
      term.focus();
    } catch (error) {
      running = false;
      exitText.textContent = String(error);
      exitBox.hidden = false;
      onError(error);
    }
  }

  return {
    // Abre la pestaña: arranca Claude Code la primera vez, después solo recupera el foco.
    // Al volver a mostrarse (desde otra pestaña o desde Actividad), xterm puede haber
    // pausado el dibujo mientras estaba oculto: se reajusta, se repinta y recupera el foco.
    show() {
      if (running) {
        requestAnimationFrame(() => {
          fit.fit();
          term.refresh(0, term.rows - 1);
          term.focus();
        });
      } else if (!term || exitBox.hidden) start();
    },
    restart: start,
    get running() {
      return running;
    },
    // Escribe un mensaje en Claude Code y lo envía (como si el usuario lo tecleara).
    say(text) {
      if (!running) return false;
      send(text);
      setTimeout(() => send('\r'), 150);
      term.focus();
      return true;
    },
    stop() {
      running = false;
      return invoke('claude_code_stop').catch(() => {});
    },
  };
}
