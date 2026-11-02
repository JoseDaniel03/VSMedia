# VSMedia 26.3 — JAR de prueba con Home y Audio Studio

El plugin de prueba de Folia se compila desde el código original de
`studio/`. El Home, VSAudio y VSMix no se recrean en Java ni se sustituyen
por páginas de demostración: CI compila el frontend Leptos a WebAssembly
con Trunk y lo incluye bajo `studio-web/` dentro del JAR.

Para Linux x86_64 también incluye `studio-native/vsmedia-studio-bridge`,
el motor Rust/Axum de audio. No se descarga software de terceros al iniciar
Minecraft. VSCore es un **subproceso** que el plugin lanza/cierra,
no una reimplementación de Rust en Java.

## Instalación CachyOS / CanvasMC / VSEngine 26.3

1. Minecraft debe estar usando Java 25 y un servidor compatible con el
   adaptador Folia de este repositorio. Crear una copia de seguridad.
2. Copiar el JAR de prueba compilado por GitHub Actions a `plugins/`.
3. Reiniciar el servidor. Verificar `/plugins` y `/vsmedia status`.
4. Entrar en `http://127.0.0.1:3070/` **desde la máquina que aloja
   Minecraft**: debe cargar el Home de VSMedia.
5. En las pestañas VSAudio y VSMix, cargar audio y probar efectos,
   proyectos y exportación. El indicador de VSCore debe figurar conectado.
6. `/vsmedia web` muestra la URL y `/vsmedia reload` recarga la
   configuración del plugin.

Si Minecraft se ejecuta en otra PC, no se debe abrir el puerto 3070 al
Internet público. Usar un túnel SSH desde la computadora local:

```bash
ssh -L 3070:127.0.0.1:3070 usuario@servidor
```

Luego abrir `http://127.0.0.1:3070/` localmente.

## Qué contiene y qué NO contiene

- Sí: plugin.yml, bootstrap VSMedia, listener Folia, `/vsmedia`,
  Home y editores VSAudio/VSMix originales, CSS/JS/WASM, servidor HTTP
  local 3070, proxy HTTP a VSCore 3061, binario Rust Linux x86_64.
- Sí: arranque/cierre automático del motor Rust integrado al JAR si
  `studio-core.autostart: true`, salvo que detecte un servicio 3061
  preexistente.
- No: binario Rust de Windows/macOS/ARM, entrega automática de assets de
  la web a Minecraft, CDN de Resource Packs, gestor nuevo de VSPack.
- VSRadio/VSStreams: sus pantallas originales siguen en la web, pero
  el servidor HTTP del JAR **todavía no implementa el proxy WebSocket**
  para VSRadio. No se debe dar por funcional la radio desde el JAR.
- El VSPack web actual es el módulo previo; el usuario enviará una
  interfaz nueva y se integrará después. La API VSPack Rust se conserva.

## Configuración y diagnóstico

Archivo: `plugins/VSMedia/config.yml`

```yaml
studio-web:
  enabled: true
  port: 3070
studio-core:
  autostart: true
```

Las interfaces web y Rust se sirven solo en `127.0.0.1`. El audio Rust
escucha en 3061; el plugin expone la misma API HTTP al navegador por
`/api/v1/*` en 3070, evitando el problema de que el navegador intente
contactar su propio puerto 3061 en otro equipo. En modo de desarrollo,
`studio/start.sh` sigue funcionando de manera independiente con
Trunk en 3060 y Axum en 3061.

El binario Rust extraído y sus datos están en
`plugins/VSMedia/vsmedia-core/` y
`plugins/VSMedia/vsmedia-data/`. Revisar
`plugins/VSMedia/vsmedia-core.log` si el indicador VSCore figura
sin conexión. En Linux, el binario se extrae como ejecutable solo para
el propietario del proceso Minecraft. No se necesita abrir puertos.

El plugin **no inicia Rust automáticamente** en sistemas que no sean Linux
x86_64. En ellos ejecutar el backend por separado:

```bash
cargo run --release --manifest-path studio/backend/Cargo.toml
```

## Compilación

La workflow `Checks` construye el frontend Trunk con sus assets,
compila Rust `vsmedia-studio-bridge` en Linux x86_64, descarga ambos
artefactos en el módulo Maven y construye el JAR con Shade. La misma
workflow verifica que el JAR incluya `studio-web/index.html`,
un WebAssembly, el ejecutable VSCore y la clase StudioWebServer.

Las pruebas automáticas verifican compilación y empaquetado; el arranque
en un servidor VSEngine/CanvasMC y la prueba de audio real siguen
requiriendo validación manual. No publicar el servidor HTTP en
0.0.0.0 ni exponerlo sin autenticación.
