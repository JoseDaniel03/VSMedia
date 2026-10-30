# API local VSMedia Studio v1

El backend Rust/Axum en `127.0.0.1:3061` integra VSCore y el renderer
nativo VSAudio/VSMix.

| Método y ruta | Función |
|---|---|
| GET /api/v1/health | Estado local de backend y almacenamiento |
| GET /api/v1/capabilities | Capacidades reales, incluido WebRTC signaling |
| POST /api/v1/assets/{id} | CAS SHA-256 y revisión |
| GET /api/v1/assets/{id} | Obtener fuente |
| POST /api/v1/audio/projects/{id} | Guardar documento `.vsaudio` |
| GET /api/v1/audio/projects/{id} | Leer documento nativo |
| POST /api/v1/audio/projects/{id}/commands | Editar con SHA esperado |
| POST /api/v1/audio/render | Render de borrador WAV, sin guardar proyecto |
| GET /api/v1/live/status | Conteos globales anónimos de salas WebRTC |
| GET /api/v1/live/ws | Upgrade a WebSocket sólo para radio P2P |
| GET /api/v1/streams/integration | Estado comprobable de integración Plasmo Voice (aún desconectado) |
| POST /api/v1/media/jobs | HTTP 501: plugin aún no conectado |
| POST /api/v1/radio/sessions | HTTP 501: broadcast Minecraft no conectado |

## Protocolo WebSocket

Cliente: `{"type":"join","room":"radio_abcd1234","mode":"radio","role":"listener"}`.
Responde `joined` con ID de sesión y pares actuales; emite
`peer_joined` o `peer_left`. Para intercambiar WebRTC:
`{"type":"signal","to":123,"kind":"offer","data":{"type":"offer","sdp":"..."}}`,
o `answer` y `candidate`. El receptor obtiene `signal` con `from`.

La sala radio exige `host` o `listener`; sólo `host` puede ofrecer y
sólo `listener` puede responder. Las salas `voice` se rechazan:
la voz por proximidad la provee Plasmo Voice (servidor/mod).
Axum no retransmite PCM ni persiste audio.

**No exponer el backend local directamente a Internet**: HTTPS/WSS,
autenticación, TURN, control de permisos y auditoría son requisitos para
un despliegue remoto seguro. Véase `LIVE-STREAMING.md`.
