# Plan de Proyecto: **Raggy Desktop**

## 1. Visión General

Software de escritorio con interfaz gráfica moderna, escrito en Rust, que funciona como un asistente de IA contextualizado sobre documentos locales compatibles. Permite indexar carpetas completas, crear sesiones persistentes por carpeta y chatear con modelos de múltiples proveedores, generando y exportando código trazable basado exclusivamente en el conocimiento indexado.

---

## 2. Objetivos

- **Objetivo principal:** Ofrecer un asistente local que responda con precisión (temperatura 0.1) basándose únicamente en documentos indexados y cite su ubicación verificable.
- **Objetivos secundarios:**
  - Multi-proveedor de LLM (OpenAI, Anthropic, DeepSeek, OpenRouter).
  - Persistencia de sesiones, memoria y artefactos generados.
  - Exportación limpia de código generado con nombres de archivo correctos.
  - Minimizar alucinaciones mediante anclaje estricto al RAG.

---

## 3. Alcance

### Incluido
- GUI moderna multiplataforma (Windows, macOS, Linux).
- Indexación RAG de carpetas con documentos: Markdown, texto, código, CSV,
  Word, Excel, PowerPoint y PDF (con OCR para los escaneados).
- Sesiones aisladas por carpeta indexada.
- Chat con streaming, historial y memoria.
- Descarga de código generado con nombre de archivo declarado.
- Soporte multi-proveedor con configuración de API Keys.

### Fuera del alcance actual
- Indexación de imágenes sueltas, audio o vídeo.
- Colaboración multiusuario o sincronización en nube.
- Fine-tuning de modelos.
- Ejecución de código generado.

---

## 4. Arquitectura Conceptual (sin código)

**Capas:**

1. **Capa UI (Frontend):** ventana nativa, panel lateral de carpetas/sesiones, vista de chat, vista de artefactos, panel de configuración de proveedores.
2. **Capa de Aplicación:** gestor de sesiones, orquestador de chat, gestor de artefactos, gestor de proveedores.
3. **Capa RAG:** chunker de Markdown, generador de embeddings, almacén vectorial local, recuperador con re-ranking.
4. **Capa de Persistencia:** base de datos local (sesiones, mensajes, memoria), almacén vectorial, sistema de archivos para artefactos.
5. **Capa de Proveedores LLM:** adaptadores normalizados por proveedor.
6. **Capa de Seguridad:** permisos restrictivos para `settings.json`, secretos
   redactados y aislamiento de contexto por sesión.

---

## 5. Módulos Funcionales

### 5.1 Gestión de Carpetas e Indexación
- Selección de carpeta raíz mediante diálogo nativo.
- Escaneo recursivo de documentos y código (Markdown, texto, código, CSV, Office
  y PDF).
- Extracción de texto por formato y **OCR de PDF escaneados** con Tesseract
  (idioma configurable); conversión de `.doc`/`.ppt` legacy con LibreOffice.
- Chunking semántico respetando encabezados, listas y bloques de código.
- Generación de embeddings y almacenamiento en índice vectorial por carpeta.
- Re-indexación incremental (solo archivos nuevos o modificados).
- Identificador único por carpeta (hash de ruta + contenido).

### 5.2 Sesiones por Carpeta
- Cada carpeta indexada genera automáticamente su espacio de sesiones.
- Múltiples sesiones de chat por carpeta.
- Historial persistente, reanudable y exportable.
- Contexto de sesión limitado a la carpeta asociada (aislamiento estricto).
- Archivos adjuntos por sesión (HU-34): se añaden uno o varios archivos al chat
  y su contenido se busca junto con el de la carpeta, distinguiendo el origen y
  buscando en la carpeta los identificadores que el adjunto mencione.

### 5.3 Motor RAG
- Recuperación de top-K fragmentos relevantes por consulta.
- Re-ranking opcional para mejorar precisión.
- Inyección de fragmentos con citas verificables de archivo y ubicación nativa
  (líneas, página, diapositiva o hoja y fila).
- Umbral relativo: el contexto se acota respecto al mejor resultado **de cada
  origen** (carpeta y adjuntos), sin descartar nunca el fragmento más relevante ni
  permitir que una fuente eclipse a la otra.
- Referencia cruzada: los identificadores que menciona un adjunto se buscan
  también en la carpeta indexada.

### 5.4 Motor de Chat
- Streaming de respuestas token a token.
- System prompt estricto: “responde solo con base en el contexto; si no está, dilo”.
- Temperatura fija en 0.1 (configurable pero bloqueada por defecto).
- Soporte de instrucciones del usuario sin romper el anclaje al RAG.
- Detección y marcado de bloques de código en la respuesta.

### 5.5 Generación y Descarga de Código
- Detección de bloques de código con nombre de archivo declarado (ej. `file_uno.py`).
- Extracción automática del nombre desde el encabezado del bloque o texto adyacente.
- Vista previa del artefacto antes de descargar.
- Descarga individual o por lote respetando estructura de rutas.
- Registro del artefacto en la memoria del proyecto.

### 5.6 Memoria de Proyecto
- Registro de artefactos generados (nombre, ruta, fecha, sesión origen).
- Árbol de archivos generados por sesión/carpeta.
- Capacidad de referenciar artefactos previos en nuevos prompts.
- Persistencia local entre ejecuciones.

### 5.7 Gestión Multi-Proveedor
- Panel de configuración con campos por proveedor:
  - OpenAI (modelos GPT).
  - Anthropic (modelos Claude).
  - DeepSeek.
  - OpenRouter (acceso a modelos agregados).
- Selección de modelo activo por sesión.
- Claves en `settings.json` sin cifrar, con permisos `0600` en Unix; variables
  de entorno como alternativa.
- Manejo de errores, rate limits y fallback opcional.

### 5.8 Configuración y Preferencias
- Tema claro/oscuro (los bloques de código en modo oscuro se resaltan con la
  paleta Monokai Pro de Sublime Text).
- Idioma de interfaz.
- Directorio de almacenamiento de datos.
- Parámetros del RAG (tamaño de chunk, top-K, umbral).
- Parámetros del modelo (temperatura, max tokens) con 0.1 como valor por defecto bloqueado.

---

## 6. Flujos de Usuario Principales

**Flujo A — Indexar y chatear:**
1. Usuario abre la app.
2. Selecciona “Añadir carpeta” y elige una carpeta con documentos compatibles.
3. La app escanea, chunkea, genera embeddings y confirma indexación.
4. Usuario crea una nueva sesión dentro de esa carpeta.
5. Escribe una consulta.
6. El sistema recupera contexto, llama al modelo elegido y responde en streaming.
7. La respuesta se guarda en el historial de la sesión.

**Flujo B — Generar y descargar código:**
1. Usuario pide “genera `file_uno.py` que haga X según la doc”.
2. El modelo responde con bloque de código etiquetado.
3. El sistema detecta el nombre, muestra vista previa.
4. Usuario pulsa “Descargar” y el archivo se guarda con el nombre correcto.
5. El artefacto queda registrado en la memoria del proyecto.

**Flujo C — Reanudar proyecto:**
1. Usuario abre la app.
2. Ve carpetas indexadas con sus sesiones y artefactos.
3. Retoma una sesión y el contexto previo se restaura.

**Flujo D — Contrastar un adjunto con la carpeta (HU-34):**
1. En un chat, el usuario pulsa «Adjuntar» y elige, por ejemplo, una factura PDF.
2. El adjunto se indexa bajo esa sesión y aparece como etiqueta sobre la
   conversación.
3. Pregunta por los datos de la factura sin repetir su folio.
4. El sistema busca en la carpeta y en los adjuntos; además, localiza en los CSV
   las filas que el adjunto referencia por su folio.
5. La respuesta cita cada fuente por separado (carpeta y adjunto) y permite
   compararlas.

---

## 7. Requisitos No Funcionales

- **Rendimiento:** indexación de 1.000 archivos `.md` en tiempos razonables; chat con latencia dominada por el proveedor.
- **Precisión:** respuestas estrictamente ancladas al RAG; sin invención de APIs ni archivos.
- **Portabilidad:** binario único o instalador por plataforma.
- **Seguridad:** las claves se almacenan sin cifrar en `settings.json` con permisos
  `0600` en Unix; no se envían datos fuera de los proveedores elegidos.
- **Usabilidad:** GUI moderna, atajos de teclado, búsqueda dentro de sesiones.
- **Escalabilidad:** soportar múltiples carpetas y sesiones concurrentes sin degradación.

---

## 8. Criterios de Aceptación

- Se puede indexar una carpeta con `.md` y consultarla después.
- Las sesiones quedan aisladas por carpeta.
- Las respuestas citan archivos fuente del RAG.
- El modelo no inventa contenido fuera del contexto indexado (verificado con pruebas adversariales).
- El código generado se descarga con el nombre declarado.
- La memoria conserva el historial de artefactos entre ejecuciones.
- Se puede cambiar de proveedor sin reiniciar la app.

---

## 9. Riesgos y Mitigaciones

| Riesgo | Mitigación |
|---|---|
| Alucinaciones del modelo | Temperatura 0.1, prompt estricto, citas obligatorias, corte relativo al mejor resultado |
| Calidad del chunking | Chunking semántico por encabezados y respeto de bloques de código |
| Costos de API | Selección de modelos económicos, cacheo de embeddings, límite de tokens |
| Fuga de claves | Cifrado local y nunca mostrarlas completas en UI |
| Índices corruptos | Re-indexación incremental y checksum por archivo |
| Diferencias entre proveedores | Capa de adaptación normalizada con contrato común |

---

## 10. Evolución Futura

- Soporte para HTML, imágenes sueltas, audio y vídeo.
- Plugins de herramientas (ejecutar tests, lint).
- Modo agente con planificación multi-paso.
- Sincronización opcional cifrada entre dispositivos.
- Sincronización y exportación estructurada de proyectos completos.
- Integración con repositorios Git locales.
