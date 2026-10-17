## 1. Sandbox cookie PoC (blocks everything else)

- [ ] 1.1 Build a throwaway axum server that serves a sandboxed page whose image, CSS and KaTeX font need a `__Host-` `SameSite=None; Path=/` cookie
- [ ] 1.2 Run it with Playwright in Chromium, Firefox and WebKit, then in Chrome with third-party cookies blocked and in real Safari on macOS or iOS. Record which ones send the cookie
- [ ] 1.3 If any engine fails, stop and update design.md and the share-access spec with the signed-token fallback before going on

## 2. Project skeleton

- [ ] 2.1 `cargo new dropgo`, add the crates named in design.md with `cargo add`, and use rustls only
- [ ] 2.2 Config through clap with the env feature: origins, OIDC, owners, secret, database URL, object store URL, size limit, trusted proxies
- [ ] 2.3 Routing by host, with 404 for unknown hosts and `/_dropgo/healthz` on any host
- [ ] 2.4 sqlx migration for `users`, `identities` (unique `(issuer, subject)`) and `shares` (partial unique index on active paths), and call the session store's own migrate
- [ ] 2.5 object_store setup from a URL (`s3://`, `file://`, `memory://`)
- [ ] 2.6 Test harness: testcontainers Postgres plus the in-memory store, and one smoke test
- [ ] 2.7 Dockerfile (musl, distroless) and GitHub Actions that run tests and push a multi-arch image to ghcr on tags

## 3. Owner auth

- [ ] 3.1 OIDC code flow with PKCE on the admin host: return URL kept in the session, allowlist check, users and identities created on first sign-in, 12-hour sessions
- [ ] 3.2 The same flow on the share host under `/_dropgo/auth/`, with `SameSite=None` session cookies
- [ ] 3.3 Origin check middleware on the admin host
- [ ] 3.4 Tests for every scenario in the owner-auth spec, using a mock OIDC provider

## 4. Upload

- [ ] 4.1 Share path parser and validator, with unit tests for the path rules
- [ ] 4.2 Path claim under `pg_advisory_xact_lock`: trash conflicting expired shares, then check prefixes. Test it with two uploads at once
- [ ] 4.3 Upload handler and spawned upload task, as in design "Uploads", with tests for the limits and for a dropped connection
- [ ] 4.4 After the ignore rules, strip a single top-level directory, then detect the entry file
- [ ] 4.5 Server-side zip unpacking and ignore rules, as in design "Uploads"
- [ ] 4.6 Overwrite: switch `upload_id`, then delete the old prefix, with a test where a store write fails partway
- [ ] 4.7 Shared ignore fixture suite that the Rust tests and a small Node test both run

## 5. Serving and access

- [ ] 5.1 Share lookup by prefix candidates, plus the 301, 404 and 410 handling
- [ ] 5.2 Response headers: sandbox CSP, nosniff, X-Robots-Tag, Referrer-Policy, a script-free CSP on `/_dropgo/` pages, CORS `*` on `/_dropgo/assets/`, and attachment plus sandbox on downloads
- [ ] 5.3 Owner-only check
- [ ] 5.4 Password form on navigation (falling back to `Accept: text/html`) and 401 otherwise, a per-share `__Host-dropgo-unlock-<id>` HMAC cookie, owner bypass, and the in-memory attempt limit
- [ ] 5.5 Tests for every scenario in the share-access spec, plus a Playwright check that `document.cookie` throws on a share page

## 6. Markdown and downloads

- [ ] 6.1 comrak render at upload with syntect highlighting, stored under `<upload-id>/rendered/`
- [ ] 6.2 Page template with the dark stylesheet, the download footer, plus vendored mermaid and KaTeX as classic scripts
- [ ] 6.3 Download endpoint for `zip`, `source` and `html`, using the same access check
- [ ] 6.4 `dropgo rerender` subcommand
- [ ] 6.5 Tests for the markdown-rendering scenarios

## 7. Lifecycle

- [ ] 7.1 Hourly job, as in design "Lifecycle"
- [ ] 7.2 Delete, restore (through the same path claim as 4.2, clearing a past expiry) and purge actions
- [ ] 7.3 Tests for every scenario in the share-lifecycle spec, with the clock injected

## 8. Admin UI

- [ ] 8.1 Base layout with nav, dark and light themes, and TOML string catalogs for `en` and `zh-TW`, plus the test that their keys match
- [ ] 8.2 Upload page: drop zone, folder, file and zip picker, client-side ignore for folders with the bundled npm `ignore` package, skipped-file preview, settings form with the browser's UTC offset on custom expiry, overwrite prompt that fills in the existing settings, and copy link when done
- [ ] 8.3 List page with filter, edit form and delete
- [ ] 8.4 Trash tab with days left, restore and purge
- [ ] 8.5 Saving theme and language to the user record from the nav toggle
- [ ] 8.6 Check every page at a 360 px width in Playwright

## 9. Docs and release

- [ ] 9.1 README.md in English: what it is, quick start with docker compose (Postgres, a local store and any OIDC provider), the env var table, and the reverse proxy notes, including the body size limit
- [ ] 9.2 README.zh-TW.md with the same content in Traditional Chinese (Taiwan usage)
- [ ] 9.3 Edit both READMEs with the humanizer skill, the i-have-adhd writing rules and the ponytail rules
- [ ] 9.4 MIT LICENSE, then tag v0.1.0
