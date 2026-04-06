# TUI Mirror Draft Replay Retrospective

Date: `2026-04-07`

This document records the investigation and fixes for the Telegram draft-message bug that only appeared on the TUI mirror path.

Related implementation:

- [rust/src/telegram_runtime/status_sync.rs](../rust/src/telegram_runtime/status_sync.rs)
- [rust/src/workspace_status.rs](../rust/src/workspace_status.rs)
- [rust/src/management_api.rs](../rust/src/management_api.rs)
- [rust/src/runtime_protocol.rs](../rust/src/runtime_protocol.rs)
- [docs/mirror-function-notes.md](mirror-function-notes.md)
- [docs/plan/management-desktop-surface/working-session-observability.md](plan/management-desktop-surface/working-session-observability.md)

## Summary

The visible Telegram symptom was:

- draft text in a Telegram thread would appear to restart from the beginning
- the text could rapidly replay from `我` -> `我先` -> `我先找...`
- this happened only when the reply was coming from a live local TUI mirror session

The bug turned out to have two layers:

1. same-turn regressive snapshots could still replay into the active Telegram draft
2. even after guarding against obvious regression, the TUI mirror path was still applying large bursts of cumulative `preview_text` snapshots one by one

The second layer was the main cause of the user-visible "從頭刷新" behavior.

## Scope

This was not a general Telegram preview bug.

It was specific to the path:

- local `hcodex` / TUI session emits mirror events
- workspace `.threadbridge/state/runtime-observer/events.jsonl` stores cumulative `preview_text`
- [status_sync.rs](../rust/src/telegram_runtime/status_sync.rs) polls and mirrors those events into a Telegram draft

Direct bot-side turns were not the primary problem here.

## User-Visible Symptom

What the user saw:

- the same Telegram draft stayed attached to one reply
- but the content looked like it was being typed again from the head
- the replay could happen more than once inside the same turn

That symptom initially looked like one of these:

- draft ownership was lost and recreated
- `turn_id` dedupe was failing
- an older preview from the same turn was overwriting a newer one

Those were reasonable first hypotheses, but they were incomplete.

## What Was Fixed First

The first fix hardened same-turn replay handling in [status_sync.rs](../rust/src/telegram_runtime/status_sync.rs):

- `MirrorPreviewState` started tracking `latest_preview_text`
- same-turn shorter previews that were a prefix of the already-applied text were skipped
- regression tests were added for:
  - same-turn regressive preview
  - turn change not being treated as regression

This fixed one real class of bug:

- old same-turn snapshots such as `我` or `我先` could no longer roll the draft back after a much longer preview had already been applied

But it did not fully fix the user-visible replay.

## Why The First Fix Was Not Enough

After the regression guard landed, the user still reported the draft replaying from the beginning in TUI mirror mode.

At that point the question changed from:

- "are we applying stale snapshots?"

to:

- "why are we still visibly replaying if stale same-turn snapshots are already filtered?"

That required better observability.

## Added Observability

To make the behavior inspectable, a new debug event was added:

- `mirror_preview_sync`

It is written into:

- `.threadbridge/state/runtime-observer/events.jsonl`

Each event records:

- `decision`
- `claim_status`
- `previous_turn_id`
- `active_turn_id`
- `turn_transition`
- `draft_id`
- preview length and preview head
- whether the new preview is a prefix of the previously applied preview
- `source_event_at`

The same records were also exposed through a session-scoped management API route:

- `GET /api/threads/:thread_key/sessions/:session_id/mirror-preview-events`

This let us inspect a single TUI session without manually parsing the raw JSONL file.

## What The Logs Actually Showed

The key investigation session was:

- `session_id = 019d63b5-d78a-7843-9fcc-b443d06cbe87`
- `turn_id = 019d63ce-884d-7c32-9186-a86e0c6dbfb6`

The event log showed:

- `326` `preview_text` events for the same turn
- `325` `mirror_preview_sync` events for the same turn
- `321` of those debug events were `decision = applied`
- only `4` were `decision = skipped_regressive`

The important part was not just the counts. It was the shape:

- the TUI ingress emitted many cumulative snapshots
- for example: `我` -> `我先` -> `我先找出...`
- later, `status_sync` applied a large batch of those snapshots almost instantly into the same Telegram draft

Observed burst examples from the log:

- `2026-04-06T17:19:47.646Z`: `49` applied updates in `0.037s`, `1 -> 122` chars
- `2026-04-06T17:20:25.360Z`: `68` applied updates in `0.039s`, `2 -> 139` chars
- `2026-04-06T17:21:23.745Z`: `46` applied updates in `0.029s`, `2 -> 127` chars

This made the Telegram draft visibly "retype" from the start even though it was still the same draft writer.

## Root Cause

The main bug was not "turn ownership is broken".

The main bug was:

- the TUI mirror pipeline stores cumulative full-text snapshots
- [status_sync.rs](../rust/src/telegram_runtime/status_sync.rs) was consuming them one by one during each poll cycle
- the polling interval is coarse enough that many snapshots accumulate before one drain
- when drained, they were applied sequentially into the Telegram draft

Relevant config:

- `WORKSPACE_STATUS_POLL_INTERVAL_MS` default is `1500`
- `STREAM_EDIT_INTERVAL_MS` default is `750`

That meant the visible replay was mostly a batch-drain effect:

- ingress recorded many valid cumulative previews
- poll-based sync later replayed all of them in order
- Telegram showed the whole buildup instead of only the newest state

The earlier regressive-snapshot bug was real, but it was only a secondary amplifier.

## Why It Only Happened In TUI Mirror Mode

This bug was specific to TUI mirror because that path has all of these properties at once:

- preview data is first materialized into workspace-local event logs
- the preview consumer is poll-based rather than attached directly to every live token update
- the stored preview events are cumulative snapshots, not already-coalesced deltas

So the TUI mirror path had a natural backlog-and-drain failure mode that direct bot-side reply handling did not share in the same way.

## Final Fix

The final behavioral fix was made in [status_sync.rs](../rust/src/telegram_runtime/status_sync.rs):

- during one poll cycle, consecutive `preview_text` events with the same `session_id` and the same `turn_id` are now coalesced
- only the last preview in that consecutive run is mirrored into Telegram

In practice, this means:

- if the log contains `我`, `我先`, `我先找...`, `我先找出目前哪個...`
- Telegram now receives only the last state from that run
- the visible "from-head replay" is removed

The implementation introduced a helper:

- `preview_event_run_end(...)`

and added focused tests for:

- collapsing consecutive same-turn snapshots
- stopping correctly at session or turn boundaries

## Effective Fix Sequence

The actual repair sequence matters:

### 1. Regression guard

First fix:

- skip same-turn shorter prefix snapshots when a longer preview is already active

This removed true rollback.

### 2. Better observability

Second step:

- add `mirror_preview_sync`
- expose the debug timeline through the management API

This made the remaining bug measurable instead of anecdotal.

### 3. Poll-batch coalescing

Final fix:

- collapse consecutive preview bursts and only apply the last snapshot per run

This removed the dominant user-visible replay pattern.

## What Was Ruled Out

The logs let us rule out a few incorrect explanations for the observed run:

- the draft was not being recreated every time
- ownership was not repeatedly lost
- the main failure was not "old turn" corruption
- the majority of applied events were not rejected-then-reclaimed writes

In the investigated run, nearly all writes were:

- same `turn_id`
- same `draft_id`
- `claim_status = already_owned`
- `decision = applied`

That is why the visible replay could happen even with ownership behaving correctly.

## Operational Lessons

There are a few maintainership lessons here.

### 1. `turn_id` dedupe is necessary but not sufficient

Preventing cross-turn confusion and same-turn rollback does not solve burst replay if the mirror surface stores cumulative snapshots.

### 2. Poll-driven consumers need coalescing

If a producer emits cumulative full-text states and a consumer is poll-based, the consumer must coalesce before rendering to a user-facing draft surface.

### 3. Observability must capture decisions, not just source events

Raw `preview_text` alone was not enough to settle the question.

What made the diagnosis possible was logging:

- what source preview arrived
- what `status_sync` decided to do with it
- which draft it targeted
- whether ownership had changed

## Debug Checklist For Future Regressions

If this class of bug appears again, check in this order:

1. `.threadbridge/state/runtime-observer/events.jsonl`
2. count `preview_text` vs `mirror_preview_sync` for one `session_id` and `turn_id`
3. inspect whether `decision` is mostly:
   - `skipped_regressive`
   - `skipped_claim_denied`
   - `applied`
4. inspect whether the same `draft_id` is being reused
5. inspect whether the applied events arrive as one burst after a poll delay
6. verify whether consecutive cumulative previews are being coalesced before Telegram write

If the log again shows:

- dozens of `applied` updates
- one `draft_id`
- one `turn_id`
- a sub-100ms apply burst after ~1.5s of source accumulation

then the bug is probably a coalescing regression, not an ownership regression.

## Current Status

As of this document:

- same-turn regressive prefix snapshots are guarded
- TUI mirror preview decisions are observable through `mirror_preview_sync`
- consecutive same-turn preview bursts are coalesced before Telegram draft write

The specific "TUI mirror draft rapidly replays from the beginning" bug now has a concrete explanation and a concrete repair path, instead of remaining a vague "draft dedupe sometimes fails" report.
