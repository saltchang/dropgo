## Why

Every share response carries `Content-Security-Policy: sandbox` without `allow-same-origin`, so an uploaded page runs with an opaque origin. Browser features that need a real origin fail there: `getUserMedia` always throws `SecurityError: Invalid security origin`, so a page that records video in the browser cannot work as a share. The owner wants to turn that isolation off for single shares that they wrote themselves.

## What Changes

- Each share gets a sandbox setting. It is on by default. With it off, the share's files and rendered pages add `allow-same-origin` to the same sandbox directive and change nothing else.
- The setting can be chosen at upload and changed later, in the Web UI, the API and the CLI.
  - Web UI: a checkbox on the upload page and the edit page, with a warning while it is checked, and a warning badge in the share list. Prototype: `proto/share-sandbox-opt-out.html` on branch `proto/share-sandbox-opt-out`.
  - API: a `sandbox` field on upload and edit, and on the share object.
  - CLI: `dropgo upload --no-sandbox`, and `dropgo edit --sandbox` or `dropgo edit --no-sandbox`. `dropgo get` and `dropgo list` show the setting through the share object.
  - Agent skill: the agent asks the owner before it turns a share's sandbox off.
- The share host refuses Service Worker script requests, so a share with the sandbox off cannot leave a worker behind that outlives the setting.
- Existing shares, and new shares that do not set it, keep the sandbox on.

## Non-goals

- Moving shares with the sandbox off to another domain.
- Changing the API's file responses. They keep the full sandbox.
- The reverse proxy's `Permissions-Policy`. It already allows camera and microphone for the same origin.

## Decisions

From the requester:

- Accept the risk that a share with the sandbox off is same-site with the other services under the same domain. Its scripts can read the owner's other shares with the visitor's cookies and send requests to same-site services. Only the owner can upload, and the owner turns this on only for pages they wrote that load no outside scripts.
- The CLI uses `dropgo upload --no-sandbox`, `dropgo edit --sandbox` and `dropgo edit --no-sandbox`.
- The Web UI shows and changes the setting on the upload page, the edit page and the share list, like visibility and expiry, so the owner can see which shares are not isolated.

From the AI:

- The API field is a boolean `sandbox`, `true` by default, so the share object reads the same way as the CLI flags.
- `dropgo upload --replace` and the Web UI overwrite keep the existing setting unless it is given, as they do for visibility and expiry. An API upload without the field turns the sandbox on, so an old client never turns it off by accident. `details.existing` on `path.exists` carries `sandbox` so the CLI can keep it.
- The share host's download links keep the full sandbox, unlike the issue's scope, which leaves them in. They answer with `Content-Disposition: attachment`, so the browser saves them and never runs them as a page.
- The share host refuses requests with `Service-Worker: script`. With `allow-same-origin`, a page could register a worker that keeps serving responses without the sandbox after the owner turns it back on, and that also covers new shares under the same path. Shares with the sandbox on cannot register workers today, so nothing they do changes.
- Rendered Markdown pages follow the setting, like the share's files.
- dropgo's pages at a share URL that show no uploaded content, such as the password form and error pages, keep their own policy that blocks scripts.
- The admin host keeps refusing requests from the share host, so a share with the sandbox off still cannot change settings through the Web UI.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `share-access`: the sandbox requirement depends on the share's setting.
- `admin-ui`: the upload page, edit page and share list show the setting.
- `agent-api`: upload, edit and the share object carry `sandbox`.
- `agent-cli`: upload and edit take the sandbox flags.
- `agent-skill`: the skill tells the agent to ask before turning the sandbox off.

## Impact

- A migration adds a `sandbox` column to shares, `true` for every existing row.
- `src/share.rs` picks the sandbox header per share and refuses Service Worker scripts.
- `src/upload.rs`, `src/admin.rs`, `src/api.rs`, `src/cli.rs`, the admin templates, `static/upload.mjs` and both i18n files change.
- The public API gains a field.
- `skills/dropgo/SKILL.md` documents the flags and the rule to ask first. `README.md` and `README.zh-TW.md` document the setting, that a share with the sandbox off is same-site with the other services under the share host's site, and that a reverse proxy's `Permissions-Policy` must not block camera and microphone for the share host.
