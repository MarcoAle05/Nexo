// Ventanas pequeñas en el centro (Configuración, el menú de carpetas del workspace): un
// <dialog> modal (atrapa el foco, Escape lo cierra y el resto de la app queda inerte). Entra y
// sale con una transición de clases (`is-open`, `is-closing`; los estilos son los de
// `.settings`), no con animaciones: así se puede cerrar a mitad de la entrada.
export function createModal({ dialog, button = null, closeButton = null, initialFocus = () => null, onOpen = () => {}, onClose = () => {} }) {
  let closing = 0;
  function open() {
    if (dialog.open) return;
    clearTimeout(closing);
    closing = 0;
    dialog.classList.remove('is-closing');
    onOpen();
    dialog.showModal();
    button?.setAttribute('aria-expanded', 'true');
    // El estado de partida (transparente y algo más abajo) tiene que estar calculado antes de
    // poner `is-open`; si no, recién salida de display: none, no habría nada desde lo que pasar.
    void dialog.offsetHeight;
    dialog.classList.add('is-open');
    initialFocus()?.focus();
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
    button?.setAttribute('aria-expanded', 'false');
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
    button?.setAttribute('aria-expanded', 'false');
    onClose();
    if (button && (!document.activeElement || document.activeElement === document.body)) button.focus();
  });
  closeButton?.addEventListener('click', close);
  button?.addEventListener('click', () => (dialog.open ? close() : open()));
  return {
    open,
    close,
    get isOpen() {
      return dialog.open && !closing;
    },
  };
}
