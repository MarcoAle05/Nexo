// Pone en <html> el tema guardado antes de que se pinte nada: sin esto, un tema claro se vería
// un instante con los colores de Órbita. Lo demás (elegirlo, guardarlo) lo hace src/themes.js.
try {
  const id = localStorage.getItem('nexo.theme');
  if (id && id !== 'orbita' && /^[a-z-]+$/.test(id)) document.documentElement.dataset.theme = id;
} catch {
  // Sin almacenamiento, Órbita.
}
