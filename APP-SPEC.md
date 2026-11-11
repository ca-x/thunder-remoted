# thunder-remoted web app — implementation spec

Repo: github.com/ca-x/thunder-remoted (local dir /home/czyt/Projects/thunder-remoted)
Library it builds on: thunder-task-rs, module `local::LocalNas` (git dep, already wired in Cargo.toml).

## Goal

Turn thunder-remoted from a CLI into a self-hosted web app: a **NAS download manager**
that runs next to a Xunlei NAS client (`pan-xunlei-com`) and manages its downloads.

Architecture: one Rust binary serving both a REST API and a static frontend.

```
browser  ──HTTP──▶  thunder-remoted serve  ──HTTP(pan-auth)──▶  NAS client :2345
```

The NAS client hands its own web UI a session token; `LocalNas::connect()` fetches it.
No cloud login is required and none should be added.

## Backend (`src/serve.rs`, wired as `thunder-remoted serve`)

Use `axum` + `tower-http` (serve static dir, gzip). Bind `--addr` (default `0.0.0.0:3600`).
NAS base from `--base` / `THUNDER_NAS_URL`, default `http://127.0.0.1:2345`.

Endpoints (JSON; wrap errors as `{"error":{"code","message"}}` with sensible HTTP status):

- `GET  /api/health`      → `{ "ok": true, "nas": "<base>", "connected": bool }`
- `GET  /api/device`      → LocalNas::device_info()
- `GET  /api/quota`       → `{ "limit": i64, "usage": i64, "remaining": i64|null }`
- `GET  /api/tasks`       → task list (support `?limit=`, `?phase=` filter server-side)
- `POST /api/analyze`     → body `{ "url": "..." }` → resource list (file tree, sizes, infohash)
- `POST /api/tasks`       → body `{ "url", "name"?, "parent_folder_id"?, "sub_file_index"?: [u32] }`
                            → create; if quota is exhausted return HTTP 409 with a clear message
- `PATCH /api/tasks/{id}` → body `{ "action": "pause"|"resume" }` (space from the task)
- `DELETE /api/tasks`     → body `{ "ids": [...] }`

Rules:
- Reconnect the LocalNas client on token/connection failure (tokens rotate): on a 401/402/network
  error, call `LocalNas::connect` again once and retry.
- Never fabricate or cache tokens in the frontend; the backend owns the NAS session.
- Quota: before creating, read quota; if `remaining == Some(0)` refuse with 409 and the
  message "今日免费额度已用完（3/3）" plus how many are used. Do not attempt to bypass.
- Keep `LocalNas` in thunder-task-rs skill: this repo must not reimplement NAS calls.

## Frontend (`web/`, Vite + React + TypeScript)

Aim for a genuinely well-designed, calm interface — not a template. Use these skills as
design direction: `ui-ux-pro-max`, `emil-design-eng`, `apple-design`, `animate`,
`frontend-design`.

Screens / parts:
1. **Header**: device name, online state, disk usage bar (usage/limit from device info), quota chip
   (`今日 3/3` with a warning tone when 0 left).
2. **Task list** (main): rows with name, phase badge, progress bar, size, created time.
   - Filter chips: 全部 / 下载中 / 已完成 / 失败 / 暂停
   - Category quick-filter for multi-file tasks (视频/音频/图片/文档/压缩包/其他) derived from
     file extensions, because the API returns file names in the resource tree
   - Row actions: 暂停 / 继续 / 删除 (with confirm), optimistic UI + refetch
   - Auto-refresh every 3s while any task is running; stop polling when the tab is hidden
3. **Add task flow** (slide-over panel or dialog):
   - Paste link (magnet/http/ftp/ed2k) → 解析 → shows parsed name, size, file count, infohash
   - If the resource has multiple files: a file picker with tri-state checkboxes, per-file size,
     "全选/反选", and the category filter chips; selected indexes go to `sub_file_index`
   - Directory: list download paths from device info as presets (`/downloads/`, `/data/`); free-text
     folder id field collapsed under 高级
   - Submit → success toast with the new task id; quota exhaustion shows the explicit message
4. **Empty/error states**: NAS unreachable → clear message with the base URL and a retry button.

Quality bar:
- TypeScript strict, no `any` in app code
- Responsive: works at 360px width and up; task list becomes cards on mobile
- Light and dark via `prefers-color-scheme`; no hardcoded background colors on containers
  (use CSS custom properties defined for both schemes)
- Motion: 150–250ms ease-out for panel/row transitions; respect `prefers-reduced-motion`
- Accessible: focus-visible rings, aria-live for toasts, keyboard-operable dialogs
- `npm run build` must output to `web/dist` and the backend must serve it

## Packaging (LazyCat app)

Add so it can be installed on the user's LazyCat box:
- `lzc-manifest.yml`: one service `thunder-remoted` (image built from this repo's Dockerfile),
  `application.subdomain: thunderdl`, upstream to `http://thunder-remoted:3600/`,
  env `THUNDER_NAS_URL=http://thunder:2345`, `THUNDER_BIND=0.0.0.0:3600`
- `lzc-build.yml`, `package.yml` (id `cloud.lazycat.app.thunder-remoted`, version 0.1.0,
  locales zh/en, icon from `logo.png` — must be ≤200KB, so downscale a copy to `icon.png`
  at 512x512 first)
- Reference conventions from ca-x repos (`raindrop`, `localsendy`) for structure.

## Dockerfile

Extend the existing one to a multi-stage build: Node builder for `web/dist`, Rust builder for the
binary, slim runtime. Final image runs `thunder-remoted serve`.

## Deliverable / verification (required)

1. `cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo test` all pass.
2. `npm ci && npm run build` in `web/` passes and emits `web/dist`.
3. Start the server against a mock NAS: add a small test fixture or use `--base` with a tiny
   local HTTP stub that returns canned `device/info`, quota and task JSON, then
   `curl` each endpoint and confirm the JSON shapes. Report the commands and the outputs.
4. Commit in logical commits with the trailer `Assisted-by: Cindy (https://cindy.app)`; push.

## Do NOT

- Do not add cloud login, captcha solving, risk-control signing, or anything that evades the
  NAS client's free-tier quota. The quota is shown to the user, never worked around.
- Do not reimplement NAS API calls outside thunder-task-rs.
- Do not commit `web/node_modules`.
