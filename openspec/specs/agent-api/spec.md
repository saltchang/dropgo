# agent-api Specification

## Purpose
The `/api/v1` JSON API that agents and scripts use with a bearer token.
## Requirements
### Requirement: Bearer authentication
Routes under `/api/v1` on the admin host SHALL authenticate only with `Authorization: Bearer <token>` and SHALL ignore session cookies. They SHALL NOT require an `Origin` header. A request without a token SHALL get 401 with code `auth.missing_token`, and a request with an unknown token SHALL get 401 with code `auth.invalid_token`.

#### Scenario: Cookie alone is not enough
- **WHEN** a request to `GET /api/v1/shares` carries a valid owner session cookie and no bearer token
- **THEN** it gets 401 with code `auth.missing_token`

#### Scenario: POST without Origin
- **WHEN** a request to `POST /api/v1/shares/{id}/trash` carries a valid token and no `Origin` header
- **THEN** the share moves to the trash

#### Scenario: Whoami
- **WHEN** `GET /api/v1/me` is called with a valid token
- **THEN** the response holds the token's name and its expiry

### Requirement: Error shape
Every error response from `/api/v1` SHALL be JSON of the form `{"error": {"code", "message", "details"}}`. That includes unknown routes, wrong methods, malformed ids, malformed JSON bodies and bodies over the size limit. `code` SHALL be a key of the message catalog. `message` SHALL be the English catalog text whatever the owner's language. `details` SHALL be an object. It SHALL hold `arg` for messages that take an argument, and `existing` with the share's id, visibility and expiry for `path.exists`.

#### Scenario: English message for a zh-TW owner
- **WHEN** an owner whose language is zh-TW uploads more than the size limit through the API
- **THEN** the response is 413 with code `upload.too_big` and an English message

#### Scenario: Path taken by the owner
- **WHEN** an upload names a path where the owner already has an active share
- **THEN** the response is 409 with code `path.exists` and `details.existing.id` set to that share's id

#### Scenario: Unknown route and bad id
- **WHEN** `GET /api/v1/nope` or `GET /api/v1/shares/not-a-uuid` is called with a valid token
- **THEN** the response is JSON in the error shape, with code `api.not_found` or `api.bad_request`

### Requirement: Share object
Responses that return a share SHALL hold its id, path, full URL, state, visibility, entry file, file count, size in bytes, created time, expiry, trashed time and purge time. Times SHALL be RFC 3339 in UTC, and fields with no value SHALL be `null`. The response SHALL NOT contain the password or its hash.

#### Scenario: Get a trashed share
- **WHEN** `GET /api/v1/shares/{id}` is called for a share in the trash
- **THEN** the share object has `state` `trashed` and a `purge_at` 30 days after `trashed_at`

### Requirement: Upload through the API
`POST /api/v1/shares` SHALL accept the same multipart fields as the Web UI upload and apply the same rules: inputs, ignore rules, limits, paths, visibility and overwrite. The size limit SHALL be the configured share limit, not a smaller default. Settings fields SHALL come before the first file field, and a settings field after a file field SHALL fail with 400 `upload.field_order`. On success the response SHALL be 201 with the share object.

An upload to a path that holds one of the owner's active shares SHALL fail with 409 `path.exists` unless `overwrite` names that share's id.

#### Scenario: Upload a Markdown file
- **WHEN** a single `notes.md` is uploaded with `path=notes` and `visibility=owner`
- **THEN** the response is 201, `url` ends in `/notes/`, and `visibility` is `owner`

#### Scenario: Overwrite with the existing id
- **WHEN** an upload to an occupied path sends `overwrite` set to the existing share's id
- **THEN** the response is 201 and the path serves the new content

#### Scenario: Settings after a file
- **WHEN** a multipart body sends a file field before the `path` field
- **THEN** the response is 400 with code `upload.field_order` and no share is created

#### Scenario: Large upload
- **WHEN** a 50 MB folder is uploaded with the default 100 MB limit
- **THEN** the response is 201

### Requirement: Check, list and get
`GET /api/v1/check?path=` SHALL report whether a path is free with the same rules as the Web UI. `GET /api/v1/shares` SHALL list the owner's active shares, and `?state=trash` SHALL list trashed and purging shares, newest first in both cases. `?path=` SHALL narrow either list to shares at that path, after the path is normalized as the Web UI does. `GET /api/v1/shares/{id}` SHALL return 404 `share.not_found` for an id the owner does not have.

#### Scenario: Another owner's share
- **WHEN** an owner requests the id of a share owned by someone else
- **THEN** the response is 404 with code `share.not_found`

#### Scenario: Filter by path
- **WHEN** `GET /api/v1/shares?path=Notes/2026/` is called and an active share is at `notes/2026`
- **THEN** the list holds exactly that share

### Requirement: Edit through the API
`PATCH /api/v1/shares/{id}` SHALL take a JSON body and change only the fields present: `path`, `visibility`, `password`, `expires_at`.
- `expires_at: null` SHALL mean the share never expires. A time in the past SHALL fail with 400 `form.bad_expiry`.
- Setting visibility to `public` or `owner` SHALL clear any password.
- Setting visibility to `password` without a password SHALL fail with 400 `form.password_required`, unless the share already has a password.
- A `password` when the resulting visibility is not `password` SHALL fail with 400 `form.password_unused`.

Editing SHALL apply only to active shares.

#### Scenario: Change only the expiry
- **WHEN** a password share is patched with `{"expires_at": null}`
- **THEN** its path, visibility and password are unchanged and it never expires

#### Scenario: Public clears the password
- **WHEN** a password share is patched with `{"visibility": "public"}`
- **THEN** the share is public and opens without a password

#### Scenario: Password on a public share
- **WHEN** a public share is patched with `{"password": "x"}` only
- **THEN** the response is 400 with code `form.password_unused` and the share is unchanged

#### Scenario: Path taken by another share
- **WHEN** a share is patched to a path held by another active share
- **THEN** the response is 409 with code `path.conflict`

### Requirement: Trash and restore through the API
`POST /api/v1/shares/{id}/trash` SHALL move an active share to the trash and return the share object. Calling it on a share that is already trashed or purging SHALL return 200 with the share unchanged.

`POST /api/v1/shares/{id}/restore` SHALL restore a trashed share at its old path, or at the path given in an optional JSON `{"path"}`. Restoring an active or purging share SHALL fail with 409 `share.not_trashed`. Restoring onto a taken path SHALL fail with 409 `restore.conflict`.

#### Scenario: Trash twice
- **WHEN** trash is called twice for the same share
- **THEN** both calls return 200 and the share is trashed once, with the first `trashed_at`

#### Scenario: Restore to a new path
- **WHEN** a trashed share's old path is taken and restore is called with `{"path": "notes-2"}`
- **THEN** the share is active at `notes-2`

### Requirement: What the API does not do
No route under `/api/v1` SHALL purge a share, change the owner's preferences, or create, list or revoke tokens.

#### Scenario: Purge route does not exist
- **WHEN** `POST /api/v1/shares/{id}/purge` is called with a valid token
- **THEN** the response is 404 in the error shape and the share is unchanged

### Requirement: Read share files
`GET /api/v1/shares/{id}/files` SHALL list every file of an active share with its relative path and size, whatever the share's visibility. `GET /api/v1/shares/{id}/files/{rel}` SHALL return that file's original bytes. `?rendered=1` SHALL return the rendered HTML of a Markdown file. A missing file SHALL get 404 `file.not_found`.

These file responses SHALL carry the same sandbox `Content-Security-Policy` as the share host, plus `X-Content-Type-Options: nosniff` and `Content-Disposition: attachment`. The Content-Type SHALL come from the file extension.

#### Scenario: Owner-only share
- **WHEN** an owner-only share holding `index.md` is read with `GET .../files/index.md?rendered=1`
- **THEN** the response is 200 HTML containing the rendered Markdown, with the sandbox CSP

#### Scenario: Missing file
- **WHEN** a file path that is not in the share is requested
- **THEN** the response is 404 with code `file.not_found`

