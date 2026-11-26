## MODIFIED Requirements

### Requirement: Upload page
The admin host root SHALL be the upload page. It has a drop zone at the top with one button that offers a folder or a single file (an .html, .md or .zip file), and a settings form below it with path, visibility, password, expiry and sandbox. Visibility offers only `owner` and `public`. The password field is always shown, disabled while visibility is `owner`, and has a button that shows the typed password. A `public` share with a password becomes a `password` share; with the field left empty it stays `public`. Expiry is one date and time field, empty for never, with buttons that fill in 1, 7 or 30 days from now or clear it. Sandbox is a checkbox that turns the sandbox off when checked; while it is checked, the form SHALL show a warning that the page's scripts can read the owner's other shares and send requests to other services on the same site. For a folder or a single file, the upload button SHALL stay disabled until the files pass the ignore and entry checks. A zip SHALL be checked by the server, which reports any error on the page.

#### Scenario: Successful upload
- **WHEN** the owner drops a folder, fills the form and presses upload
- **THEN** the page shows the share URL with a copy button

#### Scenario: Checks fail
- **WHEN** the selected folder has no entry file
- **THEN** the upload button stays disabled and the page explains why

#### Scenario: Zip with an error
- **WHEN** the owner uploads a zip with no entry file
- **THEN** the page shows the server's error message

#### Scenario: Form defaults
- **WHEN** the upload page loads
- **THEN** visibility is `owner`, the expiry field is empty and the sandbox checkbox is unchecked, whatever was chosen last time

#### Scenario: Password makes a password share
- **WHEN** the owner picks `public` and types a password
- **THEN** the share is created with `password` visibility

#### Scenario: Sandbox off at upload
- **WHEN** the owner checks the sandbox checkbox and uploads
- **THEN** the page shows the warning before the upload, and the share is created with the sandbox off

#### Scenario: Overwrite prompt
- **WHEN** the entered path belongs to one of the owner's active shares
- **THEN** the form fills in that share's visibility, expiry and sandbox setting and asks for confirmation before uploading
- **AND** for a password share, an empty password field keeps the existing password

### Requirement: Share list
A list page reachable from the top nav SHALL show every active share of the owner with its URL, visibility, entry file, type (HTML or Markdown), expiry, size and creation time, newest first. A share with the sandbox off SHALL also show a warning badge that says so. A search field SHALL match, as the owner types, any of the text a row shows. Two date fields SHALL limit the list to shares created in that range, in the browser's time zone. Each row SHALL offer copy link, edit and delete.

#### Scenario: Search
- **WHEN** the owner types `markdown` into the search
- **THEN** only Markdown shares are listed

#### Scenario: Date range
- **WHEN** the owner sets a date range with no shares created in it
- **THEN** the list shows that nothing matches

#### Scenario: Sandbox badge
- **WHEN** the owner has one share with the sandbox off and one with it on
- **THEN** only the share with the sandbox off shows the badge

### Requirement: Edit settings
Edit SHALL let the owner change a share's path, visibility, password, expiry and sandbox setting, with the same fields as the upload page. The sandbox checkbox SHALL start with the share's current setting. An empty password field SHALL keep the current password, and a checkbox SHALL remove it. Leaving the expiry field as it was SHALL keep the expiry. The upload rules for paths apply.

#### Scenario: Change path
- **WHEN** the owner changes a path from `a` to `b`
- **THEN** the share is served at `/b/` and `/a/` returns 404

#### Scenario: Turn the sandbox back on
- **WHEN** the owner opens the edit page of a share with the sandbox off, unchecks the sandbox checkbox and saves
- **THEN** the share has the sandbox on and its path, visibility, password and expiry are unchanged
