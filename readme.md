# [Alfred](https://www.alfredapp.com) workflows

## Flow Workflow

This repo currently ships a Flow-focused Alfred workflow in [flow](flow).

For a fresh Mac:

```bash
cd flow
./install.sh
~/.cargo/bin/flow-alfred link
```

That installs the Rust CLI, rebuilds the Swift window-switcher helpers for the local Mac architecture, and links the workflow into Alfred.

Workflow variables:

- `code_root` defaults to `~/code`
- `repos_root` defaults to `~/repos`
- `editor_app` is optional; if unset, Alfred uses `open`
- `frs_bin` is optional and only affects the text-to-docs external trigger

Alfred still needs the normal manual bits: Powerpack enabled, Alfred opened at least once, and Accessibility granted if you want the `win` window switcher.

Full setup notes for another machine are in [docs/flow-extension-new-mac.md](docs/flow-extension-new-mac.md).

## Contributing

[Use AI](https://nikiv.dev/how-i-code) & [flow](https://github.com/nikivdev/flow). All meaningful issues and PRs will be merged in. Thank you.

[![Discord](https://go.nikiv.dev/badge-discord)](https://go.nikiv.dev/discord) [![X](https://go.nikiv.dev/badge-x)](https://x.com/nikivdev) [![nikiv.dev](https://go.nikiv.dev/badge-nikiv)](https://nikiv.dev)
