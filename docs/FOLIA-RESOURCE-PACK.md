# VSMedia — Integración Resource Pack con Folia 26.3

## Estado y arquitectura

Existe un módulo **fuente** `java/vsmedia-folia-adapter` que compila contra Paper
API 26.3 como dependencia `provided`. No crea ni distribuye todavía un plugin
independiente, JAR final, ni `plugin.yml`. Se incorporará al bootstrap VSMedia
cuando esté listo el resto de engines.

El módulo usa **Java 25**, que exige Paper 26.3. El SDK Java común conserva
`maven.compiler.release=21`, pero las comprobaciones de todos los módulos
se ejecutan con JDK 25 en GitHub Actions.

## Uso desde el futuro bootstrap

El plugin VSMedia debe registrar un solo controlador, después de tener un
descriptor HTTPS publicado y accesible:

```java
var coordinator = new ResourcePackSessionCoordinator();
var controller = new FoliaResourcePackController(plugin, coordinator, true);
controller.setCurrent(delivery);
Bukkit.getPluginManager().registerEvents(controller, plugin);
// Deshabilitado del plugin: controller.close();
```

El `delivery` proviene del archivo JSON producido por `pack-stage`, validado
por `ResourcePackDescriptorReader.parse(bytes)`. El controlador no realiza
operaciones de red ni descomprime ZIP en los threads Folia. Para aplicar
cambios a un jugador conectado se invoca `offerTo(player, false)`, que
programa la oferta en el **EntityScheduler** de ese jugador.

El evento de unión ofrece la versión vigente; el evento de estado sigue
los resultados y el de salida limpia el estado del jugador. Se evita enviar
la misma versión si ya está aplicada o descargándose. Los fallos o rechazos
no se reintentan automáticamente: `offerTo(player, true)` permite reintentar
en forma explícita. Al actualizar el SHA-1 se puede ofrecer la nueva versión.

## Distribución por capas (opcional)

El controlador admite un pack único con `setCurrent(delivery)` o una pila
ordenada mediante `setCurrentStack(new ResourcePackStack(List.of(base, overlay)))`.
En el segundo modo utiliza `Player.addResourcePack(...)` en el scheduler
de la entidad. Minecraft 26.3 puede mantener una capa base estable en caché
mientras un overlay más pequeño cambia con frecuencia.

El orden de la lista importa: primero el pack base y después las capas
de mayor prioridad. El SDK limita la pila a ocho capas y rechaza UUIDs
o IDs lógicos repetidos. El Pack Engine ahora ofrece una separación **explícita por prefijos de assets**
con `vsmedia-pack::layers::split_into_layers(...)`. Al publicar, se usa
`delivery::compile_and_stage_layers(...)` para generar descriptores por capa.
La selección automática óptima de capas continúa pendiente.

Al modificar la composición de la pila (eliminar capas o cambiar el orden),
el futuro bootstrap debe reconciliar/remover los packs que ya no correspondan.
Este primer adaptador solo añade y actualiza las capas deseadas.

## Limitaciones

- Debe registrarse desde un plugin real marcado compatible con Folia.
- Todavía no se ha probado contra un servidor y cliente Minecraft reales.
- Las respuestas del cliente no son prueba criptográfica.
- Los eventos que conservan el mismo UUID para versiones diferentes no
  incluyen SHA-1; un callback tardío puede ser ambiguo.
- El alojamiento HTTPS, la supervisión y la medición de velocidades son
  infraestructura separada y aún pendientes.
- Los datapacks se mantienen fuera de alcance hasta la última etapa.

Fuente de compatibilidad: https://docs.papermc.io/paper/dev/folia-support/
API de packs: https://docs.papermc.io/adventure/resource-pack/

## Eliminación de overlays obsoletos

`ResourcePackSessionCoordinator.obsolete(player, desired)` identifica UUIDs
que VSMedia ofreció antes y ya no están en la pila vigente. Dentro del
EntityScheduler, el adaptador llama a `Player.removeResourcePack(uuid)` y
solo olvida ese estado cuando la llamada se realiza correctamente. No
elimina Resource Packs de otros plugins ni los instalados por el usuario.
