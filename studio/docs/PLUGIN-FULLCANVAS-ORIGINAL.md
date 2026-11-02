# VSMedia: FullCanvas original dentro del plugin Folia

La web empaquetada en el JAR se compila **sin cambiar** los archivos de UI originales importados en el commit `2512dfc253977ffb6cfb45f4aa0dcd05c3a4b0fb`. El CI verifica los hashes Git del Home/Studio, CSS, HTML y cliente API. No hay una segunda web ni una pantalla de demostración.

## Arquitectura

- `VSMedia.jar` arranca un servidor de archivos estáticos de solo lectura en **127.0.0.1:3060** que expone el frontend original compilado con Trunk (Leptos/WASM).
- El mismo plugin extrae y ejecuta el backend **Rust/Axum VSCore** en Linux x86_64, en **127.0.0.1:3061**, junto con el almacenamiento de proyectos. Al deshabilitar el plugin, cierra el proceso que haya creado.
- El cliente original ya consume `http://127.0.0.1:3061/api/v1` y WebSocket `ws://127.0.0.1:3061/api/v1/live/ws`. Esos endpoints **no se modificaron**. El servidor del plugin también tiene proxy HTTP local `/api/v1/*`, pero los URLs absolutos de la web original siguen apuntando al 3061.
- **No hay un segundo frontend**. En CachyOS basta con instalar el JAR, ejecutar VSEngine y abrir `http://127.0.0.1:3060` en esa misma PC. No hay que arrancar `studio/start.sh` ni instalar Trunk.

## Instalación

1. Usar VSEngine/CanvasMC 26.3 compatible con Folia, Java 25 y Linux x86_64.
2. Quitar JARs viejos de VSMedia, instalar el nuevo artefacto `VSMedia-Folia-26.3-Original-FullCanvas-JAR` en `plugins/`.
3. Iniciar el servidor; ejecutar `/vsmedia status`, `/vsmedia web`.
4. Abrir `http://127.0.0.1:3060`. VSAudio puede guardar proyectos en el almacenamiento del motor Rust por medio de la API existente.

La web y el motor escuchan en *loopback*. Para usar la web desde otra PC, abrir túneles SSH **para ambos puertos**: `ssh -L 3060:127.0.0.1:3060 -L 3061:127.0.0.1:3061 usuario@servidor`. No exponer 3060/3061 sin autenticación.

## Límites

La versión del frontend es la del ZIP importado originalmente. Las funciones agregadas al frontend por commits posteriores no se conservaron porque alteraban el frontend original. El backend Rust y VSPack actual no fueron reemplazados. VSRadio usa WebSocket **directo** al motor Rust, no el proxy HTTP del plugin. En Windows/macOS/ARM hay que iniciar manualmente el backend Rust; el binario embebido es Linux x86_64. No se afirma que haya una integración de emisión Minecraft/radio cuando el backend indica que no existe.

## Desarrollo

`studio/start.sh` sigue disponible para desarrolladores, pero **no forma parte del arranque del JAR**. Si se usa, no ejecutarlo a la vez que el plugin porque ambos utilizan los puertos 3060/3061.
