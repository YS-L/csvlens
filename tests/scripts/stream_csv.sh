#!/usr/bin/env bash
set -euo pipefail

# Stream CSV data in a "tricky" way for testing viewers
# - Not line buffered (outputs partial lines)
# - Variable chunk sizes
# - Configurable speed

show_usage() {
  cat >&2 <<EOF
Usage: $0 INPUT_CSV [OPTIONS]

Stream CSV data with configurable speed and tricky buffering.

Options:
  -s, --speed SPEED        Delay between chunks in seconds (default: 0.1)
  -c, --chunk-size SIZE    Base chunk size in bytes (default: random 1-50)
                          Use 'random' for variable chunks, or a number
  -l, --line-buffered      Stream complete lines instead of arbitrary chunks
  -h, --help              Show this help message

Examples:
  $0 data.csv
  $0 data.csv -s 0.05
  $0 data.csv -s 0.2 -c 10
  $0 data.csv -c random
  $0 data.csv -l           # Line buffered mode
EOF
  exit 1
}

SPEED=0.1
CHUNK_MODE="random"
CHUNK_SIZE=0
LINE_BUFFERED=false

INPUT=""
while [[ $# -gt 0 ]]; do
  case $1 in
    -s|--speed)
      SPEED="$2"
      shift 2
      ;;
    -c|--chunk-size)
      if [[ "$2" == "random" ]]; then
        CHUNK_MODE="random"
      else
        CHUNK_MODE="fixed"
        CHUNK_SIZE="$2"
      fi
      shift 2
      ;;
    -l|--line-buffered)
      LINE_BUFFERED=true
      shift
      ;;
    -h|--help)
      show_usage
      ;;
    *)
      if [[ -z "$INPUT" ]]; then
        INPUT="$1"
      else
        echo "Error: Unexpected argument '$1'" >&2
        show_usage
      fi
      shift
      ;;
  esac
done

if [[ -z "$INPUT" ]]; then
  echo "Error: INPUT_CSV is required" >&2
  show_usage
fi

if [[ ! -f "$INPUT" ]]; then
  echo "Error: Input file not found: $INPUT" >&2
  exit 1
fi

if ! [[ "$SPEED" =~ ^[0-9]+\.?[0-9]*$ ]]; then
  echo "Error: Speed must be a number" >&2
  exit 1
fi

get_chunk_size() {
  if [[ "$CHUNK_MODE" == "random" ]]; then
    echo $((RANDOM % 50 + 1))
  else
    echo "$CHUNK_SIZE"
  fi
}

# Use perl for sub-second sleep (works on both Linux and macOS)
do_sleep() {
  perl -e "select(undef, undef, undef, $1)"
}

if [[ "$LINE_BUFFERED" == "true" ]]; then
  echo "[stream_csv] Starting: $INPUT (speed=${SPEED}s, line-buffered mode)" >&2

  # Stream line by line
  while IFS= read -r line; do
    printf '%s\n' "$line"
    if (( $(echo "$SPEED > 0" | bc -l) )); then
      do_sleep "$SPEED"
    fi
  done < "$INPUT"
else
  echo "[stream_csv] Starting: $INPUT (speed=${SPEED}s, chunks=$CHUNK_MODE)" >&2

  FILE_CONTENT=$(cat "$INPUT")
  FILE_SIZE=${#FILE_CONTENT}
  POSITION=0

  # Stream content in chunks, potentially breaking lines mid-way
  while [[ $POSITION -lt $FILE_SIZE ]]; do
    CHUNK_SIZE=$(get_chunk_size)
    CHUNK="${FILE_CONTENT:$POSITION:$CHUNK_SIZE}"
    printf '%s' "$CHUNK"
    POSITION=$((POSITION + CHUNK_SIZE))

    if (( $(echo "$SPEED > 0" | bc -l) )); then
      do_sleep "$SPEED"
    fi
  done
fi

echo "[stream_csv] Complete" >&2
