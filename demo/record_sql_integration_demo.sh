#!/usr/bin/env bash
# Record pg_mentat SQL Integration demo with asciinema
set -e

OUTPUT_FILE="pg_mentat_sql_integration.cast"

echo "=============================================="
echo "Recording pg_mentat SQL Integration Demo"
echo "=============================================="
echo
echo "This will record the demo to: $OUTPUT_FILE"
echo
echo "Press Ctrl+C to cancel, or Enter to start recording..."
read

# Record the demo
asciinema rec \
  --title "pg_mentat SQL Integration - Native PostgreSQL Feel" \
  --overwrite \
  --idle-time-limit 3 \
  --command "./demo_sql_integration.sh" \
  "$OUTPUT_FILE"

echo
echo "=============================================="
echo "Recording complete!"
echo "=============================================="
echo
echo "File: $OUTPUT_FILE"
echo
echo "To play back:"
echo "  asciinema play $OUTPUT_FILE"
echo
echo "To upload to asciinema.org:"
echo "  asciinema upload $OUTPUT_FILE"
echo
echo "To embed in README:"
echo "  [![asciicast](https://asciinema.org/a/XXXXX.svg)](https://asciinema.org/a/XXXXX)"
echo
