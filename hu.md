# Historias de Usuario: Raggy Desktop

A continuación, las historias de usuario organizadas por épica, con formato estándar (Como / Quiero / Para) y criterios de aceptación.

---

## ÉPICA 1: Gestión de Carpetas e Indexación

### HU-01: Añadir carpeta para indexar
**Como** usuario,
**quiero** seleccionar una carpeta de mi sistema desde la interfaz,
**para** que la aplicación la indexe y la deje disponible como fuente de conocimiento.

**Criterios de aceptación:**
- Existe un botón visible "Añadir carpeta" en la pantalla principal.
- Al pulsarlo se abre el diálogo nativo del sistema operativo.
- Solo se aceptan carpetas (no archivos sueltos).
- Tras seleccionar, la carpeta aparece en el panel lateral con estado "Indexando…".
- Al finalizar, el estado cambia a "Indexada" con contador de archivos `.md` procesados.

---

### HU-02: Indexación automática de documentos
**Como** usuario,
**quiero** que la app escanee recursivamente la carpeta y procese todos los
documentos soportados,
**para** no tener que seleccionar archivos manualmente.

**Criterios de aceptación:**
- Se recorren todas las subcarpetas.
- Se procesan Markdown, texto, código, CSV, Word (`.docx`/`.doc`), Excel
  (`.xlsx`/`.xls`), PowerPoint (`.pptx`/`.ppt`) y PDF.
- Se muestra progreso durante la indexación.
- Los archivos vacíos, ilegibles o que necesiten una herramienta ausente
  (OCR/LibreOffice) se reportan con un aviso, pero no detienen el proceso.

---

### HU-03: Re-indexación incremental
**Como** usuario,
**quiero** que al modificar o añadir archivos `.md` la app actualice el índice automáticamente,
**para** mantener el conocimiento siempre actualizado sin reindexar todo.

**Criterios de aceptación:**
- El sistema detecta cambios por hash de contenido y fecha de modificación.
- Solo se reindexan archivos nuevos o modificados.
- Los archivos eliminados se retiran del índice.
- Existe un botón de "Reindexar todo" como respaldo manual.

---

### HU-04: Eliminar carpeta indexada
**Como** usuario,
**quiero** poder eliminar una carpeta del listado,
**para** liberar espacio y ordenar mis fuentes.

**Criterios de aceptación:**
- Se solicita confirmación antes de eliminar.
- Se advierte que las sesiones asociadas también se eliminarán o quedarán huérfanas.
- El índice vectorial y los metadatos se borran correctamente.

---

### HU-33: OCR de PDF escaneados
**Como** usuario,
**quiero** que los PDF sin capa de texto se reconozcan con OCR en español,
**para** poder buscarlos igual que el resto de documentos.

**Criterios de aceptación:**
- Si hay Tesseract con el paquete de español y Poppler, el OCR se aplica solo.
- Si falta alguna herramienta, se avisa con el comando de instalación del sistema.
- El idioma de OCR y su activación se configuran en *Configuración › OCR*.

---

## ÉPICA 2: Sesiones por Carpeta

### HU-05: Crear sesión de chat por carpeta
**Como** usuario,
**quiero** crear una nueva sesión dentro de una carpeta indexada,
**para** tener conversaciones separadas por contexto.

**Criterios de aceptación:**
- Cada carpeta indexada tiene su propia sección de sesiones.
- El botón "Nueva sesión" aparece al seleccionar la carpeta.
- La sesión queda vinculada exclusivamente a esa carpeta.
- Se puede asignar un nombre a la sesión (por defecto: fecha/hora).

---

### HU-06: Aislamiento de contexto entre sesiones
**Como** usuario,
**quiero** que cada sesión solo consulte los archivos de su carpeta asociada,
**para** evitar que el modelo mezcle información de proyectos distintos.

**Criterios de aceptación:**
- El retrieval de una sesión solo consulta el índice de su carpeta.
- Ninguna respuesta puede citar archivos de otra carpeta.
- Intentar forzar contexto externo devuelve un mensaje de límite.

---

### HU-07: Reanudar una sesión previa
**Como** usuario,
**quiero** abrir una sesión existente y ver su historial completo,
**para** continuar una conversación donde la dejé.

**Criterios de aceptación:**
- Al hacer clic en una sesión se cargan todos sus mensajes previos.
- El contexto del RAG se reconstruye automáticamente.
- El estado del modelo seleccionado se restaura.
- La sesión queda marcada como "activa" hasta cerrar la app.

---

### HU-08: Renombrar y eliminar sesiones
**Como** usuario,
**quiero** gestionar mis sesiones,
**para** mantener ordenado mi espacio de trabajo.

**Criterios de aceptación:**
- Se puede renombrar desde un menú contextual.
- Se puede eliminar con confirmación previa.
- Al eliminar, se borran mensajes y artefactos asociados.

---

## ÉPICA 3: Motor RAG

### HU-09: Chunking semántico de Markdown
**Como** usuario,
**quiero** que los archivos `.md` se dividan respetando su estructura,
**para** que el retrieval devuelva fragmentos coherentes.

**Criterios de aceptación:**
- Los chunks respetan encabezados, listas y bloques de código.
- Un bloque de código nunca se corta a la mitad.
- Cada chunk guarda metadatos: archivo origen, ruta, líneas, encabezado padre.

---

### HU-10: Recuperación de contexto relevante
**Como** usuario,
**quiero** que al hacer una consulta se recuperen los fragmentos más relevantes,
**para** que el modelo responda con información precisa.

**Criterios de aceptación:**
- Se recuperan top-K fragmentos (configurable, por defecto 5).
- Se aplica umbral mínimo de similitud para descartar ruido.
- Si no hay fragmentos por encima del umbral, el sistema lo indica al usuario.
- Los fragmentos se inyectan con cita de archivo y línea.

---

### HU-11: Citas verificables en las respuestas
**Como** usuario,
**quiero** que las respuestas indiquen de qué archivo proviene la información,
**para** poder verificar y confiar en lo que me dice el modelo.

**Criterios de aceptación:**
- Cada afirmación relevante incluye referencia al archivo fuente.
- Las citas son clicables y abren el archivo en la vista previa.
- Si el modelo no cita, la respuesta se marca como "no anclada".

---

## ÉPICA 4: Motor de Chat

### HU-12: Chat con streaming
**Como** usuario,
**quiero** ver la respuesta aparecer token a token,
**para** percibir la app como rápida y fluida.

**Criterios de aceptación:**
- La respuesta se muestra progresivamente.
- Existe botón de "Detener generación".
- Si falla la conexión, se muestra error claro y se puede reintentar.

---

### HU-13: Temperatura bloqueada en 0.1
**Como** usuario,
**quiero** que el modelo responda con mínima creatividad por defecto,
**para** reducir alucinaciones en documentación técnica.

**Criterios de aceptación:**
- La temperatura por defecto es 0.1.
- El valor es visible pero bloqueado en la configuración básica.
- Un modo avanzado permite cambiarlo, con advertencia de riesgo.
- Toda sesión nueva hereda 0.1.

---

### HU-14: Prompt de sistema estricto
**Como** usuario,
**quiero** que el modelo solo responda con base en el contexto recuperado,
**para** evitar que invente APIs, archivos o comportamientos.

**Criterios de aceptación:**
- El system prompt indica: "Responde solo con base en el contexto. Si no está, dilo explícitamente."
- El modelo no debe inventar nombres de archivos, funciones ni rutas.
- Si el usuario pide algo fuera del contexto, el modelo responde "No encuentro esa información en los archivos indexados".

---

### HU-15: Selección de proveedor y modelo por sesión
**Como** usuario,
**quiero** elegir entre GPT, Claude, DeepSeek u OpenRouter,
**para** usar el modelo que mejor se adapte a cada tarea.

**Criterios de aceptación:**
- Existe un selector visible en la barra superior del chat.
- El cambio aplica a la sesión activa sin reiniciar la app.
- Cada sesión recuerda su modelo seleccionado.
- Si el proveedor no tiene API Key configurada, se indica y se ofrece configurarla.

---

## ÉPICA 5: Generación y Descarga de Código

### HU-16: Detección de bloques de código
**Como** usuario,
**quiero** que la app identifique automáticamente los bloques de código en las respuestas,
**para** poder gestionarlos como archivos.

**Criterios de aceptación:**
- Se detectan bloques con sintaxis Markdown (```lenguaje).
- Se detecta el nombre de archivo declarado (en encabezado o texto adyacente).
- Si no hay nombre, se propone uno genérico editable por el usuario.

---

### HU-17: Descarga de código con nombre correcto
**Como** usuario,
**quiero** que al descargar se guarde con el nombre que el modelo declaró (`file_uno.py`),
**para** no tener que renombrar manualmente.

**Criterios de aceptación:**
- El botón "Descargar" aparece bajo cada bloque de código.
- El nombre del archivo se toma del encabezado del bloque o del texto previo.
- Se abre diálogo para elegir carpeta destino.
- El archivo se guarda con la extensión correcta.
- Si el archivo ya existe, se avisa y se ofrece sobrescribir o renombrar.

---

### HU-18: Vista previa antes de descargar
**Como** usuario,
**quiero** revisar el código antes de guardarlo,
**para** validar que es lo que esperaba.

**Criterios de aceptación:**
- Existe un botón "Vista previa" en cada bloque.
- La vista previa muestra el código con resaltado de sintaxis.
- Se muestra el nombre propuesto y la extensión.
- Se puede copiar al portapapeles.

---

### HU-19: Descarga por lote
**Como** usuario,
**quiero** descargar todos los bloques de código de una respuesta a la vez,
**para** ahorrar tiempo cuando el modelo genera varios archivos.

**Criterios de aceptación:**
- Existe un botón "Descargar todo" cuando hay más de un bloque.
- Se elige carpeta destino y se respetan las rutas relativas si el modelo las declaró.
- Se muestra resumen de archivos guardados.

---

## ÉPICA 6: Memoria de Proyecto

### HU-20: Registro de artefactos generados
**Como** usuario,
**quiero** que la app guarde un historial de los archivos que he generado,
**para** saber qué se ha producido en cada sesión.

**Criterios de aceptación:**
- Cada descarga se registra con: nombre, ruta, fecha, sesión, modelo.
- Existe una vista "Artefactos" por carpeta y por sesión.
- Se puede abrir la ubicación del archivo desde la vista.
- Se puede eliminar un registro (sin borrar el archivo físico) o marcar como archivado.

---

### HU-21: Referenciar artefactos previos en nuevos prompts
**Como** usuario,
**quiero** que el modelo sepa qué archivos ya generé,
**para** que no los duplique y mantenga coherencia entre ellos.

**Criterios de aceptación:**
- El system prompt incluye la lista de artefactos previos de la sesión.
- Si el usuario pide "modifica file_uno.py", el modelo tiene contexto de su contenido previo.
- El historial de artefactos se conserva entre ejecuciones de la app.

---

### HU-22: Árbol de proyecto generado
**Como** usuario,
**quiero** ver un árbol de archivos generados por carpeta,
**para** entender la estructura del proyecto que estoy construyendo.

**Criterios de aceptación:**
- Vista de árbol con jerarquía de rutas.
- Los archivos se agrupan por sesión y por carpeta origen.
- Se puede exportar el árbol como lista.

---

## ÉPICA 7: Configuración Multi-Proveedor

### HU-23: Configurar API Keys
**Como** usuario,
**quiero** introducir mis claves de OpenAI, Anthropic, DeepSeek y OpenRouter,
**para** poder usar los modelos de cada proveedor.

**Criterios de aceptación:**
- Panel de configuración con un campo por proveedor.
- Las claves se guardan cifradas en el almacén del sistema.
- Nunca se muestran completas tras guardarlas (solo últimos 4 caracteres).
- Existe botón "Probar conexión" por proveedor.

---

### HU-24: Probar conexión con proveedor
**Como** usuario,
**quiero** verificar que mi clave funciona,
**para** no descubrir el error a mitad de un chat.

**Criterios de aceptación:**
- El botón "Probar" hace una llamada mínima al proveedor.
- Se muestra resultado: éxito con modelo listado, o error descriptivo.
- Los errores comunes (clave inválida, sin crédito, rate limit) se traducen a mensajes claros.

---

### HU-25: Fallback entre proveedores
**Como** usuario,
**quiero** que si un proveedor falla, la app pueda usar otro,
**para** no quedarme bloqueado a mitad de una tarea.

**Criterios de aceptación:**
- Se puede configurar un proveedor de respaldo por sesión.
- Si el principal devuelve error de disponibilidad, se avisa y se ofrece cambiar.
- No se cambia automáticamente sin consentimiento del usuario.

---

## ÉPICA 8: Configuración y Preferencias

### HU-26: Tema claro / oscuro
**Como** usuario,
**quiero** cambiar el tema visual,
**para** adaptarlo a mi entorno o preferencia.

**Criterios de aceptación:**
- Existe opción en preferencias.
- El cambio es inmediato, sin reiniciar.
- La preferencia se guarda entre sesiones.

---

### HU-27: Configurar parámetros del RAG
**Como** usuario avanzado,
**quiero** ajustar tamaño de chunk, top-K y umbral de similitud,
**para** afinar la calidad del retrieval según mis documentos.

**Criterios de aceptación:**
- Panel avanzado con estos parámetros.
- Valores por defecto sensatos y documentados.
- Cambios aplican a nuevas indexaciones o requieren reindexar (según parámetro).
- Se advierte si el usuario elige valores extremos.

---

### HU-28: Directorio de datos configurable
**Como** usuario,
**quiero** elegir dónde se guardan los índices y la base de datos,
**para** controlar el uso de disco o mover el proyecto de equipo.

**Criterios de aceptación:**
- Se puede cambiar desde preferencias.
- Al cambiar, se ofrece migrar datos existentes.
- Si no se migra, se advierte que se empezará de cero.

---

## ÉPICA 9: Seguridad y Privacidad

### HU-29: Claves nunca visibles en logs
**Como** usuario,
**quiero** que mis API Keys nunca aparezcan en registros,
**para** evitar fugas accidentales.

**Criterios de aceptación:**
- Los logs nunca incluyen claves, ni truncadas.
- Los errores de red no exponen headers de autenticación.
- Los reportes de error al usuario omiten cualquier credencial.

---

### HU-30: Datos no salen sin consentimiento
**Como** usuario,
**quiero** que la app no envíe datos a terceros más allá del proveedor elegido,
**para** proteger mi documentación sensible.

**Criterios de aceptación:**
- Solo se envían al proveedor los fragmentos recuperados necesarios para la consulta.
- No hay telemetría por defecto.
- Existe un aviso claro en la primera ejecución explicando qué se envía y a dónde.

---

## ÉPICA 10: Exportación y Respaldo

### HU-31: Exportar sesión completa
**Como** usuario,
**quiero** exportar una sesión con su historial y artefactos,
**para** compartirla o respaldarla.

**Criterios de aceptación:**
- Exportación en Markdown con metadatos.
- Incluye artefactos generados como archivos adjuntos o embebidos.
- Se puede elegir carpeta destino.

---

### HU-32: Respaldo de índices y memoria
**Como** usuario,
**quiero** respaldar mis índices y memoria de proyecto,
**para** no perder el trabajo si cambio de equipo.

**Criterios de aceptación:**
- Función "Exportar todo" que empaqueta índice, base de datos y artefactos.
- Función "Importar" que restaura en otra instalación.
- Se valida integridad del paquete antes de restaurar.

 
