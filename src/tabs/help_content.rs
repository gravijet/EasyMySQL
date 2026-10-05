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
                       Beim Start von EasyMySQL startet der Server automatisch auf Port 3306. Er läuft, solange EasyMySQL läuft – \
                       auch wenn das Fenster geschlossen ist."),
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
                        "Menüleiste oben: alle Befehle. Rechts daneben die Werkzeuge ER-Diagramm, Abfrage-Assistent, Sicherungen und Server-Log sowie ein farbiger Punkt für den Server (grün = läuft, gelb = startet/stoppt, rot = Fehler). Ein Klick auf den Punkt öffnet das Server-Log.",
                        "Aktivitätsleiste ganz links: schaltet die Seitenleiste um – Explorer, Suchen, Datenbanken, Verlauf. Ein zweiter Klick auf das aktive Symbol blendet die Seitenleiste aus.",
                        "Seitenleiste: Breite mit der Maus an der rechten Kante ziehen.",
                        "Registerkarten in der Mitte: SQL-Dateien, Daten, Struktur, Diagramme, Einstellungen und so weiter.",
                        "Ohne offene Registerkarte bleibt die Mitte leer. Doppelklick auf die leere Fläche öffnet eine neue Abfrage.",
                        "Kurze Meldungen erscheinen unten rechts und verschwinden von selbst (Klick schließt sie sofort). Fehler erscheinen als Fenster.",
                    ]),
                    Keys(&[Cmd::ToggleSidebar, Cmd::ShowExplorer, Cmd::ShowSearch, Cmd::ShowDatabases, Cmd::ShowHistory]),
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
                title: "Schließen, Infobereich, Beenden",
                blocks: &[
                    P("Das Kreuz oben rechts am Fenster beendet EasyMySQL nicht: das Fenster verschwindet, der Server läuft weiter und das Symbol bleibt im \
                       Infobereich neben der Uhr. Ein Klick auf das Symbol holt das Fenster zurück."),
                    List(&[
                        "Richtig beenden: Rechtsklick auf das Symbol im Infobereich → Beenden, oder Datei → Beenden. Dabei wird der Server sauber heruntergefahren.",
                        "Beim Herunterfahren oder Abmelden von Windows fährt EasyMySQL den Server vorher sauber herunter.",
                        "Es läuft immer nur ein EasyMySQL. Ein zweiter Start holt das vorhandene Fenster nach vorne.",
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
                        "Datei → Neues Projekt: legt einen neuen Ordner im Speicherordner an und öffnet ihn.",
                        "Datei → Zuletzt geöffnet: die letzten zwölf Projekte.",
                        "Datei → Ordner schließen: kein Projekt geöffnet, der Explorer zeigt dann nur die Knöpfe zum Öffnen.",
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
                        "Wird eine offene Datei außerhalb geändert (z. B. in VS Code), übernimmt EasyMySQL den neuen Inhalt automatisch.",
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
                        ("Anweisung", "nur die Anweisung, in der der Cursor steht"),
                        ("Plan", "EXPLAIN der Anweisung am Cursor (bzw. der Markierung): wie MariaDB die Abfrage abarbeitet, welche Indizes benutzt werden"),
                    ]),
                    Keys(&[Cmd::RunAll, Cmd::RunStatement, Cmd::Explain]),
                    P("Oben links in der Datei steht die Datenbank, in der ausgeführt wird. Sie wird je Datei gemerkt. Ein „USE name;“ im Skript wechselt sie ebenfalls."),
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
                        "Jede Anweisung mit Ergebnis bekommt einen eigenen Reiter („Ergebnis 2 · Z. 14 (37)“ = zweites Ergebnis, Anweisung ab Zeile 14, 37 Zeilen).",
                        "„Meldungen“ zeigt jede Anweisung mit Dauer und betroffenen Zeilen. „Zeile n“ springt an die Stelle im Text.",
                        "Nach einem Fehler wird abgebrochen. Die Fehlerzeile ist rot unterstrichen, zu häufigen Fehlern steht ein Hinweis auf Deutsch dabei.",
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
                    P("Ab zwei Buchstaben schlägt der Editor Tabellen, Spalten, Funktionen und SQL-Wörter vor. Nach „tabelle.“ oder „alias.“ erscheinen die Spalten. \
                       Pfeiltasten wählen, Enter oder Tab übernimmt, Esc schließt. Ist das Wort schon vollständig getippt, bleibt Enter ein Zeilenumbruch."),
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
                id: "maus",
                title: "Scrollen mit der Maus",
                blocks: &[
                    List(&[
                        "Die Scrollgeschwindigkeit des Mausrads ist einstellbar (Standard 1,5-fach).",
                        "Mittlere Maustaste (Mausrad) gedrückt halten und die Maus bewegen: die Ansicht scrollt in diese Richtung, je weiter vom Ausgangspunkt, desto schneller. Loslassen beendet das Scrollen.",
                        "Im ER-Diagramm verschiebt die mittlere Maustaste stattdessen die Ansicht, auf Registerkarten schließt ein Mittelklick.",
                        "Die Minimap rechts im Editor: Klicken oder Ziehen springt an die Stelle.",
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
                        "Rechtsklick auf eine Tabelle: Daten, Struktur, SELECT, Name kopieren, Leeren, Löschen.",
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
                title: "Daten ansehen und bearbeiten",
                blocks: &[
                    List(&[
                        "WHERE: eine Bedingung eingeben (z. B. preis > 10) und Enter drücken.",
                        "Sortieren nach einer Spalte, auf- oder absteigend.",
                        "Es werden je 500 Zeilen gezeigt, < und > blättern.",
                        "Doppelklick auf eine Zelle: bearbeiten, Enter speichert sofort, Esc bricht ab.",
                        "Rechtsklick auf eine Zelle: Wert kopieren, Zeile kopieren, bearbeiten, auf NULL setzen, Zeile löschen.",
                        "„+ Neue Zeile“: Werte eingeben; „Standard“ angehakt = Standardwert bzw. AUTO_INCREMENT.",
                        "Hat eine Tabelle keinen Primärschlüssel, werden Änderungen über alle Spaltenwerte der Zeile zugeordnet.",
                        "Export wie bei Abfrageergebnissen.",
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
                id: "export",
                title: "Exportieren und importieren",
                blocks: &[
                    List(&[
                        "Datenbank → Exportieren: ganze Datenbank als .sql-Datei, mit oder ohne Daten. Tabellen stehen in der Reihenfolge ihrer Abhängigkeiten.",
                        "Datenbank → SQL-Datei importieren: Datei öffnen und sofort ausführen.",
                        "Einzelne Ergebnisse: Export-Knopf über der Tabelle.",
                    ]),
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
                    P("Unter „Ansicht“: Notation Krähenfuß (mit 1:n / 1:1 / n:m an der Linie), Chen (1, n, m) oder (min,max); Datentypen ein/aus; \
                       Zwischentabellen als direkte n:m-Linie zusammenfassen."),
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
                        "Der Prozentknopf zeigt den Zoom; ein Klick passt das ganze Diagramm ein.",
                        "Doppelklick auf eine Tabelle öffnet ihre Struktur, Rechtsklick bietet Daten, Struktur, SELECT und neue Beziehung.",
                        "„Anordnen“ berechnet eine neue Anordnung: verbundene Tabellen nebeneinander, möglichst wenige Kreuzungen, keine Linie durch fremde Tabellen, ungefähr Bildschirmformat. Tabellen ohne Beziehungen stehen gesammelt darunter.",
                        "Export: als SVG-Grafik speichern oder das SQL-Skript (CREATE TABLE für alle Tabellen) öffnen.",
                    ]),
                ],
            },
            Section {
                id: "er-linien",
                title: "Beziehungen hinzufügen und entfernen",
                blocks: &[
                    P("Hinzufügen – drei Wege:"),
                    List(&[
                        "Maus über eine Tabelle: rechts neben jeder Spalte erscheint ein Punkt. Vom Punkt auf die Zielspalte (meist id) ziehen.",
                        "„Verbinden“ einschalten: dann lässt sich von jeder Spalte ziehen, ohne den Punkt zu treffen. Esc beendet den Modus.",
                        "„+ Beziehung“: Tabellen und Spalten aus Listen wählen.",
                    ]),
                    P("Danach wird die Art gewählt: 1:n legt einen Fremdschlüssel an, 1:1 macht die Spalte zusätzlich eindeutig, n:m legt eine Zwischentabelle mit \
                       beiden Schlüsseln an. Unterscheiden sich die Datentypen, erscheint ein Hinweis."),
                    P("Entfernen: Linie anklicken (sie wird dick hervorgehoben) und Entf drücken oder „Beziehung löschen“, oder Rechtsklick auf die Linie. \
                       Bei n:m-Linien wird die Zwischentabelle gelöscht – vorher wird automatisch gesichert."),
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
                        "Server → Datenordner öffnen zeigt die Datenbankdateien. Bitte dort nichts von Hand ändern, solange der Server läuft.",
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
                        ("Darstellung", "helles Standarddesign; die aktive Kategorie ist hervorgehoben"),
                        ("Updates", "beim Start und alle sechs Stunden GitHub Releases prüfen; Download und Installation selbst starten"),
                        ("Systemdatenbanken zeigen", "information_schema, mysql, performance_schema, sys in der Seitenleiste"),
                        ("Schriftgröße", "Schrift im SQL-Editor, 10 bis 28 Pixel"),
                        ("Minimap", "Übersicht rechts im Editor"),
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
                        ("Visual Studio Code", "VS Code einrichten"),
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
                    Open("Verbindung einstellen", Settings("verbindung")),
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
                        Cmd::Format,
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
                    P("Außerdem: Strg+Plus / Strg+Minus vergrößert bzw. verkleinert die ganze Oberfläche, Strg+0 setzt sie zurück."),
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
            Section {
                id: "vscode",
                title: "Visual Studio Code und GitHub Copilot",
                blocks: &[
                    List(&[
                        "Server → VS Code einrichten installiert die Erweiterung SQLTools mit dem MariaDB-Treiber und legt die Verbindung „EasyMySQL“ an.",
                        "In einer Datei: Menü mit den drei Punkten → In VS Code öffnen. Speichert man dort, übernimmt EasyMySQL die Änderung.",
                        "Beim ersten Öffnen fragt VS Code, ob man dem Ordner vertraut: „Ja“ wählen, sonst sind die Erweiterungen ausgeschaltet.",
                        "In VS Code führt Strg+E Strg+E die Abfrage aus (SQLTools).",
                        "GitHub Copilot ist in aktuellem VS Code eingebaut: unten rechts auf das Copilot-Symbol klicken und mit dem GitHub-Konto anmelden.",
                    ]),
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
