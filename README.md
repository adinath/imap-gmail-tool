# imap-gmail-tool

A native Rust MCP server that lets AI agents (ironclaw, Claude Desktop, Cursor, etc.) read, search, send, and delete Gmail via IMAP/SMTP — no OAuth2 required.

## Why this exists

ironclaw's WASM sandbox blocks raw TCP sockets, and Gmail's OAuth2 flow is a 20-step nightmare for personal tooling. This tool sidesteps both problems: it's a native binary that speaks the [Model Context Protocol](https://modelcontextprotocol.io) over stdio, registered with ironclaw as an external MCP server.

## Prerequisites

- **Rust** (stable) — [install](https://rustup.rs)
- **ironclaw** CLI — or any other MCP-compatible client (Claude Desktop, Cursor, etc.)
- **macOS** — the install script handles macOS 15+ Gatekeeper signing; Linux users can adapt `scripts/install.sh` by dropping the `codesign` step
- A **Gmail account** with 2-Step Verification enabled

## Gmail App Password setup

Gmail App Passwords are a one-time setup. They're more convenient than OAuth2 and equally secure for personal use.

1. Go to your [Google Account Security settings](https://myaccount.google.com/security)
2. Under "How you sign in to Google", enable **2-Step Verification** if not already on
3. Search for **"App passwords"** (it only appears once 2SV is active)
4. Create a new app password — name it anything (e.g., "imap-gmail-tool")
5. Copy the 16-character password shown as `xxxx xxxx xxxx xxxx`

> Detailed walkthrough with screenshots: [Reddit guide](https://www.reddit.com/r/myclaw/comments/1qz0io5/how_i_connected_openclaw_to_gmail_beginner_step/)

## Installation

Clone the repo and run the install script with your credentials:

```bash
git clone https://github.com/yourname/imap-gmail-tool
cd imap-gmail-tool

GMAIL_ADDRESS=you@gmail.com \
GMAIL_APP_PASSWORD="xxxx xxxx xxxx xxxx" \
./scripts/install.sh
```

The script will:
1. Build the release binary (`cargo build --release`)
2. Install it to `~/.local/bin/imap-gmail-tool`
3. Ad-hoc sign it (required on macOS 15+ Sequoia)
4. Register it as an MCP server in ironclaw (`gmail-imap`)
5. Run a connection test

On success you'll see:
```
✓ Registered MCP server 'gmail-imap' with ironclaw.
✓ Connection test passed.
Done. Try asking ironclaw: 'list my last 5 emails'
```

### Manual registration (non-ironclaw MCP clients)

If you're using Claude Desktop, Cursor, or another MCP client, add the server to your client's config:

```json
{
  "mcpServers": {
    "gmail-imap": {
      "command": "/Users/you/.local/bin/imap-gmail-tool",
      "env": {
        "GMAIL_ADDRESS": "you@gmail.com",
        "GMAIL_APP_PASSWORD": "xxxxxxxxxxxxxxxx"
      }
    }
  }
}
```

## Available tools

| Tool | Description |
|------|-------------|
| `list_emails` | List recent emails in a folder |
| `read_email` | Read full content of an email by UID |
| `search_emails` | Search using IMAP query syntax |
| `send_email` | Send an email via Gmail SMTP |
| `delete_email` | Permanently delete an email by UID |

### Tool reference

#### `list_emails`
```
folder  (optional) — IMAP folder name, default: "INBOX"
limit   (optional) — number of emails to return, 1–100, default: 10
```
Returns: array of `{ uid, from, subject, date, seen }`

#### `read_email`
```
uid     (required) — IMAP UID of the email
folder  (optional) — IMAP folder name, default: "INBOX"
```
Returns: `{ uid, from, to, subject, date, body }` with decoded plain-text body

#### `search_emails`
```
query   (required) — IMAP search string (see examples below)
folder  (optional) — IMAP folder to search, default: "INBOX"
```

IMAP search query examples:
```
UNSEEN                          — unread emails
FROM "boss@company.com"         — from a specific sender
SUBJECT "invoice"               — subject contains word
SINCE 1-Jan-2025                — emails after a date
BEFORE 1-Mar-2025 UNSEEN        — unread emails before a date
```

Returns: array of `{ uid, from, subject, date, seen }`

#### `send_email`
```
to      (required) — recipient email address
subject (required) — email subject line
body    (required) — plain-text email body
cc      (optional) — CC recipient email address
```

#### `delete_email`
```
uid     (required) — IMAP UID of the email to delete
folder  (optional) — IMAP folder, default: "INBOX"
```

> **Warning:** deletion is permanent (flags as `\Deleted` then EXPUNGEs). There is no undo.

## Usage examples

Once registered, just talk to your AI agent naturally:

```
list my last 5 emails
read email 12345
search for unread emails from support@github.com
send an email to alice@example.com saying the meeting is at 3pm
delete email 12345
```

## Architecture

```
ironclaw / Claude Desktop / Cursor
        │  stdin/stdout (JSON-RPC, MCP protocol)
        ▼
imap-gmail-tool  (native Rust binary)
  ├── mcp.rs    — JSON-RPC server, tool dispatch
  ├── imap.rs   — list / read / search / delete (TLS port 993)
  ├── smtp.rs   — send (STARTTLS port 587)
  └── error.rs  — unified error handling
        │  IMAP + SMTP over TLS
        ▼
Gmail  (imap.gmail.com / smtp.gmail.com)
```

The binary opens a fresh TLS connection per operation — no persistent connection, no background process between calls. Simple and correct for personal use.

See [docs/adrs/001-mcp-server-architecture.md](docs/adrs/001-mcp-server-architecture.md) for the full rationale behind this design.

## Environment variables

| Variable | Required | Description |
|----------|----------|-------------|
| `GMAIL_ADDRESS` | Yes | Your Gmail address |
| `GMAIL_APP_PASSWORD` | Yes | 16-char App Password (spaces are stripped automatically) |

## Troubleshooting

**"GMAIL_ADDRESS is not set"** — pass credentials to the install script or set them in your MCP client config.

**Connection test fails** — double-check your App Password. Make sure 2-Step Verification is on and IMAP is enabled in Gmail settings (Settings → See all settings → Forwarding and POP/IMAP → Enable IMAP).

**macOS: binary killed immediately** — the install script handles this via `codesign --sign -`. If you built manually, run:
```bash
xattr -cr ~/.local/bin/imap-gmail-tool
codesign --sign - --force ~/.local/bin/imap-gmail-tool
```

**"Folder not found"** — Gmail IMAP uses labels as folders. Common names: `INBOX`, `[Gmail]/Sent Mail`, `[Gmail]/Drafts`, `[Gmail]/Trash`, `[Gmail]/All Mail`.

## Scripts

| Script | Purpose |
|--------|---------|
| `scripts/install.sh` | Build, install, sign, and register with ironclaw |
| `scripts/fresh-start.sh` | Wipe ironclaw conversation history (keeps memory and secrets) |
