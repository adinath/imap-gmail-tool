use imap::Session;
use mailparse::MailHeaderMap;
use native_tls::TlsStream;
use serde::Serialize;
use std::net::TcpStream;

use crate::error::{GmailError, Result};

// Type alias to avoid writing this mouthful everywhere
type ImapSession = Session<TlsStream<TcpStream>>;

/// Credentials loaded from environment variables.
/// Passed by reference into every operation so we don't clone config unnecessarily.
pub struct Config {
    pub address: String,
    pub app_password: String,
}

/// Compact email summary returned by list_emails and search_emails.
/// We don't include the body here — that would be expensive for large mailboxes.
#[derive(Serialize)]
pub struct EmailSummary {
    pub uid: u32,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub seen: bool,
}

/// Full email content returned by read_email.
#[derive(Serialize)]
pub struct EmailFull {
    pub uid: u32,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub date: String,
    pub body: String,
}

// ── Connection ────────────────────────────────────────────────────────────────

/// Open a fresh TLS connection to Gmail IMAP and log in.
/// We call this at the start of every operation. Simple, no connection pooling needed.
fn connect(config: &Config) -> Result<ImapSession> {
    let tls = native_tls::TlsConnector::new()?;
    let client = imap::connect(("imap.gmail.com", 993), "imap.gmail.com", &tls)?;

    // login() returns Err((error, client)) on failure — we only care about the error
    let session = client
        .login(&config.address, &config.app_password)
        .map_err(|(e, _)| e)?;

    Ok(session)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract a human-readable email address string from an IMAP Address struct.
/// Returns "unknown" rather than blowing up on missing fields.
fn format_address(addr: &imap_proto::types::Address) -> String {
    let mailbox = addr
        .mailbox
        .as_deref()
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("");
    let host = addr
        .host
        .as_deref()
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("");

    if mailbox.is_empty() && host.is_empty() {
        "unknown".to_string()
    } else {
        format!("{}@{}", mailbox, host)
    }
}

/// Convert raw IMAP bytes to a UTF-8 string, falling back to lossy conversion.
/// Subjects and dates can come back as raw bytes from the IMAP server.
fn bytes_to_string(bytes: &[u8]) -> String {
    std::str::from_utf8(bytes)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned())
}

// ── Operations ────────────────────────────────────────────────────────────────

/// List the most recent `limit` emails in `folder`, newest first.
///
/// We fetch only UIDs, flags, and envelope (headers) — NOT bodies.
/// This keeps it fast even for large mailboxes.
pub fn list_emails(config: &Config, folder: &str, limit: usize) -> Result<Vec<EmailSummary>> {
    let mut session = connect(config)?;

    let mailbox = session.select(folder).map_err(|_| {
        GmailError::FolderNotFound(folder.to_string())
    })?;

    let total = mailbox.exists as usize;
    if total == 0 {
        let _ = session.logout();
        return Ok(vec![]);
    }

    // IMAP sequence numbers are 1-based. We want the last `limit` messages.
    let start = total.saturating_sub(limit) + 1;
    let seq_range = format!("{}:{}", start, total);

    let fetches = session.fetch(&seq_range, "(UID FLAGS ENVELOPE)")?;

    // Build summaries, newest first
    let mut summaries: Vec<EmailSummary> = fetches
        .iter()
        .filter_map(|fetch| {
            let uid = fetch.uid?;
            let envelope = fetch.envelope()?;

            let subject = envelope
                .subject
                .as_deref()
                .map(bytes_to_string)
                .unwrap_or_else(|| "(no subject)".to_string());

            let from = envelope
                .from
                .as_ref()
                .and_then(|addrs| addrs.first())
                .map(format_address)
                .unwrap_or_else(|| "unknown".to_string());

            let date = envelope
                .date
                .as_deref()
                .map(bytes_to_string)
                .unwrap_or_else(|| "unknown".to_string());

            let seen = fetch
                .flags()
                .iter()
                .any(|f| matches!(f, imap::types::Flag::Seen));

            Some(EmailSummary { uid, from, subject, date, seen })
        })
        .collect();

    // Reverse so newest is first — IMAP returns oldest-first by sequence number
    summaries.reverse();

    let _ = session.logout();
    Ok(summaries)
}

/// Read the full content of a single email identified by its UID.
///
/// We fetch the raw RFC822 bytes and parse them with mailparse,
/// which handles MIME, multipart, and various encodings correctly.
pub fn read_email(config: &Config, uid: u32, folder: &str) -> Result<EmailFull> {
    let mut session = connect(config)?;

    session.select(folder).map_err(|_| {
        GmailError::FolderNotFound(folder.to_string())
    })?;

    let fetches = session.uid_fetch(uid.to_string(), "RFC822")?;
    let fetch = fetches
        .iter()
        .next()
        .ok_or_else(|| GmailError::NotFound(uid, folder.to_string()))?;

    let raw = fetch
        .body()
        .ok_or_else(|| GmailError::NotFound(uid, folder.to_string()))?;

    let parsed = mailparse::parse_mail(raw)?;

    let subject = parsed
        .headers
        .get_first_value("Subject")
        .unwrap_or_else(|| "(no subject)".to_string());

    let from = parsed
        .headers
        .get_first_value("From")
        .unwrap_or_else(|| "unknown".to_string());

    let to = parsed
        .headers
        .get_first_value("To")
        .unwrap_or_else(|| "unknown".to_string());

    let date = parsed
        .headers
        .get_first_value("Date")
        .unwrap_or_else(|| "unknown".to_string());

    // Extract plain-text body. For multipart emails, walk the parts.
    let body = extract_text_body(&parsed);

    let _ = session.logout();
    Ok(EmailFull { uid, from, to, subject, date, body })
}

/// Walk MIME parts to find the first text/plain part.
/// Falls back to text/html if no plain text exists.
/// Falls back to "(no body)" if the email has no readable text at all.
fn extract_text_body(mail: &mailparse::ParsedMail) -> String {
    // Simple case: not multipart
    if mail.subparts.is_empty() {
        return mail.get_body().unwrap_or_else(|_| "(no body)".to_string());
    }

    // Prefer text/plain
    for part in &mail.subparts {
        let ct = part.ctype.mimetype.to_lowercase();
        if ct == "text/plain" {
            if let Ok(body) = part.get_body() {
                if !body.trim().is_empty() {
                    return body;
                }
            }
        }
    }

    // Fall back to text/html
    for part in &mail.subparts {
        let ct = part.ctype.mimetype.to_lowercase();
        if ct == "text/html" {
            if let Ok(body) = part.get_body() {
                if !body.trim().is_empty() {
                    return body;
                }
            }
        }
    }

    "(no body)".to_string()
}

/// Search emails using an IMAP search query string.
///
/// Examples of valid queries:
///   "SUBJECT \"invoice\""
///   "FROM \"boss@company.com\""
///   "UNSEEN"
///   "SINCE 1-Jan-2025"
///   "SUBJECT \"meeting\" FROM \"colleague@example.com\""
///
/// Returns summaries (not full bodies) of matching messages.
pub fn search_emails(config: &Config, query: &str, folder: &str) -> Result<Vec<EmailSummary>> {
    let mut session = connect(config)?;

    session.select(folder).map_err(|_| {
        GmailError::FolderNotFound(folder.to_string())
    })?;

    let uids = session.uid_search(query)?;

    if uids.is_empty() {
        let _ = session.logout();
        return Ok(vec![]);
    }

    // Fetch envelopes for all matching UIDs in one round-trip
    let uid_set: Vec<String> = uids.iter().map(|u| u.to_string()).collect();
    let fetches = session.uid_fetch(uid_set.join(","), "(UID FLAGS ENVELOPE)")?;

    let mut summaries: Vec<EmailSummary> = fetches
        .iter()
        .filter_map(|fetch| {
            let uid = fetch.uid?;
            let envelope = fetch.envelope()?;

            let subject = envelope
                .subject
                .as_deref()
                .map(bytes_to_string)
                .unwrap_or_else(|| "(no subject)".to_string());

            let from = envelope
                .from
                .as_ref()
                .and_then(|addrs| addrs.first())
                .map(format_address)
                .unwrap_or_else(|| "unknown".to_string());

            let date = envelope
                .date
                .as_deref()
                .map(bytes_to_string)
                .unwrap_or_else(|| "unknown".to_string());

            let seen = fetch
                .flags()
                .iter()
                .any(|f| matches!(f, imap::types::Flag::Seen));

            Some(EmailSummary { uid, from, subject, date, seen })
        })
        .collect();

    // Newest first
    summaries.sort_by(|a, b| b.uid.cmp(&a.uid));

    let _ = session.logout();
    Ok(summaries)
}

/// Permanently delete an email by UID.
///
/// IMAP deletion is a two-step process:
///   1. Mark the message with the \Deleted flag
///   2. EXPUNGE to physically remove all \Deleted messages from the folder
///
/// Warning: expunge removes ALL \Deleted messages in the folder, not just this one.
/// For a personal tool this is fine — if you need surgical precision, use Gmail's Trash instead.
pub fn delete_email(config: &Config, uid: u32, folder: &str) -> Result<()> {
    let mut session = connect(config)?;

    session.select(folder).map_err(|_| {
        GmailError::FolderNotFound(folder.to_string())
    })?;

    // Step 1: flag as deleted
    session.uid_store(uid.to_string(), "+FLAGS (\\Deleted)")?;

    // Step 2: permanently remove all \Deleted messages
    session.expunge()?;

    let _ = session.logout();
    Ok(())
}
