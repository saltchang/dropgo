## ADDED Requirements

### Requirement: Visibility modes
Each share SHALL have exactly one visibility: `public`, `password` or `owner`. New shares SHALL default to `owner`.

#### Scenario: Public share
- **WHEN** a signed-out visitor opens a public share
- **THEN** the content is returned with status 200

#### Scenario: Owner-only share, another user
- **WHEN** a user who is signed in but does not own the share opens an owner-only share
- **THEN** the response is 404

#### Scenario: Owner-only share, the owner
- **WHEN** the share's owner, signed in on the share host, opens it
- **THEN** the content is returned with status 200, including its images and stylesheets

### Requirement: Share root serves the entry
A request for `/<path>/` SHALL return the share's entry file. If the entry is Markdown, the rendered page SHALL be returned.

#### Scenario: Single-file share
- **WHEN** a visitor opens the link of a share created from `notes.md`
- **THEN** the rendered `notes.md` is shown

### Requirement: Password unlock
A password share SHALL show a form on page navigation until the visitor enters the right password. Other requests to a locked share SHALL get 401. A request without `Sec-Fetch-Mode` SHALL count as a page navigation when its `Accept` header includes `text/html`. A correct password SHALL set an unlock cookie for that share that lasts 7 days and works for every URL of the share, including downloads and any capitalization of the share path. Changing the password SHALL invalidate existing unlock cookies. The signed-in owner SHALL skip the form.

#### Scenario: Correct password
- **WHEN** a visitor submits the right password
- **THEN** they are redirected to the URL they first opened on the same host, and the share and its subresources load

#### Scenario: Direct file link
- **WHEN** a visitor opens `/doc/report.pdf` of a locked share and submits the right password
- **THEN** they land on `/doc/report.pdf`

#### Scenario: Locked subresource
- **WHEN** a request for an image in a locked share is not a page navigation
- **THEN** the response is 401

#### Scenario: Download after unlock
- **WHEN** an unlocked visitor follows a download link of that share
- **THEN** the download starts without the form

#### Scenario: Password changed
- **WHEN** the owner changes the password and a visitor returns with an old unlock cookie
- **THEN** the form is shown again

#### Scenario: Owner bypass
- **WHEN** the signed-in owner opens their own password share
- **THEN** the content is returned without the form

### Requirement: Access token in the path
A page navigation that passes the access check for a password or owner-only share SHALL be redirected to the same URL under `/_dropgo/t/<token>/`. A request under that prefix SHALL be allowed without cookies when the token is valid for the share it names. A token SHALL last 12 hours and SHALL stop working when the share's password or visibility changes. A page navigation with an invalid or expired token SHALL be redirected to the URL without the prefix; any other request with one SHALL get 401.

#### Scenario: Subresources without cookies
- **WHEN** an unlocked visitor opens a password share in a browser that sends no cookies on requests from the page
- **THEN** the page's images, stylesheets and fonts load

#### Scenario: Token for another share
- **WHEN** a token issued for share `a` is used under the prefix for a URL of share `b`
- **THEN** the response is 401 for a subresource and a redirect without the prefix for a page navigation

#### Scenario: Expired token
- **WHEN** a visitor whose unlock cookie is still valid opens a token URL 13 hours after it was issued
- **THEN** they are redirected to the URL without the prefix and then to a fresh token URL

#### Scenario: Password changed
- **WHEN** the owner changes the password of a share
- **THEN** existing token URLs for that share stop working

#### Scenario: Public share
- **WHEN** a visitor opens a public share
- **THEN** no redirect to a token URL happens

### Requirement: Password attempt limit
The system SHALL allow at most 5 wrong passwords per minute for each client IP and share. Further attempts SHALL get 429.

#### Scenario: Sixth wrong attempt
- **WHEN** one IP submits 6 wrong passwords to one share within a minute
- **THEN** the sixth attempt gets 429, even if that password is correct

### Requirement: Uploaded content is sandboxed
Every response that serves uploaded or rendered content, including downloads, SHALL carry `Content-Security-Policy: sandbox allow-scripts allow-forms allow-popups allow-modals allow-downloads` without `allow-same-origin`, plus `X-Content-Type-Options: nosniff`. Share file responses SHALL carry `Access-Control-Allow-Origin: *`. Downloads SHALL also carry `Content-Disposition: attachment`. The Content-Type SHALL come from the file extension. dropgo's own pages, including the password form served at a share URL, SHALL NOT be able to load scripts.

#### Scenario: Script tries to read cookies
- **WHEN** an uploaded page runs `document.cookie`
- **THEN** it throws a SecurityError

#### Scenario: Script tries to call the admin host
- **WHEN** an uploaded page sends a `fetch` POST with credentials to the admin host
- **THEN** the admin host does not apply the change

#### Scenario: Font in an uploaded page
- **WHEN** a public share's `index.html` uses a font file from the same share in `@font-face`
- **THEN** the font loads

#### Scenario: HTML download
- **WHEN** the `html` download of a Markdown share that contains a `<script>` tag is requested
- **THEN** the response has `Content-Disposition: attachment` and the sandbox CSP

### Requirement: No indexing
Every share host response SHALL carry `X-Robots-Tag: noindex, nofollow, noarchive`. The share host SHALL NOT serve a `robots.txt` that blocks crawling, so crawlers can see that header.

#### Scenario: Header on a public share
- **WHEN** any file of a public share is requested
- **THEN** the response has the `X-Robots-Tag` header

### Requirement: Status for missing or ended shares
The share host SHALL return 410 with a short page for an expired or trashed share, 404 for a path that matches no share, and a 301 from `/<path>` to `/<path>/` for a share root without a trailing slash. When a URL matches both an active share and a trashed one, the active share SHALL serve it.

#### Scenario: Expired share
- **WHEN** a visitor opens a share whose expiry time has passed
- **THEN** the response is 410, even if the hourly job has not run yet

#### Scenario: Active share over a trashed one
- **WHEN** a trashed share is at `notes/2026`, an active share is at `notes` with a `2026/` directory, and a visitor opens `/notes/2026/`
- **THEN** the active share serves its `2026/` directory

#### Scenario: Purged share
- **WHEN** a visitor opens the path of a share that was purged
- **THEN** the response is 404

#### Scenario: Missing trailing slash
- **WHEN** a visitor opens `/notes/2026`
- **THEN** the response is a 301 to `/notes/2026/`

### Requirement: Path lookup is case-insensitive for the share path only
The share path part of a URL SHALL match regardless of case. File paths inside a share SHALL match exactly.

#### Scenario: Uppercase share path
- **WHEN** a visitor opens `/Notes/2026/Img/A.png` and the share is `notes/2026` with a file `Img/A.png`
- **THEN** the file is served

### Requirement: Visitor theme
dropgo's own pages on the share host (rendered Markdown, the password form, error pages) SHALL offer a dark and light toggle that works without scripts. The theme SHALL be the choice kept in a browser cookie, else the signed-in owner's saved theme, else dark. The toggle SHALL only write the browser cookie. Uploaded HTML SHALL get no toggle. These pages SHALL carry `Cache-Control: private`.

#### Scenario: Visitor switches theme
- **WHEN** a signed-out visitor switches a rendered page to light and later opens another share
- **THEN** both pages are shown in light

#### Scenario: Default
- **WHEN** a visitor with no theme cookie opens a rendered page
- **THEN** it is shown in dark

#### Scenario: Owner default on the share host
- **WHEN** the signed-in owner, whose saved theme is light and who has no theme cookie, opens a rendered page
- **THEN** it is shown in light

#### Scenario: Cross-site toggle
- **WHEN** another site embeds `<img src="https://s.example.com/_dropgo/theme?set=light">` and the signed-in owner opens it
- **THEN** the owner's saved theme does not change

#### Scenario: Not cached across visitors
- **WHEN** a rendered page is requested
- **THEN** the response has `Cache-Control` with `private`

#### Scenario: Foreign return path
- **WHEN** the toggle is called with `back=//evil.example/`
- **THEN** the browser is sent to `/` of the share host
