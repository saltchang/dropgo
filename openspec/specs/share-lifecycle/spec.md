# share-lifecycle Specification

## Purpose
TBD - created by archiving change add-dropgo-mvp. Update Purpose after archive.
## Requirements
### Requirement: Expiry
Each share SHALL have an optional expiry time. The default is none, which means the share never expires. The owner SHALL be able to set or change it at upload time and later. A custom expiry SHALL be read in the owner's browser time zone.

#### Scenario: Preset expiry
- **WHEN** the owner picks "7 days" at upload
- **THEN** `expires_at` is 7 days after the upload time

#### Scenario: Custom time in the owner's zone
- **WHEN** an owner whose browser is in UTC+8 picks 18:00 on a date
- **THEN** `expires_at` is 10:00 UTC on that date

#### Scenario: Extend after creation
- **WHEN** the owner changes an expiry to "never" before it passes
- **THEN** the share stays available

#### Scenario: Extend while the job runs
- **WHEN** the job has selected an expired share and the owner sets its expiry to "never" before the job writes
- **THEN** the share stays active

### Requirement: Expired shares move to the trash
A job SHALL run at least hourly and move every expired active share to the trash. The trashed share SHALL keep its path so that its URL returns 410.

#### Scenario: Job run
- **WHEN** the job runs and a share's `expires_at` has passed
- **THEN** the share's state becomes `trashed` and `trashed_at` is set

#### Scenario: Job keeps running
- **WHEN** the job has run once and a share expires afterwards
- **THEN** a later run trashes it

### Requirement: Manual delete moves to the trash
Deleting a share from the list SHALL move it to the trash. It SHALL NOT remove its files.

#### Scenario: Delete from list
- **WHEN** the owner deletes an active share
- **THEN** the share's URL returns 410 and the share appears in the trash tab

### Requirement: Trashed paths can be reused
A trashed share SHALL NOT block a new upload from taking its path or a path that overlaps it.

#### Scenario: Reuse path
- **WHEN** a share at `notes` is in the trash and the owner uploads a new share at `notes`
- **THEN** the new share is created and served at `/notes/`

### Requirement: Restore
The owner SHALL be able to restore a trashed share. Restore SHALL follow the same path rules as an upload. When the share's path now conflicts, restore SHALL fail with a message and ask the owner for a new path. Restore SHALL clear an expiry that has already passed.

#### Scenario: Restore with a free path
- **WHEN** the owner restores a share whose path is free
- **THEN** the share is active at the same path with the same visibility and password

#### Scenario: Restore with a taken path
- **WHEN** the owner restores a share whose path is now used by another share
- **THEN** the share stays in the trash and the UI asks for a new path

#### Scenario: Restore during purge
- **WHEN** the owner restores a share that the job has started to purge
- **THEN** the restore is refused and the share is purged

#### Scenario: Restore races an upload
- **WHEN** the owner restores a share at `a` while an upload claims `a/b`
- **THEN** exactly one succeeds

### Requirement: Purge after 30 days
The job SHALL permanently delete a trashed share and all of its stored files 30 days after `trashed_at`. The owner SHALL also be able to purge a trashed share right away, after confirming. A failed file delete SHALL leave the row in place so the next run retries it, and SHALL NOT stop the job from handling other shares.

#### Scenario: Automatic purge
- **WHEN** the job runs and a share has been in the trash for more than 30 days
- **THEN** its files and its row are gone and its URL returns 404

#### Scenario: Storage error during purge
- **WHEN** deleting a share's files fails
- **THEN** the share's URL still returns 410, the restore is refused, the job still trashes and purges the other due shares, and the next run tries again

