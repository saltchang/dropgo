## ADDED Requirements

### Requirement: Agent skill file
The repo SHALL contain `skills/dropgo/SKILL.md` with YAML frontmatter holding `name: dropgo` and a `description`. The skill SHALL name no single agent product. Its body SHALL list the client commands and the exit codes, tell the agent to run `dropgo auth status` when a command exits 3, and tell the agent never to ask the user to paste a token into the conversation.

#### Scenario: Frontmatter
- **WHEN** the skill file is parsed
- **THEN** its frontmatter has `name` equal to `dropgo` and a non-empty `description`

#### Scenario: Commands documented
- **WHEN** the skill body is compared with `dropgo --help`
- **THEN** every client command in the help appears in the skill

### Requirement: Release assets
Each tagged release SHALL attach `dropgo-linux-amd64`, the same binary as in that release's image, and `SKILL.md`.

#### Scenario: Binary runs
- **WHEN** `dropgo-linux-amd64` is downloaded from a release and run with `--version`
- **THEN** it prints the release's version

### Requirement: Install prompt in the README
`README.md` and `README.zh-TW.md` SHALL contain a prompt the owner can paste into any coding agent. The prompt SHALL tell the agent to:
- download the binary and the skill from the latest release
- install the binary on `PATH`, or use `cargo install` on platforms other than linux-amd64
- install the skill where that agent loads user-level skills, or append it to the agent's global instructions file when the agent has no skill support
- run `dropgo --version`
- ask the owner to run `dropgo auth login` in their own terminal

#### Scenario: Prompt has no token
- **WHEN** the prompt text is read
- **THEN** it contains no token and tells the agent not to ask for one in the chat
