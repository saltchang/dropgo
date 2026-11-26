## Context

`src/share.rs` sets `Content-Security-Policy: sandbox ...` on every response that serves uploaded or rendered content, through `content()` and the `SANDBOX` constant. The API's file read in `src/api.rs` uses the same constant. dropgo's own pages on the share host (password form, error pages) get their own policy from `own_page()`, `default-src 'none'`, which already blocks scripts. Share settings live in the `shares` table and flow through three writers: the upload task (`upload::store_and_claim`), the Web UI edit (`admin::save_edit`) and the API edit, which builds an `admin::EditForm` and calls `save_edit`.

## Goals / Non-Goals

**Goals:**

- One boolean per share that decides whether its files and rendered pages get `allow-same-origin`.
- Every writer sets it, every reader shows it, and nothing else about the sandbox changes.

**Non-Goals:**

- The items listed under Non-goals in the proposal.

## Decisions

### Storage

Migration `0003_share_sandbox.sql` adds `sandbox boolean NOT NULL DEFAULT true` to `shares`. Existing rows get `true` from the default. `ShareRow` gains `sandbox: bool`; the queries use `SELECT *`, so they pick it up without changes.

### Headers

`content()` takes the share's setting. A second constant, `SANDBOX_SAME_ORIGIN`, holds the same directive with `allow-same-origin` added, and a unit test checks that it equals `SANDBOX` plus that token, so the two cannot drift apart. `serve_file` passes `share.sandbox`; `download` and the API file read always use `SANDBOX`. `own_page()` does not change.

### Service Workers

A check in the share router's middleware answers 403 to any request with `Service-Worker: script`, before routing. Browsers send this header only when fetching a worker script, so ordinary scripts and pages are unaffected. Doing it for every share, not only those with the sandbox off, keeps the rule simple and changes nothing for sandboxed shares, which cannot register workers anyway.

### Upload

The multipart field `sandbox` is read with the other settings. Empty or `true` means on, `false` means off, anything else fails with 400 `form.bad_sandbox` in the precheck, before files are read. The server never keeps the old value on an overwrite. The Web UI and the CLI keep it themselves: both already get the existing share's settings from `path.exists`, so `upload::Existing` gains `sandbox`, and they send it back unless the owner changed it. This is the same way they keep visibility and expiry.

### Edit

`EditForm` gains `no_sandbox: Option<String>`, the checkbox, named like `clear_password`. `save_edit` stores `sandbox = no_sandbox.is_none()`. The Web UI edit page always sends the checkbox state. The API edit fills `no_sandbox` from the JSON `sandbox` field when present and from the share's current value when absent, so "only the fields present change" holds without a second code path.

### Web UI

- Upload and edit pages: a checkbox `no_sandbox` with a hint and a warning notice, as in the prototype. The notice is hidden by CSS while the checkbox is unchecked (`:has(:checked)`), so it needs no script and works on the edit page, which has no page script of its own.
- `upload.mjs` sends `sandbox=false` when the checkbox is checked and `sandbox=true` otherwise, and the overwrite prompt sets the checkbox from `existing.sandbox`.
- The share list row shows a `badge warn` with the alert icon when the sandbox is off. The search matches its text like any other row text.

### CLI

`upload` gains `--no-sandbox`. With `--replace`, the setting comes from `details.existing.sandbox` unless `--no-sandbox` is given. `edit` gains `--sandbox` and `--no-sandbox`, declared with clap's `conflicts_with`, so giving both exits 2 like any other argument error. `get` and `list` print the share object, which now holds `sandbox`.

## Risks / Trade-offs

- [A share with the sandbox off can read the owner's other shares and send same-site requests] → Accepted by the requester. The setting is off by default, the Web UI warns while it is checked, the list marks such shares, and the agent skill asks before turning it off.
- [Such a page can store data in the share host's `localStorage` or IndexedDB, which every share with the sandbox off can read] → Part of the same accepted risk; shares with the sandbox on still get an opaque origin and cannot reach it.
- [A worker registered before this change] → None can exist: until now every share response has carried the sandbox without `allow-same-origin`, so no page could register one.

## Migration Plan

The migration runs at startup like the others and only adds a column with a default, so it does not rewrite existing rows. An older dropgo refuses to start against a database with a migration it does not know, so rolling back the binary also needs `ALTER TABLE shares DROP COLUMN sandbox; DELETE FROM _sqlx_migrations WHERE version = 3;`. After that, every share is served with the full sandbox again.
