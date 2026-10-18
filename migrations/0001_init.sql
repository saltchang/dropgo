CREATE TABLE users (
    id uuid PRIMARY KEY,
    locale text NOT NULL DEFAULT 'en' CHECK (locale IN ('en', 'zh-TW')),
    theme text NOT NULL DEFAULT 'dark' CHECK (theme IN ('dark', 'light')),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE identities (
    issuer text NOT NULL,
    subject text NOT NULL,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (issuer, subject)
);

CREATE TABLE shares (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users (id),
    path text NOT NULL,
    state text NOT NULL CHECK (state IN ('active', 'trashed', 'purging')),
    visibility text NOT NULL CHECK (visibility IN ('public', 'password', 'owner')),
    password_hash text,
    upload_id uuid NOT NULL,
    entry text NOT NULL,
    file_count integer NOT NULL,
    size_bytes bigint NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz,
    trashed_at timestamptz
);

CREATE UNIQUE INDEX shares_active_path ON shares (path) WHERE state = 'active';
CREATE INDEX shares_path ON shares (path);
CREATE INDEX shares_owner ON shares (owner_id, state);
