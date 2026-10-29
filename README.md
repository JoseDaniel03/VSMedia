# VSMedia Remake 26.3

Motor multimedia modular para VSEngine/CanvasMC y servidores Folia.

> **Estado:** nueva implementación desde cero (fase de foundations). Todavía no es un plugin instalable ni un motor de audio/voz operativo.

## Arquitectura

- **VSMedia Core (Rust):** catálogo de assets, almacenamiento, versiones y servicios compartidos.
- **Pack Engine (Rust):** fusión y validación de recursos de otros productores.
- **Audio Engine (Rust):** modelos de edición, mezcla PCM, streaming y voz (estos últimos se implementarán por etapas).
- **Image Engine (Rust):** procesado y división de imágenes en mapas.
- **Font Engine (Rust):** glifos y definiciones de fuentes.
- **API pública:** contratos Rust y Java para consumidores externos, sin dependencias inversas.
- **Bridge Folia/CanvasMC (pendiente):** único componente con acceso a la API del servidor.
- **VSClient (proyecto separado):** cliente opcional futuro para streaming/voz; los sonidos de Resource Pack, mapas y glifos funcionan con cliente vanilla.

## Desarrollo

Requiere toolchain estable Rust; para los contratos Java, JDK 21 o superior y Maven.

```bash
cargo test --workspace
cargo run -p vsmedia-engine -- status
```

Ver [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) y [docs/ROADMAP.md](docs/ROADMAP.md).

**Regla:** otros plugins publican o consumen recursos a través de VSMedia; VSMedia no importa clases de VSRender, Typewriter ni VSUiRemake.
