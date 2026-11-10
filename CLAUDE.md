# EasyMySQL – Hinweise für Claude

Alle Regeln stehen in [AGENTS.md](AGENTS.md); die wichtigste: **fertige Arbeit lokal prüfen und direkt auf
`main` committen und pushen; standardmäßig keinen Pull Request anlegen**. Jeder Push auf `main` rollt die
Website aus. Ein App-Release entsteht nur für eine noch nicht veröffentlichte Version in `Cargo.toml`.

**Versionen nur in kleinen Schritten:** aktuell `1.1.x`; jede normale Änderung erhöht nur den Patch
(`1.1.17` → `1.1.18`). Nebenversion nur auf ausdrücklichen Wunsch, Hauptversion nie. Jedes Release erhöht außerdem die interne
laufende Nummer `build-number` in `Cargo.toml` um eins (Details in AGENTS.md).
