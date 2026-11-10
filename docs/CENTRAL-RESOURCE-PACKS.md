# VSMedia como distribuidor central de Resource Packs

**Integración inicial:** VSRender es el primer productor que delega el envío de Resource Packs en VSMedia. Los proveedores externos conservan la generación y su hosting HTTPS existentes; VSMedia registra sus URLs, UUID y hashes, decide las capas activas y envía un único plan desde el scheduler correcto de Folia. No hay compresión de ZIP ni conversión de modelos durante la conexión del jugador.

## API para otros plugins (sin dependencia de compilación)

El plugin puede llamar a `VSMediaPlugin.offerPluginResourcePacks(Plugin producer, UUID playerId, List<Map<String,String>> packs)`. Cada elemento de la lista tiene `uuid`, `url` HTTPS y `sha1`; puede incluir `sizeBytes`. El método valida todo el lote y devuelve **true** solo si VSMedia lo aceptó para entrega. En ese caso el plugin productor debe **omitir su propio envío**. En caso contrario sigue enviando sus packs de forma normal.

`withdrawPluginResourcePacks(Plugin producer)` retira solo las capas de ese productor. VSMedia nunca borra los packs ajenos de otros plugins.

- Límite global: 8 capas por jugador (incluido pack base configurable).
- Los clientes descargan y almacenan en caché cada capa por UUID/SHA-1; VSMedia **no descarga ni reempaqueta** los ZIP.
- El pack base se ofrece primero, después los productores en orden alfabético. Dos productores con la misma UUID no pueden coexistir.
- Los paquetes con URL HTTP, hash inválido o demasiadas capas se rechazan; el productor mantiene su ruta de envío de respaldo.
- La entrega final requiere que la URL HTTPS del productor sea accesible para los jugadores. El servidor 127.0.0.1:3060 del editor no es un host de Resource Packs público.
- Typewriter escucha eventos de estado de paquetes pero no gestiona su publicación, así que no requiere cambios en su flujo actual.
- Otros plugins ajenos deben usar este adaptador o aportar un mecanismo de integración compatible. No es posible interceptar de manera fiable la generación o distribución privada de cualquier plugin de terceros sin una API o eventos del proveedor.

## Estado de despliegue

La integración se ejecuta cuando VSMedia y el productor están instalados como plugins Folia/Paper actualizados. Es necesario compilar e instalar **ambos** JAR nuevos para evitar rutas duplicadas.
