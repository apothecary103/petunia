#!/usr/bin/env bash
set -uo pipefail
cd /Users/apothecary/Projects/petunia
export GIT_EDITOR=true
export GIT_SEQUENCE_EDITOR=/tmp/seq_editor.sh
DRIVE_LOG=/tmp/drive_log.txt
: > "$DRIVE_LOG"

git rebase -i 8ab3126 >>"$DRIVE_LOG" 2>&1

i=0
while [ -d .git/rebase-merge ]; do
  i=$((i+1))
  echo "--- stop $i, HEAD=$(git log -1 --format=%H) ---" >> "$DRIVE_LOG"
  bash /Users/apothecary/Projects/petunia/script/split_commit.sh >>"$DRIVE_LOG" 2>&1
  if ! git rebase --continue >>"$DRIVE_LOG" 2>&1; then
    echo "REBASE STOPPED / CONFLICT at stop $i" >> "$DRIVE_LOG"
    git status >> "$DRIVE_LOG"
    break
  fi
  if [ $i -gt 40 ]; then
    echo "SAFETY BREAK" >> "$DRIVE_LOG"
    break
  fi
done
echo "REBASE DRIVER DONE" >> "$DRIVE_LOG"
