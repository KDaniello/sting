# `sting-abi-service`

The public, permissionless HTTP layer over `sting-abi-pipeline::ExtractController`
— `git show b6762e5:docs/abi-layer/blocks/03-api-service-mvp.md` (в истории: `git show cf52463:git show b6762e5:docs/abi-layer/blocks/03-api-service-mvp.md`),
`git show a747a5b:docs/abi-layer/critical/SERVICE_ARCHITECTURE.md` (в истории: `git show cf52463:git show a747a5b:docs/abi-layer/critical/SERVICE_ARCHITECTURE.md`).

## Endpoints

| Method | Path | What it does |
|---|---|---|
| `POST` | `/v1/extract` | `{"source": "...", "lang": "tolk"\|"tact"\|"func"}` → a `ContractSchema` JSON body. `"auto"` is rejected — see below. |
| `POST` | `/v1/extract/batch` | `{"items": [{"id", "source", "lang"}, ...]}` → `{"results": [{"id", "status": "ok"\|"error", "schema"?, "error"?}, ...]}`. One bad item never fails the whole batch. |
| `POST` | `/v1/verify` | `{"address": "...", "source": "...", "lang": "tolk"\|"tact"\|"func"}` → `{"status": "full_match"\|"partial_match"\|"no_match", "schema"?}` — real on-chain byte comparison (Block 05 (`git show 4711a45:docs/abi-layer/blocks/05-verification-registry.md`, в истории: `git show cf52463:git show 4711a45:docs/abi-layer/blocks/05-verification-registry.md`)). **Tolk and Tact both supported** (`sting-fift`+`TolkBytecodeSource` closed the Tolk gap, `git show fec904c:docs/abi-layer/critical/PRODUCTION_READINESS_TZ.md` Фаза 2) — `FunC` fails per-request with `unsupported_language` (Tier 2 has no real compiler to recompile against, categorical, not a gap). Requires `sting-corpus` to be configured (real network access) — `503 registry_unavailable` if not. |
| `GET` | `/v1/schema/{address}` | Real lookup against the verified-schema store built by `/v1/verify` — `200` + `ContractSchema` if a `full_match`/`partial_match` has ever been recorded for this address, `404 no_verified_source` otherwise (an honest miss, not an empty success). |
| `POST` | `/v1/probe` | `{"address": "..."}` → the real Tier 3 (`sting-abi-dictionary`) `ProbeReport` JSON — bytecode-only method-id probing, no source required or read at all (`git show fec904c:docs/abi-layer/critical/PRODUCTION_READINESS_TZ.md` Фаза 4 closed the "Tier 3 only reachable from the offline CLI" gap). Requires `sting-corpus` (network), the method-id dictionary, and `libemulator` all configured — `503 tier3_unavailable` if any is missing, `502 probe_failed` if a real TVM call fails (malformed on-chain code). |
| `GET` | `/v1/health` | `200 {"status": "ok"}` if the process is up. Never rate-limited. |
| `GET` | `/v1/version` | This crate's version plus the `ContractSchema` IR version it produces. |

Every unsuccessful response has the same shape:
```json
{"error": {"code": "...", "message": "..."}}
```
Codes: `unsupported_language`, `compilation_failed`, `timeout`, `payload_too_large`,
`rate_limited`, `invalid_request` (a pragmatic addition for malformed
requests — bad JSON, an unknown `lang` — not in the original spec's code
list, which focused on extraction-specific failures), `no_verified_source`,
`onchain_resolution_failed`, `registry_unavailable`, `internal_error`,
`tier3_unavailable` (`/v1/probe` missing its dictionary/`libemulator`/corpus),
`probe_failed` (`/v1/probe`'s real TVM call failed).

### Why `"lang": "auto"` doesn't work here

`sting-abi-cli`'s own `--lang`/auto-detect works from a real file
extension. An inline HTTP `source` string has no filename at all — there
is nothing honest to detect from, and content-sniffing was already
rejected on precision grounds
(`git show d41539b:docs/abi-layer/blocks/02-normalized-ir-cli.md` (в истории: `git show cf52463:git show d41539b:docs/abi-layer/blocks/02-normalized-ir-cli.md`)
§2.2). Pass an explicit `"lang"`.

## Configuration (env vars, all optional)

| Var | Default | Meaning |
|---|---|---|
| `PORT` | `8080` | TCP port to listen on. |
| `STING_TOLKC_PATH` | *(resolved next to the binary)* | Path to the `tolk`/`tolk.exe` executable. |
| `STING_TACT_JS_PATH` | *(resolved next to the binary)* | Path to `tact.js`. |
| `STING_FIFT_PATH` | *(resolved next to the binary)* | Path to the `fift`/`fift.exe` executable — needed for Tolk `/v1/verify`. |
| `STING_DICTIONARY_PATH` | `corpus-abi/method-dictionary.json` (relative to CWD) | Path to the Tier 3 method-id dictionary `/v1/probe` probes against. |
| `STING_LIBEMULATOR_PATH` | *(resolved next to the binary)* | Path to the vendored `libemulator` shared library — needed for `/v1/probe`. |
| `MAX_BODY_BYTES` | `1000000` | Request body size limit. |
| `COMPILE_TIMEOUT_SECS` | `60` | Passed to `TolkCompiler`/`TactCompiler` — the real, process-killing timeout. Raised from `20` after `git show a747a5b:docs/abi-layer/LOAD_TEST_REPORT.md` found the old default insufficient for Tact's real cold-start cost under load. |
| `REQUEST_TIMEOUT_SECS` | `COMPILE_TIMEOUT_SECS + 30` | Async-level safety-net timeout wrapping the whole extraction call — validated against a `batch_size=3` Tact batch, see `ServiceConfig::REQUEST_TIMEOUT_HEADROOM`'s doc comment for why it doesn't scale with `BATCH_MAX_ITEMS` yet. |
| `BATCH_MAX_ITEMS` | `100` | Hard cap on `POST /v1/extract/batch`'s `items` array. |
| `RATE_LIMIT_REQUESTS` | `60` | Requests per IP per `RATE_LIMIT_WINDOW_SECS` — counts *requests*, not batch items; a `BATCH_MAX_ITEMS`-sized batch costs the same one token as a single-item request (`git show e1f3107:docs/PLATFORM_REVIEW.md` §4). |
| `RATE_LIMIT_WINDOW_SECS` | `60` | Rate-limit refill window. |

Missing either compiler at startup does **not** stop the service from
binding — see `src/main.rs::build_controller`'s doc comment. A request for
a language whose compiler wasn't found fails per-request with
`unsupported_language`, not a crash.

## Running it

```sh
cargo run -p sting-abi-service --bin sting-abi-service
```

```sh
curl -X POST http://localhost:8080/v1/extract \
  -H 'content-type: application/json' \
  -d '{"source": "...", "lang": "tolk"}'

curl -X POST http://localhost:8080/v1/verify \
  -H 'content-type: application/json' \
  -d '{"address": "EQ...", "source": "...", "lang": "tact"}'

curl http://localhost:8080/v1/schema/EQ...
```

A full, real, curl-by-curl walkthrough (including a genuine on-chain
`no_match`) — `git show 4f15c49:docs/abi-layer/SHOWCASE.md` (в истории: `git show cf52463:git show 4f15c49:docs/abi-layer/SHOWCASE.md`).

## Docker

```sh
docker build -f Dockerfile.abi-service -t sting-abi-service .
docker run --rm -p 8080:8080 \
  -v "$(pwd)/vendor/tolk:/compilers/tolk:ro" \
  -v "$(pwd)/vendor/tact:/compilers/tact:ro" \
  -v "$(pwd)/vendor/fift:/compilers/fift:ro" \
  -v "$(pwd)/vendor/libemulator:/lib/emulator:ro" \
  -v "$(pwd)/corpus-abi/method-dictionary.json:/data/method-dictionary.json:ro" \
  -e STING_TOLKC_PATH=/compilers/tolk/tolk \
  -e STING_TACT_JS_PATH=/compilers/tact/node_modules/@tact-lang/compiler/bin/tact.js \
  -e STING_FIFT_PATH=/compilers/fift/fift \
  -e STING_LIBEMULATOR_PATH=/lib/emulator/libemulator.so \
  -e STING_DICTIONARY_PATH=/data/method-dictionary.json \
  sting-abi-service
```

Every one of these mounts is independently optional — same forgiving pattern
this whole service already has for missing compilers: absent Tolk/Tact
compilers disable their respective `/v1/extract`/`/v1/verify` paths, absent
`fift` disables Tolk's half of `/v1/verify`, absent `libemulator`/dictionary
disables `/v1/probe` — the container still starts and answers
`/v1/health`/`/v1/version` either way.

The image does **not** bundle either compiler
([`Dockerfile.abi-service`](../../Dockerfile.abi-service)'s own header
comment explains why) — both are resolved at runtime via bind mounts and
the env vars above, the same resolution convention used everywhere else in
this crate's dependencies. Without either mount, the container still
starts and answers `/v1/health`/`/v1/version`; that language's extraction
fails per-request, not at startup.

**Validated this session**: the image was actually built (`docker build`)
and run (`docker run` with `vendor/tact/` bind-mounted), and answered a
real `curl -X POST .../v1/extract` with a real Tact contract end-to-end —
not just a structural review of the Dockerfile.

**Known gap, documented honestly**: this development environment only has
a *Windows* `tolk.exe` vendored locally (`vendor/tolk/`) — there is no
Linux `tolk` binary available here to mount into a Linux container, so
only the Tact path was validated *inside* a real container this session.
The Tolk path is validated by `cargo test`'s real-compiler integration
tests (native Windows) and a native `cargo run` + `curl` demonstration
(see `git show b6762e5:docs/abi-layer/blocks/03-api-service-mvp.md`'s own DoD notes) — the
HTTP/router/service code path is identical either way, only the
containerized *Linux binary* itself is unverified. Tact's `tact.js` is
plain cross-platform JS, so that path
works the same inside or outside a container once mounted.

## Testing

`tests/http.rs` — routing, error format, timeouts, rate-limiting, all via
mock extractors and `tower::ServiceExt::oneshot` (no real socket).
`tests/real_end_to_end.rs` — the same, but with the real vendored
compilers; skips gracefully if `vendor/tolk`/`vendor/tact` aren't present
locally.
