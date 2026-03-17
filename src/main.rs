mod error;
mod imap;
mod mcp;
mod smtp;

use std::io;

fn main() {
    // Load credentials from environment variables.
    // We fail fast at startup — better to crash immediately with a clear message
    // than to silently fail on the first tool call.
    let address = std::env::var("GMAIL_ADDRESS").unwrap_or_else(|_| {
        eprintln!("Error: GMAIL_ADDRESS environment variable is not set.");
        eprintln!("Set it to your Gmail address, e.g.: export GMAIL_ADDRESS=you@gmail.com");
        std::process::exit(1);
    });

    let app_password = std::env::var("GMAIL_APP_PASSWORD").unwrap_or_else(|_| {
        eprintln!("Error: GMAIL_APP_PASSWORD environment variable is not set.");
        eprintln!("Set it to your 16-character Gmail App Password (spaces are fine).");
        eprintln!("See: https://myaccount.google.com/apppasswords");
        std::process::exit(1);
    });

    // Strip spaces from app password — Gmail shows it as "xxxx xxxx xxxx xxxx"
    // but IMAP auth requires it without spaces
    let app_password = app_password.replace(' ', "");

    let config = imap::Config { address, app_password };

    eprintln!(
        "[imap-gmail-tool] v{} started for {}",
        env!("CARGO_PKG_VERSION"),
        config.address
    );

    // Hand off to the MCP server loop.
    // It reads JSON-RPC requests from stdin and writes responses to stdout
    // until stdin is closed (i.e. ironclaw shuts us down).
    mcp::run(config, io::stdin().lock(), io::stdout().lock());

    eprintln!("[imap-gmail-tool] stdin closed, exiting.");
}
