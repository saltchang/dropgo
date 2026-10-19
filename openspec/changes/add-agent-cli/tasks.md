## 1. Tokens

- [ ] 1.1 Migration `0002_api_tokens.sql`: `api_tokens` table and `users.owner_claim`
- [ ] 1.2 Write `users.owner_claim` on every sign-in
- [ ] 1.3 Token create, list, revoke and lookup functions, with tests for hashing, expiry and the allowlist check
- [ ] 1.4 Tokens page (`/tokens`): list, create form, show-once view, revoke, nav entry, en and zh-TW strings

## 2. API

- [ ] 2.1 Bearer middleware for `/api/v1`, skip the `Origin` check there, and add a JSON fallback with rejection mapping
- [ ] 2.2 JSON error type with code, English message and details, plus the share object and the new catalog keys in `en` and `zh-TW`
- [ ] 2.3 `me`, `check`, `shares` list (with `?path=`) and get
- [ ] 2.4 Upload, reusing `upload::upload`: body limit, JSON errors, 201, and `upload.field_order` in `receive`
- [ ] 2.5 `PATCH` edit through `admin::save_edit`
- [ ] 2.6 Trash (idempotent) and restore
- [ ] 2.7 Files list and file read, with `?rendered=1` and the sandbox headers
- [ ] 2.8 Integration tests for each scenario in `agent-api` and `api-tokens`

## 3. CLI

- [ ] 3.1 Split `Cli` so client commands parse without the server `Config`, and a bare `dropgo` still serves
- [ ] 3.2 Config resolution (refuse remote `http://`) and `auth login` / `auth status`
- [ ] 3.3 HTTP client, error mapping and exit codes
- [ ] 3.4 `check`, `list`, `get`, `edit`, `trash`, `restore`, `cat`, with ref resolution and `ref.ambiguous`
- [ ] 3.5 `upload` for file, directory (client-side ignore rules), zip and stdin, plus default `owner`, `--replace` keeping settings, and the local password check
- [ ] 3.6 End-to-end tests that run the CLI against a test server

## 4. Skill, release and docs

- [ ] 4.1 `skills/dropgo/SKILL.md`
- [ ] 4.2 CI: copy the binary out of the release image and attach it and `SKILL.md`
- [ ] 4.3 README (en and zh-TW): agent section and install prompt
- [ ] 4.4 Run the full verification: `cargo fmt`, `cargo test`, `web` tests and qa for the tokens page
