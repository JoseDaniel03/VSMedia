# VSPack — API de fusión e interfaces v1

El motor Rust vsmedia-pack y el bridge Axum de VSMedia 26.3 ya ofrecen un
contrato independiente de la interfaz actual, preparado para una nueva web
que podrá explorar interfaces Minecraft y obtener packs.

## Cómo se fusionan texturas de diferentes Resource Packs

- Rutas distintas conviven en el ZIP final, sin edición de píxeles.
- Para una ruta repetida, gana el pack de mayor prioridad.
- Cuando dos packs de máxima prioridad aportan diferentes bytes y tienen
  igual prioridad, la composición queda bloqueada hasta elegir una fuente.
- Los empates entre packs de prioridad inferior a un ganador superior
  no bloquean la composición. El resultado no depende del orden de carga.
- La futura interfaz puede elegir qué pack aporta CADA archivo de assets/.
  Es una elección de fuente, no una mezcla de canales RGBA.
- Fuentes, atlas, modelos y archivos JSON desconocidos conservan el
  comportamiento de prioridad por archivo. No se fusionan esquemas
  arbitrarios porque esto podría romper el formato de Minecraft.
- sounds.json se fusiona por EVENTO de sonido: distintos eventos se
  preservan, conflictos se resuelven por prioridad del evento y también
  se ignoran empates inferiores que están eclipsados por un ganador.

Una textura y su posible archivo acompañante .png.mcmeta pueden necesitar
ser elegidos conjuntamente por el editor futuro para mantener animaciones.
No se promete todavía un editor de atlas ni de modelado 3D.

## Catálogo y operaciones de los recursos

Prefijo HTTP: /api/v1

GET    /packs
       Lista packs importados: ID, nombre, prioridad, archivos, bytes.
       Incluye schema=vspack-catalog-v1 y recursos soportados.

POST   /packs/{id}
       Importa un ZIP seguro como contenido binario, con X-VSPack-Label.

POST   /packs/{id}/priority
       Cuerpo JSON: {"priority":20}

DELETE /packs/{id}
       Quita el pack de la sesión y limpia las elecciones asociadas.

GET    /packs/{id}
       Metadatos de un pack importado.

GET    /packs/{id}/download
       Descarga ZIP canónico de la fuente importada (puede diferir en bytes
       del ZIP de origen: conserva los recursos pero normaliza compresión y
       timestamps; requiere pack.mcmeta válido).

GET    /packs/merge/plan
GET    /packs/resources
       Inventario paginado incluso si la exportación se encuentra bloqueada
       por un empate. Schema: vspack-resources-v1.

GET    /packs/interfaces
       Inventario paginado de recursos relacionados con GUI/HUD/fuentes.

GET    /packs/merge/overrides
       Elecciones manuales activas para cada ruta.

POST   /packs/merge/overrides
       Cuerpo JSON:
       {"path":"assets/virgoland/textures/gui/boton.png",
        "source_id":"pack_ui"}

DELETE /packs/merge/overrides?path=assets%2Fvirgoland%2Ftextures%2Fgui%2Fboton.png
       Quita la elección de fuente y vuelve a la prioridad automática.

GET    /packs/resources/file?path=assets%2Fvirgoland%2Ffont%2Fdefault.json
       Devuelve bytes del recurso final con tipo application/octet-stream,
       Content-Disposition: attachment y X-Content-Type-Options: nosniff.

GET    /packs/texture?path=assets%2Fvirgoland%2Ftextures%2Fgui%2Fboton.png
       Vista previa en navegador de PNG, JPEG o WebP validado.

GET    /packs/export
       Compone ZIP completo (no modifica ni publica packs originales).

POST   /packs/layers/export
       Exporta bundle con ZIPs base+overlays por prefijos, UUID y hashes.

## Parámetros de búsqueda y paginación

GET /api/v1/packs/interfaces?namespace=virgoland&search=gui&limit=50&offset=0

Para /resources y /merge/plan se admiten:

- scope: all (predeterminado), textures o interfaces
- namespace: nombre exacto del espacio de nombres en assets/
- search: búsqueda de subcadena en la ruta sin sensibilidad de mayúsculas
- conflicts_only=true: muestra sólo archivos con contenidos divergentes
- limit: 1..200; offset: 0..16384

La respuesta contiene schema, scope, total, offset, limit, conflicts,
blocked, overrides, session_only y una lista items. Cada item indica:
path, namespace, category, interface_kind, chosen_source,
chosen_priority, chosen_sha256, bytes, conflict, blocked,
override_source y candidates[].

Los candidatos muestran source_id, source_name, priority, bytes y sha256.
Cuando blocked=true no hay ganador resuelto y debe seleccionarse una
fuente antes de exportar. La elección manual afecta sólo la sesión actual.

## Grupos de recursos para la próxima web de interfaces

- gui_textures: textures/gui, textures/ui y textures/hud
- fonts: font/ y textures/font/
- atlases: atlases/
- item_models: models/item/
- translations: lang/
- ui_sounds: sounds/ui/

Son categorías de exploración, no funcionalidades simuladas de
renderizado de HUD o de edición de interfaces. El nuevo frontend puede
consultarlas mediante JSON y recuperar los recursos del pack resultante.

## Límites, seguridad e integración pendiente

- Máximo 6 packs importados por sesión.
- ZIP de hasta 12 MiB comprimidos, 32 MiB descomprimidos por pack.
- Presupuesto de 64 MiB de bytes de recursos en la sesión.
- Hasta 200 recursos por página y 256 elecciones manuales.
- Validación de rutas, tamaño de archivos, ZIP malicioso y sin symlinks.
- Operaciones pesadas en tareas Rust bloqueantes separadas.
- Lectura binaria con nosniff; la vista previa sólo admite imágenes con
  firmas correctas y hasta 4 MiB.
- Escrituras limitadas a Origins locales permitidos.
- La sesión se guarda en RAM, no se expone a Internet.
- Los ZIP de fuente se recompilan de modo reproducible; no se promete
  devolver los bytes originales del ZIP importado.
- Los recursos no se publican automáticamente a VSEngine, CDN o Minecraft.
- La distribución HTTPS y la persistencia con CAS siguen siendo trabajo
  de integración y requieren autenticación antes de alojar una web remota.

Tests en Rust/CI verifican selección de prioridades entre tres packs,
resolución manual de empates, indexación por namespace, paginación de
interfaces, lectura binaria, filtrado de rutas inválidas, exportación ZIP
y preservación de eventos distintos de sounds.json.

La futura web puede conectarse a esta API mediante un bridge autenticado
cuando se implemente. Hasta recibir ese frontend, el gestor actual de
FullCanvas permite resolver conflictos con selectores de fuente.

La nueva ruta POST /api/v1/packs/merge/texture-bundle selecciona un PNG y, si está presente, su .png.mcmeta del mismo source_id. GET /api/v1/packs/diagnostics informa dependencias y metadata huérfana.
