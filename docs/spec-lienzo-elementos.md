# Especificación: elementos y capas en el lienzo de Notas

El lienzo de Notas deja de ser solo para notas. Se le pueden poner cuadros de texto, figuras, flechas, tablas, tableros Scrum y otras piezas que **no son notas**: no crean archivos en la wiki ni salen en el grafo. Y el agente de Nexo (el chat de la carta derecha) aprende a planear ahí, con diagramas y tablas, **en una capa propia**, de modo que el usuario y él puedan trabajar a la vez sin estorbarse.

**Estado:** implementada el 03-11-2026; falta probarla con la app real (ver «Notas de la implementación», al final). Las marcadas «(usuario)» las decidió el usuario:
- el alcance (la petición de `/to-spec`): los elementos, dónde se añaden, la skill y la capa propia del agente;
- las tres costuras de prueba.

Lo marcado «(propuesta)» es idea añadida al escribir esto, porque el usuario pidió ideas de elementos útiles para ingenieros. Se puede quitar sin que caiga lo demás.

Se publica en `docs/`, como la anterior, porque el proyecto no tiene gestor de issues configurado.

## Vocabulario

- **Carta**: una nota abierta en el lienzo (ya existe). Sigue siendo un archivo de `wiki/notas/`.
- **Elemento**: lo que se pone en el lienzo y no es una nota. Vive con su escritorio, no en la wiki.
- **Capa**: un grupo de elementos con un autor, que se puede ocultar, integrar o borrar de una vez. Hay una del usuario («Tú») y una por cada trabajo del agente.
- **Orden de apilado**: lo que hoy las reglas de Notas llaman «capa» de una carta (el `z`, traer al frente). Se le cambia el nombre en la regla para que no se confunda.
- **Operación**: un cambio pequeño en el lienzo (crear, cambiar, borrar…). Todo cambio es una operación, venga del usuario o del agente.
- **Motor del lienzo**: el módulo único que aplica las operaciones y guarda el resultado.
- **En uso**: el usuario lo está arrastrando o escribiendo ahora mismo.
- **Tocado**: el usuario ha cambiado ese elemento de una capa del agente; desde entonces es suyo.
- **Zona**: un rectángulo libre del lienzo que el motor le da al agente para dibujar.

## Problema

- **Para dibujar hay que crear notas.** Cada recuadro del lienzo es un archivo en `wiki/notas/`, una fila en el árbol y un nodo en el grafo. Un diagrama de veinte cajas serían veinte notas de basura.
- **No hay con qué planear.** No se pueden unir dos cosas con una flecha, ni hacer una tabla que se edite como una hoja de cálculo, ni un tablero, ni un modelo de datos.
- **El agente no puede planear en visual.** Solo sabe abrir cartas, y para eso edita a mano `.nexo/escritorio.json`, el mismo archivo que escribe la interfaz.
- **Si dibujara hoy, chocaría con el usuario:**
  - gana quien escribe el archivo el último, así que uno borraría los cambios del otro;
  - cada carta nueva lleva la vista hacia ella, y le movería el lienzo al usuario mientras trabaja;
  - cada orden pediría permiso en el chat.

## Solución

### Dónde se añaden

A la izquierda del minimapa de la barra superior aparece **«Añadir»**, solo en la vista Notas (usuario). Abre un menú con todos los elementos por grupos. Si la ventana es ancha, los más usados salen además como botones sueltos. Al lado está **«Capas»**.

### Qué se puede añadir

| Grupo | Elemento | Para qué | Origen |
| --- | --- | --- | --- |
| Básicos | **Cuadro de texto** | Texto suelto con formato, sin ser una nota | usuario |
| | **Figuras** | Rectángulo, redondeado, elipse, rombo, paralelogramo, hexágono, documento, nube | usuario |
| | **Flechas** | Unen elementos y los siguen al moverlos; rectas, en ángulo o curvas | usuario |
| | **Marco** | Región con título que mueve lo que tiene dentro: un subsistema, una fase | propuesta |
| | **Nota adhesiva** | Una idea corta de color, para ordenar ideas | propuesta |
| Datos | **Tabla tipo Excel** | Celdas, rangos, pegar desde Excel o Sheets, fórmulas sencillas | usuario |
| | **Tabla relacional** | Una tabla de base de datos: columnas, tipos, PK y FK, con relaciones de fila a fila | usuario |
| Planificación | **Tablero Scrum** | Columnas y tarjetas con puntos y responsable | usuario |
| | **Línea de tiempo** | Tareas con fechas, dependencias e hitos (tipo Gantt) | propuesta |
| Ingeniería | **Figuras de arquitectura** | Servicio, base de datos, cola, caché, API, persona, sistema externo | propuesta |
| | **Diagrama de secuencia** | Participantes y mensajes; se coloca solo. A mano con flechas sueltas es muy pesado | propuesta |
| | **Bloque de código** | Fragmentos, contratos de API, con el coloreado del lector | propuesta |
| Enlaces | **Referencia** | Un chip que abre una nota, un nodo del grafo o una fuente: une el plan con la wiki | propuesta |
| | **Imagen** | Pegar una captura o un esquema | propuesta |
| Plantillas | **Matriz de decisión** | Opciones por criterios con peso y total: es una tabla con fórmulas ya puestas | propuesta |
| | **Riesgos**, **Retrospectiva**, **Máquina de estados**, **Flujo** | Puntos de partida hechos con los elementos de arriba | propuesta |

Se quedan para después: dibujo libre, fórmulas LaTeX, importar y exportar Mermaid o SQL, exportar un marco a imagen, comentarios dirigidos al agente y símbolos de disciplinas concretas (eléctrica, mecánica).

### Cómo se reparten el lienzo el usuario y el agente

Le pides al chat, por ejemplo: «Quiero implementar un sistema RAG del grafo de nexo y planearlo con diagramas, todo en el lienzo». El agente crea una capa «RAG del grafo», pide una zona libre y va dibujando por tandas: un marco de contexto, el diagrama de arquitectura, las tablas relacionales, la secuencia de una pregunta y el tablero de tareas. Mientras tanto:

- **El agente nunca te mueve nada:** ni la vista, ni el foco, ni la selección, ni el cursor mientras escribes. Un indicador discreto en el minimapa dice dónde está dibujando, y vas allí solo si quieres.
- **No dibuja encima de lo tuyo:** el motor le da zonas libres.
- **Puedes tocar lo suyo en cualquier momento.** Lo que tocas pasa a ser tuyo: el agente ya no puede cambiarlo y se le avisa en la respuesta, para que siga con lo demás.
- **Tu Ctrl+Z deshace solo lo tuyo.** Lo del agente se quita entero borrando su capa.
- **Al terminar,** la capa se queda hasta que tú decidas: integrarla en la tuya o borrarla.

Nada de esto toca la wiki, el grafo ni las notas.

## Historias de usuario

**Añadir elementos**

1. Como usuario, quiero un apartado «Añadir» a la izquierda del minimapa, para tener a mano lo que puedo poner en el lienzo sin salir de él.
2. Como usuario, quiero que ese apartado salga solo en la vista Notas, para que no ocupe sitio en las demás.
3. Como usuario, quiero un menú con todos los elementos por grupos, con su icono y una línea que diga para qué sirve, para encontrar el que necesito aunque no me sepa el nombre.
4. Como usuario con una ventana ancha, quiero los elementos que más uso (texto, figura, flecha, tabla) como botones sueltos, para no abrir un menú cada vez.
5. Como usuario con una ventana estrecha, quiero que todo quepa en un botón, sin que pise el selector de vistas.
6. Como usuario, quiero elegir un elemento y colocarlo con un clic en el lienzo, o arrastrando para darle tamaño, para ponerlo justo donde lo quiero.
7. Como usuario, quiero cancelar con Escape un elemento elegido, para no dejarlo puesto por error.
8. Como usuario de teclado, quiero que Intro en el menú coloque el elemento en el centro del hueco libre y lo seleccione, para no necesitar el ratón.
9. Como usuario, quiero poder escribir enseguida en lo que acabo de poner si lleva texto, para no hacer un clic de más.

**Cuadros de texto y figuras**

10. Como usuario, quiero cuadros de texto que no se guarden como notas, para hacer diagramas sin llenar el árbol de notas y el grafo de cosas sueltas.
11. Como usuario, quiero negritas, código, listas y enlaces en un cuadro de texto, para que se lea bien.
12. Como usuario, quiero las figuras habituales de un diagrama de flujo (rectángulo, rombo, paralelogramo, elipse…), para dibujar procesos y decisiones.
13. Como ingeniero, quiero figuras de arquitectura (servicio, base de datos, cola, caché, API, persona, sistema externo), para dibujar sistemas sin inventar la notación.
14. Como usuario, quiero escribir dentro de una figura con doble clic o Intro, para ponerle nombre sin añadir un cuadro aparte.
15. Como usuario, quiero que la figura se vea igual de clara en cualquier tema y que su color cambie con él, para no rehacer el diagrama al cambiar de tema.
16. Como usuario, quiero elegir el color entre una paleta corta, para distinguir partes sin que el diagrama parezca un arcoíris.
17. Como usuario, quiero convertir un cuadro de texto en una nota de verdad, para conservar lo que acabó valiendo la pena.

**Flechas**

18. Como usuario, quiero tirar una flecha desde el borde de un elemento hasta otro, para unirlos sin apuntar con precisión.
19. Como usuario, quiero que la flecha siga a los elementos cuando los muevo, para no redibujarla.
20. Como usuario, quiero flechas rectas, en ángulo o curvas, para que el diagrama quede limpio.
21. Como usuario, quiero elegir el extremo (nada, flecha, triángulo, rombo o pata de gallo) y el trazo (continuo o discontinuo), para usar la notación que toca.
22. Como usuario, quiero escribir una etiqueta sobre la flecha, para decir qué pasa por ella.
23. Como usuario, quiero que una flecha pueda tener un extremo suelto, para señalar algo que aún no existe.
24. Como usuario, quiero que al borrar un elemento sus flechas no desaparezcan y queden con ese extremo suelto, para no perder trabajo sin querer.

**Tablas tipo Excel**

25. Como usuario, quiero una tabla con cabecera en la que escribir celda por celda, para llevar datos en el lienzo.
26. Como usuario, quiero moverme con flechas, Tab e Intro como en una hoja de cálculo, para no soltar el teclado.
27. Como usuario, quiero seleccionar un rango con el ratón o con Mayús y las flechas, para copiarlo o borrarlo.
28. Como usuario, quiero pegar desde Excel o Google Sheets y copiar hacia ellos, para no pasar los datos a mano.
29. Como usuario, quiero añadir y quitar filas y columnas y cambiar el ancho de las columnas, para ajustar la tabla a lo que necesito.
30. Como usuario, quiero fórmulas sencillas (`=SUMA(B2:B9)`, `=PROMEDIO`, `=MIN`, `=MAX`, `=CONTAR`, `=SI`, aritmética y referencias), para que una tabla calcule.
31. Como usuario, quiero que una fórmula mal escrita o circular muestre su error en la celda, para ver qué falla sin romper la tabla.
32. Como usuario, quiero ordenar por una columna, para ver los datos como me conviene.
33. Como usuario, quiero que la rueda desplace la tabla si es larga y el lienzo si no, igual que en una nota.
34. Como usuario, quiero que pegar algo que no cabe en el límite de la tabla me lo diga, y no que se corte en silencio.

**Tablero Scrum**

35. Como usuario, quiero un tablero con columnas (Por hacer, En curso, Hecho de partida), para seguir un trabajo.
36. Como usuario, quiero tarjetas con título, puntos, responsable y etiqueta, para organizar las tareas.
37. Como usuario, quiero arrastrar una tarjeta entre columnas, y también moverla con el teclado, para actualizar el estado.
38. Como usuario, quiero ver cuántas tarjetas y puntos hay en cada columna, para ver la carga de un vistazo.
39. Como usuario, quiero poner un límite a una columna y que avise al pasarlo, para no acumular trabajo en curso.
40. Como usuario, quiero añadir, renombrar, quitar y reordenar columnas, para adaptar el tablero a mi manera de trabajar.

**Elementos para ingenieros**

41. Como ingeniero, quiero tablas relacionales con columnas, tipos, PK y FK, para modelar datos.
42. Como ingeniero, quiero relaciones de una columna a otra, con cardinalidad en pata de gallo, para ver cómo se unen las tablas.
43. Como ingeniero, quiero un diagrama de secuencia al que añado participantes y mensajes y que se coloca solo, para explicar una interacción sin colocar flechas una a una.
44. Como ingeniero, quiero bloques de código con coloreado, para poner contratos y fragmentos junto al diagrama.
45. Como usuario, quiero una línea de tiempo con tareas, dependencias e hitos, para ver el orden del trabajo.
46. Como usuario, quiero marcos con título que muevan lo que tienen dentro, también las cartas de notas, para tratar un subsistema como un bloque.
47. Como usuario, quiero notas adhesivas, para soltar ideas sin pensar en el formato.
48. Como usuario, quiero un elemento de referencia a una nota, un nodo del grafo o una fuente, que los abra al pulsarlo, para unir el plan con lo que ya sé.
49. Como usuario, quiero que una referencia rota diga que ya no existe, para no fiarme de un enlace muerto.
50. Como usuario, quiero pegar una imagen del portapapeles, para poner capturas junto al plan.
51. Como usuario, quiero plantillas (matriz de decisión, riesgos, retrospectiva, máquina de estados, flujo), para no empezar de cero.

**Trabajar con los elementos**

52. Como usuario, quiero seleccionar con un clic, sumar con Mayús y seleccionar por área con Mayús y arrastrar, para trabajar con varios a la vez.
53. Como usuario, quiero mover el lienzo con el botón derecho desde cualquier sitio, aunque el ratón esté sobre un texto, una tabla o una nota, para no tener que buscar un hueco (lo pidió el usuario el 03-11-2026). Arrastrar el fondo y la rueda siguen haciendo lo de siempre.
54. Como usuario, quiero mover y redimensionar con ajuste a la retícula de puntos que ya se ve, para que todo quede alineado.
55. Como usuario, quiero alinear y repartir lo seleccionado, para ordenar un grupo en un momento.
56. Como usuario, quiero copiar, pegar y duplicar, también entre escritorios, para reutilizar.
57. Como usuario, quiero borrar con Supr y deshacer con Ctrl+Z y rehacer con Ctrl+Mayús+Z, para corregirme sin miedo.
58. Como usuario, quiero traer al frente y enviar al fondo, para resolver lo que se solapa.
59. Como usuario, quiero una barra pequeña junto a lo seleccionado con sus opciones (color, trazo, forma, extremos), para no ir a buscarlas a otro sitio.
60. Como usuario, quiero que el zoom funcione igual con los elementos, para ver el diagrama entero.
61. Como usuario, quiero que el minimapa enseñe los elementos, para saber dónde está cada cosa.
62. Como usuario, quiero que Inicio en el minimapa me lleve a todo lo que hay, también a los elementos, para no perderme.
63. Como usuario, quiero que una nota nueva no se abra encima de un elemento, para no tener que apartarla.
64. Como usuario, quiero que el lienzo vacío explique que también se pueden añadir elementos, para descubrirlo.
65. Como usuario, quiero que cada escritorio tenga sus elementos y que al archivarlo se queden con él, para separar los trabajos.
66. Como usuario, quiero que la lista de escritorios cuente también los elementos, para saber qué tiene cada uno.
67. Como usuario, quiero que al borrar un escritorio archivado se me diga que sus elementos se van a la papelera, porque, a diferencia de las notas, no están en ningún otro sitio.
68. Como usuario, quiero que al cerrar y volver a abrir nexo todo esté donde lo dejé, para seguir trabajando.
69. Como usuario de teclado o de lector de pantalla, quiero recorrer los elementos, moverlos y oír qué son y qué une cada flecha, para poder usar el lienzo sin ratón.
70. Como usuario que pide menos movimiento, quiero que lo que aparezca no se anime, para no marearme.

**Capas**

71. Como usuario, quiero un apartado «Capas» junto a «Añadir», con cada capa, su autor, sus elementos y su estado, para saber qué hay en el lienzo.
72. Como usuario, quiero ocultar y mostrar una capa, para ver lo mío sin lo del agente, y al revés.
73. Como usuario, quiero ir a una capa desde esa lista, para encontrar lo que dibujó el agente.
74. Como usuario, quiero integrar una capa del agente en la mía, para quedármela como trabajo propio.
75. Como usuario, quiero borrar una capa del agente de una vez y poder deshacerlo, para descartar un intento sin borrar pieza por pieza.
76. Como usuario, quiero que las cartas de notas sean siempre de mi capa, para no pensar en ello.

**El agente dibuja en el lienzo**

77. Como usuario, quiero pedirle al chat en lenguaje normal que planee algo en visual en el lienzo, para no aprender ninguna orden.
78. Como usuario, quiero también lanzarlo con `/nexo-lienzo`, para pedirlo explícitamente.
79. Como usuario, quiero que el agente dibuje diagramas, tablas relacionales, secuencias, tablas y tableros y elija el que toque, para que el plan sea visual de verdad.
80. Como usuario, quiero que dibuje en su propia capa con un nombre claro, para saber qué es suyo.
81. Como usuario, quiero ver cómo va apareciendo por tandas y no todo de golpe al final, para seguirlo y corregirlo a tiempo.
82. Como usuario, quiero que lea el grafo y lo que haga falta antes de dibujar y que marque lo que supone como «por confirmar», para no recibir un plan inventado.
83. Como usuario, quiero que me diga antes si va a ser algo grande, porque gasta de mi sesión de 5 h.
84. Como usuario, quiero que al terminar me diga en el chat qué dibujó y dónde está, para ir a verlo cuando quiera.
85. Como usuario, quiero que el chat no me pida permiso por cada trazo, pero sí para todo lo demás, para que la tarea no sea un clic constante.
86. Como usuario, quiero pedirle que ordene el lienzo, también mis cartas y elementos, para limpiarlo sin hacerlo a mano.
87. Como usuario, quiero poder deshacer ese orden con un Ctrl+Z, para volver atrás si no me gusta.
88. Como usuario, quiero pedirle cambios sobre lo que dibujó («añade una caché») y que primero mire cómo está ahora, para que tenga en cuenta lo que yo cambié.
89. Como usuario, quiero poder pedirle que me lleve a lo que dibujó, para no buscarlo.
90. Como usuario, quiero que la capa quede como «Sin terminar» si detengo al agente o se reinicia nexo a medias, para no confundir un trabajo cortado con uno acabado.
91. Como usuario, quiero que dibujar funcione aunque esté en la vista Grafo o Agentes, para seguir con lo mío y ver el resultado después.

**Que no nos estorbemos**

92. Como usuario, quiero que lo que dibuja el agente nunca mueva mi vista, mi foco, mi selección ni mi cursor, para no perder el hilo.
93. Como usuario, quiero que nunca dibuje encima de lo que ya hay, para no tener que apartar nada.
94. Como usuario, quiero poder mover, cambiar o borrar lo que dibujó mientras sigue trabajando, para corregirlo en el momento.
95. Como usuario, quiero que lo que toco se quede como lo dejé, para que el agente no lo revierta.
96. Como usuario, quiero que si cambio o borro algo suyo él siga con lo demás y lo sepa, para que no se pare ni repita lo que quité.
97. Como usuario, quiero que nada de lo que estoy arrastrando o escribiendo cambie bajo mis manos, para trabajar tranquilo.
98. Como usuario, quiero que el agente no pueda cambiar mis elementos, salvo que se lo haya pedido, para que lo mío esté a salvo.
99. Como usuario, quiero que en un tablero o una tabla compartida el agente pueda seguir añadiendo aunque yo haya movido una tarjeta o cambiado una celda, para que tocar una parte no le bloquee el resto.
100. Como usuario, quiero que Ctrl+Z deshaga solo lo mío, para no tumbar el trabajo del agente sin querer.
101. Como usuario, quiero poder cambiar de escritorio o de vista mientras dibuja y que él siga en el escritorio donde empezó, para que mi navegación no le corte.
102. Como usuario, quiero poder ocultar su capa mientras dibuja, sin que eso le afecte, para concentrarme.
103. Como usuario, quiero un indicador discreto de dónde está dibujando, con el que pueda ir allí, para decidir yo cuándo mirar.
104. Como usuario, quiero que el lienzo no vaya a tirones mientras el agente dibuja, para no notar que está trabajando.
105. Como usuario, quiero que varios agentes tengan cada uno su capa, para que tampoco se pisen entre ellos.
106. Como usuario, quiero que un lector de pantalla me anuncie solo cuándo empieza y termina una capa, no cada figura, para que no me interrumpa.

## Decisiones de implementación

### El motor del lienzo: un solo escritor

- **Módulo nuevo en la lógica sin Tauri (`nexo-core`).** Es el corazón de la funcionalidad y lo más probado. Todo cambio pasa por él, venga de la interfaz o del agente. Nadie edita a mano los archivos del lienzo, así que no se puede repetir la carrera de `.nexo/escritorio.json`.
- **Interfaz estrecha:** `leer(escritorio)`, `aplicar(escritorio, origen, operaciones)`, `zona(escritorio, tamaño, cerca_de)`, `ordenar(…)` y `diagrama(…)`.
- **`aplicar` nunca falla en bloque.** Cada operación tiene su resultado:

  ```
  { rev, hechas: [{ ref, id }], omitidas: [{ ref|id, motivo, actual }], cambios, inversas, usuario_cambio }
  ```

  - `omitidas`: lo que no se aplicó, con el motivo y el estado actual, para que el agente se adapte;
  - `cambios`: los elementos que hay que repintar. La interfaz solo pinta por aquí, también sus propias operaciones;
  - `inversas`: las operaciones que deshacen esta. Solo se usan para el Ctrl+Z del usuario;
  - `usuario_cambio`: lo que el usuario cambió en esa capa desde la última orden del agente, para que se entere sin tener que preguntar.
- **Operaciones:** crear, cambiar por partes, borrar, cambiar el orden de apilado y las de capas (nueva, renombrar, mostrar u ocultar, terminar, reabrir, integrar, borrar). Las tablas y los tableros tienen las suyas (cambiar una celda, añadir una fila, añadir o mover una tarjeta…).
- **Referencias en un lote:** un lote puede nombrar elementos que aún no existen (`ref: "a"`), para crear dos cajas y su flecha en una sola orden.
- **Los identificadores los pone el motor.**
- **Topes:** 5000 elementos por escritorio, 500 operaciones por lote, tablas de hasta 200 filas por 30 columnas y un tamaño máximo de texto. Pasarse se informa como `omitidas`, sin cortar en silencio.

### Las reglas de convivencia (dentro del motor)

| Quién | Sobre qué | Qué pasa |
| --- | --- | --- |
| Agente | Su capa, sin tocar por el usuario | Se aplica |
| Agente | Algo de su capa que el usuario tocó | Omitida, con el estado actual |
| Agente | Algo de la capa «Tú» | Omitida, salvo con el permiso explícito de abajo |
| Agente | Algo **en uso** | Omitida siempre, con cualquier permiso |
| Agente | Crear | Siempre en su propia capa |
| Agente | Añadir una fila o una tarjeta a algo de su capa | Se aplica siempre: añadir no choca con nadie |
| Agente | Crear o mover algo encima de lo ajeno | Nunca: un lote se mueve entero a un hueco y se le avisa; mover, se omite |
| Usuario | Cualquier cosa | Se aplica siempre y marca lo cambiado como **tocado** |

- **Permiso explícito:** para cuando el usuario pide en el chat «añade una fila a mi tabla». La orden lleva `--del-usuario` y la skill solo permite usarlo si se pidió en la conversación. Sus inversas entran en el Ctrl+Z del usuario, igual que las de `ordenar`.
- **Grano fino en lo compuesto:** en tablas y tableros, lo tocado es la celda o la tarjeta, no el elemento entero. Si no, mover una tarjeta dejaría al agente sin poder añadir nada más al tablero.
- **En uso:** la interfaz lo avisa al empezar un gesto y al acabarlo. Solo vive en memoria, y se limpia si la interfaz se recarga. Si una operación del agente se cuela en el instante entre el gesto y el aviso, al soltar gana el usuario, porque escribe después.
- **Estado de una capa:** `dibujando`, `terminada` o `sin_terminar`. Una capa abierta pasa a `sin_terminar` al arrancar nexo o tras varios minutos sin operaciones. Sigue la regla que ya se aplicó a los agentes: si no se sabe cómo acabó algo, nunca se da por bueno.

### Datos y guardado

- **Un archivo por escritorio:** `.nexo/lienzos/<id del escritorio>.json`, en el vault, fuera de `wiki/`. No se guarda en `.nexo/escritorio.json`:
  - ese archivo se reescribe a cada rato;
  - lo editan a mano;
  - se copia entero a `escritorios.json` al cambiar de escritorio.

  Con un archivo aparte, archivar y restaurar un escritorio no cambian: basta con su `id`.
- **El formato de las cartas no cambia.** Las notas siguen en `.nexo/escritorio.json`.
- **Forma del archivo:**

  ```jsonc
  {
    "version": 1,
    "rev": 128,
    "capas": [
      { "id": "tu", "nombre": "Tú", "autor": "usuario", "visible": true },
      { "id": "c1", "nombre": "RAG del grafo", "autor": "nexo", "visible": true, "estado": "dibujando" }
    ],
    "elementos": [
      { "id": "e1", "capa": "c1", "tipo": "figura", "x": 0, "y": 0, "w": 176, "h": 88, "z": 3,
        "tocado": false, "datos": { "forma": "cilindro", "texto": "Chunks", "color": "azul" } }
    ]
  }
  ```

  Coordenadas del lienzo, las mismas de las cartas. Una flecha guarda `desde` y `hasta` como `{ id, ancla }` o `{ x, y }`.
- **Colores por rol, nunca hexadecimales:** `neutro`, `acento`, `azul`, `verde`, `ambar`, `rojo`. Cada tema los traduce (ver «Temas»).
- **Guardado:** agrupado (como mucho 500 ms después del último cambio), en un temporal que luego se renombra y sin falta al cerrar. Si un archivo no se entiende, se aparta con otro nombre, se avisa y no se sobrescribe.
- **El id del escritorio tiene que ser estable.** Hoy, mientras nadie cambie nada en los escritorios, `escritorios.json` no se escribe y el id del primer escritorio se inventa en cada arranque. Con un archivo por id, los elementos se perderían. El primer escritorio pasa a tener un id fijo, que la interfaz y el backend calculan igual.
- **Guardián de rutas:** el id de un escritorio y el nombre de una imagen pasan por el guardián antes de formar una ruta, porque llegan de la interfaz o de la orden de control.
- **Imágenes:** en `.nexo/lienzos/recursos/`, con un nombre sacado de su contenido, hasta 20 MB. Se sirven a la interfaz como `data:` para no cambiar la CSP. Solo desde el portapapeles o un archivo soltado: nunca una URL remota, ni siquiera del agente. Un texto de una fuente no puede hacer que nexo llame a una dirección de fuera.
- **Fuera de la wiki por diseño:** los elementos no salen en el árbol, ni en el grafo, ni en «Buscar», ni en el servidor MCP, ni en los commits automáticos del vault. Consecuencia que hay que saber: el lienzo no tiene historial de versiones.
- **Borrar un escritorio archivado** manda su archivo a la papelera y su aviso lo dice. Hoy dice que «solo se pierde cómo estaban colocadas» y, con los elementos, eso ya no sería verdad.

### La orden de control y la skill

- **Se amplía `nexo ctl` con `lienzo`,** sobre la misma costura y el mismo socket. Si nexo no está abierto, sale con 2 y lo dice, como las demás.

  | Orden | Qué hace |
  | --- | --- |
  | `estado` | Escritorio activo, capas, `rev` |
  | `capa-nueva "<nombre>"`, `capa-terminar` | Empieza o acaba su capa. Volver a dibujar en ella la reabre |
  | `zona --ancho --alto [--cerca-de <id>]` | Un rectángulo libre |
  | `aplicar <capa> '<json>'` | Un lote de operaciones |
  | `diagrama <capa> '<json>'` | Un diagrama declarativo (ver abajo) |
  | `leer [--capa] [--desde <rev>] [--texto]` | El contenido, con lo que cambió el usuario |
  | `ordenar <fila\|rejilla\|flujo> [--capa] [--ids] [--notas]` | Ordena; deshacer es un solo paso para el usuario |
  | `ir-a <id>` | Lleva la vista allí. Solo si el usuario lo pide |

- **Una capa está atada a su escritorio.** Si el usuario cambia de escritorio, las operaciones siguen yendo al de origen. Si el escritorio se borró, responden que ya no existe.
- **Diagrama declarativo:** el agente da nodos, aristas, dirección y grupos, sin coordenadas. El motor calcula un trazado por niveles, pide una zona libre y lo convierte en elementos normales: no queda ningún objeto «diagrama» y el usuario puede tocar cada pieza.
  - Está en Rust para que funcione con la vista Notas oculta;
  - el tamaño de las cajas se estima por el texto, de forma conservadora, porque ahí no se puede medir la fuente.
- **Zonas:** el primer hueco que no toca nada, junto a lo que ya hay. Si el lienzo está vacío, donde el usuario está mirando. Nunca encima de nada.
- **Ordenar cartas:** la interfaz sigue siendo la única que escribe las cartas al ordenar. La orden le pasa los movimientos y ella los aplica, salvo a la carta que el usuario esté arrastrando. Así se cierra para este caso la carrera que hay hoy en ese archivo. Si la vista Notas aún no se abrió, la interfaz carga el archivo, lo cambia y lo guarda.
- **Abrir una nota no cambia:** se sigue haciendo como hoy, y su vista sigue yendo a la carta, porque el usuario lo acaba de pedir.
- **Permisos:** el chat de Nexo arranca con una regla que permite solo la orden `nexo ctl lienzo`, para que no salga una tarjeta de permiso por trazo. No afecta a nada más. Esa orden solo puede escribir en los lienzos del vault, y sigue sin usarse `bypassPermissions` ni `dontAsk`.
- **Skill `nexo-lienzo`:** se instala en las skills del vault con el mismo mecanismo que `nexo-orquestacion` (marca «la mantiene nexo», ruta real del ejecutable; se generaliza para las dos). Si no se puede instalar, el chat arranca igual. Al papel del chat se le añade una línea que manda seguirla. Dice:
  - **cuándo usarla:** planear en visual, diagramar, modelar datos, ordenar el lienzo;
  - **qué elemento sirve para cada pregunta:**
    - componentes → arquitectura;
    - pasos y decisiones → flujo;
    - datos → tablas relacionales;
    - interacción en el tiempo → secuencia;
    - trabajo → Scrum;
    - plazos → línea de tiempo;
    - elegir entre opciones → matriz de decisión;
  - **el método:** leer antes de dibujar (el grafo y, si hace falta código, delegar con `nexo-orquestacion`), crear la capa, pedir una zona, poner marcos con título y rellenar por tandas de 20 o 30 operaciones, para que el avance se vea;
  - **las reglas:**
    - nunca mover la vista sin que se pida;
    - leer `omitidas` y `usuario_cambio` en cada respuesta y no rehacer lo que el usuario cambió;
    - marcar lo supuesto como «por confirmar»;
    - usar `--del-usuario` solo si se pidió;
    - avisar antes de algo grande;
    - terminar la capa y decir en el chat qué hay y dónde.

### La interfaz

- **Barra superior:** «Añadir» y «Capas» a la izquierda de `#desk-map`, solo en Notas. Entran en la lista de TARJETAS de los temas. La barra está ajustada al píxel, así que tiene sus escalones como las demás piezas: botones sueltos en ventanas anchas y un solo botón en las estrechas. Hay que comprobar entre 1024 y 1920 px que nada pisa el selector de vistas (regla de `estilos.md`).
- **Colocar:** el elemento elegido arma el lienzo con cursor de cruz. Un clic pone el tamaño por defecto y arrastrar da el tamaño. Escape cancela. Intro en el menú lo coloca en el centro del hueco libre.
- **Gestos:** el botón derecho mueve el lienzo desde cualquier sitio y arrastrar el fondo también, Mayús y arrastrar selecciona por área. Dentro del lienzo, el menú contextual del navegador queda para Mayús y botón derecho. La rueda no cambia. Tocar un elemento **no** lo trae al frente, a diferencia de las cartas: en un diagrama eso desordenaría lo que se solapa a propósito.
- **Pintado:** un elemento en el DOM dentro de la superficie del lienzo, con la misma lógica de `translate` que las cartas mientras se arrastra. Una flecha es un SVG pequeño de su propio tamaño, nunca un SVG del tamaño del lienzo. **Solo está montado lo que cae cerca de la vista.** Sin `backdrop-filter` ni sombras grandes.
- **Capas en el DOM:** cada capa agrupa sus elementos en un contexto de apilamiento. Las cartas van en la de «Tú». Una capa del agente va por encima de la suya; como las zonas son libres, normalmente no se solapan.
- **Lo que llega del agente** solo crea o cambia sus propios nodos, agrupado por fotograma. Nunca toca la vista, el foco, la selección ni el historial. Entra con opacidad, sin movimiento, y sin nada si se pide menos movimiento.
- **Indicador:** la capa que dibuja se ve en el minimapa con el acento y un marcador que lleva allí. En el marco, «Nexo · RAG del grafo · dibujando». Se anuncia con `role="status"` al empezar y al acabar, no por figura.
- **Deshacer:** una pila del usuario por escritorio, en memoria, con las `inversas`. Si lo que iba a deshacer ya no existe, se salta y lo dice.
- **Módulos sin DOM (nuevos):**
  - **geometría:** anclajes, ruta de las flechas, selección por área, retícula, alinear y repartir;
  - **tabla:** navegación, rangos, pegar y copiar en formato de hoja de cálculo, evaluador de fórmulas;
  - **tablero:** mover tarjetas, cuentas y límites;
  - **secuencia** y **línea de tiempo:** cómo se colocan.

  La interfaz no repite las reglas del motor: envía operaciones y pinta `cambios`. Solo guarda estado propio durante un gesto en curso.
- **Fórmulas:** evaluador propio, sin dependencia nueva, con la lista corta de la historia 30. La celda guarda la fórmula y se calcula al pintar. `leer` le da al agente la fórmula, no el resultado. Los argumentos se separan con `,` o `;`; el decimal en una fórmula es `.`. Errores: `#REF!`, `#DIV/0!`, `#CICLO!`, `#ERROR!`.
- **Texto:** cuadros y figuras se escriben con un campo simple y se pintan con el mismo Markdown saneado que el resto (`renderMarkdown`). No llevan el editor de notas, demasiado pesado para tener uno por figura. El bloque de código usa el coloreado del lector.
- **Texto que llega del agente:** se trata como no fiable. Pasa por DOMPurify, y los enlaces solo abren `http(s)` o rutas que pasen el guardián.
- **Teclado:** los atajos solo actúan con el foco en el lienzo y fuera de un campo. Tab recorre los elementos por su posición y al llegar con el teclado los trae al hueco libre (`reveal`). Las flechas mueven un paso de retícula, Intro edita y Escape sale. Cada elemento tiene su nombre accesible; el de una flecha dice qué une.
- **Se ajusta lo que ya existe:** `boundsOf` y el minimapa cuentan los elementos, `nextSpot` los esquiva, el lienzo vacío menciona «Añadir» y el gestor de escritorios cuenta elementos además de notas. Sin un nodo del DOM por elemento en el minimapa, para que no pese con cientos.

### Temas

- **Tokens por rol** (relleno, línea y texto de cada color), en los doce temas. Los componentes solo usan tokens, como manda `estilos.md`.
- **Tablas, tableros y bloques de código** se pintan como tarjetas y entran en los dos selectores de esa regla, o en los temas de la otra polaridad su texto saldría del color del fondo.

## Decisiones de pruebas

**Qué hace buena una prueba aquí:**
- prueba lo que se ve desde la costura, no cómo está hecho por dentro;
- describe el escenario con palabras del dominio («el usuario movió la caja mientras el agente dibujaba»), sin detalles de la estructura interna;
- los casos dados no se cambian para que pasen: lo que falle se dice.

**Costura 1: la orden de control sobre el motor (usuario).** Es la más alta, y aquí está lo que más importa. `control::handle` ya se prueba con un nexo de mentira; `lienzo` usa el motor real en una carpeta temporal.
- **Convivencia:**
  - el agente cambia algo que el usuario tocó: omitida, con el estado actual;
  - el usuario borra algo y el agente lo une con una flecha: omitida, y el resto del lote se aplica;
  - algo en uso nunca cambia, ni con `--del-usuario`;
  - el agente no puede crear en una capa ajena;
  - dos agentes, dos capas, sin cruzarse;
  - mover una tarjeta o una celda no le impide añadir otras;
  - `usuario_cambio` cuenta lo que pasó desde su última orden;
  - la capa sigue yendo a su escritorio aunque cambie el activo.
- **Deshacer:** las inversas devuelven el lienzo al estado anterior. Las del agente solo aparecen en `ordenar` y `--del-usuario`. Borrar e integrar una capa se deshacen.
- **Zonas y diagrama:**
  - una zona nunca toca nada, tampoco una carta;
  - un diagrama con ciclos, nodos sueltos y grupos no solapa cajas;
  - sale igual con la misma entrada.
- **Datos:**
  - se guarda y se relee igual;
  - un archivo roto no se sobrescribe y queda aparte;
  - el primer escritorio tiene un id estable sin `escritorios.json`;
  - un id de escritorio o un nombre de imagen con `../` se rechaza;
  - los topes salen como `omitidas`;
  - una capa abierta sale como `sin_terminar` al recargar.
- **La skill:** nombra todas las órdenes, lleva la ruta de nexo y solo se reescribe si es suya. Precedentes: `nexo-orquestacion` y sus dos pruebas.

**Costura 2: módulos de la interfaz sin DOM (usuario),** con `node --test`. Precedentes: `tests/desk-layout.test.mjs`, `tests/desks.test.mjs`, `tests/ws-layout.test.mjs`.
- **Geometría:** el anclaje de una flecha sigue al elemento, la ruta en ángulo sale por el lado correcto, selección por área, retícula, alinear y repartir, `nextSpot` esquiva elementos.
- **Tabla:** flechas, Tab e Intro; rangos; pegar y copiar con tabuladores, comillas y saltos de línea tal como los dan Excel y Sheets; fórmulas y sus errores, también el ciclo.
- **Tablero:** mover y reordenar, cuentas de tarjetas y puntos, avisos de límite.
- **Secuencia y línea de tiempo:** cómo se colocan.

**Costura 3: temas (usuario).** Se amplía `tests/themes.test.mjs`: en los doce temas, el texto de cada rol pasa 4,5:1 sobre su relleno y su línea pasa 3:1 sobre el fondo del lienzo. También pasan las tablas y los tableros como tarjetas. Si un tema no llega, se dice cuál y qué da.

**Sin CI, en el banco de WebKitGTK:** lo que depende de pintar (el usuario lo eligió así, sin navegador en el CI):
- colocar, arrastrar, tirar flechas y editar con el Tauri simulado en Chrome;
- que WebKitGTK pinta bien cada elemento y en los doce temas;
- la barra superior de 1024 a 1920 px;
- **rendimiento,** comparado con el lienzo actual: con 300 elementos y 100 flechas, mover el lienzo y arrastrar no bajan de 30 fps; un lote de 50 elementos mientras se arrastra no da un fotograma de más de 50 ms. Las cifras se fijan al medir la base.

**Con Claude Code de verdad** (Haiku, esfuerzo bajo, una carpeta temporal, y limpiando lo que quede en `~/.claude/projects`):
- la regla de permiso deja pasar `nexo ctl lienzo`, también con un lote por la entrada estándar, y no deja pasar otra cosa;
- la skill se dispara con una petición en lenguaje normal.

**Con la app real, lo hace el usuario** (no se abre nexo desde aquí): el caso del RAG de punta a punta, tocando lo que dibuja mientras lo dibuja.

## Fuera del alcance

- **Que los elementos sean notas o entren en la wiki:** es lo contrario de lo pedido. Lo único que cruza es «Convertir en nota».
- **Buscar elementos** desde «Buscar» y leerlos con el servidor MCP.
- **Historial de versiones del lienzo** y commits automáticos.
- **Colaboración con otras personas** o con otras máquinas.
- **Acercar más allá de 0 %:** el zoom sigue siendo solo para alejar, y la rueda no hace zoom.
- **Evitar obstáculos al trazar flechas:** van por una ruta simple. El trazado por niveles del diagrama las deja limpias casi siempre.
- **Agentes de los workspaces:** la skill es solo del chat de Nexo. Un Claude de una terminal no la tiene.
- **Lo que queda para después:** dibujo libre, LaTeX, importar y exportar Mermaid o SQL, exportar un marco a imagen, comentarios para el agente, símbolos de otras disciplinas y el formato JSON Canvas de Obsidian.
- **Quitar el modo actual de abrir notas** editando `.nexo/escritorio.json`.

## Notas

- **Orden de entrega propuesto** (para `/to-tickets` o para implementar por partes). Lo arriesgado va primero:
  1. **Base:** el motor, `nexo ctl lienzo`, capas, «Añadir» y «Capas», texto, figuras, flechas y marcos. Con esto ya se pueden hacer diagramas a mano.
  2. **El agente:** la regla de permiso, la skill, las zonas, el diagrama y `ordenar`.
  3. **Datos y trabajo:** tabla tipo Excel, tablero Scrum y tabla relacional, lo que el usuario pidió por su nombre.
  4. **Lo propuesto:** adhesiva, código, referencia, imagen, secuencia, línea de tiempo y plantillas.
- **Riesgo: rendimiento en WebKitGTK** con cientos de piezas dentro de una superficie enorme escalada con `transform`. Por eso solo se monta lo cercano y las flechas son pequeñas. Se mide en el banco desde la primera entrega, no al final.
- **Riesgo: la barra superior está ajustada al píxel.** Añadir dos piezas obliga a rehacer los escalones por ancho.
- **Riesgo: el tamaño estimado de las cajas** puede quedarse corto con textos largos o en otra fuente. Es conservador y el usuario puede redimensionar. Se revisa con diagramas reales en el banco.
- **Sigue la carrera de la edición a mano de `.nexo/escritorio.json`** para abrir notas, como hoy. No se agrava: dibujar y ordenar ya no pasan por ahí.
- **La carpeta `.nexo/` está en el vault.** Obsidian no enseña las carpetas con punto, así que los lienzos no le ensucian nada.
- **Regla al implementar:** hay que actualizar `notas.md` (elementos, capas, motor, y «orden de apilado» en lugar de «capa»), `estilos.md` (la barra, los roles de color, la lista de TARJETAS), `claude-code.md` (la skill y la regla de permiso), `workspace.md` (la orden de control) y `rendimiento.md` (lo medido), y añadir los archivos nuevos al `paths` de su regla.

## Notas de la implementación (03-11-2026)

**Cambió respecto a lo de arriba:**
- **El lote va como argumento, no por la entrada estándar.** Era el riesgo que se comprobó primero, con turnos reales de Haiku y Claude Code 2.1.308: la regla de permiso no casa con un heredoc, ni con el escape `'\''`, ni con una orden que empiece por una variable. Sí casa con un argumento entre comillas simples, también en varias líneas y con `;`, `&&` o `$` dentro. Por eso un apóstrofo se escribe como `\u0027`. `-` sigue valiendo para quien use la orden a mano, pero pide permiso.
- **`capa-reabrir` no existe:** volver a dibujar en una capa la reabre. Una orden menos.
- **Las capas van por encima de las cartas de notas.** Así una flecha o un cuadro pueden señalar una nota. Las cartas no se ocultan con la capa «Tú».
- **«Nunca dibuja encima de lo tuyo» lo garantiza el motor, no la skill.** Un turno real de Haiku no pidió `zona` y dio coordenadas a mano. Un lote que pisaría algo ajeno se mueve entero a un hueco y la respuesta trae `desplazado`; mover lo suyo encima de algo se omite.
- **Lo que se está escribiendo o arrastrando dentro de una tabla o un tablero no se repinta hasta acabar,** y el foco vuelve a donde estaba. Salió probándolo: si no, una fila del agente borraba el campo en el que se escribía.
- **Elementos en escritorios no activos:** el recuento se pide al abrir el gestor (`canvas_counts`).

**Comprobado:**
- `cargo test -p nexo-core`: 42 de la lógica, más las 30 de `tests/lienzo.rs`, y `clippy -D warnings`. `cargo test --no-default-features`: 131. `npm test`: 141, con el contraste de los seis roles en los doce temas.
- Banco en Chrome con el motor de verdad detrás (no un simulacro): colocar con ratón y con teclado, escribir, tirar y reenganchar flechas, deshacer y rehacer, fórmulas, pegar de Excel, rangos, tablero con arrastre y con Ctrl+flechas, relacional, secuencia, código con color, línea de tiempo, referencia, imagen, plantillas, marcos con cartas dentro, selección por área, capas (ocultar, integrar, borrar y Ctrl+Z), escritorios, y escribir en una celda mientras el agente añade una fila a la misma tabla.
- WebKitGTK sin ventana: pinta todos los elementos y tiene `color-mix`. Las cifras de rendimiento están en `rendimiento.md`.
- La barra superior, de 1024 a 1920 px y con un vault de nombre largo: nada pisa el selector de vistas y este sigue centrado.
- Un turno real de Haiku, con la skill instalada y la regla de permiso, contra el motor de verdad: con una petición en lenguaje normal creó su capa, dibujó el diagrama, una tabla relacional y un tablero, y la terminó. Tropezó una vez queriendo guardar la ruta en una variable, y la skill ya lo avisa.

**Sin comprobar, porque necesita abrir nexo:** el caso del RAG de punta a punta desde el chat de verdad, el socket real de `nexo ctl lienzo`, `--allowedTools` con la ruta instalada del ejecutable, los diálogos nativos, y el rendimiento con GPU.

**El paquete principal de la interfaz** pasa de 414 a unos 510 kB, porque el lienzo va dentro. Si pesa al arrancar, se puede cargar al entrar en Notas, como xterm.

## Arreglos tras probarla (05-11-2026)

Lo que el usuario encontró usando la app, y qué se hizo:
- **Las tarjetas del tablero no se podían confirmar:** al pulsar Intro sin cambiar nada se quedaban en modo edición. Pasaba también con los títulos de las columnas de tablas y tableros, que se quedaban como texto editable. Toda edición repinta ahora al acabar. Además, el tablero gana: «Guardar» y «Borrar» en la tarjeta, varias tareas seguidas con Intro, el menú «⋯» de cada columna (renombrar, límite, mover, borrar) y el título editable. Las tablas también tienen título editable.
- **«La app se selecciona entera, con un margen gigante»:** era el borde de foco del lienzo, que WebKitGTK enseñaba alrededor de toda la ventana tras un clic en el fondo. Ahora solo sale si se llega con el teclado. También faltaba `-webkit-user-select`: WebKitGTK no entiende `user-select` sin prefijo.
- **«Enviar al fondo» no funcionaba bien:** entre elementos sí funcionaba, pero los elementos iban siempre por encima de las notas. Ahora lo enviado al fondo y los marcos van detrás de las notas.
- **Los puntos que se quedan al borrar o agrandar algo:** no se reproduce sin GPU. Se fuerza el repintado de esa zona, pero falta verlo en la app.
- **Minimizar una nota desde la carta izquierda:** con un botón en su fila (o en su cabecera), la carta se queda plegada en su sitio.
