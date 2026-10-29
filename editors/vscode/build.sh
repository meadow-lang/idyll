#!/usr/bin/env bash
#
# Build `idyll-<version>.vsix`.
#
# Needs node and npm. The extension bundles `vscode-languageclient`, so
# `node_modules` has to be present when it is packaged. `vsce` is a pinned
# devDependency, run from `node_modules` rather than `npx`, which would fetch
# whatever is newest (as Meadow's extension's build explains).

set -euo pipefail
cd "$(dirname "$0")"

npm install --silent
rm -f ./*.vsix
node node_modules/@vscode/vsce/vsce package --allow-missing-repository --skip-license

echo
ls -1 ./*.vsix
