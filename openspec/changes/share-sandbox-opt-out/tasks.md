## 1. Storage and the share host

- [x] 1.1 Migration `0003_share_sandbox.sql` and `ShareRow.sandbox`
- [x] 1.2 Read the `sandbox` upload field, with `form.bad_sandbox` in `en` and `zh-TW`, and store it on insert and overwrite
- [x] 1.3 `content()` picks the directive from the share's setting; downloads keep the full sandbox
- [x] 1.4 The share host answers 403 to `Service-Worker: script`
- [x] 1.5 Integration tests for each `share-access` scenario that a request can check

## 2. API

- [x] 2.1 `sandbox` in the share object and in `details.existing` of `path.exists`
- [x] 2.2 `sandbox` in `PATCH /api/v1/shares/{id}`, through `save_edit`
- [x] 2.3 Integration tests for each changed `agent-api` scenario, including the file read keeping the full sandbox

## 3. Web UI

- [x] 3.1 Upload page: the checkbox, hint and warning, and `upload.mjs` sending it and filling it from the overwrite prompt
- [x] 3.2 Edit page: the checkbox starting from the share's setting, and saving it
- [x] 3.3 Share list: the warning badge
- [x] 3.4 Integration tests for the `admin-ui` scenarios a request can check

## 4. CLI

- [x] 4.1 `upload --no-sandbox`, with `--replace` keeping the existing setting
- [x] 4.2 `edit --sandbox` and `edit --no-sandbox`, conflicting with each other
- [x] 4.3 End-to-end tests for each `agent-cli` scenario

## 5. Skill, docs and browser checks

- [ ] 5.1 `skills/dropgo/SKILL.md`: the flags and the rule to ask first, with a test for the rule
- [ ] 5.2 README (en and zh-TW): the setting, the same-site risk and the reverse proxy's `Permissions-Policy`
- [ ] 5.3 `qa/qa.mjs`: the warning, the badge, `getUserMedia` with a fake camera and a refused worker registration
- [ ] 5.4 Run the full verification: `cargo fmt`, `cargo test`, `web` tests and qa
