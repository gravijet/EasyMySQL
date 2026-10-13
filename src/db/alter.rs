// ALTER-Anweisungen fuer Spalten und Tabellen: umbenennen, Typ aendern, verschieben, kopieren, zusammenfuegen.

use super::*;

/// Wohin eine Spalte verschoben wird
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Move {
    Left,
    Right,
    First,
    Last,
}

fn base_def(c: &ColumnInfo) -> ColDef {
    let mut d = ColDef::from_info(c);
    d.primary = false;
    d.unique = false;
    d
}

/// Spalte umbenennen und/oder neu definieren (Name und Typ in `def`)
pub fn change_column(full: &str, old: &str, def: &ColDef) -> String {
    let mut d = def.clone();
    d.primary = false;
    d.unique = false;
    format!("ALTER TABLE {full} CHANGE COLUMN {} {}", q(old), d.sql())
}

/// Spalte verschieben (MODIFY ... FIRST/AFTER). `None`, wenn sie schon dort ist.
pub fn move_column(full: &str, t: &TableInfo, col: &str, to: Move) -> Option<String> {
    let i = t.columns.iter().position(|c| c.name == col)?;
    if t.columns[i].is_generated() {
        return None;
    }
    let last = t.columns.len() - 1;
    let pos = match to {
        Move::Left if i == 0 => return None,
        Move::Left if i == 1 => " FIRST".to_string(),
        Move::Left => format!(" AFTER {}", q(&t.columns[i - 2].name)),
        Move::Right if i == last => return None,
        Move::Right => format!(" AFTER {}", q(&t.columns[i + 1].name)),
        Move::First if i == 0 => return None,
        Move::First => " FIRST".to_string(),
        Move::Last if i == last => return None,
        Move::Last => format!(" AFTER {}", q(&t.columns[last].name)),
    };
    Some(format!("ALTER TABLE {full} MODIFY COLUMN {}{pos}", base_def(&t.columns[i]).sql()))
}

/// Neue Spalte mit den Werten einer vorhandenen (Kopie direkt dahinter)
pub fn duplicate_column(full: &str, t: &TableInfo, col: &str) -> Option<Vec<String>> {
    let c = t.columns.iter().find(|c| c.name == col)?;
    let mut name = format!("{col}_kopie");
    let mut n = 2;
    while t.columns.iter().any(|x| x.name.eq_ignore_ascii_case(&name)) {
        name = format!("{col}_kopie{n}");
        n += 1;
    }
    let mut d = base_def(c);
    d.name = name.clone();
    d.auto_inc = false;
    Some(vec![format!("ALTER TABLE {full} ADD COLUMN {} AFTER {}", d.sql(), q(col)), format!("UPDATE {full} SET {} = {}", q(&name), q(col))])
}

/// Tabelle mit Aufbau (Indizes, Schluessel) und optional Inhalt kopieren
pub fn duplicate_table(db: &str, src: &str, dst: &str, with_data: bool) -> Vec<String> {
    let (s, d) = (format!("{}.{}", q(db), q(src)), format!("{}.{}", q(db), q(dst)));
    let mut v = vec![format!("CREATE TABLE {d} LIKE {s}")];
    if with_data {
        v.push(format!("INSERT INTO {d} SELECT * FROM {s}"));
    }
    v
}

/// Wie Tabellen zusammengefuegt werden
#[derive(Clone, Debug, PartialEq)]
pub enum MergeMode {
    /// Zeilen untereinander: Spalten gleichen Namens zusammen, fehlende werden NULL
    Union,
    /// Spalten nebeneinander ueber gleiche Werte (Join)
    Join { left_col: String, right_col: String, left_outer: bool },
}

/// SELECT, das beide Tabellen zusammenfuegt
pub fn merge_select(db: &str, a: &TableInfo, b: &TableInfo, mode: &MergeMode) -> String {
    let (fa, fb) = (format!("{}.{}", q(db), q(&a.name)), format!("{}.{}", q(db), q(&b.name)));
    match mode {
        MergeMode::Union => {
            let mut names: Vec<&str> = a.columns.iter().map(|c| c.name.as_str()).collect();
            for c in &b.columns {
                if !names.iter().any(|n| n.eq_ignore_ascii_case(&c.name)) {
                    names.push(&c.name);
                }
            }
            let side = |t: &TableInfo, f: &str| {
                let list: Vec<String> = names
                    .iter()
                    .map(|n| match t.columns.iter().find(|c| c.name.eq_ignore_ascii_case(n)) {
                        Some(c) => q(&c.name),
                        None => format!("NULL AS {}", q(n)),
                    })
                    .collect();
                format!("SELECT {} FROM {f}", list.join(", "))
            };
            format!("{}\nUNION ALL\n{}", side(a, &fa), side(b, &fb))
        }
        MergeMode::Join { left_col, right_col, left_outer } => {
            let mut cols: Vec<String> = a.columns.iter().map(|c| format!("a.{}", q(&c.name))).collect();
            for c in &b.columns {
                if a.columns.iter().any(|x| x.name.eq_ignore_ascii_case(&c.name)) {
                    // Gleiche Namen: die Spalte von B bekommt den Tabellennamen davor (der Join-Schluessel entfaellt)
                    if c.name == *right_col && left_col == right_col {
                        continue;
                    }
                    cols.push(format!("b.{} AS {}", q(&c.name), q(&format!("{}_{}", b.name, c.name))));
                } else {
                    cols.push(format!("b.{}", q(&c.name)));
                }
            }
            format!("SELECT {}\nFROM {fa} a\n{} {fb} b ON a.{} = b.{}", cols.join(", "), if *left_outer { "LEFT JOIN" } else { "INNER JOIN" }, q(left_col), q(right_col))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str, nullable: bool) -> ColumnInfo {
        ColumnInfo { name: name.into(), col_type: ty.into(), nullable, ..Default::default() }
    }

    fn table(name: &str, cols: &[(&str, &str)]) -> TableInfo {
        TableInfo { name: name.into(), columns: cols.iter().map(|(n, t)| col(n, t, true)).collect(), ..Default::default() }
    }

    #[test]
    fn moving() {
        let t = table("t", &[("a", "int(11)"), ("b", "varchar(10)"), ("c", "text")]);
        let f = "`d`.`t`";
        assert_eq!(move_column(f, &t, "a", Move::Left), None);
        assert_eq!(move_column(f, &t, "b", Move::Left).unwrap(), "ALTER TABLE `d`.`t` MODIFY COLUMN `b` varchar(10) NULL FIRST");
        assert_eq!(move_column(f, &t, "c", Move::Left).unwrap(), "ALTER TABLE `d`.`t` MODIFY COLUMN `c` text NULL AFTER `a`");
        assert_eq!(move_column(f, &t, "a", Move::Right).unwrap(), "ALTER TABLE `d`.`t` MODIFY COLUMN `a` int(11) NULL AFTER `b`");
        assert_eq!(move_column(f, &t, "c", Move::Right), None);
        assert_eq!(move_column(f, &t, "a", Move::Last).unwrap(), "ALTER TABLE `d`.`t` MODIFY COLUMN `a` int(11) NULL AFTER `c`");
        assert_eq!(move_column(f, &t, "c", Move::First).unwrap(), "ALTER TABLE `d`.`t` MODIFY COLUMN `c` text NULL FIRST");
    }

    #[test]
    fn copies_and_merges() {
        let a = table("kunde", &[("id", "int"), ("name", "text")]);
        let b = table("adresse", &[("id", "int"), ("kunde_id", "int"), ("ort", "text")]);
        let v = duplicate_column("`d`.`kunde`", &a, "name").unwrap();
        assert_eq!(v[0], "ALTER TABLE `d`.`kunde` ADD COLUMN `name_kopie` text NULL AFTER `name`");
        assert_eq!(v[1], "UPDATE `d`.`kunde` SET `name_kopie` = `name`");
        assert_eq!(duplicate_table("d", "a", "b", true), vec!["CREATE TABLE `d`.`b` LIKE `d`.`a`", "INSERT INTO `d`.`b` SELECT * FROM `d`.`a`"]);
        let u = merge_select("d", &a, &b, &MergeMode::Union);
        assert_eq!(u, "SELECT `id`, `name`, NULL AS `kunde_id`, NULL AS `ort` FROM `d`.`kunde`\nUNION ALL\nSELECT `id`, NULL AS `name`, `kunde_id`, `ort` FROM `d`.`adresse`");
        let j = merge_select("d", &a, &b, &MergeMode::Join { left_col: "id".into(), right_col: "kunde_id".into(), left_outer: true });
        assert_eq!(j, "SELECT a.`id`, a.`name`, b.`id` AS `adresse_id`, b.`kunde_id`, b.`ort`\nFROM `d`.`kunde` a\nLEFT JOIN `d`.`adresse` b ON a.`id` = b.`kunde_id`");
    }
}
