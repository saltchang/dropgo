# owner-auth Specification

## Purpose
TBD - created by archiving change add-dropgo-mvp. Update Purpose after archive.
## Requirements
### Requirement: Admin host requires owner sign-in
Every admin host route except the OIDC callback, `/_dropgo/healthz`, static assets and routes under `/api/v1` SHALL require a signed-in owner session. A request without one SHALL be redirected to the OIDC provider. Routes under `/api/v1` SHALL require an API token instead, as the `agent-api` capability describes.

#### Scenario: Signed-out visit
- **WHEN** a browser with no session requests `/` on the admin host
- **THEN** the response is a 302 to the provider's authorization endpoint with a PKCE challenge

#### Scenario: Signed-in visit
- **WHEN** a browser with a valid owner session requests `/`
- **THEN** the upload page is returned with status 200

### Requirement: Owner allowlist
The system SHALL allow sign-in only when the configured claim (default `preferred_username`) matches an entry in `DROPGO_OWNERS`. The claim SHALL be read from the ID token, and from the userinfo endpoint when the ID token lacks it; a userinfo response whose `sub` differs from the ID token's SHALL fail the sign-in. It SHALL check this on every sign-in. Sessions SHALL expire 30 days after sign-in, whatever the activity. Every request SHALL check that the session's user still has an owner claim in `DROPGO_OWNERS`.

#### Scenario: Allowed user
- **WHEN** the provider returns an ID token whose `preferred_username` is in `DROPGO_OWNERS`
- **THEN** a session is created and the browser is redirected to the page it first asked for

#### Scenario: Claim only in userinfo
- **WHEN** the ID token has no owner claim and the userinfo response for the same `sub` has one in `DROPGO_OWNERS`
- **THEN** a session is created as for an allowed user

#### Scenario: User not on the allowlist
- **WHEN** the ID token's claim is not in `DROPGO_OWNERS`
- **THEN** no session is created and the response is 403

#### Scenario: Removed from the allowlist
- **WHEN** a user is removed from `DROPGO_OWNERS`
- **THEN** their next sign-in is refused with 403, and their current session stops working on its next request

### Requirement: Safe return after sign-in
The page to return to after sign-in SHALL NOT be taken from a URL parameter at callback time. It SHALL only be a path on the same host that starts with a single `/`. Any other value SHALL be replaced by `/`.

#### Scenario: Foreign return URL
- **WHEN** a sign-in is started with a return target of `https://evil.example/` or `//evil.example/`
- **THEN** after sign-in the browser lands on `/` of the same host

### Requirement: Users are keyed by issuer and subject
On the first allowed sign-in the system SHALL create a user and an identity whose `(issuer, subject)` pair is unique. Later sign-ins SHALL find the user by `(issuer, subject)`.

#### Scenario: First sign-in
- **WHEN** an allowed user signs in and no identity exists for their `(issuer, subject)`
- **THEN** one `users` row and one `identities` row are created

#### Scenario: Username changed at the provider
- **WHEN** a user with an existing identity signs in with the same subject and a new `preferred_username` that is also allowed
- **THEN** the existing user is used and no new user is created

### Requirement: Sign-in on the share host
The share host SHALL offer the same OIDC sign-in under `/_dropgo/auth/`. Its session cookie SHALL be `__Host-` prefixed, `Secure`, `HttpOnly`, `Path=/` and `SameSite=None`.

#### Scenario: Owner-only share triggers sign-in
- **WHEN** a signed-out visitor opens an owner-only share
- **THEN** they are sent through sign-in and come back to the same share URL

### Requirement: Cross-site requests to the admin host are refused
Every admin host request with a method other than GET, HEAD or OPTIONS SHALL be refused with 403 unless its `Origin` header equals `DROPGO_ADMIN_ORIGIN`. Requests under `/api/v1` are exempt, because those routes never read the session cookie.

#### Scenario: Request from the share host
- **WHEN** a page on the share host sends a POST to an admin host endpoint with the owner's cookie
- **THEN** the response is 403 and nothing changes

#### Scenario: Missing Origin header
- **WHEN** a POST to the admin host outside `/api/v1` has no `Origin` header
- **THEN** the response is 403

### Requirement: Health check on any host
`/_dropgo/healthz` SHALL answer 200 when the database is reachable, whatever the `Host` header, without sign-in.

#### Scenario: Probe by pod IP
- **WHEN** a probe requests `/_dropgo/healthz` with `Host: 10.0.0.5:8080`
- **THEN** the response is 200

