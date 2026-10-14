-- Fase 2 del programa de correcciones 2.2.0: identifica la configuración y la
-- versión del pipeline usadas para indexar cada archivo. El valor vacío fuerza
-- una reindexación inicial de los documentos ya existentes.

ALTER TABLE documents ADD COLUMN processing_hash TEXT NOT NULL DEFAULT '';
