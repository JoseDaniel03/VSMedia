# VSMedia Remake 26.3

Infraestructura multimedia independiente para VSEngine / CanvasMC / Folia.
Nuevo proyecto en Rust con APIs para productores Java. Los otros plugins **consumen VSMedia**, VSMedia no depende de ellos.

> **Estado: desarrollo activo.** Solo código fuente, pruebas y documentación. No se ha publicado ningún JAR, ZIP final, release ni plugin instalable. Voice Chat y streaming NO están implementados.

## VSRadio: emisoras sincronizadas para Minecraft (rama de desarrollo)

La playlist local del editor VSRadio puede publicarse mediante **◉ Emitir en
Minecraft**. VSCore guarda pistas PCM16 mono 48 kHz y un reloj UTC de emisora.
El nuevo comando `/vsradio emisora LatinSabroso` sintoniza la fuente compartida
Plasmo Voice donde se encuentre la canción; `/vsradio apagar` la desconecta.

Requiere PlasmoVoice en servidor y clientes, y una futura compilación del
plugin autorizado para desplegar los cambios. El audio de micrófono WebRTC
no está enlazado a este sistema. [Manual VSRadio](docs/VSRADIO-LIVE.md).

## VSPack: packs optimizados y publicación CDN (rama de desarrollo)

Se añadió el publicador offline tools/vspack_publish.py: exporta el ZIP de VSCore
local, optimiza archivos JSON y opcionalmente invoca PackSquash externo, divide
los ZIPs por módulos bajo un límite configurable (90 MiB predeterminado),
genera hashes SHA-1/SHA-256, UUID, manifiestos de versiones, permite subir a
Cloudflare R2 y volver a una versión anterior.

Un watcher HTTPS en el plugin Folia prepara la actualización de capas para los
jugadores desde un manifiesto público, sin realizar compresión ni descargas
en los hilos del juego. **No está desplegado ni probado in-game**: primero se
debe conectar R2 y en otra etapa compilar/instalar el plugin actualizado.

Ver [manual de optimización y publicación](docs/VSPACK-AUTO-PUBLISH.md)
y [audio y tipografías Minecraft](docs/MINECRAFT-PUBLICATION.md).

**No generar JARs ahora**: CI mantiene el job de empaquetado deshabilitado.
La publicación de resource packs no recompila el plugin.

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
| Panel web | FullCanvas Leptos, VSAudio/VSMix con proyectos CAS, controles EQ/paneo/automatización, render WAV Rust por HTTP y comandos transaccionales; VSRadio local | Arrastre visual de regiones, reproducción en vivo, códecs comprimidos, autenticación y conexión Folia |
| VSClient | No modificado | Voz y streaming, cliente Fabric |

## Pruebas sin empaquetar

Requiere Rust stable, **JDK 25** y Maven para compilar todo, incluido el módulo de Paper 26.3. Las APIs base Java conservan compatibilidad de código fuente con Java 21.

```bash
cargo test --workspace --all-targets
mvn -B -f java/pom.xml test
cargo test --manifest-path studio/backend/Cargo.toml --all-targets
cargo check --manifest-path studio/Cargo.toml --target wasm32-unknown-unknown
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

## Studio FullCanvas (desarrollo)

La web de Leptos/WebAssembly aportada para VSMedia está integrada en
`studio/`, con el puente Axum en `studio/backend/`. Mantiene el diseño
FullCanvas claro, VSAudio, VSMix, VSRadio, importación local y carpetas.
Los editores **VSAudio y VSMix** permiten guardar y recuperar proyectos
`.vsaudio` del catálogo SQLite/CAS, con fuentes identificadas por SHA-256.
Las ediciones por comandos del backend usan comparación optimista atómica
para evitar sobrescrituras concurrentes.

En CachyOS instalar Rust, el objetivo `wasm32-unknown-unknown` y Trunk;
arrancar con `cd studio && ./start.sh`. La web queda en el puerto 3060
y el bridge local en el 3061. El bridge HTTP **no se debe exponer a Internet**.

VSMix guarda y recupera múltiples regiones por pista; tanto VSMix como
VSAudio pueden previsualizar borradores con Rust sin crear revisiones de
proyecto. Las fuentes WAV utilizadas pueden subirse al CAS por acción
explícita. El reproductor HTML5 sigue siendo una escucha local, no una
emisión a Minecraft. Los endpoints remotos de streaming/voz siguen
sin implementar.
Ver [integración Studio](docs/STUDIO-INTEGRATION.md) y
[manual técnico](studio/docs/INTEGRATION.md).


## VSMedia Web v11 y comando `vsmedia web`

La web 2026 v11 se puede incorporar en `web/` sin reemplazar Rust Core ni el plugin Folia. Ver [guía de arranque](docs/WEB-V11.md).


## Inicio automático desde Folia

El plugin inicia VSMedia Web al habilitarse. La web v11 se incorpora al JAR desde la carpeta `web/` del repositorio y delega la edición/render al navegador. [Instrucciones de empaquetado y seguridad](docs/FOLIA-WEB-V11.md).

## Arranque de VSCore desde Folia (VSCinematic/VSRender)

Al activar el plugin, el backend Rust VSCore incluido en el JAR se inicia por defecto
en Linux x86_64, escuchando solamente en `127.0.0.1:3061`. Si ya existe una
instancia activa, la reutiliza y **no** ejecuta otra. Si falta el binario o no
se está ejecutando en una plataforma compatible, la web sigue activa y se
registra un aviso; no se declara una integración de fuentes/pack operativa.

Los ajustes existentes en `plugins/VSMedia/config.yml` no se sobrescriben.
Si una instalación anterior ya tenía `studio-core.autostart: false`, cambiarlo a
`true` y reiniciar para activar el nuevo valor. Para instalaciones nuevas
`true` es el valor por defecto. El puerto 3061 **no** debe exponerse a Internet.
