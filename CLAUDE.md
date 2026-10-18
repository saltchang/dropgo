# dropgo

## Verifying a change

Run these in order before calling a change done, and before every commit and PR:

1. `cargo fmt` (CI fails on `cargo fmt --check`)
2. `cargo test` (needs Docker)
3. `cd web && npm test`
4. For changes to pages, CSS, cookies or headers: `cd qa && npm run qa` against the stack in `qa/compose.yml` (see README)
