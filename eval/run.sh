#!/usr/bin/env bash
# Launch the eval with the virtualenv on the internal SSD.
#
# Why: this project lives on an external drive where uv can't hardlink, so it
# copy-installs packages — and that breaks namespace packages like google.genai
# ("cannot import name 'types' ... unknown location"). Putting the venv on /tmp
# (internal SSD) avoids it. This also keeps a big .venv out of the project dir so
# the editor doesn't choke watching it.
#
# Usage:
#   export GEMINI_API_KEY=...        # from https://aistudio.google.com/apikey
#   ./run.sh
set -euo pipefail

if [[ -z "${GEMINI_API_KEY:-}" && -z "${GOOGLE_API_KEY:-}" ]]; then
  echo "Set GEMINI_API_KEY first:  export GEMINI_API_KEY=...   (https://aistudio.google.com/apikey)" >&2
  exit 1
fi

export UV_PROJECT_ENVIRONMENT="${UV_PROJECT_ENVIRONMENT:-/tmp/ast-eval-venv}"
export UV_LINK_MODE=copy
unset VIRTUAL_ENV  # avoid uv's "VIRTUAL_ENV does not match" warning

cd "$(dirname "$0")"
exec uv run run_eval.py "$@"
