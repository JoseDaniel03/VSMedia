# VSMedia Studio API v1 (conectado parcialmente)

El puente Axum local de este proyecto ahora utiliza realmente `vsmedia-core` y `vsmedia-formats`.

- `GET /api/v1/health`: estado real de almacenamiento, sin plugin Minecraft activo.
- `GET /api/v1/capabilities`: capacidades implementadas y no implementadas.
- `POST /api/v1/assets/{id}`: carga binaria, CAS SHA-256 y revisión.
- `GET /api/v1/assets/{id}`: lectura del recurso.
- `POST /api/v1/audio/projects/{id}`: guarda `.vsaudio` (JSON nativo).
- `GET /api/v1/audio/projects/{id}`: abre el proyecto.
- `POST /api/v1/audio/projects/{id}/commands`: modifica proyecto usando comandos de `AudioStudio` de Rust; requiere SHA-256 esperado.
- `POST /api/v1/media/jobs`: 501, los jobs multimedia remotos aún no están conectados.
- `POST /api/v1/radio/sessions`: 501, no se simula la transmisión de radio.

Para producción será necesario crear sesiones autenticadas, compare-and-swap transaccional, cuotas por productor, streaming de archivos en vez de cargar el cuerpo completo en memoria y un frontend servido detrás de HTTPS. No abrir el bridge de desarrollo a Internet.
