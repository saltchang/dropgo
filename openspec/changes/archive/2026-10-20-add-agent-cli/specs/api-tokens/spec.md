## ADDED Requirements

### Requirement: Create a token in the Web UI
A signed-in owner SHALL be able to create an API token on the admin host's tokens page by giving a name and an expiry of 7, 30, 90 or 365 days, with 90 selected by default. The page SHALL show the full token exactly once, right after it is created. The token SHALL start with `dgo_`. The server SHALL NOT store the token itself, only a hash of it. Responses from the tokens page SHALL carry `Cache-Control: no-store`.

#### Scenario: Token shown once
- **WHEN** the owner creates a token named `laptop` with the default expiry
- **THEN** the page shows a token starting with `dgo_`, and the list shows `laptop` expiring 90 days from now
- **AND** reloading the tokens page no longer shows the full token

#### Scenario: Name is required
- **WHEN** the owner submits the form with an empty name
- **THEN** no token is created and the page shows an error

#### Scenario: Owner without a stored claim
- **WHEN** the owner has not signed in since the server started storing the owner claim
- **THEN** creating a token is refused and the page asks the owner to sign in again

### Requirement: List and revoke tokens
The tokens page SHALL list the owner's tokens with name, the first 8 characters after `dgo_`, created time, expiry, last used time and whether the token has expired. An owner SHALL see and revoke only their own tokens. Revoking a token SHALL take effect on the next API request that uses it.

#### Scenario: Revoked token stops working
- **WHEN** the owner revokes a token and an API request then uses it
- **THEN** the request gets 401 with code `auth.invalid_token`

#### Scenario: Another owner's token
- **WHEN** an owner posts a revoke for a token id that belongs to another owner
- **THEN** that token keeps working

#### Scenario: Last used time
- **WHEN** an API request succeeds with a token
- **THEN** the tokens page shows a last used time for that token

### Requirement: Tokens expire and follow the allowlist
An API request with a token past its expiry SHALL get 401 with code `auth.token_expired`. An API request with a token whose owner claim is no longer in `DROPGO_OWNERS` SHALL get 401 with code `auth.invalid_token`.

#### Scenario: Expired token
- **WHEN** a token's expiry has passed
- **THEN** `GET /api/v1/me` with it returns 401 with code `auth.token_expired`

#### Scenario: Owner removed from the allowlist
- **WHEN** the owner who created a token is removed from `DROPGO_OWNERS` and the server restarts
- **THEN** API requests with that token return 401
