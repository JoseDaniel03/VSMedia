# VSPack Studio — contrato backend para el futuro gestor

El backend preparado se ejecuta sobre Rust/Axum, usando el motor
vsmedia-pack. No implementa una segunda web, otro mod ni un plugin.

## Flujo de la futura interfaz

1. Importar fuentes con POST /api/v1/packs/{id}.
2. Consultar GET /api/v1/packs/manager/capabilities para conocer límites.
3. Consultar GET /api/v1/packs/manager/recipe para obtener una receta inicial.
4. Guardar/editar esa receta JSON en la nueva interfaz.
5. Consultar POST /api/v1/packs/manager/plan para analizar una selección.
6. Descargar el pack combinado con POST /api/v1/packs/manager/export.
7. Opcionalmente solicitar split/plan y split/export para separar por categorías.

Los cuatro POST del manager aceptan el MISMO cuerpo JSON.

## Ejemplo JSON de receta

~~~json
{
  "schema": "vspack-recipe-v1",
  "name": "Virgoland 26.3 - UI",
  "sources": [
    {"fingerprint": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "enabled": true, "priority": 50},
    {"fingerprint": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "enabled": true, "priority": 20}
  ],
  "choices": {
    "assets/minecraft/textures/gui/sprites/tooltip/background.png": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  },
  "exclude": []
}
~~~

Las huellas del ejemplo son ilustrativas. En la práctica, fingerprint
contiene el SHA-256 del archivo ZIP original que se importó. Si se
reimporta EXACTAMENTE el mismo ZIP, aunque cambie el ID de sesión,
su receta puede recuperarse. Reempaquetar el ZIP modifica esa huella.

choices asigna un archivo individual a una fuente concreta; así
podemos seleccionar los tooltips de MandalasGUI y conservar el resto
de la GUI de CozyUI, sin editar píxeles ni romper los archivos fuente.
Si la textura tiene un PNG.mcmeta acompañante, la receta permite
seleccionar también ese archivo.

## Rutas HTTP locales

| Método | Ruta | Propósito |
|---|---|---|
| GET | /api/v1/packs/manager/capabilities | Capacidades y categorías |
| GET | /api/v1/packs/manager/recipe | Receta portable de la sesión |
| POST | /api/v1/packs/manager/plan | Analiza la fusión de una receta |
| POST | /api/v1/packs/manager/export | Descarga Resource Pack ZIP unificado |
| POST | /api/v1/packs/manager/split/plan | Vista previa de separación por categoría |
| POST | /api/v1/packs/manager/split/export | Bundle ZIP con subpacks y hashes |

Para listar archivos/íconos, consultar candidatos de conflicto,
inspeccionar texturas y obtener recursos individuales, se conservan
las rutas anteriores documentadas en VSPACK-INTERFACES-API.md.

## División por categorías

- interface: GUI, HUD, tooltips, fuentes y atlas.
- items: texturas, modelos y definiciones de ítems/equipamiento.
- blocks: texturas/modelos/estados de bloques.
- audio: eventos y sonidos.
- particles: partículas.
- entities: recursos de entidades.
- languages: traducciones.
- other: metadatos y recursos auxiliares.

Cada ZIP interno generado tiene pack.mcmeta. Los archivos
PNG.mcmeta se mantienen junto a su textura por prefijo.
El bundle general también contiene manifest.json con SHA-1, SHA-256,
UUID y cantidad de archivos de cada subpack.

**Importante:** las referencias JSON entre categorías no se reescriben
automáticamente. Para que una interfaz funcione, puede necesitar sus
fuentes, atlas, modelos y texturas. Los subpacks se deben probar juntos
antes de publicarlos. El ZIP bundle no es un único pack instalable;
cada ZIP interno es un Resource Pack independiente.

## Límites y seguridad

- Hasta 24 packs por sesión y 96 MiB por ZIP comprimido.
- Hasta 12.000 entradas y 192 MiB descomprimidos por ZIP importado.
- 32 MiB por recurso y 384 MiB de fuentes descomprimidas en memoria.
- Hasta 65.536 contribuciones al merge estructurado.
- Recetas de máximo 1 MiB, hasta 512 elecciones y 8.192 exclusiones.
- ZIP con protección de path traversal, duplicados, enlaces simbólicos
  y tamaño descomprimido. Se admiten archivos README/License,
  RespackOpts JSON5 y miniaturas de raíz con extensiones limitadas.
- Todas las operaciones de fusión/exportación permanecen en Rust; no
  se transfieren archivos a una web de terceros.
- El bridge sigue limitado a localhost (127.0.0.1:3061).
  Los POST verifican que el Origin sea el Studio local permitido.

**Persistencia:** una receta JSON puede guardarse y volver a cargar,
pero el servidor NO conserva automáticamente los ZIP al reiniciar.
El servidor guarda proyectos de recetas localmente, pero para un
catálogo persistente de ZIP compartidos hacen falta CAS, autenticación y
almacenamiento seguro.

**Minecraft:** este backend NO instala automáticamente paquetes en
VSClient ni publica a VSEngine/CanvasMC. Sigue pendiente una capa
de publicación HTTPS y autorización. Antes de redistribuir packs
de terceros deben revisarse sus licencias.

La interfaz web nueva queda para cuando llegue su código.

## Fusión semántica de traducciones

Los documentos assets/<namespace>/lang/*.json se fusionan por
clave de traducción: el valor de mayor prioridad prevalece, pero se
conservan las claves diferentes de todos los packs. El importador
normaliza comentarios // y /* ... */ en esos archivos, incluidos
los comentarios de CozyUI, sin eliminar secuencias // dentro de strings.
Los empates de máxima prioridad que discrepan se rechazan como conflicto.
Esto NO implica modificar archivos JSON de estructuras desconocidas.

## Aviso sobre fuentes y licencias

Los proveedores de fonts/default.json no se concatenan automáticamente
sin revisar colisiones de glifos. La web dispondrá de elección de
fuente ganadora por archivo y de las rutas/candidatos para facilitar
una estrategia específica de fusión de fuentes cuando se integre.
Las licencias de terceros deben respetarse antes de redistribuir
packs fusionados desde el servidor.

## Diagnósticos de dependencias del pack unificado

La API local GET /api/v1/packs/diagnostics devuelve un informe
versionado "vspack-inspection-v1" con referencias conocidas en
modelos y proveedores bitmap de fuentes, archivos .png.mcmeta
sin textura, y referencias cruzadas entre grupos del separador.

POST /api/v1/packs/manager/plan incluye un campo inspection
sin cambiar los campos anteriores de la respuesta. El endpoint
POST /api/v1/packs/manager/split/plan incluye dependencies y sus
ejemplos de enlaces entre categorías que podrían requerir activar
varios subpacks juntos. Los informes están acotados a 300 avisos y
40.000 referencias; "truncated" identifica límites alcanzados.

Las referencias ausentes son avisos informativos, NO errores de
compilación: una textura/modelo puede ser suministrado por Minecraft
Vanilla u otro mod que no figure en el ZIP.

## Selección conjunta para tooltip y animaciones

POST /api/v1/packs/merge/texture-bundle
Content-Type: application/json

{"path":"assets/minecraft/textures/gui/sprites/tooltip/background.png",
 "source_id":"mandala"}

La operación elige simultáneamente una textura PNG y su fichero
PNG.mcmeta de la misma fuente, cuando existe. Si el pack no aporta
metadata de animación, se revoca la elección manual previa de ese
fichero y se informa needs_metadata_review=true. Esto evita que
un selector visual deje una animación elegida de otro pack.
La operación NO combina píxeles automáticamente.

## Control de tareas intensivas

Un Semaphore compartido limita a dos los trabajos de CPU de
importación y fusión activos simultáneamente. No se aumenta la
cuota de fuentes descomprimidas ni se exponen endpoints públicos.
La publicación en VSEngine y almacenamiento persistente todavía
requieren sus adaptadores y seguridad propios.

## Análisis preventivo de fuentes

GET /api/v1/packs/fonts/diagnostics devuelve un reporte
"vspack-font-review-v1" con:
- total_providers: proveedores declarados en fuentes de todos los packs.
- shared_codepoints: caracteres Unicode explícitos definidos en más de
  una fuente del mismo path en packs distintos.
- sample: hasta 300 ejemplos de Unicode (como U+E000), font y source IDs.
- opaque_providers: proveedores TTF, unihex, referencias u otros tipos
  cuyo inventario de glifos no puede determinarse sólo leyendo JSON.
- warnings/truncated: problemas y límites del diagnóstico.

El detector analiza proveedores bitmap y space con caracteres explícitos.
No asume que esos glifos causen un error real en Minecraft: identifica
conflictos potenciales antes de unir CozyUI, Enchant Icons y Redstone Tweaks.
El motor aún NO concatena automáticamente fuentes con proveedores TTF o
referencia; se conserva la resolución por prioridad de archivo para evitar
romper íconos.

El GET de diagnóstico es de solo lectura y no modifica ni exporta
Resource Packs. La futura web podrá usar estos resultados para ofrecer
opciones de resolución de glifos.

## Proyectos de fusión guardados en disco

El backend local conserva ahora **las recetas JSON**, incluso después de
reiniciar el bridge, con un máximo de 32 proyectos. Los archivos ZIP
originales **no se guardan todavía**: es necesario reimportarlos después
de reiniciar, y la recuperación muestra las huellas SHA-256 que faltan.

La ubicación predeterminada es:
\`./.vsmedia-data/vspack-projects/\`

Si se define la variable \`VSMEDIA_DATA_DIR\`, se usa:
\`$VSMEDIA_DATA_DIR/vspack-projects/\`

No se aceptan IDs con rutas, barras ni puntos; sólo letras minúsculas,
números, guiones y guiones bajos (máximo 48 caracteres).
Las escrituras usan un archivo temporal sincronizado y una sustitución
por renombrado para evitar truncar el proyecto existente. Se rechazan
enlaces simbólicos y los documentos guardados se validan al volver a leer.

| Método | Endpoint | Función |
|---|---|---|
| GET | \`/api/v1/packs/manager/projects\` | Lista proyectos guardados con fecha y revisión |
| GET | \`/api/v1/packs/manager/projects/{id}\` | Receta, revisión y \`missing_sources\` |
| POST | \`/api/v1/packs/manager/projects/{id}\` | Crear o modificar un proyecto |
| DELETE | \`/api/v1/packs/manager/projects/{id}?revision=...\` | Eliminar si la revisión coincide |

Crear un proyecto:

\`\`\`http
POST /api/v1/packs/manager/projects/virgoland-ui
Content-Type: application/json

{"recipe":{"schema":"vspack-recipe-v1","name":"Virgoland UI",
"sources":[{"fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
"enabled":true,"priority":20}],"choices":{},"exclude":[]}}
\`\`\`

Al crear, \`expected_revision\` debe omitirse o ser \`null\`.
La respuesta devuelve un \`revision\` SHA-256. Para actualizar se envía
el mismo JSON con \`expected_revision\` igual a la revisión leída; si
otra pestaña cambió el proyecto, el servidor devuelve HTTP **409**
en lugar de sobrescribirlo. El borrado también requiere la revisión
en el parámetro \`revision\`.

Guardar o recuperar un proyecto no ejecuta automáticamente una fusión
ni modifica packs importados. La web futura deberá mostrar la lista de
fuentes faltantes y permitir reimportarlas.

### Hashes de los ZIP compilados

Las exportaciones del manager (\`/manager/export\` y
\`/manager/split/export\`) incluyen \`ETag\`, \`X-VSPack-SHA1\` y
\`X-VSPack-SHA256\`. Son hashes de los **bytes del ZIP exportado**,
no de los ZIP originales utilizados por la receta. Se exponen mediante
CORS al Studio local para facilitar comprobaciones de integridad y
futuras políticas de caché, aunque no se está publicando contenido en
una CDN ni enviándolo a VSClient.

Esta persistencia es **monousuario y local**, sin autenticación remota
ni almacenamiento de fuentes CAS: no se debe exponer el bridge a
Internet hasta implementar una capa autenticada y permisos.
