# VSMedia Studio 26.3 — FullCanvas

Aplicación web ligera con **Leptos 0.8/WASM**, **Rust Audio Engine** y
**Axum** como API local. El cliente de WebRTC para VSRadio y VSStreams usa
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
el servidor Rust/Axum + VSCore local en `127.0.0.1:3061`. La emisión
de radio y salas de voz requieren que el backend esté funcionando.

## Módulos

- **Home:** biblioteca de fuentes, categorías y carpetas.
- **VSAudio:** editor de versiones, volumen, paneo, fades y exportación WAV.
- **VSMix:** edición multiclips y multipista, formas de onda, crossfades
  lineales/equal-power, automatización, selección múltiple, operaciones
  ripple, snapping, portapapeles y undo/redo. Render sin guardar y proyectos
  persistentes `.vsaudio` mediante Rust/Axum.
- **VSRadio:** playlists y programación local, reproductor con micrófono,
  emisión P2P para salas pequeñas (un emisor, 15 oyentes como máximo).
- **VSStreams:** chat de voz P2P con hasta 8 participantes por sala, permisos
  de micrófono explícitos, mute, conexión/desconexión y audio remoto.
- **Guion:** lector automático de texto/doblaje.

### Estado de radio y voz

`studio/assets/live.js` utiliza WebRTC para transportar audio entre
navegadores. `studio/backend/src/live.rs` hace señalización JSON sobre
WebSocket: **no almacena ni transmite el audio por Axum**. El cliente no
solicita micrófono al oyente de radio ni graba participantes. El servidor
escucha únicamente en loopback; no se debe exponer a Internet sin
autenticación, HTTPS/WSS, política de permisos y servidores TURN.

**Radio masiva no está implementada.** Para centenares de oyentes se
necesitaría SFU o una vía de distribución de segmentos, un servicio
de control de acceso y monitoreo. Chat de voz dentro de Minecraft también
requiere integración específica con el plugin/mod.

Ver [LIVE-STREAMING.md](docs/LIVE-STREAMING.md) y
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
