# EasyMySQL – offene Punkte

Stand: 2026-11-14, Version 1.1.19. Gesammelt beim Aufnehmen der Website-Screenshots und bei einer Durchsicht des Codes.
`cargo test --locked` läuft sauber, `cargo clippy --all-targets` ohne Warnungen. In 1.1.19 behobene Punkte sind mit
**[erledigt 1.1.19]** markiert; Abschnitt 10 listet die dabei zusätzlich gefundenen und behobenen Fehler.

Kennzeichnung: **[bestätigt]** = im Code oder in der laufenden App nachgewiesen, **[Verdacht]** = beim Lesen aufgefallen
und nicht ausprobiert, **[Idee]** = neue Funktion. Alles lief nur unter Linux.

## 1. Fehler (zuerst)

1. **[erledigt 1.1.19] Query Builder: Alias umbenennen bricht die Abfrage.** Verweise ziehen jetzt mit (auch beim
   Wechsel der Tabelle und in korrelierten Unterabfragen), Test `rename_source_updates_all_references`. Spalten und Join-Bedingungen speichern den
   Tabellennamen als Text (`Expr::Column { src, col }`, `src/tabs/builder/model.rs`). Wer den Alias ändert
   (`src/tabs/builder/mod.rs`, Zeile ~831), lässt alle Verweise auf dem alten Namen stehen, die SQL wird ungültig.
   Lösung: beim Ändern des Alias alle Verweise in Spalten, Bedingungen, Gruppierung und Sortierung mit umbenennen.
2. **[erledigt 1.1.19] Git-Diff zeigt nach dem Wiederöffnen den alten Stand.** Der Tab wird ersetzt, „Aktualisieren“ und
   ein geänderter Git-Status laden offene Diffs neu, der Diff-Tab hat einen eigenen Knopf. Zusätzlich teilten sich alle
   Commit-Diffs einen Schlüssel (ein zweiter Commit öffnete nie). Alter Befund: `open_tab` (`src/app.rs`, Zeile ~508) springt bei
   gleichem Schlüssel nur zum vorhandenen Tab; der neue Diff-Text wird verworfen (`src/app/git_panel.rs`, Zeilen ~584 und ~1054).
   Auch „Aktualisieren“ im Git-Bereich ändert das nicht. Lösung: den Inhalt des vorhandenen Tabs ersetzen.
3. **[erledigt 1.1.19] Query Builder: `COUNT` startet als `COUNT(*)`.** Bei LEFT/RIGHT JOIN schlägt „+ Funktion → COUNT“
   jetzt den Primärschlüssel der optionalen Tabelle vor; steht dort doch `COUNT(*)`, erscheint ein Hinweis. Bei einem LEFT JOIN zählt das Zeilen ohne Treffer als 1.
   Besser: eine Spalte der gejointen Tabelle vorschlagen oder bei LEFT JOIN einen Hinweis zeigen.
4. **[erledigt 1.1.19, war Testumgebung] Query Builder: Die Suchzeile im Menü „+ Spalte“ nimmt keine Tasten an.** Unter
   Xvfb ohne Fenstermanager hatte das Fenster keinen Tastaturfokus (`xdotool windowfocus` hilft). Mit Fokus filtert die Suche;
   sie hat jetzt beim Öffnen sofort den Fokus und wird beim Schließen geleert (Test `column_menu_search_takes_keys`).
5. **[erledigt 1.1.19] Query Builder: Ein Klick neben ein Feld klappte einen ganzen Abschnitt ein** – nur noch Pfeil,
   Schlüsselwort, Titel und Kurzfassung sind Klickziel (Test `section_header_click_target_is_the_title`). Alter Befund: (beim Verlassen eines
   Eingabefelds). Fehlklick-Falle; prüfen, ob die Überschrift zu viel Fläche als Klickziel hat.
6. **[wahrscheinlich Testumgebung] Eingaben kommen verzögert an.** Siehe 1.4 (fehlender X-Fokus unter Xvfb). Beim Tippen in ein Alias-Feld wurde aus „c“ ein „cc“. Kann an der
   Test-Umgebung (Xvfb, xdotool) liegen; mit echter Tastatur prüfen.
7. **[erledigt 1.1.19] Datenbankwahl pro Projekt statt „zuletzt benutzt“.** Ohne gemerkte Datenbank gilt jetzt ein
   `USE` am Dateianfang, sonst die im Projekt meistgewählte Datenbank, erst dann die zuletzt benutzte. Alter Befund: Beim Öffnen von `queries/revenue.sql` war „schule1“
   gewählt, obwohl die Datenbank des Projekts `shop` ist.
8. **[geprüft, kein Fehler] Teilstrings ohne Grenzprüfung.** Alle drei Stellen prüfen vorher ASCII bzw. die Länge. `src/sqledit/schema_fix.rs:138` rechnet `at - 1` ohne Prüfung auf `at == 0`;
   `src/backup.rs:175` schneidet `&name[..19]` und verlässt sich auf `is_backup_name` (nur sicher, wenn dort ASCII erzwungen wird);
   `src/transfer/import/mod.rs:166` schneidet `&s[19..]`. Alle drei mit Umlauten und leeren Eingaben testen.
9. **[erledigt 1.1.19] Zwei Clippy-Warnungen** (auch die in Tests): eine Funktion mit 8 statt 7 Argumenten und ein `Some(x).filter(|_| …)`.
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
5. **[erledigt] Website-CSP erlaubt `script-src 'unsafe-inline'`** (das einzige Inline-Skript der 404-Seite steht jetzt in `404.js`, die CSP erlaubt nur noch `'self'`) (`website/public/_headers`). Inline-Skripte auslagern
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
3. **[erledigt 1.1.19] `unreachable!` in Laufzeitcode** (jetzt Fehlermeldungen; das in `export.rs` ist wirklich unerreichbar): `src/tabs/code.rs:332` („nur Java ist ausführbar“) und
   `src/dev/install.rs:516`. Wird eine weitere Sprache ausführbar, stürzt die App dort ab; durch eine Fehlermeldung ersetzen.
4. **[erledigt 1.1.19] Download-Zeitgrenzen:** jetzt mit Grenzen für Namensauflösung, Verbindungsaufbau und erste Antwort;
   eine zu große Datei bricht mit Meldung ab statt still abgeschnitten zu werden. Alter Befund: `src/transfer/remote.rs` setzt eine Stunde als Gesamtgrenze, aber keine für
   Verbindungsaufbau oder Stillstand. Ein hängender Server blockiert bis zum Abbruch.
5. **[geprüft, kein Fehler] `thread::sleep` in `src/tabs/preview/pdf.rs:348,363`** steht nur in Tests. Prüfen, ob das im Oberflächen-Thread läuft (Ruckler).
6. **[Verdacht] Sehr große Tabellen:** mit mehreren Millionen Zeilen, großen BLOB-Spalten und 200 Spalten testen.
7. **[teilweise erledigt 1.1.19] Außerhalb der App geänderte Dateien:** Ungespeicherte Tabs laden alle 2 s neu (gab es schon).
   Neu: Hat der Tab ungespeicherte Änderungen, landet die fremde Fassung vor dem Überschreiben in „Frühere Fassungen“ und
   ein Hinweis erscheint. Offen: echte Rückfrage „Neu laden oder behalten?“. der offene Editor-Tab lud die Datei erst nach dem Schließen und
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

1. **[erledigt] `website/public/og.jpg` (Vorschaubild beim Teilen von Links) ist veraltet.** Neu aus der Seite selbst gerendert. Es zeigt die frühere Gestaltung
   mit Tabs und dem nackten Befehl `irm mysql.benjaminberger.at/install | iex`. Die Seite nutzt inzwischen
   `powershell -c "irm … | iex"`, weil uBlock die nackte Form beim Kopieren schluckt. Neu erzeugen (1200×630, Wortmarke
   wie auf der Seite, Befehl in der neuen Form).
2. **[erledigt] Übrig gebliebene Datei `repo/website/public/og.png`.** Entfernt, ebenso die unbenutzten `brands/*.svg` und `InstrumentSans`. Wird nirgends verwendet (die Seite verweist auf
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

## 10. In 1.1.19 zusätzlich gefunden und behoben

1. **Sicherheit, KI-Schnittstelle:** Der Lese-Schutz (`src/mcp/guard.rs`) übersah MariaDBs ausführbare Kommentare
   `/*M! … */`; `SELECT 1 /*M! ; DROP TABLE t */` galt als Leseabfrage. Außerdem wird jetzt auch ohne Backslash-Maskierung
   geprüft (Server mit `NO_BACKSLASH_ESCAPES`/`ANSI_QUOTES`).
2. **SQL-Formatierer verschluckte Code:** `SELECT a -- Kommentar` + neue Zeile `, b` wurde zu `SELECT a -- Kommentar, b`
   (Spalte b im Kommentar). Komma steht jetzt vor dem Kommentar; zusätzlich bleibt eine Anweisung unformatiert, wenn sich
   beim Formatieren die Bausteine ändern würden.
3. **Export „als INSERT“ veränderte Daten:** Der Text `null` wurde zu SQL-NULL, `1e5` in Textspalten zu 100000, Leerzeichen
   um Zahlen gingen verloren, `NaN`/`inf` ergaben ungültiges SQL (`db::data_lit`). Gleiches `NaN`-Problem in `smart_lit`
   und im Query Builder (`value_sql`).
4. **Import „Tabellen ersetzen“ konnte Daten verlieren:** Die alte Tabelle wurde vor dem Import gelöscht; scheiterte der
   Import, war beides weg. Jetzt erst in eine Hilfstabelle, Tausch nur bei Erfolg (Server-Test
   `replace_keeps_old_table_when_import_fails`).
5. **„Verbinden“ tat bei 127.0.0.1 nichts**, solange der eigene Server gestoppt war – ein anderer lokaler Server (z. B. XAMPP)
   war so nicht erreichbar, ohne jede Meldung.
6. **`DELIMITER` nach einer Anweisung in derselben Zeile** ging als SQL an den Server.
7. Query Builder: „+ Tabelle“ vergab doppelte Aliasse (`t2` auch wenn schon belegt); `COUNT` behält beim Funktionswechsel
   die Spalte; beschädigte gespeicherte Abfrage ohne Teil stürzte ab.
8. Liegengebliebene Debug-Ausgabe (`EMDBG`) im Explorer entfernt.

Weiter offen aus Abschnitt 3: vergiftete Sperren (3.1), verschluckte Schreibfehler (3.2), sehr große Tabellen (3.6).

## 11. Gestaltung (in 1.1.19 geprüft und behoben)

Alle Ansichten unter Linux (Xvfb, 1400×900) im dunklen und hellen Design durchgesehen und vergrößert verglichen.

1. **Umschalter (Sprache, Design, Assistent-Modus):** gebrochene Pixelbreiten, der Abstand der gewählten Fläche zum Rand war
   links 2 px, rechts 1 px und die Kanten unscharf. Jetzt auf ganze Pixel gerundet, ringsum gleich.
2. **Datenbankbaum:** die Markierung der gewählten Zeile begann mitten im Aufklapp-Pfeil. Jetzt liegt der Pfeil mit Luft
   innerhalb der Fläche.
3. **Tabellen (Daten, Ergebnisse):** Spaltenbreite wurde pauschal mit 7,5 px je Zeichen geschätzt, breite Werte stießen rechts
   an die Linie. Jetzt aus der echten Textbreite, links und rechts gleich viel Luft.
4. **Verlauf:** lange Anweisungen wurden hart am Rand der Seitenleiste abgeschnitten; jetzt mit „…“ und Randabstand.
5. **Claude/Codex-Seitenleiste:** als einzige ohne Innenabstand (Inhalt 6 px vom Rand, „…“-Knopf am rechten Rand, als
   gefüllter Standardknopf). Jetzt gleiche Ränder und Symbolknöpfe wie die anderen Seitenleisten.
6. **Einstellungen:** „Maus“ hatte einen Pfeil nach oben als Symbol; jetzt ein Maus-Symbol (Lucide).
7. **Dialoge:** Titel waren uneinheitlich groß (nur der Meldungsdialog hatte 15 px); jetzt überall gleich. „Neue Datenbank“:
   „Erstellen“ als Hauptknopf, bei leerem Namen gesperrt, Esc schließt.
8. **Leere Flächen:** Server-Log zeigte bei leerem Log nichts, „Noch kein Ergebnis“ im Assistenten und „Keine Verbindung“ in der
   Benutzerverwaltung standen oben links bzw. oben; jetzt mittig wie die übrigen Hinweise. „Nicht verbunden“ in der Seitenleiste
   mit Abstand zum Knopf und bündig zum Titel.
9. **Neue Abfrage:** der Editor hatte nach „+ Abfrage“/Strg+N keinen Fokus (der öffnende Klick nahm ihn sofort wieder weg),
   man musste erst hineinklicken. Jetzt kann man sofort tippen.

Geprüft und in Ordnung: Titelleiste, Einstellungen, Query Builder, Daten- und Ergebnistabelle, ER-Diagramm (mittig),
Sicherungen, Benutzerverwaltung, Git, Suche, Menüs.

## 12. Website, zweite Runde (2026-11-14)

1. **Hintergrund wurde hell:** Stand die Maus unten rechts (z. B. über „Datenschutz“), kippte die Lichtrichtung im Shader
   fast auf den Betrachter, und die ganze Fläche wurde grau (mittlere Helligkeit 44 statt 14). Das Licht kommt jetzt immer
   schräg von oben, die Maus dreht nur die Richtung. Die freie Fläche schimmert in den Spiegelfarben statt in Weiß.
2. **Leistung:** Wasser und Farbe der freien Fläche entstehen in kleinen Texturen (höchstens 240 Zeilen), nur das Relief in
   voller Auflösung; höchstens etwa 60 Bilder je Sekunde, außer während das Relief mitscrollt; Relief nur im Rechteck um
   die Schrift berechnet und erst nach dem Laden der Schrift (vorher zweimal); bei Größenänderungen erst, wenn es ruhig
   ist (Handy-Adressleiste); Scroll-Arbeit einmal je Bild; Bilder erst alle Lagen lesen, dann schreiben.
   Rauschen ohne `sin()` (gleichmäßig auch bei großen Koordinaten). Verlust des Grafikkontexts wird abgefangen.
3. **Tabellenkopf:** klebt ganz oben, genau so hoch wie die Leiste des Sprachschalters; vorher ein 122 px hoher dunkler
   Block. EasyMySQL-Spalte ohne runde Ecken. Auf 320 px breiten Handys lief „EasyMySQL“ in die Nachbarspalte; der
   Sprachschalter weicht, wenn er einen Spaltennamen verdecken würde.
4. **„Warum wechseln“** auf die sechs stärksten Gründe gekürzt (vorher 14).
5. **Installationsbefehl** brach auf schmalen Handys mitten im Wort um („instal“ / „l“); jetzt nur an Leerzeichen und vor
   `/install`.
6. **Hochzählende Zahlen** setzen am Ende den Wert in der jetzt gewählten Sprache (vorher den beim Start gemerkten Text;
   je nach Zeitpunkt eines Sprachwechsels konnte die alte Schreibweise stehen bleiben).
7. **Ohne WebGL** klebte die Überschrift am Satz darunter.


## 13. Dritte Runde (2026-11-15): Tempo, KI-Bereiche, Installer

**Erledigt:**
- KI-Bereiche (Claude, Codex) neu gestaltet:
  - Konto-Knopf mit Anmeldestand (`claude auth status`, `codex login status`), Anmelden und Abmelden.
  - Chatverlauf als eigene Ansicht: Suche, Gruppen nach Datum, Löschen; wird im Hintergrund gelesen.
  - Eigene Nachrichten als Blase, Werkzeuge als kompakte Zeilen, Codeblöcke mit Kopf.
  - Eingabekarte mit Ordner, Berechtigung (nur lesen / Dateien bearbeiten), Modell und Denkaufwand, Kontext-Ring.
  - Vorschläge im leeren Chat.
  - Tempo: 400 Nachrichten 0,14 ms je Bild, weil Nachrichten außerhalb des Bildes übersprungen werden. Der Antwortstand wird nur bei Änderungen kopiert, Animationen laufen mit 30 statt 60 Bildern je Sekunde.
- Editor:
  - Klammern, gleiche Namen und Suchtreffer werden nur noch für sichtbare Zeilen berechnet (5000 Zeilen: 1,9 → 0,5 ms je Bild).
  - SQL-Färbung zeilenweise mit Zwischenspeicher.
  - Geöffnete Datei startet oben statt mit leerer Fläche am Ende.
- Tabellen: Spaltenbreiten werden nur bei neuen Daten gemessen, unsichtbare Zellen nicht aufgebaut (Scrollen in 200 Spalten: Oberflächen-CPU −30 %).
- Linux/macOS: Ein zweiter Start reicht die Dateien an das laufende Fenster weiter (vorher zweiter Server auf denselben Daten).
- MariaDB brach bei langem Datenpfad ab („socket file path is too long“); der Socket liegt dann im Temp-Ordner.
- `install.sh` und `install.ps1` neu gestaltet: Logo mit Farbverlauf, Spinner, eigener Fortschrittsbalken, Zusammenfassung. Die Anzeige „alt → neu“ erscheint bei Updates.

**Offen:**
- Tippen in sehr großen Dateien: Jede Taste setzt den ganzen Text neu (egui-Galley, 5000 Zeilen ≈ 13 ms).
- Vergiftete Sperren (3.1) und verschluckte Schreibfehler (3.2).
- `install.ps1` ist nur unter PowerShell 7 (Linux) getestet, nicht unter Windows PowerShell 5.1.
