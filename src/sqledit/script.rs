// Skript-Simulation: spielt ein SQL-Skript Anweisung fuer Anweisung gegen ein virtuelles Schema durch.
// Tabellen aus CREATE TABLE sind weiter unten bekannt, und Fehler, die erst beim Ausfuehren kaemen, werden
// vorher gemeldet - jeweils mit Korrektur, wo sie eindeutig ist:
// - Reihenfolge: Fremdschluessel auf eine spaeter angelegte Tabelle, DROP der Elterntabelle vor der
//   Kindtabelle, INSERT einer Kindzeile vor ihrer Elternzeile (Anweisung wird verschoben)
// - INSERT in eine fehlende Tabelle: CREATE TABLE mit Typen aus den Werten, Primaer- und Fremdschluesseln
// - fehlende Spalte, doppelter Schluessel (1062), fehlende Elternzeile (1452), zu langer Text (1406),
//   Pflichtfeld ohne Wert (1364), Fremdschluessel mit falschem Typ oder ohne Index (150), Tabelle schon da (1050)
// Positionen sind Zeichenindizes wie in `lint`.

use super::dialect::{german_date, group_end, insert_parts, items, text};
use super::lint::{Fix, Kind, Problem, Severity, Tok, nearest, quote_if_needed};
use crate::db::{ColumnInfo, ForeignKey, Schema, TableInfo};
use std::collections::{HashMap, HashSet};

/// Eine Anweisung des Skripts: Tokens und Zeichenbereich (mit Semikolon, falls vorhanden)
pub(super) struct Stmt<'a> {
    pub toks: &'a [Tok],
    pub start: usize,
    pub end: usize,
    pub semi: bool,
}

/// Wert aus VALUES
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Lit {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Other,
}

impl Lit {
    /// Vergleichswert (MariaDB vergleicht Text ohne Gross-/Kleinschreibung und ohne Leerzeichen am Ende)
    fn key(&self) -> Option<String> {
        match self {
            Lit::Num(s) => Some(s.clone()),
            Lit::Str(s) => Some(s.trim_end().to_lowercase()),
            Lit::Bool(b) => Some(if *b { "1" } else { "0" }.into()),
            _ => None,
        }
    }
}

fn lit(chars: &[char], toks: &[Tok], a: usize, b: usize) -> Lit {
    let num = |t: &Tok| {
        let s = text(chars, t.start, t.end);
        s.parse::<i64>().map(|n| n.to_string()).unwrap_or(s)
    };
    match &toks[a..b] {
        [t] if t.kind == Kind::Num => Lit::Num(num(t)),
        [m, t] if m.kind == Kind::Op && m.text == "-" && t.kind == Kind::Num => Lit::Num(format!("-{}", num(t))),
        [t] if t.kind == Kind::Str && t.end >= t.start + 2 => Lit::Str(text(chars, t.start + 1, t.end - 1).replace("''", "'")),
        [t] if t.is_word("NULL") => Lit::Null,
        [t] if t.is_word("TRUE") || t.is_word("FALSE") => Lit::Bool(t.is_word("TRUE")),
        _ => Lit::Other,
    }
}

/// Was eine Anweisung tut (vorab fuer alle Anweisungen bestimmt, um "weiter unten" nachzusehen)
pub(super) enum Fact {
    Create(String),
    Drop(Vec<String>),
    Insert { table: String, cols: Option<Vec<String>>, rows: Vec<Vec<Lit>> },
    Other,
}

struct Insert {
    ti: usize,
    list: Option<(usize, usize)>,
    cols: Vec<usize>,
    rows: Vec<Vec<(usize, usize)>>,
}

fn parse_insert(toks: &[Tok]) -> Option<Insert> {
    let (ti, vi) = insert_parts(toks)?;
    let list = match toks.get(ti + 1) {
        Some(t) if t.kind == Kind::Open => Some((ti + 1, group_end(toks, ti + 1)?)),
        _ => None,
    };
    let cols = list.map(|(o, c)| items(toks, o, c).into_iter().filter(|(a, b)| b > a).map(|(a, _)| a).collect()).unwrap_or_default();
    let mut rows = Vec::new();
    let mut k = vi + 1;
    while let Some(t) = toks.get(k) {
        match t.kind {
            Kind::Open => {
                let c = group_end(toks, k)?;
                rows.push(items(toks, k, c));
                k = c + 1;
            }
            Kind::Comma => k += 1,
            _ => break,
        }
    }
    Some(Insert { ti, list, cols, rows })
}

/// Name nach `at` lesen (`[db.]name`); liefert (Index des Namens, Datenbank)
fn name_at(toks: &[Tok], at: usize) -> Option<(usize, Option<String>)> {
    let t = toks.get(at).filter(|t| t.is_name())?;
    if toks.get(at + 1).is_some_and(|d| d.kind == Kind::Dot) && toks.get(at + 2).is_some_and(|n| n.is_name()) {
        return Some((at + 2, Some(t.text.to_lowercase())));
    }
    Some((at, None))
}

/// Index des Tabellennamens nach `TABLE [IF [NOT] EXISTS]`
fn after_table(toks: &[Tok], table: usize) -> usize {
    let mut k = table + 1;
    if toks.get(k).is_some_and(|t| t.is_word("IF")) {
        k += if toks.get(k + 1).is_some_and(|t| t.is_word("NOT")) { 3 } else { 2 };
    }
    k
}

fn table_kw(toks: &[Tok]) -> Option<usize> {
    toks.iter().take(5).position(|t| t.is_word("TABLE"))
}

pub(super) fn fact(chars: &[char], toks: &[Tok]) -> Fact {
    let Some(first) = toks.first() else { return Fact::Other };
    match first.upper().as_str() {
        "CREATE" => match table_kw(toks).and_then(|k| name_at(toks, after_table(toks, k))) {
            Some((n, _)) => Fact::Create(toks[n].text.to_lowercase()),
            None => Fact::Other,
        },
        "DROP" if toks.get(1).is_some_and(|t| t.is_word("TABLE") || t.is_word("TEMPORARY")) => {
            let Some(k) = table_kw(toks) else { return Fact::Other };
            Fact::Drop(drop_names(toks, k).into_iter().map(|n| toks[n].text.to_lowercase()).collect())
        }
        "INSERT" | "REPLACE" => match parse_insert(toks) {
            Some(ins) => Fact::Insert {
                table: toks[ins.ti].text.to_lowercase(),
                cols: ins.list.map(|_| ins.cols.iter().map(|&c| toks[c].text.to_lowercase()).collect()),
                rows: ins.rows.iter().map(|r| r.iter().map(|&(a, b)| lit(chars, toks, a, b)).collect()).collect(),
            },
            None => Fact::Other,
        },
        _ => Fact::Other,
    }
}

fn drop_names(toks: &[Tok], table: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut k = after_table(toks, table);
    while let Some((n, _)) = name_at(toks, k) {
        out.push(n);
        if toks.get(n + 1).is_some_and(|t| t.kind == Kind::Comma) {
            k = n + 2;
        } else {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Typen raten

/// Sammelt die Werte einer Spalte und leitet daraus einen Typ ab
#[derive(Default)]
struct Guess {
    ints: usize,
    decs: usize,
    int_digits: usize,
    scale: usize,
    strs: usize,
    dates: usize,
    datetimes: usize,
    times: usize,
    bools: usize,
    max_len: usize,
    keys: Vec<String>,
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && s.chars().filter(|c| c.is_ascii_digit()).count() == 8
}

impl Guess {
    fn add(&mut self, l: &Lit) {
        if let Some(k) = l.key() {
            self.keys.push(k);
        }
        match l {
            Lit::Num(s) => {
                let s = s.trim_start_matches('-');
                match s.split_once('.') {
                    Some((i, f)) => {
                        self.decs += 1;
                        self.int_digits = self.int_digits.max(i.len());
                        self.scale = self.scale.max(f.len());
                    }
                    None => {
                        self.ints += 1;
                        self.int_digits = self.int_digits.max(s.len());
                    }
                }
            }
            Lit::Str(s) => {
                self.max_len = self.max_len.max(s.chars().count());
                let s = german_date(s).unwrap_or_else(|| s.clone());
                if is_iso_date(&s) {
                    self.dates += 1;
                } else if s.len() >= 16 && is_iso_date(&s[..10]) && s.as_bytes()[10] == b' ' {
                    self.datetimes += 1;
                } else if s.len() >= 5 && s.len() <= 8 && s.as_bytes()[2] == b':' && s.chars().all(|c| c.is_ascii_digit() || c == ':') {
                    self.times += 1;
                } else {
                    self.strs += 1;
                }
            }
            Lit::Bool(_) => self.bools += 1,
            _ => {}
        }
    }

    fn all_int(&self) -> bool {
        self.ints > 0 && self.decs + self.strs + self.dates + self.datetimes + self.times + self.bools == 0
    }

    fn unique(&self) -> bool {
        let set: HashSet<&String> = self.keys.iter().collect();
        set.len() == self.keys.len()
    }

    fn sql(&self) -> String {
        let nums = self.ints + self.decs;
        let temporal = self.dates + self.datetimes + self.times;
        if self.strs > 0 || (nums > 0 && temporal > 0) {
            return varchar(self.max_len);
        }
        if self.datetimes > 0 {
            "DATETIME".into()
        } else if self.dates > 0 {
            "DATE".into()
        } else if self.times > 0 {
            "TIME".into()
        } else if self.decs > 0 {
            format!("DECIMAL({}, {})", (self.int_digits + self.scale + 2).max(8), self.scale)
        } else if self.ints > 0 {
            if self.int_digits > 9 { "BIGINT" } else { "INT" }.into()
        } else if self.bools > 0 {
            "BOOLEAN".into()
        } else {
            "VARCHAR(255)".into()
        }
    }
}

/// Textspalte mit Luft nach oben
pub(super) fn varchar(len: usize) -> String {
    match [50, 100, 255, 500, 1000, 2000].into_iter().find(|s| *s >= len) {
        Some(s) => format!("VARCHAR({s})"),
        None => "TEXT".into(),
    }
}

/// Laenge einer Textspalte: varchar(20) -> 20
fn text_len(col_type: &str) -> Option<usize> {
    let t = col_type.to_ascii_lowercase();
    if !(t.starts_with("varchar") || t.starts_with("char")) {
        return None;
    }
    t.split_once('(')?.1.split(')').next()?.trim().parse().ok()
}

/// Typen eines Fremdschluessels passen zusammen (Textlaengen duerfen abweichen, Zahltypen nicht)
fn fk_types_match(a: &str, b: &str) -> bool {
    let strip = |t: String| {
        let base = t.split('(').next().unwrap_or("").trim().to_string();
        if matches!(base.as_str(), "VARCHAR" | "CHAR" | "VARBINARY" | "BINARY") { base } else { t }
    };
    strip(super::errfix::norm_type(a)) == strip(super::errfix::norm_type(b))
}

fn singular(t: &str) -> Vec<String> {
    let mut v = vec![t.to_string()];
    for suf in ["en", "n", "e", "s"] {
        if let Some(s) = t.strip_suffix(suf).filter(|s| s.len() >= 3) {
            v.push(s.to_string());
        }
    }
    v
}

// ---------------------------------------------------------------------------
// Tabellendefinitionen lesen

struct FkDecl {
    cols: Vec<usize>,
    table: usize,
    refs: Vec<usize>,
}

struct ColDecl {
    col: ColumnInfo,
    type_range: (usize, usize),
    refs: Option<FkDecl>,
}

enum Item {
    Col(ColDecl),
    Pk(Vec<usize>),
    Fk(FkDecl, Option<String>),
    Unique(Vec<usize>),
    Index(Vec<usize>),
    Skip,
}

fn fk_target(toks: &[Tok], references: usize, b: usize) -> Option<(usize, Vec<usize>)> {
    let (t, _) = name_at(toks, references + 1)?;
    let refs = match toks.get(t + 1) {
        Some(o) if o.kind == Kind::Open && t + 1 < b => {
            let c = group_end(toks, t + 1)?;
            items(toks, t + 1, c).into_iter().filter(|(x, y)| y > x).map(|(x, _)| x).collect()
        }
        _ => Vec::new(),
    };
    Some((t, refs))
}

fn parse_column(chars: &[char], toks: &[Tok], a: usize, b: usize) -> Option<ColDecl> {
    let name = toks.get(a).filter(|t| t.is_name())?;
    let ty = toks.get(a + 1).filter(|t| t.kind == Kind::Word && a + 1 < b)?;
    let mut e = a + 1;
    if toks.get(e + 1).is_some_and(|t| t.kind == Kind::Open) && e + 1 < b {
        e = group_end(toks, e + 1)?;
    }
    while e + 1 < b && toks.get(e + 1).is_some_and(|t| t.is_word("UNSIGNED") || t.is_word("SIGNED") || t.is_word("ZEROFILL")) {
        e += 1;
    }
    let col_type = text(chars, ty.start, toks[e].end).to_lowercase().replace(", ", ",");
    let mut col = ColumnInfo { name: name.text.clone(), col_type, nullable: true, ..Default::default() };
    let mut refs = None;
    let mut k = e + 1;
    while k < b {
        let t = &toks[k];
        if t.is_word("NOT") && toks.get(k + 1).is_some_and(|n| n.is_word("NULL")) {
            col.nullable = false;
            k += 2;
            continue;
        }
        if t.is_word("PRIMARY") {
            col.key = "PRI".into();
            col.nullable = false;
        } else if t.is_word("UNIQUE") && col.key.is_empty() {
            col.key = "UNI".into();
        } else if t.is_word("AUTO_INCREMENT") {
            col.extra = "auto_increment".into();
        } else if t.is_word("DEFAULT") {
            if let Some(d) = toks.get(k + 1) {
                col.default = Some(text(chars, d.start, d.end));
                k += 1;
            }
        } else if t.is_word("AS") || t.is_word("GENERATED") {
            col.extra = "VIRTUAL GENERATED".into();
        } else if t.is_word("REFERENCES") {
            refs = fk_target(toks, k, b).map(|(table, r)| FkDecl { cols: vec![a], table, refs: r });
        }
        k += 1;
    }
    Some(ColDecl { col, type_range: (ty.start, toks[e].end), refs })
}

fn parse_item(chars: &[char], toks: &[Tok], a: usize, b: usize) -> Option<Item> {
    let mut k = a;
    let mut cname = None;
    if toks.get(k)?.is_word("CONSTRAINT") {
        k += 1;
        if toks.get(k).is_some_and(|t| t.is_name() && !["PRIMARY", "FOREIGN", "UNIQUE", "CHECK"].iter().any(|w| t.is_word(w))) {
            cname = Some(toks[k].text.clone());
            k += 1;
        }
    }
    let t = toks.get(k)?;
    let names_in = |from: usize| -> Option<Vec<usize>> {
        let o = (from..b).find(|&x| toks[x].kind == Kind::Open)?;
        let c = group_end(toks, o)?;
        Some(items(toks, o, c).into_iter().filter(|(x, y)| y > x).map(|(x, _)| x).collect())
    };
    Some(if t.is_word("PRIMARY") {
        Item::Pk(names_in(k)?)
    } else if t.is_word("FOREIGN") {
        let r = (k..b).find(|&x| toks[x].is_word("REFERENCES"))?;
        let cols = names_in(k).filter(|c| c.iter().all(|&x| x < r))?;
        let (table, refs) = fk_target(toks, r, b)?;
        Item::Fk(FkDecl { cols, table, refs }, cname)
    } else if t.is_word("UNIQUE") {
        Item::Unique(names_in(k)?)
    } else if ["KEY", "INDEX", "FULLTEXT", "SPATIAL"].iter().any(|w| t.is_word(w)) {
        Item::Index(names_in(k).unwrap_or_default())
    } else if t.is_word("CHECK") || t.is_word("PERIOD") {
        Item::Skip
    } else {
        Item::Col(parse_column(chars, toks, k, b)?)
    })
}

/// Name und Spalten eines CREATE TABLE, die ein INSERT braucht (ohne AUTO_INCREMENT und berechnete)
pub(super) fn create_columns(chars: &[char], toks: &[Tok]) -> Option<(String, Vec<String>)> {
    if !toks.first()?.is_word("CREATE") {
        return None;
    }
    let (ni, _) = name_at(toks, after_table(toks, table_kw(toks)?))?;
    if toks.get(ni + 1)?.kind != Kind::Open {
        return None;
    }
    let close = group_end(toks, ni + 1)?;
    let cols = items(toks, ni + 1, close)
        .into_iter()
        .filter_map(|(a, b)| match parse_item(chars, toks, a, b)? {
            Item::Col(cd) if !cd.col.extra.contains("auto_increment") && !cd.col.is_generated() => Some(cd.col.name),
            _ => None,
        })
        .collect();
    Some((toks[ni].text.clone(), cols))
}

// ---------------------------------------------------------------------------
// Simulation

/// Tabelle aus diesem Skript: wo die Typen stehen (fuer Korrekturen) und welche Spalten einen Index haben
#[derive(Default)]
struct Def {
    types: HashMap<String, (usize, usize)>,
    indexed: HashSet<String>,
}

/// Bekannte Zeilen einer Tabelle (nur vollstaendig, wenn die Tabelle im Skript angelegt wurde)
#[derive(Default)]
struct Rows {
    exact: bool,
    vals: HashMap<String, HashSet<String>>,
    max_id: i64,
}

struct Db {
    schema: Schema,
    /// Alle Tabellen bekannt (sonst gibt es vielleicht weitere auf dem Server)
    known: bool,
    defs: HashMap<String, Def>,
    rows: HashMap<String, Rows>,
}

impl Db {
    fn new(schema: Schema, known: bool) -> Db {
        Db { schema, known, defs: HashMap::new(), rows: HashMap::new() }
    }

    fn table(&self, lower: &str) -> Option<&TableInfo> {
        self.schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(lower))
    }

    fn table_mut(&mut self, lower: &str) -> Option<&mut TableInfo> {
        self.schema.tables.iter_mut().find(|t| t.name.eq_ignore_ascii_case(lower))
    }

    fn exact_rows(&self, lower: &str) -> Option<&Rows> {
        self.rows.get(lower).filter(|r| r.exact)
    }

    fn remove(&mut self, lower: &str) {
        self.schema.tables.retain(|t| !t.name.eq_ignore_ascii_case(lower));
        self.schema.fks.retain(|f| !f.table.eq_ignore_ascii_case(lower));
        self.schema.unique_keys.retain(|(t, _)| !t.eq_ignore_ascii_case(lower));
        self.defs.remove(lower);
        self.rows.remove(lower);
        self.schema.invalidate_index();
    }
}

/// Eingefuegter Wert -> Anweisungen, die ihn einfuegen
type Inserted = HashMap<String, Vec<usize>>;

pub(super) struct Sim {
    cur: Option<Db>,
    cur_name: String,
    saved: HashMap<String, Db>,
    dbs: HashSet<String>,
    fk_checks: bool,
    /// Je (Tabelle, Spalte): Wert -> Anweisungen, die ihn einfuegen (auch vergebene AUTO_INCREMENT-Werte)
    future: std::cell::RefCell<HashMap<(String, String), Inserted>>,
}

fn err(start: usize, end: usize, message: String, fix: Option<Fix>) -> Problem {
    Problem { start, end, severity: Severity::Error, message, fix }
}

fn insert_at(at: usize, text: String) -> Option<Fix> {
    Some(Fix { start: at, end: at, text })
}

/// Verschieben nur ueber kurze Strecken: sonst wird die Korrektur selbst riesig (und das Pruefen langsam)
const MOVE_MAX: usize = 20_000;

/// Anweisung `b` (weiter unten) vor Anweisung `a` holen
fn move_up(chars: &[char], a: &Stmt, b: &Stmt) -> Fix {
    let bt = text(chars, b.start, b.end) + if b.semi { "" } else { ";" };
    let mid = text(chars, a.start, b.start);
    let kept = mid.trim_end();
    let sep = if kept.len() < mid.len() { &mid[kept.len()..] } else { "\n" };
    Fix { start: a.start, end: b.end, text: format!("{bt}{sep}{kept}") }
}

/// Anweisung `a` hinter Anweisung `b` (weiter unten) schieben
fn move_down(chars: &[char], a: &Stmt, b: &Stmt) -> Fix {
    let at = text(chars, a.start, a.end);
    let rest = text(chars, a.end, b.end) + if b.semi { "" } else { ";" };
    let kept = rest.trim_start();
    let sep = if kept.len() < rest.len() { &rest[..rest.len() - kept.len()] } else { "\n" };
    Fix { start: a.start, end: b.end, text: format!("{kept}{sep}{at}") }
}

impl Sim {
    pub fn new(schema: Option<&Schema>, databases: &[String]) -> Sim {
        let mut sim = Sim { cur: None, cur_name: String::new(), saved: HashMap::new(), dbs: databases.iter().map(|d| d.to_lowercase()).collect(), fk_checks: true, future: Default::default() };
        match schema {
            Some(s) => {
                let mut s = s.clone();
                s.invalidate_index();
                sim.cur_name = s.name.to_lowercase();
                sim.cur = Some(Db::new(s, true));
            }
            None => sim.cur = Some(Db::new(Schema::default(), false)),
        }
        sim
    }

    /// Schema fuer die Pruefung der Namen: nur, wenn alle Tabellen bekannt sind
    pub fn schema(&self) -> Option<&Schema> {
        self.cur.as_ref().filter(|d| d.known).map(|d| &d.schema)
    }

    /// Alle bekannten Tabellen (auch ohne vollstaendiges Schema)
    pub fn tables(&self) -> HashMap<String, &TableInfo> {
        self.cur.iter().flat_map(|d| d.schema.tables.iter()).map(|t| (t.name.to_lowercase(), t)).collect()
    }

    pub fn step(&mut self, chars: &[char], stmts: &[Stmt], facts: &[Fact], i: usize, out: &mut Vec<Problem>) {
        let toks = stmts[i].toks;
        let Some(first) = toks.first() else { return };
        let w1 = toks.get(1).map(|t| t.upper()).unwrap_or_default();
        match first.upper().as_str() {
            "USE" => {
                if let Some(n) = toks.get(1).filter(|t| t.is_name()) {
                    let name = n.text.to_lowercase();
                    if name != self.cur_name {
                        if let Some(db) = self.cur.take() {
                            self.saved.insert(self.cur_name.clone(), db);
                        }
                        let fresh = Schema { name: n.text.clone(), ..Default::default() };
                        self.cur = Some(self.saved.remove(&name).unwrap_or_else(|| Db::new(fresh, false)));
                        self.cur_name = name;
                    }
                }
            }
            "CREATE" if matches!(w1.as_str(), "DATABASE" | "SCHEMA") => self.create_database(toks, out),
            "DROP" if matches!(w1.as_str(), "DATABASE" | "SCHEMA") => {
                let k = after_table(toks, 1);
                if let Some(n) = toks.get(k).filter(|t| t.is_name()) {
                    let name = n.text.to_lowercase();
                    self.dbs.remove(&name);
                    self.saved.remove(&name);
                    if name == self.cur_name {
                        self.cur = Some(Db::new(Schema { name: n.text.clone(), ..Default::default() }, false));
                        self.cur_name = String::new();
                    }
                }
            }
            "SET" if toks.iter().any(|t| t.text.to_ascii_lowercase().ends_with("foreign_key_checks")) => {
                self.fk_checks = !toks.iter().rev().find(|t| t.kind == Kind::Num || t.is_word("OFF") || t.is_word("ON")).is_some_and(|t| t.is_word("OFF") || chars[t.start] == '0');
            }
            "CREATE" if table_kw(toks).is_some() => self.create_table(chars, stmts, facts, i, out),
            "DROP" if matches!(w1.as_str(), "TABLE" | "TEMPORARY") => self.drop_table(chars, stmts, facts, i, out),
            "ALTER" if table_kw(toks).is_some() => self.alter_table(chars, stmts, facts, i, out),
            "INSERT" | "REPLACE" => self.insert(chars, stmts, facts, i, out),
            "RENAME" => {
                // RENAME TABLE a TO b
                if let (Some(a), Some(b)) = (toks.get(2), toks.get(4))
                    && let Some(db) = self.cur.as_mut()
                {
                    let (from, to) = (a.text.to_lowercase(), b.text.clone());
                    if let Some(t) = db.table_mut(&from) {
                        t.name = to.clone();
                    }
                    for f in &mut db.schema.fks {
                        if f.table.eq_ignore_ascii_case(&from) {
                            f.table = to.clone();
                        }
                        if f.ref_table.eq_ignore_ascii_case(&from) {
                            f.ref_table = to.clone();
                        }
                    }
                    if let Some(r) = db.rows.remove(&from) {
                        db.rows.insert(to.to_lowercase(), r);
                    }
                    db.defs.remove(&from);
                    db.schema.invalidate_index();
                }
            }
            w @ ("UPDATE" | "DELETE" | "TRUNCATE" | "LOAD") => {
                let at = match w {
                    "UPDATE" => toks.iter().position(|t| t.is_name() && !t.is_word("UPDATE") && !t.is_word("LOW_PRIORITY") && !t.is_word("IGNORE")),
                    "DELETE" => toks.iter().position(|t| t.is_word("FROM")).map(|k| k + 1),
                    "TRUNCATE" => Some(if w1 == "TABLE" { 2 } else { 1 }),
                    _ => toks.iter().position(|t| t.is_word("TABLE")).map(|k| k + 1),
                };
                let Some((n, _)) = at.and_then(|k| name_at(toks, k)) else { return };
                let all = matches!(w, "TRUNCATE") || (w == "DELETE" && !toks.iter().any(|t| t.is_word("WHERE") || t.is_word("LIMIT")));
                if let Some(r) = self.cur.as_mut().and_then(|d| d.rows.get_mut(&toks[n].text.to_lowercase())) {
                    if all {
                        r.vals.clear();
                    } else {
                        r.exact = false;
                    }
                }
            }
            _ => {}
        }
    }

    fn create_database(&mut self, toks: &[Tok], out: &mut Vec<Problem>) {
        let k = after_table(toks, 1);
        let Some(n) = toks.get(k).filter(|t| t.is_name()) else { return };
        let name = n.text.to_lowercase();
        let ine = k > 2;
        if self.dbs.contains(&name) {
            if !ine {
                out.push(Problem {
                    start: n.start,
                    end: n.end,
                    severity: Severity::Warning,
                    message: crate::tr_format!(
                        "Die Datenbank „{}“ gibt es schon – das Skript bricht hier ab (Fehler 1007). IF NOT EXISTS ergänzen oder vorher DROP DATABASE IF EXISTS.",
                        "Database “{}” already exists – the script stops here (error 1007). Add IF NOT EXISTS or DROP DATABASE IF EXISTS first.",
                        n.text
                    ),
                    fix: insert_at(toks[1].end, " IF NOT EXISTS".into()),
                });
            }
            return;
        }
        self.dbs.insert(name.clone());
        self.saved.insert(name, Db::new(Schema { name: n.text.clone(), ..Default::default() }, true));
    }

    // -- CREATE TABLE ------------------------------------------------------

    fn create_table(&mut self, chars: &[char], stmts: &[Stmt], facts: &[Fact], i: usize, out: &mut Vec<Problem>) {
        let toks = stmts[i].toks;
        let Some(tk) = table_kw(toks) else { return };
        let k = after_table(toks, tk);
        let ine = k > tk + 1;
        let Some((ni, other_db)) = name_at(toks, k) else { return };
        if other_db.is_some_and(|d| d != self.cur_name) {
            return;
        }
        let name_tok = &toks[ni];
        let lower = name_tok.text.to_lowercase();
        let Some(db) = self.cur.as_mut() else { return };
        let exists = db.table(&lower).is_some();
        if exists && ine {
            return;
        }
        let Some(open) = toks.get(ni + 1).filter(|t| t.kind == Kind::Open).map(|_| ni + 1) else {
            // CREATE TABLE a LIKE b / AS SELECT: Spalten uebernehmen bzw. unbekannt
            if let Some(src) = toks.get(ni + 2).filter(|_| toks[ni + 1].is_word("LIKE")).and_then(|s| db.table(&s.text.to_lowercase())).cloned() {
                db.schema.tables.push(TableInfo { name: name_tok.text.clone(), ..src });
                db.schema.invalidate_index();
            } else {
                db.known = false;
            }
            return;
        };
        let Some(close) = group_end(toks, open) else { return };
        let mut table = TableInfo { name: name_tok.text.clone(), engine: "InnoDB".into(), ..Default::default() };
        let mut def = Def::default();
        let mut fkdecls: Vec<(FkDecl, Option<String>)> = Vec::new();
        let mut uniques: Vec<Vec<String>> = Vec::new();
        let lname = |x: usize| toks[x].text.to_lowercase();
        for (a, b) in items(toks, open, close) {
            match parse_item(chars, toks, a, b) {
                Some(Item::Col(cd)) => {
                    let l = cd.col.name.to_lowercase();
                    def.types.insert(l.clone(), cd.type_range);
                    if !cd.col.key.is_empty() {
                        def.indexed.insert(l.clone());
                    }
                    if cd.col.key == "UNI" {
                        uniques.push(vec![cd.col.name.clone()]);
                    }
                    if let Some(f) = cd.refs {
                        def.indexed.insert(l);
                        fkdecls.push((f, None));
                    }
                    table.columns.push(cd.col);
                }
                Some(Item::Pk(cols)) => {
                    for &c in &cols {
                        if let Some(col) = table.columns.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&toks[c].text)) {
                            col.key = "PRI".into();
                            col.nullable = false;
                        }
                    }
                    def.indexed.extend(cols.first().map(|&c| lname(c)));
                }
                Some(Item::Unique(cols)) => {
                    def.indexed.extend(cols.first().map(|&c| lname(c)));
                    uniques.push(cols.iter().map(|&c| toks[c].text.clone()).collect());
                }
                Some(Item::Index(cols)) => def.indexed.extend(cols.first().map(|&c| lname(c))),
                Some(Item::Fk(f, n)) => {
                    def.indexed.extend(f.cols.first().map(|&c| lname(c)));
                    fkdecls.push((f, n));
                }
                Some(Item::Skip) | None => {}
            }
        }
        if exists {
            out.push(Problem {
                start: name_tok.start,
                end: name_tok.end,
                severity: Severity::Warning,
                message: crate::tr_format!(
                    "Die Tabelle „{}“ gibt es schon – das Skript bricht hier ab (Fehler 1050). Korrektur: IF NOT EXISTS; zum Neuanlegen vorher DROP TABLE IF EXISTS.",
                    "Table “{}” already exists – the script stops here (error 1050). Fix: IF NOT EXISTS; to recreate it, DROP TABLE IF EXISTS first.",
                    name_tok.text
                ),
                fix: insert_at(toks[tk].end, " IF NOT EXISTS".into()),
            });
        }
        let mut fks = Vec::new();
        for (f, n) in &fkdecls {
            self.check_fk(chars, stmts, facts, i, &table, Some(&def), f, out);
            fks.push(self.foreign_key(toks, &table, f, n.clone()));
        }
        let Some(db) = self.cur.as_mut() else { return };
        db.remove(&lower);
        for u in uniques {
            db.schema.unique_keys.push((table.name.clone(), u));
        }
        db.schema.unique_keys.push((table.name.clone(), table.pk_columns()));
        db.schema.fks.extend(fks);
        db.schema.tables.push(table);
        db.schema.invalidate_index();
        db.defs.insert(lower.clone(), def);
        db.rows.insert(lower, Rows { exact: true, ..Default::default() });
    }

    fn foreign_key(&self, toks: &[Tok], own: &TableInfo, f: &FkDecl, name: Option<String>) -> ForeignKey {
        let ref_table = toks[f.table].text.clone();
        let mut ref_columns: Vec<String> = f.refs.iter().map(|&r| toks[r].text.clone()).collect();
        if ref_columns.is_empty() {
            let parent = if ref_table.eq_ignore_ascii_case(&own.name) { Some(own) } else { self.cur.as_ref().and_then(|d| d.table(&ref_table.to_lowercase())) };
            ref_columns = parent.map(|p| p.pk_columns()).unwrap_or_default();
        }
        ForeignKey {
            name: name.unwrap_or_default(),
            table: own.name.clone(),
            columns: f.cols.iter().map(|&c| toks[c].text.clone()).collect(),
            ref_table,
            ref_columns,
            ..Default::default()
        }
    }

    /// Fremdschluessel pruefen: Elterntabelle vorhanden (oder erst weiter unten), Spalte vorhanden, gleicher
    /// Typ, Index auf der Elternspalte
    #[allow(clippy::too_many_arguments)]
    fn check_fk(&self, chars: &[char], stmts: &[Stmt], facts: &[Fact], i: usize, own: &TableInfo, own_def: Option<&Def>, f: &FkDecl, out: &mut Vec<Problem>) {
        if !self.fk_checks {
            return;
        }
        let Some(db) = self.cur.as_ref() else { return };
        let toks = stmts[i].toks;
        let ptok = &toks[f.table];
        let plower = ptok.text.to_lowercase();
        let parent = if plower == own.name.to_lowercase() { Some(own) } else { db.table(&plower) };
        let Some(parent) = parent else {
            if let Some(j) = (i + 1..facts.len()).find(|&j| matches!(&facts[j], Fact::Create(n) if *n == plower)) {
                out.push(err(
                    ptok.start,
                    ptok.end,
                    crate::tr_format!(
                        "Die Tabelle „{}“ wird erst weiter unten angelegt – hier gibt es sie noch nicht (Fehler 150). Korrektur: ihr CREATE TABLE davor stellen.",
                        "Table “{}” is only created further down – it does not exist yet here (error 150). Fix: move its CREATE TABLE before this one.",
                        ptok.text
                    ),
                    Some(move_up(chars, &stmts[i], &stmts[j])),
                ));
            } else if db.known {
                let names: Vec<&str> = db.schema.tables.iter().map(|t| t.name.as_str()).collect();
                if let Some(s) = nearest(&ptok.text, names.iter().copied()) {
                    out.push(err(
                        ptok.start,
                        ptok.end,
                        crate::tr_format!("Die Tabelle „{}“ gibt es nicht. Meinten Sie „{s}“?", "Table “{}” does not exist. Did you mean “{s}”?", ptok.text),
                        Some(Fix { start: ptok.start, end: ptok.end, text: quote_if_needed(s) }),
                    ));
                } else {
                    let col_tok = f.refs.first().or(f.cols.first()).map(|&c| toks[c].text.clone()).unwrap_or_else(|| "id".into());
                    let own_type = f.cols.first().and_then(|&c| own.columns.iter().find(|x| x.name.eq_ignore_ascii_case(&toks[c].text))).map(|c| super::errfix::norm_type(&c.col_type)).unwrap_or_else(|| "INT".into());
                    let auto = if own_type.ends_with("INT") { " AUTO_INCREMENT" } else { "" };
                    out.push(err(
                        ptok.start,
                        ptok.end,
                        crate::tr_format!(
                            "Die Tabelle „{}“ gibt es nicht (Fehler 150). Korrektur: sie davor mit dem Schlüssel {col_tok} anlegen.",
                            "Table “{}” does not exist (error 150). Fix: create it before, with key {col_tok}.",
                            ptok.text
                        ),
                        insert_at(stmts[i].start, format!("CREATE TABLE {} (\n    {} {own_type}{auto} PRIMARY KEY\n);\n\n", quote_if_needed(&ptok.text), quote_if_needed(&col_tok))),
                    ));
                }
            }
            return;
        };
        let pdef = db.defs.get(&plower).or(if plower == own.name.to_lowercase() { own_def } else { None });
        for (n, &r) in f.refs.iter().enumerate() {
            let rtok = &toks[r];
            let Some(pc) = parent.columns.iter().find(|c| c.name.eq_ignore_ascii_case(&rtok.text)) else {
                let cols: Vec<&str> = parent.columns.iter().map(|c| c.name.as_str()).collect();
                let pk = parent.pk_columns();
                let s = nearest(&rtok.text, cols.iter().copied()).map(str::to_string).or_else(|| (pk.len() == 1).then(|| pk[0].clone()));
                out.push(err(
                    rtok.start,
                    rtok.end,
                    crate::tr_format!("Die Tabelle „{}“ hat keine Spalte „{}“ (Fehler 150).", "Table “{}” has no column “{}” (error 150).", parent.name, rtok.text),
                    s.map(|s| Fix { start: rtok.start, end: rtok.end, text: quote_if_needed(&s) }),
                ));
                continue;
            };
            let rl = pc.name.to_lowercase();
            if let Some(pdef) = pdef
                && !pc.is_pk()
                && !pdef.indexed.contains(&rl)
            {
                out.push(err(
                    rtok.start,
                    rtok.end,
                    crate::tr_format!(
                        "„{}.{}“ braucht einen Schlüssel, damit ein Fremdschlüssel darauf zeigen kann (Fehler 150). Korrektur: UNIQUE ergänzen.",
                        "“{}.{}” needs a key so a foreign key can point to it (error 150). Fix: add UNIQUE.",
                        parent.name,
                        pc.name
                    ),
                    pdef.types.get(&rl).and_then(|&(_, e)| insert_at(e, " UNIQUE".into())),
                ));
            }
            let Some(&ct) = f.cols.get(n) else { continue };
            let ctok = &toks[ct];
            let Some(oc) = own.columns.iter().find(|c| c.name.eq_ignore_ascii_case(&ctok.text)) else { continue };
            if !fk_types_match(&oc.col_type, &pc.col_type) {
                let want = super::errfix::norm_type(&pc.col_type);
                out.push(err(
                    ctok.start,
                    ctok.end,
                    crate::tr_format!(
                        "„{}“ ist {}, „{}.{}“ aber {want} – Fremdschlüssel brauchen denselben Typ (Fehler 150).",
                        "“{}” is {}, but “{}.{}” is {want} – foreign keys need the same type (error 150).",
                        oc.name,
                        super::errfix::norm_type(&oc.col_type),
                        parent.name,
                        pc.name
                    ),
                    own_def.and_then(|d| d.types.get(&oc.name.to_lowercase())).map(|&(s, e)| Fix { start: s, end: e, text: want.clone() }),
                ));
            }
        }
    }

    // -- DROP TABLE --------------------------------------------------------

    fn drop_table(&mut self, chars: &[char], stmts: &[Stmt], facts: &[Fact], i: usize, out: &mut Vec<Problem>) {
        let toks = stmts[i].toks;
        let Some(tk) = table_kw(toks) else { return };
        let ife = after_table(toks, tk) > tk + 1;
        let names = drop_names(toks, tk);
        let lowers: Vec<String> = names.iter().map(|&n| toks[n].text.to_lowercase()).collect();
        let Some(db) = self.cur.as_ref() else { return };
        for (n, &ni) in names.iter().enumerate() {
            let t = &toks[ni];
            if db.table(&lowers[n]).is_none() {
                let all: Vec<&str> = db.schema.tables.iter().map(|t| t.name.as_str()).collect();
                if db.known && !ife && nearest(&t.text, all.iter().copied()).is_none() {
                    out.push(err(
                        t.start,
                        t.end,
                        crate::tr_format!(
                            "Die Tabelle „{}“ gibt es hier nicht – das Skript bricht ab (Fehler 1051). Korrektur: IF EXISTS.",
                            "Table “{}” does not exist here – the script stops (error 1051). Fix: IF EXISTS.",
                            t.text
                        ),
                        insert_at(toks[tk].end, " IF EXISTS".into()),
                    ));
                }
                continue;
            }
            if !self.fk_checks {
                continue;
            }
            // Kindtabellen, die noch darauf zeigen (in derselben Liste muessen sie vorher stehen)
            let child = db.schema.fks.iter().map(|f| f.table.clone()).find(|c| {
                let cl = c.to_lowercase();
                cl != lowers[n] && !lowers[..n].contains(&cl) && db.schema.fks.iter().any(|f| f.table.eq_ignore_ascii_case(c) && f.ref_table.eq_ignore_ascii_case(&lowers[n]))
            });
            let Some(child) = child else { continue };
            let cl = child.to_lowercase();
            let msg = crate::tr_format!(
                "„{}“ kann nicht gelöscht werden, solange „{child}“ darauf verweist (Fehler 1451). Erst „{child}“ löschen.",
                "“{}” cannot be dropped while “{child}” references it (error 1451). Drop “{child}” first.",
                t.text
            );
            let fix = if lowers[n + 1..].contains(&cl) {
                // Reihenfolge in der Liste: Kinder zuerst
                let mut order: Vec<usize> = (0..names.len()).collect();
                let mut sorted = Vec::new();
                while !order.is_empty() {
                    let pick = order
                        .iter()
                        .position(|&x| !order.iter().any(|&y| y != x && db.schema.fks.iter().any(|f| f.table.eq_ignore_ascii_case(&lowers[y]) && f.ref_table.eq_ignore_ascii_case(&lowers[x]))))
                        .unwrap_or(0);
                    sorted.push(order.remove(pick));
                }
                let first = names.first().map(|&x| toks[x].start).unwrap_or(t.start);
                let last = names.last().map(|&x| toks[x].end).unwrap_or(t.end);
                let list: Vec<String> = sorted.iter().map(|&x| text(chars, toks[names[x]].start, toks[names[x]].end)).collect();
                Some(Fix { start: first, end: last, text: list.join(", ") })
            } else if let Some(j) = (i + 1..facts.len()).find(|&j| matches!(&facts[j], Fact::Drop(d) if d.contains(&cl))) {
                Some(move_up(chars, &stmts[i], &stmts[j]))
            } else {
                insert_at(t.start, format!("{}, ", quote_if_needed(&child)))
            };
            out.push(err(t.start, t.end, msg, fix));
        }
        let Some(db) = self.cur.as_mut() else { return };
        for l in lowers {
            db.remove(&l);
        }
    }

    // -- ALTER TABLE -------------------------------------------------------

    fn alter_table(&mut self, chars: &[char], stmts: &[Stmt], facts: &[Fact], i: usize, out: &mut Vec<Problem>) {
        let toks = stmts[i].toks;
        let Some(tk) = table_kw(toks) else { return };
        let Some((ni, other)) = name_at(toks, after_table(toks, tk)) else { return };
        if other.is_some_and(|d| d != self.cur_name) {
            return;
        }
        let lower = toks[ni].text.to_lowercase();
        let Some(mut table) = self.cur.as_ref().and_then(|d| d.table(&lower)).cloned() else { return };
        let mut fks = Vec::new();
        let mut dropped_fks = Vec::new();
        let mut new_name = None;
        for (a, b) in items(toks, ni, toks.len()) {
            let Some(w) = toks.get(a).filter(|_| b > a) else { continue };
            let next = |k: usize| toks.get(k).filter(|_| k < b);
            if w.is_word("ADD") {
                let mut k = a + 1;
                if next(k).is_some_and(|t| t.is_word("COLUMN")) {
                    k += 1;
                }
                let ine = next(k).is_some_and(|t| t.is_word("IF"));
                if ine {
                    k += 3;
                }
                match parse_item(chars, toks, k, b) {
                    Some(Item::Col(cd)) => {
                        let ctok = &toks[k];
                        if table.columns.iter().any(|c| c.name.eq_ignore_ascii_case(&cd.col.name)) {
                            if !ine {
                                out.push(err(
                                    ctok.start,
                                    ctok.end,
                                    crate::tr_format!("Die Spalte „{}“ gibt es schon (Fehler 1060). Korrektur: IF NOT EXISTS.", "Column “{}” already exists (error 1060). Fix: IF NOT EXISTS.", ctok.text),
                                    insert_at(toks[k - 1].end, " IF NOT EXISTS".into()),
                                ));
                            }
                            continue;
                        }
                        if let Some(f) = &cd.refs {
                            let mut with = table.clone();
                            with.columns.push(cd.col.clone());
                            self.check_fk(chars, stmts, facts, i, &with, None, f, out);
                            fks.push(self.foreign_key(toks, &with, f, None));
                        }
                        table.columns.push(cd.col);
                    }
                    Some(Item::Fk(f, n)) => {
                        self.check_fk(chars, stmts, facts, i, &table, None, &f, out);
                        fks.push(self.foreign_key(toks, &table, &f, n));
                    }
                    Some(Item::Pk(cols)) => {
                        for c in cols {
                            if let Some(col) = table.columns.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&toks[c].text)) {
                                col.key = "PRI".into();
                                col.nullable = false;
                            }
                        }
                    }
                    _ => {}
                }
            } else if w.is_word("DROP") {
                let mut k = a + 1;
                if next(k).is_some_and(|t| t.is_word("FOREIGN")) {
                    if let Some(n) = next(k + 2) {
                        dropped_fks.push(n.text.to_lowercase());
                    }
                    continue;
                }
                if next(k).is_some_and(|t| ["PRIMARY", "INDEX", "KEY", "CONSTRAINT", "CHECK", "PARTITION"].iter().any(|x| t.is_word(x))) {
                    continue;
                }
                if next(k).is_some_and(|t| t.is_word("COLUMN")) {
                    k += 1;
                }
                let ife = next(k).is_some_and(|t| t.is_word("IF"));
                if ife {
                    k += 2;
                }
                let Some(c) = next(k).filter(|t| t.is_name()) else { continue };
                if let Some(p) = table.columns.iter().position(|x| x.name.eq_ignore_ascii_case(&c.text)) {
                    table.columns.remove(p);
                } else if !ife {
                    let cols: Vec<&str> = table.columns.iter().map(|x| x.name.as_str()).collect();
                    let fix = match nearest(&c.text, cols.iter().copied()) {
                        Some(s) => Some(Fix { start: c.start, end: c.end, text: quote_if_needed(s) }),
                        None => insert_at(toks[k - 1].end, " IF EXISTS".into()),
                    };
                    out.push(err(c.start, c.end, crate::tr_format!("Die Tabelle „{}“ hat keine Spalte „{}“ (Fehler 1091).", "Table “{}” has no column “{}” (error 1091).", table.name, c.text), fix));
                }
            } else if w.is_word("MODIFY") || w.is_word("CHANGE") {
                let mut k = a + 1;
                if next(k).is_some_and(|t| t.is_word("COLUMN")) {
                    k += 1;
                }
                let old = next(k).map(|t| t.text.clone()).unwrap_or_default();
                if w.is_word("CHANGE") {
                    k += 1;
                }
                if let Some(cd) = parse_column(chars, toks, k, b)
                    && let Some(col) = table.columns.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&old))
                {
                    *col = ColumnInfo { key: if cd.col.key.is_empty() { col.key.clone() } else { cd.col.key.clone() }, ..cd.col };
                }
                if let Some(d) = self.cur.as_mut().and_then(|d| d.defs.get_mut(&lower)) {
                    d.types.remove(&old.to_lowercase());
                }
            } else if w.is_word("RENAME") {
                let k = a + 1;
                if next(k).is_some_and(|t| t.is_word("COLUMN")) {
                    if let (Some(o), Some(n)) = (next(k + 1), next(k + 3))
                        && let Some(col) = table.columns.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&o.text))
                    {
                        col.name = n.text.clone();
                    }
                } else {
                    let k = if next(k).is_some_and(|t| t.is_word("TO") || t.is_word("AS")) { k + 1 } else { k };
                    new_name = next(k).filter(|t| t.is_name()).map(|t| t.text.clone());
                }
            }
        }
        let Some(db) = self.cur.as_mut() else { return };
        db.schema.fks.retain(|f| !(f.table.eq_ignore_ascii_case(&lower) && dropped_fks.contains(&f.name.to_lowercase())));
        db.schema.fks.extend(fks);
        if let Some(n) = new_name {
            for f in &mut db.schema.fks {
                if f.table.eq_ignore_ascii_case(&lower) {
                    f.table = n.clone();
                }
                if f.ref_table.eq_ignore_ascii_case(&lower) {
                    f.ref_table = n.clone();
                }
            }
            table.name = n.clone();
            db.defs.remove(&lower);
            if let Some(r) = db.rows.remove(&lower) {
                db.rows.insert(n.to_lowercase(), r);
            }
        }
        if let Some(t) = db.table_mut(&lower) {
            *t = table;
        }
        db.schema.invalidate_index();
    }

    // -- INSERT ------------------------------------------------------------

    fn insert(&mut self, chars: &[char], stmts: &[Stmt], facts: &[Fact], i: usize, out: &mut Vec<Problem>) {
        let st = &stmts[i];
        let toks = st.toks;
        let Some(ins) = parse_insert(toks) else {
            // INSERT ... SELECT / SET: Zeilen unbekannt
            if let Some(k) = toks.iter().position(|t| t.is_word("INTO"))
                && let Some((n, _)) = name_at(toks, k + 1)
                && let Some(r) = self.cur.as_mut().and_then(|d| d.rows.get_mut(&toks[n].text.to_lowercase()))
            {
                r.exact = false;
            }
            return;
        };
        if ins.ti >= 2 && toks[ins.ti - 1].kind == Kind::Dot && !toks[ins.ti - 2].text.eq_ignore_ascii_case(&self.cur_name) {
            return;
        }
        let ttok = &toks[ins.ti];
        let lower = ttok.text.to_lowercase();
        let lenient = toks.iter().take(4).any(|t| t.is_word("IGNORE")) || toks[0].is_word("REPLACE") || toks.iter().any(|t| t.is_word("DUPLICATE"));
        let Some(db) = self.cur.as_ref() else { return };
        let Some(table) = db.table(&lower).cloned() else {
            let names: Vec<&str> = db.schema.tables.iter().map(|t| t.name.as_str()).collect();
            if db.known && ins.list.is_some() && nearest(&ttok.text, names.iter().copied()).is_none() {
                let (sql, created) = self.guess_table(toks, &ins, facts, i);
                out.push(err(
                    ttok.start,
                    ttok.end,
                    crate::tr_format!(
                        "Die Tabelle „{}“ gibt es noch nicht. Korrektur: sie davor anlegen – Typen aus den Werten, mit Primär- und Fremdschlüsseln.",
                        "Table “{}” does not exist yet. Fix: create it before – types from the values, with primary and foreign keys.",
                        ttok.text
                    ),
                    insert_at(st.start, sql),
                ));
                // Weiter so, als gaebe es sie: Folgefehler nur einmal melden
                let db = self.cur.as_mut().expect("db");
                db.schema.tables.push(created);
                db.schema.invalidate_index();
                db.rows.insert(lower, Rows::default());
            }
            return;
        };
        let names: Vec<String> = if ins.list.is_some() {
            ins.cols.iter().map(|&c| toks[c].text.clone()).collect()
        } else {
            table.columns.iter().filter(|c| !c.is_generated()).map(|c| c.name.clone()).collect()
        };
        let def = db.defs.get(&lower);
        // Spalten, die es nicht gibt (Tippfehler meldet `lint` mit Vorschlag)
        let mut added = Vec::new();
        for &c in &ins.cols {
            let ctok = &toks[c];
            let cols: Vec<&str> = table.columns.iter().map(|x| x.name.as_str()).collect();
            if cols.iter().any(|x| x.eq_ignore_ascii_case(&ctok.text)) || nearest(&ctok.text, cols.iter().copied()).is_some() || !added.is_empty() {
                continue;
            }
            let ty = column_guess(facts, i, &lower, &ctok.text.to_lowercase()).sql();
            let col = quote_if_needed(&ctok.text);
            // Im Skript angelegte Tabelle: Spalte gleich dort ergaenzen, sonst ALTER TABLE davor
            let fix = def
                .and_then(|_| self.add_to_create(chars, stmts, i, &lower, &format!("{col} {ty}")))
                .or_else(|| insert_at(st.start, format!("ALTER TABLE {} ADD COLUMN {col} {ty};\n\n", quote_if_needed(&table.name))));
            out.push(err(
                ctok.start,
                ctok.end,
                crate::tr_format!("Die Tabelle „{}“ hat keine Spalte „{}“. Korrektur: Spalte {} anlegen.", "Table “{}” has no column “{}”. Fix: add column {} .", table.name, ctok.text, ty),
                fix,
            ));
            added.push(ColumnInfo { name: ctok.text.clone(), col_type: ty.to_lowercase(), nullable: true, ..Default::default() });
        }
        // Pflichtfelder ohne Wert (nur bei Tabellen aus dem Skript, deren Definition genau bekannt ist)
        if let Some((open, _)) = ins.list.filter(|_| def.is_some()) {
            let missing: Vec<&str> = table
                .columns
                .iter()
                .filter(|c| !c.nullable && c.default.is_none() && !c.extra.to_ascii_lowercase().contains("auto_increment") && !c.is_generated() && !c.col_type.starts_with("timestamp"))
                .filter(|c| !names.iter().any(|n| n.eq_ignore_ascii_case(&c.name)))
                .map(|c| c.name.as_str())
                .collect();
            if !missing.is_empty() {
                let o = &toks[open];
                out.push(err(
                    o.start,
                    o.end,
                    crate::tr_format!(
                        "Hier fehlt {}: NOT NULL ohne Standardwert (Fehler 1364).",
                        "Missing here: {} – NOT NULL without a default (error 1364).",
                        missing.join(", ")
                    ),
                    None,
                ));
            }
        }
        // Werte: Laenge, doppelte Schluessel, Elternzeilen
        let pk = table.pk_columns();
        let pk = (pk.len() == 1).then(|| pk[0].to_lowercase());
        let auto = table.columns.iter().find(|c| c.extra.to_ascii_lowercase().contains("auto_increment")).map(|c| c.name.to_lowercase());
        let fks: Vec<&ForeignKey> = db.schema.fks.iter().filter(|f| f.table.eq_ignore_ascii_case(&lower) && f.columns.len() == 1 && f.ref_columns.len() == 1).collect();
        let rows = db.exact_rows(&lower);
        let mut seen: HashSet<String> = HashSet::new();
        let mut widened = HashSet::new();
        let mut dup_reported = false;
        let mut new_vals: Vec<(String, String)> = Vec::new();
        let mut next_id = db.rows.get(&lower).map_or(0, |r| r.max_id);
        for row in &ins.rows {
            let mut has_id = false;
            for (ci, &(a, b)) in row.iter().enumerate() {
                let Some(cname) = names.get(ci) else { break };
                let cl = cname.to_lowercase();
                let Some(col) = table.columns.iter().find(|c| c.name.eq_ignore_ascii_case(cname)).filter(|_| b > a) else { continue };
                let l = lit(chars, toks, a, b);
                let (vs, ve) = (toks[a].start, toks[b - 1].end);
                if let (Lit::Str(s), Some(max)) = (&l, text_len(&col.col_type))
                    && s.chars().count() > max
                    && widened.insert(cl.clone())
                {
                    let need = column_guess(facts, i, &lower, &cl).max_len;
                    let ty = varchar(need.max(s.chars().count()));
                    out.push(err(
                        vs,
                        ve,
                        crate::tr_format!(
                            "Der Text hat {} Zeichen, „{}“ fasst nur {max} (Fehler 1406). Korrektur: Spalte auf {ty} vergrößern.",
                            "The text has {} characters, “{}” only holds {max} (error 1406). Fix: widen the column to {ty}.",
                            s.chars().count(),
                            col.name
                        ),
                        def.and_then(|d| d.types.get(&cl)).map(|&(s, e)| Fix { start: s, end: e, text: ty.clone() }),
                    ));
                }
                let Some(key) = l.key() else { continue };
                if Some(&cl) == pk.as_ref() {
                    has_id = true;
                    if let Ok(n) = key.parse::<i64>() {
                        next_id = next_id.max(n);
                    }
                    let dup = !seen.insert(key.clone()) || rows.is_some_and(|r| r.vals.get(&cl).is_some_and(|v| v.contains(&key)));
                    if dup && !lenient && rows.is_some() && !dup_reported {
                        dup_reported = true;
                        let fix = (auto.as_ref() == Some(&cl)).then(|| self.without_id(chars, st)).flatten();
                        out.push(err(
                            vs,
                            ve,
                            crate::tr_format!(
                                "Der Schlüssel {} kommt in „{}“ schon vor (Fehler 1062).{}",
                                "Key {} already exists in “{}” (error 1062).{}",
                                text(chars, vs, ve),
                                table.name,
                                if fix.is_some() { crate::i18n::tr(" Korrektur: ID weglassen, AUTO_INCREMENT vergibt sie.", " Fix: leave out the ID, AUTO_INCREMENT assigns it.") } else { "" }
                            ),
                            fix,
                        ));
                    }
                }
                new_vals.push((cl.clone(), key.clone()));
                // Elternzeile vorhanden?
                if !self.fk_checks {
                    continue;
                }
                let Some(f) = fks.iter().find(|f| f.columns[0].eq_ignore_ascii_case(cname)) else { continue };
                let pl = f.ref_table.to_lowercase();
                let rc = f.ref_columns[0].to_lowercase();
                let Some(prows) = db.exact_rows(&pl) else { continue };
                if prows.vals.get(&rc).is_some_and(|v| v.contains(&key)) || (pl == lower && seen.contains(&key)) {
                    continue;
                }
                // Kommt die Elternzeile weiter unten? Auch ueber die dort vergebenen AUTO_INCREMENT-Werte
                let later = self.inserted_later(facts, &pl, &rc, &key, i);
                let shown = text(chars, vs, ve);
                match later {
                    Some(j) => out.push(err(
                        vs,
                        ve,
                        crate::tr_format!(
                            "{shown} gibt es in „{}“ erst weiter unten (Fehler 1452). Korrektur: diese Anweisung nach unten schieben.",
                            "{shown} only exists in “{}” further down (error 1452). Fix: move this statement down.",
                            f.ref_table
                        ),
                        (stmts[j].end - st.start < MOVE_MAX).then(|| move_down(chars, st, &stmts[j])),
                    )),
                    None => out.push(err(
                        vs,
                        ve,
                        crate::tr_format!(
                            "{shown} gibt es in „{}.{}“ nicht – die Zeile verweist ins Leere (Fehler 1452).",
                            "{shown} does not exist in “{}.{}” – the row points nowhere (error 1452).",
                            f.ref_table,
                            f.ref_columns[0]
                        ),
                        None,
                    )),
                }
            }
            if !has_id && let Some(a) = &auto {
                next_id += 1;
                new_vals.push((a.clone(), next_id.to_string()));
            }
        }
        let Some(db) = self.cur.as_mut() else { return };
        if !added.is_empty()
            && let Some(t) = db.table_mut(&lower)
        {
            t.columns.extend(added);
        }
        let r = db.rows.entry(lower).or_default();
        r.max_id = r.max_id.max(next_id);
        for (c, v) in new_vals {
            r.vals.entry(c).or_default().insert(v);
        }
        if lenient {
            r.exact = false;
        }
    }

    /// Erste Anweisung nach `i`, die in `table.col` den Wert `key` einfuegt (Index wird einmal je Spalte gebaut)
    fn inserted_later(&self, facts: &[Fact], table: &str, col: &str, key: &str, i: usize) -> Option<usize> {
        let mut cache = self.future.borrow_mut();
        let index = cache.entry((table.to_string(), col.to_string())).or_insert_with(|| {
            let info = self.cur.as_ref().and_then(|d| d.table(table));
            let auto = info.is_some_and(|t| t.columns.iter().any(|c| c.name.eq_ignore_ascii_case(col) && c.extra.contains("auto_increment")));
            let mut m: Inserted = HashMap::new();
            let mut next = 0i64;
            for (j, f) in facts.iter().enumerate() {
                let Fact::Insert { table: t, cols, rows } = f else { continue };
                if t != table {
                    continue;
                }
                let pos = match cols {
                    Some(c) => c.iter().position(|c| c == col),
                    None => info.and_then(|t| t.columns.iter().filter(|c| !c.is_generated()).position(|c| c.name.eq_ignore_ascii_case(col))),
                };
                for r in rows {
                    match pos.and_then(|p| r.get(p)).and_then(Lit::key) {
                        Some(v) => {
                            if let Ok(n) = v.parse::<i64>() {
                                next = next.max(n);
                            }
                            m.entry(v).or_default().push(j);
                        }
                        None if auto => {
                            next += 1;
                            m.entry(next.to_string()).or_default().push(j);
                        }
                        None => {}
                    }
                }
            }
            m
        });
        index.get(key)?.iter().copied().find(|&j| j > i)
    }

    /// Spalte in das CREATE TABLE dieses Skripts einfuegen (vor der schliessenden Klammer)
    fn add_to_create(&self, chars: &[char], stmts: &[Stmt], i: usize, lower: &str, coldef: &str) -> Option<Fix> {
        let st = stmts[..i].iter().rev().find(|s| matches!(fact(chars, s.toks), Fact::Create(n) if n == lower))?;
        let open = st.toks.iter().position(|t| t.kind == Kind::Open)?;
        let close = group_end(st.toks, open)?;
        let last = &st.toks[close - 1];
        let ws: String = chars[st.toks[open].end..].iter().take_while(|c| c.is_whitespace()).collect();
        let indent = ws.rsplit('\n').next().unwrap_or("");
        let sep = if ws.contains('\n') { format!(",\n{indent}") } else { ", ".into() };
        insert_at(last.end, format!("{sep}{coldef}"))
    }

    fn without_id(&self, chars: &[char], st: &Stmt) -> Option<Fix> {
        let schema = &self.cur.as_ref()?.schema;
        let sc: Vec<char> = chars[st.start..st.end].to_vec();
        let toks = super::lint::lex(&sc).toks;
        let new = super::errfix::without_auto_id(&sc, &toks, schema)?;
        Some(Fix { start: st.start, end: st.end, text: new })
    }

    /// CREATE TABLE fuer ein INSERT in eine fehlende Tabelle: Typen aus allen Werten des Skripts, Primaerschluessel
    /// (Spalte id/<tabelle>id oder neu mit AUTO_INCREMENT), Fremdschluessel ueber gleichnamige Schluesselspalten
    fn guess_table(&self, toks: &[Tok], ins: &Insert, facts: &[Fact], i: usize) -> (String, TableInfo) {
        let db = self.cur.as_ref().expect("db");
        let tname = &toks[ins.ti].text;
        let tl = tname.to_lowercase();
        let stems = singular(&tl);
        let cols: Vec<String> = ins.cols.iter().map(|&c| toks[c].text.clone()).collect();
        let guesses: Vec<Guess> = cols.iter().map(|c| column_guess(facts, i, &tl, &c.to_lowercase())).collect();
        // Fremdschluessel: Spalte heisst wie der Schluessel einer anderen Tabelle oder <tabelle>id
        let parent_of = |c: &str| -> Option<(&TableInfo, &ColumnInfo)> {
            let c = c.to_lowercase();
            db.schema.tables.iter().filter(|p| !p.name.eq_ignore_ascii_case(tname)).find_map(|p| {
                let pk = p.pk_columns();
                let [pk] = pk.as_slice() else { return None };
                let pc = p.columns.iter().find(|x| &x.name == pk)?;
                let pl = p.name.to_lowercase();
                let named = pk.eq_ignore_ascii_case(&c) && c != "id";
                let by_table = singular(&pl).iter().any(|s| [format!("{s}id"), format!("{s}_id"), format!("{s}nr"), format!("{s}_nr")].contains(&c));
                (named || by_table).then_some((p, pc))
            })
        };
        let pk_names: Vec<String> = stems.iter().flat_map(|s| [format!("{s}id"), format!("{s}_id"), format!("{s}nr"), format!("{s}_nr"), format!("{s}nummer")]).collect();
        let pk = cols
            .iter()
            .position(|c| c.eq_ignore_ascii_case("id"))
            .or_else(|| cols.iter().position(|c| pk_names.contains(&c.to_lowercase())))
            .or_else(|| {
                let c = cols.first()?.to_lowercase();
                (["id", "nr", "no"].iter().any(|s| c.ends_with(s)) && parent_of(&c).is_none() && guesses[0].all_int() && guesses[0].unique()).then_some(0)
            });
        let mut lines = Vec::new();
        let mut table = TableInfo { name: tname.clone(), engine: "InnoDB".into(), ..Default::default() };
        if pk.is_none() {
            lines.push("id INT AUTO_INCREMENT PRIMARY KEY".to_string());
            table.columns.push(ColumnInfo { name: "id".into(), col_type: "int".into(), key: "PRI".into(), extra: "auto_increment".into(), ..Default::default() });
        }
        let mut fk_lines = Vec::new();
        for (k, c) in cols.iter().enumerate() {
            let mut ty = guesses[k].sql();
            let mut extra = String::new();
            let mut col = ColumnInfo { name: c.clone(), nullable: true, ..Default::default() };
            if Some(k) == pk {
                if ty == "INT" || ty == "BIGINT" {
                    extra.push_str(" AUTO_INCREMENT");
                    col.extra = "auto_increment".into();
                }
                extra.push_str(" PRIMARY KEY");
                col.key = "PRI".into();
                col.nullable = false;
            } else if let Some((p, pc)) = parent_of(c) {
                ty = super::errfix::norm_type(&pc.col_type);
                fk_lines.push(format!("FOREIGN KEY ({}) REFERENCES {} ({})", quote_if_needed(c), quote_if_needed(&p.name), quote_if_needed(&pc.name)));
            }
            col.col_type = ty.to_lowercase();
            lines.push(format!("{} {ty}{extra}", quote_if_needed(c)));
            table.columns.push(col);
        }
        lines.extend(fk_lines);
        (format!("CREATE TABLE {} (\n    {}\n);\n\n", quote_if_needed(tname), lines.join(",\n    ")), table)
    }
}

/// Werte einer Spalte aus allen INSERTs ab Anweisung `from`
fn column_guess(facts: &[Fact], from: usize, table: &str, col: &str) -> Guess {
    let mut g = Guess::default();
    for f in &facts[from..] {
        if let Fact::Insert { table: t, cols: Some(cols), rows } = f
            && t == table
            && let Some(p) = cols.iter().position(|c| c == col)
        {
            for r in rows {
                if let Some(l) = r.get(p) {
                    g.add(l);
                }
            }
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use super::super::lint::{Env, apply_fixes, lint};
    use crate::db::{ColumnInfo, Schema, TableInfo};

    fn schema() -> Schema {
        let col = |n: &str, t: &str, key: &str, extra: &str| ColumnInfo { name: n.into(), col_type: t.into(), key: key.into(), extra: extra.into(), nullable: key.is_empty(), ..Default::default() };
        Schema {
            name: "schule".into(),
            tables: vec![TableInfo { name: "Klasse".into(), columns: vec![col("kid", "int(11)", "PRI", "auto_increment"), col("name", "varchar(10)", "", "")], ..Default::default() }],
            ..Default::default()
        }
    }

    fn msgs(sql: &str) -> Vec<String> {
        let s = schema();
        lint(sql, &Env { schema: Some(&s), databases: &["schule".to_string()] }).into_iter().map(|p| p.message).collect()
    }

    fn fixed(sql: &str) -> String {
        let s = schema();
        let p = lint(sql, &Env { schema: Some(&s), databases: &["schule".to_string()] });
        apply_fixes(sql, &p).0
    }

    #[test]
    fn tables_from_the_script_are_known() {
        let sql = "CREATE TABLE Schueler (sid INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(20) NOT NULL, kid INT, FOREIGN KEY (kid) REFERENCES Klasse(kid));\nINSERT INTO Schueler (name, kid) VALUES ('Anna', 1);\nSELECT name FROM Schueler;";
        assert_eq!(msgs(sql), Vec::<String>::new());
        // Fehlerfreies Schulskript mit neuer Datenbank
        let sql = "DROP DATABASE IF EXISTS firma;\nCREATE DATABASE firma;\nUSE firma;\nCREATE TABLE p (id INT PRIMARY KEY);\nCREATE TABLE c (id INT PRIMARY KEY, pid INT, FOREIGN KEY (pid) REFERENCES p (id));\nINSERT INTO p VALUES (1), (2);\nINSERT INTO c VALUES (1, 2);\nDROP TABLE c, p;";
        assert_eq!(msgs(sql), Vec::<String>::new());
    }

    #[test]
    fn order_of_statements_is_repaired() {
        // Elterntabelle erst weiter unten
        let sql = "CREATE TABLE c (id INT PRIMARY KEY, pid INT, FOREIGN KEY (pid) REFERENCES p (id));\n\nCREATE TABLE p (id INT PRIMARY KEY);";
        assert_eq!(fixed(sql), "CREATE TABLE p (id INT PRIMARY KEY);\n\nCREATE TABLE c (id INT PRIMARY KEY, pid INT, FOREIGN KEY (pid) REFERENCES p (id));");
        // Elternzeile erst weiter unten
        let sql = "CREATE TABLE p (id INT PRIMARY KEY);\nCREATE TABLE c (id INT PRIMARY KEY, pid INT REFERENCES p (id));\nINSERT INTO c VALUES (1, 7);\nINSERT INTO p VALUES (7);";
        assert!(fixed(sql).ends_with("INSERT INTO p VALUES (7);\nINSERT INTO c VALUES (1, 7);"), "{}", fixed(sql));
        // DROP: Kind vor Eltern
        let sql = "CREATE TABLE p (id INT PRIMARY KEY);\nCREATE TABLE c (id INT PRIMARY KEY, pid INT REFERENCES p (id));\nDROP TABLE p, c;";
        assert!(fixed(sql).ends_with("DROP TABLE c, p;"));
        let sql = "CREATE TABLE p (id INT PRIMARY KEY);\nCREATE TABLE c (id INT PRIMARY KEY, pid INT REFERENCES p (id));\nDROP TABLE p;";
        assert!(fixed(sql).ends_with("DROP TABLE c, p;"));
    }

    #[test]
    fn missing_tables_are_created_from_the_inserts() {
        let sql = "INSERT INTO Schueler (sid, vorname, kid, geboren, schnitt) VALUES (1, 'Anna', 1, '2008-03-01', 1.5), (2, 'Ben', 2, '12.5.2009', 2.25);\nINSERT INTO Schueler (sid, vorname, kid, geboren, schnitt) VALUES (3, 'Ca', 1, NULL, 3);";
        let out = fixed(sql);
        assert!(
            out.starts_with(
                "CREATE TABLE Schueler (\n    sid INT AUTO_INCREMENT PRIMARY KEY,\n    vorname VARCHAR(50),\n    kid INT,\n    geboren DATE,\n    schnitt DECIMAL(8, 2),\n    FOREIGN KEY (kid) REFERENCES Klasse (kid)\n);\n\nINSERT INTO Schueler"
            ),
            "{out}"
        );
        // Nur einmal: die zweite Anweisung kennt die Tabelle dann
        assert_eq!(msgs(sql).len(), 2, "{:?}", msgs(sql)); // + deutsches Datum
        // Ohne Schluesselspalte: id ergaenzen
        assert!(fixed("INSERT INTO Notiz (text) VALUES ('x');").starts_with("CREATE TABLE Notiz (\n    id INT AUTO_INCREMENT PRIMARY KEY,\n    text VARCHAR(50)\n);"));
    }

    #[test]
    fn predicted_runtime_errors() {
        let base = "CREATE TABLE t (id INT AUTO_INCREMENT PRIMARY KEY, kurz VARCHAR(3), pflicht INT NOT NULL);\n";
        let m = msgs(&format!("{base}INSERT INTO t (id, kurz, pflicht) VALUES (1, 'a', 1), (1, 'b', 2);"));
        assert!(m.iter().any(|m| m.contains("1062")), "{m:?}");
        assert_eq!(fixed(&format!("{base}INSERT INTO t (id, kurz, pflicht) VALUES (1, 'a', 1), (1, 'b', 2);")), format!("{base}INSERT INTO t (kurz, pflicht) VALUES ('a', 1), ('b', 2);"));
        assert!(msgs(&format!("{base}INSERT INTO t (kurz) VALUES ('a');")).iter().any(|m| m.contains("1364")));
        assert_eq!(fixed(&format!("{base}INSERT INTO t (kurz, pflicht) VALUES ('abcdef', 1);")), format!("{base}INSERT INTO t (kurz, pflicht) VALUES ('abcdef', 1);").replace("VARCHAR(3)", "VARCHAR(50)"));
        // Fremdschluessel: falscher Typ, fehlender Index, Elternzeile fehlt
        assert_eq!(fixed("CREATE TABLE c (x BIGINT, FOREIGN KEY (x) REFERENCES Klasse (kid));"), "CREATE TABLE c (x INT, FOREIGN KEY (x) REFERENCES Klasse (kid));");
        assert_eq!(fixed("CREATE TABLE p (id INT PRIMARY KEY, code VARCHAR(5));\nCREATE TABLE c (x VARCHAR(9) REFERENCES p (code));"), "CREATE TABLE p (id INT PRIMARY KEY, code VARCHAR(5) UNIQUE);\nCREATE TABLE c (x VARCHAR(9) REFERENCES p (code));");
        let m = msgs("CREATE TABLE p (id INT PRIMARY KEY);\nCREATE TABLE c (pid INT REFERENCES p (id));\nINSERT INTO p VALUES (1);\nINSERT INTO c VALUES (2);");
        assert!(m.iter().any(|m| m.contains("1452")), "{m:?}");
        // Auto-ID der Eltern wird mitgezaehlt
        assert_eq!(msgs("CREATE TABLE p (id INT AUTO_INCREMENT PRIMARY KEY, n INT);\nCREATE TABLE c (pid INT REFERENCES p (id));\nINSERT INTO p (n) VALUES (5), (6);\nINSERT INTO c VALUES (2);"), Vec::<String>::new());
        // Schon vorhanden
        assert_eq!(fixed("CREATE TABLE Klasse (kid INT);"), "CREATE TABLE IF NOT EXISTS Klasse (kid INT);");
        assert_eq!(msgs("SET FOREIGN_KEY_CHECKS = 0;\nCREATE TABLE c (pid INT REFERENCES spaeter (id));"), Vec::<String>::new());
    }

    #[test]
    fn missing_columns_are_added() {
        assert_eq!(
            fixed("CREATE TABLE t (\n    id INT PRIMARY KEY\n);\nINSERT INTO t (id, preis) VALUES (1, 2.5);"),
            "CREATE TABLE t (\n    id INT PRIMARY KEY,\n    preis DECIMAL(8, 1)\n);\nINSERT INTO t (id, preis) VALUES (1, 2.5);"
        );
        assert_eq!(fixed("INSERT INTO Klasse (kid, raum) VALUES (1, 'A1');"), "ALTER TABLE Klasse ADD COLUMN raum VARCHAR(50);\n\nINSERT INTO Klasse (kid, raum) VALUES (1, 'A1');");
    }
}
