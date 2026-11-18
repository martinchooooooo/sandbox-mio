#!/bin/sh
# Prints headers JSON from a secret file named by $1.
printf '{"X-Dummy":"%s"}' "$(cat "$1")"
