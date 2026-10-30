# VSPack 26.3 — Gestor de Resource Packs

## Funcionalidad entregada

La pestaña **VSPack** de FullCanvas (Rust/Leptos + JS ligero)
utiliza **el motor Rust `vsmedia-pack` existente**. No hay otro
motor ZIP independiente ni se modifica VSMix, VSRadio o VSStreams.

Desde VSPack se puede:

1. Importar múltiples archivos `.zip` locales.
2. Inspeccionar la lista de fuentes, archivos, tamaño y prioridad.
3. Modificar prioridades por pack (-1000 a 1000); el número mayor
   prevalece al haber un conflicto.
4. Analizar automáticamente recursos unidos, categorías (texturas,
   modelos, sonidos, fuentes, idiomas, shaders, manifiesto y otros),
   namespaces y conflictos con la fuente ganadora/perdedora.
5. Filtrar rutas por texto y categoría, recorrer resultados paginados
   de 80 por pantalla sin montar miles de nodos DOM simultáneamente.
   Previsualizar **una textura PNG, JPG o WebP a demanda** mediante un
   endpoint de firma binaria validada, sin cargar todas las imágenes a
   la vez ni habilitar SVG/HTML activos.
6. Quitar fuentes, recalcular la composición y descargar el ZIP final.
7. Fusionar `sounds.json` por **nombre de evento**: los eventos distintos
   conviven sin sobrescribir el archivo completo. Un conflicto de evento
   se resuelve según la prioridad. El empate con contenidos distintos
   es un error explícito que impide exportar.
8. Generar ZIP determinista: rutas ordenadas, timestamps estables,
   almacenar medios ya comprimidos (PNG/OGG/MP3/WebP) y DEFLATE
   para JSON y otros archivos. El manifiesto `pack.mcmeta` debe
   ser un JSON con objeto `pack`.
9. **Exportar base + overlays** con hasta cinco reglas `id=prefijo/`,
   reutilizando `vsmedia-pack::layers` para separar recursos de
   cambio frecuente. La salida **no es un pack instalable**: es un
   ZIP de transporte que contiene `manifest.json`, `packs/base.zip`
   y `packs/overlay.zip`, cada ZIP interno compatible con
   Resource Packs de Minecraft y con su hash SHA-1/SHA-256 y UUID
   estable. No publica ni asigna automáticamente las capas a jugadores.

**No admite datapacks**: la lista de entradas permitidas está
limitada a rutas `assets/...`, `pack.mcmeta` y `pack.png`.

## Ejecutar y probar

En CachyOS / Linux, desde `studio/`:

```bash
./start.sh
```

Abrir **http://127.0.0.1:3060** y seleccionar **VSPack**.
El bridge Axum debe estar ejecutándose en `127.0.0.1:3061`.

Prueba recomendada:

- Importar un Resource Pack base que incluya `pack.mcmeta`.
- Importar un overlay que modifique `assets/<namespace>/textures/...`.
- Comprobar que el conflicto indica el origen ganador, y cambiar
  la prioridad para intercambiar qué archivo prevalece.
- Buscar el recurso resultante y filtrar por texturas o sonidos.
- Exportar y abrir el ZIP; importar nuevamente ese ZIP en una nueva
  sesión para verificar que los archivos combinados sean correctos.

## API HTTP local del gestor

| Método | Ruta | Función |
|---|---|---|
| GET | `/api/v1/packs` | Estado y cuotas de paquetes en memoria |
| POST | `/api/v1/packs/{id}` | Importar ZIP en bytes, nombre en `X-VSPack-Label` codificado con percent-encoding UTF-8 |
| POST | `/api/v1/packs/{id}/priority` | JSON `{"priority":20}` |
| DELETE | `/api/v1/packs/{id}` | Quitar fuente del gestor |
| GET | `/api/v1/packs/preview` | Inventario, categorías, conflictos y capacidades de exportación |
| GET | `/api/v1/packs/texture?path=...` | Ver una imagen PNG/JPEG/WebP validada, máximo 4 MiB |
| GET | `/api/v1/packs/export` | Descargar ZIP final, `Content-Type: application/zip` |
| POST | `/api/v1/packs/layers/export` | Crear bundle ZIP de subpacks base+overlays a partir de `base_id` y `rules` |

**Protecciones:**

- Sólo en localhost; las solicitudes de escritura comprueban
  `Origin` del Studio (`localhost:3060` y `127.0.0.1:3060`).
- Hasta **6 paquetes**, **12 MiB** por ZIP comprimido y
  **64 MiB** combinados de entradas descomprimidas en RAM.
- Hasta **2048 archivos** por ZIP importado, **32 MiB**
  descomprimidos por ZIP y **8 MiB** por archivo.
- Validación de rutas, rechazo de traversal `../`, enlaces simbólicos,
  entradas duplicadas, archivos descomprimidos que excedan cuotas y
  manifiestos inválidos. El límite de merge estructurado existente
  es 16384 contribuciones.
- Descompresión, combinación y creación del ZIP corren en
  `spawn_blocking`, nunca sobre los hilos regionales de Folia.
- Las rutas y nombres de ZIP se muestran mediante `textContent`,
  sin evaluar contenido del pack como HTML o JavaScript.
- La vista previa sólo permite PNG/JPG/WebP con cabecera binaria
  correcta, `Content-Type` fijo y `nosniff`. La URL Blob local
  anterior se revoca al cambiar de recurso o salir del editor.
- No se importan archivos remotos por URL. No se sube ningún ZIP
  a una CDN, al servidor de Minecraft ni al almacenamiento CAS
  automáticamente.
- Los packs cargados son **temporales**: reiniciar el bridge
  limpia la sesión. El navegador debe volver a subirlos cuando
  se reinicie el gestor. No se generan publicaciones ni versiones
  permanentes al importar.

## Pruebas

GitHub Actions verifica:

- Tests de `vsmedia-pack` (importación ZIP segura, mezcla
  determinista, priorización, `sounds.json`, partición de overlays
  y publicación inmutable de distribución de packs).
- Tests de `studio/backend/src/packs.rs`: importación y
  previsualización de dos ZIP, cambio de prioridad, conflictos,
  exportación reproducible, contenido de textura y eventos de
  sonido, rechazo por `Origin`, ZIP inválido, liberación de memoria
  y acceso bajo demanda a texturas de firma validada.
- `node --check studio/assets/pack.js`, compilación Leptos
  para `wasm32`, pruebas Axum, Rust y Java.

**Pendiente para producción real** (no forma parte de este gestor
local): pruebas manuales de navegador, conexión del adaptador
Paper/Folia de VSEngine a la distribución de Resource Packs,
servidor HTTPS/CDN, entrega por jugador y mediciones de
tiempo de descarga, caché SHA-1 y RAM bajo carga. El motor Rust
ya contiene rutinas para capas base+overlays y publicación
inmutable, pero **este panel no las publica automáticamente**.
No asumir una función completada por el simple hecho de que
exista un módulo Rust.

No se genera ningún JAR, ZIP final de release ni tag;
el único ZIP que se descarga es el que solicita el usuario
al presionar **Exportar Resource Pack** en la interfaz.

### Exportación de capas para caché Minecraft

En la columna de exportación, elegir el identificador del pack base
(por ejemplo `virgoland`) y escribir reglas como:

```text
ui=assets/virgoland/textures/ui/
sounds=assets/virgoland/sounds/
```

Al pulsar **Exportar capas separadas**, el backend genera un ZIP contenedor
con `manifest.json` y ZIPs de Resource Pack ordenados (base primero).
El manifiesto informa `logical_id`, `uuid`, `sha1`, `sha256`,
`bytes`, `filename` y cantidad de recursos por capa. En futuras
revisiones, una capa unchanged puede conservar su hash y URL, mientras
otra se actualiza. La regla se valida y las rutas se separan conforme
al prefijo más específico; no se mezclan ni duplican bytes de origen
en varios overlays.

**Este bundle no puede aplicarse directamente a Minecraft:** primero
hay que publicar cada ZIP interno por HTTPS y usar el adaptador
Paper/Folia con UUID, SHA-1 y orden correctos. Estas acciones no se
automatizan desde la interfaz local.

## Resolución de texturas y próxima web de interfaces

El backend expone un **catálogo versionado**, paginado y desacoplado
del frontend actual. Para la futura web de interfaces se pueden
obtener packs importados (metadatos y descarga ZIP), explorar recursos
de GUI/HUD/fonts/atlas y escoger el ganador de cada ruta de textura.
El selector de conflictos ya está disponible en el VSPack actual.

La mezcla usa prioridades y decisiones explícitas por archivo: **no
combina píxeles RGBA automáticamente** ni interpreta JSON desconocidos
como si fueran una sola definición. El merge de eventos de sonidos es
específico de sounds.json. Las rutas distintas conviven sin conflictos.

Ver la especificación: VSPACK-INTERFACES-API.md.

**No está conectada todavía una web externa**; tampoco existe
almacenamiento persistente de packs, repositorio remoto ni distribución
Minecraft automática. El adaptador VSEngine, autenticación y HTTPS
se implementarán cuando se integre la nueva interfaz.
