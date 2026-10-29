# VSMedia Studio — Rust 1.0 (prototipo funcional local)

Aplicación web de código Rust (Leptos 0.8, compilado a WebAssembly). **No usa React, Vue, Electron ni JavaScript de aplicación**. Toda la interfaz está en español y usa solamente el tema claro. Ventana completa, sin logo, barra inferior ni controles de docking/layout.

## Requisitos
- Rust estable, `wasm32-unknown-unknown`, Trunk 0.22.1 o posterior
- Navegador moderno con Web Audio y MediaRecorder; localhost o HTTPS para permisos de micrófono

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cargo check --target wasm32-unknown-unknown
./start.sh
```

Abrí http://127.0.0.1:3060.

## Espacios y funciones reales

- **Home:** accesos, biblioteca de audio compartida, carpetas y subcarpetas, clasificación por género, tipo y carpeta. Carpeta/etiquetas se editan en la interfaz. Las carpetas, el guion y la programación se conservan en `localStorage`; **los contenidos binarios de audio no**.
- **VSAudio:** reproductor del navegador, múltiples versiones de un clip, recorte por segundos, ganancia, fade in/out, inversión en la **exportación WAV PCM de 16-bit**. El audio original se puede descargar sin conversiones. La vista previa utiliza el original con posición y volumen ajustados; fundidos e inversión se escuchan en el archivo exportado.
- **VSMix:** crear/quitar y renombrar pistas; vincular archivos a cada una; transporte simultáneo del navegador, volúmenes por pista, Mute, Solo, fundidos de entrada y salida en reproducción (actualizados al evento de progreso del navegador), captura de micrófono en pista seleccionada. **No hay todavía alineación de reloj de audio a nivel sample ni render de mezcla multipista:** es un MVP de reproducción local, no una DAW terminada.
- **Guion/doblaje:** edición de texto, guardado local y lector que se desplaza a velocidad configurable, con reproducir/pausar/inicio.
- **VSRadio:** crear/eliminar playlists, agregar material importado, reordenar arriba/abajo, quitar elementos, reproducir y avanzar automáticamente, grabación de micrófono al archivo local, programación editable.
- **API futura:** contratos tipados en `src/api_contract.rs`, endpoints documentados en `docs/API.md`. No se simula un transmisor: aparece claramente «Sin emisión conectada».

## Backend opcional (preparado para integrar el plugin)

`./start.sh` inicia **Axum conectado a VSCore** en `127.0.0.1:3061` y Trunk en `127.0.0.1:3060`. Se implementaron subida de audios, lectura y guardado de proyectos `.vsaudio` con revisiones, y comandos de edición Rust. El botón **Guardar proyecto** en VSAudio hace la publicación explícita. Streaming, emisión de radio y reproducción Minecraft siguen sin conexión y responden **501**.

## Limitaciones y seguridad

Los archivos se alojan en memoria del navegador hasta que el usuario pulsa **Guardar proyecto**. Esa acción publica el audio y el proyecto `.vsaudio` en el VSCore SQLite/CAS local. No hay subida automática ni servicios externos. Los permisos del micrófono son solicitados al pulsar Grabar. El audio grabado se entrega en el contenedor y códec disponible en MediaRecorder (normalmente WebM, según navegador). No se exporta MP3 ni se promete edición avanzada en tiempo real.

Si un archivo no puede decodificarse con Web Audio, el editor simple informa el fallo al exportar. WAV se renderiza como PCM estéreo o mono (los canales adicionales no se incluyen). Clips extensos pueden exceder memoria. Para reemplazar esto por un motor Rust de alto rendimiento y exportación profesional, usar la futura conexión de backend (API v1).

## Arquitectura
```
src/
  main.rs          Entrypoint WebAssembly
  app.rs           UI y lógica de espacios, biblioteca, edición y radio
  media.rs         Modelos de audio, organización, descarga y almacenamiento
  recording.rs     MediaRecorder (micrófono)
  export.rs        Decodificación Web Audio, procesamiento PCM y encoder WAV Rust
  teleprompter.rs  Motor del lector automático
  api_contract.rs  Contratos tipados para plugin/backend futuro
assets/studio.css   Tema claro único y diseño full-canvas
backend/            Puente Axum HTTP opcional; sin plugin ni funciones simuladas
```

## Pruebas

`python tests/check_project.py` prueba la estructura del ZIP, HTML, CSS, dependencias de archivo y algunas regresiones de diseño. **No sustituye `cargo check`**: este entorno de creación no dispone de Cargo/Rust, así que la compilación real debe validarse en CachyOS antes de utilizar la aplicación.

## Integración con el motor VSMedia

Ver [INTEGRATION.md](docs/INTEGRATION.md): endpoints reales, ruta de almacenamiento, límites, seguridad del modo local y funciones pendientes de conectar con Minecraft.
