# VSMedia Remake 26.3

Infraestructura multimedia independiente para VSEngine / CanvasMC / Folia.
Nuevo proyecto en Rust con APIs para productores Java. Los otros plugins **consumen VSMedia**, VSMedia no depende de ellos.

> **Estado: desarrollo activo.** Solo código fuente, pruebas y documentación. No se ha publicado ningún JAR, ZIP final, release ni plugin instalable. Voice Chat y streaming NO están implementados.

## Arquitectura

| Módulo | Implementado actualmente | Pendiente principal |
|---|---|---|
| Core Rust | SQLite + SHA-256 + revisiones, ACL, jobs acotados | Jobs persistentes, cuotas globales, backups |
| Formatos nativos | .vsaudio / .vsimage / .vsstream / .vspack / .vsproject, versión 1, referencias SHA-256 | Snapshot portátil deduplicado, importación atómica; pendientes migraciones y edición web |
| Pack Engine | Merge, conflictos, ZIP seguro, export determinista y unión de sounds.json | Otros esquemas JSON, resource-pack 26.3, distribución |
| Audio Engine | Modelo multipista, WAV PCM/float, render timeline, Rubato FFT SRC, EQ/FFT/RMS y espacial | MP3/FLAC/OGG, edición avanzada, Opus, streaming, voz |
| Image Engine | Tiles 128x128, Lanczos3, PNG/JPEG/WebP/GIF y fotogramas GIF/APNG | Paleta MapView, marcos, web URL segura |
| Font Engine | Generación JSON bitmap, asignador de glifos | Composición final y formatos |
| IPC v1 | Servicio Rust loopback con tokens y límites | Transferencia masiva, E2E contra Java, hardening |
| Java API + Bridge | Interfaces + cliente socket asíncrono | Adaptador plugin Folia/Bukkit y ciclo de vida |
| Panel web | No iniciado | Studio de audio e imágenes, gestión |
| VSClient | No modificado | Voz y streaming, cliente Fabric |

## Pruebas sin empaquetar

Requiere Rust stable, JDK 21 y Maven.

```bash
cargo test --workspace --all-targets
mvn -B -f java/pom.xml test
```

Los checks no publican releases ni suben archivos compilados. Ver [formatos nativos](docs/NATIVE-FORMATS.md), [arquitectura](docs/ARCHITECTURE.md),
[protocolo IPC](docs/IPC.md), [matemáticas](docs/MATH-AND-PERFORMANCE.md), [referencias técnicas](docs/RESEARCH.md) y [roadmap](docs/ROADMAP.md).

## Seguridad y compatibilidad

La API del Rust daemon se limita al loopback; usa tokens por productor y ACL por namespace.
No se debe exponer el puerto IPC a Internet. El bridge Minecraft/Folia necesita respetar
Entity/Region schedulers. La transmisión en tiempo real y voz requerirán VSClient.
Los sonidos de Resource Packs, fuentes y mapas podrán usarse sin mod una vez integrados.

**Regla central:** VSRender, Typewriter, VSUiRemake y otros producen/consumen assets mediante la API pública.
No se agregan dependencias de estos proyectos al Core.
