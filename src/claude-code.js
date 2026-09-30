// Pestaña "Claude Code": una terminal real (xterm.js) conectada a un pseudo-terminal
// en Rust que ejecuta `claude` dentro del vault.
import '@xterm/xterm/css/xterm.css';
import { Channel, invoke } from '@tauri-apps/api/core';
import { FitAddon } from '@xterm/addon-fit';
import { Terminal } from '@xterm/xterm';

export function createClaudeCode({ container, exitBox, exitText, onError }) {
  let term = null;
  let fit = null;
  let running = false;

  function ensureTerminal() {
    if (term) return;
    term = new Terminal({
      allowTransparency: true,
      cursorBlink: true,
      fontFamily: "'Geist Mono', ui-monospace, Menlo, monospace",
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
    term.onData((data) => running && invoke('claude_code_write', { data }).catch(onError));
    term.onResize(({ cols, rows }) => running && invoke('claude_code_resize', { cols, rows }).catch(() => {}));
    new ResizeObserver(() => {
      if (container.offsetParent) fit.fit();
    }).observe(container);
  }

  async function start() {
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
    show() {
      if (running) {
        fit.fit();
        term.focus();
      } else if (!term || exitBox.hidden) start();
    },
    restart: start,
    stop() {
      running = false;
      return invoke('claude_code_stop').catch(() => {});
    },
  };
}
