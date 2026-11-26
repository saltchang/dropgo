## MODIFIED Requirements

### Requirement: Agent skill file
The repo SHALL contain `skills/dropgo/SKILL.md` with YAML frontmatter holding `name: dropgo` and a `description`. The skill SHALL name no single agent product. Its body SHALL list the client commands and the exit codes, tell the agent to run `dropgo auth status` when a command exits 3, and tell the agent never to ask the user to paste a token into the conversation. It SHALL also tell the agent to ask the user before it turns a share's sandbox off, and to turn it off only for a page that loads no scripts from other sites.

#### Scenario: Frontmatter
- **WHEN** the skill file is parsed
- **THEN** its frontmatter has `name` equal to `dropgo` and a non-empty `description`

#### Scenario: Commands documented
- **WHEN** the skill body is compared with `dropgo --help`
- **THEN** every client command in the help appears in the skill

#### Scenario: Sandbox rule
- **WHEN** the skill body is read
- **THEN** it tells the agent to ask the user before passing `--no-sandbox`
