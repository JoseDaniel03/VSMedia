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

`GET /api/v1/live/ws` abre WebSocket de señalización **sólo para
radio** (roles emisor/oyente; máximo 16 conexiones por sala).
Los pedidos de salas `voice` se rechazan: VSStreams utiliza Plasmo
Voice como motor de proximidad dentro de Minecraft. El backend sólo
intercambia oferta, respuesta y candidatos ICE, sin transportar PCM ni
grabar audio. Las salas son efímeras y se vacían al desconectar.

`GET /api/v1/live/status` presenta métricas agregadas (cantidad de salas,
participantes y límites), sin códigos de sala.
`GET /api/v1/streams/integration` informa si el backend está
conectado al servidor Plasmo Voice; en la etapa actual devuelve
`server_connected: false` y no realiza operaciones administrativas.

El sistema no es un servidor de streaming masivo y no existe sesión de
radio Minecraft habilitada: `POST /api/v1/radio/sessions` y
`POST /api/v1/media/jobs` siguen devolviendo HTTP 501.

**Seguridad:** bind loopback, Origin local aceptado para la página web,
límite de 48 KiB de señal y de 128 conexiones totales. Los códigos de sala
no son autenticación. Para producción exigir HTTPS/WSS, auth, TURN, gestión
de invitaciones y controles de abusos en el reverse proxy.

Más información en `../docs/LIVE-STREAMING.md`.

## VSPack

El servidor local también expone un gestor de Resource Packs sobre
`/api/v1/packs`. Implementa importación ZIP acotada, merge estructurado
con prioridades y vista previa de conflictos, además de exportación
determinista binaria. Usa `vsmedia-pack`, no un segundo compresor.
Se mantienen packs importados en memoria limitada hasta reiniciar
el bridge. El gestor no almacena ni publica al Minecraft remoto.

Ver `../docs/VSPACK.md` para límites, API y pruebas.
