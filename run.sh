#!/usr/bin/env bash
# non-nix linux / mac launcher: makes a venv on first run, re-syncs packages when requirements.txt changes
cd "$(dirname "$0")"
[ -d .venv ] || python3 -m venv .venv
sum=$(sha1sum requirements.txt | cut -d' ' -f1)
if [ "$(cat .venv/.req 2>/dev/null)" != "$sum" ]; then
  .venv/bin/pip install -q -r requirements.txt && echo "$sum" > .venv/.req
fi
exec .venv/bin/python nexus.py "$@"
