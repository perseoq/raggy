-- Fase 3: citas asociadas a cada mensaje del asistente (HU-11).
-- Se guardan como JSON con la lista de fuentes usadas en la generación.

ALTER TABLE messages ADD COLUMN citations TEXT;
