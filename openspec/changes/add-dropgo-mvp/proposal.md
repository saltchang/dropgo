## Why

I often want to show someone an HTML page or a Markdown note without setting up hosting each time. dropgo lets one owner drop a file or folder in a browser and get a share link a few seconds later. The owner signs in through their own OIDC provider, so dropgo never manages accounts for the people it shares with.

## What Changes

This is a new project with no existing code or specs.

- Upload page at the admin host (`drop.example.com`). It accepts one file, a folder, or a zip. Files matched by `.gitignore`, `.dockerignore`, dotfiles or `node_modules` are skipped.
- Shares are served from a separate host (`s.example.com`) under a custom path that may contain slashes, such as `/notes/2026/q3/`.
- Each share has one of three visibility modes: public, password, or owner-only. Owner-only means signed in through OIDC as the owner.
- Markdown is rendered to HTML. Viewers can download the original files (zip or `.md`) and the rendered HTML.
- Shares can have an expiry date; the default is never. An expired share returns 410 and moves to the trash. Trash items are deleted 30 days after they enter it.
- Every share response carries noindex headers.
- List page with edit, delete, a trash tab and restore.
- UI in English and Traditional Chinese, dark theme by default. The owner's language and theme are stored on the server.
- Storage is S3-compatible or local disk. The database is Postgres.

## Capabilities

### New Capabilities

- `owner-auth`: OIDC sign-in, allowlist of owners, user and identity records, sessions.
- `share-upload`: accepted inputs, ignore rules, size limits, path rules, overwrite.
- `share-access`: visibility modes, password unlock, isolation of uploaded content, noindex, error responses.
- `markdown-rendering`: how Markdown becomes a page, and the download formats.
- `share-lifecycle`: expiry, trash, restore and purge.
- `admin-ui`: upload page, list page, edit, trash tab, language, theme, mobile layout.

### Modified Capabilities

None.

## Impact

- New Rust service, with Postgres for metadata and an object store for files.
- Deployment needs two hostnames, one OIDC client in the identity provider, one database and one bucket. The owner's homelab setup (Authelia, CNPG, R2, Traefik) lives in a separate repo and is out of scope here.
- The project will be published as open source under MIT. Docs and code comments are written in English, and the README is in English and Traditional Chinese.
