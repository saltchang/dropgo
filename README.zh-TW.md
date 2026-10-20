# dropgo

[English](README.md)

在瀏覽器裡丟進一個 HTML 檔、Markdown 筆記、資料夾或 zip，就拿到一條分享連結。你用自己的 OIDC provider 登入，收到連結的人不需要帳號。

- 管理端（`drop.example.com`）：上傳、列表、編輯、垃圾桶。
- 分享端（`s.example.com`）：每個分享放在你指定的路徑下，例如 `/notes/2026/q3/`。
- 每個分享可以公開、設密碼，或只有你自己看得到。
- Markdown 會轉成網頁，支援程式碼上色、Mermaid 和 KaTeX。訪客可以下載原始檔、zip 或轉好的 HTML。
- 分享可以設到期時間。過期和刪除的分享會在垃圾桶留 30 天。
- 上傳時會略過 `.gitignore`、`.dockerignore` 列出的檔案、dotfile 和 `node_modules`。
- 介面有英文和正體中文，預設深色主題。

## 快速開始

你需要 Docker 和一個 OIDC provider（Authelia、Keycloak、Authentik、Google 都可以）。

1. 在 provider 建一個 confidential client，填這兩個 redirect URI（全部小寫）：
   - `https://drop.example.com/auth/callback`
   - `https://s.example.com/_dropgo/auth/callback`

   dropgo 先從 ID token 讀 owner claim，ID token 裡沒有時再去問 userinfo endpoint。Authelia 4.39 之後的預設就是這樣。userinfo 的回應必須是一般 JSON，不能是簽章過的 JWT。
2. 把下面存成 `compose.yml`，填入你的值：

   ```yaml
   services:
     db:
       image: postgres:17-alpine
       environment:
         POSTGRES_PASSWORD: change-me
       volumes: [db:/var/lib/postgresql/data]
     dropgo:
       image: ghcr.io/saltchang/dropgo:0.1.0
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

3. 執行 `docker compose up -d`。
4. 用 HTTPS reverse proxy 把兩個 hostname 都導到 8080 port，然後打開 `https://drop.example.com`。

兩個 host 都必須是 HTTPS。session cookie 用 `__Host-` 前綴，瀏覽器只在 HTTPS 下接受。

## 設定

每個選項都是環境變數，也可以用 flag，寫在 `serve` 或 `rerender` 之後（`dropgo serve --help`）。flag 寫在子命令前面（例如 `dropgo --database-url X rerender`）已不再支援。

| 環境變數 | 預設值 | 用途 |
|---|---|---|
| `DROPGO_ADMIN_ORIGIN` | 必填 | 管理端 origin，例如 `https://drop.example.com` |
| `DROPGO_SHARE_ORIGIN` | 必填 | 分享端 origin，例如 `https://s.example.com` |
| `DROPGO_OIDC_ISSUER` | 必填 | provider 的 issuer URL |
| `DROPGO_OIDC_CLIENT_ID` | 必填 | Client ID |
| `DROPGO_OIDC_CLIENT_SECRET` | 必填 | Client secret |
| `DROPGO_OWNERS` | 空 | 可以登入的 owner claim 值，用逗號分隔 |
| `DROPGO_OWNER_CLAIM` | `preferred_username` | 拿來比對 `DROPGO_OWNERS` 的 claim |
| `DROPGO_SECRET` | 必填 | 解鎖 cookie 和存取 token 用的金鑰，至少 32 bytes |
| `DROPGO_DATABASE_URL` | 必填 | Postgres URL，啟動時自動跑 migration |
| `DROPGO_STORE_URL` | 必填 | `s3://bucket/prefix`、`file:///path` 或 `memory://` |
| `DROPGO_MAX_SHARE_BYTES` | `104857600`（100 MB） | 單一分享的上限，以解壓縮後計算。每個分享另有 10,000 個檔案的上限 |
| `DROPGO_TMP_DIR` | `TMPDIR` | 上傳檔案存進儲存空間前暫放的地方 |
| `DROPGO_TRUSTED_PROXIES` | 空 | proxy 的 IP 或 CIDR，用逗號分隔。用來判斷密碼嘗試次數限制要看哪個 client IP |
| `DROPGO_LISTEN` | `0.0.0.0:8080` | 監聽位址 |

用 S3 的話，設定常見的 `AWS_ACCESS_KEY_ID`、`AWS_SECRET_ACCESS_KEY`、`AWS_REGION`；R2、MinIO 這類服務再加上 `AWS_ENDPOINT`。

`/_dropgo/healthz` 在任何 host 都回 200，給健康檢查用。

## 給 coding agent 使用

`dropgo` 執行檔同時也是 client。它透過 `/api/v1` JSON API 上傳、列出、編輯、丟進垃圾桶、還原與讀取分享，輸出 JSON，並用 exit code 說明錯在哪裡。`skills/dropgo/SKILL.md` 教 coding agent 怎麼用它。

1. 登入管理站，打開 **Token**，建立一組 token。
2. 把下面的提示貼給你的 coding agent（Claude Code、Codex、opencode、Copilot 或其他）：

   ```text
   Install the dropgo CLI and its agent skill for me.
   1. Download dropgo-linux-amd64 and SKILL.md from the latest release of https://github.com/saltchang/dropgo/releases.
   2. Install the binary on my PATH as `dropgo` and make it executable. On macOS or Linux on another CPU, run `cargo install --git https://github.com/saltchang/dropgo --tag <that release tag>` instead.
   3. Install SKILL.md as a skill named `dropgo` where you load user-level skills. If you have no skill support, append it to your global instructions file.
   4. Run `dropgo --version` and show me the output.
   5. Ask me to run `dropgo auth login` in my own terminal. Do not ask me for the token in this chat.
   ```

3. agent 請你執行時，在自己的終端機跑 `dropgo auth login`。它會詢問管理站網址和 token，然後以 0600 權限存到 `~/.config/dropgo/config.toml`。`DROPGO_URL` 與 `DROPGO_TOKEN` 會覆蓋設定檔。

Token 的效期可選 7、30、90 或 365 天，可在 tokens 頁撤銷。

## 部署注意事項

- **請求大小。** reverse proxy 的 request body 上限至少要設成 `DROPGO_MAX_SHARE_BYTES` 加 16 MB。Traefik 和 nginx 預設都會擋掉大檔上傳。
- **暫存目錄。** 上傳的檔案會先寫進 `DROPGO_TMP_DIR`，空間要夠所有同時進行的上傳使用。在 Kubernetes 上請掛一個 `emptyDir`，因為 image 的根目錄應該維持唯讀。
- **關機。** 收到 SIGTERM 後，dropgo 不再接新請求，但會把手上的上傳做完。termination grace period 要夠你最大的上傳跑完，例如 120 秒。
- **只跑一個 replica。** 密碼嘗試次數記在記憶體裡。要跑多個 replica，就得接受每個 replica 各自計算。
- **主題或上色規則有變動時。** Markdown 是在上傳時轉成網頁的。升級後如果頁面模板有改，執行一次 `dropgo rerender`。

## 開發

```sh
cargo fmt           # 每次 commit 和開 PR 前都要跑，CI 會跑 cargo fmt --check
cargo test          # 需要 Docker：測試會啟動 Postgres 和 mock OIDC provider
cd web && npm test  # 忽略規則的 fixture，和 Rust 測試共用
cd web && npm run vendor  # JS 套件升級後重建 static/vendor
```

### 瀏覽器 QA

`qa/` 裡有一套本機環境（Caddy 提供 HTTPS、Postgres、mock OIDC provider，以及用這個 repo 建出來的 dropgo），還有一支 Playwright 腳本，會把每一頁都走過一次。改到頁面、CSS、cookie 或 header 之後跑一次。它會用到 8080、8081、8443、18080、55432 這幾個 port，不在 CI 裡跑。

```sh
cd qa
docker compose up -d --build
npm ci && npx playwright install chromium   # 第一次才需要
npm run qa      # 大約 2 分鐘，截圖會存到 qa/shots/
docker compose down -v
```

想自己操作的話，打開 https://drop.localhost:8443 並接受憑證警告，https://s.localhost:8443 也要接受一次。mock 登入表單（`qa/login.html`）已經預先填好 owner `alice`，直接按 Sign in 就好。把 subject 改掉，就會得到另一個空的帳號。

## 授權

MIT
