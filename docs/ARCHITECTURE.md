# Arquitectura — contrato inicial

## Regla de dependencias

`VSRender / Typewriter / VSUiRemake / otros → VSMedia API → VSMedia Core → Engines`

**No existe dependencia inversa.** Los productores publican contenido con `AssetId` y VSMedia conserva el catálogo.
El puente Java de Minecraft es un adaptador opcional de plataforma; ninguna crate Rust usa Bukkit/Folia.

## Procesos y límites

- **Plugin Java:** eventos/players/mapas/comandos mediante schedulers correctos de Folia. No procesamiento pesado en hilos de región.
- **Rust daemon (objetivo):** procesamiento y almacenamiento, separado de la JVM. IPC autenticado y versionado pendiente.
- **VSClient:** streaming/voz con consentimiento y captura bajo control del usuario. Integración posterior en el repositorio del mod.
- **Cliente vanilla:** sonidos ya incorporados al Resource Pack, fuentes y mapas.

## API

`vsmedia-api` define identificadores, tipos, metadatos y contrato de almacenamiento. `java/vsmedia-api` define el contrato asíncrono para plugins de Minecraft. **Todavía no existe un puente conectando ambas APIs.**

## Seguridad esencial

1. Validar nombres/rutas antes de acceder a disco; impedir rutas absolutas y traversal.
2. Resolver conflictos de archivos de packs mediante prioridades explícitas, nunca sobrescribir silenciosamente.
3. Límite de archivos, imágenes, duración de audio y memoria; no ejecutar ffmpeg con parámetros arbitrarios del usuario.
4. Descarga por URL solo mediante fetcher con protección SSRF y tamaños máximos, por implementar.
5. Voz: autenticación, cifrado, permisos, mute, no grabación predeterminada.
6. Mantener hashes, versiones y pruebas automatizadas.

## Primer alcance funcional

Core: catálogo de assets y objetos hash en disco. El catálogo inicial usa un único escritor local; migrar a un almacén transaccional antes de habilitar múltiples procesos. Engines: algoritmos aislados con tests de unidad. **No confundirlos con un sistema listo para producción.**
