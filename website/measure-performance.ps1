# Misst die vorhandenen UI-Benchmarks im optimierten Build, ohne Datenbank oder GUI-Fenster.
# Auf Windows im Repository ausfuehren: powershell -File website/measure-performance.ps1
param(
    [ValidateRange(3, 21)][int]$Wiederholungen = 7,
    [string]$Ausgabepfad = (Join-Path $PSScriptRoot 'public/render-benchmarks.json')
)

$ErrorActionPreference = 'Stop'
$Projekt = Split-Path $PSScriptRoot -Parent
Push-Location $Projekt
try {
    $Editor = @()
    $Raster = @()
    for ($Lauf = 0; $Lauf -lt $Wiederholungen; $Lauf++) {
        $Protokoll = [System.IO.Path]::GetTempFileName()
        try {
            $Prozess = Start-Process -FilePath 'cargo' -ArgumentList @('test', '--release', '--locked', 'performance_render', '--', '--ignored', '--nocapture', '--test-threads=1') -Wait -PassThru -WindowStyle Hidden -RedirectStandardOutput $Protokoll
            if ($Prozess.ExitCode -ne 0) { throw "Benchmark fehlgeschlagen: $($Prozess.ExitCode)" }
            $Ausgabe = Get-Content -LiteralPath $Protokoll -Raw -Encoding UTF8
            $TrefferEditor = [regex]::Match($Ausgabe, '5,000 SQL lines: ([\d.]+) ms/frame')
            $TrefferRaster = [regex]::Match($Ausgabe, '50,000 rows . 12 columns: ([\d.]+) ms/frame')
            if (!$TrefferEditor.Success -or !$TrefferRaster.Success) { throw 'Benchmark-Ausgabe unvollstaendig' }
            $Editor += [double]::Parse($TrefferEditor.Groups[1].Value, [cultureinfo]::InvariantCulture)
            $Raster += [double]::Parse($TrefferRaster.Groups[1].Value, [cultureinfo]::InvariantCulture)
            Write-Host ('Lauf {0}: Editor {1} ms, Raster {2} ms' -f ($Lauf + 1), $Editor[-1], $Raster[-1])
        } finally {
            Remove-Item -LiteralPath $Protokoll -ErrorAction SilentlyContinue
        }
    }
    function Median($Werte) {
        $Sortiert = @($Werte | Sort-Object)
        $Mitte = [int][math]::Floor($Sortiert.Count / 2)
        if ($Sortiert.Count % 2) { return $Sortiert[$Mitte] }
        return ($Sortiert[$Mitte - 1] + $Sortiert[$Mitte]) / 2
    }
    $Version = [regex]::Match((Get-Content Cargo.toml -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
    $Daten = [ordered]@{
        measured_at = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        version = $Version
        source_base = (git rev-parse HEAD)
        source_hashes = @{
            'src/sqledit/mod.rs' = (Get-FileHash 'src/sqledit/mod.rs' -Algorithm SHA256).Hash.ToLowerInvariant()
            'src/tabs/sql.rs' = (Get-FileHash 'src/tabs/sql.rs' -Algorithm SHA256).Hash.ToLowerInvariant()
        }
        command = 'cargo test --release --locked performance_render -- --ignored --nocapture --test-threads=1'
        profile = 'release; opt-level=3; lto=true; codegen-units=1'
        cpu = (Get-CimInstance Win32_Processor).Name.Trim()
        os = (Get-CimInstance Win32_OperatingSystem).Caption
        viewport = @{ width = 1200; height = 760 }
        repetitions = $Wiederholungen
        scope = 'CPU UI preparation in headless egui. Excludes GPU presentation, SQL execution, data loading, startup and RAM. Median of per-run mean frame times.'
        editor = @{ sql_lines = 5000; warmup_frames = 3; measured_frames_per_run = 10; samples_ms = $Editor; median_ms = (Median $Editor); source = 'src/sqledit/mod.rs::performance_render_large_editor' }
        grid = @{ rows = 50000; columns = 12; warmup_frames = 5; measured_frames_per_run = 30; samples_ms = $Raster; median_ms = (Median $Raster); source = 'src/tabs/sql.rs::performance_render_large_result' }
        download_sizes_checked_at = '2026-11-07'
        downloads = @(
            @{ app = 'EasyMySQL'; version = '7.1.0'; bytes = 76907468; source = 'https://github.com/gravijet/EasyMySQL/releases/tag/v7.1.0' },
            @{ app = 'XAMPP'; version = '8.5.12'; bytes = 200178909; source = 'https://www.apachefriends.org/download.html'; file = 'xampp-windows-x64-8.5.12-0-VS17-installer.exe' },
            @{ app = 'MySQL Workbench'; version = '26.10.0'; bytes = 273432576; source = 'https://dev.mysql.com/downloads/workbench/'; file = 'mysql-workbench-26.10.0-winx64.msi' }
        )
    }
    [System.IO.File]::WriteAllText($Ausgabepfad, ($Daten | ConvertTo-Json -Depth 6) + "`n", [System.Text.UTF8Encoding]::new($false))
} finally {
    Pop-Location
}
