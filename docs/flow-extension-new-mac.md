# Flow Alfred Extension: New Mac Setup

This runbook is for installing the Flow Alfred workflow on a clean Mac with minimal repo-specific assumptions.

## Prerequisites

- macOS (Apple Silicon or Intel)
- [Alfred 5](https://www.alfredapp.com/) with Powerpack enabled (required for workflows)
- [Xcode Command Line Tools](https://developer.apple.com/xcode/resources/)
- [Rust via rustup](https://rustup.rs/)
- Optional package manager: [Homebrew](https://brew.sh/)
- Optional but useful: [Flow CLI](https://github.com/nikivdev/flow) (`f`) for repo tasks

## Fast Path

```bash
git clone https://github.com/nikivdev/alfred.git ~/code/alfred
cd ~/code/alfred/flow
./install.sh
~/.cargo/bin/flow-alfred link
```

If you use Flow tasks instead of direct commands:

```bash
cd ~/code/alfred
f setup
~/.cargo/bin/flow-alfred link
```

## What `install.sh` Does

- Builds `flow-alfred`
- Installs it to `~/.cargo/bin`
- Rebuilds the Swift helper binaries for the current Mac architecture
- Copies those helpers into `Flow.alfredworkflow/bin`

## Validation

Run these after install:

```bash
~/.cargo/bin/flow-alfred --help
~/.cargo/bin/flow-alfred code --root ~/code ""
~/.cargo/bin/flow-alfred repos --root ~/repos ""
~/.cargo/bin/flow-alfred windows ""
```

To confirm Alfred sees the linked workflow:

```bash
syncfolder="$(defaults read com.runningwithcrayons.Alfred-Preferences syncfolder 2>/dev/null || true)"
if [ -n "$syncfolder" ] && [ -d "$syncfolder/Alfred.alfredpreferences/workflows" ]; then
  wf_dir="$syncfolder/Alfred.alfredpreferences/workflows"
else
  wf_dir="$HOME/Library/Application Support/Alfred/Alfred.alfredpreferences/workflows"
fi

echo "Workflow dir: $wf_dir"
ls -la "$wf_dir"
ls -la "$wf_dir/nikiv.dev.flow"
readlink "$wf_dir/nikiv.dev.flow" || true
```

## Workflow Variables

- `code_root`: defaults to `~/code`
- `repos_root`: defaults to `~/repos`
- `editor_app`: optional app name or app path; if unset, project open actions use plain `open`
- `frs_bin`: optional path override for the `frs` binary used by the text-to-docs external trigger

## Manual Steps After Codex Finishes

1. Open Alfred Preferences -> Workflows -> `Flow` and confirm it is present.
2. In the Flow workflow `[x]` menu, set `code_root`, `repos_root`, and optionally `editor_app` if your machine layout differs from the defaults.
3. Trigger `win` once in Alfred, then grant Accessibility when prompted in System Settings -> Privacy & Security -> Accessibility.
4. If you use the text-to-docs external trigger, set `frs_bin` or ensure `frs` is on your shell path.

## Quick Validation In Alfred

- `code <query>`: shows repos under `code_root`.
- `repos <query>`: shows repos under `repos_root`.
- `win <query>`: shows windows for current app and switches on enter.

## Troubleshooting

- `flow-alfred not found` in Alfred:
  - From repo root, run `cargo install --path ./flow --force`
  - Confirm `~/.cargo/bin/flow-alfred` exists.
- `Destination exists and is not a symlink` on link:
  - Remove existing real folder for bundle ID, then relink:
    - `rm -rf "<alfred_workflows_dir>/nikiv.dev.flow"`
    - `~/.cargo/bin/flow-alfred link Flow.alfredworkflow --bundle-id nikiv.dev.flow`
- `win` shows no windows:
  - Re-check Accessibility permission for Alfred.
  - Rebuild and re-copy Swift helpers.
- Open action uses the wrong app:
  - Set the workflow `editor_app` variable to an app name like `Zed`, `Visual Studio Code`, or a full app path.
