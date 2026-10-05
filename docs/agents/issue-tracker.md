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

## Wayfinding operations

`wayfinder` keeps its **map** in one issue and its tickets in **child** issues.

- **Map**: an issue labeled `wayfinder:map` whose body holds Notes,
  Decisions so far, and Fog. Create it with
  `gh issue create --label wayfinder:map --body-file <file>`.
- **Child ticket**: an issue linked to the map as a GitHub sub-issue through
  `gh api`. Without sub-issues, list the child in a task list in the map body
  and start the child body with `Part of #<map>`. Label it
  `wayfinder:<type>`, where type is `research`, `prototype`, `grilling`, or
  `task`. Assign the claimed ticket to the driving developer.
- **Blocking**: use native issue dependencies:
  `gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`.
  Get the blocker's database ID with
  `gh api repos/<owner>/<repo>/issues/<n> --jq .id`; it is not the issue
  number or `node_id`. Without dependencies, start the child body with
  `Blocked by: #<n>, #<n>`. A ticket is unblocked when every blocker is closed.
- **Frontier**: list the map's open children, then drop those with an
  assignee or an open blocker (`issue_dependencies_summary.blocked_by > 0`,
  or an open issue in the `Blocked by` line). The first remaining child in map
  order wins.
- **Claim**: run `gh issue edit <n> --add-assignee @me` as the session's first
  write.
- **Resolve**: comment the answer with `--body-file`, close the issue, then
  append a gist and link to the map's Decisions so far.
