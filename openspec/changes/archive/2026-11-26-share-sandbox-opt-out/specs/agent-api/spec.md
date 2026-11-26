## MODIFIED Requirements

### Requirement: Share object
Responses that return a share SHALL hold its id, path, full URL, state, visibility, sandbox setting as the boolean `sandbox`, entry file, file count, size in bytes, created time, expiry, trashed time and purge time. Times SHALL be RFC 3339 in UTC, and fields with no value SHALL be `null`. The response SHALL NOT contain the password or its hash.

#### Scenario: Get a trashed share
- **WHEN** `GET /api/v1/shares/{id}` is called for a share in the trash
- **THEN** the share object has `state` `trashed` and a `purge_at` 30 days after `trashed_at`

#### Scenario: Existing share
- **WHEN** a share created before this change is returned
- **THEN** its share object has `sandbox` `true`

### Requirement: Upload through the API
`POST /api/v1/shares` SHALL accept the same multipart fields as the Web UI upload and apply the same rules: inputs, ignore rules, limits, paths, visibility and overwrite. The field `sandbox` set to `false` SHALL turn the sandbox off; set to `true` or left out, the sandbox SHALL be on, including on an overwrite. Any other value SHALL fail with 400 `form.bad_sandbox`. The size limit SHALL be the configured share limit, not a smaller default. Settings fields SHALL come before the first file field, and a settings field after a file field SHALL fail with 400 `upload.field_order`. On success the response SHALL be 201 with the share object.

An upload to a path that holds one of the owner's active shares SHALL fail with 409 `path.exists` unless `overwrite` names that share's id.

#### Scenario: Upload a Markdown file
- **WHEN** a single `notes.md` is uploaded with `path=notes` and `visibility=owner`
- **THEN** the response is 201, `url` ends in `/notes/`, `visibility` is `owner` and `sandbox` is `true`

#### Scenario: Upload with the sandbox off
- **WHEN** an upload sends `sandbox=false`
- **THEN** the response is 201 with `sandbox` `false`

#### Scenario: Overwrite with the existing id
- **WHEN** an upload to an occupied path sends `overwrite` set to the existing share's id
- **THEN** the response is 201 and the path serves the new content

#### Scenario: Settings after a file
- **WHEN** a multipart body sends a file field before the `path` field
- **THEN** the response is 400 with code `upload.field_order` and no share is created

#### Scenario: Large upload
- **WHEN** a 50 MB folder is uploaded with the default 100 MB limit
- **THEN** the response is 201

### Requirement: Edit through the API
`PATCH /api/v1/shares/{id}` SHALL take a JSON body and change only the fields present: `path`, `visibility`, `password`, `expires_at`, `sandbox`.
- `expires_at: null` SHALL mean the share never expires. A time in the past SHALL fail with 400 `form.bad_expiry`.
- Setting visibility to `public` or `owner` SHALL clear any password.
- Setting visibility to `password` without a password SHALL fail with 400 `form.password_required`, unless the share already has a password.
- A `password` when the resulting visibility is not `password` SHALL fail with 400 `form.password_unused`.
- `sandbox` SHALL be a boolean; `false` turns the sandbox off and `true` turns it on. Any other value SHALL fail with 400 `api.bad_request`.

Editing SHALL apply only to active shares.

#### Scenario: Change only the expiry
- **WHEN** a password share with the sandbox off is patched with `{"expires_at": null}`
- **THEN** its path, visibility, password and sandbox setting are unchanged and it never expires

#### Scenario: Public clears the password
- **WHEN** a password share is patched with `{"visibility": "public"}`
- **THEN** the share is public and opens without a password

#### Scenario: Password on a public share
- **WHEN** a public share is patched with `{"password": "x"}` only
- **THEN** the response is 400 with code `form.password_unused` and the share is unchanged

#### Scenario: Path taken by another share
- **WHEN** a share is patched to a path held by another active share
- **THEN** the response is 409 with code `path.conflict`

#### Scenario: Turn the sandbox off
- **WHEN** a share is patched with `{"sandbox": false}`
- **THEN** the response holds `sandbox` `false` and the share's other settings are unchanged

### Requirement: Error shape
Every error response from `/api/v1` SHALL be JSON of the form `{"error": {"code", "message", "details"}}`. That includes unknown routes, wrong methods, malformed ids, malformed JSON bodies and bodies over the size limit. `code` SHALL be a key of the message catalog. `message` SHALL be the English catalog text whatever the owner's language. `details` SHALL be an object. It SHALL hold `arg` for messages that take an argument, and `existing` with the share's id, visibility, expiry and sandbox setting for `path.exists`.

#### Scenario: English message for a zh-TW owner
- **WHEN** an owner whose language is zh-TW uploads more than the size limit through the API
- **THEN** the response is 413 with code `upload.too_big` and an English message

#### Scenario: Path taken by the owner
- **WHEN** an upload names a path where the owner already has an active share with the sandbox off
- **THEN** the response is 409 with code `path.exists`, `details.existing.id` set to that share's id and `details.existing.sandbox` set to `false`

#### Scenario: Unknown route and bad id
- **WHEN** `GET /api/v1/nope` or `GET /api/v1/shares/not-a-uuid` is called with a valid token
- **THEN** the response is JSON in the error shape, with code `api.not_found` or `api.bad_request`

### Requirement: Read share files
`GET /api/v1/shares/{id}/files` SHALL list every file of an active share with its relative path and size, whatever the share's visibility. `GET /api/v1/shares/{id}/files/{rel}` SHALL return that file's original bytes. `?rendered=1` SHALL return the rendered HTML of a Markdown file. A missing file SHALL get 404 `file.not_found`.

These file responses SHALL carry the sandbox `Content-Security-Policy` without `allow-same-origin`, whatever the share's sandbox setting, plus `X-Content-Type-Options: nosniff` and `Content-Disposition: attachment`. The Content-Type SHALL come from the file extension.

#### Scenario: Owner-only share
- **WHEN** an owner-only share holding `index.md` is read with `GET .../files/index.md?rendered=1`
- **THEN** the response is 200 HTML containing the rendered Markdown, with the sandbox CSP

#### Scenario: Missing file
- **WHEN** a file path that is not in the share is requested
- **THEN** the response is 404 with code `file.not_found`

#### Scenario: Share with the sandbox off
- **WHEN** a file of a share with the sandbox off is read through the API
- **THEN** the CSP has no `allow-same-origin`
