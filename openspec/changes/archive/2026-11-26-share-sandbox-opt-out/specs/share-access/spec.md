## MODIFIED Requirements

### Requirement: Uploaded content is sandboxed
Every response that serves uploaded or rendered content, including downloads, SHALL carry `Content-Security-Policy: sandbox allow-scripts allow-forms allow-popups allow-modals allow-downloads` without `allow-same-origin`, plus `X-Content-Type-Options: nosniff`. The one exception is a share whose sandbox setting is off: its files and rendered pages SHALL carry the same directive with `allow-same-origin` added, while its downloads keep the directive without it. Share file responses SHALL carry `Access-Control-Allow-Origin: *`. Downloads SHALL also carry `Content-Disposition: attachment`. The Content-Type SHALL come from the file extension. dropgo's pages at a share URL that show no uploaded content, such as the password form and error pages, SHALL NOT be able to load scripts, whatever the share's sandbox setting.

#### Scenario: Script tries to read cookies
- **WHEN** an uploaded page of a share with the sandbox on runs `document.cookie`
- **THEN** it throws a SecurityError

#### Scenario: Script tries to call the admin host
- **WHEN** an uploaded page sends a `fetch` POST with credentials to the admin host
- **THEN** the admin host does not apply the change, whatever the share's sandbox setting

#### Scenario: Font in an uploaded page
- **WHEN** a public share's `index.html` uses a font file from the same share in `@font-face`
- **THEN** the font loads

#### Scenario: HTML download
- **WHEN** the `html` download of a Markdown share that contains a `<script>` tag is requested
- **THEN** the response has `Content-Disposition: attachment` and the sandbox CSP

#### Scenario: Sandbox off
- **WHEN** any file or rendered page of a share with the sandbox off is requested
- **THEN** the CSP is the sandbox directive with `allow-same-origin` added and nothing else changed

#### Scenario: Camera with the sandbox off
- **WHEN** a page of a share with the sandbox off calls `navigator.mediaDevices.getUserMedia({video: true})` in a browser with a fake camera and media permission granted
- **THEN** the call resolves with a video stream

#### Scenario: Download with the sandbox off
- **WHEN** the `zip` download of a share with the sandbox off is requested
- **THEN** the CSP is the sandbox directive without `allow-same-origin`

#### Scenario: Password form with the sandbox off
- **WHEN** a locked password share with the sandbox off shows its password form
- **THEN** the form response's CSP blocks scripts

#### Scenario: Sandbox turned back on
- **WHEN** a share's sandbox setting changes from off to on
- **THEN** its next responses carry the sandbox directive without `allow-same-origin`

## ADDED Requirements

### Requirement: No Service Workers on the share host
The share host SHALL answer 403 to any request that carries the header `Service-Worker: script`, whatever the share's sandbox setting.

#### Scenario: Register a worker
- **WHEN** a page of a share with the sandbox off calls `navigator.serviceWorker.register('sw.js')` for a `sw.js` in the same share
- **THEN** the registration fails and no worker is installed

#### Scenario: Ordinary script
- **WHEN** a page loads `app.js` from its share with a `<script>` tag
- **THEN** the script is returned with status 200
