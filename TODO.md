# EasyMySQL – offene Punkte

Stand: 2026-11-12, Version 1.1.18. Gesammelt beim Aufnehmen der Website-Screenshots und bei einer Durchsicht des Codes.
`cargo test --locked` läuft sauber (284 bestanden, 29 ignoriert), `cargo clippy` meldet 2 Kleinigkeiten (siehe 1.9).

Kennzeichnung: **[bestätigt]** = im Code oder in der laufenden App nachgewiesen, **[Verdacht]** = beim Lesen aufgefallen
und nicht ausprobiert, **[Idee]** = neue Funktion. Alles lief nur unter Linux.

## 1. Fehler (zuerst)

1. **[bestätigt] Query Builder: Alias umbenennen bricht die Abfrage.** Spalten und Join-Bedingungen speichern den
   Tabellennamen als Text (`Expr::Column { src, col }`, `src/tabs/builder/model.rs`). Wer den Alias ändert
   (`src/tabs/builder/mod.rs`, Zeile ~831), lässt alle Verweise auf dem alten Namen stehen, die SQL wird ungültig.
   Lösung: beim Ändern des Alias alle Verweise in Spalten, Bedingungen, Gruppierung und Sortierung mit umbenennen.
2. **[bestätigt] Git-Diff zeigt nach dem Wiederöffnen den alten Stand.** `open_tab` (`src/app.rs`, Zeile ~508) springt bei
   gleichem Schlüssel nur zum vorhandenen Tab; der neue Diff-Text wird verworfen (`src/app/git_panel.rs`, Zeilen ~584 und ~1054).
   Auch „Aktualisieren“ im Git-Bereich ändert das nicht. Lösung: den Inhalt des vorhandenen Tabs ersetzen.
3. **[bestätigt] Query Builder: `COUNT` startet als `COUNT(*)`.** Bei einem LEFT JOIN zählt das Zeilen ohne Treffer als 1.
   Besser: eine Spalte der gejointen Tabelle vorschlagen oder bei LEFT JOIN einen Hinweis zeigen.
4. **[bestätigt] Query Builder: Die Suchzeile im Menü „+ Spalte“ nimmt keine Tasten an.** Bei Tabellen mit vielen Spalten
   muss man scrollen.
5. **[bestätigt] Query Builder: Ein Klick neben ein Feld klappte einen ganzen Abschnitt ein** (beim Verlassen eines
   Eingabefelds). Fehlklick-Falle; prüfen, ob die Überschrift zu viel Fläche als Klickziel hat.
6. **[Verdacht] Eingaben kommen verzögert an.** Beim Tippen in ein Alias-Feld wurde aus „c“ ein „cc“. Kann an der
   Test-Umgebung (Xvfb, xdotool) liegen; mit echter Tastatur prüfen.
7. **[Verdacht] Datenbankwahl pro Projekt statt „zuletzt benutzt“.** Beim Öffnen von `queries/revenue.sql` war „schule1“
   gewählt, obwohl die Datenbank des Projekts `shop` ist.
8. **[Verdacht] Teilstrings ohne Grenzprüfung.** `src/sqledit/schema_fix.rs:138` rechnet `at - 1` ohne Prüfung auf `at == 0`;
   `src/backup.rs:175` schneidet `&name[..19]` und verlässt sich auf `is_backup_name` (nur sicher, wenn dort ASCII erzwungen wird);
   `src/transfer/import/mod.rs:166` schneidet `&s[19..]`. Alle drei mit Umlauten und leeren Eingaben testen.
9. **[bestätigt] Zwei Clippy-Warnungen:** eine Funktion mit 8 statt 7 Argumenten und ein `Some(x).filter(|_| …)`.
   AGENTS.md verlangt „keine neuen Warnungen“; die zwei bestehenden bereinigen.

## 2. Sicherheit und Datenschutz

1. **[bestätigt] Gespeicherte Passwörter liegen im Klartext** (`src/settings.rs`, `secret_enc`/`secret_dec`): setzt nur ein
   Steuerzeichen davor und maskiert Sonderzeichen. Betrifft DB- und SSH-Passwort und die Profile (`src/profiles.rs`),
   aber nur bei „Passwort merken“. Die Datei wird privat geschrieben, jeder Prozess des Benutzers kann sie trotzdem lesen.
   Besser: Schlüsselbund des Systems (Windows Credential Manager, macOS Keychain, Secret Service). Bis dahin in den
   Einstellungen deutlich sagen, dass es nicht verschlüsselt ist.
2. **[bestätigt] „Zertifikat nicht prüfen“ nimmt jedes Zertifikat an** (`src/transfer/server.rs:82`,
   `src/transfer/import/maria_source.rs:25`). Als Auswahl gewollt; beim Aktivieren eine Warnung zeigen und solche
   Verbindungen in der Liste markieren.
3. **[bestätigt] Download per Adresse erlaubt `http://`** (`src/transfer/remote.rs:51`). Bei Klartext-HTTP kann der Inhalt
   unterwegs verändert werden. Hinweis vor dem Import anzeigen.
4. **[Verdacht] Interne Adressen werden beim Download nicht abgewiesen** (`localhost`, `192.168.…`). Für eine Desktop-App
   gering; relevant, falls die KI-Schnittstelle (`src/mcp/`) Importe aus Adressen anstoßen darf. Das prüfen.
5. **[Verdacht] Website-CSP erlaubt `script-src 'unsafe-inline'`** (`website/public/_headers`). Inline-Skripte auslagern
   oder mit Hash freigeben.
6. **[bestätigt, positiv]** Das Update prüft SHA-256 und Größe vor dem Installieren (`src/updates.rs`). Beibehalten.
7. **[Idee]** Warnung beim Verbinden ohne TLS mit einem Server außerhalb von `localhost`.

## 3. Stabilität

1. **[bestätigt] Rund 830 `unwrap()`/`expect()` außerhalb von Tests.** Der Großteil sind Mutex-Sperren (`src/server.rs` 40,
   `src/updates.rs` 34, `src/dev/lsp.rs` 29, `src/dev/debug.rs` 28). Stürzt ein Hintergrundthread ab, ist die Sperre
   vergiftet und die Oberfläche stürzt mit. Ein Hilfsaufruf, der vergiftete Sperren übergeht, würde das abfangen.
2. **[bestätigt] Rund 320 `let _ = …`** verschlucken Fehler, vor allem in `src/server.rs` (30), `src/workspace.rs` (22),
   `src/tabs/er/svg.rs` (16), `src/updates.rs` (15). Bei Schreibvorgängen (Einstellungen, Sitzung, Backups) sollte ein
   Fehler sichtbar werden, statt dass Daten still verloren gehen.
3. **[bestätigt] `unreachable!` in Laufzeitcode:** `src/tabs/code.rs:332` („nur Java ist ausführbar“) und
   `src/dev/install.rs:516`. Wird eine weitere Sprache ausführbar, stürzt die App dort ab; durch eine Fehlermeldung ersetzen.
4. **[Verdacht] Download-Zeitgrenzen:** `src/transfer/remote.rs` setzt eine Stunde als Gesamtgrenze, aber keine für
   Verbindungsaufbau oder Stillstand. Ein hängender Server blockiert bis zum Abbruch.
5. **[Verdacht] `thread::sleep` in `src/tabs/preview/pdf.rs:348,363`.** Prüfen, ob das im Oberflächen-Thread läuft (Ruckler).
6. **[Verdacht] Sehr große Tabellen:** mit mehreren Millionen Zeilen, großen BLOB-Spalten und 200 Spalten testen.
7. **[bestätigt] Außerhalb der App geänderte Dateien:** der offene Editor-Tab lud die Datei erst nach dem Schließen und
   Öffnen neu. Dateiüberwachung oder Hinweis „Datei wurde geändert“ ergänzen (hängt mit 1.2 zusammen).

## 4. Neue Funktionen (Datenbank)

1. **Prozessliste und Server-Status** als eigene Ansicht (laufende Abfragen, Sperren, Verbindungen). `kill_query` gibt es,
   eine Ansicht dazu nicht.
2. **Langsame Abfragen:** Slow-Query-Log lesen, Verlauf nach Dauer sortieren, `EXPLAIN` als Grafik.
3. **Diagramm aus Ergebnissen** (Balken, Linie, Torte) im SQL-Editor und Query Builder.
4. **Schema-Vergleich zweier Datenbanken** mit erzeugtem Änderungsskript; dasselbe für Daten einer Tabelle.
5. **Gespeicherte Abfragen mit Namen, Tags und Suche** statt nur Dateien in `queries/`.
6. **Lesezeichen und Kommentare** an Tabellen, Spalten und Abfragen.
7. **Binlog und Replikation** (Status, Position, einfacher Replikat-Assistent); kommt im Code nicht vor.
8. **Export als Parquet und als Excel-Datei mit mehreren Blättern.** `xlsx` kommt nur in einer Datei vor, `parquet` nirgends.
9. **Volltextsuche über alle Tabellen** einer Datenbank („wo steht dieser Wert?“).
10. **Daten maskieren** beim Export (E-Mail, Namen) für Testkopien.
11. **Zeitplan für Sicherungen** mit automatischem Aufräumen. Backups gibt es (`src/backup.rs`), einen Zeitplan habe ich
    nicht gefunden.
12. **Rechte-Assistent mit Vorschau** („was darf dieser Benutzer wirklich?“).

## 5. Neue Funktionen (Editor und Entwicklung)

1. **Mehrfach-Cursor und Spaltenauswahl** im Editor (kommt im Code nicht vor).
2. **Suchen und Ersetzen über das ganze Projekt** mit Vorschau.
3. **Debugger** für weitere Sprachen neben Java.
4. **Tastenkürzel vom Nutzer änderbar** (es gibt `keymap::Cmd`; prüfen, ob die Einstellungen Änderungen erlauben).
5. **Git:** Zweige anlegen und wechseln, Stash, Konflikte lösen, Verlauf einer Datei. Prüfen, was davon schon in
   `src/app/git_panel.rs` steckt, und Lücken schließen.

## 6. Bedienung und Barrierefreiheit

1. **[bestätigt] Keine Unterstützung für Screenreader:** `Cargo.toml` aktiviert bei eframe keine
   Barrierefreiheits-Schnittstelle (AccessKit). Sehbehinderte Nutzer können die App damit nicht bedienen.
2. **Kontrast und Schriftgröße prüfen** im hellen Thema und bei Skalierung 150–200 %.
3. **Tastaturbedienung der Tabellen** prüfen (Pfeiltasten, Enter zum Bearbeiten, Tab zur nächsten Zelle).
4. **Weitere Sprachen:** bisher nur Deutsch und Englisch, die Texte stecken fest in `src/i18n.rs` (1500 Zeilen). Für weitere
   Sprachen wäre eine Übersetzungsdatei nötig.
5. **Fehlermeldungen des Servers mit Hinweis ergänzen** (z. B. „Unknown column ‘category.id’ in on clause“ → „Alias geändert?
   Dann Bedingung anpassen“).

## 7. Qualität und Wartung

1. **Große Dateien aufteilen:** `src/tabs/code.rs` (1886 Zeilen), `src/sqledit/mod.rs` (1733), `src/tabs/settings.rs` (1669),
   `src/i18n.rs` (1529), `src/dev/autofix.rs` (1500), `src/app.rs` (1483).
2. **Tests zu den Fixes:** „Alias umbenennen ändert alle Verweise“ (1.1) und „Diff-Tab zeigt nach Wiederöffnen den neuen
   Stand“ (1.2). Von den 313 Tests prüfen fast alle Logik, kaum welche die Oberfläche.
3. **Windows und macOS:** diese Durchsicht und die Screenshots liefen nur unter Linux. Der Windows-Check läuft per
   `cargo check --target x86_64-pc-windows-gnu`; ein echter Lauf auf Windows und macOS steht aus.
4. **[Verdacht] Unterschiedliche Grafik-Rückends:** `Cargo.toml` nutzt `glow` (Zeile 13) und an anderer Stelle `wgpu`
   (Zeile 40). Prüfen, ob das gewollt ist und ob Darstellungsunterschiede auftreten.

## 8. Website

1. **[bestätigt] `website/public/og.jpg` (Vorschaubild beim Teilen von Links) ist veraltet.** Es zeigt die frühere Gestaltung
   mit Tabs und dem nackten Befehl `irm mysql.benjaminberger.at/install | iex`. Die Seite nutzt inzwischen
   `powershell -c "irm … | iex"`, weil uBlock die nackte Form beim Kopieren schluckt. Neu erzeugen (1200×630, Wortmarke
   wie auf der Seite, Befehl in der neuen Form).
2. **[bestätigt] Übrig gebliebene Datei `repo/website/public/og.png`.** Wird nirgends verwendet (die Seite verweist auf
   `og.jpg`). Entfernen.
3. **[bestätigt] Nur deutsche Metadaten:** `<html lang="de">`, kein `hreflang`, Beschreibung und Open-Graph-Texte nur
   deutsch, obwohl ein Schalter die Seite auf Englisch umstellt.
4. **[Verdacht] Die Screenshots zeigen die englische Oberfläche**, die Seite ist standardmäßig deutsch. Entweder deutsche
   Bilder zusätzlich aufnehmen oder bewusst so lassen.
5. **Query-Builder-Screenshot** zeigt vor allem den FROM-Block. Ein Ausschnitt mit SELECT, WHERE und Ergebnis wäre
   aussagekräftiger.
6. **Messwerte in `website/public/benchmarks.json`** mit der aktuellen Version neu erheben (Skripte liegen im Ordner
   `website/`).

## 9. Offen aus dieser Sitzung

- Testdaten und Demo-Projekt für die Screenshots liegen unter `/tmp/emt` (Datenbank `shop`, Projekt `Shop`) und gehen beim
  Aufräumen von `/tmp` verloren. Wer die Screenshots erneuern will, sollte den Aufbau als Skript ins Repository legen
  (z. B. `website/screenshots/`).
- Der Aufbau-Skript-Entwurf liegt unter
  `/tmp/claude-1000/-home-benj-EasyMySQL/b418496d-335a-4e60-9790-696d5e7969b6/scratchpad/s/gen.py`.
