# ADR 001 — MCP Server Architecture for Gmail IMAP Tool

**Date:** 2026-03-16
**Status:** Accepted

---

## Context

We need a tool that lets ironclaw AI agents read and manage Gmail. Three approaches were considered:

1. **WASM tool with Gmail REST API** — fits ironclaw's WASM sandbox, but requires OAuth2 setup which is notoriously painful for personal tools and previously blocked the developer.

2. **WASM tool with raw IMAP** — ironclaw's WASM sandbox explicitly blocks TCP sockets (only HTTP is allowed via `http_request` host function). Not viable.

3. **Native MCP server** — a native Rust binary that speaks the Model Context Protocol over stdio. ironclaw's MCP client integration allows any process to be registered as a tool provider. No sandbox restrictions.

## Decision

Build a **native Rust binary that implements an MCP stdio server**.

- IMAP operations (list, read, search, delete) use the `imap` crate with native TLS.
- Email sending uses `lettre` over SMTP (port 587, STARTTLS) — IMAP is read-only.
- Authentication uses Gmail App Passwords — simpler and equally secure for personal use.
- The binary is registered in ironclaw's MCP server config, launched on demand.

## Consequences

**Good:**
- No sandbox restrictions — full TCP access for IMAP and SMTP.
- App Password auth is 3 steps vs ~20 for OAuth2.
- MCP is a standard protocol — the tool works with Claude Desktop, Cursor, and any other MCP client, not just ironclaw.
- Synchronous Rust — no async runtime needed, simpler code.

**Trade-offs:**
- Requires a native binary on the machine (not portable like WASM).
- A fresh TLS connection is opened per tool call (no connection pooling). Acceptable for personal/infrequent use; revisit if latency becomes a problem.
- App Passwords require Gmail 2-Step Verification to be enabled.

## Alternatives Rejected

| Option | Reason rejected |
|--------|----------------|
| WASM + Gmail REST API | OAuth2 "Authorization blocked" errors; complex setup |
| WASM + raw IMAP | ironclaw WASM sandbox blocks TCP sockets |
| Compile into ironclaw | Requires forking ironclaw; unsustainable |
