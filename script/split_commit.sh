#!/usr/bin/env bash
set -uo pipefail
cd /Users/apothecary/Projects/petunia
LOG=/tmp/split_log.txt

subj_full=$(git log -1 --format=%s HEAD)
# strip any existing "type(scope): " or "type: " prefix to recover original subject
subj=$(echo "$subj_full" | sed -E 's/^[a-z]+(\([a-zA-Z0-9_.-]+\))?: //')

git reset HEAD~1 >/dev/null
git reset >/dev/null

files=$(git status --porcelain | awk '{print $2}')
echo "=== splitting: $subj_full ($(echo "$files" | wc -l) files) ===" >> "$LOG"

groups=$(echo "$files" | xargs -n1 dirname | sort -u)

while IFS= read -r g; do
  [ -z "$g" ] && continue
  gfiles=$(echo "$files" | awk -v d="$g" 'index($0, d"/")==1 || $0==d')
  [ -z "$gfiles" ] && continue

  type="feat"
  low=$(echo "$subj" | tr 'A-Z' 'a-z')
  case "$low" in fix\ *|*" fix "*|*bug*) type="fix";; esac
  case "$g" in *Cargo.toml|*Cargo.lock|*flake.nix|.) type="chore";; esac
  case "$g" in *.md) type="docs";; esac

  scope=$(echo "$g" | sed -E 's#^crates/##; s#^src/##; s#/#-#g')
  if [ "$g" = "." ]; then msg="$type: $subj"; else msg="$type($scope): $subj"; fi

  echo "$gfiles" | tr '\n' '\0' | xargs -0 git add --
  git commit -m "$msg" >>"$LOG" 2>&1
done <<< "$groups"

if [ -n "$(git status --porcelain)" ]; then
  git add -A
  git commit -m "feat: $subj" >>"$LOG" 2>&1
fi
