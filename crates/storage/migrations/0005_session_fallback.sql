-- Fase 4: proveedor de respaldo por sesión (HU-25).

ALTER TABLE sessions ADD COLUMN fallback_provider TEXT;
