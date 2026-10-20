---
name: dropgo
description: Share files, folders or Markdown notes as links on the owner's dropgo server, and list, edit, trash, restore or read existing shares. Use when the user asks to share, publish, upload or drop something with dropgo, or asks about their dropgo shares.
---

# dropgo

`dropgo` is a command-line client for the owner's dropgo server. Every command prints JSON on stdout when it succeeds. When it fails, it prints `{"error":{"code","message","details"}}` on stderr and exits with a non-zero code. Branch on `error.code`, not on the message.

## Setup

- Run `dropgo auth status` first. Exit 0 means it is ready.
- On exit 3, ask the user to run `dropgo auth login` in their own terminal. That command asks for the server URL and a token that the user creates on the server's tokens page.
- Never ask the user to paste a token into the conversation, and never put one on a command line.

## Commands

| Command | Does |
|---|---|
| `dropgo auth login [--url URL]` | Save the server URL and a token. Run by the user. |
| `dropgo auth status` | Show the server and the token name and expiry. |
| `dropgo check PATH` | Check whether a share path is free. |
| `dropgo upload SOURCE [--path P] [--visibility public\|password\|owner] [--password PW] [--expires never\|1d\|7d\|30d\|RFC3339] [--replace] [--name FILE]` | Upload a file, folder, `.zip`, or `-` for stdin (needs `--name`). The default visibility is `owner`. `--replace` overwrites the share at `--path` and keeps the settings you leave out. |
| `dropgo list [--trash]` | List active shares, or the trash. |
| `dropgo get REF` | Show one share. |
| `dropgo edit REF [--path] [--visibility] [--password] [--expires]` | Change only the given settings. |
| `dropgo trash REF` | Move a share to the trash. It is deleted for good after 30 days. |
| `dropgo restore REF [--path P]` | Restore a trashed share, at its old path or at `--path`. |
| `dropgo cat REF [FILE] [--rendered]` | List a share's files, or print one file. `--rendered` prints a Markdown file as HTML. |

`REF` is a share id or a share path. The share object holds `url`, the link to give the user.

## Exit codes

| Code | Meaning | What to do |
|---|---|---|
| 0 | Success | |
| 2 | Bad command-line arguments | Fix the command; see `dropgo <command> --help`. |
| 3 | Not configured, or the token is missing, invalid or expired | Run `dropgo auth status`, then ask the user to run `dropgo auth login`. |
| 4 | Not found | Check the ref with `dropgo list` or `dropgo list --trash`. |
| 5 | Conflict, such as `path.exists` or `ref.ambiguous` | `path.exists`: pick another path or use `--replace`. `ref.ambiguous`: retry with an id from `details.candidates`. |
| 6 | The input was rejected | Read `error.code` and `error.message`. |
| 7 | Network or server error | Retry once, then tell the user. |

## Notes

- A share is public only when you pass `--visibility public`. Ask the user before you make something public.
- Folder uploads skip `.git`, `node_modules` and the files that `.gitignore` or `.dockerignore` exclude.
