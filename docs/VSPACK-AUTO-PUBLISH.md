# VSPack: optimización, publicación y distribución automática (26.3)

Estado: **implementado en rama de desarrollo**, pendiente de configurar Cloudflare R2, desplegar una futura versión del plugin y validar con clientes reales. **En esta tanda no se generan JARs.**

## Componentes incluidos

- Exportar el Resource Pack unificado desde VSCore local (GET /api/v1/packs/export): incluye sonidos OGG y fuentes.
- Verificar ZIP (rutas seguras, metadatos, tamaño máximo expandido, duplicados, enlaces).
- Minificar JSON y MCMeta sin alterar contenido binario.
- Optimización opcional con PackSquash instalado de forma externa: PNG/OGG y otros recursos según compatibilidad de su versión. Si no está disponible o falla, se informa y se preservan los originales. La opción --require-packsquash impide una publicación sin el optimizador.
- Separación por categorías y división de grupos grandes para no superar el límite configurado por ZIP (90 MiB por defecto, admite hasta 250 MiB).
- Cada evento de sounds.json se agrupa con los OGG a los que referencia; las animaciones PNG mantienen su archivo .png.mcmeta.
- ZIPs deterministas; manifest.json con SHA-1, SHA-256, UUID y orden de capas. Máximo 31 capas remotas por versión.
- Subida a Cloudflare R2: primero los ZIPs con nombres inmutables, al final el puntero current.json. Rollback de versiones anteriores.
- Un watcher Folia asíncrono lee el manifiesto HTTPS público, conserva el catálogo anterior si falla, ofrece nuevas capas a conectados y a quienes entren después.

Los overlays deben aplicarse **todos juntos** respetando el orden. No es un parche binario incremental: solo los packs con hashes distintos necesitan nueva descarga, siempre sujeto a la caché de Minecraft.

## Construcción desde Studio, sin JAR

Requiere Python 3.11+ y VSCore ejecutándose en 127.0.0.1:3061. Publicá el OGG de cada canción o mezcla y las fuentes en la web de VSMedia primero. La carpeta de salida debe estar vacía.

~~~bash
python3 tools/vspack_publish.py studio ./publicaciones/v001 --max-mib 90 --profile lossless
python3 tools/vspack_publish.py verify ./publicaciones/v001
~~~

Si ya exportaste un ZIP:

~~~bash
python3 tools/vspack_publish.py build ~/Descargas/vsmedia-resourcepack.zip ./publicaciones/v001 --max-mib 90 --profile lossless
~~~

Para aplicar PackSquash real, instalarlo desde https://github.com/ComunidadAylas/PackSquash y utilizar la ruta del ejecutable:

~~~bash
python3 tools/vspack_publish.py studio ./publicaciones/v002 \
  --max-mib 90 --profile compact --packsquash /ruta/packsquash --require-packsquash
~~~

Perfiles: lossless (JSON + ZIP sin recodificar PNG/OGG), balanced y compact (permiten optimización externa PackSquash). No se promete una reducción fija.

## Configuración R2 privada

Crear bucket de Cloudflare R2, un dominio público HTTPS como packs.ejemplo.com y un token con acceso al bucket. Guardar las credenciales **solo en variables de entorno privadas o secretos de GitHub Actions**, jamás en la web, ni en la configuración pública, ni versionadas.

~~~bash
export VSPACK_R2_ENDPOINT="https://ID-CUENTA.r2.cloudflarestorage.com"
export VSPACK_R2_BUCKET="virgoland-packs"
export VSPACK_R2_PUBLIC_URL="https://packs.ejemplo.com"
export VSPACK_R2_ACCESS_KEY_ID="CREDENCIAL_PRIVADA"
export VSPACK_R2_SECRET_ACCESS_KEY="SECRETO_PRIVADO"
python3 -m pip install --user boto3
~~~

Publicar versión construida:

~~~bash
python3 tools/vspack_publish.py publish-r2 ./publicaciones/v002 --prefix virgoland
~~~

Un comando para obtener pack del Studio, dividirlo y subirlo:

~~~bash
python3 tools/vspack_publish.py studio ./publicaciones/v003 \
  --max-mib 90 --profile balanced --publish-r2 --prefix virgoland
~~~

Se cargan primero los ZIPs bajo virgoland/versions/ y al final el manifiesto público virgoland/current.json. Archivar las carpetas publicaciones/vNNN para poder restaurar versiones anteriores.

~~~bash
python3 tools/vspack_publish.py rollback-r2 ./publicaciones/v002 --prefix virgoland
~~~

Rollback requiere el published-manifest.json histórico, valida los ZIPs locales y restaura solo current.json, sin borrar archivos del CDN.

## Modo automático continuo: watch

Para que las modificaciones publicadas en VSMedia se conviertan en nuevas
versiones sin ejecutar manualmente cada comando, iniciar el publicador como
servicio de usuario en CachyOS/Linux. El proceso consulta el ZIP exportado
por VSCore cada 300 segundos por defecto, compara su SHA-256 y solo genera
o sube una nueva versión cuando cambia.

~~~bash
python3 tools/vspack_publish.py watch ./publicaciones \
  --interval-seconds 300 --max-mib 90 --profile balanced --publish-r2
~~~

El modo watch necesita que el plugin/VSCore ya esté iniciado y las mismas
variables R2 privadas del paso anterior. No debe exponerse el puerto local
3061 públicamente. Con --once hace una sola comprobación (útil para
pruebas o cron). Para con Ctrl+C. El proceso no corre en los schedulers de
Folia y no manipula ningún JAR. Al fallar un build/publicación conserva
current.json y el último estado válido. Guarda las versiones bajo
publicaciones/revision-<SHA256> y publication-state.json en la carpeta raíz.

**Nota**: la detección consulta el ZIP completo, así que no conviene
establecer un intervalo muy corto, especialmente con packs grandes.
Los ZIPs de terceros importados en VSPack actualmente son **solo de sesión**.
Si VSCore reinicia y desaparecen las fuentes importadas, el watcher bloquea
la publicación cuando detecta que antes había packs cargados; hay que reimportarlos
para evitar publicar accidentalmente una versión incompleta. No se debe
interpretar el mecanismo como persistencia automática de los ZIPs originales.

## Plugin de servidor Folia

En una **futura compilación del plugin** que contenga el watcher, establecer en plugins/VSMedia/config.yml:

~~~yaml
publication:
  enabled: true
  manifest-url: "https://packs.ejemplo.com/virgoland/current.json"
  check-seconds: 300
~~~

Recargar con /vsmedia reload. Después, publicar un ZIP nuevo no requiere recompilar el plugin. Los clientes descargan desde R2 y no desde el servidor Minecraft. El intervalo de comprobación se restringe a 60-3600 segundos.

El watcher no sigue redirecciones y exige HTTPS, mismo host para todos los ZIPs y hashes y tamaños válidos.
El comando de administración /vsmedia packs <jugador> muestra por capa si el cliente
la descargó, rechazó, falló o notificó SUCCESSFULLY_LOADED. No confirma
reproducción individual de audio. La actualización de recursos en clientes reales requiere confirmación in-game.

## Pruebas sin empaquetar JAR

~~~bash
python3 -m unittest discover -s tests -p 'test_vspack_publish.py' -v
mvn -B -f java/pom.xml test
~~~

El trabajo de CI que antes empaquetaba JAR está deshabilitado en esta rama.

## Limitaciones y tareas operativas pendientes

- No hay cuenta real R2 conectada ni subida real verificada, por falta de credenciales y dominio públicos configurados.
- El panel actual exporta WAV de VSMix/VSAudio y permite importarlo como OGG al pack. Todavía no existe un botón directo de publicación desde una mezcla no exportada.
- VSRadio de playlists sincronizadas ya está implementada por Plasmo Voice (véase [VSRadio Live](VSRADIO-LIVE.md)); no utiliza el Resource Pack. Su reproducción en Minecraft requiere una próxima compilación del plugin y pruebas con clientes. La entrada de micrófono/DJ WebRTC en directo sigue separada.
- No se validó en un servidor Minecraft real ni se confirmó aceptación/descarga en clientes. Una compilación futura del plugin será necesaria solo para desplegar este nuevo código de distribución.
- Revisar conflictos de recursos y el límite global de capas al sumar plugins externos antes de producción.
