# share-upload Specification

## Purpose
TBD - created by archiving change add-dropgo-mvp. Update Purpose after archive.
## Requirements
### Requirement: Accepted inputs
The upload SHALL accept a single `.html` or `.md` file, a folder, or a `.zip` file. A folder and a zip become one share with the same relative paths. After the ignore rules run, if every remaining file sits under one top-level directory, that directory SHALL be stripped.

#### Scenario: Single Markdown file
- **WHEN** the owner uploads `notes.md`
- **THEN** one share is created whose entry is `notes.md`

#### Scenario: Zip upload
- **WHEN** the owner uploads `site.zip` containing `index.html` and `img/a.png`
- **THEN** the share contains `index.html` and `img/a.png`

#### Scenario: Wrapped folder
- **WHEN** the owner drops a folder `site/` or uploads a zip whose only top-level entry is `site/`, holding `site/index.html`
- **THEN** the share contains `index.html` at its root

#### Scenario: macOS zip
- **WHEN** a zip holds `site/index.html` and `__MACOSX/site/._index.html`
- **THEN** the share contains `index.html` at its root and nothing from `__MACOSX/`

### Requirement: Entry file is required
Each share SHALL have an entry file: `index.html`, else `index.md`, else the single top-level file whose name ends in `.html` or `.md`, with both extensions counted together. An upload with no entry file SHALL be rejected.

#### Scenario: Two candidate files
- **WHEN** the top level holds `report.html` and `README.md` and no index file
- **THEN** the upload is rejected with a message that names the missing entry file

### Requirement: Ignore rules
The upload SHALL skip dotfiles and dot-directories, `node_modules` and `__MACOSX` directories, and files matched by a `.gitignore` in their own directory or any parent directory, whether or not a `.git` directory is present. It SHALL also skip files matched by a `.dockerignore` at the top level, with every pattern anchored at the top level as Docker does. No other ignore file SHALL be read. Folder and zip uploads SHALL give the same result.

#### Scenario: Nested gitignore
- **WHEN** a folder has `docs/.gitignore` containing `*.log` and holds `docs/a.log` and `a.log`
- **THEN** `docs/a.log` is skipped and `a.log` is kept

#### Scenario: Dockerignore is anchored
- **WHEN** the top-level `.dockerignore` contains `drafts` and the upload holds `drafts/a.md` and `docs/drafts/b.md`
- **THEN** `drafts/a.md` is skipped and `docs/drafts/b.md` is kept

#### Scenario: Dockerignore negation
- **WHEN** the top-level `.dockerignore` contains `drafts` and `!drafts/keep.md`
- **THEN** `drafts/keep.md` is kept and the rest of `drafts/` is skipped

#### Scenario: Other ignore files have no effect
- **WHEN** the upload holds a `.ignore` file containing `*.md`
- **THEN** no `.md` file is skipped because of it

#### Scenario: Preview before upload
- **WHEN** the owner selects a folder
- **THEN** the page shows how many files will be skipped, and the list can be expanded, before anything is sent

#### Scenario: Same result for zip
- **WHEN** the same folder, without a `.git` directory, is uploaded once as a folder and once as a zip
- **THEN** both shares contain the same file list

### Requirement: Path safety and limits
The server SHALL reject an upload in which any relative path is absolute or contains `..`, `\`, NUL or an empty segment. It SHALL also reject an upload whose total size after unzip exceeds `DROPGO_MAX_SHARE_BYTES` (default 100 MB) or that holds more than 10,000 files. A rejected upload SHALL create no share and leave no stored files. A dropped connection after the upload was received SHALL NOT stop the share from being created.

#### Scenario: Large but allowed upload
- **WHEN** the owner uploads a 50 MB folder with the default limit
- **THEN** the upload succeeds

#### Scenario: Zip slip
- **WHEN** a zip contains the entry `../../etc/passwd`
- **THEN** the upload is rejected and nothing is stored

#### Scenario: Zip bomb
- **WHEN** a 1 MB zip expands to more than `DROPGO_MAX_SHARE_BYTES`
- **THEN** the server stops unpacking, rejects the upload and stores nothing

### Requirement: Share path rules
A share path SHALL be 1 to 8 segments of lowercase `[a-z0-9-_]`, each 1 to 64 characters long, joined by `/`. Uppercase input SHALL be lowercased. A segment starting with `_` SHALL be rejected. An empty path SHALL get 8 random characters. A path SHALL be rejected if it equals another live share's path, is a prefix of one, or has one as a prefix. A live share is one that is active and not expired. The only exception is a confirmed overwrite of the owner's own share at the same path.

#### Scenario: Custom nested path
- **WHEN** the owner enters `Notes/2026/Q3`
- **THEN** the share is served at `/notes/2026/q3/`

#### Scenario: Nesting conflict
- **WHEN** an active share exists at `notes` and the owner enters `notes/2026`
- **THEN** the upload is rejected with a message that names the conflicting share

#### Scenario: Reserved segment
- **WHEN** the owner enters `_dropgo/x` or `docs/_x`
- **THEN** the upload is rejected

#### Scenario: Expired share does not block
- **WHEN** a share at `notes` has expired but the hourly job has not run, and the owner uploads to `notes`
- **THEN** the old share moves to the trash and the new upload succeeds

#### Scenario: Connection drops during the transfer
- **WHEN** the owner's upload to `notes` is cut off before the server has received it all, and the owner uploads to `notes` again
- **THEN** no share was created by the first attempt and the new upload succeeds

#### Scenario: Connection drops after the transfer
- **WHEN** the connection drops after the server has received the whole upload to `notes`
- **THEN** the share at `notes` is still created, and a new upload to `notes` gets the overwrite prompt

#### Scenario: Retry while the first upload is still being written
- **WHEN** the owner uploads to `notes` again without confirming an overwrite, and the first upload to `notes` becomes active before the second one claims the path
- **THEN** the second upload is rejected with the overwrite prompt and the first share is unchanged

#### Scenario: Server shutdown during writing
- **WHEN** the server gets SIGTERM while an upload task is writing files
- **THEN** the server finishes that upload before it exits

#### Scenario: Two uploads at once
- **WHEN** two uploads claim `a` and `a/b` at the same time
- **THEN** exactly one succeeds

### Requirement: Overwrite an existing share
Uploading to the path of one of the owner's active shares SHALL replace that share's files and settings after the owner confirms. The old content SHALL keep being served until every new file is stored. An empty password field SHALL keep the existing password.

#### Scenario: Confirmed overwrite
- **WHEN** the owner uploads to an existing path and confirms
- **THEN** the share serves the new files and the old files are deleted

#### Scenario: Overwrite fails partway
- **WHEN** storing a new file fails during an overwrite
- **THEN** the share still serves the old files unchanged

#### Scenario: Password kept
- **WHEN** the owner overwrites a password share and leaves the password field empty
- **THEN** the old password still unlocks the share and existing unlock cookies keep working

