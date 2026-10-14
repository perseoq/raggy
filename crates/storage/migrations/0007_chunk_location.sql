-- Fase 1 (v2.0.0): ubicación de origen del fragmento para documentos que no son
-- Markdown (p. ej. `filas 1-100` de un CSV, `pág. 3` de un PDF).
-- Para Markdown y código la columna queda a NULL y la cita usa las líneas.

ALTER TABLE chunks ADD COLUMN location TEXT;
