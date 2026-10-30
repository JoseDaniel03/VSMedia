# VSMedia Studio — Bridge Axum local

Iniciar desde `studio/`:

```bash
cargo run --manifest-path backend/Cargo.toml
```

El backend queda disponible sólo en `127.0.0.1:3061` y reutiliza la
base SQLite/CAS del motor VSMedia. La web Trunk corre en
`127.0.0.1:3060`. Los endpoints existentes admiten importación y
render WAV de proyectos `.vsaudio`, comandos de edición y previsualización
sin crear una revisión.

## Señalización WebRTC

`GET /api/v1/live/ws` abre un WebSocket de señalización para salas de
`radio` (roles emisor/oyente; máximo 16 conexiones) o `voice`
(participantes con micrófono; máximo 8 conexiones). El backend sólo
intercambia oferta, respuesta y candidatos ICE, sin transportar PCM ni
grabar audio. Las salas son efímeras y se vacían al desconectar.

`GET /api/v1/live/status` presenta métricas agregadas (cantidad de salas,
participantes y límites), sin códigos de sala.

El sistema no es un servidor de streaming masivo y no existe sesión de
radio Minecraft habilitada: `POST /api/v1/radio/sessions` y
`POST /api/v1/media/jobs` siguen devolviendo HTTP 501.

**Seguridad:** bind loopback, Origin local aceptado para la página web,
límite de 48 KiB de señal y de 128 conexiones totales. Los códigos de sala
no son autenticación. Para producción exigir HTTPS/WSS, auth, TURN, gestión
de invitaciones y controles de abusos en el reverse proxy.

Más información en `../docs/LIVE-STREAMING.md`.
