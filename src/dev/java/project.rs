// Neues Java-Projekt mit fertiger Datenbankverbindung: ohne Build-Werkzeug, mit Maven oder mit Gradle.
// Fuer jede gewaehlte Tabelle entstehen ein Record und eine DAO-Klasse.

use super::Driver;
use crate::db::TableInfo;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Plain,
    Maven,
    GradleKotlin,
    GradleGroovy,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Plain => crate::i18n::tr("Einfach", "Simple"),
            Kind::Maven => "Maven",
            Kind::GradleKotlin => "Gradle (Kotlin)",
            Kind::GradleGroovy => "Gradle (Groovy)",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Kind::Plain => "einfach",
            Kind::Maven => "maven",
            Kind::GradleKotlin => "gradle-kts",
            Kind::GradleGroovy => "gradle",
        }
    }

    pub fn from_id(s: &str) -> Kind {
        match s {
            "maven" => Kind::Maven,
            "gradle-kts" => Kind::GradleKotlin,
            "gradle" => Kind::GradleGroovy,
            _ => Kind::Plain,
        }
    }

    fn source_dir(self) -> &'static str {
        if self == Kind::Plain { "src" } else { "src/main/java" }
    }

    fn resource_dir(self) -> &'static str {
        if self == Kind::Plain { "src" } else { "src/main/resources" }
    }
}

#[derive(Clone, Debug)]
pub struct Spec {
    /// Name des Projektordners
    pub name: String,
    pub kind: Kind,
    pub package: String,
    pub database: String,
    pub tables: Vec<TableInfo>,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Wird nur in db.properties geschrieben, wenn EasyMySQL Passwoerter speichern darf
    pub password: Option<String>,
    pub driver: Driver,
    pub driver_version: String,
    /// JUnit-Version fuer Tests (None = ohne Tests)
    pub junit_version: Option<String>,
}

/// Gueltiger Paketname? ("app", "de.schule.app")
pub fn valid_package(p: &str) -> bool {
    !p.is_empty() && p.split('.').all(super::source::is_identifier)
}

/// Name fuer Maven/Gradle: "Mein Projekt" -> "mein-projekt"
fn artifact_id(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let t = out.trim_end_matches('-').to_string();
    if t.is_empty() { "app".into() } else { t }
}

fn en() -> bool {
    crate::i18n::is_english()
}

/// Wert in einer .properties-Datei
fn prop(v: &str) -> String {
    v.replace('\\', "\\\\").replace('\n', "\\n")
}

fn db_properties(s: &Spec) -> String {
    let url = super::jdbc_url(s.driver, &s.host, s.port, &s.database);
    let head = if en() {
        "# Connection to the database.\n# When the program is started from EasyMySQL, EasyMySQL passes server, database and password itself.\n"
    } else {
        "# Verbindung zur Datenbank.\n# Startet man das Programm in EasyMySQL, gibt EasyMySQL Server, Datenbank und Passwort selbst mit.\n"
    };
    let pw_note = match &s.password {
        Some(_) => "",
        None if en() => "# Enter the password here to run the program outside EasyMySQL.\n",
        None => "# Für den Start außerhalb von EasyMySQL hier das Passwort eintragen.\n",
    };
    format!("{head}url={}\nuser={}\n{pw_note}password={}\n", prop(&url), prop(&s.user), prop(s.password.as_deref().unwrap_or("")))
}

fn database_java(s: &Spec) -> String {
    let doc = if en() {
        "/**\n * Connection to the database.\n *\n * The connection settings are in db.properties. When the program is started from EasyMySQL,\n * EasyMySQL passes them itself (server, database and password of the tab).\n */"
    } else {
        "/**\n * Verbindung zur Datenbank.\n *\n * Die Verbindungsdaten stehen in db.properties. Startet man das Programm in EasyMySQL,\n * gibt EasyMySQL sie selbst mit (Server, Datenbank und Passwort aus dem Reiter).\n */"
    };
    let (from_easy, missing) = if en() { ("Started from EasyMySQL", "db.properties is missing (url, user, password)") } else { ("Start aus EasyMySQL", "db.properties fehlt (url, user, password)") };
    format!(
        r#"package {pkg};

import java.io.IOException;
import java.io.InputStream;
import java.io.Reader;
import java.nio.file.Files;
import java.nio.file.Path;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.SQLException;
import java.util.Properties;

{doc}
public final class Database {{
    private Database() {{
    }}

    public static Connection connect() throws SQLException {{
        Properties p = settings();
        return DriverManager.getConnection(p.getProperty("url"), p.getProperty("user"), p.getProperty("password", ""));
    }}

    private static Properties settings() {{
        Properties p = new Properties();
        try (InputStream in = Database.class.getResourceAsStream("/db.properties")) {{
            if (in != null) {{
                p.load(in);
            }}
        }} catch (IOException e) {{
            throw new IllegalStateException("db.properties: " + e.getMessage(), e);
        }}
        Path file = Path.of("db.properties");
        if (Files.isRegularFile(file)) {{
            try (Reader r = Files.newBufferedReader(file)) {{
                p.load(r);
            }} catch (IOException e) {{
                throw new IllegalStateException("db.properties: " + e.getMessage(), e);
            }}
        }}
        // {from_easy}
        set(p, "url", System.getenv("EASYMYSQL_DB_URL"));
        set(p, "user", System.getenv("EASYMYSQL_DB_USER"));
        set(p, "password", System.getenv("EASYMYSQL_DB_PASSWORD"));
        if (p.getProperty("url") == null) {{
            throw new IllegalStateException("{missing}");
        }}
        return p;
    }}

    private static void set(Properties p, String key, String value) {{
        if (value != null) {{
            p.setProperty(key, value);
        }}
    }}
}}
"#,
        pkg = s.package
    )
}

fn main_java(s: &Spec) -> String {
    let connected = if en() { "Connected to " } else { "Verbunden mit " };
    let body = match s.tables.first() {
        Some(t) => {
            let rec = crate::javagen::class_name(&t.name);
            let comment = if en() { format!("All rows of the table {}", t.name) } else { format!("Alle Zeilen der Tabelle {}", t.name) };
            format!("\n            // {comment}\n            for ({rec} row : new {rec}Dao(con).findAll()) {{\n                System.out.println(row);\n            }}\n")
        }
        None => {
            let comment = if en() { "Tables of the database" } else { "Tabellen der Datenbank" };
            format!(
                "\n            // {comment}\n            DatabaseMetaData meta = con.getMetaData();\n            try (ResultSet rs = meta.getTables(con.getCatalog(), null, \"%\", new String[] {{\"TABLE\"}})) {{\n                while (rs.next()) {{\n                    System.out.println(rs.getString(\"TABLE_NAME\"));\n                }}\n            }}\n"
            )
        }
    };
    format!(
        "package {pkg};\n\nimport java.sql.*;\n\npublic class Main {{\n    public static void main(String[] args) throws SQLException {{\n        try (Connection con = Database.connect()) {{\n            System.out.println(\"{connected}\" + con.getMetaData().getURL());\n{body}        }}\n    }}\n}}\n",
        pkg = s.package
    )
}

fn pom(s: &Spec) -> String {
    let id = artifact_id(&s.name);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd">
    <modelVersion>4.0.0</modelVersion>

    <groupId>{pkg}</groupId>
    <artifactId>{id}</artifactId>
    <version>1.0</version>

    <properties>
        <maven.compiler.release>17</maven.compiler.release>
        <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
        <!-- mvn compile exec:java -->
        <exec.mainClass>{pkg}.Main</exec.mainClass>
    </properties>

    <dependencies>
        <dependency>
            <groupId>{group}</groupId>
            <artifactId>{artifact}</artifactId>
            <version>{version}</version>
        </dependency>{junit}
    </dependencies>
</project>
"#,
        pkg = s.package,
        group = s.driver.group(),
        artifact = s.driver.artifact(),
        version = s.driver_version,
        junit = match &s.junit_version {
            Some(v) => format!("\n        <dependency>\n            <groupId>org.junit.jupiter</groupId>\n            <artifactId>junit-jupiter</artifactId>\n            <version>{v}</version>\n            <scope>test</scope>\n        </dependency>"),
            None => String::new(),
        }
    )
}

fn gradle_kts(s: &Spec) -> String {
    let input = if en() { "Console input (Scanner) also works with \"gradle run\"" } else { "Eingaben über die Konsole (Scanner) auch bei \"gradle run\"" };
    format!(
        r#"plugins {{
    application
}}

repositories {{
    mavenCentral()
}}

dependencies {{
    implementation("{group}:{artifact}:{version}"){junit}
}}

application {{
    mainClass = "{pkg}.Main"
}}

tasks.withType<JavaCompile> {{
    options.release = 17
    options.encoding = "UTF-8"
}}

// {input}
tasks.named<JavaExec>("run") {{
    standardInput = System.`in`
}}
{test}"#,
        group = s.driver.group(),
        artifact = s.driver.artifact(),
        version = s.driver_version,
        pkg = s.package,
        junit = match &s.junit_version {
            Some(v) => format!("\n    testImplementation(platform(\"org.junit:junit-bom:{v}\"))\n    testImplementation(\"org.junit.jupiter:junit-jupiter\")\n    testRuntimeOnly(\"org.junit.platform:junit-platform-launcher\")"),
            None => String::new(),
        },
        test = if s.junit_version.is_some() { "\ntasks.test {\n    useJUnitPlatform()\n}\n" } else { "" }
    )
}

fn gradle_groovy(s: &Spec) -> String {
    let input = if en() { "Console input (Scanner) also works with \"gradle run\"" } else { "Eingaben über die Konsole (Scanner) auch bei \"gradle run\"" };
    format!(
        r#"plugins {{
    id 'application'
}}

repositories {{
    mavenCentral()
}}

dependencies {{
    implementation '{group}:{artifact}:{version}'{junit}
}}

application {{
    mainClass = '{pkg}.Main'
}}

tasks.withType(JavaCompile).configureEach {{
    options.release = 17
    options.encoding = 'UTF-8'
}}

// {input}
tasks.named('run', JavaExec) {{
    standardInput = System.in
}}
{test}"#,
        group = s.driver.group(),
        artifact = s.driver.artifact(),
        version = s.driver_version,
        pkg = s.package,
        junit = match &s.junit_version {
            Some(v) => format!("\n    testImplementation platform('org.junit:junit-bom:{v}')\n    testImplementation 'org.junit.jupiter:junit-jupiter'\n    testRuntimeOnly 'org.junit.platform:junit-platform-launcher'"),
            None => String::new(),
        },
        test = if s.junit_version.is_some() { "\ntest {\n    useJUnitPlatform()\n}\n" } else { "" }
    )
}

fn gitignore(kind: Kind) -> String {
    let mut s = String::from("*.class\nout/\nbin/\n");
    match kind {
        Kind::Maven => s.push_str("target/\n"),
        Kind::GradleKotlin | Kind::GradleGroovy => s.push_str(".gradle/\nbuild/\n"),
        Kind::Plain => {}
    }
    s.push_str(".idea/\n*.iml\n");
    s
}

fn readme(s: &Spec) -> String {
    let main = format!("{}/{}/Main.java", s.kind.source_dir(), s.package.replace('.', "/"));
    let props = format!("{}/db.properties", s.kind.resource_dir());
    let (run_de, run_en) = match s.kind {
        Kind::Plain => ("", ""),
        Kind::Maven => ("\nIm Terminal: `mvn compile exec:java`\n", "\nIn a terminal: `mvn compile exec:java`\n"),
        Kind::GradleKotlin | Kind::GradleGroovy => ("\nIm Terminal: `gradle run`\n", "\nIn a terminal: `gradle run`\n"),
    };
    if en() {
        format!(
            "# {name}\n\nJava program for the database `{db}`.\n\n- `{main}` – start here (▶ in EasyMySQL or F5)\n- `{props}` – connection settings for starting outside EasyMySQL\n- One record and one DAO class per table: `findAll`, `findById`, `insert`, `update`, `delete`\n{run_en}",
            name = s.name,
            db = s.database
        )
    } else {
        format!(
            "# {name}\n\nJava-Programm für die Datenbank `{db}`.\n\n- `{main}` – hier geht es los (▶ in EasyMySQL oder F5)\n- `{props}` – Verbindungsdaten für den Start außerhalb von EasyMySQL\n- Je Tabelle ein Record und eine DAO-Klasse: `findAll`, `findById`, `insert`, `update`, `delete`\n{run_de}",
            name = s.name,
            db = s.database
        )
    }
}

/// JUnit-Test: Verbindung klappt, und je Tabelle laesst sich findAll aufrufen
fn test_java(s: &Spec) -> String {
    let (conn_name, table_name) = if en() { ("connects", "reads all rows of ") } else { ("verbindet", "liest alle Zeilen von ") };
    let mut tests = format!(
        "    @Test\n    void {}() throws SQLException {{\n        try (Connection con = Database.connect()) {{\n            assertTrue(con.isValid(2));\n        }}\n    }}\n",
        if en() { "connectionWorks" } else { "verbindungKlappt" }
    );
    for t in s.tables.iter().take(3) {
        let class = crate::javagen::class_name(&t.name);
        tests.push_str(&format!(
            "\n    @Test\n    @DisplayName(\"{table_name}{name}\")\n    void findAll{class}() throws SQLException {{\n        try (Connection con = Database.connect()) {{\n            assertNotNull(new {class}Dao(con).findAll());\n        }}\n    }}\n",
            name = t.name
        ));
    }
    format!(
        "package {pkg};\n\nimport static org.junit.jupiter.api.Assertions.*;\n\nimport java.sql.*;\nimport org.junit.jupiter.api.*;\n\n@DisplayName(\"Database {conn_name}\")\nclass DatabaseTest {{\n{tests}}}\n",
        pkg = s.package
    )
}

/// Alle Dateien des Projekts: (relativer Pfad, Inhalt)
pub fn files(s: &Spec) -> Vec<(PathBuf, String)> {
    let src = Path::new(s.kind.source_dir()).join(s.package.replace('.', "/"));
    let mut out = vec![
        (src.join("Main.java"), main_java(s)),
        (src.join("Database.java"), database_java(s)),
        (Path::new(s.kind.resource_dir()).join("db.properties"), db_properties(s)),
        (PathBuf::from(".gitignore"), gitignore(s.kind)),
        (PathBuf::from("README.md"), readme(s)),
    ];
    for t in &s.tables {
        let class = crate::javagen::class_name(&t.name);
        out.push((src.join(format!("{class}.java")), crate::javagen::record_file(t, Some(&s.package))));
        out.push((src.join(format!("{class}Dao.java")), crate::javagen::dao_file(t, Some(&s.package))));
    }
    if s.junit_version.is_some() {
        let test_dir = if s.kind == Kind::Plain { Path::new("test").to_path_buf() } else { PathBuf::from("src/test/java") };
        out.push((test_dir.join(s.package.replace('.', "/")).join("DatabaseTest.java"), test_java(s)));
    }
    let id = artifact_id(&s.name);
    match s.kind {
        Kind::Plain => out.push((
            PathBuf::from(".vscode/settings.json"),
            "{\n  \"java.project.sourcePaths\": [\"src\", \"test\"],\n  \"java.project.referencedLibraries\": [\"lib/**/*.jar\"],\n  \"java.project.outputPath\": \"bin\"\n}\n".into(),
        )),
        Kind::Maven => out.push((PathBuf::from("pom.xml"), pom(s))),
        Kind::GradleKotlin => {
            out.push((PathBuf::from("build.gradle.kts"), gradle_kts(s)));
            out.push((PathBuf::from("settings.gradle.kts"), format!("rootProject.name = \"{id}\"\n")));
        }
        Kind::GradleGroovy => {
            out.push((PathBuf::from("build.gradle"), gradle_groovy(s)));
            out.push((PathBuf::from("settings.gradle"), format!("rootProject.name = '{id}'\n")));
        }
    }
    out
}

/// Projekt in `dir` anlegen (der Ordner darf noch nicht existieren oder muss leer sein). Liefert Main.java.
pub fn write(dir: &Path, s: &Spec) -> Result<PathBuf, String> {
    if dir.exists() && std::fs::read_dir(dir).map(|mut r| r.next().is_some()).unwrap_or(true) {
        return Err(crate::tr_format!("„{}“ gibt es schon und ist nicht leer.", "“{}” already exists and is not empty.", dir.display()));
    }
    let mut main = PathBuf::new();
    for (rel, content) in files(s) {
        let p = dir.join(&rel);
        crate::workspace::atomic_write(&p, &content).map_err(|e| format!("{}: {e}", p.display()))?;
        if rel.file_name().is_some_and(|n| n == "Main.java") {
            main = p;
        }
    }
    // Einfaches Projekt: Treiber nach lib/ kopieren, damit es auch in anderen Programmen (VS Code, IntelliJ) laeuft
    if s.kind == Kind::Plain
        && let Some((_, _, jar)) = super::super::tools().driver(s.driver).cloned()
        && let Some(name) = jar.file_name()
    {
        let lib = dir.join("lib");
        let _ = std::fs::create_dir_all(&lib);
        let _ = std::fs::copy(&jar, lib.join(name));
    }
    crate::workspace::set_file_db(dir, &main, &s.database);
    Ok(main)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ColumnInfo;

    fn spec(kind: Kind) -> Spec {
        let col = |name: &str, ty: &str, key: &str, extra: &str| ColumnInfo { name: name.into(), col_type: ty.into(), nullable: key.is_empty(), key: key.into(), extra: extra.into(), ..Default::default() };
        Spec {
            name: "Mein Schulprojekt".into(),
            kind,
            package: "de.schule".into(),
            database: "schule".into(),
            tables: vec![TableInfo {
                name: "schueler".into(),
                columns: vec![col("id", "int(11)", "PRI", "auto_increment"), col("vorname", "varchar(50)", "", ""), col("geboren", "date", "", "")],
                ..Default::default()
            }],
            host: "127.0.0.1".into(),
            port: 3306,
            user: "root".into(),
            password: None,
            driver: Driver::MariaDb,
            driver_version: "4.0.0".into(),
            junit_version: Some("6.0.1".into()),
        }
    }

    #[test]
    fn layouts_follow_the_build_tool() {
        let names = |k: Kind| files(&spec(k)).into_iter().map(|(p, _)| p.to_string_lossy().replace('\\', "/")).collect::<Vec<_>>();
        let plain = names(Kind::Plain);
        assert!(plain.contains(&"src/de/schule/Main.java".to_string()) && plain.contains(&"src/db.properties".to_string()) && plain.contains(&".vscode/settings.json".to_string()));
        let maven = names(Kind::Maven);
        assert!(maven.contains(&"pom.xml".to_string()) && maven.contains(&"src/main/java/de/schule/SchuelerDao.java".to_string()) && maven.contains(&"src/main/resources/db.properties".to_string()));
        assert!(names(Kind::GradleKotlin).contains(&"settings.gradle.kts".to_string()));
        assert!(names(Kind::GradleGroovy).contains(&"build.gradle".to_string()));
        let pom = files(&spec(Kind::Maven)).into_iter().find(|(p, _)| p.ends_with("pom.xml")).unwrap().1;
        assert!(pom.contains("<artifactId>mariadb-java-client</artifactId>") && pom.contains("<version>4.0.0</version>") && pom.contains("<artifactId>mein-schulprojekt</artifactId>"));
        let props = files(&spec(Kind::Plain)).into_iter().find(|(p, _)| p.ends_with("db.properties")).unwrap().1;
        assert!(props.contains("url=jdbc:mariadb://127.0.0.1:3306/schule?sslMode=disable\nuser=root\n"), "{props}");
        assert!(valid_package("de.schule") && !valid_package("de..x") && !valid_package("class") && !valid_package(""));
    }

    /// Das erzeugte einfache Projekt muss sich uebersetzen lassen (nur mit JDK).
    #[test]
    fn generated_plain_project_compiles() {
        let cfg = super::super::super::Config { jdk: "system".into(), ..Default::default() };
        let tools = super::super::super::detect(&std::env::temp_dir().join("easymysql-kein-werkzeug-ordner"), &cfg);
        let Some(jdk) = tools.jdk else { return };
        let dir = std::env::temp_dir().join(format!("easymysql-javaprojekt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = spec(Kind::Plain);
        s.junit_version = None;
        let main = write(&dir, &s).unwrap();
        assert!(main.ends_with("Main.java"));
        assert!(write(&dir, &s).is_err(), "nicht in einen vollen Ordner schreiben");
        let sources = super::super::run::java_files(&dir.join("src"));
        assert_eq!(sources.len(), 4);
        let out = std::process::Command::new(jdk.javac()).arg("-d").arg(dir.join("bin")).args(&sources).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
