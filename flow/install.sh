#!/bin/bash
set -e

cd "$(dirname "$0")"

echo "Building flow-alfred..."
cargo build --release

echo "Installing to ~/.cargo/bin..."
cargo install --path .

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
