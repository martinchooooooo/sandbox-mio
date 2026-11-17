# Prints the hex of one xterm SGR mouse report: sgr.sh <button-code> <col> <row> <M|m>
printf '1b5b3c%s' "$(printf '%s;%s;%s%s' "$1" "$2" "$3" "$4" | od -An -tx1 | tr -d ' \n')"
