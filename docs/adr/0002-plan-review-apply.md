# ADR-0002: Every file change goes through Plan → Review → Apply, with a durable write-ahead journal

- Status: Accepted (revised after architecture review)
- Date: 2026-10-20

## Context
Kipple's promise is "safe by default": dry-run summaries, undo for every action, recovery after crashes, conflict handling, bulk limits for automatic rules, and a History screen. If cleanup, rename, move, tidy presets, rules, retry and undo each changed files their own way, these guarantees would be implemented many times and would drift apart.

## Decision
1. Every feature that changes files asks the backend for a **Plan**: an ordered list of Operations plus Issues, stored in `state.db` and referenced by `plan_id` ([ADR-0007](0007-backend-trust-boundary.md)).
2. A shared **Validator** checks the Plan when it's created **and again at apply time**: locations and destination tokens, protected paths, collisions, name validity, case-only renames, placeholders, locks, trash availability, and a re-hash of duplicates.
3. The UI shows a dry-run summary and the user resolves Issues. **"Replace" in a conflict becomes a journaled `Trash(existing)` operation**, so it can be undone.
4. The **Executor** is the *only* component allowed to change the file system. It:
   - commits the Batch and **all entries as `pending` before the first operation**, with `synchronous=FULL`,
   - executes each operation and sub-step, recording status and undo info. A Trash entry only counts as done once its trash location is recorded,
   - on startup, **reconciles** interrupted Batches by file identity (`volume_id + file_id`) and offers **Resume** or **Roll back**. It never guesses. An unknown outcome is reported.
5. **Trashing on a volume without a Trash** is a blocking `NoTrashOnVolume` issue. Only an explicit, per-batch "delete permanently" consent (manual applies only) or the v1 holding area gets past it. **Rules can never delete permanently.**
6. **Cross-volume moves** are refused in MVP. In v1 they use copy-to-temp → fsync → verify hash → rename → trash the source. Recovery only removes temp files, and only while the source still exists.
7. **Undo** is an inverse Plan through the same Validator and Executor. It checks identity per entry, warns if the content changed, and skips entries whose identity doesn't match.
8. An **Apply policy** (max operations and bytes, ask or abort, `allow_permanent_delete=false` for rules) lets automatic rules pause on large runs.

## Consequences
- The safety features are implemented once and tested with fault injection: a "crash" at operation N or sub-step N, and simulated power loss.
- Planners stay pure and easy to test with TDD.
- One fully synced commit per batch start, plus group-committed status updates. This is negligible compared with the file operations themselves.
- File-mutating APIs are allowed only in `kipple-engine::executor` and the adapters it calls (`Trash`, `Tags`, `ShellIntegration`). This is enforced with clippy `disallowed-methods`.
