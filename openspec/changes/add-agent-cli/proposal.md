## Why

Coding agents such as Claude Code, Codex, opencode and Copilot cannot use dropgo today. Sign-in needs a browser, and most admin actions are HTML forms that answer with a redirect. The owner wants an agent to publish a page, check it and manage shares without the Web UI, and to get clear JSON back for every result and every error.

## What Changes

- Personal API tokens. The owner creates them on a new tokens page in the Web UI.
  - Each token has a name and an expiry. The default expiry is 90 days.
  - The token is shown once. The server keeps only its hash and records when it was last used.
  - Only the Web UI can create or revoke tokens. The API cannot.
- A JSON API under `/api/v1` on the admin host. It takes `Authorization: Bearer <token>` and covers these actions:
  - check a path, upload, list, get and edit a share
  - move a share to the trash and restore it
  - read a share's files and rendered pages, whatever the share's visibility
- Purge and preferences stay in the Web UI only.
- Every API error has the same shape: a stable `code`, an English `message` and optional `details`.
- `dropgo` client subcommands wrap that API.
  - They print JSON to stdout and errors as JSON to stderr, with one exit code per kind of error.
  - They need no server config.
  - `dropgo auth login` asks for the URL and the token. The token is typed without echo, checked against the server, then saved to `~/.config/dropgo/config.toml`.
- Uploading to a path that is already taken fails with 409 and the existing share. `--replace` overwrites it.
- Each GitHub release attaches a linux-amd64 binary.
- `skills/dropgo/SKILL.md` is an agent skill that works with any coding agent. The README gives a prompt that tells an agent to install the CLI and the skill on its own machine.

## Capabilities

### New Capabilities

- `api-tokens`: creating, listing, revoking and expiring tokens, and the tokens page in the Web UI.
- `agent-api`: the `/api/v1` endpoints, bearer auth, the JSON response and error shapes, and what tokens may not do.
- `agent-cli`: the client subcommands, where they read config, `auth login`, the output format and exit codes.
- `agent-skill`: the skill file, the install prompt in the README, and the release binary.

### Modified Capabilities

- `owner-auth`: `/api/v1` is exempt from the session sign-in and the `Origin` check, because it takes only bearer tokens.

The Web UI routes keep their behavior. The new tokens page is part of `api-tokens`.

## Impact

- New migration: an `api_tokens` table and a `users.owner_claim` column.
- `src/main.rs` and `src/config.rs` change so that client subcommands start without the database or store settings. **Breaking:** server flags must follow `serve` or `rerender`; `dropgo --database-url X rerender` no longer parses. Env vars and a bare `dropgo` are unchanged.
- Bearer requests skip the `Origin` check, which only guards cookie sessions.
- New dependencies for the client: `reqwest` with multipart, which `openidconnect` already pulls in, and `rpassword` for reading the token without echo.
- CI builds and attaches the release binary.
- README in English and Traditional Chinese gains an agent section and the install prompt.
