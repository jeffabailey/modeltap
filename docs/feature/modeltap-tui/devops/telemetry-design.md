# Telemetry Design — modeltap-tui

**Status:** Design only. **NOT shipped in v1.** Spec'd here for forward-compatibility so the v1 JSONL schema, opt-in mechanism, and upload path don't require a breaking change to enable later.

**Hard constraint (C5, NFR Privacy):** No data leaves the machine without explicit opt-in. This is non-negotiable. Local-AI users are privacy-sensitive by selection. Default behavior in v1 is local-log-only; no upload code path is even compiled in v1 (gated behind a feature flag).

## 1. Why Defer to v1.x

1. **v1 doesn't need it.** First-100-users survey + CI benchmark trend + GitHub issues cover K1, K3, K4, K5. K2 needs first-30-days baseline collection; that can come from the same survey.
2. **Privacy-careful design takes time.** Endpoint, schema, retention, deletion, audit, infra-cost — none of this should be rushed. Better to ship v1 telemetry-free and add it considered.
3. **Avoid feature creep on the walking skeleton.** US-01..US-06 has zero dependency on telemetry; adding it in v1 widens the critical path.
4. **Forward-compat is cheap.** The v1 local JSONL schema (in `kpi-instrumentation.md`) is upload-ready: it already excludes PII, paths, and model names. The v1.x uploader reads the same file and aggregates; no breaking change needed.

## 2. Design Principles

| Principle | Implementation |
|---|---|
| **Opt-in only, ever** | First-run prompt? No — no first-run prompts at all. Telemetry off by default forever. User must explicitly run `modeltap telemetry enable`. |
| **Aggregate before upload** | Never upload raw `launch.log` lines. Uploader aggregates to bucket counts and timing histograms. |
| **No identity** | No installation ID, no machine ID, no user-supplied identifier. Each upload is independent and unlinkable to prior uploads. |
| **Verifiable** | `modeltap telemetry preview` shows exactly what would be uploaded — verbatim. |
| **Cancellable** | `modeltap telemetry disable` stops uploads immediately. `modeltap telemetry purge` requests server-side deletion of any prior uploads (if endpoint supports it; design choice). |
| **Open source the receiver** | The endpoint code is OSS so users can audit the server too. |

## 3. Opt-In UX

```
$ modeltap telemetry status
Telemetry is OFF (default).

modeltap does not send any data to any server. The local file
~/.modeltap/launch.log records session events for your own use
(see `modeltap stats`); it never leaves your machine.

To opt in to anonymous aggregate telemetry (used by maintainers
to improve modeltap):

  modeltap telemetry enable

Run `modeltap telemetry preview` first to see exactly what would
be uploaded.
```

```
$ modeltap telemetry preview
The following aggregate would be uploaded ONCE PER WEEK if telemetry is enabled:

  schema: modeltap.telemetry.v1
  modeltap_version: "1.0.0"
  platform: "macos-aarch64"
  week_start_utc: "2026-04-21"
  sessions_count: 7
  inventory_size_bucket: "21-50"      // bucketed, not exact
  median_first_paint_bucket_ms: "100-200"
  bytes_reclaimed_bucket_gb: "10-50"
  tools_present: ["ollama", "llama-cli", "hf"]   // which plugins reported any models
  community_plugins_present: []        // names of any non-bundled plugins (none in v1)

NOTHING ELSE is uploaded. No paths. No model names. No SHA256s.
No machine ID. No installation ID. No timestamps within sessions.

Each weekly upload is independent and not linkable to any prior upload.

Endpoint: https://telemetry.modeltap.dev/v1/aggregate (HTTPS, TLS 1.3 min)
```

```
$ modeltap telemetry enable
Telemetry enabled. The first weekly upload will occur on next launch
after 2026-05-05 (one week from now). You can cancel at any time with
`modeltap telemetry disable`.
```

The opt-in state is stored in `~/.modeltap/config.toml`:

```toml
[telemetry]
enabled = true
last_upload_utc = "2026-04-28T14:00:00Z"
```

## 4. Wire Schema (`modeltap.telemetry.v1`)

JSON POST body to the endpoint:

```json
{
  "schema": "modeltap.telemetry.v1",
  "modeltap_version": "1.0.0",
  "platform": "macos-aarch64",
  "week_start_utc": "2026-04-21",
  "sessions_count": 7,
  "inventory_size_bucket": "21-50",
  "median_first_paint_bucket_ms": "100-200",
  "p90_first_paint_bucket_ms": "200-500",
  "median_full_inventory_bucket_ms": "1000-2000",
  "bytes_reclaimed_bucket_gb": "10-50",
  "actions_count_bucket": "11-50",
  "tools_present": ["ollama", "llama-cli", "hf"],
  "community_plugins_present": [],
  "dedupable_ratio_bucket": "20-40"
}
```

### 4.1 Bucket definitions

To prevent fingerprinting, all numeric values are bucketed:

| Field | Buckets |
|---|---|
| `inventory_size_bucket` | `0`, `1-5`, `6-10`, `11-20`, `21-50`, `51-100`, `101-250`, `251-500`, `501-1000`, `1000+` |
| `*_first_paint_bucket_ms` | `0-100`, `100-200`, `200-500`, `500-1000`, `1000-2000`, `2000-5000`, `5000+` |
| `*_full_inventory_bucket_ms` | `0-500`, `500-1000`, `1000-2000`, `2000-5000`, `5000-10000`, `10000+` |
| `bytes_reclaimed_bucket_gb` | `0`, `0-1`, `1-5`, `5-10`, `10-50`, `50-100`, `100-500`, `500+` |
| `actions_count_bucket` | `0`, `1-3`, `4-10`, `11-50`, `51+` |
| `dedupable_ratio_bucket` | `0`, `1-10`, `10-20`, `20-40`, `40-60`, `60-80`, `80-100` (percent) |

Buckets chosen so even one user's data point doesn't uniquely identify them — every bucket should have ≥ 5 distinct expected populations to prevent re-identification.

### 4.2 What is NOT in the wire schema

- IP address (server discards on receipt; not logged)
- User agent / installation ID / hardware ID
- Timestamps within a session
- Model names, paths, SHA256s, repo IDs, quantization tags
- Hostname or username
- Free-text fields of any kind

`tools_present` is the names of the bundled plugins that found any models — used to know which integrations users actually use. `community_plugins_present` lists the names of plugins not bundled with modeltap, useful for K4 ecosystem health.

## 5. Endpoint Design (3 candidates)

### 5.1 Cloudflare Workers (recommended for v1.x)

- POST endpoint at `https://telemetry.modeltap.dev/v1/aggregate`
- Worker validates schema, drops invalid payloads, writes accepted records to D1 or R2
- Free tier sufficient for expected volume (estimate: 1000 weekly uploads × 1 KB each = 1 MB/week)
- Cloudflare offers IP-discard on the edge (no logging required)
- Open-source the Worker code in a sibling repo `<org>/modeltap-telemetry-receiver`

**Pros:** zero ops cost; HTTPS for free; geo-distributed; minimal attack surface.
**Cons:** vendor lock-in to Cloudflare (mitigated by simple API surface — easy to rehost).

### 5.2 Static log-receiver (S3 / R2 with presigned writes)

- Client POSTs to a signed upload URL fetched from a tiny endpoint
- Receiver appends to a daily JSONL file in object storage
- Aggregation is a separate batch job

**Pros:** maximally simple receiver; storage costs trivial.
**Cons:** signed-URL fetch is one extra round trip; aggregation is a separate concern; harder to validate schema before storage.

### 5.3 No remote endpoint at all (community-driven baseline)

Don't ship telemetry upload at all. Rely on:
- First-100-users survey at v1 launch (covered in `kpi-instrumentation.md`)
- Annual community survey thereafter
- GitHub issues for K4/K5

**Pros:** zero infra; aligns most strongly with privacy-by-default ethos.
**Cons:** harder to spot K1/K2/K3 trends; relies on survey response rates.

**Recommendation:** ship v1 with option 5.3 (no telemetry upload at all). Defer 5.1 (Cloudflare Workers) to v1.x ONLY IF the maintainer finds the survey-based approach insufficient. The implementation cost of 5.1 is small (~1 day) but the policy cost (privacy review, ToS, deletion process) is non-trivial.

## 6. Upload Cadence

- Weekly, on first launch after 7 days since `last_upload_utc`
- Single upload per week (not per session) — caps server load and reduces fingerprinting risk
- Skip silently if endpoint is unreachable (no retry storms; telemetry is best-effort)
- 5-second timeout; never block the TUI on upload

## 7. Server-Side Considerations (when v1.x ships)

- **Retention:** 90 days. Aggregate to monthly summaries after 90 days; delete raw events.
- **Access:** maintainer-only. No third-party access. No analytics SaaS.
- **Geographic restriction:** none (no PII, so GDPR/CCPA jurisdictional questions don't apply — but reviewing with counsel is still wise before shipping).
- **Deletion request:** since uploads are unlinked, a "delete my data" request can only mean "stop uploading" (which `telemetry disable` already accomplishes). Document this explicitly so users understand.

## 8. Forward-Compatibility Checks

The v1 local JSONL schema (`modeltap.launch.v1`) is the upload-source. v1.x uploader is a pure function of the local log. Verify:

- [x] No PII in `modeltap.launch.v1` (already enforced — see `kpi-instrumentation.md` §2.2)
- [x] All numeric fields bucketable into the wire schema
- [x] `session_id` is per-launch only (not persisted) — already true
- [x] `tools_registered` field exists in `launch.inventory` event — already true
- [x] No timestamps within session leak — `ts` on each event is the only time field, and aggregation discards individual `ts` values

If any of these change in v1.0.x, the v1.x uploader work is non-trivial.

## 9. Auditability

The opt-in flow is auditable end-to-end:
- `modeltap telemetry preview` shows the exact payload before opt-in
- `modeltap telemetry status` shows the current state
- `modeltap telemetry log` (deferred to v2 if useful) prints the last 4 weekly uploads with their full payloads
- The receiver source is open-source, so privacy claims are verifiable against the code that processes the data

## 10. Definition of Done (this design)

- [x] Opt-in UX specified
- [x] Wire schema specified
- [x] Bucket definitions specified
- [x] Three endpoint options compared with recommendation
- [x] Cadence and retry policy specified
- [x] Privacy guarantees enumerated
- [x] Forward-compat with `modeltap.launch.v1` verified
- [ ] Implementation: deferred to v1.x. Tracked in CHANGELOG under "Unreleased / Deferred."
