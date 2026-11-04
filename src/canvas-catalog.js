// Lo que se puede poner en el lienzo: el catálogo del menú «Añadir», las formas de las figuras y
// las plantillas. Sin DOM (se prueba con node). Los nombres de `tipo`, `forma` y los roles de color
// son los que valida el motor (nexo_core::lienzo::datos).

const icon = (paths) => `<svg width="18" height="18" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;

export const ICONS = {
  texto: icon('<path d="M4 5V4h12v1M10 4v12M8 16h4"/>'),
  figura: icon('<rect x="3.5" y="5" width="13" height="10" rx="2"/>'),
  flecha: icon('<path d="M3.5 14.5 15 5M9 4.5h6.5V11"/>'),
  marco: icon('<path d="M3.5 7V4.5h4M12.5 4.5h4V7M16.5 13v2.5h-4M7.5 15.5h-4V13"/>'),
  adhesiva: icon('<path d="M4 4h12v8l-4 4H4z"/><path d="M12 16v-4h4"/>'),
  tabla: icon('<rect x="3.5" y="4" width="13" height="12" rx="1.5"/><path d="M3.5 8.5h13M3.5 12.5h13M8 4v12M12.5 4v12"/>'),
  entidad: icon('<rect x="3.5" y="4" width="13" height="12" rx="1.5"/><path d="M3.5 8h13M6 11h2M6 13.5h2M10 11h4M10 13.5h4"/>'),
  tablero: icon('<rect x="3.5" y="4" width="3.6" height="12" rx="1"/><rect x="8.2" y="4" width="3.6" height="8" rx="1"/><rect x="12.9" y="4" width="3.6" height="10" rx="1"/>'),
  'linea-tiempo': icon('<path d="M3.5 4.5v11"/><rect x="5.5" y="5" width="7" height="2.6" rx="1"/><rect x="8" y="9" width="8" height="2.6" rx="1"/><rect x="11" y="13" width="5" height="2.6" rx="1"/>'),
  secuencia: icon('<path d="M6 3.5v13M14 3.5v13" stroke-dasharray="2 2"/><path d="M6 7h8M12 5.5 14 7l-2 1.5M14 12.5H6M8 11l-2 1.5L8 14"/>'),
  codigo: icon('<path d="m7 6-4 4 4 4M13 6l4 4-4 4"/>'),
  referencia: icon('<path d="M8.5 11.5a3 3 0 0 0 4.2 0l2.3-2.3a3 3 0 0 0-4.2-4.2l-1 1"/><path d="M11.5 8.5a3 3 0 0 0-4.2 0L5 10.8A3 3 0 0 0 9.2 15l1-1"/>'),
  imagen: icon('<rect x="3.5" y="4.5" width="13" height="11" rx="1.5"/><circle cx="7.5" cy="8.5" r="1.2"/><path d="m4 14 4-3.5 3 2.5 2.5-2 3 2.5"/>'),
  plantilla: icon('<rect x="3.5" y="3.5" width="5.5" height="5.5" rx="1"/><rect x="11" y="3.5" width="5.5" height="5.5" rx="1"/><rect x="3.5" y="11" width="5.5" height="5.5" rx="1"/><rect x="11" y="11" width="5.5" height="5.5" rx="1"/>'),
  rectangulo: icon('<rect x="3.5" y="5.5" width="13" height="9"/>'),
  redondeado: icon('<rect x="3.5" y="5.5" width="13" height="9" rx="2.5"/>'),
  elipse: icon('<ellipse cx="10" cy="10" rx="7" ry="4.8"/>'),
  rombo: icon('<path d="m10 3.5 7 6.5-7 6.5L3 10z"/>'),
  paralelogramo: icon('<path d="M6.5 5.5H17l-3.5 9H3z"/>'),
  hexagono: icon('<path d="M6.5 5h7l3.5 5-3.5 5h-7L3 10z"/>'),
  documento: icon('<path d="M3.5 4.5h13v9.5c-2.2-1.3-4.4-.7-6.5 0s-4.3 1.3-6.5 0z"/>'),
  nube: icon('<path d="M6 15a3.2 3.2 0 0 1-.4-6.4 4.4 4.4 0 0 1 8.5-.8A3.7 3.7 0 0 1 14 15z"/>'),
  cilindro: icon('<ellipse cx="10" cy="5.5" rx="5.5" ry="2"/><path d="M4.5 5.5v9c0 1.1 2.5 2 5.5 2s5.5-.9 5.5-2v-9"/>'),
  servicio: icon('<rect x="3.5" y="5" width="13" height="10" rx="2"/><circle cx="7" cy="8" r="1"/><path d="M10 8h4M6 12h8"/>'),
  cola: icon('<path d="M5 6.5h10a2 3.5 0 0 1 0 7H5a2 3.5 0 0 1 0-7z"/><path d="M15 6.5a2 3.5 0 0 0 0 7M7.5 8v4M10.5 8v4"/>'),
  cache: icon('<rect x="3.5" y="5" width="13" height="10" rx="2"/><path d="m10.8 6.8-2.5 3.7h3.4l-2.5 3.7"/>'),
  api: icon('<path d="M7 4.5c-2 0-2 1.2-2 2.6S4.7 9.6 3.5 10c1.2.4 1.5 1.5 1.5 2.9s0 2.6 2 2.6M13 4.5c2 0 2 1.2 2 2.6s.3 2.5 1.5 2.9c-1.2.4-1.5 1.5-1.5 2.9s0 2.6-2 2.6"/>'),
  persona: icon('<circle cx="10" cy="6" r="2.5"/><path d="M4.5 16.5c.6-3.3 2.7-5 5.5-5s4.9 1.7 5.5 5"/>'),
  externo: icon('<rect x="3.5" y="5.5" width="13" height="9" rx="2" stroke-dasharray="2.5 2"/>'),
  inicio: icon('<circle cx="10" cy="10" r="4" fill="currentColor"/>'),
};

export const COLORS = [
  { id: 'neutro', label: 'Neutro' },
  { id: 'acento', label: 'Acento' },
  { id: 'azul', label: 'Azul' },
  { id: 'verde', label: 'Verde' },
  { id: 'ambar', label: 'Ámbar' },
  { id: 'rojo', label: 'Rojo' },
];

export const FORM_LABELS = {
  rectangulo: 'Rectángulo',
  redondeado: 'Redondeado',
  elipse: 'Elipse',
  rombo: 'Rombo (decisión)',
  paralelogramo: 'Paralelogramo (entrada o salida)',
  hexagono: 'Hexágono (preparación)',
  documento: 'Documento',
  nube: 'Nube',
  inicio: 'Inicio o fin',
  cilindro: 'Base de datos',
  servicio: 'Servicio',
  cola: 'Cola',
  cache: 'Caché',
  api: 'API',
  persona: 'Persona',
  externo: 'Sistema externo',
};
const FLOW_FORMS = ['rectangulo', 'redondeado', 'elipse', 'rombo', 'paralelogramo', 'hexagono', 'documento', 'nube', 'inicio'];
const ARCH_FORMS = ['servicio', 'cilindro', 'cola', 'cache', 'api', 'persona', 'externo'];
export const FORMS = [...FLOW_FORMS, ...ARCH_FORMS];

const SIZE = {
  texto: [264, 66],
  figura: [176, 88],
  marco: [660, 440],
  adhesiva: [176, 176],
  tabla: [398, 138],
  tablero: [748, 396],
  entidad: [264, 134],
  secuencia: [572, 264],
  codigo: [484, 198],
  'linea-tiempo': [660, 176],
  referencia: [264, 44],
  imagen: [330, 220],
};
const SHAPE_SIZE = { inicio: [44, 44], persona: [88, 110], rombo: [176, 110], cilindro: [132, 110] };
export const sizeOf = (tipo, forma) => (tipo === 'figura' && SHAPE_SIZE[forma]) || SIZE[tipo] || [176, 88];
// Los que se escriben nada más ponerlos.
export const WRITES_AT_ONCE = new Set(['texto', 'figura', 'adhesiva', 'marco', 'codigo']);

const item = (key, tipo, label, hint, extra = {}) => ({ key, tipo, label, hint, icon: extra.icon ?? ICONS[tipo], datos: extra.datos ?? {}, ...extra });
const form = (forma) => item(`figura-${forma}`, 'figura', FORM_LABELS[forma], FORM_LABELS[forma], { icon: ICONS[forma], datos: { forma } });
// Una plantilla es un punto de partida hecho con los mismos elementos.
const tpl = (key, label, hint) => item(key, null, label, hint, { icon: ICONS.plantilla, template: key });

export const CATALOG = [
  {
    group: 'Básicos',
    items: [
      item('texto', 'texto', 'Cuadro de texto', 'Texto suelto, sin ser una nota', { quick: true, shortcut: 'T' }),
      item('figura', 'figura', 'Figura', 'Una caja con texto', { quick: true, shortcut: 'R', datos: { forma: 'redondeado' } }),
      item('flecha', 'flecha', 'Flecha', 'Une dos elementos y los sigue', { quick: true, shortcut: 'F' }),
      item('marco', 'marco', 'Marco', 'Una región con título que mueve lo de dentro', { shortcut: 'M' }),
      item('adhesiva', 'adhesiva', 'Nota adhesiva', 'Una idea corta, sin pensar en el formato'),
    ],
    strip: FLOW_FORMS.map(form),
    stripLabel: 'Figuras',
  },
  {
    group: 'Datos',
    items: [
      item('tabla', 'tabla', 'Tabla', 'Celdas como en Excel: rangos, pegar y fórmulas', { quick: true, datos: { columnas: ['Columna 1', 'Columna 2', 'Columna 3'], filas: [['', '', ''], ['', '', ''], ['', '', '']] } }),
      item('entidad', 'entidad', 'Tabla relacional', 'Una tabla de base de datos: columnas, tipos, PK y FK', { datos: { nombre: 'tabla', columnas: [{ nombre: 'id', tipo: 'uuid', pk: true }, { nombre: 'nombre', tipo: 'text' }] } }),
    ],
  },
  {
    group: 'Planificación',
    items: [
      item('tablero', 'tablero', 'Tablero Scrum', 'Columnas y tarjetas con puntos y responsable', { datos: { titulo: 'Tablero' } }),
      item('linea-tiempo', 'linea-tiempo', 'Línea de tiempo', 'Tareas con fechas, dependencias e hitos', { datos: { titulo: 'Línea de tiempo' } }),
    ],
  },
  {
    group: 'Ingeniería',
    items: [
      item('secuencia', 'secuencia', 'Diagrama de secuencia', 'Quién habla con quién, y en qué orden', { datos: { participantes: ['Cliente', 'Servidor'], mensajes: [{ de: 'Cliente', a: 'Servidor', texto: 'petición' }, { de: 'Servidor', a: 'Cliente', texto: 'respuesta', tipo: 'respuesta' }] } }),
      item('codigo', 'codigo', 'Bloque de código', 'Fragmentos y contratos, con color'),
    ],
    strip: ARCH_FORMS.map(form),
    stripLabel: 'Arquitectura',
  },
  {
    group: 'Enlaces',
    items: [
      item('referencia', 'referencia', 'Referencia', 'Abre una nota, un nodo del grafo o una fuente'),
      item('imagen', 'imagen', 'Imagen', 'Un archivo de tu equipo; también puedes pegarla'),
    ],
  },
  {
    group: 'Plantillas',
    items: [
      tpl('decision', 'Matriz de decisión', 'Opciones por criterios con peso y total'),
      tpl('riesgos', 'Riesgos', 'Probabilidad, impacto y mitigación'),
      tpl('retro', 'Retrospectiva', 'Qué fue bien, qué mejorar, acciones'),
      tpl('estados', 'Máquina de estados', 'Estados y transiciones'),
      tpl('flujo', 'Flujo', 'Inicio, pasos y una decisión'),
    ],
  },
];

export const catalogItem = (key) => {
  for (const g of CATALOG) {
    const found = [...g.items, ...(g.strip ?? [])].find((i) => i.key === key);
    if (found) return found;
  }
  return null;
};

// El dibujo de una figura en una caja de w×h; las que son solo una caja van con CSS (null).
export function shapePath(forma, w, h) {
  const s = (v) => Math.round(v * 10) / 10;
  switch (forma) {
    case 'rombo':
      return `M ${s(w / 2)} 1 L ${s(w - 1)} ${s(h / 2)} L ${s(w / 2)} ${s(h - 1)} L 1 ${s(h / 2)} Z`;
    case 'elipse':
    case 'inicio':
      return `M 1 ${s(h / 2)} A ${s(w / 2 - 1)} ${s(h / 2 - 1)} 0 1 1 ${s(w - 1)} ${s(h / 2)} A ${s(w / 2 - 1)} ${s(h / 2 - 1)} 0 1 1 1 ${s(h / 2)} Z`;
    case 'paralelogramo': {
      const d = Math.min(w * 0.18, 44);
      return `M ${s(d)} 1 L ${s(w - 1)} 1 L ${s(w - d)} ${s(h - 1)} L 1 ${s(h - 1)} Z`;
    }
    case 'hexagono': {
      const d = Math.min(w * 0.18, h / 2);
      return `M ${s(d)} 1 L ${s(w - d)} 1 L ${s(w - 1)} ${s(h / 2)} L ${s(w - d)} ${s(h - 1)} L ${s(d)} ${s(h - 1)} L 1 ${s(h / 2)} Z`;
    }
    case 'documento': {
      const o = Math.min(h * 0.14, 14);
      return `M 1 1 L ${s(w - 1)} 1 L ${s(w - 1)} ${s(h - o)} C ${s(w * 0.75)} ${s(h - o * 2)} ${s(w * 0.6)} ${s(h - o * 2)} ${s(w / 2)} ${s(h - o)} S ${s(w * 0.2)} ${s(h)} 1 ${s(h - o)} Z`;
    }
    case 'nube': {
      const [x, y] = [w / 100, h / 60];
      const p = (a, b) => `${s(a * x)} ${s(b * y)}`;
      return `M ${p(25, 54)} C ${p(6, 54)} ${p(2, 34)} ${p(18, 30)} C ${p(14, 12)} ${p(40, 2)} ${p(50, 16)} C ${p(58, 4)} ${p(82, 8)} ${p(80, 26)} C ${p(98, 26)} ${p(100, 54)} ${p(78, 54)} Z`;
    }
    case 'cilindro':
    case 'cache-cilindro': {
      const r = Math.min(h * 0.14, 16);
      return `M 1 ${s(r)} A ${s(w / 2 - 1)} ${s(r - 1)} 0 0 1 ${s(w - 1)} ${s(r)} L ${s(w - 1)} ${s(h - r)} A ${s(w / 2 - 1)} ${s(r - 1)} 0 0 1 1 ${s(h - r)} Z M 1 ${s(r)} A ${s(w / 2 - 1)} ${s(r - 1)} 0 0 0 ${s(w - 1)} ${s(r)}`;
    }
    case 'cola': {
      const r = Math.min(w * 0.08, 16);
      return `M ${s(r)} 1 L ${s(w - r)} 1 A ${s(r - 1)} ${s(h / 2 - 1)} 0 0 1 ${s(w - r)} ${s(h - 1)} L ${s(r)} ${s(h - 1)} A ${s(r - 1)} ${s(h / 2 - 1)} 0 0 1 ${s(r)} 1 Z M ${s(w - r)} 1 A ${s(r - 1)} ${s(h / 2 - 1)} 0 0 0 ${s(w - r)} ${s(h - 1)}`;
    }
    case 'persona': {
      const c = w / 2;
      const r = Math.min(w * 0.2, h * 0.16);
      const top = r * 2 + 3;
      const pie = Math.max(top + 10, h - 30);
      return `M ${s(c - r)} ${s(r + 1)} A ${s(r)} ${s(r)} 0 1 1 ${s(c + r)} ${s(r + 1)} A ${s(r)} ${s(r)} 0 1 1 ${s(c - r)} ${s(r + 1)} Z M ${s(c - w * 0.38)} ${s(pie)} C ${s(c - w * 0.38)} ${s(top + 2)} ${s(c + w * 0.38)} ${s(top + 2)} ${s(c + w * 0.38)} ${s(pie)} Z`;
    }
    default:
      return null;
  }
}

// Las operaciones de una plantilla, con su esquina en (x, y). Son elementos normales.
export function templateOps(key, x, y) {
  const fig = (ref, dx, dy, texto, forma = 'redondeado', color = 'neutro') => {
    const [w, h] = forma === 'rombo' ? [176, 110] : forma === 'inicio' ? [44, 44] : [176, 88];
    return { op: 'crear', ref, tipo: 'figura', x: x + dx, y: y + dy, w, h, datos: { forma, texto, color } };
  };
  const arrow = (de, a, texto) => ({ op: 'crear', tipo: 'flecha', datos: { desde: { id: de }, hasta: { id: a }, ...(texto ? { texto } : {}) } });
  switch (key) {
    case 'decision':
      return [
        {
          op: 'crear',
          tipo: 'tabla',
          x,
          y,
          datos: {
            titulo: 'Matriz de decisión',
            columnas: ['Criterio', 'Peso', 'Opción A', 'Opción B'],
            filas: [
              ['Coste', '3', '', ''],
              ['Esfuerzo', '2', '', ''],
              ['Riesgo', '2', '', ''],
              ['Total', '', '=B2*C2+B3*C3+B4*C4', '=B2*D2+B3*D3+B4*D4'],
            ],
          },
        },
      ];
    case 'riesgos':
      return [
        {
          op: 'crear',
          tipo: 'tabla',
          x,
          y,
          datos: {
            titulo: 'Riesgos',
            columnas: ['Riesgo', 'Probabilidad (1-5)', 'Impacto (1-5)', 'Exposición', 'Mitigación'],
            filas: [
              ['', '', '', '=B2*C2', ''],
              ['', '', '', '=B3*C3', ''],
              ['', '', '', '=B4*C4', ''],
            ],
          },
        },
      ];
    case 'retro':
      return [{ op: 'crear', tipo: 'tablero', x, y, datos: { titulo: 'Retrospectiva', columnas: ['Qué fue bien', 'Qué mejorar', 'Acciones'] } }];
    case 'estados':
      return [fig('i', 0, 22, '', 'inicio'), fig('a', 110, 0, 'Estado A'), fig('b', 374, 0, 'Estado B'), fig('f', 638, 22, '', 'inicio'), arrow('i', 'a'), arrow('a', 'b', 'evento'), arrow('b', 'f')];
    case 'flujo':
      return [fig('a', 0, 11, 'Inicio', 'elipse'), fig('b', 264, 11, 'Paso'), fig('c', 528, 0, '¿Condición?', 'rombo', 'ambar'), fig('d', 792, 11, 'Fin', 'elipse'), arrow('a', 'b'), arrow('b', 'c'), arrow('c', 'd', 'sí')];
    default:
      return [];
  }
}
