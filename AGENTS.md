# Hinweise für KI-Assistenten (Claude, Codex, Copilot, Cursor …)

Diese Regeln gelten für jede automatisierte Arbeit an EasyMySQL.

## Arbeitsablauf: standardmäßig direkt auf main

1. Fertige Arbeit lokal prüfen, direkt auf `main` committen und nach `origin/main` pushen.
2. Vor der Arbeit `main` per Fast-forward aktualisieren; fremde Änderungen erhalten und nie mit Force pushen.
3. Standardmäßig keinen Arbeits-Branch und keinen Pull Request anlegen. Branches und Pull Requests nur
   verwenden, wenn der Nutzer sie ausdrücklich verlangt.
4. Jeder Push auf `main` startet sofort den Website-Workflow, unabhängig von den geänderten Dateien.
   Die Veröffentlichung braucht die Laufzeit von GitHub Actions und Cloudflare. Die Tests laufen auch auf `main`.
   App-Releases und Windows-UI-Builds werden nur auf ausdruecklichen Nutzerwunsch manuell in GitHub Actions gestartet; niemals nach einem Push ausloesen.
5. Im Ergebnis nennen, was geändert und geprüft wurde und was offen ist.

## Version

Die Version in `Cargo.toml` wächst mit dem Umfang der Änderung (siehe „Versioning“ in der README):
große Umbauten → Hauptversion, neue Funktionen → Nebenversion, Fehlerbehebungen → Patch.
Ein Release entsteht nur, wenn `Cargo.toml` eine Version ohne vorhandenes Tag hat.

## Bauen und prüfen

Schnelle Verifikation (Clippy und Tests; `&&` in PowerShell ab Version 7):

```sh
cargo clippy --locked && cargo test --locked
```

```sh
cargo build
cargo test --locked                              # alle Tests ohne Netz
cargo check --target x86_64-pc-windows-gnu       # Windows (Hauptziel) mitprüfen
cargo test e2e_ -- --ignored --test-threads=1    # echte Downloads (Python, Node, Sprachserver …)
```

`cargo clippy` soll ohne neue Warnungen bleiben.

## Architektur-Kurzuebersicht

- `src/mcp/`: MCP-Server, Protokoll, SQL-Schutz und Setup fuer KI-Assistenten.
- `plugins/easymysql/`: Claude-/Codex-Plugin mit dem eingebetteten EasyMySQL-Skill.
- `website/public/install.*`: Installer fuer Windows (`install.ps1`) und macOS/Linux (`install.sh`).
- `.github/workflows/release.yml`: Windows-Installer, universelle macOS-App, Linux-Paket und Veroeffentlichung.

## Konventionen

- Kommentare und Bezeichner im Code sind deutsch (ohne Umlaute in Kommentaren), Texte der Oberfläche
  zweisprachig: `crate::i18n::tr("Deutsch", "English")` bzw. `crate::tr_format!(…)`.
- Handbuchtexte (`src/tabs/help_content.rs`) brauchen einen englischen Eintrag in `i18n::text()`;
  der Test `manual_is_translated` prüft das. Jeder Befehl (`keymap::Cmd`) muss im Handbuch stehen.
- Werkzeuge (JDK, Python, Node.js, Treiber, Sprachserver, Git) lädt `src/dev/install.rs` – immer die
  neueste stabile Version mit Prüfsumme.
- Neue Einstellungen gehören in `src/settings.rs` (Speichern/Laden mit Test) und in die Einstellungsseite.
- Keine Passwörter oder Tokens in Dateien des Projekts schreiben.
