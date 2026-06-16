#!/usr/bin/env bash
#
# Fetches the latest ai.robots.txt list from GitHub and updates the
# ROBOTS_NO_LLMS const in src/cmd/seo.rs.
#
# Usage: ./scripts/update-robots-presets.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
SEO_FILE="$REPO_ROOT/src/cmd/seo.rs"
UPSTREAM_URL="https://raw.githubusercontent.com/ai-robots-txt/ai.robots.txt/main/robots.txt"

echo "Fetching ai.robots.txt from GitHub..."
ROBOTS_RAW=$(curl -fsSL "$UPSTREAM_URL")

# Extract only User-agent and Disallow lines
ROBOTS_FILTERED=$(echo "$ROBOTS_RAW" | grep -E '^User-agent:|^Disallow:')

# Find line numbers for the const block
START_LINE=$(grep -n '^const ROBOTS_NO_LLMS: &str = r"' "$SEO_FILE" | head -1 | cut -d: -f1)
# Find the closing line (contains just ");
END_LINE=$(awk "NR>=$START_LINE && /\";\$/{print NR; exit}" "$SEO_FILE")

if [ -z "$START_LINE" ] || [ -z "$END_LINE" ]; then
    echo "Error: Could not find ROBOTS_NO_LLMS const in $SEO_FILE"
    exit 1
fi

# Build new file
TEMP_FILE=$(mktemp)

# Lines before the const (1 to START_LINE-1)
if [ "$START_LINE" -gt 1 ]; then
    head -n $((START_LINE - 1)) "$SEO_FILE" > "$TEMP_FILE"
fi

# The new const
echo "const ROBOTS_NO_LLMS: &str = r\"$ROBOTS_FILTERED\";" >> "$TEMP_FILE"

# Lines after the const (END_LINE+1 to end)
TOTAL_LINES=$(wc -l < "$SEO_FILE")
if [ "$END_LINE" -lt "$TOTAL_LINES" ]; then
    tail -n +$((END_LINE + 1)) "$SEO_FILE" >> "$TEMP_FILE"
fi

mv "$TEMP_FILE" "$SEO_FILE"

echo "Done. Updated ROBOTS_NO_LLMS const in $SEO_FILE (lines $START_LINE-$END_LINE)"
