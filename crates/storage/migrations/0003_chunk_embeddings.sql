-- Fase 2: vector de embedding por fragmento para la recuperación híbrida (HU-10).
-- El vector se guarda como una secuencia de f32 en little-endian.

ALTER TABLE chunks ADD COLUMN embedding BLOB;
