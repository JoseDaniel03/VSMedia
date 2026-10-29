# Studio FullCanvas — integración de fuentes

Se incorporó la web del archivo proporcionado por el administrador como un
paquete de **código fuente** bajo `studio/`, sin sustituir ni duplicar los
engines existentes.

- Frontend: Leptos + WebAssembly (`studio/Cargo.toml`), diseño claro full-canvas,
  biblioteca, VSAudio, VSMix, VSRadio y grabación local.
- Backend: Axum local (`studio/backend/Cargo.toml`) conectado a
  `vsmedia-core`, `vsmedia-formats` y `AudioStudio` de Rust.
- Comando de desarrollo: `cd studio && ./start.sh` (puertos 3060/3061).
- Publicación explícita desde VSAudio: audio en CAS y revisión nativa `.vsaudio`.
- Los endpoints de radio y emisión Minecraft aún responden 501; no se simula
  una función remota inexistente.
- No activar este servidor local sin autenticación en una IP pública.

Ver `studio/docs/INTEGRATION.md`. No se publican JAR ni releases.

**Verificación pendiente:** Cargo, Trunk y pruebas integradas de navegador.
