#!/bin/sh
# Reads the secret from the file named by $1 into the env var $2, then execs the server.
export "$2=$(cat "$1")"; shift 2; exec "$@"
