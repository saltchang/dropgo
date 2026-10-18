## ADDED Requirements

### Requirement: Upload page
The admin host root SHALL be the upload page. It has a drop zone at the top with one button that offers a folder or a single file (an .html, .md or .zip file), and a settings form below it with path, visibility, password and expiry. Visibility offers only `owner` and `public`. The password field is always shown, disabled while visibility is `owner`, and has a button that shows the typed password. A `public` share with a password becomes a `password` share; with the field left empty it stays `public`. Expiry is one date and time field, empty for never, with buttons that fill in 1, 7 or 30 days from now or clear it. For a folder or a single file, the upload button SHALL stay disabled until the files pass the ignore and entry checks. A zip SHALL be checked by the server, which reports any error on the page.

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
- **THEN** visibility is `owner` and the expiry field is empty, whatever was chosen last time

#### Scenario: Password makes a password share
- **WHEN** the owner picks `public` and types a password
- **THEN** the share is created with `password` visibility

#### Scenario: Overwrite prompt
- **WHEN** the entered path belongs to one of the owner's active shares
- **THEN** the form fills in that share's visibility and expiry and asks for confirmation before uploading
- **AND** for a password share, an empty password field keeps the existing password

### Requirement: Share list
A list page reachable from the top nav SHALL show every active share of the owner with its URL, visibility, entry file, type (HTML or Markdown), expiry, size and creation time, newest first. A search field SHALL match, as the owner types, any of the text a row shows. Two date fields SHALL limit the list to shares created in that range, in the browser's time zone. Each row SHALL offer copy link, edit and delete.

#### Scenario: Search
- **WHEN** the owner types `markdown` into the search
- **THEN** only Markdown shares are listed

#### Scenario: Date range
- **WHEN** the owner sets a date range with no shares created in it
- **THEN** the list shows that nothing matches

### Requirement: Edit settings
Edit SHALL let the owner change a share's path, visibility, password and expiry, with the same fields as the upload page. An empty password field SHALL keep the current password, and a checkbox SHALL remove it. Leaving the expiry field as it was SHALL keep the expiry. The upload rules for paths apply.

#### Scenario: Change path
- **WHEN** the owner changes a path from `a` to `b`
- **THEN** the share is served at `/b/` and `/a/` returns 404

### Requirement: Trash tab
The list page SHALL have a trash tab showing trashed shares with the number of days left before purge, a restore action and a purge action that asks for confirmation. A share being purged SHALL be shown as deleting, without restore or purge actions.

#### Scenario: Days left
- **WHEN** a share was trashed 10 days ago
- **THEN** the trash tab shows 20 days left

### Requirement: Language and theme
The admin UI SHALL be available in English and Traditional Chinese (`zh-TW`). The theme SHALL be dark by default, with a light option. On first sign-in, the language SHALL default to the best match of `Accept-Language`, falling back to English. The choice SHALL be saved on the user record and apply on every device.

#### Scenario: Saved on the server
- **WHEN** the owner switches to light theme and `zh-TW` on one device and then signs in on another
- **THEN** the second device shows the light theme in `zh-TW`

### Requirement: Mobile layout
Every admin page SHALL work on a 360 px wide screen without horizontal scrolling. On devices without folder selection, the page SHALL offer file and zip selection.

#### Scenario: Phone upload
- **WHEN** the owner opens the upload page on a phone and picks a zip
- **THEN** the upload completes the same way as on desktop
