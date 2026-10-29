# Investigación de otros proyectos — 29/10/2026

Este documento reúne criterios de diseño. **No se han copiado módulos de terceros ni se declara compatibilidad interoperable** por estudiar una implementación.

## Audio y edición

- [Symphonia](https://github.com/pdeljanov/symphonia) — decodificación y demux modular. Evaluar usar el stack de codecs del proyecto en vez de crear nuestros decoders.
- [Rubato](https://docs.rs/rubato/latest/rubato/) — resampling FFT/sinc y algoritmos asíncronos para relojes de audio con drift. Reutilizar antes que escribir SRC propio. No integrado todavía.
- [rustfft](https://docs.rs/rustfft/latest/rustfft/) — FFT implementado por librería, **ya integrado** para obtener análisis espectral; Hann + escalado de amplitud en nuestro wrapper.
- [Audio EQ Cookbook (RBJ)](https://www.w3.org/TR/audio-eq-cookbook/) — diseño matemático clásico de biquads, utilizado para low-pass, high-pass, peaking EQ en el motor.
- [Plasmo Voice](https://github.com/plasmoapp/plasmo-voice) — referencia de enrutamiento eficiente de voz, evitar decodificaciones/copiados innecesarios en el servidor. No está incluido ni interoperable con VSClient.

## Imágenes

- [image-rs](https://github.com/image-rs/image) — escalado Lanczos3 y gestión de píxeles. **Ya integrada** para remuestreo offline de fotos a mosaicos.
- [resize](https://docs.rs/resize/latest/resize/) — referencia para precomputar matrices y reutilizar buffers cuando se redimensionan secuencias con tamaños iguales. Decidir después según benchmarks.
- Diferenciar pixel art (nearest) de fotografías (Lanczos3). Evitar resampling caro para GIFs que no necesitan alta calidad.

## Resource Packs

- La API de fusión por archivo permanece para binarios y documentos desconocidos.
- `assets/<namespace>/sounds.json`: por ser un mapa de eventos con claves identificables, fusionar claves independientes y resolver colisiones con prioridades explícitas.
- No fusionar arbitrariamente dos `pack.mcmeta` ni todos los `*.json`: sus esquemas no son equivalentes.
- Futuro: definir estrategias para `font/*.json` y assets nuevos con detección de conflictos reales de glifos; no concatenar arrays sin validación.

## Riesgos pendientes

- Benchmark de DSP y procesamiento masivo con carga real; las pruebas actuales son unitarias.
- Comparar licencias actuales y versiones antes de introducir nuevos crates.
- Garantizar que todas las llamadas complejas sean cancelables y fuera de los threads de región de Folia.
- Streaming/voz requieren cifrado, autenticación y adaptador VSClient aún no implementados.
