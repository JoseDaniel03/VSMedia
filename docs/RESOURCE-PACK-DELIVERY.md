# VSMedia — Distribución rápida de Resource Packs (fase 1)

## Idea y límites de la mejora

Minecraft Java descarga los Resource Packs desde una URL HTTP(S). El servidor
de juego transmite la **URL**, el UUID de pack y el SHA-1 del ZIP, no cada byte.
Cuando Minecraft ya tiene un pack con el mismo SHA-1 en su caché, puede evitar
volver a descargarlo.

**Esto no hace que una primera descarga sea instantánea.** La velocidad real
depende del tamaño del pack, ancho de banda, latencia, CDN, ubicación geográfica
y tiempo de aplicación local del pack en el cliente.

VSMedia aplica cuatro mejoras sin modificar el cliente vanilla:

1. Empaquetado determinista: mismos recursos → mismos bytes → mismo SHA-1.
2. Evitar comprimir de nuevo PNG/JPEG/WebP/OGG y otros medios ya comprimidos.
   Los archivos JSON siguen usando DEFLATE. Esto ahorra trabajo del servidor
   y puede ayudar en el tiempo de compilación; no siempre reduce más bytes.
3. Publicación inmutable: cada ZIP se almacena como
   `<sha256>.zip`. Si no cambia, la URL no cambia.
4. SHA-1 de Minecraft y UUID estable por ID lógico de pack. El primer hash
   es para caché de Minecraft; SHA-256 es para integridad de VSMedia.

## Preparación desde la CLI (solo infraestructura)

Cuando ya exista un Resource Pack ZIP válido:

```bash
cargo run -p vsmedia-engine -- pack-stage \
  ./resourcepack.zip \
  /srv/vsmedia/public-packs \
  https://packs.example.com/ \
  virgoland
```

El comando imprime un descriptor JSON con `uuid`, `sha1`, `sha256`,
`size_bytes`, `filename` y `url`. No inicia un HTTP listener, no sirve el
ZIP por el puerto IPC y no publica ningún release en GitHub. Es una herramienta
para el futuro entorno de producción, no un artefacto final entregado ahora.

Nunca publicar secretos del Core, tokens, proyectos internos o archivos
`.vsbundle` en el directorio web. Solo los packs explícitamente aprobados.

## Ejemplo de configuración HTTPS: Caddy

Un servidor separado puede servir el directorio estático. Este ejemplo se
utiliza **cuando el administrador ya dispone de dominio y DNS**.
Sustituir los valores de dominio y ruta.

```caddyfile
packs.example.com {
    root * /srv/vsmedia/public-packs

    @packs path_regexp versioned ^/[0-9a-f]{64}\.zip$
    handle @packs {
        header Content-Type "application/zip"
        header Cache-Control "public, max-age=31536000, immutable"
        file_server
    }

    respond 404
}
```

Caddy gestiona HTTPS, la transferencia estática y respuestas HTTP de archivos.
No se debe enviar una ZIP con Content-Encoding adicional: ya contiene sus
propias entradas comprimidas. Un proxy/CDN puede almacenar las URLs inmutables
por largos períodos. Configurar las rutas públicas solamente para estos ZIPs.

**Seguridad:** HTTP público únicamente en el dominio dedicado de packs;
VSMedia IPC permanece en 127.0.0.1 y no debe exponerse jamás a Internet.

## Adaptador Folia / VSEngine (fuente, sin plugin final)

Cuando se incorpore el plugin, al entrar el jugador:
- Comprobar el pack actual del servidor. No enviar repetidamente la misma versión.
- Enviar URL HTTPS, UUID y SHA-1 **de 20 bytes** usando la API Paper/Folia.
- Manejar respuestas de aceptación, descarga, aplicación o fallo sin bloquear
  los schedulers regionales. No ejecutar HTTP o compresión en esos threads.
- Si varios packs se apilan, gestionar IDs/orden de forma estable; no generar
  un UUID aleatorio por cada conexión.

El SDK Java incluye `ResourcePackDelivery`, tipo independiente de Bukkit que
valida URL y SHA-1 antes de construir la oferta del pack.

## Pendiente, no declarar implementado

- No hay servidor HTTP público dentro del proceso Rust; se usa Caddy/Nginx/CDN.
- Ya existe un adaptador Java fuente para Folia 26.3 y un coordinador de ofertas por jugador, pero todavía no se ha registrado como plugin ejecutable ni probado con jugadores reales.
- No hay patch/delta nativo de Resource Packs vanilla: una actualización con un
  SHA-1 diferente generalmente requiere bajar el ZIP completo actualizado.
- No se han medido descargas reales, tiempos de aplicación o rendimiento CDN.
- La distribución en capas ya permite particionado por prefijos explícitos de assets; faltan reglas automáticas y reconciliación de capas eliminadas.
- La gestión de datapacks sigue aplazada hasta la última etapa del proyecto.

## Coordinación de entrega por jugador

El módulo fuente Java `vsmedia-folia-adapter` usa Paper API 26.3, compila
con Java 25 y se construye en CI sin empaquetar un plugin. Separa responsabilidades:
`ResourcePackSessionCoordinator` gestiona estados por jugador y UUID;
`FoliaResourcePackController` envía la oferta mediante
`player.getScheduler().run(...)`, y escucha `PlayerResourcePackStatusEvent`
y `PlayerQuitEvent`. La descarga real de bytes continúa delegada al
servidor HTTPS estático o CDN. No se ejecuta E/S de archivos ni red
sobre los hilos regionales.

El cliente Java puede interpretar el JSON administrativo generado por el
motor Rust mediante `ResourcePackDescriptorReader`, que valida hashes,
tamaño y URL de publicación. No se debe dar por válida una publicación que
todavía no esté accesible desde Internet.

**Límites:** el estado se conserva solo durante la sesión y hasta 4096
asociaciones jugador/pack; las respuestas de estados provienen del cliente y
no prueban integridad criptográfica. Los eventos de versiones distintas que
reutilizan UUID pueden ser ambiguos, porque el protocolo no incluye el SHA-1
en los callbacks. El flujo completo debe probarse con una instancia Paper/Folia
26.3 y clientes reales antes de activarlo en Virgoland.

## Particionado del Resource Pack por capas

Para reducir actualizaciones grandes, VSMedia puede separar un pack ya
fusionado en un **base** relativamente estático y uno o varios **overlays**
de cambios frecuentes. Se configura con reglas de prefijos de rutas:

```rust
let rules = vec![OverlayRule {
    logical_id: "ui".into(),
    prefixes: vec!["assets/virgoland/textures/ui/".into()],
}];
let descriptors = compile_and_stage_layers(
    &merged, "virgoland", &rules, limits, public_dir, public_url
)?;
```

El módulo coloca cada asset en una sola capa, conserva `pack.mcmeta` en
cada una, valida los nombres y prioriza el prefijo más específico. Las
capas vacías no se publican. El ejemplo no genera un paquete final para
descargar al usuario: es código fuente de la futura canalización.

El cliente Minecraft recibe estos packs como una pila mediante Paper
`addResourcePack`. Si cambia únicamente la interfaz, el SHA-1 y la URL
del pack base **permanecen iguales**; solo el overlay requiere descarga
actualizada. Esta invariancia tiene pruebas automáticas de Rust.

No hay aún selección automática del mejor particionado por frecuencia de
cambios ni un servidor/CDN publicado desde VSMedia. La mejora de velocidad
real dependerá del patrón de cambios y de la red del jugador.
