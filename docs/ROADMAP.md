# Roadmap — VSMedia Remake 26.3

## Core y contratos

- [x] Workspace Rust + 4 engines aislados + API Java.
- [x] Registro de assets por SHA-256 y versión.
- [x] SQLite transaccional con historial por revision.
- [x] Permisos por productor/namespace (denegar por defecto).
- [x] Jobs locales con cola limitada, workers y cancelación cooperativa.
- [x] IPC loopback JSON versionado con token, ACL y cuotas de mensajes.
- [x] Cliente IPC Java asíncrono y pruebas de contrato simulado.
- [ ] IPC transferencia de recursos grandes/chunks, rotación de credenciales y auditoría.
- [ ] Persistencia de jobs, progreso, cancelación remota y métricas.
- [ ] Plugin bridge real de Folia/CanvasMC con ciclo de vida y schedulers.
- [ ] Prueba end-to-end real entre proceso Rust y puente Java.
- [ ] Cuotas agregadas por productor, recolección de objetos huérfanos y respaldo DB.

## Formatos nativos de proyectos (fase de esquemas)

- [x] `vsmedia-formats`: contratos comunes versionados y 5 extensiones nativas.
- [x] Proyecto maestro `.vsproject` con nodos, entradas y dependencias acíclicas locales.
- [x] Referencias inmutables SHA-256; mismos recursos compartidos sin copias.
- [x] Almacenamiento mediante el AssetRepository existente; pruebas de persistencia SQLite y revisión desactualizada.
- [x] API IPC `publish_native` / `read_native` y contratos Java `NativeFormatsApi`.
- [ ] Resolver dependencias entre proyectos anidados globalmente y asegurar ACL de lectura por referencia.
- [ ] Portabilidad: paquete autocontenido opcional, import/export transaccional, rollback y migración de esquemas.
- [ ] Herramientas del editor para crear, abrir, duplicar, importar y exportar proyectos.

## Engines

- **Pack:** merge con prioridades, detección de conflictos, importación ZIP segura y export ZIP determinista. La unión por clave de \`sounds.json\` ya está implementada. Pendiente fuentes y otros JSON tipados, formato específico 26.3 y distribución.
- **Audio:** modelo multipista, mezcla PCM mono/estéreo, fades, balance, cadena reutilizable de efectos, EQ biquad (low/high/peak), FFT/Hann, RMS y atenuación espacial 3D. Importación WAV int/float, export WAV PCM16, remuestreo FFT Rubato con límites, render multipista con caché por asset. Pendiente edición no destructiva completa, otros códecs, exportación OGG, streaming/Opus/voz y Web Studio.
- **Image:** escalado RGBA nearest/Lanczos3 y mosaicos de mapas 128×128, deltas de fotogramas animados. Decodificador estático PNG/JPEG/WebP/GIF y secuencias GIF/APNG con límites de memoria. Pendiente WebP animado, conversión paleta/vistas MapView.
- **Font:** generador JSON de fuentes bitmap y registro de glifos con asignación global de Unicode PUA. Pendiente fuentes completas, integración con pack y compatibilidad.

## Distribución y estado

No se genera **JAR, ZIP descargable, release ni paquete final** mientras sigamos construyendo.
Los checks compilan y ejecutan pruebas del **código fuente**. No hay publicación de artefactos.
Se mantiene VSMedia independiente de VSRender, Typewriter y VSUiRemake.
VSClient se actualizará después para streaming/voz; vanilla conserva sonidos de pack y mapas.
