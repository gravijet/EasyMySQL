#!/bin/sh
# EasyMySQL installer for macOS and Linux
#
#   curl -fsSL https://mysql.benjaminberger.at/install.sh | sh
#
# Options as environment variables, e.g.  curl -fsSL … | EASYMYSQL_YES=1 sh
#   EASYMYSQL_YES=1          answer every question with yes
#   EASYMYSQL_AI=1 / 0       download and connect Claude Code and Codex without asking / never
#   EASYMYSQL_LOCAL_AI=1 / 0 download the own local AI (about 1.7 GB) without asking / never
#   EASYMYSQL_NO_MARIADB=1   do not install MariaDB
#   EASYMYSQL_BIN=dir        where the `easymysql` command goes (default ~/.local/bin)
#   NO_COLOR=1               plain output without colors and animations
#
# The script downloads the newest release, checks its SHA-256 checksum and installs it for the
# current user. It never clears the terminal, so everything stays in the scrollback.

set -eu

SITE="https://mysql.benjaminberger.at"
START="$(date +%s)"

# --- Terminal ----------------------------------------------------------------------------------------
# Farben und Animationen nur in einem echten Terminal; NO_COLOR und TERM=dumb schalten beides ab
FANCY=0
if [ -t 1 ] && [ "${TERM:-dumb}" != "dumb" ] && [ -z "${NO_COLOR:-}" ]; then FANCY=1; fi
# Unicode-Zeichen nur mit UTF-8-Gebietsschema, sonst einfache ASCII-Zeichen
case "${LC_ALL:-${LC_CTYPE:-${LANG:-}}}" in
  *UTF-8*|*utf-8*|*UTF8*|*utf8*) UTF=1 ;;
  *) UTF=0 ;;
esac
COLS="$( (tput cols 2>/dev/null || stty size 2>/dev/null </dev/tty | cut -d' ' -f2) | head -n1)"
case "$COLS" in ''|*[!0-9]*) COLS="${COLUMNS:-80}" ;; esac
case "$COLS" in ''|*[!0-9]*) COLS=80 ;; esac

esc() { printf '\033[%sm' "$1"; }
# Farbe als 24-Bit-Wert oder, wenn das Terminal das nicht kann, als naechste der 256 Farben
TRUE=0
case "${COLORTERM:-}" in truecolor|24bit) TRUE=1 ;; esac
case "${TERM_PROGRAM:-}" in iTerm.app|WezTerm|vscode|ghostty|Hyper) TRUE=1 ;; esac
rgb() {
  if [ "$TRUE" = "1" ]; then esc "38;2;$1;$2;$3"
  else esc "38;5;$((16 + 36 * (($1 * 5 + 127) / 255) + 6 * (($2 * 5 + 127) / 255) + (($3 * 5 + 127) / 255)))"
  fi
}

if [ "$FANCY" = "1" ]; then
  B="$(esc 1)"; D="$(esc 2)"; R="$(esc 0)"
  FG="$(rgb 236 233 226)"; MUTE="$(rgb 150 156 176)"; SH="$(rgb 70 66 110)"
  GRN="$(rgb 120 220 160)"; YEL="$(rgb 255 200 110)"; RED="$(rgb 255 110 120)"; CYN="$(rgb 140 200 255)"
  ACC="$(rgb 179 140 255)"
  # Verlauf Blau -> Violett -> Rosa -> Orange in 16 Stufen (G0 … G15)
  i=0
  while [ $i -lt 16 ]; do
    if [ $i -lt 6 ]; then t=$((i * 100 / 5)); r=$((122 + (179 - 122) * t / 100)); g=$((162 + (140 - 162) * t / 100)); b=255
    elif [ $i -lt 11 ]; then t=$(((i - 5) * 100 / 5)); r=$((179 + (255 - 179) * t / 100)); g=$((140 + (143 - 140) * t / 100)); b=$((255 + (177 - 255) * t / 100))
    else t=$(((i - 10) * 100 / 5)); r=255; g=$((143 + (184 - 143) * t / 100)); b=$((177 + (107 - 177) * t / 100))
    fi
    eval "G$i=\"\$(rgb $r $g $b)\""
    i=$((i + 1))
  done
else
  B=""; D=""; R=""; FG=""; MUTE=""; SH=""; GRN=""; YEL=""; RED=""; CYN=""; ACC=""
  i=0; while [ $i -lt 16 ]; do eval "G$i=''"; i=$((i + 1)); done
fi

if [ "$UTF" = "1" ]; then
  OK="✓"; BAD="✗"; ELL="…"; WARN="!"; DOT="·"; ASK="?"; ARROW="→"; FULL="━"; EMPTY="━"; RULE="─"
  SPIN="⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏"
  TL="╭"; TR="╮"; BL="╰"; BR="╯"; V="│"
else
  OK="+"; BAD="x"; ELL=".."; WARN="!"; DOT="-"; ASK="?"; ARROW="->"; FULL="#"; EMPTY="-"; RULE="-"
  SPIN="| / - \\"
  TL="+"; TR="+"; BL="+"; BR="+"; V="|"
fi

# Pause fuer Animationen; Systeme ohne Bruchteile von Sekunden warten einfach nicht
nap() { sleep "$1" 2>/dev/null || true; }

# --- Output ------------------------------------------------------------------------------------------
# Jede Zeile: Zeichen, Bezeichnung in fester Breite, Wert. Hinweise stehen eingerueckt darunter.
say()    { printf '%s\n' "$*"; }
line()   { printf '  %s%s%s %s%-10s%s %s\n' "$1" "$2" "$R" "$B$FG" "$3" "$R" "$4"; }
done_()  { line "$GRN" "$OK" "$1" "$2"; }
warn()   { line "$YEL" "$WARN" "$1" "$YEL$2$R"; }
detail() { printf '               %s%s%s\n' "$MUTE" "$*" "$R"; }
code()   { printf '               %s%s%s\n' "$CYN" "$*" "$R"; }
has()    { command -v "$1" >/dev/null 2>&1; }

cursor_off() { [ "$FANCY" = "1" ] && printf '\033[?25l' || true; }
cursor_on()  { [ "$FANCY" = "1" ] && printf '\033[?25h' || true; }
clear_line() { [ "$FANCY" = "1" ] && printf '\r\033[2K' || true; }

fail() {
  clear_line; cursor_on
  printf '\n  %s%s Installation stopped%s\n' "$RED$B" "$BAD" "$R" >&2
  printf '    %s\n' "$*" >&2
  printf '    %sDownload by hand: %s%s\n\n' "$MUTE" "$SITE" "$R" >&2
  exit 1
}

# Laufende Arbeit mit Spinner anzeigen: spin "Bezeichnung" "Text" PID
# Ohne Terminal steht nur eine Zeile da; der Rueckgabewert ist der des Prozesses
spin() {
  label="$1"; text="$2"; pid="$3"; n=0
  if [ "$FANCY" = "1" ]; then
    cursor_off
    while kill -0 "$pid" 2>/dev/null; do
      # shellcheck disable=SC2086
      set -- $SPIN
      k=$((n % $# + 1)); eval "f=\${$k}"
      eval "c=\${G$(( (n / 2) % 16 ))}"
      printf '\r  %s%s%s %s%-10s%s %s%s%s' "$c" "$f" "$R" "$B$FG" "$label" "$R" "$MUTE" "$text" "$R"
      n=$((n + 1)); nap 0.08
    done
    clear_line; cursor_on
  fi
  wait "$pid"
}

# Questions go to the terminal even when the script itself comes from a pipe (curl | sh)
ask() {
  question="$1"; default="${2:-y}"
  if [ "${EASYMYSQL_YES:-}" = "1" ]; then return 0; fi
  if ! (exec </dev/tty) 2>/dev/null; then [ "$default" = "y" ]; return; fi
  if [ "$default" = "y" ]; then hint="Y/n"; else hint="y/N"; fi
  printf '  %s%s%s %s %s(%s)%s ' "$CYN$B" "$ASK" "$R" "$question" "$MUTE" "$hint" "$R" >/dev/tty
  read -r answer </dev/tty || answer=""
  case "$answer" in
    "") [ "$default" = "y" ] ;;
    [YyJj]*) return 0 ;;
    *) return 1 ;;
  esac
}

as_root() {
  if [ "$(id -u)" = "0" ]; then "$@"
  elif has sudo; then sudo "$@"
  else fail "This step needs root rights, but sudo is missing: $*"
  fi
}

mb() { awk "BEGIN { printf \"%.1f MB\", $1 / 1048576 }"; }

# Pfad im Heimatordner kurz mit ~ anzeigen
short() { case "$1" in "$HOME"/*) printf '~%s' "${1#"$HOME"}" ;; *) printf '%s' "$1" ;; esac; }

# --- Banner ------------------------------------------------------------------------------------------
banner() {
  say ""
  if [ "$UTF" = "1" ] && [ "$COLS" -ge 84 ]; then
    for row in \
      "${G0}█████${G1}██${SH}╗ ${G1}█${G2}████${SH}╗ ${G3}████${G4}███${SH}╗${G4}█${G5}█${SH}╗   ${G6}██${SH}╗${G6}██${G7}█${SH}╗   ${G8}███${SH}╗${G9}██${SH}╗   ${G10}██${SH}╗${G10}█${G11}█████${G12}█${SH}╗ ${G12}██${G13}████${SH}╗ ${G14}██${SH}╗${R}" \
      "${G0}██${SH}╔════╝${G1}██${SH}╔══${G2}██${SH}╗${G3}██${SH}╔════╝╚${G5}██${SH}╗ ${G5}█${G6}█${SH}╔╝${G6}██${G7}██${SH}╗ ${G8}████${SH}║╚${G9}██${SH}╗ ${G10}██${SH}╔╝${G10}█${G11}█${SH}╔════╝${G12}██${SH}╔═══${G13}██${SH}╗${G14}██${SH}║${R}" \
      "${G0}█████${SH}╗  ${G1}██${G2}█████${SH}║${G3}████${G4}███${SH}╗ ╚${G5}████${SH}╔╝ ${G6}██${SH}╔${G7}███${G8}█${SH}╔${G8}██${SH}║ ╚${G9}███${G10}█${SH}╔╝ ${G10}█${G11}█████${G12}█${SH}╗${G12}██${SH}║   ${G13}██${SH}║${G14}██${SH}║${R}" \
      "${G0}██${SH}╔══╝  ${G1}██${SH}╔══${G2}██${SH}║╚════${G4}██${SH}║  ╚${G5}██${SH}╔╝  ${G6}██${SH}║╚${G7}██${SH}╔╝${G8}██${SH}║  ╚${G9}██${SH}╔╝  ╚════${G11}█${G12}█${SH}║${G12}██${SH}║▄▄ ${G13}██${SH}║${G14}██${SH}║${R}" \
      "${G0}█████${G1}██${SH}╗${G1}██${SH}║  ${G2}██${SH}║${G3}████${G4}███${SH}║   ${G5}██${SH}║   ${G6}██${SH}║ ╚═╝ ${G8}██${SH}║   ${G9}██${SH}║   ${G10}█${G11}█████${G12}█${SH}║╚${G12}██${G13}████${SH}╔╝${G14}████${G15}███${SH}╗${R}" \
      "${SH}╚══════╝╚═╝  ╚═╝╚══════╝   ╚═╝   ╚═╝     ╚═╝   ╚═╝   ╚══════╝ ╚══▀▀═╝ ╚══════╝${R}"
    do
      printf '  %s\n' "$row"
      if [ "$FANCY" = "1" ]; then nap 0.035; fi
    done
  else
    printf '  %s%sE%sa%ss%sy%sM%sy%sS%sQ%sL%s\n' "$B" "$G0" "$G2" "$G4" "$G6" "$G8" "$G10" "$G12" "$G14" "$G15" "$R"
  fi
  say ""
  printf '  %sMariaDB server and database manager%s  %s%s%s  %sSQL editor %s ER diagrams %s backups %s AI%s\n' \
    "$FG" "$R" "$SH" "$DOT" "$R" "$MUTE" "$DOT" "$DOT" "$DOT" "$R"
  say ""
}

# --- Helpers -----------------------------------------------------------------------------------------
# Value of "key" inside the "kind" object of the compact JSON from /api/release
field() {
  printf '%s' "$JSON" | tr -d '\n' | sed -n "s/.*\"$1\":{\([^}]*\)}.*/\1/p" | sed -n "s/.*\"$2\":\"\{0,1\}\([^\",]*\)\"\{0,1\}.*/\1/p"
}

sha256_of() {
  if has sha256sum; then sha256sum "$1" | cut -d' ' -f1
  elif has shasum; then shasum -a 256 "$1" | cut -d' ' -f1
  else return 1
  fi
}

find_mariadb() {
  for d in /opt/homebrew/opt/mariadb/bin /opt/homebrew/bin /usr/local/opt/mariadb/bin /usr/local/bin /usr/sbin /usr/local/sbin /usr/bin; do
    if [ -x "$d/mariadbd" ]; then printf '%s\n' "$d/mariadbd"; return 0; fi
  done
  if has mariadbd; then command -v mariadbd; return 0; fi
  return 1
}

# Fortschrittsbalken mit Verlauf: bar Prozent Breite
bar() {
  pct="$1"; width="$2"; fill=$((pct * width / 100)); j=0; out=""
  while [ $j -lt "$width" ]; do
    if [ $j -lt "$fill" ]; then eval "c=\${G$((j * 16 / width))}"; out="$out$c$FULL"
    else out="$out$SH$EMPTY"
    fi
    j=$((j + 1))
  done
  printf '%s%s' "$out" "$R"
}

# Herunterladen mit eigenem Balken (Prozent, Groesse, Tempo); ohne Terminal still
download() {
  url="$1"; out="$2"; total="$3"
  case "$total" in ''|null|*[!0-9]*) total=0 ;; esac
  t0="$(date +%s)"
  curl -fsSL --retry 3 --connect-timeout 20 -o "$out" "$url" </dev/null &
  BG=$!
  if [ "$FANCY" = "1" ]; then
    cursor_off
    width=28
    if [ "$COLS" -lt 72 ]; then width=14; fi
    while kill -0 "$BG" 2>/dev/null; do
      got=0; [ -f "$out" ] && got="$(wc -c <"$out" | tr -d ' ')"
      secs=$(( $(date +%s) - t0 )); [ "$secs" -lt 1 ] && secs=1
      if [ "$total" -gt 0 ]; then
        pct=$((got * 100 / total)); [ "$pct" -gt 100 ] && pct=100
        printf '\r  %s%s%s %s%-10s%s %s %s%3d%%%s  %s%s / %s  %s/s%s\033[K' "$ACC" "$ARROW" "$R" "$B$FG" "Download" "$R" \
          "$(bar "$pct" "$width")" "$FG" "$pct" "$R" "$MUTE" "$(mb "$got")" "$(mb "$total")" "$(mb $((got / secs)))" "$R"
      else
        printf '\r  %s%s%s %s%-10s%s %s%s%s\033[K' "$ACC" "$ARROW" "$R" "$B$FG" "Download" "$R" "$MUTE" "$(mb "$got")" "$R"
      fi
      nap 0.1
    done
    clear_line; cursor_on
  fi
  if ! wait "$BG"; then BG=""; return 1; fi
  BG=""
  secs=$(( $(date +%s) - t0 )); [ "$secs" -lt 1 ] && secs="<1"
  got="$(wc -c <"$out" | tr -d ' ')"
  if [ "$FANCY" = "1" ]; then
    done_ "Download" "$(bar 100 "$width") $MUTE$(mb "$got") in ${secs} s$R"
  else
    done_ "Download" "$(mb "$got") in ${secs} s"
  fi
}

# Befehl im Hintergrund mit Spinner; Ausgabe landet im Protokoll und erscheint nur bei einem Fehler
quietly() {
  label="$1"; text="$2"; shift 2
  "$@" >"$TMP/step.log" 2>&1 </dev/null &
  if spin "$label" "$text" $!; then return 0; fi
  warn "$label" "failed: $*"
  tail -n 12 "$TMP/step.log" | while IFS= read -r l; do detail "$l"; done
  return 1
}

install_mariadb() {
  if [ "$OS" = "macos" ]; then
    if has brew; then
      ask "Install MariaDB with Homebrew (brew install mariadb)?" y || { warn "MariaDB" "skipped - EasyMySQL needs it: brew install mariadb"; return; }
      quietly "MariaDB" "brew install mariadb (this can take a few minutes)" brew install mariadb || return
    else
      warn "MariaDB" "Homebrew is missing. Install it from https://brew.sh, then run:"
      code "brew install mariadb"
      return
    fi
  else
    # Only the programs, no system service: EasyMySQL runs its own server in your home folder
    if has apt-get; then set -- apt-get install -y mariadb-server-core mariadb-client; how="apt (mariadb-server-core, mariadb-client)"
    elif has dnf; then set -- dnf install -y mariadb-server; how="dnf (mariadb-server)"
    elif has pacman; then set -- pacman -S --needed --noconfirm mariadb; how="pacman (mariadb)"
    elif has zypper; then set -- zypper --non-interactive install mariadb mariadb-client; how="zypper (mariadb)"
    else
      warn "MariaDB" "no known package manager - install the MariaDB server with your distribution's tools."
      return
    fi
    ask "Install the MariaDB programs with $how?" y || { warn "MariaDB" "skipped"; return; }
    # Passwort einmal sichtbar abfragen, danach laeuft die Installation mit Spinner
    if [ "$(id -u)" != "0" ]; then
      has sudo || { warn "MariaDB" "needs root rights, but sudo is missing"; return; }
      sudo -v </dev/tty || { warn "MariaDB" "no root rights - skipped"; return; }
      if [ "$1" = "apt-get" ]; then quietly "MariaDB" "updating package lists" sudo -n apt-get update -qq || return; fi
      quietly "MariaDB" "installing with $1" sudo -n "$@" || return
    else
      if [ "$1" = "apt-get" ]; then quietly "MariaDB" "updating package lists" apt-get update -qq || return; fi
      quietly "MariaDB" "installing with $1" "$@" || return
    fi
  fi
  if p="$(find_mariadb)"; then done_ "MariaDB" "$(short "$p")"; else warn "MariaDB" "installed, but mariadbd was not found. Set EASYMYSQL_MARIADB_BIN to its folder."; fi
}

# Rahmen um die Zusammenfassung; Inhalt ist reines ASCII, damit die Breiten stimmen
card() {
  w=56
  rule=""; j=0; while [ $j -lt $w ]; do rule="$rule$RULE"; j=$((j + 1)); done
  printf '  %s%s%s%s%s\n' "$SH" "$TL" "$rule" "$TR" "$R"
  for row in "$@"; do
    # Zeilen: "t|Titel", "c|Befehl|Erklaerung" oder leer
    kind="${row%%|*}"; rest="${row#*|}"
    case "$kind" in
      t) printf '  %s%s%s  %s%-*s%s%s%s%s\n' "$SH" "$V" "$R" "$B$GRN" $((w - 2)) "$rest" "$R" "$SH" "$V" "$R" ;;
      c) cmd="${rest%%|*}"; what="${rest#*|}"
         printf '  %s%s%s  %s%-22s%s%s%-*s%s%s%s\n' "$SH" "$V" "$R" "$ACC" "$cmd" "$R" "$MUTE" $((w - 24)) "$what" "$R" "$SH$V" "$R" ;;
      *) printf '  %s%s%s%*s%s%s%s\n' "$SH" "$V" "$R" "$w" "" "$SH" "$V" "$R" ;;
    esac
  done
  printf '  %s%s%s%s%s\n' "$SH" "$BL" "$rule" "$BR" "$R"
}

main() {
  BIN="${EASYMYSQL_BIN:-$HOME/.local/bin}"
  TMP=""; BG=""
  trap 'cursor_on; [ -n "$BG" ] && kill "$BG" 2>/dev/null; [ -n "$TMP" ] && rm -rf "$TMP"' EXIT
  trap 'clear_line; cursor_on; printf "\n  %sCancelled.%s\n\n" "$YEL" "$R"; exit 130' INT TERM
  banner

  # 1. System ---------------------------------------------------------------------------------------
  has curl || fail "curl is required."
  has tar || fail "tar is required."
  case "$(uname -s)" in
    Darwin) OS="macos"; KIND="macos"
            ver="$(sw_vers -productVersion 2>/dev/null || echo "?")"
            done_ "System" "macOS $ver $DOT $(uname -m) $MUTE(universal app for Apple Silicon and Intel)$R" ;;
    Linux)  OS="linux"
            case "$(uname -m)" in
              x86_64|amd64) KIND="linux" ;;
              *) fail "There is no ready-made package for Linux on $(uname -m) yet. Build it from source: cargo install --locked --git https://github.com/gravijet/EasyMySQL" ;;
            esac
            distro="$( (. /etc/os-release 2>/dev/null && printf '%s' "${PRETTY_NAME:-Linux}") || echo Linux)"
            done_ "System" "$distro $DOT $(uname -m)" ;;
    *) fail "This script is for macOS and Linux. On Windows run in PowerShell: irm $SITE/install.ps1 | iex" ;;
  esac
  TMP="$(mktemp -d 2>/dev/null || mktemp -d -t easymysql)"

  # Vorhandene Version fuer die Anzeige "alt -> neu"
  OLD=""
  for e in "$BIN/easymysql" "$(command -v easymysql 2>/dev/null || true)"; do
    if [ -n "$e" ] && [ -x "$e" ]; then
      OLD="$("$e" --version 2>/dev/null </dev/null | sed -n 's/.*\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' | head -n1)" || OLD=""
      [ -n "$OLD" ] && break
    fi
  done

  # 2. Release --------------------------------------------------------------------------------------
  curl -fsSL --retry 2 --connect-timeout 20 -H 'Accept: application/json' -o "$TMP/release.json" "$SITE/api/release" </dev/null &
  spin "Version" "asking $SITE for the newest release" $! || fail "The release information could not be loaded. Check your internet connection."
  JSON="$(cat "$TMP/release.json")"
  VERSION="$(printf '%s' "$JSON" | sed -n 's/.*"version":"\([0-9.]*\)".*/\1/p')"
  NAME="$(field "$KIND" name)"; URL="$(field "$KIND" url)"; SHA="$(field "$KIND" sha256)"; SIZE="$(field "$KIND" size)"
  [ -n "$VERSION" ] || fail "The release information could not be read."
  if [ -z "$URL" ] || [ -z "$SHA" ] || [ "$SHA" = "null" ]; then
    fail "EasyMySQL $VERSION has no $OS package yet. It is built with the next release; until then: $SITE"
  fi
  case "$SIZE" in ''|null) sz="" ;; *) sz=" $DOT $(mb "$SIZE")" ;; esac
  if [ -n "$OLD" ] && [ "$OLD" != "$VERSION" ]; then
    done_ "Version" "EasyMySQL $OLD $ACC$ARROW$R $B$VERSION$R$MUTE$sz$R"
  elif [ -n "$OLD" ]; then
    done_ "Version" "EasyMySQL $B$VERSION$R$MUTE$sz $DOT reinstalling$R"
  else
    done_ "Version" "EasyMySQL $B$VERSION$R$MUTE$sz$R"
  fi

  # 3. Download -------------------------------------------------------------------------------------
  ARCHIVE="$TMP/$NAME"
  download "$SITE$URL" "$ARCHIVE" "$SIZE" || fail "The download failed. Check your internet connection and try again."

  # 4. Verify ---------------------------------------------------------------------------------------
  sha256_of "$ARCHIVE" >"$TMP/sha" 2>/dev/null &
  spin "Checksum" "verifying SHA-256" $! || fail "Neither sha256sum nor shasum is available to verify the download."
  GOT="$(cat "$TMP/sha")"
  [ "$GOT" = "$SHA" ] || fail "Checksum mismatch - the download was deleted. Expected $SHA, got $GOT."
  done_ "Checksum" "SHA-256 verified $MUTE$DOT $(printf '%s' "$GOT" | cut -c1-12)$ELL$(printf '%s' "$GOT" | cut -c53-64)$R"

  # 5. Install --------------------------------------------------------------------------------------
  mkdir -p "$BIN" "$TMP/unpack"
  tar -xzf "$ARCHIVE" -C "$TMP/unpack" </dev/null &
  spin "Install" "unpacking" $! || fail "The archive could not be unpacked."
  if [ "$OS" = "macos" ]; then
    [ -d "$TMP/unpack/EasyMySQL.app" ] || fail "The archive does not contain EasyMySQL.app."
    if [ -w /Applications ]; then APPS="/Applications"; else APPS="$HOME/Applications"; mkdir -p "$APPS"; fi
    APP="$APPS/EasyMySQL.app"
    rm -rf "$APP"
    mv "$TMP/unpack/EasyMySQL.app" "$APP"
    xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true
    EXE="$APP/Contents/MacOS/easymysql"
    ln -sf "$EXE" "$BIN/easymysql"
    done_ "Install" "$(short "$APP")"
  else
    SRC="$TMP/unpack/EasyMySQL"
    [ -x "$SRC/easymysql" ] || fail "The archive does not contain the easymysql program."
    DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
    DIR="$DATA/easymysql-app"
    mkdir -p "$DIR" "$DATA/applications" "$DATA/icons/hicolor/256x256/apps"
    # Replace atomically: a running EasyMySQL keeps its old file
    cp "$SRC/easymysql" "$DIR/easymysql.new" && mv -f "$DIR/easymysql.new" "$DIR/easymysql"
    chmod 755 "$DIR/easymysql"
    EXE="$DIR/easymysql"
    ln -sf "$EXE" "$BIN/easymysql"
    cp "$SRC/easymysql.png" "$DATA/icons/hicolor/256x256/apps/easymysql.png"
    sed "s|^Exec=easymysql|Exec=$EXE|" "$SRC/easymysql.desktop" > "$DATA/applications/easymysql.desktop"
    has update-desktop-database && update-desktop-database "$DATA/applications" >/dev/null 2>&1 || true
    has gtk-update-icon-cache && gtk-update-icon-cache -q "$DATA/icons/hicolor" >/dev/null 2>&1 || true
    done_ "Install" "$(short "$DIR")"
    detail "menu entry $DOT $(short "$DATA/applications/easymysql.desktop")"
  fi
  detail "command    $DOT $(short "$BIN/easymysql")"
  case ":$PATH:" in
    *":$BIN:"*) ;;
    *) warn "PATH" "$(short "$BIN") is not in your PATH yet. Add this line to ~/.zshrc or ~/.bashrc:"
       case "$BIN" in
         "$HOME"/*) code "export PATH=\"\$HOME${BIN#"$HOME"}:\$PATH\"" ;;
         *) code "export PATH=\"$BIN:\$PATH\"" ;;
       esac ;;
  esac

  # MariaDB ----------------------------------------------------------------------------------------
  if p="$(find_mariadb)"; then
    done_ "MariaDB" "$(short "$p")"
  elif [ "${EASYMYSQL_NO_MARIADB:-}" = "1" ]; then
    warn "MariaDB" "missing (skipped by EASYMYSQL_NO_MARIADB)"
  else
    say ""
    detail "EasyMySQL runs its own MariaDB server and needs the MariaDB programs."
    install_mariadb
  fi

  # AI assistants ----------------------------------------------------------------------------------
  found=""
  has claude && found="Claude Code"
  has codex && found="${found:+$found, }Codex"
  want=0
  if [ "${EASYMYSQL_AI:-}" = "1" ]; then want=1
  elif [ "${EASYMYSQL_AI:-}" != "0" ]; then
    if [ "$found" = "Claude Code, Codex" ]; then
      say ""
      detail "Found: $found"
      if ask "Connect EasyMySQL to your AI assistants (read-only until you allow changes)?" y; then want=1; fi
    elif [ "${EASYMYSQL_YES:-}" != "1" ]; then
      say ""
      [ -n "$found" ] && detail "Found: $found"
      if ask "Download the AI assistants Claude Code and Codex (about 250 MB) and connect them?" "$([ -n "$found" ] && echo y || echo n)"; then want=1; fi
    elif [ -n "$found" ]; then want=1
    fi
  fi
  # install-ai laedt nur, was fehlt (neueste Version mit Pruefsumme), und traegt beide ein
  # Aeltere Versionen kennen install-ai noch nicht: dann nur eintragen
  if [ "$want" = "1" ]; then
    ai_cmd=setup-ai
    "$EXE" --help 2>/dev/null </dev/null | grep -q install-ai && ai_cmd=install-ai
    say ""
    if "$EXE" "$ai_cmd" </dev/null; then done_ "AI" "Claude Code and Codex are connected"
    else warn "AI" "setting up the AI assistants did not fully work (see above)"
    fi
  fi

  # Local AI (llama.cpp + Qwen2.5-Coder 1.5B, about 1.7 GB) -----------------------------------------
  # Nicht mit EASYMYSQL_YES: so ein grosser Download nur auf ausdruecklichen Wunsch
  local_ai=0
  if [ "${EASYMYSQL_LOCAL_AI:-}" = "1" ]; then local_ai=1
  elif [ "${EASYMYSQL_LOCAL_AI:-}" != "0" ] && [ "${EASYMYSQL_YES:-}" != "1" ] && (exec </dev/tty) 2>/dev/null; then
    say ""
    if ask "Install the own local AI for code suggestions and auto-fix (free, runs offline, about 1.7 GB)?" n; then local_ai=1; fi
  fi
  if [ "$local_ai" = "1" ] && "$EXE" --help 2>/dev/null </dev/null | grep -q install-local-ai; then
    say ""
    if "$EXE" install-local-ai </dev/null; then done_ "Local AI" "ready - suggestions and auto-fix run on this computer"
    else warn "Local AI" "the download did not work; try again under Settings > Editor"
    fi
  fi

  # Done -------------------------------------------------------------------------------------------
  took=$(( $(date +%s) - START ))
  say ""
  card "t|EasyMySQL $VERSION is ready  ($took s)" "" \
    "c|easymysql|open the app, the server starts" \
    "c|mariadb -u root|console while EasyMySQL runs" \
    "c|easymysql install-ai|connect Claude Code + Codex" \
    "c|easymysql install-local-ai|own local AI (about 1.7 GB)" \
    "c|easymysql mcp|MCP server for other AI apps"
  say ""
  if ask "Start EasyMySQL now?" y; then
    if [ "$OS" = "macos" ]; then open "$APP"; done_ "Started" "EasyMySQL opens in a moment"
    elif [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then (nohup "$EXE" >/dev/null 2>&1 </dev/null &); done_ "Started" "EasyMySQL opens in a moment"
    else detail "No graphical session found; start it later with: easymysql"
    fi
  fi
  say ""
}

main "$@"
