-- The owner claim from the last sign-in, so API requests can check it against DROPGO_OWNERS.
ALTER TABLE users ADD COLUMN owner_claim text;

CREATE TABLE api_tokens (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name text NOT NULL,
    prefix text NOT NULL,
    hash bytea NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    last_used_at timestamptz
);

CREATE INDEX api_tokens_user ON api_tokens (user_id);
