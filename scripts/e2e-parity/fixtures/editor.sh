#!/usr/bin/env bash
# Stand-in for $EDITOR: Ctrl+G should hand the draft file to it and read the
# result back into the composer.
set -eu
printf 'PARITY_EDITOR_TEXT\n' >"$1"
