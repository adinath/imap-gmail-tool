#!/usr/bin/env bash
# install.sh — build, install, and register imap-gmail-tool with ironclaw
#
# Usage:
#   GMAIL_ADDRESS=you@gmail.com GMAIL_APP_PASSWORD="xxxx xxxx xxxx xxxx" ./scripts/install.sh

set -euo pipefail

BINARY_NAME="imap-gmail-tool"
INSTALL_DIR="${HOME}/.local/bin"
MCP_SERVER_NAME="gmail-imap"

# ── Credentials ───────────────────────────────────────────────────────────────

if [[ -z "${GMAIL_ADDRESS:-}" ]]; then
  echo "Error: GMAIL_ADDRESS is not set."
  echo "Usage: GMAIL_ADDRESS=you@gmail.com GMAIL_APP_PASSWORD='xxxx xxxx xxxx xxxx' ./scripts/install.sh"
  exit 1
fi

if [[ -z "${GMAIL_APP_PASSWORD:-}" ]]; then
  echo "Error: GMAIL_APP_PASSWORD is not set."
  echo "Usage: GMAIL_ADDRESS=you@gmail.com GMAIL_APP_PASSWORD='xxxx xxxx xxxx xxxx' ./scripts/install.sh"
  exit 1
fi

# ── Build ─────────────────────────────────────────────────────────────────────

echo "→ Building ${BINARY_NAME} (release)..."
cargo build --release

# ── Install binary ────────────────────────────────────────────────────────────

echo "→ Installing to ${INSTALL_DIR}/${BINARY_NAME}..."
mkdir -p "${INSTALL_DIR}"
cp "target/release/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

echo "✓ Binary installed: ${INSTALL_DIR}/${BINARY_NAME}"

# macOS 15 (Sequoia) Gatekeeper rejects unsigned local binaries with SIGKILL.
# Ad-hoc signing marks it as a locally-trusted binary.
echo "→ Signing binary (required on macOS 15+)..."
xattr -cr "${INSTALL_DIR}/${BINARY_NAME}"
codesign --sign - --force "${INSTALL_DIR}/${BINARY_NAME}"
echo "✓ Binary signed."

# ── Register with ironclaw ────────────────────────────────────────────────────

echo "→ Registering '${MCP_SERVER_NAME}' MCP server with ironclaw..."

# Remove old registration if it exists (idempotent re-installs)
ironclaw mcp remove "${MCP_SERVER_NAME}" 2>/dev/null && echo "  (removed previous registration)" || true

ironclaw mcp add "${MCP_SERVER_NAME}" \
  --transport stdio \
  --command "${INSTALL_DIR}/${BINARY_NAME}" \
  --env "GMAIL_ADDRESS=${GMAIL_ADDRESS}" \
  --env "GMAIL_APP_PASSWORD=${GMAIL_APP_PASSWORD}" \
  --description "Read, search, send and delete Gmail via IMAP/SMTP"

echo ""
echo "✓ Registered MCP server '${MCP_SERVER_NAME}' with ironclaw."
echo ""
echo "→ Testing connection..."
ironclaw mcp test "${MCP_SERVER_NAME}" && echo "✓ Connection test passed." || echo "✗ Connection test failed — check credentials."
echo ""
echo "Done. Try asking ironclaw: 'list my last 5 emails'"
