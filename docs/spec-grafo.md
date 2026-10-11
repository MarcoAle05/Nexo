# Especificación del grafo de la wiki

Esta es la fuente de verdad de cómo `graph::build_graph` convierte `wiki/` en nodos y aristas.
Si el código y este documento discrepan, uno de los dos tiene un error: se decide cuál y se arregla
junto con el caso dorado correspondiente (ver «Cómo cambiar una regla»).

- **Entrada:** la raíz del vault (`root`); solo se lee `root/wiki/**` y `root/wiki/fuentes/*.md` (fichas).
- **Salida:** `Graph { nodes, edges }`, la misma estructura que recibe la interfaz (`read_graph`).
- **Casos dorados:** `src-tauri/tests/fixtures/grafo/<caso>/{vault/, expected.json}`. El test
  `el_grafo_cumple_los_casos_dorados` (en `graph.rs`) construye cada `vault/` y compara el resultado
  con `expected.json` como conjuntos de nodos y aristas. `el_orden_de_los_nodos_es_estable` comprueba G14.

Las reglas marcadas **(D1)** a **(D5)** son decisiones que cambian el comportamiento anterior
al documento; el resto describe lo que el código ya hacía y ahora queda fijado por un caso.
Hay 17 casos en total.

## Reglas

### G1. Inventario de notas
Hay un nodo por cada archivo con extensión `.md` (sin distinguir mayúsculas: `.MD` cuenta) dentro de
`wiki/`, en cualquier profundidad. Se ignoran los archivos y carpetas cuyo nombre empieza por `.`
(`.obsidian`, `.git`, `.oculta.md`) y cualquier archivo con otra extensión.
Casos: `ocultos_y_extensiones`, `basico`.

### G2. Id y grupo
- `id`: ruta relativa a `wiki/`, con `/` como separador y las mayúsculas originales (`Mis temas/Idea.md`).
- `group`: la carpeta de primer nivel del `id`; cadena vacía si la nota está en la raíz de `wiki/`.

Casos: `basico`, `tipos`, `unicode_y_espacios`.

### G3. Sintaxis de enlaces
Cuentan estos dos tipos de enlace:
- `[[destino]]`, también con `|alias`, `#encabezado` o `^bloque` (se toma lo anterior al primero de esos signos, sin espacios sobrantes).
- `[texto](destino.md)`, con `#ancla` opcional; `%20` equivale a un espacio.

No cuentan: destinos vacíos, enlaces con `://` y enlaces markdown cuyo destino no termina en `.md`
(imágenes, PDF…). Un prefijo `./` se ignora al resolver.
**(D4)** No cuentan tampoco los wikilinks cuyo destino contiene `://` ni los que apuntan a adjuntos
(extensiones, sin distinguir mayúsculas: `png`, `jpg`, `jpeg`, `gif`, `svg`, `webp`, `bmp`, `pdf`, `mp3`, `wav`,
`ogg`, `m4a`, `mp4`, `webm`, `mov`, `mkv`), sean o no incrustados (`![[...]]`). Un destino con otra extensión
(`[[Node.js]]`) sigue siendo un enlace.
Casos: `basico`, `codigo_y_externos`, `enlaces_no_notas`.

### G4. Zonas donde los enlaces no cuentan
- Bloques de código cercados: desde una línea que, tras los espacios iniciales, empieza por ```` ``` ```` hasta la siguiente línea igual.
- **(D2)** Código en línea: lo que queda entre dos acentos graves (`` `[[ejemplo]]` ``). Un acento grave sin pareja es texto normal.

Casos: `codigo_y_externos`, `codigo_en_linea`.

### G5. Resolución de un enlace a una nota
Se quita `.md`, se pasa a minúsculas y se busca, en este orden:
1. Ruta completa desde `wiki/` (`temas/rag`).
2. Ruta relativa a la carpeta de la nota que enlaza (`sub/deep` desde `temas/rag.md`).
3. Nombre de la nota (último segmento) en cualquier carpeta (`chunking`).

`..` no se interpreta: `../index.md` se resuelve por su nombre (`index`), paso 3.
Casos: `resolucion_ruta`, `unicode_y_espacios`.

### G6. Nombres repetidos
Cuando el paso 3 de G5 encuentra varias notas con el mismo nombre:
1. Gana la primera cuya ruta está en la carpeta de la nota que enlaza **o dentro de ella** (subcarpetas incluidas). Una nota de la raíz no tiene carpeta, así que no aplica este criterio.
2. Si ninguna cumple, gana la primera **en orden de id** (orden de bytes de la cadena). **(D1)**

Casos: `duplicados_misma_carpeta`, `duplicados_ambiguos`.

### G7. Enlaces rotos (fantasmas)
Un enlace sin destino crea un nodo fantasma con `id = "?" + destino` (el destino tal como se escribió, sin espacios sobrantes y con sus mayúsculas), `label = destino`, `group = ""`, `kind = "missing"`, y una arista desde la nota. Un mismo texto de destino genera un solo fantasma aunque lo citen varias notas.
Casos: `fantasmas`, `ocultos_y_extensiones` (enlazar a algo que está en una carpeta oculta es un enlace roto).

### G8. Autoenlaces
Un enlace de una nota a sí misma no crea arista (ni fantasma).
Caso: `autoenlaces_y_duplicados`.

### G9. Aristas
Las aristas no tienen dirección: se guardan como par `[a, b]` con `a < b` (orden de bytes), sin repetidos. Que A enlace a B, que B enlace a A o que ambas se enlacen produce una sola arista. Todo enlace que se resuelve a una nota distinta de sí misma, o a un fantasma, produce arista.
Casos: `basico`, `autoenlaces_y_duplicados`.

### G10. Fichas de fuentes
Una **ficha** es un archivo `wiki/fuentes/<nombre>.md` directamente dentro de `fuentes/` (sin subcarpetas), con extensión `.md` en minúsculas, cuyo nombre no empieza por `_` ni `.`, y cuyo frontmatter contiene la línea `source: raw/<ruta>`. (Es lo que devuelve `fichas::all`.)
Además de los enlaces normales, hay una arista entre una ficha y **toda otra nota cuyo texto contenga la cadena `raw/<ruta>`**, esté donde esté en el texto (también dentro de bloques de código o del frontmatter).
Casos: `fichas`, `fichas_borde`.

### G11. Tipo de nodo (`kind`)
Se aplica el primero que cumpla:
1. `source`: la nota es una ficha (G10); el nodo lleva además `source = <ruta>` sin el prefijo `raw/`.
2. `conversation`: el id empieza por `conversaciones/` y el **nombre del archivo** no es `_index.md`.
3. `index`: el **nombre del archivo** es exactamente `_index.md` o `_master-index.md`. **(D3)**
4. `note`: el resto. Los fantasmas son `missing` (G7).

**(D3)** Antes se miraba si el id *terminaba* en `_index.md`, así que `notas/mi_index.md` salía como `index` y `conversaciones/mi_index.md` no salía como `conversation`.
Casos: `tipos`, `fichas`.

### G12. Etiqueta (`label`)
Se aplica la primera que exista:
1. Ficha: nombre del archivo de la fuente sin extensión (`docs/informe.pdf` → `informe`). Tiene prioridad sobre `title`.
2. `title:` del frontmatter, si no está vacío.
3. Por nombre de archivo: `_master-index` → `Índice maestro`; `_index` → nombre de su carpeta (`Índice` si está en la raíz); cualquier otro → el nombre sin `.md`.

Casos: `etiquetas`, `fichas`, `frontmatter`.

### G13. Frontmatter
Hay frontmatter solo si el texto empieza por `---` + salto de línea (LF o CRLF) y existe más adelante un cierre (`\n---`). Sin cierre, se trata la nota como si no tuviera frontmatter.
Cada línea `clave: valor` se separa en el primer `:`; el valor se recorta y se le quitan las comillas dobles de los extremos. Se descartan las líneas sin `:` y las líneas que empiezan por espacio, tabulador, `-` o `#` (elementos de lista, claves anidadas, comentarios). **(D5)**
Casos: `frontmatter`, `etiquetas`, `frontmatter_anidado`.

### G14. Orden de salida **(D1)**
El resultado no depende del sistema de archivos. Los nodos van primero las notas ordenadas por `id` (orden de bytes) y después los fantasmas ordenados por `id`. Las aristas van ordenadas.
Test: `el_orden_de_los_nodos_es_estable` (recorre todos los casos).

## Mapa regla → caso

| Caso | Reglas |
|---|---|
| `basico` | G1, G2, G3, G9 |
| `codigo_y_externos` | G3, G4 |
| `codigo_en_linea` | G4 (D2) |
| `resolucion_ruta` | G5 |
| `duplicados_misma_carpeta` | G6.1 |
| `duplicados_ambiguos` | G6.2, G14 (D1) |
| `fantasmas` | G7 |
| `autoenlaces_y_duplicados` | G8, G9 |
| `fichas` | G10, G11, G12 |
| `fichas_borde` | G10 |
| `tipos` | G11 (D3) |
| `etiquetas` | G12, G13 |
| `frontmatter` | G13 |
| `ocultos_y_extensiones` | G1, G7 |
| `unicode_y_espacios` | G3, G5 |
| `enlaces_no_notas` | G3 (D4) |
| `frontmatter_anidado` | G13 (D5) |

## Qué falla antes de aplicar D1–D5

Comprobado ejecutando la lógica de `graph.rs` tal cual estaba en `main` (commit `8fe789d`) sobre los 15 casos de
la primera ronda, y con D1–D3 ya aplicadas (commit `ab5b962`) sobre los dos de la segunda:

| Caso | Fallo | Decisión |
|---|---|---|
| `codigo_en_linea` | 4 fantasmas falsos (`?ejemplo`, `?a`, `?b`, `?y.md`) | D2 |
| `tipos` | `notas/mi_index.md` sale `index`; `conversaciones/mi_index.md` sale `index` | D3 |
| `duplicados_ambiguos` | enlaza a `b/tema.md`; según el sistema de archivos podría ser `a/` o `c/` | D1 |
| `el_orden_de_los_nodos_es_estable` | los nodos salen en el orden de `read_dir` | D1 |
| `enlaces_no_notas` | 3 fantasmas falsos (`?https://ejemplo.com`, `?foto.png`, `?informe.PDF`) | D4 |
| `frontmatter_anidado` | `anidada.md` sale con la etiqueta `Interno`; `mixta.md` sale con `Sangrado` | D5 |

De los 17 casos, 5 fallaban antes de su decisión: 3 de los 15 de la primera ronda y los 2 de la segunda.
Los otros 12 describen comportamiento que ya era correcto y pasan sin tocar nada.

## Fuera de alcance (comportamiento actual, sin caso)

No se prueba ni se garantiza todavía; si algún día importa, se escribe primero el caso:
- Bloques cercados con `~~~` y código en línea con varios acentos (` ``a`` `).
- `title: 'comillas simples'` (solo se quitan las dobles).
- Fantasmas que solo difieren en mayúsculas (`[[Falta]]` y `[[falta]]` son dos nodos).
- Normalización Unicode (NFC/NFD) en nombres de archivo, típica de macOS.
- Duplicados donde ninguna candidata está en la carpeta de la nota pero sí en una carpeta ancestro (hoy gana la primera por id, no la ancestro más cercana).
- G10 usa coincidencia por subcadena: `raw/a.pdf` también coincide con `raw/a.pdf.bak`.
- Dos fichas que declaran la misma `source`: gana una según el orden del sistema de archivos.
- Enlaces markdown a destinos como `mailto:a@b.md` (no contienen `://`).

## Cómo cambiar una regla

1. Edita la regla en este documento.
2. Cambia o añade el caso: carpeta nueva en `tests/fixtures/grafo/<nombre>/` con `vault/wiki/...` y un `expected.json` escrito **a mano** a partir de la regla (no copiando la salida del programa).
3. Ejecuta `cargo test graph` y comprueba que falla por el motivo esperado.
4. Cambia el código hasta que pase. Un commit por regla.

Formato de `expected.json`: `{"nodes": [{id, label, group, kind, source?}], "edges": [[a, b]]}`; `source` solo en las fichas. El orden dentro de cada lista da igual para este test.

Los fixtures se leen byte a byte (`crlf.md` usa CRLF): `tests/fixtures/grafo/.gitattributes` desactiva la conversión de saltos de línea de git.
