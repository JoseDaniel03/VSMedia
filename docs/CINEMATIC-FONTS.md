# Fuentes de VSCinematic

El backend Studio añade un catálogo persistente para VSCinematic 0.2. El editor importa Google Fonts mediante su plugin, envía TTF/OTF y licencia, y VSMedia valida el archivo con ttf-parser antes de guardarlo en SQLite/CAS del namespace studio (`fonts/SHA256.json`, kind Font).

| Método | Ruta | Función |
|---|---|---|
| GET | `/api/v1/fonts` | Defaults minecraft:default/uniform/alt, providers de los packs importados y fuentes persistidas |
| POST | `/api/v1/fonts/import` | `{family,source,data,license?}`; data base64, máximo 8 MiB de binario |
| GET | `/api/v1/fonts/file?font=namespace:id` | TTF directo para preview; bitmap/unihex/reference sin TTF devuelven 404 |
| GET | `/api/v1/packs/export` | Pack compuesto, incluyendo fuentes persistidas |

Límite 256 fuentes y 96 MiB de documentos en CAS. WOFF/WOFF2 no son aceptados como TTF. Los nombres del usuario no se convierten en rutas: el ID es `vsmedia:imported/SHA256`. No se modifica minecraft:default; cada provider importado referencia el default como fallback para glifos ausentes. Los archivos y providers quedan en `assets/vsmedia/font/imported/`, el origen en `font_sources/` y la licencia en `licenses/`.

El catálogo usa los packs cargados en la sesión del Studio más las fuentes persistidas. Los ZIP de origen siguen siendo temporales; las fuentes importadas sobreviven reinicios. Sin un pack base, se genera uno de fuentes para Minecraft 26.3 (resource format 97.1). Con un pack base, se conserva su metadata. La exportación no equivale a que un jugador ya haya aceptado o cargado el pack.

El backend escucha loopback, puerto 3061 por defecto. `VSMEDIA_BIND` permite otra dirección **loopback** para pruebas. `VSMEDIA_DATA_DIR` configura el CAS. VSCinematic consulta este API desde su servidor, sin abrir CORS adicional, y ofrece la exportación al jugador mediante Paper.

El plugin VSMedia existente empaqueta este ejecutable Rust. Para actualizarlo, usar la CI Checks (artefacto de plugin) o reconstruir el backend y el JAR con los recursos nativos actualizados. No reemplazar un plugin en ejecución sin reiniciarlo.
