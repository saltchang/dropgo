## ADDED Requirements

### Requirement: Markdown files are served as pages
A request for a `.md` file in a share SHALL return the rendered HTML page. A request for a directory with `index.md` and no `index.html` SHALL return the rendered `index.md`. Rendering SHALL support GFM tables, task lists, strikethrough, autolinks and footnotes, and SHALL allow raw HTML.

#### Scenario: Link between Markdown files
- **WHEN** `index.md` links to `guide.md` and the visitor follows the link
- **THEN** the rendered `guide.md` is shown

#### Scenario: Table
- **WHEN** a `.md` file holds a GFM table
- **THEN** the page has a `<table>` element

### Requirement: Code, diagrams and math
Fenced code blocks with a language SHALL be highlighted on the server. Fenced `mermaid` blocks SHALL be drawn as diagrams, and `$...$` and `$$...$$` SHALL be drawn as math, by vendored scripts and fonts served from `/_dropgo/assets/`.

#### Scenario: Mermaid block
- **WHEN** a `.md` file holds a fenced `mermaid` block and the page loads in a browser
- **THEN** an SVG diagram is shown in place of the block

#### Scenario: Math fonts load
- **WHEN** a rendered page with `$x^2$` loads in a browser
- **THEN** the KaTeX font requests succeed

#### Scenario: Highlighted code
- **WHEN** a `.md` file holds a fenced `rust` block
- **THEN** the HTML response already contains highlight markup before any script runs

### Requirement: Rendered page layout
Rendered pages SHALL use dropgo's stylesheet in the visitor's theme (see share-access), fit a 360 px wide screen without horizontal scrolling, and end with a footer that has download links. Uploaded `.html` files SHALL be served byte for byte unchanged.

#### Scenario: HTML is untouched
- **WHEN** an uploaded `index.html` is requested
- **THEN** the body equals the uploaded bytes

### Requirement: Downloads
`/_dropgo/download/<share-id>` SHALL offer `format=zip` for every share, `format=source` for a single-file share, and `format=html` for a share whose entry is Markdown. It SHALL apply the same access rules as the share.

#### Scenario: Zip of a folder share
- **WHEN** a visitor allowed to view a folder share requests `format=zip`
- **THEN** they get a zip of the original files with their relative paths

#### Scenario: Standalone HTML
- **WHEN** `format=html` is requested for a Markdown share
- **THEN** the response is one HTML file in the dark theme with the stylesheet inlined, no footer and no reference to the share host

#### Scenario: Download of an owner-only share
- **WHEN** a signed-out visitor requests a download of an owner-only share
- **THEN** they are sent to sign-in, as for the share itself

### Requirement: Re-render command
`dropgo rerender` SHALL rebuild the rendered pages of every share that has not been purged, from the stored sources.

#### Scenario: After a stylesheet change
- **WHEN** the operator runs `dropgo rerender` after upgrading and later restores a trashed share
- **THEN** both active and restored shares use the new template
