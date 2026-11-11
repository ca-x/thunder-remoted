# NAS Web App Verification — 2026-11-11

All required verification commands pass. The only NAS used was the loopback
fixture in `tests/mock_nas.py`; no real device or cloud account was contacted.

## Commands And Results

| Command | Observed result |
| --- | --- |
| `cargo build` | Exit 0; dev binary built successfully. |
| `cargo clippy --all-targets -- -D warnings` | Exit 0; no warnings. |
| `cargo test` | Exit 0; 3 passed, 0 failed. |
| `cd web && npm ci && npm run build` | Exit 0; strict TypeScript and Vite production build emitted `web/dist`. |
| `cd web && npm test` | Exit 0; 1 test file, 3 tests passed. |
| `python3 tests/smoke.py` | Exit 0; 7 live curl smoke tests passed. |
| `python3 tests/browser.py` | Exit 0; 3 real Chromium UI smoke tests passed. |
| `cargo fmt --all -- --check` | Exit 0. |
| `git diff --check` | Exit 0. |
| `lzc-cli project lint` | Exit 0; “No manifest lint warnings found.” |

The frontend build emitted `index.html` (0.46 kB), CSS (9.87 kB), and JavaScript
(237.65 kB; 75.06 kB gzipped). Neither `web/dist` nor `web/node_modules` is tracked.
The lockfile is tracked for reproducible `npm ci` builds.

The full endpoint transcript, including exact JSON and HTTP status outputs from
curl, is recorded in [verification-smoke.txt](verification-smoke.txt).

## Endpoint Coverage

| Request | Verified response |
| --- | --- |
| `GET /api/health` | 200; `ok`, loopback `nas` URL, `connected: true`; NAS failure yields `connected: false`. |
| `GET /api/device` | 200; device name, disk usage/limit, two directory presets. |
| `GET /api/quota` | 200; `{ "limit": 3, "usage": 0, "remaining": 3 }`; unlimited returns `remaining: null`. |
| `GET /api/tasks?limit=1` | 200; one task. Phase filtering is applied before the limit. |
| `POST /api/analyze` | 200; resource ID, name, sizes, nested file tree, infohash. |
| `POST /api/tasks` | 200; new task ID. Both URL and selected-file paths tested; selected indexes `0,2`, resource ID, file count and folder ID reach LocalNas. |
| `PATCH /api/tasks/task-1` | 200 for pause and resume; NAS stub asserts the task's own `space` is used. |
| `DELETE /api/tasks` | 200; both selected tasks are removed. |

Additional smoke assertions:
- Exhausted quota returns 409 with `今日免费额度已用完（3/3），已使用 3 次` and no create attempt.
- Two concurrent creations with one slot left produce exactly one NAS submission: 200 and 409.
- An unreadable quota fails closed; missing analyzed file ID cannot silently download all files.
- Token rotation, HTTP 401, HTTP 402, and a dropped connection reconnect once and succeed.
- Two consecutive 401 responses stop after one retry and return a structured error.
- Invalid JSON, links, actions, limits and IDs return JSON errors; missing task/route returns 404; unsupported method returns 405.
- `/` serves built HTML and JavaScript assets use gzip when requested.

## Browser Coverage

Headless Chromium uses the same local-only fixture. It verifies:
- Initial device information and tasks; pause, resume and confirmed deletion.
- Link analysis, nested tri-state checkboxes, category filtering, and selected-index submission.
- New task ID toast, initial modal focus, Escape dismissal and destructive-confirmation focus.
- A 360px viewport without horizontal overflow, dark mode and reduced-motion behavior.
- Three-second task polling while visible and no polling in a hidden tab.
- NAS-error base URL and retry, empty state, exhausted-quota chip and explicit submission error.

Screenshots are written under a temporary `thunder-browser-*` directory, not the repository.

## Packaging Checks And Limits

`package.yml` uses the spec's explicit package identity
`cloud.lazycat.app.thunder-remoted`, version `0.1.0`, with zh/en locales.
All three YAML files parse. LazyCat's installed image-config normalizer accepts
the Dockerfile/context configuration and `embed:thunder-remoted` alias. The icon
is exactly 512×512 pixels and 33,791 bytes, below 200 KB; `logo.png` is unchanged.

`docker build -t thunder-remoted:smoke .` was attempted but could not start:

```text
ERROR: permission denied while trying to connect to the docker API at unix:///var/run/docker.sock
```

Consequently, a container image build and an installed LazyCat package have **not**
been runtime-verified here. No device deployment or registry publication was attempted.
The repository's Docker CI job remains responsible for image-build verification.

LocalNas only supports selected indexes on file-backed submissions. The backend
therefore analyzes the link and uses its returned resource file ID; if the NAS
does not supply that ID, it explicitly refuses selection rather than silently
ignoring it. All NAS HTTP operations remain inside thunder-task-rs. As specified,
mutations retry once after transport failure; exactly-once delivery cannot be
guaranteed if the NAS accepted a request before its connection dropped.
