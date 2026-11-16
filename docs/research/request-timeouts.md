# Reader request timeout policy

Reader HTTP and worker requests need phase-specific limits. A single short
timeout can reject a valid first use while loading a large OCR, speech, or TTS
model; a single resettable timeout can keep a disconnected request alive
indefinitely. `reader-core::request_budget` supplies a monotonic policy with
independent connection, queue, model-load, inference, stream-idle, and absolute
total limits.

## Recommended defaults

| Phase or cap | Default | Purpose |
| --- | ---: | --- |
| Connection | 3 s | Bound connection establishment |
| Queue wait | 30 s | Bound time waiting for a worker/request slot |
| Cold model load | 180 s | Allow legitimate first-use model startup |
| Warm model load | 30 s | Catch a stalled reload/reinitialization sooner |
| Warm inference | 60 s | Bound a non-streaming inference after model readiness |
| Stream inactivity | 60 s | Bound a stream that stops making progress |
| Total request | 300 s | Hard cap across all stages and retries/progress |

These defaults are policy recommendations, not measured guarantees for every
machine, model, or provider. Make them configurable where the owning service
already exposes request policy. Keep the total cap longer than the cold-load
allowance so a normal cold load can finish and still leave time for inference.

## Integration contract

Create `RequestBudget::new(started_at, cold_start, budgets)` once per logical
request. Move it through `Connecting`, `Queued`, `LoadingModel`, and then
`Inference` or `Streaming` with `set_stage(stage, Instant::now())`. For a
stream, call `progress(now)` only when bytes or meaningful response events are
actually received. The budget's `deadline()` returns the next monotonic wakeup
deadline (the earlier of the phase deadline and hard total deadline), while
`expired(now)` identifies the limit that fired. `finish()` disables further
deadlines.

The cold model-load allowance is distinct from warm inference. Time spent in
`LoadingModel` is governed by `cold_model_load` for a cold request and
`warm_model_load` otherwise. Once ready, transition to `Inference`, which has
its own `warm_inference` bound. A stuck loader therefore expires, but a cold
load may legitimately exceed the ordinary inference duration.

The original start time never changes. Stage changes start only that stage's
budget, and progress moves only the streaming inactivity deadline. Neither
repeated progress nor stage cycling can extend `total`.

## HTTP compatibility

OpenAI-compatible endpoints often emit no headers or response events until
audio processing has completed. An HTTP client cannot infer that hidden server
work is progressing. For those endpoints, use an adequately long absolute
first-response/inference allowance; do not treat elapsed silence as proof the
server is stuck, and do not invent a progress wire protocol that ordinary
OpenAI-compatible clients do not understand. Once response bytes arrive, a
separate inactivity timeout can bound a stalled stream. Disconnect or
cancellation should still stop local work where the provider and transport
allow it, without relying on client-side progress resets to bypass the hard
request cap.

## Failure handling

On expiration, classify the stage/limit for logs and return the existing
service error/fallback result. Do not retry a request indefinitely. A
configurable local fallback may run after remote failure, but it needs its own
bounded request lifetime and must not silently reset the original request's
overall budget unless the product explicitly defines it as a separate job.
