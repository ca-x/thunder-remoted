# thunder-remoted

Remote download controller for Xunlei (迅雷) NAS devices, built on
[thunder-task-rs](https://github.com/lib-x/thunder-task-rs).

## Self-hosted Web App

The web manager connects to the local `pan-xunlei-com` NAS client. It does not
require cloud login, import tokens, or bypass the client's daily quota. The
existing cloud CLI commands below remain available separately.

```sh
cd web && npm ci && npm run build && cd ..
cargo build --release --locked
./target/release/thunder-remoted serve --base http://127.0.0.1:2345
```

Open `http://localhost:3600`. The binary serves `web/dist` relative to its working
directory; use `--web-dir /absolute/path/to/dist` to relocate it. `--addr` overrides
`THUNDER_BIND` (default `0.0.0.0:3600`), and `--base` overrides `THUNDER_NAS_URL`
(default `http://127.0.0.1:2345`). For frontend development, `cd web && npm run dev`
proxies `/api` to the backend on port 3600.

The UI includes device storage and daily quota, task status/type filters,
pause/resume/delete, and a link-analysis dialog with nested file selection and
download-directory presets. It polls every three seconds while tasks are active,
stops polling in hidden tabs, and supports system dark mode and reduced motion.
File selections use the analyzed resource's `file_id` through LocalNas's
file-backed task builder; clients that do not return a resource ID receive an
explicit error rather than silently downloading every file.

### Docker

```sh
docker build -t thunder-remoted:0.1.0 .
docker run --rm -p 3600:3600 \
  -e THUNDER_NAS_URL=http://nas-host:2345 thunder-remoted:0.1.0
```

Replace `nas-host` with a hostname reachable **inside** the container. Container
loopback is not the NAS host. The multi-stage image builds the frontend and
locked Rust binary, then runs the service as an unprivileged user.

### LazyCat

`package.yml`, `lzc-manifest.yml`, and `lzc-build.yml` target LPK v2 (lzcos 1.5.0+).
The package is `cloud.lazycat.app.thunder-remoted` version `0.1.0`, with Chinese
and English metadata. The build uses `embed:thunder-remoted` from the Dockerfile;
`icon.png` is a 512×512 copy of `logo.png` under 200 KB.

Run `lzc-cli project release` with an authenticated LazyCat CLI. The single
service routes `thunderdl` to port 3600 and defaults to `http://thunder:2345` for
the **already installed** NAS client. Ensure `thunder` resolves from this service;
if the NAS client lives in another application/network, change `THUNDER_NAS_URL`
in the manifest to its reachable internal address before packaging. This package
does not install a NAS client or mount download disks; the NAS owns the files.

### API And Safety

REST routes: `GET /api/health`, `/api/device`, `/api/quota`, `/api/tasks`;
`POST /api/analyze`, `/api/tasks`; `PATCH /api/tasks/{id}` with
`{"action":"pause"}` or `{"action":"resume"}`; and `DELETE /api/tasks` with
`{"ids":["task-id"]}`. Task lists accept `limit` and exact NAS `phase` strings
such as `PHASE_TYPE_RUNNING`. All API errors use
`{"error":{"code":"…","message":"…"}}`.

The backend owns the injected local NAS session, reconnects once on 401/402 or
transport failure, and bounds NAS operations to 20 seconds. Creation is serialized
with a mandatory quota check; exhausted quota returns 409, and unreadable quota
fails closed. A network failure after the NAS has accepted a mutation can leave
the outcome ambiguous; the mandated single retry cannot guarantee exactly-once
delivery because LocalNas has no idempotency-key support.

There is intentionally no application-level login. Keep the port on a trusted
LAN or behind an authenticated reverse proxy. LazyCat ingress authentication
is retained (no `public_path` exemption). Do not expose port 3600 publicly.

### Verification (Mock Only)

```sh
cargo build
cargo clippy --all-targets -- -D warnings
cargo test
(cd web && npm ci && npm run build && npm test)
python3 tests/smoke.py
```

The smoke suite starts a loopback-only NAS fixture and the real server, uses
`curl` on every endpoint, and checks selected-file serialization, task space,
quota exhaustion/unlimited quota, rotating tokens, 401/402/network reconnects,
bounded retries, JSON errors, static HTML and gzip. No real NAS is contacted.
For manual exploration, run `python3 tests/mock_nas.py --port 42345`, then
`cargo run -- serve --addr 127.0.0.1:3600 --base http://127.0.0.1:42345`.

With `chromium` and `chromedriver` installed, `python3 tests/browser.py` additionally
checks real UI actions, nested checkbox selection, modal keyboard focus/Escape,
360px dark mode, reduced motion, and hidden-tab polling. It saves screenshots to
a temporary directory. The browser runs without its sandbox only for this local
test fixture; do not point the suite at untrusted pages.

## Existing Cloud CLI

```
thunder-remoted login        # scan a QR code in the Xunlei app to sign in
thunder-remoted devices      # list devices authorized for your account
thunder-remoted analyze "magnet:?xt=..."     # preview a link
thunder-remoted add "magnet:?xt=..." -d nas   # submit to a device
thunder-remoted list [--all]
thunder-remoted task pause|resume|delete <ids...>
```

Credentials come from `--client-id` / `--client-secret` or the
`THUNDER_CLIENT_ID` / `THUNDER_CLIENT_SECRET` environment variables. The
login session is stored in the OS config dir (`~/.config/thunder-remoted/`
on Linux) and refreshed automatically.

This tool talks to the same cloud API the official clients use, with your
own account and the official OAuth device flow. Server-side account limits
apply unchanged; nothing here bypasses them.

## Build

```
cargo build --release
```

Docker:

```
docker build -t thunder-remoted .
docker run --rm -it -v $PWD:/data thunder-remoted devices
```

## License

MIT
