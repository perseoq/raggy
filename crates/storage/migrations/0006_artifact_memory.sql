-- Fase 5: memoria de proyecto. Se amplía la tabla de artefactos con la ruta
-- relativa, el lenguaje, el contenido generado y la marca de archivado
-- (HU-20, HU-21, HU-22).

ALTER TABLE artifacts ADD COLUMN relative_path TEXT NOT NULL DEFAULT '';
ALTER TABLE artifacts ADD COLUMN language TEXT;
ALTER TABLE artifacts ADD COLUMN content TEXT NOT NULL DEFAULT '';
ALTER TABLE artifacts ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
