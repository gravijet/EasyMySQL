// Hinweise beim Zeigen mit der Maus: Signatur farbig, darunter die Beschreibung. Java (Methoden, Felder,
// Variablen, Klassen, Enum-Werte mit Javadoc) und SQL (Tabellen mit Spalten, Spalten mit Typ und Schluesseln,
// Funktionen). Der Java-Sprachserver ergaenzt, sobald er bereit ist.

use crate::style::Semi;
use super::Lang;
use super::highlight::is_word_char;
use crate::db::Schema;
use eframe::egui;

/// Wort an `index`: (von, bis, Text)
pub fn word_at(text: &str, index: usize) -> Option<(usize, usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    if index > chars.len() {
        return None;
    }
    let mut a = index;
    while a > 0 && is_word_char(chars[a - 1]) {
        a -= 1;
    }
    let mut b = index;
    while b < chars.len() && is_word_char(chars[b]) {
        b += 1;
    }
    (b > a && !chars[a].is_ascii_digit()).then(|| (a, b, chars[a..b].iter().collect()))
}

/// Karte am Mauszeiger
pub fn card(ctx: &egui::Context, layer: egui::LayerId, id: egui::Id, title: &str, body: &str, lang: Lang) {
    let pal = crate::style::pal();
    egui::Tooltip::always_open(ctx.clone(), layer, id, egui::PopupAnchor::Pointer).show(|ui| {
        ui.set_max_width(560.0);
        if !title.is_empty() {
            let font = egui::FontId::monospace(super::font_size());
            let mut job = super::lang::highlight_lang(title, font, lang);
            job.wrap.max_width = 540.0;
            ui.label(job);
        }
        if !body.is_empty() {
            if !title.is_empty() {
                ui.add_space(4.0);
                ui.separator();
            }
            ui.label(egui::RichText::new(body).color(pal.text));
        }
    });
}

/// Inhalt einer Hover-Karte: Titel (als Code gefaerbt), Spalten als Raster, Text, Beispiel
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub title: String,
    /// Spalten einer Tabelle: (Name, Typ, Kennzeichen "PK"/"FK"/"UNI"/"", Hinweis wie "→ kunde.id")
    pub rows: Vec<(String, String, &'static str, String)>,
    pub body: String,
    /// Beispiel (SQL), gefaerbt dargestellt
    pub example: String,
}

/// Karte am Mauszeiger mit Raster und Beispiel
pub fn card_info(ctx: &egui::Context, layer: egui::LayerId, id: egui::Id, info: &Info, lang: Lang) {
    let pal = crate::style::pal();
    egui::Tooltip::always_open(ctx.clone(), layer, id, egui::PopupAnchor::Pointer).show(|ui| {
        ui.set_max_width(560.0);
        let mono = egui::FontId::monospace(super::font_size());
        let small = egui::FontId::monospace((super::font_size() - 1.0).max(11.0));
        if !info.title.is_empty() {
            let mut job = super::lang::highlight_lang(&info.title, mono.clone(), lang);
            job.wrap.max_width = 540.0;
            ui.label(job);
        }
        if !info.rows.is_empty() {
            ui.add_space(4.0);
            egui::Grid::new(id.with("rows")).num_columns(3).spacing(egui::vec2(14.0, 3.0)).show(ui, |ui| {
                for (name, typ, mark, note) in &info.rows {
                    let (badge, color) = match *mark {
                        "PK" => ("PK", pal.warn_text),
                        "FK" => ("FK", pal.accent),
                        "UNI" => ("UNI", pal.ok_text),
                        _ => ("", pal.text_weak),
                    };
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if !badge.is_empty() {
                            ui.label(egui::RichText::new(badge).font(egui::FontId::proportional(10.0)).semi().color(color).background_color(color.gamma_multiply(0.14)));
                        }
                        ui.label(egui::RichText::new(name).font(mono.clone()).color(pal.text));
                    });
                    ui.label(egui::RichText::new(typ).font(small.clone()).color(pal.syn_type));
                    ui.label(egui::RichText::new(note).font(small.clone()).color(pal.text_weak));
                    ui.end_row();
                }
            });
        }
        if !info.body.is_empty() {
            ui.add_space(4.0);
            if !info.title.is_empty() && info.rows.is_empty() {
                ui.separator();
            }
            ui.label(egui::RichText::new(&info.body).color(if info.rows.is_empty() { pal.text } else { pal.text_weak }));
        }
        if !info.example.is_empty() {
            ui.add_space(6.0);
            egui::Frame::new().fill(pal.bg).stroke(egui::Stroke::new(1.0, pal.border)).corner_radius(4).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
                let mut job = super::lang::highlight_lang(&info.example, small.clone(), lang);
                job.wrap.max_width = 520.0;
                ui.label(job);
            });
        }
    });
}

/// Signatur und Beschreibung haeufiger SQL-Funktionen (deutsch, englisch)
pub fn function_doc(f: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match f {
        "COUNT" => ("COUNT(expr)", "Anzahl der Zeilen (ohne NULL-Werte; COUNT(*) zählt alle).", "Number of rows (without NULL values; COUNT(*) counts all)."),
        "SUM" => ("SUM(expr)", "Summe der Werte.", "Sum of the values."),
        "AVG" => ("AVG(expr)", "Durchschnitt der Werte.", "Average of the values."),
        "MIN" => ("MIN(expr)", "Kleinster Wert.", "Smallest value."),
        "MAX" => ("MAX(expr)", "Größter Wert.", "Largest value."),
        "NOW" => ("NOW()", "Aktuelles Datum und Uhrzeit.", "Current date and time."),
        "CURRENT_TIMESTAMP" => ("CURRENT_TIMESTAMP()", "Aktuelles Datum und Uhrzeit (wie NOW()).", "Current date and time (like NOW())."),
        "CURDATE" | "CURRENT_DATE" => ("CURDATE()", "Heutiges Datum.", "Today's date."),
        "CURTIME" | "CURRENT_TIME" => ("CURTIME()", "Aktuelle Uhrzeit.", "Current time."),
        "CONCAT" => ("CONCAT(a, b, …)", "Hängt Texte aneinander (NULL ergibt NULL).", "Joins texts together (NULL gives NULL)."),
        "CONCAT_WS" => ("CONCAT_WS(sep, a, b, …)", "Hängt Texte mit Trennzeichen aneinander und überspringt NULL.", "Joins texts with a separator and skips NULL."),
        "LENGTH" => ("LENGTH(text)", "Länge in Bytes (Umlaute zählen doppelt).", "Length in bytes (accented letters count twice)."),
        "CHAR_LENGTH" => ("CHAR_LENGTH(text)", "Länge in Zeichen.", "Length in characters."),
        "UPPER" | "UCASE" => ("UPPER(text)", "In Großbuchstaben.", "To upper case."),
        "LOWER" | "LCASE" => ("LOWER(text)", "In Kleinbuchstaben.", "To lower case."),
        "SUBSTRING" | "SUBSTR" => ("SUBSTRING(text, start, len)", "Teil eines Textes (Start bei 1).", "Part of a text (start at 1)."),
        "SUBSTRING_INDEX" => ("SUBSTRING_INDEX(text, trenner, n)", "Text bis zum n-ten Trennzeichen (negativ: von hinten).", "Text up to the n-th delimiter (negative: from the end)."),
        "LEFT" => ("LEFT(text, n)", "Die ersten n Zeichen.", "The first n characters."),
        "RIGHT" => ("RIGHT(text, n)", "Die letzten n Zeichen.", "The last n characters."),
        "TRIM" => ("TRIM(text)", "Entfernt Leerzeichen am Anfang und Ende.", "Removes spaces at the start and end."),
        "LTRIM" => ("LTRIM(text)", "Entfernt Leerzeichen am Anfang.", "Removes leading spaces."),
        "RTRIM" => ("RTRIM(text)", "Entfernt Leerzeichen am Ende.", "Removes trailing spaces."),
        "REPLACE" => ("REPLACE(text, von, nach)", "Ersetzt alle Vorkommen.", "Replaces all occurrences."),
        "LOCATE" => ("LOCATE(teil, text)", "Position von teil in text (0 = nicht enthalten).", "Position of part in text (0 = not found)."),
        "INSTR" => ("INSTR(text, teil)", "Position von teil in text (0 = nicht enthalten).", "Position of part in text (0 = not found)."),
        "LPAD" => ("LPAD(text, länge, füll)", "Links auffüllen, z. B. LPAD(nr, 5, '0') → 00042.", "Pad on the left, e.g. LPAD(nr, 5, '0') → 00042."),
        "RPAD" => ("RPAD(text, länge, füll)", "Rechts auffüllen.", "Pad on the right."),
        "REVERSE" => ("REVERSE(text)", "Text rückwärts.", "Text reversed."),
        "REPEAT" => ("REPEAT(text, n)", "Text n-mal hintereinander.", "Text repeated n times."),
        "FORMAT" => ("FORMAT(zahl, stellen, 'de_DE')", "Zahl mit Tausendertrennzeichen als Text.", "Number with thousands separators as text."),
        "REGEXP_REPLACE" => ("REGEXP_REPLACE(text, muster, ersatz)", "Ersetzt Treffer eines regulären Ausdrucks.", "Replaces matches of a regular expression."),
        "ROUND" => ("ROUND(x, stellen)", "Rundet auf die Stellen.", "Rounds to the digits."),
        "CEIL" | "CEILING" => ("CEIL(x)", "Aufrunden auf eine ganze Zahl.", "Round up to an integer."),
        "FLOOR" => ("FLOOR(x)", "Abrunden auf eine ganze Zahl.", "Round down to an integer."),
        "TRUNCATE" => ("TRUNCATE(x, stellen)", "Abschneiden ohne Runden.", "Cut off without rounding."),
        "MOD" => ("MOD(a, b)", "Rest der Division a / b.", "Remainder of a / b."),
        "POWER" | "POW" => ("POWER(x, y)", "x hoch y.", "x to the power of y."),
        "SQRT" => ("SQRT(x)", "Quadratwurzel.", "Square root."),
        "ABS" => ("ABS(x)", "Betrag.", "Absolute value."),
        "RAND" => ("RAND()", "Zufallszahl zwischen 0 und 1; ORDER BY RAND() mischt.", "Random number between 0 and 1; ORDER BY RAND() shuffles."),
        "GREATEST" => ("GREATEST(a, b, …)", "Größter der Werte.", "Largest of the values."),
        "LEAST" => ("LEAST(a, b, …)", "Kleinster der Werte.", "Smallest of the values."),
        "IFNULL" => ("IFNULL(x, ersatz)", "Ersatzwert, wenn x NULL ist.", "Fallback when x is NULL."),
        "NULLIF" => ("NULLIF(a, b)", "NULL, wenn a = b, sonst a (z. B. gegen Division durch 0).", "NULL if a = b, otherwise a (e.g. against division by 0)."),
        "ISNULL" => ("ISNULL(x)", "1, wenn x NULL ist.", "1 if x is NULL."),
        "COALESCE" => ("COALESCE(a, b, …)", "Erster Wert, der nicht NULL ist.", "First value that is not NULL."),
        "IF" => ("IF(bedingung, dann, sonst)", "Wert je nach Bedingung.", "Value depending on the condition."),
        "CAST" => ("CAST(x AS typ)", "Typ umwandeln, z. B. CAST(preis AS SIGNED).", "Convert the type, e.g. CAST(price AS SIGNED)."),
        "CONVERT" => ("CONVERT(x, typ)", "Typ oder Zeichensatz umwandeln.", "Convert type or character set."),
        "DATE_FORMAT" => ("DATE_FORMAT(datum, '%d.%m.%Y')", "Datum als Text formatieren (%H:%i für die Uhrzeit).", "Format a date as text (%H:%i for the time)."),
        "STR_TO_DATE" => ("STR_TO_DATE(text, '%d.%m.%Y')", "Text als Datum lesen.", "Read text as a date."),
        "DATE_ADD" | "ADDDATE" => ("DATE_ADD(datum, INTERVAL 7 DAY)", "Zeitspanne addieren.", "Add a time span."),
        "DATE_SUB" | "SUBDATE" => ("DATE_SUB(datum, INTERVAL 1 MONTH)", "Zeitspanne abziehen.", "Subtract a time span."),
        "DATEDIFF" => ("DATEDIFF(a, b)", "Tage zwischen zwei Daten (a − b).", "Days between two dates (a − b)."),
        "TIMESTAMPDIFF" => ("TIMESTAMPDIFF(YEAR, von, bis)", "Abstand in einer Einheit, z. B. das Alter in Jahren.", "Difference in a unit, e.g. the age in years."),
        "LAST_DAY" => ("LAST_DAY(datum)", "Letzter Tag des Monats.", "Last day of the month."),
        "DATE" => ("DATE(x)", "Nur das Datum eines Zeitpunkts.", "Only the date of a timestamp."),
        "TIME" => ("TIME(x)", "Nur die Uhrzeit eines Zeitpunkts.", "Only the time of a timestamp."),
        "YEAR" => ("YEAR(datum)", "Jahr eines Datums.", "Year of a date."),
        "MONTH" => ("MONTH(datum)", "Monat eines Datums (1–12).", "Month of a date (1–12)."),
        "DAY" | "DAYOFMONTH" => ("DAY(datum)", "Tag eines Datums.", "Day of a date."),
        "HOUR" => ("HOUR(zeit)", "Stunde.", "Hour."),
        "MINUTE" => ("MINUTE(zeit)", "Minute.", "Minute."),
        "SECOND" => ("SECOND(zeit)", "Sekunde.", "Second."),
        "WEEK" => ("WEEK(datum, 3)", "Kalenderwoche (Modus 3 = ISO).", "Week number (mode 3 = ISO)."),
        "WEEKDAY" => ("WEEKDAY(datum)", "Wochentag, 0 = Montag.", "Day of the week, 0 = Monday."),
        "DAYOFWEEK" => ("DAYOFWEEK(datum)", "Wochentag, 1 = Sonntag.", "Day of the week, 1 = Sunday."),
        "DAYNAME" => ("DAYNAME(datum)", "Name des Wochentags.", "Name of the weekday."),
        "MONTHNAME" => ("MONTHNAME(datum)", "Name des Monats.", "Name of the month."),
        "UNIX_TIMESTAMP" => ("UNIX_TIMESTAMP(datum)", "Sekunden seit 1970.", "Seconds since 1970."),
        "FROM_UNIXTIME" => ("FROM_UNIXTIME(sekunden)", "Zeitpunkt aus Sekunden seit 1970.", "Timestamp from seconds since 1970."),
        "GROUP_CONCAT" => ("GROUP_CONCAT(expr SEPARATOR ', ')", "Werte einer Gruppe als ein Text.", "Values of a group as one text."),
        "LAST_INSERT_ID" => ("LAST_INSERT_ID()", "Id der zuletzt eingefügten Zeile dieser Verbindung.", "Id of the last row inserted by this connection."),
        "ROW_COUNT" => ("ROW_COUNT()", "Zahl der zuletzt geänderten Zeilen.", "Number of rows changed last."),
        "UUID" => ("UUID()", "Eindeutige Kennung als Text.", "Unique identifier as text."),
        "MD5" => ("MD5(text)", "MD5-Prüfsumme (nicht für Passwörter).", "MD5 checksum (not for passwords)."),
        "SHA2" => ("SHA2(text, 256)", "SHA-256-Prüfsumme.", "SHA-256 checksum."),
        "JSON_EXTRACT" => ("JSON_EXTRACT(json, '$.pfad')", "Wert aus JSON lesen (kurz: spalte->'$.pfad').", "Read a value from JSON (short: column->'$.path')."),
        "JSON_OBJECT" => ("JSON_OBJECT('k', v, …)", "JSON-Objekt bauen.", "Build a JSON object."),
        "JSON_ARRAYAGG" => ("JSON_ARRAYAGG(expr)", "Werte einer Gruppe als JSON-Liste.", "Values of a group as a JSON array."),
        "ROW_NUMBER" => ("ROW_NUMBER() OVER (ORDER BY …)", "Laufende Nummer je Zeile im Fenster.", "Running number per row in the window."),
        "RANK" => ("RANK() OVER (ORDER BY …)", "Rang mit Lücken bei Gleichstand.", "Rank with gaps on ties."),
        "DENSE_RANK" => ("DENSE_RANK() OVER (ORDER BY …)", "Rang ohne Lücken.", "Rank without gaps."),
        "LAG" => ("LAG(expr, 1) OVER (ORDER BY …)", "Wert der vorigen Zeile.", "Value of the previous row."),
        "LEAD" => ("LEAD(expr, 1) OVER (ORDER BY …)", "Wert der nächsten Zeile.", "Value of the next row."),
        "DATABASE" => ("DATABASE()", "Name der gewählten Datenbank.", "Name of the selected database."),
        "USER" | "CURRENT_USER" => ("CURRENT_USER()", "Angemeldeter Benutzer.", "Signed-in user."),
        "VERSION" => ("VERSION()", "Version des Servers.", "Server version."),
        _ => return None,
    })
}

/// Erklaerung und Beispiel zu SQL-Schluesselwoertern (auch zweiteilige wie LEFT JOIN)
pub fn keyword_doc(k: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match k {
        "SELECT" => ("Liest Spalten aus Tabellen.", "Reads columns from tables.", "SELECT name, preis FROM produkt"),
        "FROM" => ("Nennt die Tabelle(n), aus denen gelesen wird.", "Names the table(s) to read from.", "SELECT * FROM kunde"),
        "WHERE" => ("Filtert Zeilen vor dem Gruppieren.", "Filters rows before grouping.", "SELECT * FROM kunde WHERE ort = 'Wien'"),
        "JOIN" | "INNER JOIN" => ("Verbindet Zeilen zweier Tabellen; nur Zeilen mit Partner bleiben.", "Combines rows of two tables; only rows with a match remain.", "SELECT * FROM bestellung b JOIN kunde k ON k.id = b.kunde_id"),
        "LEFT JOIN" => ("Wie JOIN, behält aber alle Zeilen der linken Tabelle (ohne Partner: NULL).", "Like JOIN but keeps every row of the left table (no match: NULL).", "SELECT k.name, b.id FROM kunde k LEFT JOIN bestellung b ON b.kunde_id = k.id"),
        "RIGHT JOIN" => ("Wie JOIN, behält aber alle Zeilen der rechten Tabelle.", "Like JOIN but keeps every row of the right table.", "SELECT * FROM a RIGHT JOIN b ON b.a_id = a.id"),
        "CROSS JOIN" => ("Jede Zeile mit jeder kombinieren (kartesisches Produkt).", "Combine every row with every row (Cartesian product).", "SELECT * FROM groesse CROSS JOIN farbe"),
        "ON" => ("Bedingung, wie zwei Tabellen zusammengehören.", "Condition for how two tables belong together.", "… JOIN kunde k ON k.id = b.kunde_id"),
        "USING" => ("JOIN über gleichnamige Spalten.", "JOIN on columns with the same name.", "… JOIN kunde USING (kunde_id)"),
        "GROUP BY" => ("Fasst Zeilen mit gleichem Wert zusammen, für COUNT, SUM …", "Groups rows with the same value, for COUNT, SUM …", "SELECT ort, COUNT(*) FROM kunde GROUP BY ort"),
        "HAVING" => ("Filtert Gruppen nach dem GROUP BY.", "Filters groups after GROUP BY.", "SELECT ort, COUNT(*) n FROM kunde GROUP BY ort HAVING n > 5"),
        "ORDER BY" => ("Sortiert das Ergebnis (ASC aufsteigend, DESC absteigend).", "Sorts the result (ASC ascending, DESC descending).", "SELECT * FROM produkt ORDER BY preis DESC"),
        "LIMIT" => ("Begrenzt die Zahl der Zeilen; mit OFFSET ab einer Stelle.", "Limits the number of rows; with OFFSET from a position.", "SELECT * FROM produkt LIMIT 10 OFFSET 20"),
        "DISTINCT" => ("Entfernt doppelte Zeilen.", "Removes duplicate rows.", "SELECT DISTINCT ort FROM kunde"),
        "UNION" => ("Hängt Ergebnisse untereinander und entfernt Doppelte (UNION ALL behält sie).", "Appends results and removes duplicates (UNION ALL keeps them).", "SELECT name FROM kunde UNION SELECT name FROM lieferant"),
        "UNION ALL" => ("Hängt Ergebnisse untereinander, Doppelte bleiben.", "Appends results, duplicates stay.", "SELECT a FROM x UNION ALL SELECT a FROM y"),
        "AS" => ("Gibt einer Spalte oder Tabelle einen anderen Namen.", "Gives a column or table another name.", "SELECT COUNT(*) AS anzahl FROM kunde k"),
        "IN" => ("Prüft, ob ein Wert in einer Liste oder Unterabfrage vorkommt.", "Checks whether a value is in a list or subquery.", "WHERE ort IN ('Wien', 'Graz')"),
        "LIKE" => ("Textmuster: % beliebig viele Zeichen, _ genau eines.", "Text pattern: % any number of characters, _ exactly one.", "WHERE name LIKE 'A%'"),
        "BETWEEN" => ("Bereich inklusive beider Grenzen.", "Range including both bounds.", "WHERE preis BETWEEN 10 AND 20"),
        "IS NULL" => ("Prüft auf fehlenden Wert (= NULL funktioniert nicht).", "Checks for a missing value (= NULL does not work).", "WHERE email IS NULL"),
        "IS NOT NULL" => ("Prüft, ob ein Wert vorhanden ist.", "Checks that a value is present.", "WHERE email IS NOT NULL"),
        "EXISTS" => ("Wahr, wenn die Unterabfrage mindestens eine Zeile liefert.", "True if the subquery returns at least one row.", "WHERE EXISTS (SELECT 1 FROM bestellung b WHERE b.kunde_id = k.id)"),
        "CASE" => ("Wert je nach Bedingung, wie if/else.", "Value depending on conditions, like if/else.", "CASE WHEN preis > 100 THEN 'teuer' ELSE 'günstig' END"),
        "INSERT INTO" | "INSERT" => ("Fügt Zeilen ein.", "Inserts rows.", "INSERT INTO kunde (name, ort) VALUES ('Ada', 'Wien')"),
        "VALUES" => ("Die Werte der neuen Zeile(n).", "The values of the new row(s).", "INSERT INTO t (a, b) VALUES (1, 'x'), (2, 'y')"),
        "UPDATE" => ("Ändert vorhandene Zeilen – ohne WHERE alle!", "Changes existing rows – without WHERE all of them!", "UPDATE produkt SET preis = preis * 1.1 WHERE id = 3"),
        "SET" => ("Neue Werte bei UPDATE (oder eine Variable).", "New values in UPDATE (or a variable).", "UPDATE kunde SET ort = 'Linz' WHERE id = 1"),
        "DELETE FROM" | "DELETE" => ("Löscht Zeilen – ohne WHERE alle! EasyMySQL sichert vorher.", "Deletes rows – without WHERE all of them! EasyMySQL backs up first.", "DELETE FROM sitzung WHERE ablauf < NOW()"),
        "CREATE TABLE" => ("Legt eine Tabelle an.", "Creates a table.", "CREATE TABLE kunde (\n  id INT AUTO_INCREMENT PRIMARY KEY,\n  name VARCHAR(100) NOT NULL\n)"),
        "ALTER TABLE" => ("Ändert eine Tabelle (Spalten, Schlüssel, Name).", "Changes a table (columns, keys, name).", "ALTER TABLE kunde ADD COLUMN email VARCHAR(255)"),
        "DROP TABLE" => ("Löscht eine Tabelle samt Daten. EasyMySQL sichert vorher.", "Deletes a table with its data. EasyMySQL backs up first.", "DROP TABLE IF EXISTS alt"),
        "TRUNCATE" | "TRUNCATE TABLE" => ("Leert eine Tabelle schnell; AUTO_INCREMENT beginnt neu.", "Empties a table quickly; AUTO_INCREMENT restarts.", "TRUNCATE TABLE protokoll"),
        "PRIMARY KEY" => ("Eindeutiger Schlüssel jeder Zeile, nie NULL.", "Unique key of every row, never NULL.", "id INT AUTO_INCREMENT PRIMARY KEY"),
        "FOREIGN KEY" | "REFERENCES" => ("Verweis auf eine Zeile einer anderen Tabelle.", "Reference to a row of another table.", "FOREIGN KEY (kunde_id) REFERENCES kunde(id)"),
        "AUTO_INCREMENT" => ("Zählt die Id beim Einfügen selbst hoch.", "Counts the id up by itself on insert.", "id INT AUTO_INCREMENT PRIMARY KEY"),
        "NOT NULL" => ("Die Spalte braucht immer einen Wert.", "The column always needs a value.", "name VARCHAR(100) NOT NULL"),
        "DEFAULT" => ("Wert, wenn beim Einfügen keiner angegeben ist.", "Value when none is given on insert.", "erstellt DATETIME DEFAULT CURRENT_TIMESTAMP"),
        "UNIQUE" => ("Jeder Wert darf nur einmal vorkommen.", "Each value may appear only once.", "email VARCHAR(255) UNIQUE"),
        "INDEX" | "CREATE INDEX" => ("Beschleunigt Suchen und Sortieren nach Spalten.", "Speeds up searching and sorting by columns.", "CREATE INDEX idx_name ON kunde(name)"),
        "VIEW" | "CREATE VIEW" => ("Gespeicherte Abfrage, die man wie eine Tabelle liest.", "Saved query that reads like a table.", "CREATE VIEW teure AS SELECT * FROM produkt WHERE preis > 100"),
        "WITH" => ("Benannte Teilabfrage (CTE) für die folgende Anweisung.", "Named subquery (CTE) for the following statement.", "WITH top AS (SELECT * FROM produkt ORDER BY preis DESC LIMIT 5)\nSELECT * FROM top"),
        "OVER" => ("Fensterfunktion: rechnet über Nachbarzeilen, ohne zu gruppieren.", "Window function: computes over neighbouring rows without grouping.", "SELECT name, RANK() OVER (ORDER BY punkte DESC) FROM spieler"),
        "USE" => ("Wählt die Datenbank für die folgenden Anweisungen.", "Selects the database for the following statements.", "USE shop"),
        "BEGIN" | "START TRANSACTION" => ("Beginnt eine Transaktion: alles oder nichts.", "Starts a transaction: all or nothing.", "START TRANSACTION;\nUPDATE konto SET stand = stand - 10 WHERE id = 1;\nCOMMIT"),
        "COMMIT" => ("Übernimmt die Änderungen der Transaktion.", "Keeps the changes of the transaction.", "COMMIT"),
        "ROLLBACK" => ("Verwirft die Änderungen der Transaktion.", "Discards the changes of the transaction.", "ROLLBACK"),
        "EXPLAIN" => ("Zeigt, wie der Server die Abfrage ausführt (Indizes, Reihenfolge).", "Shows how the server runs the query (indexes, order).", "EXPLAIN SELECT * FROM kunde WHERE name = 'Ada'"),
        _ => return None,
    })
}

/// Woerter vor und nach `a..b` (fuer zweiteilige Schluesselwoerter)
fn neighbour_words(chars: &[char], a: usize, b: usize) -> (String, String) {
    let mut i = a;
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    let pe = i;
    while i > 0 && is_word_char(chars[i - 1]) {
        i -= 1;
    }
    let prev: String = chars[i..pe].iter().collect();
    let mut j = b;
    while j < chars.len() && chars[j].is_whitespace() {
        j += 1;
    }
    let ns = j;
    while j < chars.len() && is_word_char(chars[j]) {
        j += 1;
    }
    (prev.to_uppercase(), chars[ns..j].iter().collect::<String>().to_uppercase())
}

/// Alias -> Tabelle in der Anweisung um `index` (FROM kunde k, JOIN bestellung AS b, FROM a x, b y)
fn alias_table(chars: &[char], index: usize, alias: &str) -> Option<String> {
    use super::lint::{Kind, known_word, lex};
    let index = index.min(chars.len());
    let st = chars[..index].iter().rposition(|c| *c == ';').map_or(0, |p| p + 1);
    let en = chars[index..].iter().position(|c| *c == ';').map_or(chars.len(), |p| index + p);
    let toks = lex(&chars[st..en]).toks;
    for i in 0..toks.len() {
        let t = &toks[i];
        if !(t.is_word("FROM") || t.is_word("JOIN") || t.is_word("UPDATE") || t.is_word("INTO")) {
            continue;
        }
        let mut j = i + 1;
        while let Some(n) = toks.get(j).filter(|n| n.is_name()) {
            let mut name = n.text.clone();
            if toks.get(j + 1).is_some_and(|d| d.kind == Kind::Dot)
                && let Some(m) = toks.get(j + 2).filter(|m| m.is_name())
            {
                name = m.text.clone();
                j += 2;
            }
            j += 1;
            let explicit = toks.get(j).is_some_and(|a| a.is_word("AS"));
            if let Some(a) = toks.get(j + usize::from(explicit)).filter(|a| a.is_name() && (explicit || a.kind == Kind::Quoted || !known_word(&a.upper()))) {
                if a.text.eq_ignore_ascii_case(alias) {
                    return Some(name);
                }
                j += 1 + usize::from(explicit);
            }
            if toks.get(j).is_some_and(|c| c.kind == Kind::Comma) && !t.is_word("JOIN") {
                j += 1;
            } else {
                break;
            }
        }
    }
    None
}

fn column_mark(schema: &Schema, t: &crate::db::TableInfo, c: &crate::db::ColumnInfo) -> (&'static str, String) {
    if let Some(fk) = schema.fks.iter().find(|f| f.table == t.name && f.columns.iter().any(|x| x.eq_ignore_ascii_case(&c.name))) {
        let note = format!("→ {}.{}", fk.ref_table, fk.ref_columns.join(", "));
        return (if c.is_pk() { "PK" } else { "FK" }, note);
    }
    let mark = if c.is_pk() { "PK" } else if c.key == "UNI" { "UNI" } else { "" };
    let mut note = String::new();
    if c.extra.to_ascii_lowercase().contains("auto_increment") {
        note.push_str("auto_increment");
    } else if let Some(d) = &c.default {
        note = format!("= {d}");
    }
    (mark, note)
}

fn table_info(schema: &Schema, t: &crate::db::TableInfo, alias: Option<&str>) -> Info {
    let mut info = Info::default();
    let kind = if t.is_view { "VIEW" } else { "TABLE" };
    info.title = format!("{kind} {}.{}", schema.name, t.name);
    for c in t.columns.iter().take(20) {
        let (mark, note) = column_mark(schema, t, c);
        info.rows.push((c.name.clone(), format!("{}{}", c.col_type, if c.nullable { "" } else { " NOT NULL" }), mark, note));
    }
    let mut body = Vec::new();
    if let Some(a) = alias {
        body.push(crate::tr_format!("Alias „{a}“ für {}", "Alias “{a}” for {}", t.name));
    }
    if t.columns.len() > 20 {
        body.push(crate::tr_format!("… und {} weitere Spalten", "… and {} more columns", t.columns.len() - 20));
    }
    let refs: Vec<String> = schema.fks.iter().filter(|f| f.ref_table == t.name && f.table != t.name).map(|f| format!("{}.{}", f.table, f.columns.join(", "))).collect();
    if !refs.is_empty() {
        body.push(crate::tr_format!("Verwiesen von: {}", "Referenced by: {}", refs.join(", ")));
    }
    if let Some(r) = t.rows {
        body.push(crate::tr_format!("≈ {} Zeilen", "≈ {} rows", r));
    }
    if !t.comment.is_empty() {
        body.push(t.comment.clone());
    }
    info.body = body.join("\n");
    info
}

/// SQL: Tabelle, Alias, Spalte, Funktion, Datenbank oder Schluesselwort unter dem Mauszeiger
pub fn sql_hover(text: &str, index: usize, schema: Option<&Schema>) -> Option<Info> {
    use crate::i18n::tr;
    let (a, b, word) = word_at(text, index)?;
    let chars: Vec<char> = text.chars().collect();
    let call = chars[b..].iter().find(|c| !c.is_whitespace()) == Some(&'(');
    let upper = word.to_uppercase();
    // Qualifiziert: k.name / kunde.name / shop.kunde
    let qualifier: Option<String> = (a > 0 && chars[a - 1] == '.').then(|| {
        let mut q = a - 1;
        while q > 0 && (is_word_char(chars[q - 1]) || chars[q - 1] == '`') {
            q -= 1;
        }
        chars[q..a - 1].iter().filter(|c| **c != '`').collect()
    });
    if qualifier.is_none() && call && let Some((sig, de, en)) = function_doc(&upper) {
        return Some(Info { title: sig.into(), body: tr(de, en).into(), ..Default::default() });
    }
    if let Some(schema) = schema {
        // Spalte einer bestimmten Tabelle bzw. eines Alias
        if let Some(q) = &qualifier {
            let table = alias_table(&chars, a, q).unwrap_or_else(|| q.clone());
            if let Some(t) = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&table)) {
                if let Some(c) = t.columns.iter().find(|c| c.name.eq_ignore_ascii_case(&word)) {
                    return Some(column_info(schema, t, c, &[]));
                }
            } else if q.eq_ignore_ascii_case(&schema.name)
                && let Some(t) = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&word))
            {
                return Some(table_info(schema, t, None));
            }
        }
        if let Some(t) = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&word)) {
            return Some(table_info(schema, t, None));
        }
        // Alias: FROM kunde k … k
        if qualifier.is_none()
            && let Some(table) = alias_table(&chars, a, &word)
            && let Some(t) = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&table))
        {
            return Some(table_info(schema, t, Some(&word)));
        }
        if qualifier.is_none() && word.eq_ignore_ascii_case(&schema.name) {
            let views = schema.tables.iter().filter(|t| t.is_view).count();
            return Some(Info {
                title: format!("DATABASE {}", schema.name),
                body: crate::tr_format!("{} Tabellen, {} Sichten, {} Fremdschlüssel", "{} tables, {} views, {} foreign keys", schema.tables.len() - views, views, schema.fks.len()),
                ..Default::default()
            });
        }
        // Spalte: zuerst in den Tabellen dieser Anweisung, sonst in allen
        if qualifier.is_none() && keyword_doc(&upper).is_none() {
            let lower = text.to_lowercase();
            let mut hits: Vec<(&crate::db::TableInfo, &crate::db::ColumnInfo)> = Vec::new();
            for t in &schema.tables {
                if let Some(c) = t.columns.iter().find(|c| c.name.eq_ignore_ascii_case(&word)) {
                    hits.push((t, c));
                }
            }
            hits.sort_by_key(|(t, _)| !lower.contains(&t.name.to_lowercase()));
            if let Some((t, c)) = hits.first() {
                let also: Vec<&str> = hits[1..].iter().map(|(t, _)| t.name.as_str()).collect();
                return Some(column_info(schema, t, c, &also));
            }
        }
    }
    if qualifier.is_some() {
        return None;
    }
    // Schluesselwort, auch zweiteilig (LEFT JOIN, GROUP BY, IS NOT NULL)
    let (prev, next) = neighbour_words(&chars, a, b);
    let candidates = [
        format!("{prev} {upper} {next}"),
        format!("{prev} {upper}"),
        format!("{upper} {next}"),
        upper.clone(),
    ];
    for k in candidates.iter() {
        if let Some((de, en, ex)) = keyword_doc(k.trim()) {
            return Some(Info { title: k.trim().to_string(), body: tr(de, en).into(), example: ex.into(), ..Default::default() });
        }
    }
    if let Some((sig, de, en)) = function_doc(&upper).filter(|_| call) {
        return Some(Info { title: sig.into(), body: tr(de, en).into(), ..Default::default() });
    }
    None
}

fn column_info(schema: &Schema, t: &crate::db::TableInfo, c: &crate::db::ColumnInfo, also: &[&str]) -> Info {
    use crate::i18n::tr;
    let mut body = Vec::new();
    if c.is_pk() {
        body.push(tr("Primärschlüssel", "Primary key").to_string());
    } else if c.key == "UNI" {
        body.push(tr("Eindeutig (UNIQUE)", "Unique").to_string());
    }
    if let Some(fk) = schema.fks.iter().find(|f| f.table == t.name && f.columns.iter().any(|x| x.eq_ignore_ascii_case(&c.name))) {
        body.push(crate::tr_format!("Verweist auf {}.{}", "References {}.{}", fk.ref_table, fk.ref_columns.join(", ")));
    }
    let used: Vec<String> = schema.fks.iter().filter(|f| f.ref_table == t.name && f.ref_columns.iter().any(|x| x.eq_ignore_ascii_case(&c.name))).map(|f| format!("{}.{}", f.table, f.columns.join(", "))).collect();
    if !used.is_empty() {
        body.push(crate::tr_format!("Verwendet von: {}", "Used by: {}", used.join(", ")));
    }
    if let Some(d) = &c.default {
        body.push(format!("DEFAULT {d}"));
    }
    if !c.extra.is_empty() {
        body.push(c.extra.clone());
    }
    if !c.comment.is_empty() {
        body.push(c.comment.clone());
    }
    if !also.is_empty() {
        body.push(crate::tr_format!("Auch in: {}", "Also in: {}", also.join(", ")));
    }
    Info {
        title: format!("{}.{} {}{}", t.name, c.name, c.col_type, if c.nullable { "" } else { " NOT NULL" }),
        body: body.join("\n"),
        ..Default::default()
    }
}

/// Wie `sql_hover`, als (Titel, Text) – Spalten einer Tabelle als Zeilen im Text
#[cfg(test)]
pub fn sql_info(text: &str, index: usize, schema: Option<&Schema>) -> Option<(String, String)> {
    let i = sql_hover(text, index, schema)?;
    let mut body: Vec<String> = i.rows.iter().map(|(n, t, m, note)| {
        let badge = match *m { "PK" => "  PK", "UNI" => "  UNIQUE", "FK" => "  FK", _ => "" };
        let note = if note.is_empty() { String::new() } else { format!("  {note}") };
        format!("{n}  {t}{badge}{note}")
    }).collect();
    if !i.body.is_empty() {
        body.push(i.body);
    }
    if !i.example.is_empty() {
        body.push(i.example);
    }
    Some((i.title, body.join("\n")))
}

/// Java: Mitglied, Klasse oder Variable dieser Datei unter dem Mauszeiger
pub fn java_info(text: &str, index: usize) -> Option<(String, String)> {
    use crate::dev::java::outline::{self, Kind};
    use crate::i18n::tr;
    let (a, _, word) = word_at(text, index)?;
    if crate::dev::java::source::KEYWORDS.contains(&word.as_str()) {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let classes = outline::outline(text);
    // Qualifiziert: Farbe.ROT, this.name, Klasse.methode
    let qualifier: Option<String> = (a > 0 && chars[a - 1] == '.').then(|| {
        let mut q = a - 1;
        while q > 0 && is_word_char(chars[q - 1]) {
            q -= 1;
        }
        chars[q..a - 1].iter().collect()
    });
    let here = outline::class_at(&classes, a);
    let owner = match qualifier.as_deref() {
        Some("this") | None => here,
        Some(q) => classes.iter().find(|c| c.name == q),
    };
    if let Some(c) = owner
        && let Some(m) = c.members.iter().find(|m| m.name == word && (qualifier.is_some() || m.kind != Kind::Constructor || m.name == c.name))
    {
        let mods = if m.is_static && m.kind != Kind::Constant { "static " } else { "" };
        let what = match m.kind {
            Kind::Method => tr("Methode in", "Method in"),
            Kind::Constructor => tr("Konstruktor von", "Constructor of"),
            Kind::Field => tr("Feld in", "Field in"),
            Kind::Constant => tr("Wert von", "Value of"),
        };
        let mut body = format!("{what} {}", c.name);
        if let Some(d) = &m.doc {
            body = format!("{d}\n\n{body}");
        }
        return Some((format!("{mods}{}", outline::signature(m)), body));
    }
    if let Some(c) = classes.iter().find(|c| c.name == word) {
        let mut body = crate::tr_format!("{} Mitglieder", "{} members", c.members.len());
        if let Some(d) = &c.doc {
            body = format!("{d}\n\n{body}");
        }
        return Some((format!("{} {}", c.kind, c.name), body));
    }
    if qualifier.is_none()
        && let Some((n, t, _)) = outline::locals(text, a + 1).into_iter().rev().find(|(n, _, _)| *n == word)
    {
        return Some((format!("{t} {n}"), tr("Lokale Variable", "Local variable").into()));
    }
    None
}

/// Antwort des Sprachservers in Signatur (Code-Block) und Beschreibung trennen
pub fn split_lsp(text: &str) -> (String, String) {
    let mut code = Vec::new();
    let mut rest = Vec::new();
    let mut in_code = false;
    for l in text.lines() {
        if l.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code { code.push(l) } else { rest.push(l) }
    }
    let body = rest.join("\n").replace("**", "").replace('`', "").trim().to_string();
    (code.join("\n").trim().to_string(), body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ColumnInfo, TableInfo};

    #[test]
    fn java_members_locals_and_enums() {
        let src = "class A {\n    enum Farbe { ROT }\n    /** Das Alter in Jahren. */\n    private int alter;\n    static int summe(int a, int b) { return a + b; }\n    void m() {\n        String s = \"x\";\n        int x = summe(alter, 2) + s.length();\n        Farbe f = Farbe.ROT;\n    }\n}\n";
        let at = |w: &str| src[..src.rfind(w).unwrap()].chars().count() + 1;
        assert_eq!(java_info(src, at("summe(alter")).unwrap().0, "static int summe(int a, int b)");
        let (t, b) = java_info(src, at("alter, 2")).unwrap();
        assert_eq!(t, "int alter");
        assert!(b.starts_with("Das Alter in Jahren."), "{b}");
        assert_eq!(java_info(src, at("s.length")).unwrap().0, "String s");
        assert_eq!(java_info(src, at("ROT;")).unwrap().0, "Farbe ROT");
        assert!(java_info(src, at("void")).is_none());
    }

    #[test]
    fn sql_tables_columns_and_functions() {
        let schema = Schema {
            name: "shop".into(),
            tables: vec![TableInfo {
                name: "kunde".into(),
                columns: vec![
                    ColumnInfo { name: "id".into(), col_type: "int(11)".into(), key: "PRI".into(), ..Default::default() },
                    ColumnInfo { name: "name".into(), col_type: "varchar(50)".into(), nullable: true, ..Default::default() },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let sql = "SELECT COUNT(*), name FROM kunde WHERE id = 1";
        let at = |w: &str| sql.find(w).unwrap() + 1;
        let (t, b) = sql_info(sql, at("kunde"), Some(&schema)).unwrap();
        assert_eq!(t, "TABLE shop.kunde");
        assert!(b.contains("id  int(11) NOT NULL") && b.contains("name  varchar(50)"), "{b}");
        assert_eq!(sql_info(sql, at("id ="), Some(&schema)).unwrap().0, "kunde.id int(11) NOT NULL");
        assert_eq!(sql_info(sql, at("COUNT"), None).unwrap().0, "COUNT(expr)");
        // Schluesselwort mit Erklaerung und Beispiel
        let w = sql_hover(sql, at("WHERE"), Some(&schema)).unwrap();
        assert_eq!(w.title, "WHERE");
        assert!(!w.example.is_empty());
        assert_eq!(split_lsp("```java\nint summe(int a)\n```\nAddiert **zwei** Zahlen."), ("int summe(int a)".to_string(), "Addiert zwei Zahlen.".to_string()));
    }

    #[test]
    fn alias_zweiteilige_woerter_und_datenbank() {
        let schema = Schema {
            name: "shop".into(),
            tables: vec![
                TableInfo { name: "kunde".into(), columns: vec![ColumnInfo { name: "id".into(), col_type: "int".into(), key: "PRI".into(), ..Default::default() }, ColumnInfo { name: "name".into(), col_type: "varchar(50)".into(), nullable: true, ..Default::default() }], ..Default::default() },
                TableInfo { name: "lieferant".into(), columns: vec![ColumnInfo { name: "name".into(), col_type: "varchar(80)".into(), nullable: true, ..Default::default() }], ..Default::default() },
            ],
            ..Default::default()
        };
        let sql = "SELECT l.name FROM kunde k LEFT JOIN lieferant AS l ON l.name = k.name GROUP BY k.name";
        let at = |w: &str| sql.find(w).unwrap() + 1;
        // l.name gehoert zu lieferant, nicht zur ersten Tabelle mit dieser Spalte
        assert_eq!(sql_hover(sql, at("l.name") + 2, Some(&schema)).unwrap().title, "lieferant.name varchar(80)");
        let k = sql_hover(sql, sql.rfind("k.name").unwrap() + 1, Some(&schema)).unwrap();
        assert_eq!(k.title, "TABLE shop.kunde");
        assert!(k.body.contains("Alias"), "{k:?}");
        assert_eq!(k.rows[0].2, "PK");
        assert_eq!(sql_hover(sql, at("JOIN"), Some(&schema)).unwrap().title, "LEFT JOIN");
        assert_eq!(sql_hover(sql, at("GROUP"), Some(&schema)).unwrap().title, "GROUP BY");
        assert!(sql_hover("USE shop", 5, Some(&schema)).unwrap().title.starts_with("DATABASE shop"));
        assert_eq!(sql_hover("SELECT DATE_ADD(x, INTERVAL 1 DAY)", 9, None).unwrap().title, "DATE_ADD(datum, INTERVAL 7 DAY)");
    }
}
