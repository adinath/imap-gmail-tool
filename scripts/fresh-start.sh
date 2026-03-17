#!/usr/bin/env bash
# fresh-start.sh — wipe ironclaw's conversation and job history for a clean slate.
#
# Clears:  conversations, messages, jobs, tool failures, rate limits, routine runs
# Keeps:   memory docs (identity/persona), secrets (API keys), settings, tool registrations
#
# Usage: ./scripts/fresh-start.sh

set -euo pipefail

DB_URL="postgres://octopus:secret@localhost:5432/ironclaw"

echo "→ Clearing ironclaw history..."

psql "$DB_URL" -c "
TRUNCATE conversation_messages, conversations,
         agent_jobs, job_actions, job_events,
         repair_attempts, estimation_snapshots,
         tool_failures, tool_rate_limit_state,
         routine_runs, leak_detection_events
CASCADE;" 2>&1 | grep -v NOTICE

echo ""
echo "✓ Done. Current state:"

psql "$DB_URL" -t -c "
SELECT '  conversations : ' || COUNT(*) FROM conversations UNION ALL
SELECT '  messages      : ' || COUNT(*) FROM conversation_messages UNION ALL
SELECT '  jobs          : ' || COUNT(*) FROM agent_jobs UNION ALL
SELECT '  tool_failures : ' || COUNT(*) FROM tool_failures UNION ALL
SELECT '  memory docs   : ' || COUNT(*) || ' (kept)' FROM memory_documents UNION ALL
SELECT '  secrets       : ' || COUNT(*) || ' (kept)' FROM secrets;" 2>&1

echo ""
echo "ironclaw starts fresh. Identity and API keys are intact."
