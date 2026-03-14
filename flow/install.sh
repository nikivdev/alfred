#!/bin/bash
set -e

cd "$(dirname "$0")"

echo "Building flow-alfred..."
cargo build --release

echo "Installing to ~/.cargo/bin..."
cargo install --path . --force

if ! command -v swiftc >/dev/null 2>/dev/null; then
  echo "swiftc not found. Install Xcode Command Line Tools before using the window switcher."
  exit 1
fi

echo "Building Swift helpers..."
mkdir -p "$HOME/.cargo/bin" Flow.alfredworkflow/bin
swiftc -O swift-helpers/windows.swift -o "$HOME/.cargo/bin/flow-windows"
swiftc -O swift-helpers/raise-window.swift -o "$HOME/.cargo/bin/flow-raise-window"
cp "$HOME/.cargo/bin/flow-windows" Flow.alfredworkflow/bin/flow-windows
cp "$HOME/.cargo/bin/flow-raise-window" Flow.alfredworkflow/bin/flow-raise-window
chmod +x Flow.alfredworkflow/bin/find-flow-alfred \
  Flow.alfredworkflow/bin/open-project \
  Flow.alfredworkflow/bin/frs-write-doc \
  "$HOME/.cargo/bin/flow-windows" \
  "$HOME/.cargo/bin/flow-raise-window" \
  Flow.alfredworkflow/bin/flow-windows \
  Flow.alfredworkflow/bin/flow-raise-window

if command -v codesign >/dev/null 2>/dev/null; then
  codesign --force --sign - \
    "$HOME/.cargo/bin/flow-windows" \
    "$HOME/.cargo/bin/flow-raise-window" \
    Flow.alfredworkflow/bin/flow-windows \
    Flow.alfredworkflow/bin/flow-raise-window >/dev/null 2>/dev/null || true
fi

echo ""
echo "Done! Commands available:"
echo ""
echo "  # Link workflow for development (creates symlink)"
echo "  flow-alfred link"
echo ""
echo "  # Or pack and install"
echo "  flow-alfred pack"
echo "  flow-alfred install Flow-Workflow.alfredworkflow"
echo ""
