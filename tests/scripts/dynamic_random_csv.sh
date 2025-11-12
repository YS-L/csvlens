#!/usr/bin/env bash
set -euo pipefail

# Dynamically rewrites OUTPUT_CSV every few seconds
# Each rewrite keeps the header and replaces the rows with a random sample

if [ "$#" -lt 2 ]; then
  echo "Usage: $0 INPUT_CSV OUTPUT_CSV [interval_seconds] [sample_size]" >&2
  exit 1
fi

INPUT="$1"
OUTPUT="$2"
INTERVAL="${3:-1}"
SAMPLE_SIZE="${4:-5}"

if [ ! -f "$INPUT" ]; then
  echo "Input file not found: $INPUT" >&2
  exit 1
fi

header=$(head -n 1 "$INPUT")

while :; do
  {
    echo "$header"
    # Shuffle lines using sort -R (works on macOS, Linux, BSD)
    tail -n +2 "$INPUT" | sort -R | head -n "$SAMPLE_SIZE"
  } > "$OUTPUT"

  echo "[dynamic_csv] $(date '+%H:%M:%S') → wrote $SAMPLE_SIZE random rows to $OUTPUT"

  sleep "$INTERVAL"
done
