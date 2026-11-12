# VSMedia → Minecraft: audio y tipografías

Esta tanda agrega la publicación **dentro del ZIP** de sonido OGG Vorbis y fuentes
TTF/OTF y controles de títulos, chat y audio para el plugin Folia.
**Agregar recursos al ZIP no los distribuye automáticamente al jugador.**
La publicación HTTPS y aplicación efectiva del resource pack siguen siendo
un paso operativo separado hasta configurar R2 y desplegar el watcher del
plugin. No se generan JARs en esta tanda.

## Desde VSAudio, VSMix o VSRadio

1. Exportá un audio o una mezcla desde su editor (por ejemplo WAV).
2. Abrí **✦ Publicar a Minecraft** (disponible como acceso contextual).
3. Elegí el archivo, el identificador (ej. `intro`) y **Agregar sonido al pack**.
   El navegador convierte WAV/MP3/FLAC/WebM a OGG Vorbis con el codec compilado
   por `scripts/build-browser-vorbis.sh`; Minecraft/Folia no codifica audio.
   También podés subir un archivo OGG Vorbis existente.
4. Consultá los sonidos guardados y elegí **Exportar Resource Pack ZIP**.
5. Publicá el ZIP en una URL HTTPS accesible desde los clientes de Minecraft.
   Actualizá `resource-pack.url`, `sha1`, `uuid` y `size-bytes` en
   `plugins/VSMedia/config.yml`, y ejecutá `/vsmedia reload` y
   `/vsmedia send <jugador>` para volver a ofrecerlo.
6. El cliente debe aceptar/descargar/aplicar el nuevo pack para poder oír
   sonidos personalizados. El identificador publicado será
   `vsmedia:published.intro`, registrado en `assets/vsmedia/sounds.json`.

Los archivos están en CAS SQLite y sobreviven reinicios del bridge.
Se insertan en `assets/vsmedia/sounds/published/*.ogg` del ZIP unificado.
No se sobreescriben eventos de sonido ajenos en un `sounds.json` válido.
Máximo de 256 archivos y 192 MiB de sonidos publicados; 16 MiB por OGG.
La cabecera Vorbis se valida de forma básica (no decodifica todos los frames).

### Comandos del plugin

Los subcomandos de control requieren `vsmedia.admin` (OP por defecto) y
se ejecutan en el scheduler de la entidad del jugador.

```text
/vsmedia play <jugador|*> <id-sonido> [volumen] [tono]
/vsmedia stopsound <jugador|*> <id-sonido>
/vsmedia title <jugador|*> <fuente> <color> <texto...>
/vsmedia subtitle <jugador|*> <fuente> <color> <texto...>
/vsmedia preview <jugador|*> <fuente> <color> <texto...>
/vsmedia chatfont <jugador> <fuente|default>
```

Ejemplos:

```text
/vsmedia play Dani vsmedia:published.intro 1 1
/vsmedia title Dani vsmedia:imported/ID_REAL #D6B771 Bienvenidos a Virgoland
/vsmedia subtitle * minecraft:default gold Apertura en cinco segundos
/vsmedia preview Dani minecraft:uniform white Verificando la fuente
/vsmedia chatfont Dani minecraft:uniform
/vsmedia chatfont Dani default
```

No se ocupa `/title`, porque ya es un comando de Minecraft. Usamos
`/vsmedia title` con fuente Namespaced Key, color hexadecimal o nombre
de color, y jugador explícito. El comando `subtitle` muestra un
subtítulo con title vacío. `chatfont` afecta al **texto que envía el
jugador configurado**, no todo el HUD, ni el cuadro de escribir del
cliente. Se restablece al desconectar.

## Tipografías

En la pestaña **Tipografías** de Minecraft podés importar un TTF/OTF.
Se guarda en el catálogo persistente y su definición JSON en el pack
final. En **Jugador** elegís una fuente, ves el TTF en la web cuando
está disponible y enviás una previsualización real como title/subtitle.

Ejemplo de fuente importada:
`vsmedia:imported/<sha256 del TTF>`

Esta función no cambia automáticamente `minecraft:default`.
Para cambiar **toda** la tipografía vanilla del chat/menús se necesita
un override explícito del `minecraft:default` o un mod de cliente,
con posible conflicto con otros packs; no se hace silenciosamente.

## API local

| Método | Ruta | Efecto |
|---|---|---|
| `POST` | `/api/v1/sounds/{id}` | Persiste OGG Vorbis en CAS y registra evento |
| `GET` | `/api/v1/sounds` | Enumera eventos publicados y sus hashes |
| `POST` | `/api/v1/fonts/import` | Persiste TTF/OTF |
| `GET` | `/api/v1/fonts` | Catálogo de fonts |
| `GET` | `/api/v1/packs/export` | ZIP combinado con sonidos + fonts |
| `POST` | `/api/v1/minecraft/action` | Control local de jugador, devuelve 202 programado |

El endpoint de control sólo existe en el servidor HTTP **loopback**
empaquetado en el plugin. Requiere `Origin` local, el header
`X-VSMedia-Control: 1` y body form-urlencoded con acciones permitidas.
No es un endpoint para exponer directamente a Internet ni una prueba
de que un jugador haya reproducido un sonido. El API de VSCore local
3061 no tiene acceso a la API Bukkit.

## Qué sigue sin estar terminado

- **Distribución automática**: ya existe un publicador externo de packs
  con perfiles de optimización, separación por tamaño, hashes, R2 y watcher
  HTTPS en Folia en la rama de desarrollo. **Todavía es necesario configurar
  una cuenta real R2, desplegar una futura compilación del plugin y comprobar
  aceptación real en los clientes**. Guía: [VSPack Auto Publish](VSPACK-AUTO-PUBLISH.md).
- **Publicación con un solo clic desde el proyecto abierto**: el panel usa
  el archivo exportado, porque la sesión VSMix sigue siendo un borrador
  del navegador. No se declara publicado un WAV sin convertirlo.
- **VSRadio dentro de Minecraft**: por ahora WebRTC funciona entre
  navegadores; el bridge no tiene transporte de radio hacia los clientes
  de Minecraft. `/api/v1/radio/sessions` sigue devolviendo 501,
  intencionalmente en vez de simular una transmisión.
- **Confirmación de reproducción**: el servidor sólo programa el packet
  de sonido/título. Aceptación del pack y estado exacto de ejecución
  necesitan telemetría del cliente.
- **Seguridad remota**: todo el Studio está diseñado para localhost.
  Un proxy público exigiría autenticación, permisos y protección CSRF
  por sesión, no sólo el header local actual.

## Compilación y controles

```bash
bash scripts/build-browser-vorbis.sh
cargo test --manifest-path studio/backend/Cargo.toml --all-targets
node --check studio/assets/minecraft.js
cd studio && trunk build --release
mvn -B -f ../java/pom.xml verify
```

El navegador y la ejecución dentro de un cliente Minecraft real
requieren pruebas manuales además de GitHub Actions.
