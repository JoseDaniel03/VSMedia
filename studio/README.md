# VSMedia Studio 26.3 — FullCanvas

Aplicación web ligera con **Leptos 0.8/WASM**, **Rust Audio Engine** y
**Axum** como API local. El cliente de WebRTC para VSRadio usa
JavaScript nativo de navegador porque implementa el transporte multimedia
sin pasar PCM ni paquetes RTP por la capa WASM. Interfaz clara en español.

## Iniciar en CachyOS / Linux

Requisitos: Rust estable, `wasm32-unknown-unknown`, Trunk 0.22.1 o
posterior, y navegador con WebRTC/Web Audio.

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cd studio
./start.sh
```

Abrir http://127.0.0.1:3060. El script inicia Trunk en el puerto 3060 y
el servidor Rust/Axum + VSCore local en `127.0.0.1:3061`. La emisión P2P de VSRadio requiere que el backend esté funcionando.
VSStreams requiere Plasmo Voice en Minecraft y una integración futura con VSEngine para mostrar telemetría.

## Módulos

- **Home:** biblioteca de fuentes, categorías y carpetas.
- **VSAudio:** editor de versiones, volumen, paneo, fades y exportación WAV.
- **VSMix:** edición multiclips y multipista, formas de onda, crossfades
  lineales/equal-power, automatización, selección múltiple, operaciones
  ripple, snapping, portapapeles y undo/redo. Render sin guardar y proyectos
  persistentes `.vsaudio` mediante Rust/Axum.
- **VSRadio:** playlists persistentes con reconexión por nombre al reimportar,
  reproducción anterior/siguiente, repetir, notas de programación local,
  mezcla micrófono+playlist y emisión P2P
  para un emisor y hasta 15 oyentes.
- **VSStreams:** panel de integración de Plasmo Voice, mod/plugin de voz
  por proximidad para Minecraft; sólo presenta estado real de integración
  y requisitos. La administración avanzada se implementará después.
- **VSPack:** gestor de ZIPs de Resource Packs, importación validada,
  capas por prioridad, conflictos, buscador de archivos, vista previa de
  texturas y exportación ZIP determinista o bundle base+overlays mediante
  el mismo Rust Pack Engine.
  No publica automáticamente en Minecraft ni incorpora datapacks.
- **Guion:** lector automático de texto/doblaje.

### Estado de radio y voz

`studio/assets/live.js` utiliza WebRTC para transportar audio entre
navegadores. `studio/backend/src/live.rs` hace señalización JSON sobre
WebSocket: **no almacena ni transmite el audio por Axum**. El cliente no
solicita micrófono al oyente de radio ni graba participantes. El servidor
escucha únicamente en loopback; no se debe exponer a Internet sin
autenticación, HTTPS/WSS, política de permisos y servidores TURN.

**Radio masiva no está implementada.** Para centenares de oyentes se
necesitaría SFU o distribución segmentada y un servicio de control de acceso.
VSStreams no tiene un chat WebRTC alternativo: reutiliza Plasmo Voice
para la voz dentro de Minecraft y espera el futuro adaptador VSEngine.

Ver [VSPACK.md](docs/VSPACK.md), [LIVE-STREAMING.md](docs/LIVE-STREAMING.md) y
[AUDIO-EDITING.md](../docs/AUDIO-EDITING.md).

## Seguridad y pruebas

El editor conserva fuentes locales en memoria hasta la publicación explícita
a CAS. El puente API escucha en `127.0.0.1:3061` y limita entradas, tamaños
y permisos por namespace. Los códigos de sala no son credenciales.

GitHub Actions ejecuta `cargo test --workspace --all-targets`, pruebas
Axum, `cargo check` de Leptos wasm32, pruebas del editor y `node --check`
sobre el cliente WebRTC, además del módulo Java. Las conexiones y dispositivos
de audio requieren también **pruebas manuales en navegadores reales**.

Sin empaquetado de JAR/ZIP/release durante el desarrollo.

## Backend para la futura web de interfaces

VSPack ya expone el catálogo y la descarga de fuentes por ID,
un inventario paginado de recursos de interfaz, resolución de colisiones
por archivo y descargas de assets individuales. Las decisiones
se aplican a la compilación final. La API es local y de sesión,
independiente de las clases CSS del editor, y está documentada en
[Contrato de recursos e interfaces](docs/VSPACK-INTERFACES-API.md).

La nueva web de gestión de interfaces no está construida ni conectada
en esta fase. Se integrará cuando se proporcione su código.

## Preparación del nuevo gestor de recursos

El backend VSPack Studio ya dispone de una API independiente del diseño
de la futura página: importar hasta 24 Resource Packs, combinar texturas
por prioridad, elegir el ganador de cada ruta, guardar recetas JSON
portables, previsualizar la composición y exportar un único ZIP o separar
por categorías (GUI/HUD, ítems, bloques, audio, partículas, entidades,
idiomas y otros). Se preservan sidecars PNG.mcmeta.

[Contrato API del gestor](docs/VSPACK-MANAGER-API.md) ·
[API de recursos e interfaces](docs/VSPACK-INTERFACES-API.md).

La persistencia de fuentes en CAS, el proxy autenticado y la instalación
automática en VSClient se conectarán posteriormente. No se ha integrado
una página nueva ni se ha publicado un Resource Pack a Minecraft.

VSPack también prepara un inspector de referencias y la selección conjunta de PNG/.png.mcmeta para tooltips y animaciones. Véase docs/VSPACK-MANAGER-API.md.

## Guardar proyectos de VSPack

Las recetas de fusión se guardan ahora en
\`VSMEDIA_DATA_DIR/vspack-projects/\` (por defecto,
\`./.vsmedia-data/vspack-projects/\`). Es un almacenamiento local
duradero que permite recuperar los perfiles al reiniciar el backend,
con revisión para evitar sobrescrituras accidentales. Las fuentes ZIP
siguen siendo efímeras y hay que reimportarlas al recuperar una receta.
La exportación del gestor incluye SHA-1, SHA-256 y ETag para
comprobar la integridad. Más detalles:
[VSPACK-MANAGER-API.md](docs/VSPACK-MANAGER-API.md).
