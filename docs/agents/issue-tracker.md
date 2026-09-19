# Issue tracker: GitHub

Issues and specs live in this repository's GitHub Issues. Use the `gh` CLI
inside the clone; resolve the repository from its Git remote.

## Conventions

- Create an issue with `gh issue create --title "..." --body-file <file>`.
- Read an issue with `gh issue view <number> --comments`; fetch its labels
  with `gh issue view <number> --json labels`.
- List issues with `gh issue list --state open --json number,title,body,labels`.
  Add label and state filters for the task.
- Comment with `gh issue comment <number> --body-file <file>`.
- Apply or remove labels with `gh issue edit <number> --add-label "..."`
  or `--remove-label "..."`. Use the mapping in [triage labels](triage-labels.md).
- Close an issue with `gh issue close <number>`.

For multiline bodies, write the exact text to a temporary file, pass it with
`--body-file`, and remove the file afterward.

When a skill says "publish to the issue tracker", create a GitHub issue.
When it says "fetch the relevant ticket", read the issue and its comments.

## Pull requests as a triage surface

**PRs as a request surface: no.**
