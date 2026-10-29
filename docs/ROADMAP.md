# Roadmap de desarrollo — VSMedia Remake 26.3

## Fase 0 — Fundaciones (actual)
- [x] Workspace Rust con cuatro engines aislados.
- [x] Contratos de API Rust y Java sin referencias a otros plugins.
- [x] Almacenamiento por SHA-256 con catálogo local.
- [x] Merge determinista de entradas con conflictos y prioridades.
- [x] Modelo multipista y mezclador PCM mono básico.
- [x] Mosaicos RGBA 128×128; todavía no mapas Bukkit.
- [x] Generador inicial de documentos de fuente JSON.
- [x] Validar compilación Rust (11 tests) y Java API con GitHub Actions.
- [ ] Versionar esquema de protocolo IPC con pruebas de compatibilidad.

## Fase 1 — Core de producción
- [ ] Base de metadatos transaccional y almacenamiento de objetos atómico.
- [ ] Control de permisos, rate limits, cuotas, auditoría y métricas.
- [ ] API de jobs (progreso, cancelación, prioridades y persistencia).
- [ ] IPC local autenticado y APIs estables de publicación/consulta.
- [ ] Bridge Java + plugin Folia/CanvasMC con schedulers seguros.

## Fase 2 — Packs, fuentes e imágenes
- [ ] Import ZIP seguro, merge de JSON tipado, pack.mcmeta, ZIP reproducible.
- [ ] Detección de cambios, historial, distribución y rolling versions.
- [ ] Decodificadores de imagen, GIF/APNG y protección contra descompresión masiva.
- [ ] Paleta de mapas vanilla, API de MapView y marcos, carteles animados con límites.
- [ ] Gestión de glyph allocations y colisiones de providers.

## Fase 3 — Audio Studio
- [ ] Decodificación/encoding WAV, OGG, MP3, FLAC según bibliotecas/licencias.
- [ ] Mezcla estéreo, resampling, timeline multipista, EQ, fades, pitch y time stretch.
- [ ] Export OGG + sounds.json para Minecraft.
- [ ] Web Audio Studio: grabación con consentimiento, UI multipista y export.

## Fase 4 — Streaming y Voice Chat
- [ ] Server Rust de media con Opus + transporte autenticado y cifrado.
- [ ] Posicionamiento por proximidad, canales, mute y herramientas de privacidad.
- [ ] Adaptación de VSClient (repositorio independiente, cambio explícito futuro).
- [ ] Fuentes en vivo autorizadas y estaciones de radio.
- [ ] Benchmark de CPU, ancho de banda, paquetes perdidos y jitter.

### Límites conocidos
El proyecto aún **no incluye** un plugin JAR instalable, cliente de voz,
decodificación real de audio ni reproducción de GIF/mapas en Minecraft.
Los algoritmos iniciales son pruebas de base, no features de producción.
