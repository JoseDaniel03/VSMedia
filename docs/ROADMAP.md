# Roadmap — VSMedia Remake 26.3

## Core y contratos

- [x] Workspace Rust + 4 engines aislados + API Java.
- [x] Registro de assets por SHA-256 y versión.
- [x] SQLite transaccional con historial por revision.
- [x] Permisos por productor/namespace (denegar por defecto).
- [x] Jobs locales con cola limitada, workers y cancelación cooperativa.
- [x] IPC loopback JSON versionado con token, ACL y cuotas de mensajes.
- [x] Cliente IPC Java asíncrono y pruebas de contrato simulado.
- [x] IPC subidas y descargas de hasta 64 MiB en chunks de 256 KiB, sesiones autenticadas, cuota en memoria y verificación SHA-256.
- [ ] Reanudación de transferencias, disco staging, rotación de credenciales y auditoría.
- [ ] Persistencia de jobs, progreso, cancelación remota y métricas.
- [x] Adaptador de entrega Resource Pack para Paper/Folia 26.3 como fuente Java 25 (sin plugin ejecutable), usando scheduler de entidad y eventos.
- [ ] Bootstrap del plugin VSMedia, lifecycle y schedulers de los demás engines.
- [ ] Prueba end-to-end real entre proceso Rust y puente Java (hoy existen pruebas Rust y contrato con servidor mock en Java).
- [ ] Cuotas agregadas por productor, recolección de objetos huérfanos y respaldo DB.

## Formatos nativos de proyectos (fase de esquemas)

- [x] `vsmedia-formats`: contratos comunes versionados y 5 extensiones nativas.
- [x] Proyecto maestro `.vsproject` con nodos, entradas y dependencias acíclicas locales.
- [x] Referencias inmutables SHA-256; mismos recursos compartidos sin copias.
- [x] Almacenamiento mediante el AssetRepository existente; pruebas de persistencia SQLite y revisión desactualizada.
- [x] API IPC `publish_native` / `read_native` y contratos Java `NativeFormatsApi`.
- [x] Resolver el cierre transitivo de proyectos anidados con límites y detección de ciclos; permisos de lectura por cada referencia en exportación autenticada.
- [ ] Endpoints públicos seguros para compartir proyectos y transferencias grandes; autenticación de la identidad de origen.
- [x] Snapshot portátil opcional con closure de referencias, hashes, deduplicación y ZIP determinista.
- [x] Importación atómica SQLite con detección de conflictos y reimportación idempotente.
- [x] Versión de esquema del catálogo SQLite (migración 0→1 y rechazo de versiones futuras); lectura/escritura por namespaces en API de portabilidad autenticada.
- [ ] Migraciones futuras de manifiestos nativos, firma/confianza del origen y cuotas globales por productor.
- [ ] Herramientas del editor para crear, abrir, duplicar, importar y exportar proyectos.

## Engines

- **Pack:** merge con prioridades, detección de conflictos, importación ZIP segura y export ZIP determinista; optimización Store/Deflate para archivos ya comprimidos. Generador de URL inmutable SHA-256 y SHA-1 del pack para caché de Minecraft. Pendientes servidor estático HTTPS/CDN configurado, integración real con jugador Folia y benchmarks de red. La unión por clave de \`sounds.json\` ya está implementada. Pendiente fuentes y otros JSON tipados, formato específico 26.3 y distribución.
- **Audio:** modelo multipista, mezcla PCM mono/estéreo, fades, balance, cadena reutilizable de efectos, EQ biquad (low/high/peak), FFT/Hann, RMS y atenuación espacial 3D. Importación WAV int/float, export WAV PCM16, remuestreo FFT Rubato con límites, render multipista con caché por asset. Edición no destructiva por clip (offset de origen, reversa, fades, paneo, mute, curva de volumen, EQ en cadena) y pista (mute/solo/volumen/paneo); resumen estéreo por buckets de ondas. Identificadores persistentes de clips/pistas, comandos de movimiento y edición mediante IDs, sesiones de undo/redo con cuota en RAM. Pendiente UI, persistencia del historial, colaboración, más efectos, edición de regiones compleja, otros códecs, exportación OGG, streaming/Opus/voz.
- **Image:** escalado RGBA nearest/Lanczos3 y mosaicos de mapas 128×128, deltas de fotogramas animados. Decodificador estático PNG/JPEG/WebP/GIF y secuencias GIF/APNG con límites de memoria. Pendiente WebP animado, conversión paleta/vistas MapView.
- **Font:** generador JSON de fuentes bitmap y registro de glifos con asignación global de Unicode PUA. Pendiente fuentes completas, integración con pack y compatibilidad.

## Distribución y estado

No se genera **JAR, ZIP descargable, release ni paquete final** mientras sigamos construyendo.
Los checks compilan y ejecutan pruebas del **código fuente**. No hay publicación de artefactos.
Se mantiene VSMedia independiente de VSRender, Typewriter y VSUiRemake.
VSClient se actualizará después para streaming/voz; vanilla conserva sonidos de pack y mapas.

## Data Packs — etapa final (aplazada)

La compatibilidad con Data Packs se estudiará **al terminar Core, Audio,
Image, Font, Pack de recursos, interfaces y APIs**. No se añade su importación
ni distribución todavía. `vspack` actualmente representa Resource Packs.

## Resource Pack Delivery (base terminada, integración pendiente)

- [x] SHA-1 exacto de ZIP para que Minecraft aproveche su caché nativa.
- [x] SHA-256 para identificar archivos públicos inmutables sin invalidación global.
- [x] UUID de pack estable por identificador lógico.
- [x] Publicación atómica en carpeta estática, sin sobrescribir packs ya publicados.
- [x] ZIP determinista con Store para medios comprimidos y Deflate para JSON.
- [x] CLI administrativa para preparar un pack para servidor estático HTTPS/CDN.
- [x] API de oferta por jugador con estado, supresión de reenvíos y eventos, y adaptador de scheduler Folia fuente.
- [x] Pilas opcionales de packs base + overlay en Java/Paper 26.3, con orden y límites; caché independiente por hash.
- [x] Particionado configurable por prefijos y publicación de capas independientes, con SHA-1 de cada ZIP y prueba de base inmutable.
- [x] Reconciliación del adaptador Folia: elimina solamente las capas anteriores administradas por VSMedia.
- [x] Recomendaciones de overlays según frecuencia de cambios de rutas y tamaño, manteniendo reglas fijas para preservar la caché.
- [ ] Automatizar evaluación estadística de tamaños/descargas y benchmark real bajo carga.
- [ ] Integrar y habilitar el adaptador en un plugin VSMedia real; pruebas con jugadores.
- [ ] Medir consumo de ancho de banda, caché real y latencia desde distintos clientes.
- [ ] Configurar alojamiento HTTPS/CDN y descarga pública; no hay servicio HTTP abierto por defecto.


### Studio: edición de regiones conectada

- [x] VSMix con múltiples regiones por pista y guardado/recuperación nativa.
- [x] Render Rust de borradores VSAudio y VSMix sin crear revisiones de proyecto.
- [x] Mezcla de clips superpuestos, paneo y fades con verificación SHA-256.
- [x] Reutilización de recursos idénticos durante previsualización local.
- [ ] Arrastre de regiones, ajuste de bordes y formas de onda editables desde la web.
- [ ] Decodificación MP3/FLAC/OGG/Opus, buses, efectos adicionales y audio en vivo.

## Studio VSPack — gestión de Resource Packs (26.3)

- [x] Pantalla propia FullCanvas y acceso desde Home / pestañas.
- [x] Importación de varios ZIP con prevención de rutas peligrosas,
  symlinks, entradas duplicadas, ZIP bombs y límites globales de memoria.
- [x] Inventario de recursos por namespace, nombre y categoría,
  filtros y paginación.
- [x] Prioridades por fuente; conflictos ganadores/perdedores;
  merge semántico de sonidos por evento.
- [x] Exportación ZIP determinista, Store/Deflate según formato,
  manifiesto `pack.mcmeta` requerido; tests end-to-end de merge.
- [x] CORS local, límites de recursos y separación de trabajos CPU.
- [ ] Sesiones persistentes respaldadas por CAS y gestión de versiones
  de packs guardados.
- [ ] Vista previa real de texturas/modelos/sonidos dentro del editor.
- [ ] Distribución HTTPS/CDN, integración final con el adaptador
  Paper/Folia del servidor y benchmarks reales con clientes.
- [ ] Instrumentación de overlays base/delta y controles administrativos
  de publicación desde el Studio.

**Alcance:** el gestor local de ZIPs está desarrollado y probado
automáticamente; la distribución de packs a Minecraft sigue pendiente
y no se debe marcar como verificada sin pruebas reales. Los datapacks
no forman parte de esta fase.
