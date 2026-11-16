# Provider client settings

Research date: 2026-11-16. Primary sources only. Recommendations below are product policy, not requirements imposed by OpenAI. Scope: one saved URL and credential per STT, TTS, and OCR feature; cleanup remains independent. No server access-token changes or per-model account system.

Implementation choice requested by the user: automatically refresh GET-only
catalog discovery after a 600 ms pause in URL/key edits, retain inline manual
refresh icons for model and voice fields, cancel stale browser requests, and
ignore stale results. This replaces the separate fetch-button row. It does not
change the recommendation against implicit inference requests.

## Documented facts

- The official Python SDK merges operation paths into `base_url`, retaining its path prefix. A base URL identifies the API root; an operation endpoint identifies a particular resource. Passing a full speech endpoint as the SDK base would append another operation path. See [`_prepare_url`](https://github.com/openai/openai-python/blob/main/src/openai/_base_client.py).
- STT uses [`POST /audio/transcriptions`](https://developers.openai.com/api/reference/python/resources/audio/subresources/transcriptions/methods/create), with an audio file and model; TTS uses [`POST /audio/speech`](https://developers.openai.com/api/reference/python/resources/audio/subresources/speech/methods/create), with text, model, and voice. These are inference operations, not connection probes.
- Vision text extraction uses image inputs through [Chat Completions](https://developers.openai.com/api/reference/python/resources/chat/subresources/completions/methods/create) or [Responses](https://developers.openai.com/api/reference/python/resources/responses/methods/create), according to the selected protocol. OpenAI's [vision guide](https://developers.openai.com/api/docs/guides/images-vision) documents both; an `/ocr` route is not the standard OpenAI vision operation.
- [`GET /models`](https://developers.openai.com/api/reference/resources/models/methods/list) lists models and basic metadata. This does not execute any of the above POST operations or demonstrate that a model supports a particular modality. OpenAI's documentation does not guarantee that third-party compatible servers implement discovery.
- Azure routing is provider-specific: the official [Azure SDK adapter](https://github.com/openai/openai-python/blob/main/src/openai/lib/azure.py) constructs deployment paths, preserves `api-version` query parameters, and distinguishes `api-key` authentication from bearer authentication. A URL alone cannot make a generic bearer client fully Azure-compatible.
- [HTTP semantics](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.4) distinguish redirects that can change POST into GET (301/302/303) from method-preserving 307/308, and advise removing sensitive headers during redirection. [401](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.5.2) concerns authentication; [404](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.5.5) can mean absent or concealed resources; [405](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.5.6) means the method is unsupported for that resource. A browser GET to a POST endpoint does not test inference.
- The [official SDK](https://github.com/openai/openai-python#timeouts) exposes timeouts, defaults to ten minutes, and retries certain errors twice by default. Those are SDK defaults, not a recommended desktop settings-probe policy.

## Recommended settings contract

1. Give STT, TTS, and OCR separate persisted URL/token pairs, loaded back into their own controls. Save and Clear must affect only that feature; cleanup retains its existing independent credential. A blank credential means no Authorization header for servers permitting anonymous access. Never inherit another feature's token or silently restore a cleared token from defaults/environment. Distinguish omitted secret (unchanged) from explicit clear in API updates; never persist a masked placeholder as a token. Clear must survive reload/restart. URL clear must not silently contact a public default provider.
2. Label the field “API base URL or full endpoint.” Parse structurally, trim surrounding whitespace, allow HTTP/HTTPS (including private-network HTTP), and reject userinfo/fragments. Preserve host, port, path case, custom prefixes, and query parameters. Reject malformed input before saving; saving valid syntax does not require a successful network test.
3. Resolve only exact, known operation suffixes. A matching full endpoint is used once, without appending it again. A base receives the selected operation path by appending to its existing path, never by an origin-root URL join that discards the prefix. Do not blindly add `/v1`, truncate at `/v1`, or search-and-replace embedded path segments. Show the resolved operation URL. A clearly mismatched known suffix (e.g. speech in STT) should produce a correction message, not a guessed rewrite. Keep the entered value so normalization is visible and reversible.

| Entered value | TTS operation URL |
| --- | --- |
| `http://100.85.156.101:8765/v1` | `http://100.85.156.101:8765/v1/audio/speech` |
| `http://100.85.156.101:8765/v1/audio/speech` | Same URL |
| `https://host/proxy/team/openai/v1/` | `https://host/proxy/team/openai/v1/audio/speech` |

Preserve Azure-shaped deployment paths and `api-version` when resolving an explicitly entered operation endpoint. Do not claim Azure support until routing, query handling, and authentication are supported together; show a precise unsupported-auth/protocol message if needed. Arbitrary custom operation paths cannot reliably be distinguished from bases by guessing: require an explicit endpoint mode if supporting those paths.

## Discovery, validation, and failures

- Offer explicit “Refresh models,” using GET on a known API base plus `/models`. For recognized full endpoints, remove only the exact operation suffix to obtain a candidate discovery base, preserving the preceding prefix. For ambiguous/provider-specific URLs, skip guessing and allow manual model IDs. Do not send credentials to alternate hosts or sweep guessed paths.
- A successful model list means discovery succeeded, not “STT/TTS/OCR tested.” Accept manually entered model IDs even when discovery is missing, empty, or incompatible. On discovery 404/405, say “Model discovery unavailable; enter a model ID.” Keep saved settings intact. Do not conclude the inference endpoint is broken or the token invalid.
- A 401 should identify the feature and request that failed and suggest checking its credential/auth scheme; 403 indicates access denied. For operation 404, report the resolved path and possible route/model mismatch using sanitized provider details. Connection failures and timeouts remain separate states. Never erase credentials or rewrite URLs after a failed probe.
- Disable automatic redirects for authenticated discovery and inference. Report the redirect and let the user correct the saved URL. Never forward bearer tokens, API keys, or inference bodies to a different origin or an HTTPS-to-HTTP redirect. If same-origin redirects are later supported, bound hops and enforce method/body semantics explicitly.
- Use a bounded discovery budget (recommendation: 3-second connect, 10-second total, no automatic retries), cancellation, and stale-result suppression when settings change. Inference needs the existing longer [request budget](request-timeouts.md), including cold model loading; a discovery timeout is not an inference limit. Avoid automatic POST retries without an explicit duplicate-work policy: timeout does not prove the server performed no work.
- Save, Clear, URL normalization, and model refresh must never start inference. A separate explicit feature test should disclose that it sends audio/text/image data and may incur provider charges, then use a small user-approved sample. Never silently record a microphone, capture a screen, or use billable inference as a fallback for missing `/models`.

Suggested acceptance checks: independent save/clear across all four features after restart; blank-token anonymous server; base/full-endpoint equivalence; preserved proxy/Azure prefix and query; missing `/models` with manual model; GET/POST distinction; redirect blocked without secret forwarding; distinct 401/404/timeout messages; no inference requests during Save/Clear/Refresh models. Use mocked HTTP fixtures for these checks.
