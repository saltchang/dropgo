-- Off only for shares whose owner turned the sandbox's origin isolation off.
ALTER TABLE shares ADD COLUMN sandbox boolean NOT NULL DEFAULT true;
