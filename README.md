# EasyMySQL

MariaDB-Datenbankserver und grafische Datenbankverwaltung für Windows in **einem** Programm.
Einmal installieren, starten, und `mysql -u root` funktioniert in der Eingabeaufforderung.

![ER-Diagramm](docs/er-diagramm.png)

## Installation

1. Unter [Releases](https://github.com/gravijet/EasyMySQL/releases) die Datei `EasyMySQL-Setup-x.y.z.exe` herunterladen und ausführen
   (enthält bereits alles, funktioniert auch ohne Internet).
   (Windows SmartScreen: *Weitere Informationen → Trotzdem ausführen*.)
2. EasyMySQL starten. Beim ersten Start wird der Datenbankserver automatisch eingerichtet.
3. In der Eingabeaufforderung (cmd):

   ```
   mysql -u root
   ```

Das Setup installiert bzw. setzt:

| Was | Wo |
|---|---|
| EasyMySQL | `C:\Program Files\EasyMySQL\EasyMySQL.exe` |
| MariaDB 11.8 LTS (Server und alle Kommandozeilenprogramme: `mysql`, `mariadb`, `mysqldump`, `mysqladmin`, ...) | `C:\Program Files\EasyMySQL\mariadb` |
| Umgebungsvariable `PATH` (damit `mysql` überall funktioniert) | `...\EasyMySQL\mariadb\bin` |
| Umgebungsvariablen `EASYMYSQL_HOME`, `MARIADB_HOME` | Installationsordner |
| MariaDB ODBC-Treiber 3.2 und ODBC-Datenquelle **EasyMySQL** (optional, z. B. für Excel/Access) | Systemweit |
| Microsoft Visual C++ Laufzeit | Systemweit |
| Datenbanken (Datenordner) | `C:\ProgramData\EasyMySQL\data` |

Zugang: Server `127.0.0.1`, Port `3306`, Benutzer `root`, **kein Passwort**.
Der Server ist nur vom eigenen PC aus erreichbar (`bind-address=127.0.0.1`).

## Datensicherheit, Sicherungen und Reparatur

- **Absturzsicher**: MariaDB läuft mit InnoDB und sofortigem Schreiben jeder bestätigten Änderung
  (`innodb_flush_log_at_trx_commit=1`, Doublewrite). Auch wenn EasyMySQL oder der PC hart beendet
  wird (Task-Manager, Stromausfall), gehen bestätigte Daten nicht verloren. Getestet mit
  `kill -9` mitten in zehntausenden Schreibvorgängen: keine einzige bestätigte Zeile fehlte.
- **Windows herunterfahren/abmelden**: EasyMySQL fährt die Datenbank vorher sauber herunter.
- **Nach einem Absturz** erkennt EasyMySQL das beim nächsten Start, MariaDB stellt die Daten
  automatisch wieder her und EasyMySQL prüft danach alle Tabellen.
- **Automatische Sicherungen** (*Server → Sicherungen & Reparatur*): beim Start und alle 2 Stunden,
  außerdem automatisch **vor jedem Löschen/Leeren** von Datenbanken und Tabellen (auch im
  SQL-Editor: `DROP`, `TRUNCATE`, `DELETE`/`UPDATE` ohne `WHERE`). Aufbewahrt werden die letzten 10
  und eine pro Tag für 14 Tage (einstellbar, Ordner frei wählbar, z. B. USB-Stick).
  Wiederherstellen per Klick, auch unter neuem Namen.
- **Reparieren per Knopfdruck**:
  - *Prüfen und reparieren*: prüft alle Tabellen (`CHECK TABLE`) und repariert beschädigte.
  - *MyISAM → InnoDB*: wandelt nicht absturzsichere Tabellen um (EasyMySQL weist darauf hin).
  - *Server retten*: wenn MariaDB gar nicht mehr startet. Der alte Datenordner bleibt erhalten,
    die Daten werden gerettet (notfalls aus der neuesten Sicherung) und in einen neuen
    Datenordner eingespielt.

## Verhalten

- Der Datenbankserver läuft **nur, solange EasyMySQL geöffnet ist**.
- Schließen mit **X** beendet das Programm nicht. Es läuft im Infobereich (neben der Uhr) weiter,
  damit `mysql -u root` weiter funktioniert.
- Richtig beenden: Rechtsklick auf das Symbol im Infobereich → **Beenden** (oder *Datei → Beenden*).
  Dabei wird der Server sauber heruntergefahren.
- **Kein Autostart** mit Windows.
- Es läuft immer nur eine EasyMySQL-Instanz. Ein zweiter Start holt das vorhandene Fenster nach vorne.

## Funktionen

Die Oberfläche ist wie Visual Studio Code aufgebaut (dunkles Design, helles Design in den
Einstellungen): links die Aktivitätsleiste und Seitenleiste, in der Mitte Registerkarten, unten die
Statusleiste.

- **Explorer mit Projekten**: Projekte liegen in `Dokumente\EasyMySQL\Projekte`. Jedes Projekt ist ein
  Ordner mit Unterordnern und `.sql`-Dateien. Dateien und Ordner anlegen, umbenennen und löschen
  per Rechtsklick. Die Datenbank wird je Datei gemerkt.
- **Automatisch speichern**: jede Änderung wird nach kurzer Pause sicher gespeichert (auch neue,
  noch unbenannte Dateien). *Frühere Fassungen* stellt ältere Stände einer Datei wieder her.
  Beim nächsten Start sind alle Registerkarten wieder da.
- **SQL-Editor**: Syntax-Hervorhebung, Zeilennummern, Minimap, Klammerpaare, Suchen/Ersetzen,
  Autovervollständigung für Befehle, Funktionen, Tabellen und Spalten (auch nach `alias.`),
  Zeilen verschieben/kopieren, Kommentieren, Schriftgröße einstellbar.
  - **Mehrere Anweisungen pro Datei**: `Strg+Alt+S` führt die ganze Datei aus, oder nur den
    markierten Teil. `Strg+Enter` führt nur die Anweisung aus, in der der Cursor steht.
  - Jede Anweisung bekommt ein **eigenes Ergebnis** bzw. eine Meldung (betroffene Zeilen, Dauer).
    Fehler werden rot unterstrichen und auf Deutsch erklärt. `DELIMITER` für Prozeduren/Trigger
    wird unterstützt.
  - `Strg+Alt+L` **formatiert** das SQL übersichtlich.
- **Visual Studio Code + GitHub Copilot**: „In VS Code öffnen“ bearbeitet eine Datei in VS Code.
  *Werkzeuge → VS Code einrichten* installiert SQLTools mit MariaDB-Treiber und legt die Verbindung
  „EasyMySQL“ an. GitHub Copilot ist in aktuellem VS Code eingebaut: einmal mit dem GitHub-Konto
  anmelden, dann schlägt Copilot beim Tippen SQL vor. Beim ersten Öffnen fragt VS Code, ob man dem
  Ordner vertraut → „Trust“ wählen. Speichert man die Datei in VS Code, übernimmt EasyMySQL die
  Änderungen automatisch.
- **Datenbank-Explorer**: Datenbanken → Tabellen → Spalten als Baum, Rechtsklick-Menüs.
- **Datenbanken und Tabellen erstellen**: Tabellen-Designer mit Datentypen, Primärschlüssel,
  AUTO_INCREMENT, NOT NULL, UNIQUE, Standardwerten und **Fremdschlüsseln (Beziehungen)**.
- **Daten ansehen und bearbeiten**: Doppelklick auf eine Zelle ändert den Wert, neue Zeilen einfügen,
  Zeilen löschen, Filter (WHERE), Sortierung, Blättern, CSV-Export.
- **Struktur ändern**: Spalten hinzufügen/ändern/löschen, Indizes, Fremdschlüssel, Tabelle umbenennen.
- **Abfrage-Assistent**: SELECT-Abfragen grafisch zusammenklicken: Spalten anhaken,
  Tabellen über Beziehungen verknüpfen (JOIN), Bedingungen, Sortierung, COUNT/SUM/AVG/MIN/MAX.
- **ER-Diagramm (Reverse Engineering)**: liest Tabellen, Spalten, Primär- und Fremdschlüssel einer
  bestehenden Datenbank aus und zeichnet sie mit Beziehungslinien.
  - **Beziehungsarten**: 1:1 (eindeutiger Fremdschlüssel), 1:n und n:m (Zwischentabelle) werden
    erkannt. Optionale Beziehungen (Fremdschlüssel darf leer sein) sind extra gekennzeichnet.
  - **Notation wählbar**: Krähenfuß (Standard, mit Beschriftung „1:n“, „1:1“, „n:m“ in der
    Linienmitte), Chen (1, n, m) oder (min,max).
  - **n:m zusammenfassen**: reine Zwischentabellen ausblenden und als direkte n:m-Linie zeigen.
  - **Automatisch anordnen**: Ebenen-Layout (referenzierte Tabellen links), Kreuzungen werden
    minimiert, lange Linien laufen über eigene Spuren um Tabellen herum, Linien am selben
    Anschluss werden aufgefächert.
  - **Beziehungen anlegen**: am Punkt ● neben einer Spalte ziehen und auf die Zielspalte fallen
    lassen (oder „+ Beziehung“). Dabei **1:n, 1:1 oder n:m** wählen: 1:1 macht die Spalte
    zusätzlich eindeutig, n:m legt die Zwischentabelle mit beiden Schlüsseln an.
    Rechtsklick auf eine Linie löscht sie.
  - **Alles anzeigen** zoomt passend, Maus über Tabelle/Linie hebt die Beziehungen hervor und
    erklärt sie (z. B. „1:n – ein Datensatz in kunde gehört zu vielen in bestellung“).
  - Kästen verschiebbar, Zoom, Export als **SVG**, Erzeugen des **SQL-Skripts** (CREATE TABLE ...).
- **SQL-Export/-Import**: ganze Datenbank als `.sql` sichern (Struktur + Daten) und wieder einspielen.
- **Server-Steuerung**: Starten, Stoppen, Neustart, Server-Log, Datenordner öffnen.

### Tastenkürzel

| Taste | Funktion |
|---|---|
| `Strg+Alt+S` (oder `F5`) | ganze Datei bzw. Markierung ausführen |
| `Strg+Enter` | Anweisung an der Cursorposition ausführen |
| `Strg+Alt+L` (oder `Strg+Umschalt+F`) | SQL formatieren |
| `Strg+S` | speichern (passiert auch automatisch) |
| `Strg+N` / `Strg+W` | neue Abfrage / Registerkarte schließen |
| `Strg+B` | Seitenleiste ein/aus |
| `Strg+Leertaste` | Vorschläge |
| `Strg+F` / `Strg+H` / `Strg+G` | Suchen / Ersetzen / Gehe zu Zeile |
| `Strg+#` | Zeilen aus-/einkommentieren |
| `Alt+↑/↓`, `Alt+Umschalt+↑/↓` | Zeile verschieben, Zeile kopieren |

![SQL-Abfrage](docs/sql-abfrage.png)

## Selbst bauen

Voraussetzung: [Rust](https://rustup.rs) (stable).

```
cargo build --release
```

Für das komplette Setup (MariaDB, Treiber, Inno Setup) siehe `.github/workflows/release.yml`.
Der Build holt automatisch die neueste MariaDB-Version der LTS-Reihe 11.8 (mit Prüfsumme); ist die
MariaDB-Schnittstelle nicht erreichbar, wird die fest hinterlegte Version 11.8.9 aus dem MariaDB-Archiv verwendet.
Das fertige Setup enthält MariaDB bereits vollständig und installiert komplett offline.
Nach einem Update auf eine neuere MariaDB-Version passt EasyMySQL die Systemtabellen beim Start automatisch an (`mariadb-upgrade`).
Jeder Push auf `main` baut das Setup automatisch auf GitHub und veröffentlicht es als
Release `vX.Y.Z` (Version aus `Cargo.toml`).

Zum Entwickeln unter Linux sucht EasyMySQL `mariadbd` in `/usr/sbin`. Mit der Umgebungsvariable
`EASYMYSQL_MARIADB_BIN` kann ein anderer MariaDB-`bin`-Ordner angegeben werden.

## Aufbau

| Datei | Inhalt |
|---|---|
| `src/main.rs` | Programmstart, Fenster, nur eine Instanz |
| `src/app.rs`, `src/app/shell.rs` | Hauptfenster im VS-Code-Stil: Menü, Aktivitätsleiste, Seitenleiste, Registerkarten, Dialoge |
| `src/workspace.rs` | Projekte, Dateibaum, sicheres Speichern, frühere Fassungen, Sitzung |
| `src/server.rs` | MariaDB einrichten, starten, sauber stoppen, Absturzerkennung, Rettung |
| `src/backup.rs`, `src/repair.rs` | Sicherungen (erstellen, aufräumen, wiederherstellen), Prüfen und Reparieren |
| `src/db.rs` | Verbindung, Abfragen, Anweisungen trennen, Metadaten, Beziehungsarten, SQL-Export |
| `src/tabs/` | SQL-Editor, Daten, Struktur, Tabellen-Designer, ER-Diagramm, Abfrage-Assistent, Sicherungen |
| `src/sqledit.rs` | Code-Editor: Hervorhebung, Autovervollständigung, Formatierung, Suchen/Ersetzen |
| `src/style.rs` | dunkles und helles Farbschema |
| `src/vscode.rs` | Anbindung an Visual Studio Code (SQLTools, Copilot) |
| `src/platform.rs` | Windows: Infobereich-Symbol, Fenster aus-/einblenden, Herunterfahren abfangen |
| `installer/EasyMySQL.iss` | Inno-Setup-Skript |

Lizenz: EasyMySQL unter MIT. MariaDB steht unter der GPL v2.
