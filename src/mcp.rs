/// MCP (Model Context Protocol) server over stdio.
///
/// Protocol flow:
///   1. ironclaw launches this binary as a subprocess
///   2. ironclaw sends JSON-RPC requests on our stdin, one per line
///   3. We write JSON-RPC responses to stdout, one per line
///   4. stderr is for our own logging — ironclaw ignores it
///
/// We handle three methods:
///   - initialize    → handshake, tell ironclaw what we support
///   - tools/list    → return the list of tools we expose
///   - tools/call    → execute a tool and return the result
use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::imap::{self, Config};
use crate::smtp;

// ── JSON-RPC wire types ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Request {
    // We don't validate the jsonrpc version — if ironclaw sends it, we trust it
    #[allow(dead_code)]
    jsonrpc: String,
    // id is absent for notifications (e.g. notifications/initialized)
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Serialize)]
struct Response {
    jsonrpc: String,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

impl Response {
    fn ok(id: Option<Value>, result: Value) -> Self {
        Self { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
    }

    fn err(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(RpcError { code, message: message.into() }),
        }
    }
}

// ── Tool response helpers ─────────────────────────────────────────────────────

/// Wrap a successful string result in MCP's content envelope.
fn tool_ok(text: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": text.into() }] })
}

/// Wrap an error in MCP's content envelope with isError=true.
/// We use this for tool-level errors (bad input, Gmail unreachable, etc.)
/// rather than JSON-RPC errors — per MCP spec, protocol errors vs tool errors are distinct.
fn tool_err(msg: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": msg.into() }],
        "isError": true
    })
}

// ── Tool definitions ──────────────────────────────────────────────────────────

/// The full list of tools we advertise to ironclaw.
/// Each tool has a name, description, and a JSON Schema for its inputs.
fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "list_emails",
                "description": "List recent emails in a Gmail folder. Returns uid, from, subject, date, and seen status. Does NOT fetch email bodies (use read_email for that).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "folder": {
                            "type": "string",
                            "description": "IMAP folder name. Defaults to INBOX. Common values: INBOX, [Gmail]/Sent Mail, [Gmail]/Drafts, [Gmail]/All Mail, [Gmail]/Trash."
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of emails to return, most recent first. Defaults to 10, max 100.",
                            "default": 10,
                            "minimum": 1,
                            "maximum": 100
                        }
                    }
                }
            },
            {
                "name": "read_email",
                "description": "Read the full content of a single email by its UID. Returns from, to, subject, date, and decoded plain-text body.",
                "inputSchema": {
                    "type": "object",
                    "required": ["uid"],
                    "properties": {
                        "uid": {
                            "type": "integer",
                            "description": "The IMAP UID of the email (from list_emails or search_emails)."
                        },
                        "folder": {
                            "type": "string",
                            "description": "IMAP folder containing the email. Defaults to INBOX."
                        }
                    }
                }
            },
            {
                "name": "search_emails",
                "description": "Search emails using an IMAP search query. Returns matching email summaries (uid, from, subject, date, seen). Examples: SUBJECT \"invoice\", FROM \"boss@company.com\", UNSEEN, SINCE 1-Jan-2025.",
                "inputSchema": {
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "IMAP search query string. Supports standard IMAP search criteria: SUBJECT, FROM, TO, SINCE, BEFORE, UNSEEN, SEEN, ALL, etc."
                        },
                        "folder": {
                            "type": "string",
                            "description": "IMAP folder to search in. Defaults to INBOX."
                        }
                    }
                }
            },
            {
                "name": "send_email",
                "description": "Send an email via Gmail SMTP. The email is sent from the configured Gmail account.",
                "inputSchema": {
                    "type": "object",
                    "required": ["to", "subject", "body"],
                    "properties": {
                        "to": {
                            "type": "string",
                            "description": "Recipient email address, e.g. someone@example.com"
                        },
                        "subject": {
                            "type": "string",
                            "description": "Email subject line."
                        },
                        "body": {
                            "type": "string",
                            "description": "Plain-text email body."
                        },
                        "cc": {
                            "type": "string",
                            "description": "Optional CC recipient email address."
                        }
                    }
                }
            },
            {
                "name": "delete_email",
                "description": "Permanently delete an email by UID. WARNING: this is irreversible — the email is expunged from the folder immediately.",
                "inputSchema": {
                    "type": "object",
                    "required": ["uid"],
                    "properties": {
                        "uid": {
                            "type": "integer",
                            "description": "The IMAP UID of the email to delete."
                        },
                        "folder": {
                            "type": "string",
                            "description": "IMAP folder containing the email. Defaults to INBOX."
                        }
                    }
                }
            }
        ]
    })
}

// ── Tool dispatch ─────────────────────────────────────────────────────────────

fn dispatch(name: &str, args: &Value, config: &Config) -> Value {
    match name {
        "list_emails" => {
            let folder = args["folder"].as_str().unwrap_or("INBOX");
            let limit = args["limit"].as_u64().unwrap_or(10).min(100) as usize;

            match imap::list_emails(config, folder, limit) {
                Ok(emails) => tool_ok(serde_json::to_string_pretty(&emails).unwrap()),
                Err(e) => tool_err(format!("list_emails failed: {e}")),
            }
        }

        "read_email" => {
            let uid = match args["uid"].as_u64() {
                Some(u) => u as u32,
                None => return tool_err("'uid' is required and must be an integer"),
            };
            let folder = args["folder"].as_str().unwrap_or("INBOX");

            match imap::read_email(config, uid, folder) {
                Ok(email) => tool_ok(serde_json::to_string_pretty(&email).unwrap()),
                Err(e) => tool_err(format!("read_email failed: {e}")),
            }
        }

        "search_emails" => {
            let query = match args["query"].as_str() {
                Some(q) => q,
                None => return tool_err("'query' is required"),
            };
            let folder = args["folder"].as_str().unwrap_or("INBOX");

            match imap::search_emails(config, query, folder) {
                Ok(emails) => tool_ok(serde_json::to_string_pretty(&emails).unwrap()),
                Err(e) => tool_err(format!("search_emails failed: {e}")),
            }
        }

        "send_email" => {
            let to = match args["to"].as_str() {
                Some(t) => t,
                None => return tool_err("'to' is required"),
            };
            let subject = match args["subject"].as_str() {
                Some(s) => s,
                None => return tool_err("'subject' is required"),
            };
            let body = match args["body"].as_str() {
                Some(b) => b,
                None => return tool_err("'body' is required"),
            };
            let cc = args["cc"].as_str();

            match smtp::send_email(config, to, subject, body, cc) {
                Ok(()) => tool_ok(format!("Email sent successfully to {to}")),
                Err(e) => tool_err(format!("send_email failed: {e}")),
            }
        }

        "delete_email" => {
            let uid = match args["uid"].as_u64() {
                Some(u) => u as u32,
                None => return tool_err("'uid' is required and must be an integer"),
            };
            let folder = args["folder"].as_str().unwrap_or("INBOX");

            match imap::delete_email(config, uid, folder) {
                Ok(()) => tool_ok(format!("Email uid={uid} deleted from '{folder}'")),
                Err(e) => tool_err(format!("delete_email failed: {e}")),
            }
        }

        unknown => tool_err(format!("Unknown tool: '{unknown}'")),
    }
}

// ── Server loop ───────────────────────────────────────────────────────────────

/// Run the MCP server, reading from `input` and writing to `output`.
///
/// We take generic I/O so this is testable without actual stdin/stdout.
/// ironclaw will launch us and use our stdio pipes directly.
pub fn run(config: Config, input: impl BufRead, mut output: impl Write) {
    for line in input.lines() {
        let line = match line {
            Ok(l) if l.trim().is_empty() => continue, // skip blank lines
            Ok(l) => l,
            Err(e) => {
                eprintln!("[imap-gmail-tool] stdin read error: {e}");
                break;
            }
        };

        let response = handle_line(&line, &config);

        // Notifications (no id) don't get a response
        if let Some(resp) = response {
            let json = serde_json::to_string(&resp).unwrap();
            if let Err(e) = writeln!(output, "{json}") {
                eprintln!("[imap-gmail-tool] stdout write error: {e}");
                break;
            }
            // Flush after every response — ironclaw is waiting on the other end
            let _ = output.flush();
        }
    }
}

fn handle_line(line: &str, config: &Config) -> Option<Response> {
    let req: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            // Can't parse → send a parse error, id is unknown so use null
            return Some(Response::err(None, -32700, format!("Parse error: {e}")));
        }
    };

    // Notifications have no id and expect no response
    if req.id.is_none() && req.method.starts_with("notifications/") {
        eprintln!("[imap-gmail-tool] notification: {}", req.method);
        return None;
    }

    let id = req.id.clone();

    let result = match req.method.as_str() {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "imap-gmail-tool",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),

        "tools/list" => tool_definitions(),

        "tools/call" => {
            let params = req.params.unwrap_or(Value::Null);
            let name = match params["name"].as_str() {
                Some(n) => n.to_string(),
                None => return Some(Response::err(id, -32602, "Missing 'name' in params")),
            };
            let args = params["arguments"].clone();
            dispatch(&name, &args, config)
        }

        // ping is optional but polite to support
        "ping" => json!({}),

        unknown => {
            return Some(Response::err(
                id,
                -32601,
                format!("Method not found: '{unknown}'"),
            ));
        }
    };

    Some(Response::ok(id, result))
}
