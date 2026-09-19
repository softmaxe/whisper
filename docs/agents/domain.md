# Domain documentation

This repository uses a single-context layout:

- `CONTEXT.md` at the repository root contains the domain glossary.
- `docs/adr/` contains architectural decision records.

## Before exploring

Read the root `CONTEXT.md` and ADRs relevant to the area being explored.
If these files do not exist, proceed silently.

Create domain documents lazily through `domain-modeling`, when terms are
resolved or an architectural decision warrants a record. Keep `CONTEXT.md`
limited to domain vocabulary; record implementation decisions in ADRs.

## Vocabulary and decisions

Use the glossary's canonical terms in issues, proposals, tests, and explanations.
Flag vocabulary gaps for `domain-modeling`.

If a proposal conflicts with an existing ADR, identify the ADR and explain
why the decision should be revisited.
