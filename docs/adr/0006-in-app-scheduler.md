# ADR-0006: Scheduled scans and rules run in-app, not through OS schedulers

- Status: Accepted
- Date: 2026-10-20

## Context
v1 adds scheduled scans and scheduled rules. Options are OS schedulers (launchd, Windows Task Scheduler, systemd timers or cron) or a scheduler inside the running app (tray mode, optional launch at login).

## Decision
Kipple uses an **in-app scheduler** in `kipple-engine`:
- It runs while Kipple runs, either open or in the tray. Launch at login is opt-in.
- Runs missed while the app was closed are **caught up once** at the next start, never once per missed slot.
- Kipple installs no OS-level jobs.
- Schedules are checked against **wall-clock time**, and re-checked when the system wakes from sleep. Monotonic timers (e.g. tokio's) may not advance during sleep.

## Consequences
- One implementation for all three OSes, nothing extra to clean up on uninstall, and no extra permissions.
- Schedules don't run while Kipple is fully closed. That's acceptable, and clearer for users ("Kipple only acts while it's running").
- Watch-mode rules also need the app running, so the same mental model applies to all automation.
