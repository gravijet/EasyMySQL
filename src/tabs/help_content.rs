// Inhalt des Handbuchs (Hilfe → Handbuch). Tastenkuerzel werden live aus der aktuellen
// Belegung angezeigt, "Oeffnen"-Knoepfe fuehren den beschriebenen Befehl direkt aus.

use crate::keymap::Cmd;

pub struct Chapter {
    pub title: &'static str,
    pub sections: &'static [Section],
}

pub struct Section {
    pub id: &'static str,
    pub title: &'static str,
    pub blocks: &'static [B],
}

pub enum Target {
    Cmd(Cmd),
    Settings(&'static str),
    /// anderer Abschnitt des Handbuchs
    Section(&'static str),
}

pub enum B {
    P(&'static str),
    List(&'static [&'static str]),
    Steps(&'static [&'static str]),
    /// Tabelle aus zwei Spalten
    Table(&'static [(&'static str, &'static str)]),
    /// Befehle mit ihren aktuellen Tastenkuerzeln
    Keys(&'static [Cmd]),
    /// Programmcode (Java u. a.) ohne SQL-Farben, mit Kopieren-Knopf
    Source(&'static str),
    Tip(&'static str),
    /// SQL-Beispiel (kann in eine neue Abfrage uebernommen werden)
    Code(&'static str),
    /// Befehl fuer die Eingabeaufforderung
    Shell(&'static str),
    Open(&'static str, Target),
}

use B::*;
use Target::*;

pub const CHAPTERS: &[Chapter] = &[
    Chapter {
        title: "Erste Schritte",
        sections: &[
            Section {
                id: "ueberblick",
                title: "Was EasyMySQL macht",
                blocks: &[
                    P("EasyMySQL enthält einen vollständigen MariaDB-Datenbankserver und eine Oberfläche, um damit zu arbeiten. \
                       Beim Start von EasyMySQL startet der Server automatisch auf Port 3306. Er läuft, solange EasyMySQL läuft."),
                    P("Solange der Server läuft, funktioniert in der Eingabeaufforderung (cmd):"),
                    Shell("mysql -u root"),
                    Table(&[
                        ("Server", "127.0.0.1 (nur vom eigenen PC erreichbar)"),
                        ("Port", "3306"),
                        ("Benutzer", "root, ohne Passwort"),
                    ]),
                    P("Andere Programme wie Excel oder Access erreichen die Datenbanken über die ODBC-Datenquelle „EasyMySQL“, sofern sie beim Setup \
                       mitinstalliert wurde."),
                ],
            },
            Section {
                id: "oberflaeche",
                title: "Aufbau des Fensters",
                blocks: &[
                    List(&[
                        "Titelleiste oben: Menü und Werkzeuge (ER-Diagramm, Abfrage-Assistent), ein farbiger Punkt für den Server (grün = läuft, gelb = startet/stoppt, rot = Fehler; Klick öffnet das Server-Log) und ganz rechts Minimieren, Maximieren und Schließen. Es gibt keine zusätzliche Fensterzeile des Betriebssystems; Ziehen an der freien Fläche verschiebt das Fenster, Doppelklick maximiert, die Fensterkanten ändern die Größe.",
                        "Aktivitätsleiste ganz links: schaltet die Seitenleiste um – Explorer, Suchen, Datenbanken, Verlauf. Die Seitenleiste lässt sich unter Einstellungen → Darstellung → Seitenleiste ein- und ausschalten.",
                        "Seitenleiste: Breite mit der Maus an der rechten Kante ziehen.",
                        "Registerkarten in der Mitte: SQL-Dateien, Daten, Struktur, Diagramme, Einstellungen und so weiter.",
                        "Ohne offene Registerkarte bleibt die Mitte leer. Doppelklick auf die leere Fläche öffnet eine neue Abfrage.",
                        "Kurze Meldungen erscheinen unten rechts und verschwinden von selbst (Klick schließt sie sofort). Fehler erscheinen als Fenster.",
                    ]),
                    Keys(&[Cmd::ToggleSidebar, Cmd::ShowExplorer, Cmd::ShowSearch, Cmd::ShowDatabases, Cmd::ShowHistory]),
                ],
            },
            Section {
                id: "terminal",
                title: "Terminal",
                blocks: &[
                    P("Das Terminal öffnet sich unter dem Editor, wie in Visual Studio Code. Es startet im Projektordner und kennt die MariaDB-Programme (mysql, mysqldump), auch ohne die Installation im PATH."),
                    List(&[
                        "Ansicht → Neues Terminal öffnet ein Terminal. Das Plus im Terminalbereich öffnet ein weiteres in einer neuen Registerkarte, der Pfeil daneben lässt die Shell wählen: PowerShell, Eingabeaufforderung oder die MariaDB-Konsole (mysql mit den Verbindungsdaten).",
                        "Mit der Maus markierter Text wird mit Strg+C kopiert, ohne Markierung unterbricht Strg+C das laufende Programm. Rechtsklick: Kopieren und Einfügen, Mausrad: durch die Ausgabe blättern.",
                        "Beim Ausblenden laufen die Shells weiter. Der Mülleimer beendet das aktive Terminal, das Kreuz an der Registerkarte ebenfalls.",
                        "Solange der Cursor im Terminal steht, gehören die Tasten der Shell. Nur das Ein- und Ausblenden und die Befehlsliste funktionieren weiter.",
                    ]),
                    Keys(&[Cmd::ToggleTerminal, Cmd::NewTerminal]),
                    Open("Terminal öffnen", Cmd(Cmd::NewTerminal)),
                ],
            },
            Section {
                id: "registerkarten",
                title: "Registerkarten",
                blocks: &[
                    List(&[
                        "Klick wählt aus, mittlere Maustaste oder das Kreuz schließt.",
                        "Rechtsklick: Schließen, Andere schließen, Rechts davon schließen, Alle Registerkarten schließen, bei Dateien auch Pfad kopieren und Im Datei-Explorer zeigen.",
                        "Eine laufende Abfrage erkennt man am drehenden Kreis statt des Kreuzes.",
                        "Datei → Alle Registerkarten schließen schließt alle Tabs; mit dem Befehl zum Wiederöffnen kommen sie einzeln zurück. Beim nächsten Start sind die noch offenen Registerkarten wieder da – auch unbenannte Abfragen und der Abfrage-Assistent mit seinem Inhalt.",
                    ]),
                    Keys(&[Cmd::CloseTab, Cmd::CloseAllTabs, Cmd::ReopenTab, Cmd::NextTab, Cmd::PrevTab]),
                ],
            },
            Section {
                id: "befehle",
                title: "Befehle und Schnellöffnen",
                blocks: &[
                    P("Die Befehlsliste enthält fast alles, was EasyMySQL kann. Ein paar Buchstaben tippen genügt, die Reihenfolge muss nur ungefähr stimmen \
                       („nab“ findet „Neue Abfrage“). Pfeiltasten wählen, Enter führt aus, Esc schließt."),
                    P("Schnellöffnen sucht dasselbe für Dateien im geöffneten Projekt. Beginnt die Eingabe mit „>“, zeigt die Liste wieder Befehle."),
                    Keys(&[Cmd::CommandPalette, Cmd::QuickOpen, Cmd::CheckUpdates]),
                    Open("Befehle öffnen", Cmd(Cmd::CommandPalette)),
                ],
            },
            Section {
                id: "beenden",
                title: "Schließen und Beenden",
                blocks: &[
                    P("Das Kreuz oben rechts am Fenster beendet EasyMySQL vollständig und fährt den Server sauber herunter. Ungespeicherte Dateien werden vorher abgefragt."),
                    List(&[
                        "Wer EasyMySQL im Hintergrund laufen lassen möchte (Server bleibt an), schaltet unter Einstellungen → Start „Im Infobereich weiterlaufen“ ein. Dann versteckt das Kreuz nur das Fenster; ein Klick auf das Symbol neben der Uhr holt es zurück, Rechtsklick → Beenden beendet alles.",
                        "Beim Herunterfahren oder Abmelden von Windows fährt EasyMySQL den Server vorher sauber herunter.",
                        "Es läuft immer nur ein EasyMySQL. Ein zweiter Start (auch ein Doppelklick auf eine .sql-Datei) holt das vorhandene Fenster nach vorne.",
                        "Einstellungen → Start → „Beim Start wieder öffnen“ stellt alle Registerkarten beim nächsten Start so wieder her, wie sie waren. Ungespeicherte Abfragen bleiben immer erhalten.",
                    ]),
                ],
            },
            Section {
                id: "dateien-ziehen",
                title: "Dateien ziehen",
                blocks: &[
                    List(&[
                        "Eine Datei aus dem Windows-Explorer auf das Fenster ziehen öffnet sie im Editor; ein Ordner wird zum Projekt.",
                        "Auf einen Ordner im Dateibaum gezogen, wird die Datei dorthin kopiert. Dateien und Ordner lassen sich auch innerhalb des Dateibaums auf einen Ordner ziehen, um sie zu verschieben.",
                        "Registerkarten lassen sich mit der Maus umsortieren. Sind viele offen, schrumpfen die inaktiven auf ihr Symbol (Name im Tooltip); der Pfeil rechts listet alle.",
                    ]),
                ],
            },
        ],
    },
    Chapter {
        title: "Projekte und Dateien",
        sections: &[
            Section {
                id: "projekte",
                title: "Projektordner",
                blocks: &[
                    P("Ein Projekt ist einfach ein Ordner mit SQL-Dateien und Unterordnern. Jeder beliebige Ordner kann geöffnet werden – auch auf einem \
                       USB-Stick oder in einem Cloud-Ordner."),
                    List(&[
                        "Datei → Ordner öffnen: vorhandenen Ordner als Projekt öffnen.",
                        "Datei → Zuletzt geöffnet: die letzten zwölf Projekte.",
                        "Das Menü mit den drei Punkten im Explorer bietet dasselbe.",
                    ]),
                    Keys(&[Cmd::OpenFolder]),
                    Open("Ordner öffnen", Cmd(Cmd::OpenFolder)),
                    Tip("Wo neue Projekte landen, legt der Speicherordner in den Einstellungen fest."),
                    Open("Speicherorte einstellen", Settings("speicherorte")),
                ],
            },
            Section {
                id: "explorer",
                title: "Explorer",
                blocks: &[
                    List(&[
                        "Klick auf eine Datei öffnet sie, Klick auf einen Ordner klappt ihn auf oder zu.",
                        "Die Symbole oben: neue Datei, neuer Ordner, aktualisieren, alle Ordner zuklappen, Projektmenü.",
                        "Rechtsklick auf Datei oder Ordner: umbenennen, löschen, Pfad kopieren, im Datei-Explorer zeigen.",
                        "Rechtsklick oder Doppelklick auf die freie Fläche: neue Datei oder neuer Ordner im Projekt.",
                        "Neue Dateien ohne Endung bekommen automatisch „.sql“.",
                        "Löschen verschiebt in den Ordner „.papierkorb“ im Projekt. Von dort lässt sich jede Datei zurückholen.",
                        "Die Datei der aktiven Registerkarte ist im Explorer hervorgehoben.",
                    ]),
                    Keys(&[Cmd::NewFile]),
                ],
            },
            Section {
                id: "speichern",
                title: "Speichern und frühere Fassungen",
                blocks: &[
                    P("Jede Änderung wird eine Sekunde nach dem letzten Tastendruck gespeichert. Dabei wird erst eine Zwischendatei geschrieben und dann \
                       umbenannt – bei einem Absturz bleibt immer entweder die alte oder die neue Fassung vollständig erhalten. Ein Punkt (●) im Namen der \
                       Registerkarte zeigt ungespeicherte Änderungen."),
                    List(&[
                        "Unbenannte Abfragen (Neue Abfrage) werden ebenfalls laufend gesichert und sind nach einem Neustart wieder da. Leere unbenannte Abfragen verschwinden beim Schließen.",
                        "Speichern bei einer unbenannten Abfrage fragt nach einem Namen und legt die Datei im Projekt an („Anderer Ort …“ für einen beliebigen Ordner).",
                        "Frühere Fassungen (Menü mit den drei Punkten in der Datei): höchstens alle 5 Minuten wird der vorherige Stand aufbewahrt, bis zu 50 je Datei. „Wiederherstellen“ setzt den Text zurück.",
                        "Wird eine offene Datei außerhalb geändert, übernimmt EasyMySQL den neuen Inhalt automatisch.",
                    ]),
                    Keys(&[Cmd::Save, Cmd::SaveAll, Cmd::NewQuery, Cmd::OpenFile]),
                ],
            },
            Section {
                id: "suche",
                title: "Suchen in Dateien",
                blocks: &[
                    P("Die Seitenleiste „Suchen“ durchsucht alle .sql-, .txt-, .md-, .csv- und .json-Dateien des Projekts. Die Suche beginnt ab zwei Zeichen \
                       beim Tippen. „Aa“ beachtet Groß- und Kleinschreibung. Ein Klick auf einen Treffer öffnet die Datei an dieser Zeile."),
                    Keys(&[Cmd::ShowSearch]),
                    Open("Suchen öffnen", Cmd(Cmd::ShowSearch)),
                ],
            },
        ],
    },
    Chapter {
        title: "SQL-Editor",
        sections: &[
            Section {
                id: "ausfuehren",
                title: "Ausführen",
                blocks: &[
                    P("Eine Datei darf beliebig viele Anweisungen enthalten, getrennt durch Semikolon."),
                    Table(&[
                        ("Ausführen", "die ganze Datei – oder nur den markierten Teil"),
                    ]),
                    Keys(&[Cmd::RunAll, Cmd::RunStatement, Cmd::Explain]),
                    P("Oben links in der Datei steht die Datenbank, in der ausgeführt wird. Sie wird je Datei gemerkt. Ein „USE name;“ im Skript wechselt sie ebenfalls."),
                    P("Jede Datei kann auf einem anderen Server laufen: Links neben der Datenbank steht „Standard-Server“ – das ist der Server aus den Einstellungen. Dort lassen sich gespeicherte Server wählen (Einstellungen → Verbindung → Weitere Server). Die Wahl wird je Datei gemerkt; Datenbanken, Vorschläge und Prüfung kommen dann von diesem Server."),
                    P("Prozeduren, Funktionen und Trigger brauchen ein anderes Trennzeichen:"),
                    Code("DELIMITER //\nCREATE PROCEDURE anzahl_kunden()\nBEGIN\n  SELECT COUNT(*) FROM kunde;\nEND //\nDELIMITER ;\n\nCALL anzahl_kunden();"),
                    Tip("Bei DROP, TRUNCATE sowie DELETE oder UPDATE ohne WHERE sichert EasyMySQL die betroffene Datenbank vorher automatisch. Schlägt diese Sicherung fehl, wird nichts ausgeführt."),
                ],
            },
            Section {
                id: "ergebnisse",
                title: "Ergebnisse und Meldungen",
                blocks: &[
                    List(&[
                        "Jede Anweisung mit Ergebnis bekommt einen eigenen Reiter („Ergebnis 2 · Z. 14 (37)“ = zweites Ergebnis, Anweisung ab Zeile 14, 37 Zeilen). Das Kreuz im Reiter schließt ihn.",
                        "„Meldungen“ zeigt jede Anweisung mit Dauer und betroffenen Zeilen. „Zeile n“ springt an die Stelle im Text.",
                        "Die Uhr rechts oben öffnet den Verlauf: Doppelklick fügt eine frühere Anweisung in den Editor ein, Rechtsklick führt sie erneut aus.",
                        "Nach einem Fehler wird abgebrochen. Die Fehlerzeile ist rot unterstrichen, zu häufigen Fehlern steht ein Hinweis in der gewählten Sprache dabei.",
                        "Angezeigt werden höchstens 50 000 Zeilen je Ergebnis.",
                        "Die Trennlinie zwischen Editor und Ergebnis lässt sich verschieben.",
                        "Rechtsklick auf eine Zelle: Wert oder Zeile kopieren. Spaltenbreiten lassen sich ziehen.",
                    ]),
                    P("Export (Knopf über dem Ergebnis): kopieren für Excel, als INSERT-Anweisungen oder Markdown-Tabelle kopieren, als CSV (Semikolon, wie im deutschen Excel), JSON oder SQL speichern."),
                ],
            },
            Section {
                id: "vorschlaege",
                title: "Vorschläge beim Tippen",
                blocks: &[
                    P("Beim Tippen erscheinen Vorschläge. Pfeiltasten wählen, Enter oder Tab übernimmt, Esc schließt. Ist das Wort schon vollständig getippt, bleibt Enter ein Zeilenumbruch."),
                    P("Der Editor schlägt vor, was an der Stelle passt:"),
                    List(&[
                        "Nach FROM, JOIN, INTO und UPDATE kommen die Tabellen, nach SELECT, WHERE, ON und ORDER BY die Spalten der Tabellen aus der Anweisung (mit Datentyp). Nach „alias.“ oder „tabelle.“ erscheinen deren Spalten.",
                        "Nach ON schlägt der Editor die fertige Join-Bedingung aus den Fremdschlüsseln vor, z. B. s.klasse_id = k.id.",
                        "Nach einer Tabelle folgen passende Fortsetzungen (WHERE, LEFT JOIN, GROUP BY …). In CREATE TABLE werden Datentypen und Einschränkungen angeboten, nach USE die Datenbanken.",
                        "Am Anfang einer Anweisung gibt es Vorlagen (SELECT * FROM, INSERT INTO … VALUES, CREATE TABLE …); der Cursor springt an die erste Stelle zum Ausfüllen.",
                        "Namen behalten die Schreibweise der Datenbank, und gleiche Groß-/Kleinschreibung steht weiter oben. SQL-Wörter und Funktionen übernehmen deine Schreibweise: „sel“ ergibt select, „SEL“ ergibt SELECT. Funktionen werden mit Klammern eingefügt.",
                    ]),
                    Keys(&[Cmd::Suggest]),
                    P("In den Einstellungen lassen sich die automatischen Vorschläge abschalten – dann nur noch per Tastenkürzel."),
                    Open("Editor-Einstellungen", Settings("editor")),
                ],
            },
            Section {
                id: "bearbeiten",
                title: "Zeilen bearbeiten",
                blocks: &[
                    Table(&[
                        ("Zeile ausschneiden", "ohne Markierung die ganze Zeile (in die Zwischenablage), mit Markierung normales Ausschneiden"),
                        ("Zeile duplizieren", "Kopie darunter, der Cursor bleibt, wo er ist"),
                        ("Zeile löschen", "ohne Zwischenablage"),
                        ("Zeile markieren", "markiert die ganze aktuelle Zeile bzw. alle Zeilen der Markierung"),
                        ("Zeile verschieben", "aktuelle bzw. markierte Zeilen nach oben/unten"),
                        ("Zeile kopieren", "Kopie oberhalb bzw. unterhalb, Cursor wandert mit"),
                        ("Einrücken", "bei mehrzeiliger Markierung zwei Leerzeichen je Zeile"),
                        ("Kommentar ein/aus", "setzt oder entfernt „-- “ vor den Zeilen"),
                    ]),
                    Keys(&[
                        Cmd::CutLine,
                        Cmd::DuplicateLine,
                        Cmd::DeleteLine,
                        Cmd::SelectLine,
                        Cmd::MoveLineUp,
                        Cmd::MoveLineDown,
                        Cmd::CopyLineUp,
                        Cmd::CopyLineDown,
                        Cmd::Indent,
                        Cmd::Outdent,
                        Cmd::ToggleComment,
                    ]),
                    P("Klammern und Anführungszeichen werden automatisch geschlossen, das Gegenstück einer Klammer am Cursor wird hervorgehoben. Rückschritt zwischen einem leeren Paar entfernt beide."),
                ],
            },
            Section {
                id: "suchen",
                title: "Suchen, Ersetzen, Gehe zu",
                blocks: &[
                    List(&[
                        "Suchen öffnet eine Leiste über dem Text; markierter Text wird zum Suchbegriff. Enter springt zum nächsten Treffer, alle Treffer sind hervorgehoben.",
                        "„Aa“ beachtet Groß- und Kleinschreibung.",
                        "Ersetzen: einzeln oder alle auf einmal.",
                        "Gehe zu Zeile: Zeilennummer eingeben, Enter.",
                        "Esc schließt die Leisten, der Cursor bleibt im Text.",
                    ]),
                    Keys(&[Cmd::Find, Cmd::Replace, Cmd::GotoLine]),
                ],
            },
            Section {
                id: "formatieren",
                title: "SQL formatieren",
                blocks: &[
                    P("Formatieren schreibt SQL-Wörter groß und setzt jede Klausel (SELECT, FROM, JOIN, WHERE, GROUP BY, ORDER BY, …) an den Zeilenanfang. \
                       Spaltenlisten bleiben in einer Zeile und werden erst umbrochen, wenn die Zeile länger als 100 Zeichen würde. Lange WHERE-Bedingungen \
                       werden an AND/OR umbrochen, CREATE TABLE bekommt eine Spalte je Zeile."),
                    List(&[
                        "Spaltennamen wie „date“ oder „text“ bleiben klein; Datentypen werden nur in CREATE/ALTER großgeschrieben.",
                        "Kommentare, Texte in Anführungszeichen und DELIMITER-Abschnitte bleiben unverändert, ebenso Prozeduren und Trigger (BEGIN … END).",
                        "Zweimal formatieren ändert nichts mehr.",
                    ]),
                    Code("SELECT k.nachname, COUNT(*) AS anzahl\nFROM kunde k\nLEFT JOIN bestellung b ON b.kunde_id = k.id\nWHERE k.ort IN ('Berlin', 'Hamburg')\nGROUP BY k.nachname\nORDER BY anzahl DESC;"),
                    Keys(&[Cmd::Format]),
                ],
            },
            Section {
                id: "sql-pruefen",
                title: "Fehler erkennen und korrigieren",
                blocks: &[
                    P("Beim Tippen prüft der Editor die Anweisung. Fehler sind rot, Hinweise gelb unterkringelt, am Zeilenrand steht ein Punkt. Fährt man mit der Maus darüber, erscheint die Erklärung und, falls möglich, ein Vorschlag."),
                    List(&[
                        "Erkannt werden Tippfehler in Schlüsselwörtern (SELCT, FORM, WEHRE), fehlende Klammern, Anführungszeichen, Kommas und Semikolons, == statt = und = NULL statt IS NULL.",
                        "Ist eine Datenbank gewählt, findet die Prüfung auch unbekannte Tabellen und Spalten und schlägt ähnliche Namen vor („Meinten Sie vorname?“).",
                        "Der Knopf „Korrigieren“ oder das Tastenkürzel bessert alle Fehler mit Vorschlag selbst aus. Danach kurz prüfen, ob die Anweisung noch das Gewünschte tut.",
                        "Prozeduren und Trigger werden nicht geprüft. Unter Einstellungen → Editor lässt sich die Prüfung ausschalten.",
                    ]),
                    Keys(&[Cmd::FixAll]),
                    Open("Einstellungen zum Editor", Settings("editor")),
                ],
            },
            Section {
                id: "maus",
                title: "Scrollen mit der Maus",
                blocks: &[
                    List(&[
                        "Die Scrollgeschwindigkeit des Mausrads ist einstellbar (Standard 1,5-fach).",
                        "Mittlere Maustaste (Mausrad) gedrückt halten und die Maus bewegen: die Ansicht scrollt in diese Richtung, je weiter vom Ausgangspunkt, desto schneller. Loslassen beendet das Scrollen.",
                        "Im ER-Diagramm verschiebt die mittlere Maustaste stattdessen die Ansicht, auf Registerkarten schließt ein Mittelklick.",
                    ]),
                    Open("Maus-Einstellungen", Settings("maus")),
                ],
            },
        ],
    },
    Chapter {
        title: "Datenbanken und Tabellen",
        sections: &[
            Section {
                id: "datenbanken",
                title: "Seitenleiste Datenbanken",
                blocks: &[
                    List(&[
                        "Datenbanken aufklappen zeigt die Tabellen, Tabellen aufklappen die Spalten mit Datentyp. PK = Primärschlüssel, FK = Fremdschlüssel. Sichten sind kursiv.",
                        "Doppelklick auf eine Tabelle öffnet ihre Daten.",
                        "Rechtsklick auf eine Tabelle: Daten, Struktur, SELECT, Name kopieren, Java-Klasse, Exportieren, Umbenennen, Kopieren, Zusammenfügen, Leeren, Löschen.",
                        "Rechtsklick auf eine Datenbank: neue Abfrage, neue Tabelle, ER-Diagramm, Abfrage-Assistent, Exportieren, Löschen.",
                        "Die zuletzt angeklickte Datenbank ist fett und wird für neue Abfragen verwendet.",
                        "„+“ legt eine neue Datenbank an (mit Sortierung, z. B. utf8mb4_german2_ci für deutsche Sortierung von Umlauten).",
                    ]),
                    Tip("Vor dem Leeren oder Löschen wird automatisch gesichert."),
                    Open("Datenbanken zeigen", Cmd(Cmd::ShowDatabases)),
                ],
            },
            Section {
                id: "daten",
                title: "Daten und Struktur direkt bearbeiten",
                blocks: &[
                    List(&[
                        "WHERE: eine Bedingung eingeben (z. B. preis > 10) und Enter drücken. Es werden je 500 Zeilen gezeigt, < und > blättern.",
                        "Doppelklick auf eine Zelle: bearbeiten, Enter speichert sofort, Esc bricht ab. Rechtsklick: kopieren, auf NULL setzen, Zeile löschen.",
                        "„+ Zeile“ legt sofort eine Zeile mit Standardwerten an und springt dorthin; sind Pflichtwerte nötig, fragt ein Dialog danach.",
                        "Klick auf eine Spaltenüberschrift öffnet das Spaltenmenü: umbenennen, Datentyp ändern, nach links/rechts/an den Anfang/ans Ende verschieben, kopieren, neue Spalte danach, analysieren, sortieren, löschen. „+ Spalte“ fügt eine Spalte am Ende hinzu.",
                        "„Spalte analysieren“ zeigt Anzahl, leere und verschiedene Werte, kleinsten und größten Wert, den Durchschnitt und die häufigsten Werte; ein Klick auf einen Wert zeigt genau diese Zeilen.",
                        "Strg+Z (oder der Pfeil in der Leiste) macht die letzte Änderung rückgängig: Werte, gelöschte Zeilen. Tabellen mit Binärdaten sind davon ausgenommen.",
                        "Hat eine Tabelle keinen Primärschlüssel, werden Änderungen über alle Spaltenwerte der Zeile zugeordnet.",
                    ]),
                ],
            },
            Section {
                id: "struktur",
                title: "Struktur ändern",
                blocks: &[
                    List(&[
                        "Spalten hinzufügen, ändern (Name, Datentyp, NULL, Standardwert, Kommentar, Position) und löschen.",
                        "Indizes anlegen (normal oder eindeutig, auch über mehrere Spalten) und löschen.",
                        "Fremdschlüssel anlegen und löschen, mit Verhalten beim Löschen und Ändern (RESTRICT, CASCADE, SET NULL, NO ACTION).",
                        "Auch eingehende Fremdschlüssel anderer Tabellen werden angezeigt.",
                        "Die Pfeile über der Liste verschieben die gewählte Spalte nach oben oder unten.",
                        "Tabelle umbenennen. Unten steht die vollständige CREATE-Anweisung.",
                    ]),
                ],
            },
            Section {
                id: "designer",
                title: "Neue Tabelle (Tabellen-Designer)",
                blocks: &[
                    P("Datenbank → Neue Tabelle. Je Spalte: Name, Datentyp (Liste oder eigener Typ wie DECIMAL(8,3)), Primärschlüssel, AUTO_INCREMENT, NOT NULL, \
                       UNIQUE, Standardwert, Kommentar und optional ein Verweis auf eine andere Tabelle (Fremdschlüssel). „SQL anzeigen“ zeigt die \
                       CREATE-Anweisung, „Im SQL-Editor öffnen“ übernimmt sie in eine neue Abfrage."),
                ],
            },
            Section {
                id: "tabellenaktionen",
                title: "Tabellen kopieren, umbenennen, zusammenfügen",
                blocks: &[
                    List(&[
                        "Rechtsklick auf eine Tabelle: Umbenennen, Kopieren (mit oder ohne Daten, samt Indizes), Zusammenfügen.",
                        "Zusammenfügen erzeugt aus zwei Tabellen eine neue: „Zeilen untereinander“ (UNION ALL, Spalten gleichen Namens werden zusammengelegt, fehlende bleiben leer) oder „Spalten nebeneinander“ (JOIN über gleiche Werte, vorgeschlagen werden Fremdschlüssel).",
                        "Das erzeugte SELECT wird angezeigt. „In Editor öffnen“ übernimmt die Anweisung zum Anpassen.",
                    ]),
                ],
            },
            Section {
                id: "export",
                title: "Exportieren",
                blocks: &[
                    List(&[
                        "Datenbank → Exportieren oder Rechtsklick auf eine Datenbank bzw. Tabelle. Tabellen: alle oder eine Auswahl.",
                        "In eine Datei: SQL (für MariaDB/MySQL, mit Sichten und Beziehungen), JSON, CSV (eine Datei je Tabelle, Semikolon, UTF-8), XML, eine SQLite-Datei, ein SQL-Skript für PostgreSQL, JSON-Zeilen für MongoDB (mongoimport), eine Redis-Datei (dump.rdb) oder Redis-Befehle für redis-cli.",
                        "Auf einen anderen Server: MariaDB/MySQL, PostgreSQL oder Redis. Dafür Server, Port, Benutzer und Passwort eintragen; auf Wunsch mit Verschlüsselung (TLS) und über einen SSH-Tunnel mit Schlüsseldatei oder Passwort.",
                        "Redis kennt keine Tabellen: Jede Zeile wird ein Hash mit dem Namen „tabelle:schlüssel“ (der Schlüssel ist der Primärschlüssel, sonst eine Zeilennummer).",
                        "Die Daten werden Zeile für Zeile geschrieben, auch riesige Tabellen belegen kaum Speicher. Der Export läuft im Hintergrund, zeigt den Fortschritt und lässt sich abbrechen.",
                        "Einzelne Ergebnisse: Export-Knopf über der Tabelle.",
                    ]),
                    Keys(&[Cmd::Export]),
                    Open("Exportieren", Cmd(Cmd::Export)),
                ],
            },
            Section {
                id: "import",
                title: "Importieren",
                blocks: &[
                    List(&[
                        "Datenbank → Importieren (oder eine Datei auf die Registerkarte ziehen). Quellen: SQLite-Datei, MongoDB-Export, CSV-Dateien, Redis (Datei oder Server), ein anderer MariaDB/MySQL-Server und ein PostgreSQL-Server.",
                        "Jede Quelle gibt es auch von einem anderen Rechner: als Datei aus dem Netz (Adresse mit http:// oder https://), als Datei auf einem Server (per SSH, mit Schlüsseldatei oder Passwort) und, wo es passt, als Server mit Port, Benutzer, Passwort, Verschlüsselung (TLS) und SSH-Tunnel.",
                        "Spaltentypen werden aus den Werten bestimmt (Zahlen, Datum, Text, JSON). Tabellennamen lassen sich ändern, Tabellen abwählen.",
                        "SQLite: Primärschlüssel, AUTO_INCREMENT und Beziehungen werden übernommen.",
                        "MongoDB: Exporte von mongoexport (JSON oder eine Zeile je Dokument) und mongodump (.bson). Verschachtelte Felder landen in einer JSON-Spalte, _id ist der Primärschlüssel.",
                        "CSV: Trennzeichen und Kodierung (UTF-8 oder Windows) werden erkannt, die erste Zeile ist die Kopfzeile.",
                        "Redis: Strings, Hashes, Listen, Sets und Sorted Sets werden zu den Tabellen redis_strings, redis_hashes, redis_lists, redis_sets und redis_zsets. Gelesen wird ein laufender Server oder eine RDB-Datei (dump.rdb).",
                        "MariaDB/MySQL und PostgreSQL: Nach „Verbinden“ eine Datenbank des Servers wählen. Tabellen werden samt Schlüsseln und Beziehungen übernommen (PostgreSQL-Typen werden nach MariaDB übersetzt).",
                        "Ziel ist eine neue oder eine vorhandene Datenbank. Gleichnamige Tabellen werden übersprungen, sofern „ersetzen“ nicht angehakt ist.",
                        "SQL-Dateien: Datenbank → SQL-Datei importieren führt die Datei sofort aus.",
                    ]),
                    Tip("Ein Import läuft im Hintergrund und lässt sich abbrechen. Eine fehlgeschlagene Tabelle wird wieder entfernt."),
                    Open("Importieren", Cmd(Cmd::Import)),
                ],
            },
        ],
    },
    Chapter {
        title: "Abfrage-Assistent",
        sections: &[
            Section {
                id: "assistent",
                title: "Aufbau",
                blocks: &[
                    P("Der Assistent baut SELECT-Abfragen aus Bausteinen zusammen – von „alle Spalten einer Tabelle“ bis zu verschachtelten Unterabfragen mit \
                       WITH, UNION und Fensterfunktionen. Unten links steht das erzeugte SQL (formatiert), rechts das Ergebnis nach „Ausführen“."),
                    List(&[
                        "Jeder Abschnitt (FROM, SELECT, WHERE, GROUP BY, ORDER BY, LIMIT) lässt sich über das Dreieck ein- und ausklappen.",
                        "Verschachtelte Abfragen haben einen farbigen Rand links; jede Ebene eine andere Farbe.",
                        "Der Inhalt wird laufend gespeichert und ist nach einem Neustart wieder da. „Neu“ beginnt von vorn.",
                        "„Im Editor öffnen“ übernimmt das SQL in eine neue Abfrage, „Kopieren“ in die Zwischenablage.",
                        "„Als Sicht speichern“ legt eine Sicht (CREATE OR REPLACE VIEW) in der Datenbank an.",
                    ]),
                    Keys(&[Cmd::QueryBuilder, Cmd::RunAll]),
                    Open("Abfrage-Assistent öffnen", Cmd(Cmd::QueryBuilder)),
                ],
            },
            Section {
                id: "assistent-mehr",
                title: "Daten ändern, Rückgängig, Live-Vorschau",
                blocks: &[
                    List(&[
                        "Oben wechselt der Assistent zwischen Abfragen, Einfügen, Ändern und Löschen. Einfügen füllt Zeilen aus Werten (leer = DEFAULT) oder holt sie mit einer Abfrage (INSERT … SELECT). Ändern setzt Spalten auf neue Werte, Löschen entfernt Zeilen; beide nutzen dieselben Bedingungen wie SELECT.",
                        "Anweisungen, die Daten ändern, werden erst nach einer Rückfrage ausgeführt; vor dem Löschen entsteht automatisch eine Sicherung. Fehlt die Bedingung, warnt der Assistent, dass alle Zeilen betroffen sind.",
                        "Rückgängig und Wiederholen (Strg+Z, Strg+Y) gehen Schritt für Schritt durch deine Änderungen im Assistenten.",
                        "Die Live-Vorschau führt die Abfrage kurz nach der letzten Änderung aus und zeigt höchstens 200 Zeilen. Der Knopf mit der Lupe zeigt den Ausführungsplan (EXPLAIN).",
                        "Die Klammern-Taste kopiert die Abfrage als fertige Java-Klasse (JDBC).",
                    ]),
                ],
            },
            Section {
                id: "assistent-from",
                title: "FROM und JOIN",
                blocks: &[
                    P("Die erste Zeile ist die Haupttabelle. „+ Tabelle“ bietet zuerst die Tabellen an, die über Fremdschlüssel verbunden sind – die ON-Bedingung \
                       wird dann automatisch ausgefüllt. Wird dieselbe Tabelle zweimal gebraucht, bekommt sie automatisch einen Alias."),
                    Table(&[
                        ("INNER JOIN", "nur Zeilen, die in beiden Tabellen einen Partner haben"),
                        ("LEFT JOIN", "alle Zeilen der linken Tabelle, fehlende Partner als NULL"),
                        ("RIGHT JOIN", "alle Zeilen der rechten Tabelle"),
                        ("CROSS JOIN", "jede Zeile mit jeder"),
                        ("NATURAL JOIN", "über alle gleichnamigen Spalten (ohne ON)"),
                    ]),
                    List(&[
                        "Als Quelle geht eine Tabelle, eine WITH-Abfrage oder eine Unterabfrage (abgeleitete Tabelle, braucht einen Alias).",
                        "AS: Alias, unter dem die Spalten angesprochen werden (k.nachname statt kunde.nachname).",
                        "ON-Bedingungen funktionieren wie WHERE (mehrere, mit UND/ODER). „USING …“ verknüpft stattdessen über gleichnamige Spalten.",
                    ]),
                ],
            },
            Section {
                id: "assistent-ausdruecke",
                title: "Spalten und Ausdrücke",
                blocks: &[
                    P("Ohne Einträge unter SELECT liefert die Abfrage alle Spalten (*). Jeder Eintrag ist ein Ausdruck mit optionalem Namen (AS). \
                       Die Art des Ausdrucks wählt das kleine Auswahlfeld davor:"),
                    Table(&[
                        ("Spalte", "eine Spalte oder tabelle.* – auch aus äußeren Abfragen (korrelierte Unterabfrage) oder ein Alias ohne Tabelle"),
                        ("Wert", "Zahl, Text, NULL, TRUE/FALSE, @variable. Text wird automatisch in Anführungszeichen gesetzt"),
                        ("Funktion", "über 60 Funktionen nach Gruppen (Zusammenfassen, Fenster, Text, Zahl, Datum, Logik) oder jede andere per Name; Argumente sind wieder Ausdrücke"),
                        ("Rechnung", "zwei Ausdrücke mit + − * / DIV %"),
                        ("CASE", "WHEN Bedingung THEN Wert … ELSE Wert END"),
                        ("CAST", "Umwandlung in CHAR, SIGNED, DECIMAL, DATE, …"),
                        ("Unterabfrage", "eine Abfrage, die genau einen Wert liefert"),
                        ("SQL", "beliebiger SQL-Text, wenn nichts anderes passt"),
                    ]),
                    P("Wechselt man die Art, wird der bisherige Ausdruck wo möglich übernommen: aus einer Spalte wird bei „Funktion“ COUNT(spalte), \
                       bei „Rechnung“ spalte + 1, bei „CAST“ CAST(spalte AS CHAR). Die Funktion wählt man danach über ihren Namen."),
                    P("Zusammenfassungen (COUNT, SUM, AVG, MIN, MAX, GROUP_CONCAT …) haben DISTINCT. Mit „OVER“ wird jede Zusammenfassung zur Fensterfunktion mit \
                       PARTITION BY, ORDER BY und Rahmen – dazu ROW_NUMBER, RANK, DENSE_RANK, NTILE, LAG, LEAD, FIRST_VALUE usw."),
                    Code("SELECT k.ort, k.nachname, SUM(b.betrag) AS umsatz,\n  RANK() OVER (PARTITION BY k.ort ORDER BY SUM(b.betrag) DESC) AS platz\nFROM kunde k\nJOIN bestellung b ON b.kunde_id = k.id\nGROUP BY k.ort, k.nachname;"),
                ],
            },
            Section {
                id: "assistent-where",
                title: "Bedingungen (WHERE, ON, HAVING)",
                blocks: &[
                    P("Eine Bedingung vergleicht einen Ausdruck: = <> < <= > >= <=> LIKE, NOT LIKE, REGEXP, IN, NOT IN, BETWEEN, NOT BETWEEN, IS NULL, IS NOT NULL."),
                    List(&[
                        "Mehrere Bedingungen: oben UND (alle müssen gelten) oder ODER (eine genügt), dazu NICHT.",
                        "„+ Gruppe“ fügt eine Klammer ein – darin gilt die jeweils andere Verknüpfung, beliebig tief.",
                        "„Unterabfrage“ neben dem Vergleich: IN (SELECT …) oder Vergleich mit einer Unterabfrage, wahlweise mit ANY (mindestens ein Wert) oder ALL (alle Werte).",
                        "„+ EXISTS“: gilt, wenn die Unterabfrage mindestens eine Zeile liefert (bzw. keine bei NOT EXISTS).",
                        "„+ SQL“: Bedingung als freier Text.",
                        "LIKE: % steht für beliebig viele Zeichen, _ für genau eines ('M%' = beginnt mit M).",
                    ]),
                    Code("SELECT * FROM artikel a\nWHERE a.preis > ALL (SELECT AVG(preis) FROM artikel)\n  AND NOT EXISTS (SELECT 1 FROM bewertung b WHERE b.artikel_id = a.id);"),
                ],
            },
            Section {
                id: "assistent-gruppen",
                title: "GROUP BY, HAVING, ORDER BY, LIMIT",
                blocks: &[
                    List(&[
                        "GROUP BY fasst Zeilen mit gleichen Werten zusammen. Stehen Zusammenfassungen und normale Spalten im SELECT, weist der Assistent auf fehlende Spalten in GROUP BY hin – „Übernehmen“ ergänzt sie.",
                        "WITH ROLLUP fügt Zwischen- und Gesamtsummen hinzu.",
                        "HAVING filtert nach dem Gruppieren, z. B. COUNT(*) > 1.",
                        "ORDER BY: mehrere Sortierungen, auf- oder absteigend; auch nach den Namen (AS) aus SELECT.",
                        "LIMIT begrenzt die Zeilenzahl, OFFSET überspringt Zeilen (Blättern).",
                    ]),
                ],
            },
            Section {
                id: "assistent-mengen",
                title: "UNION, EXCEPT, INTERSECT",
                blocks: &[
                    P("„+ UNION / EXCEPT / INTERSECT“ hängt eine weitere Teilabfrage an (als Kopie der ersten, damit die Spaltenzahl stimmt). \
                       Alle Teile müssen gleich viele Spalten liefern; die Namen kommen aus dem ersten Teil."),
                    Table(&[
                        ("UNION", "Zeilen aus beiden Teilen, doppelte nur einmal"),
                        ("UNION ALL", "Zeilen aus beiden Teilen, doppelte bleiben (schneller)"),
                        ("EXCEPT", "Zeilen des ersten Teils, die im zweiten nicht vorkommen"),
                        ("EXCEPT ALL", "wie EXCEPT, doppelte Zeilen werden einzeln abgezogen"),
                        ("INTERSECT", "nur Zeilen, die in beiden Teilen vorkommen"),
                        ("INTERSECT ALL", "wie INTERSECT, mit doppelten"),
                    ]),
                    P("Unter den Teilen gibt es ORDER BY, LIMIT und OFFSET für das Gesamtergebnis. Hat ein einzelner Teil eigene Sortierung oder Begrenzung, \
                       wird er automatisch in Klammern gesetzt."),
                    Code("SELECT ort FROM kunde\nEXCEPT\nSELECT name FROM ort;"),
                ],
            },
            Section {
                id: "assistent-with",
                title: "WITH (benannte Hilfsabfragen)",
                blocks: &[
                    P("„+ WITH“ legt eine benannte Hilfsabfrage (Common Table Expression) an. Sie kann danach wie eine Tabelle verwendet werden: als Quelle in \
                       FROM („WITH-Abfrage“), in Unterabfragen und in weiteren WITH-Abfragen weiter unten. Optional lassen sich die Spaltennamen festlegen."),
                    P("Mit RECURSIVE darf eine WITH-Abfrage sich selbst verwenden – z. B. für Zahlenreihen oder Bäume (Kategorie mit Oberkategorie). \
                       Aufbau: ein Startteil, dann UNION ALL mit einem Teil, der die WITH-Abfrage selbst als Quelle hat, und eine Abbruchbedingung."),
                    Code("WITH RECURSIVE zahlen (n) AS (\n  SELECT 1\n  UNION ALL\n  SELECT n + 1 FROM zahlen WHERE n < 10\n)\nSELECT n FROM zahlen;"),
                ],
            },
        ],
    },
    Chapter {
        title: "ER-Diagramm",
        sections: &[
            Section {
                id: "er",
                title: "Diagramm lesen",
                blocks: &[
                    P("Das ER-Diagramm liest Tabellen, Spalten, Primär- und Fremdschlüssel einer Datenbank aus und zeichnet die Beziehungen. PK und FK stehen \
                       vor den Spalten, * markiert Pflichtspalten (NOT NULL)."),
                    List(&[
                        "1:n: ein Datensatz der einen Tabelle gehört zu vielen der anderen (Fremdschlüssel ohne UNIQUE).",
                        "1:1: der Fremdschlüssel ist eindeutig.",
                        "n:m: über eine Zwischentabelle, deren Primärschlüssel aus zwei Fremdschlüsseln besteht.",
                        "Optionale Beziehungen (Fremdschlüssel darf NULL sein) haben einen Kreis.",
                        "Maus über einer Linie erklärt die Beziehung, Maus über einer Tabelle hebt ihre Linien hervor.",
                    ]),
                    P("Unter „Ansicht“: Notation Chen (1, n, m) – Standard –, Krähenfuß (mit 1:n / 1:1 / n:m an der Linie) oder (min,max); Datentypen ein/aus; \
                       Zwischentabellen als direkte n:m-Linie zusammenfassen. Lange Aufzählungen (ENUM, SET) erscheinen gekürzt als enum(7); der volle Typ steht im Tooltip der Spalte."),
                    Tip("Namen von Tabellen und Datenbanken zeigt EasyMySQL so, wie sie beim Erstellen geschrieben wurden (z. B. Spielleitung). \
                         Eine Tabelle, die schon klein gespeichert ist, ändert man mit RENAME TABLE schueler TO Schueler;"),
                    Keys(&[Cmd::ErDiagram]),
                    Open("ER-Diagramm öffnen", Cmd(Cmd::ErDiagram)),
                ],
            },
            Section {
                id: "er-bedienung",
                title: "Bedienen und anordnen",
                blocks: &[
                    List(&[
                        "Tabelle ziehen: verschieben. Die Anordnung wird je Datenbank gespeichert.",
                        "Hintergrund oder mittlere Maustaste ziehen: Ansicht verschieben. Mausrad: Zoom um die Mausposition.",
                        "Der Prozentknopf zeigt den Zoom; ein Klick passt das ganze Diagramm ein. Auch sehr große Diagramme lassen sich weit herauszoomen; die Namen der wichtigsten Tabellen bleiben dabei lesbar.",
                        "Doppelklick auf eine Tabelle öffnet ihre Struktur, Rechtsklick bietet Daten, Struktur, SELECT und neue Beziehung.",
                        "„Anordnen“ berechnet eine neue Anordnung: Eltern links, Kinder rechts, in Ebenen mit möglichst wenigen Kreuzungen; lange Linien laufen auf eigenen Spuren zwischen den Tabellen, nie durch fremde. Getrennte Gruppen stehen nebeneinander, Tabellen ohne Beziehungen gesammelt darunter.",
                        "Export: als SVG-Grafik speichern oder das SQL-Skript (CREATE TABLE für alle Tabellen) öffnen.",
                    ]),
                ],
            },
            Section {
                id: "er-linien",
                title: "Beziehungen hinzufügen und entfernen",
                blocks: &[
                    P("Hinzufügen – zwei Wege:"),
                    List(&[
                        "Maus über eine Tabelle: rechts neben jeder Spalte erscheint ein Punkt. Vom Punkt auf die Zielspalte (meist id) ziehen.",
                        "„+ Beziehung“: Tabellen und Spalten aus Listen wählen.",
                    ]),
                    P("Danach wird die Art gewählt: 1:n legt einen Fremdschlüssel an, 1:1 macht die Spalte zusätzlich eindeutig, n:m legt eine Zwischentabelle mit \
                       beiden Schlüsseln an. Unterscheiden sich die Datentypen, erscheint ein Hinweis."),
                    P("Entfernen: Linie anklicken und Entf drücken, oder Rechtsklick auf die Linie. \
                       Bei n:m-Linien wird die Zwischentabelle gelöscht – vorher wird automatisch gesichert."),
                ],
            },
        ],
    },
    Chapter {
        title: "Java und JDBC",
        sections: &[
            Section {
                id: "java-verbinden",
                title: "Java mit der Datenbank verbinden",
                blocks: &[
                    P("Java spricht über JDBC mit der Datenbank. Dafür braucht das Projekt einen Treiber, den MariaDB Connector/J – eine einzelne JAR-Datei."),
                    Steps(&[
                        "Treiber hinzufügen: Mit Maven die Abhängigkeit unten eintragen. Ohne Maven die Datei „mariadb-java-client“ von mariadb.com/downloads/connectors herunterladen und dem Projekt als Bibliothek hinzufügen (IntelliJ: Projektstruktur → Bibliotheken, Eclipse: Build Path → Add External JARs).",
                        "Die Verbindungsadresse lautet jdbc:mariadb://localhost:3306/datenbankname. Der Datenbankname am Ende ist optional.",
                        "Benutzer und Passwort angeben. In EasyMySQL ist es standardmäßig der Benutzer root ohne Passwort. EasyMySQL muss laufen, denn der Server startet mit dem Programm.",
                    ]),
                    Source("<dependency>\n  <groupId>org.mariadb.jdbc</groupId>\n  <artifactId>mariadb-java-client</artifactId>\n  <version>3.5.3</version>\n</dependency>"),
                    Tip("Auch der „MySQL Connector/J“ funktioniert. Die Adresse beginnt dann mit jdbc:mysql://."),
                ],
            },
            Section {
                id: "java-abfragen",
                title: "Daten abfragen",
                blocks: &[
                    P("Eine Abfrage hat immer drei Schritte: verbinden, Anweisung vorbereiten, das Ergebnis Zeile für Zeile lesen. „try (…)“ schließt alles automatisch wieder."),
                    Source("import java.sql.*;\n\npublic class Beispiel {\n    public static void main(String[] args) throws SQLException {\n        String url = \"jdbc:mariadb://localhost:3306/schule\";\n        String sql = \"SELECT id, vorname FROM schueler WHERE klasse_id = ?\";\n\n        try (Connection con = DriverManager.getConnection(url, \"root\", \"\");\n             PreparedStatement ps = con.prepareStatement(sql)) {\n            ps.setInt(1, 3);\n            try (ResultSet rs = ps.executeQuery()) {\n                while (rs.next()) {\n                    int id = rs.getInt(\"id\");\n                    String vorname = rs.getString(\"vorname\");\n                    System.out.println(id + \" \" + vorname);\n                }\n            }\n        }\n    }\n}"),
                    List(&[
                        "Immer PreparedStatement mit ? verwenden und die Werte mit setInt, setString oder setObject einsetzen. Werte nie mit + in den SQL-Text kleben – das öffnet SQL-Injection.",
                        "rs.next() springt zur nächsten Zeile und liefert false, wenn keine mehr kommt.",
                        "Spalten lassen sich über den Namen (rs.getString(\"vorname\")) oder die Nummer ab 1 lesen. Bei NULL liefert getInt die Zahl 0; mit rs.wasNull() oder rs.getObject(\"spalte\", Integer.class) erkennt man das.",
                    ]),
                ],
            },
            Section {
                id: "java-aendern",
                title: "Daten ändern und Transaktionen",
                blocks: &[
                    P("INSERT, UPDATE und DELETE laufen mit executeUpdate(). Es liefert die Zahl der betroffenen Zeilen."),
                    Source("// Zeile einfügen und den neuen AUTO_INCREMENT-Wert lesen\nString sql = \"INSERT INTO schueler (vorname, klasse_id) VALUES (?, ?)\";\ntry (PreparedStatement ps = con.prepareStatement(sql, Statement.RETURN_GENERATED_KEYS)) {\n    ps.setString(1, \"Mia\");\n    ps.setInt(2, 3);\n    ps.executeUpdate();\n    try (ResultSet keys = ps.getGeneratedKeys()) {\n        if (keys.next()) System.out.println(\"Neue id: \" + keys.getLong(1));\n    }\n}\n\n// Mehrere Anweisungen zusammen oder gar nicht ausführen\ncon.setAutoCommit(false);\ntry {\n    // ... mehrere executeUpdate()-Aufrufe ...\n    con.commit();\n} catch (SQLException e) {\n    con.rollback();\n    throw e;\n}"),
                ],
            },
            Section {
                id: "java-typen",
                title: "SQL-Typen und Java-Typen",
                blocks: &[
                    Table(&[
                        ("INT, SMALLINT, TINYINT", "int / Integer"),
                        ("BIGINT", "long / Long"),
                        ("DECIMAL, NUMERIC", "BigDecimal"),
                        ("FLOAT, DOUBLE", "float, double"),
                        ("VARCHAR, CHAR, TEXT, ENUM", "String"),
                        ("DATE", "LocalDate"),
                        ("DATETIME, TIMESTAMP", "LocalDateTime"),
                        ("TIME", "LocalTime"),
                        ("TINYINT(1), BIT(1)", "boolean / Boolean"),
                        ("BLOB, VARBINARY", "byte[]"),
                    ]),
                    P("Mit rs.getObject(\"spalte\", LocalDate.class) und ps.setObject(1, datum) lassen sich Datum und Uhrzeit direkt mit den Klassen aus java.time verwenden. Spalten, die NULL sein dürfen, bekommen den Klassentyp (Integer statt int)."),
                ],
            },
            Section {
                id: "java-erzeugen",
                title: "Java-Code von EasyMySQL erzeugen lassen",
                blocks: &[
                    List(&[
                        "Rechtsklick auf eine Tabelle in der Seitenleiste → Java-Klasse kopieren. In der Zwischenablage liegen ein Record mit passenden Feldern und eine DAO-Klasse mit findAll, findById, insert, update und delete.",
                        "Im SQL-Editor kopiert der Befehl „Als Java-Code kopieren“ die markierte Anweisung (sonst die unter dem Cursor) als fertige Klasse mit main-Methode.",
                        "Host, Port und Benutzer stammen aus der Verbindung. Das Passwort wird nie eingetragen – dort steht ein leerer Text, den man anpasst.",
                    ]),
                    Keys(&[Cmd::CopyJava]),
                ],
            },
            Section {
                id: "java-fehler",
                title: "Typische Java-Fehler",
                blocks: &[
                    Table(&[
                        ("No suitable driver found", "Die Treiber-JAR fehlt im Klassenpfad bzw. in der Maven-Abhängigkeit. Die Adresse muss mit jdbc:mariadb:// beginnen."),
                        ("Communications link failure", "EasyMySQL bzw. der Server läuft nicht, oder Host und Port stimmen nicht (Standard: localhost, 3306)."),
                        ("Access denied for user", "Benutzer oder Passwort stimmen nicht. Standard: root ohne Passwort."),
                        ("Unknown database", "Den Datenbanknamen am Ende der Adresse prüfen oder die Datenbank zuerst anlegen."),
                    ]),
                ],
            },
        ],
    },
    Chapter {
        title: "Verlauf",
        sections: &[Section {
            id: "verlauf",
            title: "Verlauf aller Anweisungen",
            blocks: &[
                P("Jede im Editor oder Assistenten ausgeführte Anweisung landet im Verlauf – mit Zeitpunkt, Datenbank und Dauer. Fehlgeschlagene sind rot."),
                List(&[
                    "Filtern durchsucht SQL und Datenbank.",
                    "Doppelklick öffnet die Anweisung in einer neuen Abfrage; Rechtsklick: öffnen, erneut ausführen, kopieren.",
                    "Der Papierkorb oben leert den Verlauf.",
                    "Aufbewahrt werden die letzten 3000 Anweisungen.",
                ]),
                Keys(&[Cmd::ShowHistory]),
                Open("Verlauf zeigen", Cmd(Cmd::ShowHistory)),
            ],
        }],
    },
    Chapter {
        title: "Server und Datensicherheit",
        sections: &[
            Section {
                id: "server",
                title: "Server steuern",
                blocks: &[
                    List(&[
                        "Server → Starten, Stoppen, Neu starten.",
                        "Läuft auf Port 3306 schon ein anderer Datenbankserver, verwendet EasyMySQL diesen („externer Server“) und startet keinen eigenen.",
                        "Nach einem Update auf eine neuere MariaDB-Version werden die Systemtabellen beim Start automatisch angepasst.",
                    ]),
                ],
            },
            Section {
                id: "log",
                title: "Server-Log",
                blocks: &[
                    P("Das Server-Log sammelt Meldungen von EasyMySQL und MariaDB – immer, auch wenn die Registerkarte gar nicht offen ist. Es wird zusätzlich in \
                       die Datei server.log geschrieben; „Frühere Sitzungen“ zeigt auch die Meldungen vor dem letzten Start."),
                    List(&[
                        "Alles / Server / Anweisungen filtert nach Herkunft, das Textfeld nach Inhalt.",
                        "„Alle Anweisungen mitschreiben“ protokolliert jede Anweisung an den Server – auch aus der Eingabeaufforderung oder anderen Programmen (MariaDB general log, Datei abfragen.log).",
                        "„Interne ausblenden“ versteckt die Anweisungen, die EasyMySQL selbst im Hintergrund sendet.",
                        "Fehler sind rot, Warnungen gelb. „Leeren“ leert nur die Anzeige.",
                    ]),
                    Keys(&[Cmd::ServerLog]),
                    Open("Server-Log öffnen", Cmd(Cmd::ServerLog)),
                ],
            },
            Section {
                id: "absturzsicher",
                title: "Absturzsicherheit",
                blocks: &[
                    P("MariaDB läuft mit InnoDB und schreibt jede bestätigte Änderung sofort auf die Festplatte. Auch wenn EasyMySQL oder der PC hart beendet wird \
                       (Task-Manager, Stromausfall), gehen bestätigte Daten nicht verloren. Nach einem solchen Ende stellt MariaDB beim nächsten Start alles \
                       wieder her, danach prüft EasyMySQL alle Tabellen."),
                    P("Tabellen mit der Speicher-Engine MyISAM sind nicht absturzsicher. EasyMySQL weist beim Verbinden darauf hin und kann sie in InnoDB umwandeln."),
                ],
            },
            Section {
                id: "benutzer",
                title: "Benutzer und Rechte",
                blocks: &[
                    List(&[
                        "Server → Benutzer & Rechte. Links stehen alle Konten, „+“ legt ein neues an (Name, Host, Passwort, auf Wunsch mit einer eigenen Datenbank und allen Rechten darauf).",
                        "Host „localhost“ erlaubt nur diesen Computer, „%“ jeden Computer.",
                        "Rechts: Passwort ändern, Konto löschen und die Rechte. Oben stehen die Stellen, an denen das Konto Rechte hat; ein Klick wählt sie aus.",
                        "„Gilt für“ wählt global, eine Datenbank oder eine einzelne Tabelle. Die Voreinstellungen „Nur lesen“, „Lesen & Schreiben“ und „Alles“ setzen die Haken; „Übernehmen“ führt nur die nötigen GRANT- und REVOKE-Anweisungen aus (sie stehen im Tooltip).",
                    ]),
                    Tip("Das Konto root hat alle Rechte. Wer Rechte vergibt, braucht selbst das Recht GRANT OPTION."),
                    Open("Benutzer & Rechte", Cmd(Cmd::Users)),
                ],
            },
            Section {
                id: "sicherungen",
                title: "Sicherungen",
                blocks: &[
                    List(&[
                        "Automatisch beim Start und danach im eingestellten Abstand (Standard alle 2 Stunden), außerdem vor jedem Löschen oder Leeren.",
                        "Aufbewahrt werden die neuesten Sicherungen (Standard 10) und zusätzlich eine je Tag (Standard 14 Tage).",
                        "„Jetzt sichern“ erstellt sofort eine Sicherung aller Datenbanken.",
                        "Wiederherstellen: Sicherung und Datenbank wählen; unter altem Namen (ersetzt die Datenbank) oder unter neuem Namen.",
                        "Der Ordner ist frei wählbar, z. B. ein USB-Stick oder Netzlaufwerk.",
                    ]),
                    Keys(&[Cmd::Backups]),
                    Open("Sicherungen & Reparatur", Cmd(Cmd::Backups)),
                    Open("Sicherungs-Einstellungen", Settings("sicherungen")),
                ],
            },
            Section {
                id: "reparatur",
                title: "Prüfen, reparieren, Server retten",
                blocks: &[
                    Table(&[
                        ("Alle Tabellen prüfen", "CHECK TABLE für jede Tabelle, ändert nichts"),
                        ("Prüfen und reparieren", "sichert zuerst, prüft dann alle Tabellen und repariert beschädigte"),
                        ("MyISAM → InnoDB", "wandelt nicht absturzsichere Tabellen um"),
                        ("Server retten", "wenn MariaDB nicht mehr startet"),
                    ]),
                    P("Server retten legt den alten Datenordner unverändert beiseite (data-defekt-…), startet MariaDB auf einer Kopie in immer stärkeren \
                       Wiederherstellungsstufen, sichert die geretteten Daten und spielt sie in einen neuen Datenordner ein. Was sich nicht retten lässt, \
                       kommt aus der neuesten Sicherung. Danach hat root wieder kein Passwort."),
                ],
            },
        ],
    },
    Chapter {
        title: "Einstellungen",
        sections: &[
            Section {
                id: "einstellungen",
                title: "Alle Einstellungen",
                blocks: &[
                    P("Datei → Einstellungen öffnet eine Registerkarte; links springt man zu den Abschnitten. Die aktive Kategorie wird beim Scrollen hervorgehoben. Änderungen gelten sofort und werden gespeichert."),
                    Table(&[
                        ("Sprache", "Englisch als Standard, Deutsch wählbar"),
                        ("Darstellung", "dunkles Standarddesign, helles Design wählbar; die aktive Kategorie ist hervorgehoben"),
                        ("Updates", "beim Start und alle sechs Stunden nach neuen Versionen suchen; auf Wunsch automatisch im Hintergrund laden und installieren"),
                        ("Seitenleiste", "Explorer, Suche, Datenbanken und Verlauf links ein- oder ausblenden"),
                        ("Systemdatenbanken zeigen", "information_schema, mysql, performance_schema, sys in der Seitenleiste"),
                        ("Schriftgröße", "Schrift im SQL-Editor, 10 bis 28 Pixel"),
                        ("Vorschläge beim Tippen", "aus = nur noch per Tastenkürzel"),
                        ("Klammern schließen", "( [ { ' \" ` automatisch schließen"),
                        ("Scrollgeschwindigkeit", "Faktor für das Mausrad"),
                        ("Mittlere Maustaste", "gedrückt halten und ziehen zum Scrollen"),
                        ("Speicherordner", "wohin EasyMySQL Projekte und eigene Dateien speichert (siehe Speicherorte)"),
                        ("Sicherungen (Ordner)", "wohin Sicherungen geschrieben werden"),
                        ("Verbindung", "Server, Port, Benutzer, Passwort – auch für einen anderen MariaDB/MySQL-Server"),
                        ("Anweisungen mitschreiben", "jede Anweisung im Server-Log"),
                        ("Automatisch sichern", "Abstand und Aufbewahrung"),
                        ("Tastenkürzel", "alle Kürzel ändern, siehe unten"),
                    ]),
                    Open("Einstellungen öffnen", Cmd(Cmd::Settings)),
                    Open("Wo liegt was?", Section("speicherorte")),
                ],
            },
            Section {
                id: "verbindung",
                title: "Verbindung zu einem anderen Server",
                blocks: &[
                    P("Standard ist der eigene Server (127.0.0.1, Port 3306, root ohne Passwort). Unter Einstellungen → Verbindung lässt sich jeder andere \
                       MariaDB- oder MySQL-Server eintragen. „Passwort speichern“ legt es in der Einstellungsdatei ab (unverschlüsselt); sonst muss es nach \
                       jedem Start neu eingegeben werden. „Standard“ setzt die Werte für den eigenen Server."),
                    P("„Über SSH-Tunnel verbinden“ erreicht einen Server, der nur von seinem eigenen Rechner aus zugänglich ist: EasyMySQL baut mit dem OpenSSH-Client (ssh) einen Tunnel auf. \
                       Server und Port gelten dann aus Sicht des SSH-Servers (meist 127.0.0.1 und 3306). Anmeldung mit Schlüsseldatei, ssh-Agent oder Passwort; unbekannte Server werden beim ersten Mal gemerkt."),
                    P("Unter „Weitere Server“ lassen sich beliebig viele Server mit Namen speichern (auch mit SSH-Tunnel). Eine SQL-Datei kann sich einen davon aussuchen; ohne Auswahl gilt der Server oben. Verbunden wird erst, wenn eine Datei den Server braucht."),
                    Open("Verbindung einstellen", Settings("verbindung")),
                ],
            },
            Section {
                id: "updates",
                title: "Updates",
                blocks: &[
                    P("EasyMySQL sucht beim Start und danach alle sechs Stunden nach einer neuen Version. Ist eine da, erscheint oben rechts eine Meldung; „Installieren“ lädt das Update im Hintergrund und installiert es, ohne dass EasyMySQL geschlossen werden muss."),
                    List(&[
                        "Ist Einstellungen → Updates → „Automatisch aktualisieren“ eingeschaltet, laufen Download und Installation ganz von selbst; die Meldung erscheint erst, wenn alles fertig ist.",
                        "Nach der Installation fragt EasyMySQL, ob es neu starten soll (Knopf „Jetzt neu starten“). Mit „Später“ arbeiten Sie weiter; oben in der Titelleiste bleibt ein Knopf zum Neustarten.",
                        "Liegt EasyMySQL in einem geschützten Ordner (Programme), fragt Windows einmal nach der Freigabe für den Austausch der Programmdatei.",
                        "Die Versionsnummer richtet sich nach der Größe der Änderungen: große Neuerungen erhöhen die erste Zahl (4.0.0), neue Funktionen die zweite (4.1.0), kleine Korrekturen die dritte (4.1.1).",
                    ]),
                    Open("Nach Updates suchen", Cmd(Cmd::CheckUpdates)),
                ],
            },
            Section {
                id: "tastenkuerzel",
                title: "Tastenkürzel anpassen",
                blocks: &[
                    List(&[
                        "Einstellungen → Tastenkürzel: Klick auf ein Kürzel, dann die neue Kombination drücken. Esc bricht ab.",
                        "„+“ fügt eine weitere Kombination für denselben Befehl hinzu, Rechtsklick auf ein Kürzel entfernt es.",
                        "Rot = dieselbe Kombination ist noch für einen anderen Befehl vergeben (der Hinweis nennt ihn).",
                        "„Standard“ je Zeile oder „Alle auf Standard“ stellt die ursprüngliche Belegung her.",
                        "Menüs, Hinweise und dieses Handbuch zeigen immer die aktuelle Belegung.",
                    ]),
                    Open("Tastenkürzel ändern", Settings("tastenkuerzel")),
                    Open("Alle Tastenkürzel", Section("alle-kuerzel")),
                ],
            },
            Section {
                id: "alle-kuerzel",
                title: "Alle Tastenkürzel",
                blocks: &[
                    Keys(&[
                        Cmd::RunAll,
                        Cmd::RunStatement,
                        Cmd::Explain,
                        Cmd::NewQuery,
                        Cmd::NewFile,
                        Cmd::OpenFile,
                        Cmd::OpenFolder,
                        Cmd::Save,
                        Cmd::SaveAll,
                        Cmd::CloseTab,
                        Cmd::CloseAllTabs,
                        Cmd::ReopenTab,
                        Cmd::NextTab,
                        Cmd::PrevTab,
                        Cmd::CommandPalette,
                        Cmd::QuickOpen,
                        Cmd::ToggleSidebar,
                        Cmd::ShowExplorer,
                        Cmd::ShowSearch,
                        Cmd::ShowDatabases,
                        Cmd::ShowHistory,
                        Cmd::Settings,
                        Cmd::Help,
                        Cmd::ServerLog,
                        Cmd::ErDiagram,
                        Cmd::QueryBuilder,
                        Cmd::Backups,
                        Cmd::Import,
                        Cmd::Users,
                        Cmd::Format,
                        Cmd::FixAll,
                        Cmd::CopyJava,
                        Cmd::FontLarger,
                        Cmd::FontSmaller,
                        Cmd::FontReset,
                        Cmd::Suggest,
                        Cmd::Find,
                        Cmd::Replace,
                        Cmd::GotoLine,
                        Cmd::ToggleComment,
                        Cmd::CutLine,
                        Cmd::DuplicateLine,
                        Cmd::DeleteLine,
                        Cmd::SelectLine,
                        Cmd::MoveLineUp,
                        Cmd::MoveLineDown,
                        Cmd::CopyLineUp,
                        Cmd::CopyLineDown,
                        Cmd::Indent,
                        Cmd::Outdent,
                    ]),
                ],
            },
            Section {
                id: "speicherorte",
                title: "Speicherorte",
                blocks: &[
                    Table(&[
                        ("Speicherordner (Standard Dokumente\\EasyMySQL)", "neue Projekte; im Unterordner .easymysql: unbenannte Abfragen, frühere Fassungen, Verlauf, Diagramm-Anordnungen, Inhalte des Abfrage-Assistenten"),
                        ("Projektordner", "beliebig, die Dateien liegen genau dort; .easymysql darin merkt sich die Datenbank je Datei, .papierkorb enthält gelöschte Dateien"),
                        ("C:\\ProgramData\\EasyMySQL\\data", "die Datenbanken selbst (MariaDB)"),
                        ("C:\\ProgramData\\EasyMySQL\\backups", "Sicherungen (Ordner einstellbar)"),
                        ("C:\\ProgramData\\EasyMySQL", "Einstellungen, Tastenkürzel, server.log, abfragen.log"),
                        ("%LOCALAPPDATA%\\EasyMySQL", "statt ProgramData, falls dieser Ordner nicht beschreibbar ist"),
                    ]),
                    P("Wird der Speicherordner geändert, fragt EasyMySQL, ob der bisherige Inhalt mitgenommen werden soll („Inhalt mitnehmen“ verschiebt alles, \
                       offene Dateien und die Liste der zuletzt geöffneten Projekte werden angepasst) oder ob nur künftig dort gespeichert wird."),
                    Open("Speicherorte einstellen", Settings("speicherorte")),
                ],
            },
        ],
    },
    Chapter {
        title: "Fehlerbehebung",
        sections: &[
            Section {
                id: "fehler",
                title: "Häufige Fehlermeldungen",
                blocks: &[
                    Table(&[
                        ("1046 No database selected", "oben in der Datei eine Datenbank wählen oder USE name; ausführen"),
                        ("1064 Syntax error", "Schreibweise prüfen: fehlendes Komma, Klammer oder Anführungszeichen; die Stelle steht nach „near“"),
                        ("1146 Table doesn't exist", "Tabellenname oder gewählte Datenbank falsch"),
                        ("1054 Unknown column", "Spaltenname falsch oder Tabelle fehlt in FROM"),
                        ("1062 Duplicate entry", "Wert in einer eindeutigen Spalte (z. B. Primärschlüssel) gibt es schon"),
                        ("1451 Cannot delete or update a parent row", "andere Zeilen verweisen noch darauf – erst diese löschen oder ON DELETE CASCADE verwenden"),
                        ("1452 Cannot add or update a child row", "der Fremdschlüssel verweist auf einen Datensatz, den es nicht gibt"),
                        ("1005/1215 Foreign key constraint", "beide Spalten brauchen denselben Datentyp, das Ziel einen Primärschlüssel oder eindeutigen Index"),
                    ]),
                ],
            },
            Section {
                id: "server-probleme",
                title: "Server startet nicht",
                blocks: &[
                    Steps(&[
                        "Server-Log öffnen und die letzten roten Zeilen lesen.",
                        "Server → Neu starten.",
                        "Läuft ein anderes Programm auf Port 3306 (z. B. XAMPP), wird dieses verwendet – das andere Programm beenden und EasyMySQL neu starten.",
                        "Hilft das nicht: Sicherungen & Reparatur → Server retten.",
                    ]),
                    Open("Server-Log öffnen", Cmd(Cmd::ServerLog)),
                ],
            },
        ],
    },
];

/// Alle Abschnitts-IDs (fuer Tests)
#[cfg(test)]
pub fn section_ids() -> Vec<&'static str> {
    CHAPTERS.iter().flat_map(|c| c.sections.iter().map(|s| s.id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_unique_and_links_valid() {
        let ids = section_ids();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "doppelte Abschnitts-ID");
        let settings: Vec<&str> = crate::tabs::settings::SECTIONS.iter().map(|s| s.0).collect();
        for c in CHAPTERS {
            for s in c.sections {
                for b in s.blocks {
                    if let B::Open(_, t) = b {
                        match t {
                            Target::Section(id) => assert!(ids.contains(id), "{id}"),
                            Target::Settings(id) => assert!(settings.contains(id), "{id}"),
                            Target::Cmd(_) => {}
                        }
                    }
                }
            }
        }
    }

    /// Jeder deutsche Text des Handbuchs braucht eine englische Fassung (Ausnahmen: Namen und Beispiele).
    #[test]
    fn manual_is_translated() {
        use crate::i18n::Language;
        let mut missing = Vec::new();
        let mut check = |t: &str| {
            if Language::English.text(t) == t && t.chars().any(|c| "äöüÄÖÜß".contains(c)) {
                missing.push(t.to_string());
            }
        };
        for c in CHAPTERS {
            check(c.title);
            for s in c.sections {
                check(s.title);
                for b in s.blocks {
                    match b {
                        B::P(x) | B::Tip(x) => check(x),
                        B::Open(x, _) => check(x),
                        B::List(l) | B::Steps(l) => l.iter().for_each(|x| check(x)),
                        B::Table(rows) => rows.iter().for_each(|(a, b)| {
                            check(a);
                            check(b);
                        }),
                        _ => {}
                    }
                }
            }
        }
        assert!(missing.is_empty(), "Englische Fassung fehlt:\n{}", missing.join("\n"));
    }

    #[test]
    fn every_command_documented() {
        let mut seen = Vec::new();
        for c in CHAPTERS {
            for s in c.sections {
                for b in s.blocks {
                    if let B::Keys(k) = b {
                        seen.extend(k.iter().copied());
                    }
                }
            }
        }
        for cmd in Cmd::all() {
            assert!(seen.contains(&cmd), "{cmd:?} fehlt im Handbuch");
        }
    }
}
