# agent-cli Specification

## Purpose
The `dropgo` client commands, their config, JSON output and exit codes.
## Requirements
### Requirement: Client commands need no server config
The `dropgo` binary SHALL provide the client commands `auth login`, `auth status`, `check`, `upload`, `list`, `get`, `edit`, `trash`, `restore` and `cat`. They SHALL run with none of the server's env vars set. Running `dropgo` with no subcommand SHALL still start the server.

#### Scenario: Client without server env
- **WHEN** `dropgo list` runs in a shell with only `DROPGO_URL` and `DROPGO_TOKEN` set
- **THEN** it prints the share list and does not complain about missing server settings

#### Scenario: Bare binary serves
- **WHEN** the Docker image starts with no arguments
- **THEN** the server listens as before

### Requirement: Config resolution
The client SHALL take the server URL and token from `DROPGO_URL` and `DROPGO_TOKEN` when they are set, and otherwise from `config.toml` in `$XDG_CONFIG_HOME/dropgo/`, or in `~/.config/dropgo/` when `XDG_CONFIG_HOME` is unset. When neither gives both values, a command SHALL fail with exit code 3 and code `config.missing`. A URL that uses `http://` for a host other than `localhost`, `127.0.0.1` or `::1` SHALL be refused with exit code 3 and code `config.insecure_url`.

#### Scenario: Env overrides the file
- **WHEN** the config file holds token A and `DROPGO_TOKEN` holds token B
- **THEN** requests use token B

#### Scenario: Plain HTTP to a remote host
- **WHEN** `DROPGO_URL` is `http://drop.example.com`
- **THEN** the command exits 3 with code `config.insecure_url` and sends no request

#### Scenario: Nothing configured
- **WHEN** no env var and no config file exist
- **THEN** `dropgo list` exits 3 and stderr holds code `config.missing`

### Requirement: Interactive login
On a terminal, `dropgo auth login` SHALL ask for the server URL unless `--url` is given, then read the token without echoing it. Off a terminal it SHALL take `--url` and read the token from stdin. In both cases it SHALL verify the token with the server and write the config file with mode 0600 only after the server accepts it.

#### Scenario: Wrong token is not saved
- **WHEN** the owner pastes a revoked token
- **THEN** the command exits 3 and the config file is unchanged

#### Scenario: Scripted login
- **WHEN** `echo "$T" | dropgo auth login --url https://drop.example.com` runs with a valid token
- **THEN** the config file holds the URL and token and has mode 0600

### Requirement: Output and exit codes
Each command SHALL print one JSON value to stdout on success. The exception is `cat` with a file argument, which SHALL write the file's bytes. On failure a command SHALL print `{"error": {"code", "message", "details"}}` to stderr. Server errors SHALL pass through unchanged. Failures found by the client SHALL use the codes `config.missing`, `config.insecure_url`, `network`, `bad_input` and `ref.ambiguous`. The command SHALL then exit with the code for that kind of failure: 3 auth or config, 4 not found, 5 conflict, 6 input rejected, 7 network or server error. Bad arguments SHALL exit 2.

#### Scenario: Not found
- **WHEN** `dropgo get no-such-path` runs
- **THEN** it exits 4 and stderr holds code `share.not_found`

#### Scenario: Server unreachable
- **WHEN** `DROPGO_URL` points at a port where nothing listens
- **THEN** the command exits 7 and stderr holds code `network`

### Requirement: Share references
Commands that take a share SHALL accept its id or its path. An argument in UUID form SHALL always be treated as an id. A path SHALL be normalized as the Web UI does. It SHALL resolve among active shares, and for `restore` among trashed shares. When a path matches more than one trashed share, the command SHALL fail with exit 5 and code `ref.ambiguous`, with the matching ids and trashed times in `details.candidates`.

#### Scenario: Two trashed shares at one path
- **WHEN** two trashed shares were both at `notes` and `dropgo restore notes` runs
- **THEN** it exits 5 with code `ref.ambiguous` and lists both ids

#### Scenario: Trash by path
- **WHEN** `dropgo trash notes/2026` runs and an active share has that path
- **THEN** that share moves to the trash

### Requirement: Upload from the CLI
`dropgo upload` SHALL accept a file, a directory, a `.zip` file, or `-` for stdin together with `--name`. A directory SHALL be filtered with the same ignore rules as the server before it is sent. Uploading to a path that holds one of the owner's shares SHALL fail with exit 5 and code `path.exists`, unless `--replace` is given. With `--replace`, that share SHALL be overwritten and SHALL keep any visibility, password or expiry not given on the command line. A new share with no `--visibility` SHALL be `owner`. `--password` with a resulting visibility other than `password` SHALL fail with exit 6 and code `bad_input` before anything is sent. `--expires` SHALL accept `never`, `1d`, `7d`, `30d` or an RFC 3339 time.

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
- **THEN** it exits 0, `demo` serves the new content, and its visibility and expiry are unchanged

### Requirement: Read files from the CLI
`dropgo cat <ref>` SHALL print the share's file list as JSON. `dropgo cat <ref> <file>` SHALL write that file's bytes, and `--rendered` SHALL write the rendered HTML of a Markdown file.

#### Scenario: Read an owner-only page
- **WHEN** `dropgo cat private-notes index.md --rendered` runs for an owner-only share
- **THEN** stdout holds the rendered HTML

