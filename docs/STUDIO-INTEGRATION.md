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

## Biblioteca de proyectos y edición segura (fase actual)

El backend ofrece ahora catálogo `GET /api/v1/audio/projects`, metadatos de
proyecto y fuentes, y escritura condicional por SHA-256 dentro de la transacción
SQLite. Se agregaron pruebas HTTP de extremo a extremo para creación, listado,
lectura, edición y prevención de conflictos de revisión.

VSAudio guarda y abre una versión de clip. VSMix puede guardar y recuperar
varios clips por pista, con posición, inicio de fuente, duración, volumen,
paneo y fades independientes; el modelo sigue siendo compatible con sesiones
anteriores de una única fuente por pista. Los audios se descargan desde el CAS y sus SHA-256 se comparan
con el manifiesto antes de incorporarlos al navegador.

La UI usa un catálogo de proyectos explícito; no sube los audios al importar
archivos locales. Abrir un proyecto crea una copia de trabajo en el navegador:
una nueva operación **Guardar** crea otro proyecto, no modifica sin aviso la
revisión previa. La edición remota con comandos condicionales existe en Axum,
pero todavía no se expusieron todos esos controles en la web.

## Previsualización sin guardar proyecto

Las interfaces VSAudio y VSMix incluyen **▶ Motor (borrador)**, que
compone un manifiesto temporal y lo envía a
`POST /api/v1/audio/render`. El backend no publica ninguna revisión
`.vsaudio`, pero sí puede importar de manera explícita las fuentes locales
al CAS cuando todavía no existen allí. El procesamiento Rust real devuelve
un WAV PCM16 estéreo de 48 kHz, solo para fuentes WAV.

El editor multipista almacena varios fragmentos por canal en el mismo
formato nativo. Los audios repetidos se suben una sola vez durante la
composición del proyecto y la API reutiliza la revisión vigente si se
recibe el mismo SHA-256. Todavía quedan pendientes los códecs comprimidos,
las operaciones de arrastre visual y la reproducción nativa en vivo.
