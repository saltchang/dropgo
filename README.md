# dropgo

[正體中文](README.zh-TW.md)

Drop an HTML file, a Markdown note, a folder or a zip in your browser and get a share link. You sign in through your own OIDC provider. The people you share with need no account.

- Admin host (`drop.example.com`): upload, list, edit, trash.
- Share host (`s.example.com`): serves each share under a path you pick, such as `/notes/2026/q3/`.
- Each share is public, behind a password, or visible only to you.
- Markdown is rendered with code highlighting, Mermaid and KaTeX. Visitors can download the source, a zip or the rendered HTML.
- Shares can expire. Expired and deleted shares go to the trash for 30 days.
- `.gitignore`, `.dockerignore`, dotfiles and `node_modules` are skipped on upload.
- UI in English and Traditional Chinese, dark theme by default.

## Quick start

You need Docker and an OIDC provider (Authelia, Keycloak, Authentik, Google and so on).

1. In your provider, create a confidential client with these two redirect URIs, in lower case:
   - `https://drop.example.com/auth/callback`
   - `https://s.example.com/_dropgo/auth/callback`

   dropgo reads the owner claim from the ID token, and asks the userinfo endpoint when the ID token lacks it, as Authelia 4.39 and later does by default. The userinfo response must be plain JSON, not a signed JWT.
2. Save this as `compose.yml` and fill in the values:

   ```yaml
   services:
     db:
       image: postgres:17-alpine
       environment:
         POSTGRES_PASSWORD: change-me
       volumes: [db:/var/lib/postgresql/data]
     dropgo:
       image: ghcr.io/saltchang/dropgo:latest
       depends_on: [db]
       ports: ["8080:8080"]
       volumes: [files:/data]
       environment:
         DROPGO_ADMIN_ORIGIN: https://drop.example.com
         DROPGO_SHARE_ORIGIN: https://s.example.com
         DROPGO_OIDC_ISSUER: https://auth.example.com
         DROPGO_OIDC_CLIENT_ID: dropgo
         DROPGO_OIDC_CLIENT_SECRET: change-me
         DROPGO_OWNERS: your-username
         DROPGO_SECRET: output-of-openssl-rand-hex-32
         DROPGO_DATABASE_URL: postgres://postgres:change-me@db/postgres
         DROPGO_STORE_URL: file:///data
   volumes:
     db:
     files:
   ```

3. Run `docker compose up -d`.
4. Point both hostnames at port 8080 through an HTTPS reverse proxy, then open `https://drop.example.com`.

Both hosts need HTTPS. The session cookies use the `__Host-` prefix, which browsers only accept over HTTPS.

## Configuration

Every option is an env var, and also a flag (`dropgo --help`).

| Env var | Default | What it does |
|---|---|---|
| `DROPGO_ADMIN_ORIGIN` | required | Admin origin, such as `https://drop.example.com` |
| `DROPGO_SHARE_ORIGIN` | required | Share origin, such as `https://s.example.com`. |
| `DROPGO_OIDC_ISSUER` | required | Issuer URL of your provider |
| `DROPGO_OIDC_CLIENT_ID` | required | Client ID |
| `DROPGO_OIDC_CLIENT_SECRET` | required | Client secret |
| `DROPGO_OWNERS` | empty | Comma-separated values of the owner claim that may sign in |
| `DROPGO_OWNER_CLAIM` | `preferred_username` | Claim checked against `DROPGO_OWNERS` |
| `DROPGO_SECRET` | required | Key for unlock cookies and access tokens, at least 32 bytes |
| `DROPGO_DATABASE_URL` | required | Postgres URL. Migrations run on start |
| `DROPGO_STORE_URL` | required | `s3://bucket/prefix`, `file:///path` or `memory://` |
| `DROPGO_MAX_SHARE_BYTES` | `104857600` (100 MB) | Largest share, counted after unzip. Each share also has a 10,000 file limit |
| `DROPGO_TMP_DIR` | `TMPDIR` | Where uploads wait before they are stored |
| `DROPGO_TRUSTED_PROXIES` | empty | Comma-separated proxy IPs or CIDRs. Used to find the client IP for the password attempt limit |
| `DROPGO_LISTEN` | `0.0.0.0:8080` | Listen address |

For S3, set the usual `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`, and `AWS_ENDPOINT` for R2, MinIO and the like.

`/_dropgo/healthz` answers 200 on any host, for probes.

## Deployment notes

- **Body size.** Set your reverse proxy's request body limit to at least `DROPGO_MAX_SHARE_BYTES` plus 16 MB. Traefik and nginx both cut large uploads off by default.
- **Temp dir.** Uploads are written to `DROPGO_TMP_DIR` before they are stored. It needs room for every upload that can run at once. On Kubernetes, mount an `emptyDir` there, because the image's root filesystem should stay read-only.
- **Shutdown.** On SIGTERM dropgo stops taking requests and finishes the uploads it already has. Give it a termination grace period long enough for your biggest upload, for example 120 seconds.
- **One replica.** The password attempt limit lives in memory. Run more replicas only if you accept a per-replica limit.
- **Theme or highlight changes.** Markdown is rendered at upload. After an upgrade that changes the page template, run `dropgo rerender` once.

## Development

```sh
cargo fmt           # required before every commit and PR; CI runs cargo fmt --check
cargo test          # needs Docker: tests start Postgres and a mock OIDC provider
cd web && npm test  # ignore-rule fixtures, shared with the Rust tests
cd web && npm run vendor  # rebuild static/vendor after a JS dependency bump
```

### Browser QA

`qa/` holds a local stack (Caddy for HTTPS, Postgres, a mock OIDC provider, dropgo built from this repo) and a Playwright script that walks through every page. Run it after changes to pages, CSS, cookies or headers. It needs ports 8080, 8081, 8443, 18080 and 55432, and is not part of CI.

```sh
cd qa
docker compose up -d --build
npm ci && npx playwright install chromium   # once
npm run qa      # about 2 minutes; screenshots land in qa/shots/
docker compose down -v
```

To try it by hand, open https://drop.localhost:8443 and accept the certificate warning, then do the same once for https://s.localhost:8443. The mock sign-in form (`qa/login.html`) comes filled in as the owner `alice`, so press Sign in. Change the subject to get a second, empty account.

## License

MIT
