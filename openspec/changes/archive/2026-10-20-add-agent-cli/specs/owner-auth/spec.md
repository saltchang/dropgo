## MODIFIED Requirements

### Requirement: Admin host requires owner sign-in
Every admin host route except the OIDC callback, `/_dropgo/healthz`, static assets and routes under `/api/v1` SHALL require a signed-in owner session. A request without one SHALL be redirected to the OIDC provider. Routes under `/api/v1` SHALL require an API token instead, as the `agent-api` capability describes.

#### Scenario: Signed-out visit
- **WHEN** a browser with no session requests `/` on the admin host
- **THEN** the response is a 302 to the provider's authorization endpoint with a PKCE challenge

#### Scenario: Signed-in visit
- **WHEN** a browser with a valid owner session requests `/`
- **THEN** the upload page is returned with status 200

### Requirement: Cross-site requests to the admin host are refused
Every admin host request with a method other than GET, HEAD or OPTIONS SHALL be refused with 403 unless its `Origin` header equals `DROPGO_ADMIN_ORIGIN`. Requests under `/api/v1` are exempt, because those routes never read the session cookie.

#### Scenario: Request from the share host
- **WHEN** a page on the share host sends a POST to an admin host endpoint with the owner's cookie
- **THEN** the response is 403 and nothing changes

#### Scenario: Missing Origin header
- **WHEN** a POST to the admin host outside `/api/v1` has no `Origin` header
- **THEN** the response is 403
