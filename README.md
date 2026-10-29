# VSMedia Remake 26.3

Infraestructura multimedia independiente para VSEngine / CanvasMC / Folia.
Nuevo proyecto en Rust con APIs para productores Java. Los otros plugins **consumen VSMedia**, VSMedia no depende de ellos.

> **Estado: desarrollo activo.** Solo código fuente, pruebas y documentación. No se ha publicado ningún JAR, ZIP final, release ni plugin instalable. Voice Chat y streaming NO están implementados.

## Arquitectura

| Módulo | Implementado actualmente | Pendiente principal |
|---|---|---|
| Core Rust | SQLite v1, SHA-256 + revisiones, ACL transaccionales, jobs acotados | Jobs persistentes, cuotas globales, backups |
| Formatos nativos | .vsaudio / .vsimage / .vsstream / .vspack / .vsproject, versión 1, referencias SHA-256 | Migración futura de manifiestos, edición web, control de permisos end-to-end |
| Pack Engine | ZIP determinista, compresión por formato, publicación HTTPS inmutable, capas base/overlays y recomendador de prefijos por historial, SHA-1 | HTTP/CDN operativo, selección inteligente de capas y métricas reales |
| Audio Engine | Multipista, WAV PCM/float, remuestreo FFT, trims/fades/pan, EQ, automatización, IDs estables, comandos de edición y undo/redo | UI completa, MP3/FLAC/OGG, más efectos, Opus y voz |
| Image Engine | Tiles 128x128, Lanczos3, PNG/JPEG/WebP/GIF y fotogramas GIF/APNG | Paleta MapView, marcos, web URL segura |
| Font Engine | Generación JSON bitmap, asignador de glifos | Composición final y formatos |
| IPC v1 | Loopback autenticado, carga/descarga por bloques hasta 64 MiB y verificación SHA-256 | Reanudación, staging a disco, integración E2E y hardening |
| Java API + Bridge | Interfaces, socket asíncrono, parser de descriptor CDN y seguimiento de packs | Bootstrap y persistencia de configuración |
| Folia Delivery Adapter | Java 25/Paper 26.3, scheduler de jugador, estados, base/overlay y retiro de capas propias | Bootstrap en servidor, CDN y pruebas con Minecraft real |
| Panel web | No iniciado | Studio de audio e imágenes, gestión |
| VSClient | No modificado | Voz y streaming, cliente Fabric |

## Pruebas sin empaquetar

Requiere Rust stable, **JDK 25** y Maven para compilar todo, incluido el módulo de Paper 26.3. Las APIs base Java conservan compatibilidad de código fuente con Java 21.

```bash
cargo test --workspace --all-targets
mvn -B -f java/pom.xml test
```

Los checks no publican releases ni suben archivos compilados. Ver [formatos nativos](docs/NATIVE-FORMATS.md), [arquitectura](docs/ARCHITECTURE.md),
[protocolo IPC](docs/IPC.md), [Resource Pack rápido](docs/RESOURCE-PACK-DELIVERY.md), [adaptador Folia](docs/FOLIA-RESOURCE-PACK.md), [edición de audio](docs/AUDIO-EDITING.md), [matemáticas](docs/MATH-AND-PERFORMANCE.md), [referencias técnicas](docs/RESEARCH.md) y [roadmap](docs/ROADMAP.md).

## Seguridad y compatibilidad

La API del Rust daemon se limita al loopback; usa tokens por productor y ACL por namespace.
No se debe exponer el puerto IPC a Internet. El bridge Minecraft/Folia necesita respetar
Entity/Region schedulers. La transmisión en tiempo real y voz requerirán VSClient.
Los sonidos de Resource Packs, fuentes y mapas podrán usarse sin mod una vez integrados.

**Regla central:** VSRender, Typewriter, VSUiRemake y otros producen/consumen assets mediante la API pública.
No se agregan dependencias de estos proyectos al Core.

## Secuencia de desarrollo

Continuamos con Core, APIs y multimedia antes de construir el panel y los
adaptadores de Minecraft. **La gestión de Data Packs queda aplazada para el
último tramo del proyecto**, sin modificar su comportamiento actual.
