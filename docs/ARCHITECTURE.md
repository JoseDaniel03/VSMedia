# Arquitectura de VSMedia Remake 26.3

## Regla de dependencias

`VSRender / Typewriter / VSUiRemake / otros → VSMedia API (Java) → Bridge Java → IPC v1 → Core Rust → Engines`

La dirección es unidireccional. VSMedia no importa clases de los plugins consumidores.

## Componentes

- `crates/vsmedia-api`: identificadores versionados, tipos, metadatos y trait de almacenamiento.
- `crates/vsmedia-core`: SQLite transaccional con historial, hashes SHA-256 y trabajos con cola/worker pool. `FsAssetStore` es un backend legado de prueba, no apto para producción.
- `crates/vsmedia-protocol`: esquema de mensajes tipados y límites de tamaño IPC.
- `crates/vsmedia-formats`: formatos editables VSAudio, VSImage, VSStream, VSPack y VSProject; validación de SHA-256 y grafos; snapshot portátil ZIP determinista y verificación de cierre transitivo.
- `crates/vsmedia-engine`: proceso Rust de desarrollo que sirve IPC autenticado solo por 127.0.0.1. En tareas de lectura/escritura usa `spawn_blocking`.
- `crates/vsmedia-pack`: fusión determinista y prioridad por productor, ZIP import/export, cuotas y validación de rutas.
- `crates/vsmedia-audio`: modelo multipista y rutinas primitivas DSP PCM.
- `crates/vsmedia-image`: mosaicos y cálculo de cambios por fotograma para actualizaciones de mapas.
- `crates/vsmedia-font`: definiciones bitmap y asignación global de glifos del Área de Uso Privado.
- `java/vsmedia-api`: contrato asíncrono que deben compilar los plugins consumidores, además de `NativeFormatsApi` para importar/exportar manifiestos.
- `java/vsmedia-bridge`: cliente asíncrono con pool acotado, socket loopback y framing; no depende de Bukkit.

## Integración Minecraft pendiente

`VSMediaPlugin` Java administrará los eventos y jugadores mediante los schedulers de VSEngine/Folia,
el ciclo de vida del proceso Rust y la distribución de packs. No debe ejecutar DSP ni I/O
bloqueante sobre los hilos regionales. MapViews y colocación de marcos requieren APIs de plataforma.

## Audio / VSClient

El servidor Rust gestionará radio/streaming/voice chat y VSClient se ocupará de captura,
decodificación y espacialización. **Nada de esto está activo aún.**
El diseño debe incluir Opus, autenticación, privacidad, silenciar/ensordecer y ningún registro de audio
por defecto. Minecraft vanilla utiliza sonidos ya presentes en Resource Packs.

## Límites e integridad

- Hashes SHA-256 y validación en lectura; catálogo SQLite en WAL con revisiones atómicas.
- Cuotas per-asset, ZIP seguro con control de expansión, límites de imágenes y de audio.
- Tokens independientes por productor y autorización denegada por defecto por namespace.
- IPC experimental con mensajes JSON de máximo 4 MiB y blobs inline de 2 MiB.
- El IPC **no es un servicio público**; requiere un mecanismo de transferencia por streaming/chunks
  antes de usar assets grandes en producción.
- El trabajo de procesamiento offline se ejecutará como jobs con cancelación cooperativa.
  Todavía faltan estado persistente, progreso y retención limitada de jobs.
- Importación remota por URL requiere protección SSRF, DNS pinning y cuotas; no implementada.

## Estado y prueba

Las pruebas actuales verifican algoritmos de engines y contratos de red con mocks. No se han
validado todavía mapas reales en Minecraft, builds de plugin, VSClient, cargas de producción o
la interoperabilidad completa Rust↔Java sobre un proceso real.


## Portabilidad y atomicidad

El adaptador `vsmedia-formats::portable` exporta e inspecciona snapshots
autocontenidos sin disco ni acceso a red. El importador **solo administrativo**
usa `AtomicAssetRepository::import_if_absent_or_identical`. La implementación
SQLite garantiza commit o rollback de todas las entradas del catálogo y no
reemplaza una clave existente si cambió su hash/tipo.

Los objetos de contenido se escriben antes del commit; una falla de proceso
puede dejar objetos huérfanos, pero **nunca referencias de catálogo parciales**.
Se incorporará garbage collection seguro antes de habilitar exportaciones masivas.
Las variantes de confianza administrativa `export` e `import` **no** se
deben exponer a productores remotos. Las variantes `export_for` e
`import_for` aplican permisos por productor: la exportación valida lectura
de cada recurso vinculado y la importación comprueba permisos de escritura de
todos los namespaces dentro de la misma transacción. Antes de exponerlas por
IPC/web faltan cuotas por productor, transporte grande y autenticación de las
peticiones de proyecto. El esquema SQLite está versionado por `user_version`
y migración 0→1; formatos nativos siguen en versión 1 sin migración histórica.
