# Hinweise für KI-Assistenten (Claude, Codex, Copilot, Cursor …)

Diese Regeln gelten für jede automatisierte Arbeit an EasyMySQL.

## Arbeitsablauf: immer über einen Pull Request

1. Nie direkt auf `main` committen oder pushen. Ein Push auf `main` baut und veröffentlicht sofort ein Release.
2. Für jede Aufgabe einen eigenen Branch anlegen: `fix/<thema>`, `feature/<thema>`, `design/…`, `docs/…`, `chore/…`,
   `refactor/…`, `perf/…` oder `ci/…` (z. B. `feature/python-support`).
3. Wenn die Arbeit fertig ist: committen, den Branch pushen und einen Pull Request gegen `main` öffnen
   (`gh pr create --base main --fill` oder mit Titel und Beschreibung). Der Workflow
   `.github/workflows/auto-pr.yml` öffnet ihn sonst selbst, sobald der Branch gepusht ist.
4. Die Beschreibung nennt, was sich geändert hat, wie es getestet wurde und was offen ist.
   `.github/workflows/auto-merge.yml` mergt ihn selbst, sobald der Workflow „Tests“ grün ist, und startet das
   Release, wenn `Cargo.toml` eine Version ohne Tag hat. Rote Tests = kein Merge.

## Version

Die Version in `Cargo.toml` wächst mit dem Umfang der Änderung (siehe „Versioning“ in der README):
große Umbauten → Hauptversion, neue Funktionen → Nebenversion, Fehlerbehebungen → Patch.
Ein Release entsteht nur, wenn `Cargo.toml` eine Version ohne vorhandenes Tag hat.

## Bauen und prüfen

```sh
cargo build
cargo test --locked                              # alle Tests ohne Netz
cargo check --target x86_64-pc-windows-gnu       # Windows (Hauptziel) mitprüfen
cargo test e2e_ -- --ignored --test-threads=1    # echte Downloads (Python, Node, Sprachserver …)
```

`cargo clippy` soll ohne neue Warnungen bleiben.

## Konventionen

- Kommentare und Bezeichner im Code sind deutsch (ohne Umlaute in Kommentaren), Texte der Oberfläche
  zweisprachig: `crate::i18n::tr("Deutsch", "English")` bzw. `crate::tr_format!(…)`.
- Handbuchtexte (`src/tabs/help_content.rs`) brauchen einen englischen Eintrag in `i18n::text()`;
  der Test `manual_is_translated` prüft das. Jeder Befehl (`keymap::Cmd`) muss im Handbuch stehen.
- Werkzeuge (JDK, Python, Node.js, Treiber, Sprachserver, Git) lädt `src/dev/install.rs` – immer die
  neueste stabile Version mit Prüfsumme.
- Neue Einstellungen gehören in `src/settings.rs` (Speichern/Laden mit Test) und in die Einstellungsseite.
- Keine Passwörter oder Tokens in Dateien des Projekts schreiben.
