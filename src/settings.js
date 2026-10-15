// Configuración: el botón del engranaje (arriba a la derecha) abre una ventana pequeña en el
// centro. Es un <dialog> modal (atrapa el foco, Escape la cierra y el resto de la app queda
// inerte). Entra y sale con una transición de clases (`is-open`, `is-closing`), no con
// animaciones: así se puede cerrar a mitad de la entrada. La primera opción es el tema.
import { THEMES, applyTheme, currentTheme } from './themes.js';

const GROUPS = [
  { scheme: 'dark', title: 'Oscuros' },
  { scheme: 'light', title: 'Claros' },
];

// Una tarjeta por tema: una miniatura de la app pintada con los tokens de ese tema (su
// `data-theme` le da los colores; ver styles.css: la barra, la carta de la izquierda, la del chat
// con su tono más hondo y la galaxia) y el nombre. La radio de verdad va dentro,
// oculta: las flechas recorren todos los temas y el lector de pantalla los anuncia como grupo.
function themeOption(theme) {
  const label = document.createElement('label');
  label.className = 'theme-option';
  const radio = document.createElement('input');
  radio.type = 'radio';
  radio.name = 'theme';
  radio.value = theme.id;
  radio.className = 'theme-radio';
  radio.setAttribute('aria-labelledby', `theme-name-${theme.id}`);
  radio.setAttribute('aria-describedby', `theme-hint-${theme.id}`);
  const preview = document.createElement('span');
  preview.className = 'theme-preview';
  preview.dataset.theme = theme.id;
  preview.setAttribute('aria-hidden', 'true');
  preview.innerHTML = '<span class="tp-bar"></span><span class="tp-card"><span class="tp-line"></span><span class="tp-line tp-dim"></span><span class="tp-pill"></span></span><span class="tp-side"></span><span class="tp-orbit"></span><span class="tp-core"></span>';
  const check = document.createElement('span');
  check.className = 'theme-check';
  check.setAttribute('aria-hidden', 'true');
  const name = document.createElement('span');
  name.className = 'theme-name';
  name.id = `theme-name-${theme.id}`;
  name.textContent = theme.name;
  const hint = document.createElement('span');
  hint.className = 'theme-hint';
  hint.id = `theme-hint-${theme.id}`;
  hint.textContent = theme.hint;
  label.append(radio, preview, check, name, hint);
  return label;
}

export function createSettings({ button, dialog, groups, closeButton }) {
  for (const { scheme, title } of GROUPS) {
    const group = document.createElement('div');
    group.className = 'theme-group';
    const heading = document.createElement('p');
    heading.className = 'theme-group-title';
    heading.textContent = title;
    const grid = document.createElement('div');
    grid.className = 'theme-grid';
    grid.append(...THEMES.filter((t) => t.scheme === scheme).map(themeOption));
    group.append(heading, grid);
    groups.append(group);
  }
  const radios = [...groups.querySelectorAll('.theme-radio')];
  const sync = () => {
    const id = currentTheme().id;
    for (const r of radios) r.checked = r.value === id;
  };
  sync();
  groups.addEventListener('change', (e) => {
    if (e.target.matches('.theme-radio')) applyTheme(e.target.value);
  });

  let closing = 0;
  function open() {
    if (dialog.open) return;
    clearTimeout(closing);
    closing = 0;
    dialog.classList.remove('is-closing');
    sync();
    dialog.showModal();
    button.setAttribute('aria-expanded', 'true');
    // El estado de partida (transparente y algo más abajo) tiene que estar calculado antes de
    // poner `is-open`; si no, recién salida de display: none, no habría nada desde lo que pasar.
    void dialog.offsetHeight;
    dialog.classList.add('is-open');
    (radios.find((r) => r.checked) ?? radios[0])?.focus();
  }
  function finish() {
    clearTimeout(closing);
    closing = 0;
    if (dialog.open) dialog.close();
    dialog.classList.remove('is-closing');
  }
  function close() {
    if (!dialog.open || closing) return;
    dialog.classList.remove('is-open');
    dialog.classList.add('is-closing');
    button.setAttribute('aria-expanded', 'false');
    // La salida dura 150 ms (styles.css); si `transitionend` no llega (sin movimiento, o una
    // transición cortada), se cierra igual poco después.
    closing = setTimeout(finish, 220);
  }
  dialog.addEventListener('transitionend', (e) => {
    if (e.target === dialog && e.propertyName === 'opacity' && dialog.classList.contains('is-closing')) finish();
  });
  // Escape: se cierra con la misma salida que el botón.
  dialog.addEventListener('cancel', (e) => {
    e.preventDefault();
    close();
  });
  // Un clic fuera del recuadro cae en el propio <dialog> (su fondo): cierra.
  dialog.addEventListener('click', (e) => {
    if (e.target === dialog) close();
  });
  dialog.addEventListener('close', () => {
    button.setAttribute('aria-expanded', 'false');
    if (!document.activeElement || document.activeElement === document.body) button.focus();
  });
  closeButton.addEventListener('click', close);
  button.addEventListener('click', () => (dialog.open ? close() : open()));
  return { open, close };
}
