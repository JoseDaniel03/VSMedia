# API Bridge opcional — Rust / Axum

Se ejecuta **independientemente** de la web y **no es necesario** para usar Home, VSAudio, VSMix o VSRadio localmente.

```bash
cargo run --manifest-path backend/Cargo.toml
```

Escucha en `127.0.0.1:3061`. `GET /api/v1/health` confirma que el puente está vivo y `GET /api/v1/capabilities` devuelve `false` para las funciones que dependen del plugin. Las peticiones de procesamiento/streaming devuelven HTTP **501 Not Implemented** hasta integrar el plugin. No hay autorización, subida ni broadcasting implementados, por lo que el puente se restringe a localhost.

Para comunicar la web desde un origen diferente, configurar un reverse proxy a `/api/v1/` del mismo origen y añadir autenticación antes de permitir acciones reales de producción.
