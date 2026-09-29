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

## Verhalten

- Der Datenbankserver läuft **nur, solange EasyMySQL geöffnet ist**.
- Schließen mit **X** beendet das Programm nicht. Es läuft im Infobereich (neben der Uhr) weiter,
  damit `mysql -u root` weiter funktioniert.
- Richtig beenden: Rechtsklick auf das Symbol im Infobereich → **Beenden** (oder *Datei → Beenden*).
  Dabei wird der Server sauber heruntergefahren.
- **Kein Autostart** mit Windows.
- Es läuft immer nur eine EasyMySQL-Instanz. Ein zweiter Start holt das vorhandene Fenster nach vorne.

## Funktionen

- **Objekt-Explorer**: Datenbanken → Tabellen → Spalten als Baum, Rechtsklick-Menüs.
- **Datenbanken und Tabellen erstellen**: Tabellen-Designer mit Datentypen, Primärschlüssel,
  AUTO_INCREMENT, NOT NULL, UNIQUE, Standardwerten und **Fremdschlüsseln (Beziehungen)**.
- **Daten ansehen und bearbeiten**: Doppelklick auf eine Zelle ändert den Wert, neue Zeilen einfügen,
  Zeilen löschen, Filter (WHERE), Sortierung, Blättern, CSV-Export.
- **Struktur ändern**: Spalten hinzufügen/ändern/löschen, Indizes, Fremdschlüssel, Tabelle umbenennen.
- **SQL-Editor**: Syntax-Hervorhebung, Zeilennummern, Autovervollständigung für Befehle,
  Funktionen, Tabellen und Spalten (auch nach `alias.`), Strg+Leertaste für Vorschläge,
  **Formatieren** auf Knopfdruck (Strg+Umschalt+F), F5 oder Strg+Enter führt aus (auch nur den
  markierten Text), SQL-Dateien öffnen/speichern, Ergebnis als CSV.
- **Visual Studio Code + GitHub Copilot**: „In VS Code öffnen“ bearbeitet eine Abfrage in VS Code.
  *Werkzeuge → VS Code einrichten* installiert SQLTools mit MariaDB-Treiber und legt die Verbindung
  „EasyMySQL“ an (Ordner `Dokumente\EasyMySQL`). GitHub Copilot ist in aktuellem VS Code eingebaut:
  einmal mit dem GitHub-Konto anmelden, dann schlägt Copilot beim Tippen SQL vor. Beim ersten
  Öffnen fragt VS Code, ob man dem Ordner vertraut → „Trust“ wählen. Speichert man die Datei in
  VS Code, übernimmt EasyMySQL die Änderungen automatisch.
- **Abfrage-Assistent**: SELECT-Abfragen grafisch zusammenklicken: Spalten anhaken,
  Tabellen über Beziehungen verknüpfen (JOIN), Bedingungen, Sortierung, COUNT/SUM/AVG/MIN/MAX.
- **ER-Diagramm (Reverse Engineering)**: liest Tabellen, Spalten, Primär- und Fremdschlüssel einer
  bestehenden Datenbank aus und zeichnet sie mit Beziehungslinien (Krähenfuß-Notation).
  - **Automatisch anordnen**: Ebenen-Layout (referenzierte Tabellen links), Kreuzungen werden
    minimiert, lange Linien laufen über eigene Spuren um Tabellen herum
  - **Alles anzeigen** zoomt passend, Maus über Tabelle/Linie hebt die Beziehungen hervor
  - **Beziehungen anlegen**: am Punkt ● neben einer Spalte ziehen und auf die Zielspalte fallen
    lassen (oder „+ Beziehung“); Rechtsklick auf eine Linie löscht sie
  - Kästen verschiebbar, Zoom, Export als **SVG**, Erzeugen des **SQL-Skripts** (CREATE TABLE ...)
- **SQL-Export/-Import**: ganze Datenbank als `.sql` sichern (Struktur + Daten) und wieder einspielen.
- **Server-Steuerung**: Starten, Stoppen, Neustart, Server-Log, Datenordner öffnen.

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
| `src/app.rs` | Hauptfenster: Menü, Werkzeugleiste, Objekt-Explorer, Dialoge |
| `src/server.rs` | MariaDB einrichten, starten, stoppen |
| `src/db.rs` | Verbindung, Abfragen, Metadaten, SQL-Export |
| `src/tabs/` | SQL-Editor, Daten, Struktur, Tabellen-Designer, ER-Diagramm, Abfrage-Assistent |
| `src/sqledit.rs` | Code-Editor: Hervorhebung, Autovervollständigung, Formatierung |
| `src/vscode.rs` | Anbindung an Visual Studio Code (SQLTools, Copilot) |
| `src/platform.rs` | Windows: Infobereich-Symbol, Fenster aus-/einblenden |
| `installer/EasyMySQL.iss` | Inno-Setup-Skript |

Lizenz: EasyMySQL unter MIT. MariaDB steht unter der GPL v2.
