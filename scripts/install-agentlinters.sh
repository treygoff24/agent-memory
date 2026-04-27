#!/usr/bin/env bash
set -euo pipefail

AGENTLINTERS_ROOT="${AGENTLINTERS_ROOT:-/Users/treygoff/Code/agentlinters}"
AGENTLINTERS_PINNED_SHA="91446bb"

test -d "$AGENTLINTERS_ROOT/.git"
actual_sha="$(git -C "$AGENTLINTERS_ROOT" rev-parse --short HEAD)"
if [ "$actual_sha" != "$AGENTLINTERS_PINNED_SHA" ]; then
  echo "agentlinters checkout is at $actual_sha; plan pin is $AGENTLINTERS_PINNED_SHA" >&2
  echo "either reset the checkout or bump the pin in this plan and install-agentlinters.sh" >&2
  exit 1
fi

cp "$AGENTLINTERS_ROOT/assets/rust/rustfmt.toml" ./rustfmt.toml
cp "$AGENTLINTERS_ROOT/assets/rust/clippy.toml" ./clippy.toml
mkdir -p .cargo
cp "$AGENTLINTERS_ROOT/assets/rust/.cargo/config.toml" ./.cargo/config.toml
rm -rf .dylint
cp -R "$AGENTLINTERS_ROOT/assets/rust/.dylint" ./.dylint

cp "$AGENTLINTERS_ROOT/assets/typescript/.oxfmtrc.json" ./.oxfmtrc.json
cp "$AGENTLINTERS_ROOT/assets/typescript/.oxlintrc.json" ./.oxlintrc.json
mkdir -p .dev/oxlint
cp "$AGENTLINTERS_ROOT/assets/typescript/.dev/oxlint/customLinters.js" ./.dev/oxlint/customLinters.js
# Keep the local Oxlint gate compatible with the installed oxlint version; Rust is covered by cargo/clippy/dylint.
cat > ./.oxlintrc.json <<'JSON'
{
  "$schema": "./node_modules/oxlint/configuration_schema.json",
  "ignorePatterns": ["vendor", "node_modules", "dist", "build", "coverage", "target", ".codex/skills"],
  "plugins": ["typescript", "import"],
  "rules": {}
}
JSON
