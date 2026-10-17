#!/bin/sh
# Runs the bootstrap fixture in Wine (docs/development/testing.md): with the
# dinput8 override the installer sets on Linux, the proxy starts the host
# before the game's entry point; without it, Wine loads its own dinput8 and
# the host never starts.
#
#   sh tests/wine.sh <folder written by the wine_fixture test>
set -eu
fixture=$(cd "$1" && pwd)
WINEPREFIX=${WINEPREFIX:-$(mktemp -d)/prefix}
export WINEPREFIX WINEDEBUG=-all

# A folder of the Linux file system as the game sees it, under Wine's `Z:`.
windows() { printf 'Z:%s' "$1" | tr / '\\'; }

wineboot -i
wineserver -w
printf '{"installation_dir":"%s"}' "$(windows "$fixture" | sed 's/\\/\\\\/g')" > "$fixture/angelica.json"

trace() {
  rm -f "$fixture/trace.txt" "$fixture/angelica-loader.log"
  ANGELICA_TEST_TRACE=$(windows "$fixture/trace.txt") ANGELICA_TEST_STATUS=0 ANGELICA_TEST_WINE=1 \
    wine "$fixture/game.exe" || true
  wineserver -w
  if [ -f "$fixture/trace.txt" ]; then tr -d '\r' < "$fixture/trace.txt"; fi
}

without=$(trace)
case "$without" in
  *host*) echo "the host started without the override"; exit 1 ;;
esac

# Wine writes the registry back when its server stops, so the server is
# stopped (above) before the file changes.
cat >> "$WINEPREFIX/user.reg" <<EOF

[Software\\\\Wine\\\\AppDefaults\\\\game.exe\\\\DllOverrides] $(date +%s)
"dinput8"="native,builtin"
EOF

with=$(trace)
if [ "$with" != "$(printf 'load\nhost\ngame')" ]; then
  echo "trace with the override: $with"
  cat "$fixture/angelica-loader.log" 2>/dev/null || true
  exit 1
fi
echo "the proxy started the host before the game's entry point in Wine"
