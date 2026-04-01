# [Alfred](https://www.alfredapp.com) workflows

## Dev 

With [flow](https://github.com/nikivdev/flow), run `f setup`, then `f` will search through list of tasks.

Currently making workflow for [flow](https://github.com/nikivdev/flow) which can be found [here](flow).

## Flow Alfred

The workflow includes lightweight repo/session utilities plus recipe search for
copy-pasteable Codex instructions, plus a docs search lane for `~/docs`.

- `code <query>` searches `~/code`
- `repos <query>` searches `~/repos`
- `agents <query>` searches the generated `~/run` Codex agent catalog
- `codex-tabs <query>` searches live open Codex tabs across Zed windows
- `docs <query>` searches `~/docs`
- `docs-recent <query>` searches docs touched in the last 5 days
- `recipe <query>` searches `~/docs/codex/recipes`

In `recipe`, pressing return copies the recipe payload to the clipboard and
`cmd` opens the source markdown note in Zed Preview.

In `agents`, pressing return autopastes the stable published
`~/run/agent-context/<agent-id>/run.md` path into the frontmost app, `cmd`
opens the focused source context when one exists or falls back to the source
skill file in Zed, and `alt` copies the `$mention` form. It reads from
`~/run/.ai/artifacts/codex/run-agent-skills.json`, so refresh that first with
`f run --config ~/run/flow.toml codex-skills-sync` when the run-owned agent
catalog changes.

In `codex-tabs`, pressing return opens the live `zed://codex/session/...` jump
URL so Zed focuses the right window and tab, `cmd` copies the project path, and
`alt` copies the Codex session id. It reads from the snapshot exported by the
forked Zed build at
`~/Library/Application Support/Zed/state/open-codex-sessions.json`.

In `docs`, pressing return opens the note in Zed, `cmd` copies the absolute
path, and `alt` copies the configured docs-root path. Empty-query results are
sorted by most recent activity first. `docs-recent` uses the same actions, but
filters the list to docs touched in the last 5 days.

## Contributing

[Use AI](https://nikiv.dev/how-i-code) & [flow](https://github.com/nikivdev/flow). All meaningful issues and PRs will be merged in. Thank you.

[![Discord](https://go.nikiv.dev/badge-discord)](https://go.nikiv.dev/discord) [![X](https://go.nikiv.dev/badge-x)](https://x.com/nikivdev) [![nikiv.dev](https://go.nikiv.dev/badge-nikiv)](https://nikiv.dev)
