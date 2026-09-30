// Datenmodell des Abfrage-Assistenten und Erzeugung des SQL-Textes.
// Abgedeckt: WITH (auch RECURSIVE), UNION/EXCEPT/INTERSECT (je mit ALL), alle JOIN-Arten,
// Unterabfragen (als Tabelle, Wert, IN/ANY/ALL, EXISTS), Ausdruecke, Funktionen,
// Fensterfunktionen (OVER), CASE, CAST, GROUP BY mit ROLLUP, HAVING, ORDER BY, LIMIT/OFFSET.

use crate::db::{lit, q};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Query {
    #[serde(default)]
    pub with: Vec<Cte>,
    #[serde(default)]
    pub recursive: bool,
    pub parts: Vec<Part>,
    /// Sortierung/Begrenzung des Gesamtergebnisses (bei mehreren Teilen)
    #[serde(default)]
    pub order: Vec<OrderItem>,
    #[serde(default)]
    pub limit: String,
    #[serde(default)]
    pub offset: String,
}

impl Default for Query {
    fn default() -> Self {
        Self { with: Vec::new(), recursive: false, parts: vec![Part::default()], order: Vec::new(), limit: String::new(), offset: String::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Cte {
    pub name: String,
    /// optionale Spaltennamen, durch Komma getrennt
    #[serde(default)]
    pub columns: String,
    pub query: Query,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Part {
    pub op: SetOp,
    pub select: Select,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum SetOp {
    #[default]
    Union,
    UnionAll,
    Except,
    ExceptAll,
    Intersect,
    IntersectAll,
}

impl SetOp {
    pub const ALL: [SetOp; 6] = [SetOp::Union, SetOp::UnionAll, SetOp::Except, SetOp::ExceptAll, SetOp::Intersect, SetOp::IntersectAll];
    pub fn sql(self) -> &'static str {
        match self {
            SetOp::Union => "UNION",
            SetOp::UnionAll => "UNION ALL",
            SetOp::Except => "EXCEPT",
            SetOp::ExceptAll => "EXCEPT ALL",
            SetOp::Intersect => "INTERSECT",
            SetOp::IntersectAll => "INTERSECT ALL",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            SetOp::Union => "Zeilen beider Teile, doppelte nur einmal",
            SetOp::UnionAll => "Zeilen beider Teile, mit doppelten",
            SetOp::Except => "Zeilen des ersten Teils, die im zweiten nicht vorkommen",
            SetOp::ExceptAll => "wie EXCEPT, doppelte werden einzeln abgezogen",
            SetOp::Intersect => "nur Zeilen, die in beiden Teilen vorkommen",
            SetOp::IntersectAll => "wie INTERSECT, mit doppelten",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Select {
    #[serde(default)]
    pub distinct: bool,
    #[serde(default)]
    pub sources: Vec<Source>,
    /// leer = *
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default = "Cond::group")]
    pub filter: Cond,
    #[serde(default)]
    pub group: Vec<Expr>,
    #[serde(default)]
    pub rollup: bool,
    #[serde(default = "Cond::group")]
    pub having: Cond,
    #[serde(default)]
    pub order: Vec<OrderItem>,
    #[serde(default)]
    pub limit: String,
    #[serde(default)]
    pub offset: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub join: JoinKind,
    pub from: From,
    #[serde(default)]
    pub alias: String,
    #[serde(default = "Cond::group")]
    pub on: Cond,
    /// Spalten fuer USING, durch Komma getrennt (statt ON)
    #[serde(default)]
    pub using: String,
}

impl Source {
    pub fn table(name: &str) -> Self {
        Self { join: JoinKind::Inner, from: From::Table(name.to_string()), alias: String::new(), on: Cond::group(), using: String::new() }
    }

    /// Name, unter dem die Spalten angesprochen werden
    pub fn reference(&self) -> String {
        if !self.alias.trim().is_empty() {
            return self.alias.trim().to_string();
        }
        match &self.from {
            From::Table(t) | From::Cte(t) => t.clone(),
            From::Sub(_) => "unterabfrage".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum From {
    Table(String),
    Cte(String),
    Sub(Box<Query>),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Cross,
    Natural,
    NaturalLeft,
    NaturalRight,
}

impl JoinKind {
    pub const ALL: [JoinKind; 7] = [JoinKind::Inner, JoinKind::Left, JoinKind::Right, JoinKind::Cross, JoinKind::Natural, JoinKind::NaturalLeft, JoinKind::NaturalRight];
    pub fn sql(self) -> &'static str {
        match self {
            JoinKind::Inner => "INNER JOIN",
            JoinKind::Left => "LEFT JOIN",
            JoinKind::Right => "RIGHT JOIN",
            JoinKind::Cross => "CROSS JOIN",
            JoinKind::Natural => "NATURAL JOIN",
            JoinKind::NaturalLeft => "NATURAL LEFT JOIN",
            JoinKind::NaturalRight => "NATURAL RIGHT JOIN",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            JoinKind::Inner => "nur Zeilen mit Partner in beiden Tabellen",
            JoinKind::Left => "alle Zeilen links, rechts NULL ohne Partner",
            JoinKind::Right => "alle Zeilen rechts, links NULL ohne Partner",
            JoinKind::Cross => "jede Zeile mit jeder (kartesisches Produkt)",
            JoinKind::Natural => "verknüpft über alle gleichnamigen Spalten",
            JoinKind::NaturalLeft => "wie NATURAL JOIN, alle Zeilen links",
            JoinKind::NaturalRight => "wie NATURAL JOIN, alle Zeilen rechts",
        }
    }
    /// braucht ON/USING
    pub fn has_condition(self) -> bool {
        matches!(self, JoinKind::Inner | JoinKind::Left | JoinKind::Right)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub expr: Expr,
    #[serde(default)]
    pub alias: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderItem {
    pub expr: Expr,
    #[serde(default)]
    pub desc: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    /// Spalte; `col` = "*" fuer alle Spalten einer Tabelle, `src` leer = ohne Tabellenname (z. B. Alias)
    Column { src: String, col: String },
    /// *
    All,
    /// Wert: Zahl, Text, NULL, TRUE/FALSE, @variable
    Value(String),
    Func {
        name: String,
        #[serde(default)]
        args: Vec<Expr>,
        #[serde(default)]
        distinct: bool,
        #[serde(default)]
        over: Option<Window>,
    },
    Op { op: String, left: Box<Expr>, right: Box<Expr> },
    Case {
        whens: Vec<(Cond, Expr)>,
        #[serde(default)]
        otherwise: Option<Box<Expr>>,
    },
    Cast { expr: Box<Expr>, ty: String },
    Sub(Box<Query>),
    /// freier SQL-Ausdruck
    Raw(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Window {
    #[serde(default)]
    pub partition: Vec<Expr>,
    #[serde(default)]
    pub order: Vec<OrderItem>,
    /// z. B. ROWS BETWEEN 2 PRECEDING AND CURRENT ROW
    #[serde(default)]
    pub frame: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Cond {
    Group {
        /// true = ODER, false = UND
        any: bool,
        #[serde(default)]
        not: bool,
        items: Vec<Cond>,
    },
    Cmp {
        left: Expr,
        op: CmpOp,
        #[serde(default)]
        right: Vec<Expr>,
        #[serde(default)]
        quant: Quant,
        /// Unterabfrage statt Werten (IN, = ANY, > ALL, ...)
        #[serde(default)]
        sub: Option<Box<Query>>,
    },
    Exists { not: bool, query: Box<Query> },
    Raw(String),
}

impl Cond {
    pub fn group() -> Cond {
        Cond::Group { any: false, not: false, items: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Cond::Group { items, .. } => items.iter().all(|c| c.is_empty()),
            Cond::Raw(s) => s.trim().is_empty(),
            _ => false,
        }
    }
}

impl Default for Cond {
    fn default() -> Self {
        Cond::group()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    NullSafeEq,
    Like,
    NotLike,
    Regexp,
    NotRegexp,
    In,
    NotIn,
    Between,
    NotBetween,
    IsNull,
    IsNotNull,
}

impl CmpOp {
    pub const ALL: [CmpOp; 17] = [
        CmpOp::Eq,
        CmpOp::Ne,
        CmpOp::Lt,
        CmpOp::Le,
        CmpOp::Gt,
        CmpOp::Ge,
        CmpOp::NullSafeEq,
        CmpOp::Like,
        CmpOp::NotLike,
        CmpOp::Regexp,
        CmpOp::NotRegexp,
        CmpOp::In,
        CmpOp::NotIn,
        CmpOp::Between,
        CmpOp::NotBetween,
        CmpOp::IsNull,
        CmpOp::IsNotNull,
    ];
    pub fn sql(self) -> &'static str {
        match self {
            CmpOp::Eq => "=",
            CmpOp::Ne => "<>",
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
            CmpOp::NullSafeEq => "<=>",
            CmpOp::Like => "LIKE",
            CmpOp::NotLike => "NOT LIKE",
            CmpOp::Regexp => "REGEXP",
            CmpOp::NotRegexp => "NOT REGEXP",
            CmpOp::In => "IN",
            CmpOp::NotIn => "NOT IN",
            CmpOp::Between => "BETWEEN",
            CmpOp::NotBetween => "NOT BETWEEN",
            CmpOp::IsNull => "IS NULL",
            CmpOp::IsNotNull => "IS NOT NULL",
        }
    }
    /// Anzahl der Werte rechts: None = Liste
    pub fn arity(self) -> Option<usize> {
        match self {
            CmpOp::IsNull | CmpOp::IsNotNull => Some(0),
            CmpOp::Between | CmpOp::NotBetween => Some(2),
            CmpOp::In | CmpOp::NotIn => None,
            _ => Some(1),
        }
    }
    /// Unterabfrage moeglich
    pub fn allows_sub(self) -> bool {
        matches!(self, CmpOp::Eq | CmpOp::Ne | CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge | CmpOp::In | CmpOp::NotIn)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum Quant {
    #[default]
    None,
    Any,
    All,
}

// ---------------------------------------------------------------------------
// Funktionen

pub struct FuncDef {
    pub name: &'static str,
    pub group: &'static str,
    /// Standard-Argumente beim Einfuegen: "c" = Spalte, "*" = *, "0" = Zahl, sonst SQL-Text
    pub args: &'static [&'static str],
    pub aggregate: bool,
    pub window: bool,
    pub hint: &'static str,
}

const fn f(name: &'static str, group: &'static str, args: &'static [&'static str], hint: &'static str) -> FuncDef {
    FuncDef { name, group, args, aggregate: false, window: false, hint }
}
const fn agg(name: &'static str, args: &'static [&'static str], hint: &'static str) -> FuncDef {
    FuncDef { name, group: "Zusammenfassen", args, aggregate: true, window: true, hint }
}
const fn win(name: &'static str, args: &'static [&'static str], hint: &'static str) -> FuncDef {
    FuncDef { name, group: "Fenster (OVER)", args, aggregate: false, window: true, hint }
}

pub const FUNCS: &[FuncDef] = &[
    agg("COUNT", &["*"], "Anzahl der Zeilen bzw. Werte ungleich NULL"),
    agg("SUM", &["c"], "Summe"),
    agg("AVG", &["c"], "Durchschnitt"),
    agg("MIN", &["c"], "kleinster Wert"),
    agg("MAX", &["c"], "größter Wert"),
    agg("GROUP_CONCAT", &["c"], "Werte als Text verketten"),
    agg("STDDEV", &["c"], "Standardabweichung"),
    agg("VARIANCE", &["c"], "Varianz"),
    win("ROW_NUMBER", &[], "laufende Nummer"),
    win("RANK", &[], "Rang mit Lücken"),
    win("DENSE_RANK", &[], "Rang ohne Lücken"),
    win("NTILE", &["4"], "in n gleich große Gruppen teilen"),
    win("LAG", &["c", "1"], "Wert der vorherigen Zeile"),
    win("LEAD", &["c", "1"], "Wert der nächsten Zeile"),
    win("FIRST_VALUE", &["c"], "erster Wert im Fenster"),
    win("LAST_VALUE", &["c"], "letzter Wert im Fenster"),
    win("PERCENT_RANK", &[], "relativer Rang 0 … 1"),
    win("CUME_DIST", &[], "kumulierte Verteilung"),
    f("CONCAT", "Text", &["c", "' '"], "Texte verbinden"),
    f("CONCAT_WS", "Text", &["', '", "c"], "mit Trennzeichen verbinden"),
    f("UPPER", "Text", &["c"], "Großbuchstaben"),
    f("LOWER", "Text", &["c"], "Kleinbuchstaben"),
    f("LENGTH", "Text", &["c"], "Länge in Bytes"),
    f("CHAR_LENGTH", "Text", &["c"], "Länge in Zeichen"),
    f("SUBSTRING", "Text", &["c", "1", "3"], "Teil (Text, Start, Länge)"),
    f("LEFT", "Text", &["c", "3"], "Zeichen von links"),
    f("RIGHT", "Text", &["c", "3"], "Zeichen von rechts"),
    f("TRIM", "Text", &["c"], "Leerzeichen entfernen"),
    f("REPLACE", "Text", &["c", "'a'", "'b'"], "ersetzen"),
    f("LOCATE", "Text", &["'a'", "c"], "Position von Text"),
    f("LPAD", "Text", &["c", "5", "'0'"], "links auffüllen"),
    f("REVERSE", "Text", &["c"], "umdrehen"),
    f("FORMAT", "Text", &["c", "2"], "Zahl formatieren"),
    f("ROUND", "Zahl", &["c", "2"], "runden"),
    f("CEIL", "Zahl", &["c"], "aufrunden"),
    f("FLOOR", "Zahl", &["c"], "abrunden"),
    f("TRUNCATE", "Zahl", &["c", "2"], "abschneiden"),
    f("ABS", "Zahl", &["c"], "Betrag"),
    f("MOD", "Zahl", &["c", "2"], "Rest"),
    f("POWER", "Zahl", &["c", "2"], "Potenz"),
    f("SQRT", "Zahl", &["c"], "Wurzel"),
    f("RAND", "Zahl", &[], "Zufallszahl 0 … 1"),
    f("GREATEST", "Zahl", &["c", "0"], "größter der Werte"),
    f("LEAST", "Zahl", &["c", "0"], "kleinster der Werte"),
    f("NOW", "Datum", &[], "jetzt"),
    f("CURDATE", "Datum", &[], "heute"),
    f("DATE", "Datum", &["c"], "Datumsteil"),
    f("YEAR", "Datum", &["c"], "Jahr"),
    f("MONTH", "Datum", &["c"], "Monat"),
    f("DAY", "Datum", &["c"], "Tag"),
    f("HOUR", "Datum", &["c"], "Stunde"),
    f("WEEK", "Datum", &["c"], "Kalenderwoche"),
    f("WEEKDAY", "Datum", &["c"], "Wochentag (0 = Montag)"),
    f("DAYNAME", "Datum", &["c"], "Name des Wochentags"),
    f("MONTHNAME", "Datum", &["c"], "Name des Monats"),
    f("DATE_FORMAT", "Datum", &["c", "'%d.%m.%Y'"], "Datum als Text"),
    f("DATE_ADD", "Datum", &["c", "INTERVAL 1 DAY"], "Zeitraum addieren"),
    f("DATE_SUB", "Datum", &["c", "INTERVAL 1 DAY"], "Zeitraum abziehen"),
    f("DATEDIFF", "Datum", &["NOW()", "c"], "Tage zwischen zwei Daten"),
    f("TIMESTAMPDIFF", "Datum", &["YEAR", "c", "NOW()"], "Abstand in Einheit (YEAR, MONTH, DAY, …)"),
    f("LAST_DAY", "Datum", &["c"], "letzter Tag des Monats"),
    f("IF", "Logik", &["c > 0", "'ja'", "'nein'"], "wenn, dann, sonst"),
    f("IFNULL", "Logik", &["c", "0"], "Ersatz für NULL"),
    f("COALESCE", "Logik", &["c", "0"], "erster Wert ungleich NULL"),
    f("NULLIF", "Logik", &["c", "0"], "NULL, wenn gleich"),
];

pub fn func_def(name: &str) -> Option<&'static FuncDef> {
    FUNCS.iter().find(|f| f.name.eq_ignore_ascii_case(name))
}

pub const TYPES: &[&str] = &["CHAR", "SIGNED", "UNSIGNED", "DECIMAL(10,2)", "DOUBLE", "DATE", "DATETIME", "TIME", "JSON"];
pub const OPS: &[&str] = &["+", "-", "*", "/", "DIV", "%"];

// ---------------------------------------------------------------------------
// SQL erzeugen

/// Wert als SQL: Zahlen, NULL, TRUE/FALSE und @Variablen bleiben, schon gequoteter Text bleibt,
/// alles andere wird zu Text.
pub fn value_sql(v: &str) -> String {
    let t = v.trim();
    let upper = t.to_ascii_uppercase();
    let number = !t.is_empty() && t.parse::<f64>().is_ok() && !(t.len() > 1 && t.starts_with('0') && !t.starts_with("0."));
    let quoted = t.len() >= 2 && ((t.starts_with('\'') && t.ends_with('\'')) || (t.starts_with('"') && t.ends_with('"')));
    if number || quoted || matches!(upper.as_str(), "NULL" | "TRUE" | "FALSE" | "DEFAULT") || (t.starts_with('@') && t.len() > 1) {
        t.to_string()
    } else {
        lit(v)
    }
}

fn list(items: &[String]) -> String {
    items.join(", ")
}

fn idents(csv: &str) -> String {
    csv.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).map(q).collect::<Vec<_>>().join(", ")
}

impl Expr {
    pub fn column(src: &str, col: &str) -> Self {
        Expr::Column { src: src.to_string(), col: col.to_string() }
    }

    pub fn func(name: &str, first_col: Option<Expr>) -> Self {
        let def = func_def(name);
        let args = def
            .map(|d| {
                d.args
                    .iter()
                    .map(|a| match *a {
                        "c" => first_col.clone().unwrap_or(Expr::Value("0".into())),
                        "*" => Expr::All,
                        x if x.parse::<f64>().is_ok() || x.starts_with('\'') => Expr::Value(x.into()),
                        x => Expr::Raw(x.into()),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let over = def.filter(|d| d.window && !d.aggregate).map(|_| Window::default());
        Expr::Func { name: name.to_string(), args, distinct: false, over }
    }

    pub fn is_aggregate(&self) -> bool {
        match self {
            Expr::Func { name, over: None, .. } => func_def(name).is_some_and(|d| d.aggregate),
            Expr::Func { args, .. } => args.iter().any(|a| a.is_aggregate()),
            Expr::Op { left, right, .. } => left.is_aggregate() || right.is_aggregate(),
            Expr::Cast { expr, .. } => expr.is_aggregate(),
            Expr::Case { whens, otherwise } => whens.iter().any(|(_, e)| e.is_aggregate()) || otherwise.as_ref().is_some_and(|e| e.is_aggregate()),
            Expr::Raw(r) => {
                let u = r.to_ascii_uppercase();
                ["COUNT(", "SUM(", "AVG(", "MIN(", "MAX(", "GROUP_CONCAT("].iter().any(|k| u.contains(k))
            }
            _ => false,
        }
    }

    /// Fensterfunktion (OVER) – gehoert nie in GROUP BY
    pub fn is_window(&self) -> bool {
        match self {
            Expr::Func { over: Some(_), .. } => true,
            Expr::Op { left, right, .. } => left.is_window() || right.is_window(),
            Expr::Cast { expr, .. } => expr.is_window(),
            Expr::Raw(r) => r.to_ascii_uppercase().contains(" OVER"),
            _ => false,
        }
    }

    pub fn sql(&self) -> String {
        match self {
            Expr::Column { src, col } => {
                let c = if col == "*" { "*".to_string() } else { q(col) };
                if src.is_empty() { c } else { format!("{}.{c}", q(src)) }
            }
            Expr::All => "*".into(),
            Expr::Value(v) => value_sql(v),
            Expr::Func { name, args, distinct, over } => {
                let mut a: Vec<String> = args.iter().map(|x| x.sql()).collect();
                if a.is_empty() && name.eq_ignore_ascii_case("COUNT") {
                    a.push("*".into());
                }
                let mut s = format!("{}({}{})", name.to_ascii_uppercase(), if *distinct { "DISTINCT " } else { "" }, list(&a));
                if let Some(w) = over {
                    s.push_str(&format!(" OVER ({})", w.sql()));
                }
                s
            }
            Expr::Op { op, left, right } => {
                let wrap = |e: &Expr| if matches!(e, Expr::Op { .. }) { format!("({})", e.sql()) } else { e.sql() };
                format!("{} {op} {}", wrap(left), wrap(right))
            }
            Expr::Case { whens, otherwise } => {
                let mut s = String::from("CASE");
                for (c, e) in whens {
                    s.push_str(&format!(" WHEN {} THEN {}", c.sql(0).unwrap_or_else(|| "TRUE".into()), e.sql()));
                }
                if let Some(e) = otherwise {
                    s.push_str(&format!(" ELSE {}", e.sql()));
                }
                s.push_str(" END");
                s
            }
            Expr::Cast { expr, ty } => format!("CAST({} AS {ty})", expr.sql()),
            Expr::Sub(qr) => format!("({})", qr.sql()),
            Expr::Raw(r) => r.trim().to_string(),
        }
    }

    /// Anzeigename (Spaltenueberschrift ohne Alias)
    pub fn label(&self) -> String {
        match self {
            Expr::Column { col, .. } => col.clone(),
            e => e.sql(),
        }
    }
}

impl Window {
    pub fn sql(&self) -> String {
        let mut parts = Vec::new();
        if !self.partition.is_empty() {
            parts.push(format!("PARTITION BY {}", list(&self.partition.iter().map(|e| e.sql()).collect::<Vec<_>>())));
        }
        if !self.order.is_empty() {
            parts.push(format!("ORDER BY {}", order_sql(&self.order)));
        }
        if !self.frame.trim().is_empty() {
            parts.push(self.frame.trim().to_string());
        }
        parts.join(" ")
    }
}

fn order_sql(o: &[OrderItem]) -> String {
    list(&o.iter().map(|i| format!("{}{}", i.expr.sql(), if i.desc { " DESC" } else { "" })).collect::<Vec<_>>())
}

impl Cond {
    /// SQL der Bedingung; None bei leerer Gruppe. `depth` > 0: Gruppe in Klammern.
    pub fn sql(&self, depth: usize) -> Option<String> {
        match self {
            Cond::Group { any, not, items } => {
                let parts: Vec<String> = items.iter().filter_map(|c| c.sql(depth + 1)).collect();
                if parts.is_empty() {
                    return None;
                }
                let joined = parts.join(if *any { " OR " } else { " AND " });
                Some(if *not {
                    format!("NOT ({joined})")
                } else if depth > 0 && parts.len() > 1 {
                    format!("({joined})")
                } else {
                    joined
                })
            }
            Cond::Cmp { left, op, right, quant, sub } => {
                let l = left.sql();
                let r: Vec<String> = right.iter().map(|e| e.sql()).collect();
                Some(match (op, sub) {
                    (CmpOp::IsNull | CmpOp::IsNotNull, _) => format!("{l} {}", op.sql()),
                    (CmpOp::In | CmpOp::NotIn, Some(sq)) => format!("{l} {} ({})", op.sql(), sq.sql()),
                    (_, Some(sq)) if op.allows_sub() => {
                        let qn = match quant {
                            Quant::None => "",
                            Quant::Any => "ANY ",
                            Quant::All => "ALL ",
                        };
                        format!("{l} {} {qn}({})", op.sql(), sq.sql())
                    }
                    (CmpOp::In | CmpOp::NotIn, _) => format!("{l} {} ({})", op.sql(), if r.is_empty() { "NULL".into() } else { list(&r) }),
                    (CmpOp::Between | CmpOp::NotBetween, _) => format!(
                        "{l} {} {} AND {}",
                        op.sql(),
                        r.first().cloned().unwrap_or("NULL".into()),
                        r.get(1).cloned().unwrap_or("NULL".into())
                    ),
                    _ => format!("{l} {} {}", op.sql(), r.first().cloned().unwrap_or("NULL".into())),
                })
            }
            Cond::Exists { not, query } => Some(format!("{}EXISTS ({})", if *not { "NOT " } else { "" }, query.sql())),
            Cond::Raw(r) => Some(r.trim().to_string()).filter(|s| !s.is_empty()),
        }
    }
}

impl Source {
    fn sql(&self) -> String {
        let base = match &self.from {
            From::Table(t) | From::Cte(t) => q(t),
            From::Sub(qr) => format!("({})", qr.sql()),
        };
        let alias = self.alias.trim();
        match (&self.from, alias.is_empty()) {
            (From::Sub(_), true) => format!("{base} AS {}", q("unterabfrage")),
            (_, true) => base,
            _ => format!("{base} AS {}", q(alias)),
        }
    }
}

impl Select {
    pub fn sql(&self) -> String {
        let mut s = String::from("SELECT ");
        if self.distinct {
            s.push_str("DISTINCT ");
        }
        if self.items.is_empty() {
            s.push('*');
        } else {
            let items: Vec<String> = self
                .items
                .iter()
                .map(|i| if i.alias.trim().is_empty() { i.expr.sql() } else { format!("{} AS {}", i.expr.sql(), q(i.alias.trim())) })
                .collect();
            s.push_str(&list(&items));
        }
        for (k, src) in self.sources.iter().enumerate() {
            if k == 0 {
                s.push_str(&format!(" FROM {}", src.sql()));
                continue;
            }
            s.push_str(&format!(" {} {}", src.join.sql(), src.sql()));
            if src.join.has_condition() {
                if !src.using.trim().is_empty() {
                    s.push_str(&format!(" USING ({})", idents(&src.using)));
                } else if let Some(c) = src.on.sql(0) {
                    s.push_str(&format!(" ON {c}"));
                }
            }
        }
        if let Some(w) = self.filter.sql(0) {
            s.push_str(&format!(" WHERE {w}"));
        }
        if !self.group.is_empty() {
            s.push_str(&format!(" GROUP BY {}", list(&self.group.iter().map(|e| e.sql()).collect::<Vec<_>>())));
            if self.rollup {
                s.push_str(" WITH ROLLUP");
            }
        }
        if let Some(h) = self.having.sql(0) {
            s.push_str(&format!(" HAVING {h}"));
        }
        if !self.order.is_empty() {
            s.push_str(&format!(" ORDER BY {}", order_sql(&self.order)));
        }
        limit_sql(&mut s, &self.limit, &self.offset);
        s
    }

    fn has_tail(&self) -> bool {
        !self.order.is_empty() || !self.limit.trim().is_empty()
    }

    /// Spalten, die in GROUP BY stehen muessen (alle Spalten ohne Zusammenfassung)
    pub fn needed_group(&self) -> Vec<Expr> {
        if !self.items.iter().any(|i| i.expr.is_aggregate()) {
            return Vec::new();
        }
        self.items
            .iter()
            .filter(|i| !i.expr.is_aggregate() && !i.expr.is_window() && !matches!(i.expr, Expr::Value(_) | Expr::Sub(_)))
            .map(|i| i.expr.clone())
            .collect()
    }
}

fn limit_sql(s: &mut String, limit: &str, offset: &str) {
    let l = limit.trim();
    if l.parse::<u64>().is_ok() {
        s.push_str(&format!(" LIMIT {l}"));
        let o = offset.trim();
        if o.parse::<u64>().is_ok_and(|o| o > 0) {
            s.push_str(&format!(" OFFSET {o}"));
        }
    }
}

impl Query {
    /// Einzeiliges SQL (fuer die Anzeige wird es formatiert)
    pub fn sql(&self) -> String {
        let mut s = String::new();
        let ctes: Vec<&Cte> = self.with.iter().filter(|c| !c.name.trim().is_empty()).collect();
        if !ctes.is_empty() {
            s.push_str("WITH ");
            if self.recursive {
                s.push_str("RECURSIVE ");
            }
            let defs: Vec<String> = ctes
                .iter()
                .map(|c| {
                    let cols = if c.columns.trim().is_empty() { String::new() } else { format!(" ({})", idents(&c.columns)) };
                    format!("{}{cols} AS ({})", q(c.name.trim()), c.query.sql())
                })
                .collect();
            s.push_str(&list(&defs));
            s.push(' ');
        }
        let many = self.parts.len() > 1;
        for (i, p) in self.parts.iter().enumerate() {
            if i > 0 {
                s.push_str(&format!(" {} ", p.op.sql()));
            }
            let sel = p.select.sql();
            if many && p.select.has_tail() {
                s.push_str(&format!("({sel})"));
            } else {
                s.push_str(&sel);
            }
        }
        if many {
            if !self.order.is_empty() {
                s.push_str(&format!(" ORDER BY {}", order_sql(&self.order)));
            }
            limit_sql(&mut s, &self.limit, &self.offset);
        }
        s
    }

    /// Mehrzeilig formatiert
    pub fn pretty(&self) -> String {
        crate::sqlfmt::format_sql(&self.sql()).trim_end().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(s: &str, c: &str) -> Expr {
        Expr::column(s, c)
    }

    fn cmp(l: Expr, op: CmpOp, r: Vec<Expr>) -> Cond {
        Cond::Cmp { left: l, op, right: r, quant: Quant::None, sub: None }
    }

    fn sel(table: &str) -> Select {
        Select { sources: vec![Source::table(table)], ..Default::default() }
    }

    fn one(s: Select) -> Query {
        Query { parts: vec![Part { op: SetOp::Union, select: s }], ..Default::default() }
    }

    #[test]
    fn simple_select_star() {
        assert_eq!(one(sel("kunde")).sql(), "SELECT * FROM `kunde`");
        assert_eq!(Query::default().sql(), "SELECT *");
    }

    #[test]
    fn joins_where_group_having() {
        let mut s = sel("kunde");
        s.sources[0].alias = "k".into();
        let mut b = Source::table("bestellung");
        b.alias = "b".into();
        b.join = JoinKind::Left;
        b.on = Cond::Group { any: false, not: false, items: vec![cmp(col("b", "kunde_id"), CmpOp::Eq, vec![col("k", "id")])] };
        s.sources.push(b);
        s.items = vec![
            Item { expr: col("k", "nachname"), alias: String::new() },
            Item { expr: Expr::func("COUNT", None), alias: "anzahl".into() },
        ];
        s.filter = Cond::Group {
            any: false,
            not: false,
            items: vec![
                cmp(col("k", "ort"), CmpOp::In, vec![Expr::Value("Berlin".into()), Expr::Value("Hamburg".into())]),
                Cond::Group { any: true, not: false, items: vec![cmp(col("b", "status"), CmpOp::IsNull, vec![]), cmp(col("b", "datum"), CmpOp::Between, vec![Expr::Value("2024-01-01".into()), Expr::Value("2024-12-31".into())])] },
            ],
        };
        s.group = s.needed_group();
        s.having = Cond::Group { any: false, not: false, items: vec![cmp(Expr::func("COUNT", None), CmpOp::Gt, vec![Expr::Value("1".into())])] };
        s.order = vec![OrderItem { expr: Expr::column("", "anzahl"), desc: true }];
        s.limit = "10".into();
        s.offset = "5".into();
        assert_eq!(
            one(s).sql(),
            "SELECT `k`.`nachname`, COUNT(*) AS `anzahl` FROM `kunde` AS `k` LEFT JOIN `bestellung` AS `b` ON `b`.`kunde_id` = `k`.`id` \
             WHERE `k`.`ort` IN ('Berlin', 'Hamburg') AND (`b`.`status` IS NULL OR `b`.`datum` BETWEEN '2024-01-01' AND '2024-12-31') \
             GROUP BY `k`.`nachname` HAVING COUNT(*) > 1 ORDER BY `anzahl` DESC LIMIT 10 OFFSET 5"
        );
    }

    #[test]
    fn set_operations_and_cte() {
        let mut a = sel("kunde");
        a.items = vec![Item { expr: col("kunde", "ort"), alias: String::new() }];
        let mut b = sel("hersteller");
        b.items = vec![Item { expr: col("hersteller", "name"), alias: String::new() }];
        b.order = vec![OrderItem { expr: col("hersteller", "name"), desc: false }];
        b.limit = "3".into();
        let mut qr = Query {
            parts: vec![Part { op: SetOp::Union, select: a.clone() }, Part { op: SetOp::Except, select: b }],
            limit: "20".into(),
            ..Default::default()
        };
        assert_eq!(
            qr.sql(),
            "SELECT `kunde`.`ort` FROM `kunde` EXCEPT (SELECT `hersteller`.`name` FROM `hersteller` ORDER BY `hersteller`.`name` LIMIT 3) LIMIT 20"
        );
        qr.parts[1].op = SetOp::IntersectAll;
        assert!(qr.sql().contains(" INTERSECT ALL ("));
        qr.parts.push(Part { op: SetOp::UnionAll, select: a });
        assert!(qr.sql().contains(" UNION ALL SELECT"));
        // WITH RECURSIVE
        let mut base = Select::default();
        base.items = vec![Item { expr: Expr::Value("1".into()), alias: "n".into() }];
        let mut step = sel("zahlen");
        step.items = vec![Item { expr: Expr::Op { op: "+".into(), left: Box::new(col("zahlen", "n")), right: Box::new(Expr::Value("1".into())) }, alias: String::new() }];
        step.filter = Cond::Group { any: false, not: false, items: vec![cmp(col("zahlen", "n"), CmpOp::Lt, vec![Expr::Value("10".into())])] };
        let cte = Query { parts: vec![Part { op: SetOp::Union, select: base }, Part { op: SetOp::UnionAll, select: step }], ..Default::default() };
        let mut main = one(Select { sources: vec![Source { from: From::Cte("zahlen".into()), ..Source::table("x") }], ..Default::default() });
        main.recursive = true;
        main.with = vec![Cte { name: "zahlen".into(), columns: String::new(), query: cte }];
        assert_eq!(
            main.sql(),
            "WITH RECURSIVE `zahlen` AS (SELECT 1 AS `n` UNION ALL SELECT `zahlen`.`n` + 1 FROM `zahlen` WHERE `zahlen`.`n` < 10) SELECT * FROM `zahlen`"
        );
    }

    #[test]
    fn subqueries_everywhere() {
        let inner = one(Select {
            items: vec![Item { expr: Expr::func("AVG", Some(col("artikel", "preis"))), alias: String::new() }],
            ..sel("artikel")
        });
        let mut s = sel("artikel");
        s.items = vec![
            Item { expr: col("artikel", "name"), alias: String::new() },
            Item { expr: Expr::Sub(Box::new(inner.clone())), alias: "schnitt".into() },
            Item { expr: Expr::Cast { expr: Box::new(col("artikel", "preis")), ty: "SIGNED".into() }, alias: String::new() },
            Item {
                expr: Expr::Case {
                    whens: vec![(cmp(col("artikel", "preis"), CmpOp::Gt, vec![Expr::Value("100".into())]), Expr::Value("teuer".into()))],
                    otherwise: Some(Box::new(Expr::Value("günstig".into()))),
                },
                alias: "klasse".into(),
            },
        ];
        s.filter = Cond::Group {
            any: false,
            not: false,
            items: vec![
                Cond::Cmp { left: col("artikel", "preis"), op: CmpOp::Gt, right: vec![], quant: Quant::All, sub: Some(Box::new(inner.clone())) },
                Cond::Cmp { left: col("artikel", "id"), op: CmpOp::NotIn, right: vec![], quant: Quant::None, sub: Some(Box::new(inner.clone())) },
                Cond::Exists { not: true, query: Box::new(inner.clone()) },
                Cond::Raw("1 = 1".into()),
                Cond::Group { any: true, not: true, items: vec![] },
            ],
        };
        let mut src = Source { from: From::Sub(Box::new(inner)), alias: "u".into(), ..Source::table("x") };
        src.join = JoinKind::Cross;
        s.sources.push(src);
        let sql = one(s).sql();
        assert!(sql.contains("(SELECT AVG(`artikel`.`preis`) FROM `artikel`) AS `schnitt`"), "{sql}");
        assert!(sql.contains("CAST(`artikel`.`preis` AS SIGNED)"));
        assert!(sql.contains("CASE WHEN `artikel`.`preis` > 100 THEN 'teuer' ELSE 'günstig' END AS `klasse`"));
        assert!(sql.contains("`artikel`.`preis` > ALL (SELECT"));
        assert!(sql.contains("`artikel`.`id` NOT IN (SELECT"));
        assert!(sql.contains("NOT EXISTS (SELECT"));
        assert!(sql.contains("CROSS JOIN (SELECT AVG(`artikel`.`preis`) FROM `artikel`) AS `u`"));
        assert!(sql.ends_with("AND 1 = 1"), "leere Gruppe faellt weg: {sql}");
    }

    #[test]
    fn window_functions_and_values() {
        let e = Expr::Func {
            name: "rank".into(),
            args: vec![],
            distinct: false,
            over: Some(Window {
                partition: vec![col("k", "ort")],
                order: vec![OrderItem { expr: col("k", "umsatz"), desc: true }],
                frame: String::new(),
            }),
        };
        assert_eq!(e.sql(), "RANK() OVER (PARTITION BY `k`.`ort` ORDER BY `k`.`umsatz` DESC)");
        assert_eq!(Expr::func("ROW_NUMBER", None).sql(), "ROW_NUMBER() OVER ()");
        assert_eq!(Expr::Func { name: "COUNT".into(), args: vec![col("", "ort")], distinct: true, over: None }.sql(), "COUNT(DISTINCT `ort`)");
        assert_eq!(value_sql("12.5"), "12.5");
        assert_eq!(value_sql("007"), "'007'");
        assert_eq!(value_sql("null"), "null");
        assert_eq!(value_sql("'x'"), "'x'");
        assert_eq!(value_sql("O'Neil"), "'O''Neil'");
        assert_eq!(value_sql("@v"), "@v");
        assert_eq!(Expr::func("DATE_ADD", Some(col("b", "datum"))).sql(), "DATE_ADD(`b`.`datum`, INTERVAL 1 DAY)");
        let op = Expr::Op {
            op: "*".into(),
            left: Box::new(Expr::Op { op: "+".into(), left: Box::new(Expr::Value("1".into())), right: Box::new(Expr::Value("2".into())) }),
            right: Box::new(Expr::Value("3".into())),
        };
        assert_eq!(op.sql(), "(1 + 2) * 3");
    }

    #[test]
    fn using_and_natural() {
        let mut s = sel("a");
        let mut b = Source::table("b");
        b.using = "id, typ".into();
        s.sources.push(b);
        let mut c = Source::table("c");
        c.join = JoinKind::NaturalLeft;
        c.on = Cond::Group { any: false, not: false, items: vec![Cond::Raw("x".into())] };
        s.sources.push(c);
        assert_eq!(one(s).sql(), "SELECT * FROM `a` INNER JOIN `b` USING (`id`, `typ`) NATURAL LEFT JOIN `c`");
    }

    #[test]
    fn json_roundtrip() {
        let mut s = sel("kunde");
        s.items = vec![Item { expr: Expr::func("COUNT", None), alias: "n".into() }];
        let qr = one(s);
        let j = serde_json::to_string(&qr).unwrap();
        let back: Query = serde_json::from_str(&j).unwrap();
        assert_eq!(back, qr);
    }

    /// Beispiel fuer Bildschirmfotos: EASYMYSQL_EXAMPLE=datei cargo test example -- --ignored
    #[test]
    #[ignore]
    fn example() {
        let mut k = sel("kunde");
        k.sources[0].alias = "k".into();
        let mut b = Source::table("bestellung");
        b.alias = "b".into();
        b.join = JoinKind::Left;
        b.on = Cond::Group { any: false, not: false, items: vec![cmp(col("b", "kunde_id"), CmpOp::Eq, vec![col("k", "id")])] };
        k.sources.push(b);
        k.items = vec![
            Item { expr: col("k", "nachname"), alias: String::new() },
            Item { expr: Expr::func("COUNT", Some(col("b", "id"))), alias: "bestellungen".into() },
        ];
        let inner = one(Select { items: vec![Item { expr: col("kunde", "ort"), alias: String::new() }], ..sel("kunde") });
        k.filter = Cond::Group {
            any: false,
            not: false,
            items: vec![
                Cond::Cmp { left: col("k", "ort"), op: CmpOp::In, right: vec![], quant: Quant::None, sub: Some(Box::new(inner)) },
                Cond::Group { any: true, not: false, items: vec![cmp(col("b", "status"), CmpOp::IsNull, vec![]), cmp(col("b", "datum"), CmpOp::Between, vec![Expr::Value("2024-01-01".into()), Expr::Value("2024-12-31".into())])] },
            ],
        };
        k.group = k.needed_group();
        let mut h = sel("hersteller");
        h.items = vec![Item { expr: col("hersteller", "name"), alias: String::new() }, Item { expr: Expr::Value("0".into()), alias: String::new() }];
        let qr = Query { parts: vec![Part { op: SetOp::Union, select: k }, Part { op: SetOp::Except, select: h }], ..Default::default() };
        let j = serde_json::json!({ "db": "shop", "query": qr });
        std::fs::write(std::env::var("EASYMYSQL_EXAMPLE").unwrap(), j.to_string()).unwrap();
    }

    /// Gegen einen laufenden Server mit der Datenbank "shop" pruefen, ob das SQL gueltig ist.
    #[test]
    #[ignore]
    fn valid_on_server() {
        use mysql::prelude::*;
        let db = crate::db::Db::connect(&crate::db::ConnInfo::default()).unwrap();
        let mut c = db.new_conn().unwrap();
        c.query_drop("USE shop").unwrap();
        let mut k = sel("kunde");
        k.sources[0].alias = "k".into();
        k.items = vec![
            Item { expr: col("k", "ort"), alias: String::new() },
            Item { expr: Expr::func("COUNT", None), alias: "n".into() },
            Item {
                expr: Expr::Func { name: "RANK".into(), args: vec![], distinct: false, over: Some(Window { order: vec![OrderItem { expr: Expr::func("COUNT", None), desc: true }], ..Default::default() }) },
                alias: "platz".into(),
            },
        ];
        k.group = k.needed_group();
        k.rollup = false;
        let mut h = sel("hersteller");
        h.items = vec![Item { expr: col("hersteller", "name"), alias: String::new() }, Item { expr: Expr::Value("0".into()), alias: String::new() }, Item { expr: Expr::Value("0".into()), alias: String::new() }];
        for op in SetOp::ALL {
            let qr = Query { parts: vec![Part { op: SetOp::Union, select: k.clone() }, Part { op, select: h.clone() }], ..Default::default() };
            c.query_drop(qr.sql()).unwrap_or_else(|e| panic!("{}: {e}", qr.sql()));
        }
    }
}
