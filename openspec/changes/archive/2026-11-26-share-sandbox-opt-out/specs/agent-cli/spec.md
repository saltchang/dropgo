## MODIFIED Requirements

### Requirement: Upload from the CLI
`dropgo upload` SHALL accept a file, a directory, a `.zip` file, or `-` for stdin together with `--name`. A directory SHALL be filtered with the same ignore rules as the server before it is sent. Uploading to a path that holds one of the owner's shares SHALL fail with exit 5 and code `path.exists`, unless `--replace` is given. With `--replace`, that share SHALL be overwritten and SHALL keep any visibility, password, expiry or sandbox setting not given on the command line. A new share with no `--visibility` SHALL be `owner`. `--no-sandbox` SHALL turn the sandbox off; a new share without it SHALL have the sandbox on. `--password` with a resulting visibility other than `password` SHALL fail with exit 6 and code `bad_input` before anything is sent. `--expires` SHALL accept `never`, `1d`, `7d`, `30d` or an RFC 3339 time.

#### Scenario: Markdown from stdin
- **WHEN** `printf '# Hi' | dropgo upload - --name hi.md --path hi` runs
- **THEN** stdout holds the share object, whose URL ends in `/hi/`

#### Scenario: Folder with node_modules
- **WHEN** a directory holding `index.html` and a `node_modules/` with 20,000 files is uploaded
- **THEN** the upload succeeds and the share holds no file under `node_modules/`

#### Scenario: Password without visibility
- **WHEN** `dropgo upload notes.md --password x` runs for a new path
- **THEN** it exits 6 with code `bad_input` and no share is created

#### Scenario: Replace
- **WHEN** `dropgo upload ./site --path demo --replace` runs and `demo` holds one of the owner's shares
- **THEN** it exits 0, `demo` serves the new content, and its visibility, expiry and sandbox setting are unchanged

#### Scenario: Upload with the sandbox off
- **WHEN** `dropgo upload ./recorder --path recorder --no-sandbox` runs
- **THEN** stdout holds the share object with `sandbox` `false`

## ADDED Requirements

### Requirement: Sandbox setting from the CLI
`dropgo edit` SHALL accept `--no-sandbox` to turn a share's sandbox off and `--sandbox` to turn it back on, and SHALL leave the setting unchanged when neither is given. Giving both SHALL exit 2.

#### Scenario: Turn the sandbox back on
- **WHEN** `dropgo edit recorder --sandbox` runs for a share with the sandbox off
- **THEN** stdout holds the share object with `sandbox` `true`

#### Scenario: Edit without the flag
- **WHEN** `dropgo edit recorder --expires 7d` runs for a share with the sandbox off
- **THEN** the share still has the sandbox off

#### Scenario: Get shows the setting
- **WHEN** `dropgo get recorder` runs for a share with the sandbox off
- **THEN** stdout holds the share object with `sandbox` `false`
