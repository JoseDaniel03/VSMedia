# Web v11: arranque automático con Folia

El método `VSMediaPlugin.onEnable()` inicia el servidor web automáticamente. No se requiere `vsmedia web` en la terminal. Por defecto escucha en **127.0.0.1:3060** (configurable con `studio-web.port`), y se detiene cuando el plugin se apaga.

**Publicación:** copiar el contenido de VSMedia Studio v11 compacta a la carpeta `web/` en la raíz del repositorio antes de compilar el plugin. Maven incluye sus archivos estáticos en `studio-web-v11/` dentro del JAR. El plugin detecta este `index.html` y lo sirve automáticamente; si falta, avisa y sirve el antiguo FullCanvas. La edición JavaScript, las sprites, Blockbench, las mezclas y los ZIPs del navegador ya son locales. No se necesita Node.js, FFmpeg ni Trunk para **servir** v11 desde un JAR ya compilado.

La conversión de archivos de audio a OGG/Opus requiere un **encoder en el navegador** o en la propia PC del editor. El servidor del plugin no ejecuta FFmpeg y rechaza `/api/audio/convert` para v11. WebM/Opus se puede codificar en el navegador con MediaRecorder cuando está disponible. La API Rust v1 sigue siendo un servicio optativo para operaciones persistentes, en el puerto 3061.

Por seguridad la interfaz web se vincula solamente a loopback; para abrirla desde otro equipo, habilitar un túnel SSH o proxy HTTPS autenticado. No publicar directamente el puerto del plugin con endpoints de administración sin autenticación.

**Compilación:** `mvn -B -f java/pom.xml package` (requiere Java 25 y dependencias de Paper 26.3).
