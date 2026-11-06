#!/usr/bin/env bash
# Das fertige Archiv pruefen, bevor es als Release-Artefakt hochgeladen wird.
set -euo pipefail
art="$1"
version="$2"
pruefordner="$(mktemp -d)"
trap 'rm -rf "$pruefordner"' EXIT

if [ "$art" = macos ]; then
  tar -xzf "dist/EasyMySQL-macos-universal-$version.tar.gz" -C "$pruefordner"
  app="$pruefordner/EasyMySQL.app"
  programm="$app/Contents/MacOS/easymysql"
  lipo -verify_arch arm64 x86_64 "$programm"
  plutil -lint "$app/Contents/Info.plist"
  test -s "$app/Contents/Resources/EasyMySQL.icns"
  codesign --verify --deep --strict --verbose=2 "$app"
else
  tar -xzf "dist/EasyMySQL-linux-x64-$version.tar.gz" -C "$pruefordner"
  programm="$pruefordner/EasyMySQL/easymysql"
  test -s "$pruefordner/EasyMySQL/easymysql.png"
  grep -q '^Exec=easymysql %F$' "$pruefordner/EasyMySQL/easymysql.desktop"
  ldd "$programm" | tee "$pruefordner/bibliotheken.txt"
  if grep -q 'not found' "$pruefordner/bibliotheken.txt"; then exit 1; fi
fi

test "$("$programm" --version)" = "EasyMySQL $version"
python3 - "$programm" "$version" <<'PY'
import json
import subprocess
import sys

anfragen = [
    {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "release-pruefung", "version": "1"}}},
    {"jsonrpc": "2.0", "method": "notifications/initialized"},
    {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
]
prozess = subprocess.run(
    [sys.argv[1], "mcp"], input="".join(json.dumps(a) + "\n" for a in anfragen),
    text=True, capture_output=True, timeout=30, check=True,
)
antworten = [json.loads(z) for z in prozess.stdout.splitlines()]
assert len(antworten) == 2, prozess.stdout
assert antworten[0]["id"] == 1, antworten
assert antworten[0]["result"]["protocolVersion"] == "2025-06-18", antworten
assert antworten[0]["result"]["serverInfo"]["version"] == sys.argv[2], antworten
assert antworten[1]["id"] == 2, antworten
assert any(w["name"] == "get_schema" for w in antworten[1]["result"]["tools"]), antworten
print("MCP initialize und tools/list: OK")
PY
