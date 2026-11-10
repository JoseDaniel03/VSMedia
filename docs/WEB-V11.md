# VSMedia Web v11 y comando único

Este repositorio conserva Rust Core, APIs Java/Folia y el frontend Leptos en `studio/`. La **web v11**, basada en Node/HTML/JS con VSAudio, VSMix, VSRadio, VS Pack, VS Pixel Studio y Blockbench, se coloca por separado en `web/`.

## Instalar en CachyOS / Linux

```bash
./vsmedia import-web ~/Descargas/VSMedia-Studio-2026-Tamano-Optimizado-v11.zip
./vsmedia install
vsmedia web
```

Se abre `http://127.0.0.1:3060`. Comandos: `vsmedia web --port 3050`, `vsmedia web --no-browser`, `vsmedia web --build`, `vsmedia doctor`.

La web v11 usa Node y FFmpeg opcional para `/api/audio/*`, sin recompilar Blockbench en cada arranque. El backend Rust/Axum anterior continúa en `studio/backend/` (3061) y el plugin Folia conserva su frontend original; **la web v11 todavía no sustituye ni conecta automáticamente esos endpoints**.

Para publicar la web completa, después de importarla, revisá `git status` y ejecutá `./vsmedia publish-web`: crea un commit solo con `web/` y hace push sin `--force`. Es necesario tener permisos de escritura y el clon actualizado.
