# Spark — reactive components

Spark is Anvilforge's answer to Livewire: server-rendered components with
state and methods that look like normal Rust, an action protocol the
browser drives without a JS framework, and partial re-render in place.
Templates are `.forge.html`; the runtime is a hand-authored ~12 KB JS
file (`dist/spark.min.js`) with no Node toolchain on either side.

This page describes the architecture — particularly the state model,
because the "server-held state" framing that's natural for Livewire does
not describe Spark, and the operational implications are different.

## The mental model in one paragraph

A Spark component is a Rust struct with `#[spark_component]`. On the
initial page render, the server constructs the struct (via `mount`),
renders its template, and emits the HTML alongside a **signed snapshot**
of the struct's state. The snapshot lives in the DOM, not in server
memory. On every interaction (`spark:click`, `spark:model`, …) the
runtime ships the snapshot back to `POST /_spark/update`; the server
verifies the signature, deserializes the struct, dispatches the action,
re-renders, returns refreshed HTML plus a fresh signed snapshot. The JS
runtime morphs the existing DOM in place. Between requests, the server
holds nothing.

## State lives in the DOM, not in server memory

This is the load-bearing design choice and the answer to most "what about
session affinity / memory residency / failure modes" questions.

Each snapshot is a base-64-URL JSON envelope:

```text
b64url({
  v: 1,
  data: <struct's serialized state>,
  memo: { id, class, view, listeners, errors? },
  checksum: HMAC-SHA256(APP_KEY, canonical(data) || canonical(memo))
})
```

The checksum is verified in constant time on every inbound request. Tamper
attempts return `HTTP 419` and the browser auto-reloads the page (matching
Livewire's stale-session UX). Optional `SPARK_ENCRYPT=true` flips the
envelope to AES-256-GCM under the same `APP_KEY`, so the client can't
even read its own state if you don't want it to.

The implementation is in [crates/spark/src/snapshot.rs](../../../crates/spark/src/snapshot.rs).
The encoded form is hard-capped at 64 KB by the decoder; anything larger
is rejected before deserialization.

### Consequence — no session affinity

The server has no per-component memory between requests. Any node that
shares `APP_KEY` can verify and act on a snapshot produced by any other
node. There is **no sticky-session requirement** behind a load balancer,
no warm/cold node distinction, no need to migrate component state during
a scale-out event. This is the operational difference from server-held
reactive models (LiveView, Hotwire-with-Cable, Phoenix LiveView): those
all keep component process state pinned to a specific node and need
process-or-session affinity at the LB layer. Spark does not.

### Consequence — memory residency is near zero

Between interactions, the server allocates **nothing per component
instance**. A page with 1000 connected users typing into a `spark:model`
input field doesn't pin 1000 component instances in RAM the way a
LiveView app would. The hot path is `decode → deserialize → method →
render → encode → drop`; the struct lives on the request task's stack +
arena for the duration of one HTTP call.

There is one small piece of per-component state the server *does* hold,
intentionally: the **revision tracker** behind optimistic concurrency
control. Each entry is one `(memo.id, u64)` pair — kilobytes per
100k active components, not megabytes. The tracker is an LRU with a
30-minute idle TTL, so abandoned tabs aren't pinning entries forever.

The other exception, also intentional: **Bellows broadcast subscribers**.
A component that subscribes to a channel via `#[spark_on("posts.created")]`
holds a WebSocket subscription on the Bellows broker for the lifetime of
the page. That subscription costs one entry in the broker's subscriber
list and a Tokio task waiting on the channel; it does not hold the
component itself, which is still rehydrated from the latest snapshot
each time an event fires. See [Broadcasting](broadcasting.md).

## Failure modes

These are the things that change behavior in a way users will notice. We
list them explicitly because "server-held state" frameworks have a
different failure profile and it's worth being clear about which problems
apply.

| Failure | Behavior | What you do |
|---|---|---|
| **Tampered snapshot** | HMAC verification fails → `HTTP 419` → JS runtime reloads the page. | Nothing; the auto-reload UX matches Livewire. |
| **Cross-origin POST with replayed snapshot** | Session-bound CSRF token mismatch → `HTTP 419` → reload. The HMAC proves the snapshot came from this app's render path, but the CSRF token proves the *request* came from a page the user is currently on. | Nothing — automatic when the session layer is installed. CSRF is opt-in for stateless apps (no session = no check). |
| **`APP_KEY` rotation mid-flight** | When you set `APP_KEYS="2:newkey,1:oldkey"` (priority order — first entry signs new envelopes), the server verifies *any* snapshot whose `kid` matches a key in the ring. In-flight clients with `kid=1` envelopes keep validating; new responses go out signed under `kid=2`. Drop the old key from the env once you're past the rotation window. | Use `APP_KEYS` for zero-reload rotation. The legacy single-key `APP_KEY` still works — missing-`kid` envelopes fall back to the first key in the ring, so you can move to `APP_KEYS` without invalidating existing browsers. |
| **Deploy rollover with struct schema change** | A 0.3.0 snapshot deserialized into 0.3.1 `Counter { count: i32, label: String, new_field: String }` fails on the missing field → `419` → reload. | Make new fields `Option<T>` with `#[serde(default)]` during the rollover, or accept that in-flight clients reload once. Same situation Livewire has with new public properties. |
| **Snapshot grew past 64 KB** | Decoder refuses; client sees `HTTP 413`-ish error and the runtime surfaces it. | Move heavy state out of the component (e.g. into a DB row keyed by an id field in the snapshot). |
| **Network partition mid-action** | The browser's pending `/update` request times out; the component's UI shows the snapshot it had before the click. The server never saw the action; nothing was applied. | Default loading-state UX via `spark:loading[.delay.<ms>]`. Idempotent actions if you care. |
| **Cross-tab updates to the same component** | Each tab holds its own snapshot. A change in tab A is invisible to tab B until tab B's next interaction (or until a `#[spark_on]` broadcast lands). | Use Bellows broadcasts for "live across tabs/users" semantics; the snapshot model gives "live within this tab". |
| **Replay** | The `rev` field + per-`memo.id` revision tracker on the server already block replay within a 24-hour window: a captured snapshot with rev=N can only be POSTed once, because the next attempt sees the tracker expecting rev=N+1 and returns HTTP 409. After 24 hours of no interaction with a given component instance, its tracker entry evicts and a stale snapshot would re-validate as fresh — but its HMAC is still required to pass. For destructive actions (`charge_card`) include an explicit nonce or idempotency key in the action body as defense in depth. | Nothing for the typical case — the rev tracker is automatic. For one-shot money-moving actions, treat them like you would any non-Spark endpoint: add an idempotency token. |
| **Race between two `POST /_spark/update` calls from the same component** | Each snapshot carries a monotonic `rev` field. The server tracks the last revision it issued per `memo.id` and rejects any incoming snapshot whose `rev` doesn't match — the loser gets `HTTP 409` and reloads. Lost-update on the same field across racing requests can't happen. | Nothing for the typical case; the runtime serializes requests per-component anyway. The 409 only fires on malicious replay, multi-tab races, or buggy clients. |

## Resource ceilings

The bound on Spark is not server memory — there isn't any per-active
component. The bounds are:

- **CPU per interaction.** Decode (~1.55 µs for a small component) +
  HMAC verify + JSON deserialize + your action body + render +
  re-encode. For non-trivial pages, the user's action body dominates.
- **Bandwidth per interaction.** Snapshot size goes both ways. A 4 KB
  snapshot means every click costs ~8 KB round-trip (in + out).
  Encryption mode adds AES-GCM tag overhead (~28 B).
- **Bellows subscribers per process.** WebSocket subscribers do live in
  memory, in the broker's subscriber map; the limit is the broker's
  config (`BELLOWS_MAX_SUBSCRIBERS`, default 50_000).

## When Spark is the wrong choice

Spark trades client-side framework code for server round-trips. The trade
is good for forms, dashboards, admin UIs, real-time-but-not-100ms feeds.
It is wrong for anything where one of these is true:

- **Interaction latency budget < ~50 ms.** Loopback + render is fast,
  but a real LAN RTT is 5–50 ms, the open internet is 50–500 ms. Every
  click is a network round-trip. A drag-to-resize handle needs to be
  client-side.
- **Heavy client-side state machine.** A canvas editor, a code editor,
  a chart with brushable axes — those need to keep state in the
  browser and call the server for persistence.
- **Offline-first.** Spark requires a connected server to handle any
  state change.

For mixed workloads, the usual pattern is: Spark for the surrounding
chrome (navigation, lists, forms), a small island of client JS for the
component that needs to be hands-on (Alpine, Stimulus, a single React
mount, whatever). The Spark runtime's `spark:ignore` attribute opts a
subtree out of the morph engine so client-rendered content survives
re-renders.

## What the snapshot wire format looks like

For inspectability — paste this into your browser console on a page with
a Spark component:

```js
document.querySelector('[spark\\:snapshot]').getAttribute('spark:snapshot')
// → "eyJ2IjoxLCJkYXRhIjp7ImNvdW50IjowfSwibWVtbyI6eyJpZCI6IjAxSE5...
//    ...","checksum":"7c2e9f..."}"
//
// Base64-url-decode it and you get the envelope. With SPARK_ENCRYPT=true,
// the same attribute starts with "enc:" and the inner JSON is unreadable
// without APP_KEY.
```

The format is stable across patch releases of Anvilforge. Minor releases
may add fields to `memo` with `#[serde(default)]`; major releases may
break the format outright, in which case a migration note ships with
that release.

## Trade-offs at a glance

| Concern | Spark answer |
|---|---|
| Session affinity | None required. Any node can serve any interaction. |
| Server memory per active component | ~16 B (revision tracker entry); the component instance itself is dropped between requests. |
| Bytes on the wire per click | Snapshot in + snapshot out (~1–8 KB typical, 64 KB hard cap). |
| Tamper protection | HMAC-SHA256, constant-time verify. Optional AES-256-GCM envelope. |
| Visibility into state | Client-readable by default (HMAC mode); opaque under `SPARK_ENCRYPT=true`. |
| Failure UX on stale snapshot | Auto-reload (matches Livewire). |
| Latency floor | One network RTT per interaction. Not a fit for sub-50 ms interactions. |
| Deploy compatibility | Snapshots tied to struct shape; use `#[serde(default)]` on new fields. |
| Memory-resident state available | Yes — via Bellows subscribers, opt-in per component. |

## Where this lives in the code

- [crates/spark/src/snapshot.rs](../../../crates/spark/src/snapshot.rs) —
  envelope format, sign/verify
- [crates/spark/src/crypto.rs](../../../crates/spark/src/crypto.rs) —
  HMAC + AES-GCM primitives
- [crates/spark/src/http.rs](../../../crates/spark/src/http.rs) —
  `/_spark/update` handler
- [crates/spark/src/render.rs](../../../crates/spark/src/render.rs) —
  initial mount + `boot_script`
- [crates/spark/src/morph.rs](../../../crates/spark/src/morph.rs) —
  server-side morph helper
- [crates/spark/dist/spark.min.js](../../../crates/spark/dist/spark.min.js) —
  the 12 KB browser runtime

Performance numbers for the encode/decode/render path are in
[Benchmarks & methodology](../production/benchmarks.md).
