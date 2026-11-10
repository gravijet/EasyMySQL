#!/bin/sh
# EasyMySQL installer for macOS and Linux
#
#   curl -fsSL https://mysql.benjaminberger.at/install.sh | sh
#
# Options as environment variables, e.g.  curl -fsSL … | EASYMYSQL_YES=1 sh
#   EASYMYSQL_YES=1          answer every question with yes
#   EASYMYSQL_AI=1 / 0       download and connect Claude Code and Codex without asking / never
#   EASYMYSQL_NO_MARIADB=1   do not install MariaDB
#   EASYMYSQL_BIN=dir        where the `easymysql` command goes (default ~/.local/bin)
#
# The script downloads the newest release, checks its SHA-256 checksum and installs it for the
# current user. It never clears the terminal, so everything stays in the scrollback.

set -eu

SITE="https://mysql.benjaminberger.at"
STEPS=5

# --- Output ----------------------------------------------------------------------------------------
if [ -t 1 ] && [ "${TERM:-dumb}" != "dumb" ] && [ -z "${NO_COLOR:-}" ]; then
  B="$(printf '\033[1m')"; D="$(printf '\033[2m')"; R="$(printf '\033[0m')"
  ACC="$(printf '\033[38;5;209m')"; GRN="$(printf '\033[32m')"; YEL="$(printf '\033[33m')"; RED="$(printf '\033[31m')"; CYN="$(printf '\033[36m')"
else
  B=""; D=""; R=""; ACC=""; GRN=""; YEL=""; RED=""; CYN=""
fi

say()  { printf '%s\n' "$*"; }
step() { printf '\n  %s%s/%s%s  %s%s%s\n' "$D" "$1" "$STEPS" "$R" "$B" "$2" "$R"; }
ok()   { printf '        %s+%s %s\n' "$GRN" "$R" "$*"; }
info() { printf '        %s.%s %s%s%s\n' "$D" "$R" "$D" "$*" "$R"; }
warn() { printf '        %s!%s %s\n' "$YEL" "$R" "$*"; }
fail() { printf '\n  %sInstallation stopped:%s %s\n  %sDownload by hand: %s%s\n\n' "$RED" "$R" "$*" "$D" "$SITE" "$R" >&2; exit 1; }
has()  { command -v "$1" >/dev/null 2>&1; }

# Questions go to the terminal even when the script itself comes from a pipe (curl | sh)
ask() {
  question="$1"; default="${2:-y}"
  if [ "${EASYMYSQL_YES:-}" = "1" ]; then return 0; fi
  if ! (exec </dev/tty) 2>/dev/null; then [ "$default" = "y" ]; return; fi
  if [ "$default" = "y" ]; then hint="[Y/n]"; else hint="[y/N]"; fi
  printf '        %s?%s %s %s ' "$CYN" "$R" "$question" "$hint" >/dev/tty
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

banner() {
  say ""
  say "     ${ACC}.-~~~~~~-.${R}   ${B}EasyMySQL${R}"
  say "    ${ACC}(~~~~~~~~~~)${R}  MariaDB server and database manager"
  say "    ${ACC}|~~~~~~~~~~|${R}  ${D}SQL editor, ER diagrams, backups, AI assistants${R}"
  say "    ${ACC}(__________)${R}  ${D}mysql.benjaminberger.at${R}"
}

# Value of "key" inside the "kind" object of the compact JSON from /api/release
field() {
  printf '%s' "$JSON" | tr -d '\n' | sed -n "s/.*\"$1\":{\([^}]*\)}.*/\1/p" | sed -n "s/.*\"$2\":\"\{0,1\}\([^\",]*\)\"\{0,1\}.*/\1/p"
}

sha256_of() {
  if has sha256sum; then sha256sum "$1" | cut -d' ' -f1
  elif has shasum; then shasum -a 256 "$1" | cut -d' ' -f1
  else fail "Neither sha256sum nor shasum is available to verify the download."
  fi
}

find_mariadb() {
  for d in /opt/homebrew/opt/mariadb/bin /opt/homebrew/bin /usr/local/opt/mariadb/bin /usr/local/bin /usr/sbin /usr/local/sbin /usr/bin; do
    if [ -x "$d/mariadbd" ]; then printf '%s\n' "$d/mariadbd"; return 0; fi
  done
  if has mariadbd; then command -v mariadbd; return 0; fi
  return 1
}

install_mariadb() {
  if [ "$OS" = "macos" ]; then
    if has brew; then
      ask "Install MariaDB with Homebrew (brew install mariadb)?" y || { warn "Skipped. EasyMySQL needs MariaDB: brew install mariadb"; return; }
      brew install mariadb
    else
      warn "Homebrew is missing. Install it from https://brew.sh, then run: brew install mariadb"
      return
    fi
  elif has apt-get; then
    # Only the programs, no system service: EasyMySQL runs its own server in your home folder
    ask "Install the MariaDB programs with apt (mariadb-server-core, mariadb-client)?" y || { warn "Skipped."; return; }
    as_root apt-get update -qq
    as_root apt-get install -y mariadb-server-core mariadb-client
  elif has dnf; then
    ask "Install MariaDB with dnf (mariadb-server)?" y || { warn "Skipped."; return; }
    as_root dnf install -y mariadb-server
  elif has pacman; then
    ask "Install MariaDB with pacman (mariadb)?" y || { warn "Skipped."; return; }
    as_root pacman -S --needed --noconfirm mariadb
  elif has zypper; then
    ask "Install MariaDB with zypper (mariadb)?" y || { warn "Skipped."; return; }
    as_root zypper --non-interactive install mariadb mariadb-client
  else
    warn "No known package manager. Install the MariaDB server with your distribution's tools."
    return
  fi
  if p="$(find_mariadb)"; then ok "MariaDB: $p"; else warn "MariaDB was installed, but mariadbd was not found. Set EASYMYSQL_MARIADB_BIN to its folder."; fi
}

main() {
  banner
  BIN="${EASYMYSQL_BIN:-$HOME/.local/bin}"
  TMP=""
  trap '[ -n "$TMP" ] && rm -rf "$TMP"' EXIT INT TERM

  # 1. System ---------------------------------------------------------------------------------------
  step 1 "Checking this computer"
  has curl || fail "curl is required."
  has tar || fail "tar is required."
  case "$(uname -s)" in
    Darwin) OS="macos"; KIND="macos"
            ver="$(sw_vers -productVersion 2>/dev/null || echo "?")"
            ok "macOS $ver, $(uname -m) (universal app: Apple Silicon and Intel)" ;;
    Linux)  OS="linux"
            case "$(uname -m)" in
              x86_64|amd64) KIND="linux" ;;
              *) fail "There is no ready-made package for Linux on $(uname -m) yet. Build it from source: cargo install --locked --git https://github.com/gravijet/EasyMySQL" ;;
            esac
            distro="$( (. /etc/os-release 2>/dev/null && printf '%s' "${PRETTY_NAME:-Linux}") || echo Linux)"
            ok "$distro, $(uname -m)" ;;
    *) fail "This script is for macOS and Linux. On Windows run in PowerShell: irm $SITE/install.ps1 | iex" ;;
  esac

  # 2. Release --------------------------------------------------------------------------------------
  step 2 "Finding the newest version"
  JSON="$(curl -fsSL -H 'Accept: application/json' "$SITE/api/release")" || fail "The release information could not be loaded."
  VERSION="$(printf '%s' "$JSON" | sed -n 's/.*"version":"\([0-9.]*\)".*/\1/p')"
  NAME="$(field "$KIND" name)"; URL="$(field "$KIND" url)"; SHA="$(field "$KIND" sha256)"; SIZE="$(field "$KIND" size)"
  [ -n "$VERSION" ] || fail "The release information could not be read."
  if [ -z "$URL" ] || [ -z "$SHA" ] || [ "$SHA" = "null" ]; then
    fail "EasyMySQL $VERSION has no $OS package yet. It is built with the next release; until then: $SITE"
  fi
  ok "EasyMySQL $VERSION"
  if [ -n "$SIZE" ] && [ "$SIZE" != "null" ]; then info "$NAME, $(awk "BEGIN { printf \"%.1f MB\", $SIZE / 1048576 }")"; else info "$NAME"; fi

  # 3. Download -------------------------------------------------------------------------------------
  step 3 "Downloading"
  TMP="$(mktemp -d 2>/dev/null || mktemp -d -t easymysql)"
  ARCHIVE="$TMP/$NAME"
  printf '%s' "$ACC"
  curl -fL --progress-bar -o "$ARCHIVE" "$SITE$URL" || { printf '%s' "$R"; fail "The download failed."; }
  printf '%s' "$R"

  # 4. Verify ---------------------------------------------------------------------------------------
  step 4 "Verifying the SHA-256 checksum"
  GOT="$(sha256_of "$ARCHIVE")"
  [ "$GOT" = "$SHA" ] || fail "Checksum mismatch - the download was deleted. Expected $SHA, got $GOT."
  ok "$GOT"

  # 5. Install --------------------------------------------------------------------------------------
  step 5 "Installing"
  mkdir -p "$BIN" "$TMP/unpack"
  tar -xzf "$ARCHIVE" -C "$TMP/unpack"
  if [ "$OS" = "macos" ]; then
    [ -d "$TMP/unpack/EasyMySQL.app" ] || fail "The archive does not contain EasyMySQL.app."
    if [ -w /Applications ]; then APPS="/Applications"; else APPS="$HOME/Applications"; mkdir -p "$APPS"; fi
    APP="$APPS/EasyMySQL.app"
    rm -rf "$APP"
    mv "$TMP/unpack/EasyMySQL.app" "$APP"
    xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true
    EXE="$APP/Contents/MacOS/easymysql"
    ln -sf "$EXE" "$BIN/easymysql"
    ok "App: $APP"
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
    ok "Program: $EXE"
    ok "Menu entry: $DATA/applications/easymysql.desktop"
  fi
  ok "Command: $BIN/easymysql"
  case ":$PATH:" in
    *":$BIN:"*) ;;
    *) warn "$BIN is not in your PATH. Add this line to ~/.zshrc or ~/.bashrc:"
       say "          export PATH=\"$BIN:\$PATH\"" ;;
  esac

  # MariaDB ----------------------------------------------------------------------------------------
  say ""
  if p="$(find_mariadb)"; then
    ok "MariaDB found: $p"
  elif [ "${EASYMYSQL_NO_MARIADB:-}" = "1" ]; then
    warn "MariaDB is missing (skipped by EASYMYSQL_NO_MARIADB)."
  else
    info "EasyMySQL runs its own MariaDB server and needs the MariaDB programs."
    install_mariadb
  fi

  # AI assistants ----------------------------------------------------------------------------------
  found=""
  has claude && found="Claude Code"
  has codex && found="${found:+$found, }Codex"
  want=0
  if [ "${EASYMYSQL_AI:-}" = "1" ]; then want=1
  elif [ "${EASYMYSQL_AI:-}" != "0" ]; then
    say ""
    if [ "$found" = "Claude Code, Codex" ]; then
      info "Found: $found"
      if ask "Connect EasyMySQL to your AI assistants (read-only until you allow changes)?" y; then want=1; fi
    elif [ "${EASYMYSQL_YES:-}" != "1" ]; then
      [ -n "$found" ] && info "Found: $found"
      if ask "Download the AI assistants Claude Code and Codex (about 250 MB) and connect them?" "$([ -n "$found" ] && echo y || echo n)"; then want=1; fi
    elif [ -n "$found" ]; then want=1
    fi
  fi
  # install-ai laedt nur, was fehlt (neueste Version mit Pruefsumme), und traegt beide ein
  # Aeltere Versionen kennen install-ai noch nicht: dann nur eintragen
  ai_cmd=setup-ai
  "$EXE" --help 2>/dev/null | grep -q install-ai && ai_cmd=install-ai
  if [ "$want" = "1" ]; then "$EXE" "$ai_cmd" || warn "Setting up the AI assistants did not fully work (see above)."; fi

  # Done -------------------------------------------------------------------------------------------
  say ""
  say "  ${GRN}Done.${R} ${B}EasyMySQL $VERSION is installed.${R}"
  say ""
  say "    ${ACC}easymysql${R}            open the app (the server starts with it)"
  say "    ${ACC}mariadb -u root${R}      MariaDB console, while EasyMySQL runs"
  say "    ${ACC}easymysql install-ai${R} download and connect Claude Code and Codex"
  say "    ${ACC}easymysql mcp${R}        MCP server for any other AI client"
  say ""
  if ask "Start EasyMySQL now?" y; then
    if [ "$OS" = "macos" ]; then open "$APP"
    elif [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then (nohup "$EXE" >/dev/null 2>&1 &)
    else info "No graphical session found; start it later with: easymysql"
    fi
  fi
  say ""
}

main "$@"
