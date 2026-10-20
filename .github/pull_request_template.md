## What & why

<!-- What does this change, and why? Link the issue and task, e.g. "Closes #12 · Task K-2.6". -->

## How it was tested

<!-- Tests written first? Which ones? Anything tested by hand, and on which OS? -->

## Definition of Done

- [ ] Tests written first and passing (CI green on macOS, Windows, Linux)
- [ ] `cargo fmt` / `clippy` / ESLint / type checks clean
- [ ] No user-visible text in code. EN **and** NL strings added in `locales/`
- [ ] No file mutation outside the Executor · no raw paths from the frontend
- [ ] Design tokens only · keyboard accessible · visible focus
- [ ] SPDX licence header on new source files (`pnpm check:spdx`)
- [ ] Docs updated if behaviour or terms changed (CONTEXT.md / ADRs / ARCHITECTURE.md)
- [ ] Every commit is signed off (`git commit -s`, see CONTRIBUTING.md → DCO)

## Screenshots (UI changes)

<!-- Light + dark, EN + NL -->
