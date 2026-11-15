# EasyMySQL installer for Windows
#
#   irm https://mysql.benjaminberger.at/install.ps1 | iex
#
# From cmd.exe:
#   powershell -ExecutionPolicy Bypass -c "irm https://mysql.benjaminberger.at/install.ps1 | iex"
#
# Options as environment variables (set them before the command):
#   $env:EASYMYSQL_PORTABLE = "1"   portable version for this user only, no administrator rights
#   $env:EASYMYSQL_AI = "1" / "0"   download and connect Claude Code and Codex without asking / never
#   $env:EASYMYSQL_YES = "1"        answer every question with yes
#   $env:NO_COLOR = "1"             plain output without colors and animations
#
# The script downloads the newest release from mysql.benjaminberger.at, checks its SHA-256 checksum
# and runs the regular setup silently. It never clears the terminal, so everything stays in the scrollback.
# The file is plain ASCII on purpose: special characters are built at runtime, so no code page can garble them.

& {
    param([switch]$Portable, [switch]$Yes)
    Set-StrictMode -Version 2
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'

    $Site = 'https://mysql.benjaminberger.at'
    $AppId = '{6F3C2A51-8E4D-4B7A-9C1E-EA5E3A7D2B10}_is1'
    if ($env:EASYMYSQL_PORTABLE -eq '1') { $Portable = $true }
    if ($env:EASYMYSQL_YES -eq '1') { $Yes = $true }
    $Interactive = [Environment]::UserInteractive -and -not [Console]::IsInputRedirected
    $Started = [Diagnostics.Stopwatch]::StartNew()

    # --- Terminal ---------------------------------------------------------------------------------
    # Farben per VT-Sequenz (24 Bit) nur in einer echten Konsole, die sie versteht
    $VT = $false
    try { $VT = [bool]$Host.UI.SupportsVirtualTerminal } catch {}
    try { if ([Console]::IsOutputRedirected) { $VT = $false } } catch {}
    if ($env:NO_COLOR) { $VT = $false }
    # Windows Terminal und VS Code haben Schriften mit allen Zeichen (Haken, Braille-Spinner, runde Ecken)
    $Rich = $VT -and ([bool]$env:WT_SESSION -or $env:TERM_PROGRAM -eq 'vscode')
    $Cols = 80
    try { $Cols = [int]$Host.UI.RawUI.WindowSize.Width } catch {}
    if ($Cols -lt 20) { $Cols = 80 }

    function U([int[]]$Codes) { -join ($Codes | ForEach-Object { [char]$_ }) }
    $E = [char]27
    function Rgb([int]$R, [int]$G, [int]$B) { if ($VT) { "$E[38;2;${R};${G};${B}m" } else { '' } }
    $Rs = if ($VT) { "$E[0m" } else { '' }
    $Bo = if ($VT) { "$E[1m" } else { '' }
    $Fg = Rgb 236 233 226; $Mute = Rgb 150 156 176; $Sh = Rgb 70 66 110
    $Grn = Rgb 120 220 160; $Yel = Rgb 255 200 110; $Red = Rgb 255 110 120; $Cyn = Rgb 140 200 255; $Acc = Rgb 179 140 255

    # Verlauf Blau -> Violett -> Rosa -> Orange in 16 Stufen
    $Grad = @(for ($i = 0; $i -lt 16; $i++) {
        if ($i -lt 6) { $t = $i / 5; Rgb ([int](122 + 57 * $t)) ([int](162 - 22 * $t)) 255 }
        elseif ($i -lt 11) { $t = ($i - 5) / 5; Rgb ([int](179 + 76 * $t)) ([int](140 + 3 * $t)) ([int](255 - 78 * $t)) }
        else { $t = ($i - 10) / 5; Rgb 255 ([int](143 + 41 * $t)) ([int](177 - 70 * $t)) }
    })

    if ($Rich) {
        $OkG = U 0x2713; $BadG = U 0x2717; $Arrow = U 0x2192; $Full = U 0x2501; $Empty = U 0x2501
        $Spin = @(0x280B, 0x2819, 0x2839, 0x2838, 0x283C, 0x2834, 0x2826, 0x2827, 0x2807, 0x280F | ForEach-Object { U $_ })
        $TL = U 0x256D; $TR = U 0x256E; $BL = U 0x2570; $BR = U 0x256F
    } else {
        $OkG = '+'; $BadG = 'x'; $Arrow = '>'; $Full = U 0x2588; $Empty = U 0x2591
        $Spin = @('|', '/', '-', '\')
        $TL = U 0x250C; $TR = U 0x2510; $BL = U 0x2514; $BR = U 0x2518
    }
    $HLine = U 0x2500; $VLine = U 0x2502
    if ($VT) { $Dot = U 0x00B7; $Ell = U 0x2026 } else { $Dot = '-'; $Ell = '..' }

    # --- Output -----------------------------------------------------------------------------------
    # Jede Zeile: Zeichen, Bezeichnung in fester Breite, Wert. Hinweise stehen eingerueckt darunter.
    function Out([string]$Text) { Write-Host $Text -NoNewline }
    function Line([string]$Color, [string]$Glyph, [string]$Label, [string]$Value) {
        if ($VT) { Out ("  $Color$Glyph$Rs $Bo$Fg{0,-10}$Rs {1}`n" -f $Label, $Value) }
        else {
            $c = switch ($Glyph) { $OkG { 'Green' } '!' { 'Yellow' } default { 'Gray' } }
            Write-Host "  $Glyph " -ForegroundColor $c -NoNewline
            Write-Host ('{0,-10} ' -f $Label) -ForegroundColor White -NoNewline
            Write-Host $Value
        }
    }
    function Done([string]$Label, [string]$Value) { Line $Grn $OkG $Label $Value }
    function Warn([string]$Label, [string]$Value) { Line $Yel '!' $Label "$Yel$Value$Rs" }
    function Detail([string]$Text) { if ($VT) { Out "               $Mute$Text$Rs`n" } else { Write-Host "               $Text" -ForegroundColor DarkGray } }
    function Code([string]$Text) { if ($VT) { Out "               $Cyn$Text$Rs`n" } else { Write-Host "               $Text" -ForegroundColor Cyan } }
    function ClearLine { if ($VT) { Out "`r$E[2K" } }
    function Cursor([bool]$On) { if ($VT) { if ($On) { Out "$E[?25h" } else { Out "$E[?25l" } } }
    function Fail([string]$Text) { throw $Text }
    function Size([double]$Bytes) { '{0:N1} MB' -f ($Bytes / 1MB) }
    function Short([string]$Path) {
        $h = $env:USERPROFILE
        if ($h -and $Path.StartsWith($h, [StringComparison]::OrdinalIgnoreCase)) { return '~' + $Path.Substring($h.Length) }
        return $Path
    }

    function Ask([string]$Question, [bool]$Default = $true) {
        if ($Yes) { return $true }
        if (-not $Interactive) { return $Default }
        $hint = if ($Default) { 'Y/n' } else { 'y/N' }
        if ($VT) { Out "  $Cyn${Bo}?$Rs $Question $Mute($hint)$Rs " } else { Write-Host '  ? ' -ForegroundColor Cyan -NoNewline; Write-Host "$Question ($hint) " -NoNewline }
        $a = Read-Host
        if ([string]::IsNullOrWhiteSpace($a)) { return $Default }
        $a = $a.Trim().ToLower()
        return $a.StartsWith('y') -or $a.StartsWith('j')
    }

    # Fortschrittsbalken mit Verlauf
    function Bar([double]$Fraction, [int]$Width) {
        $fill = [int][Math]::Floor([Math]::Min(1.0, [Math]::Max(0.0, $Fraction)) * $Width)
        $sb = New-Object Text.StringBuilder
        for ($j = 0; $j -lt $Width; $j++) {
            if ($j -lt $fill) { [void]$sb.Append($Grad[[Math]::Min(15, [int][Math]::Floor($j * 16 / $Width))]).Append($Full) }
            else { [void]$sb.Append($Sh).Append($Empty) }
        }
        [void]$sb.Append($Rs)
        $sb.ToString()
    }

    # Wartet auf eine Bedingung und zeigt dabei einen Spinner; ohne VT bleibt es bei einer Zeile
    function Spin([string]$Label, [string]$Text, [scriptblock]$Busy) {
        if (-not $VT) { Line '' $Arrow $Label $Text; while (& $Busy) { Start-Sleep -Milliseconds 100 }; return }
        Cursor $false
        $n = 0
        try {
            while (& $Busy) {
                $f = $Spin[$n % $Spin.Count]; $c = $Grad[[int][Math]::Floor($n / 2) % 16]
                Out ("`r  $c$f$Rs $Bo$Fg{0,-10}$Rs $Mute{1}$Rs$E[K" -f $Label, $Text)
                $n++
                Start-Sleep -Milliseconds 80
            }
        } finally { ClearLine; Cursor $true }
    }

    function Banner {
        Write-Host ''
        $rows = @(
            '#######q #####q #######q##q   ##q###q   ###q##q   ##q#######q ######q ##q'
            '##p====d##p==##q##p====db##q ##pd####q ####|b##q ##pd##p====d##p===##q##|'
            '#####q  #######|#######q b####pd ##p####p##| b####pd #######q##|   ##|##|'
            '##p==d  ##p==##|b====##|  b##pd  ##|b##pd##|  b##pd  b====##|##|__ ##|##|'
            '#######q##|  ##|#######|   ##|   ##| b=d ##|   ##|   #######|b######pd#######q'
            'b======db=d  b=db======d   b=d   b=d     b=d   b=d   b======d b==^^=d b======d'
        )
        if ($VT -and $Cols -ge 84) {
            $map = @{ '#' = 0x2588; 'q' = 0x2557; 'p' = 0x2554; '=' = 0x2550; '|' = 0x2551; 'b' = 0x255A; 'd' = 0x255D; '_' = 0x2584; '^' = 0x2580 }
            foreach ($r in $rows) {
                $sb = New-Object Text.StringBuilder
                [void]$sb.Append('  ')
                $last = ''
                for ($x = 0; $x -lt $r.Length; $x++) {
                    $ch = [string]$r[$x]
                    if ($ch -eq ' ') { [void]$sb.Append(' '); continue }
                    $col = if ($ch -eq '#') { $Grad[[Math]::Min(15, [int][Math]::Floor($x * 16 / 78))] } else { $Sh }
                    if ($col -ne $last) { [void]$sb.Append($col); $last = $col }
                    [void]$sb.Append([char]$map[$ch])
                }
                [void]$sb.Append($Rs)
                Out ($sb.ToString() + "`n")
                Start-Sleep -Milliseconds 35
            }
        } elseif ($VT) {
            $word = 'EasyMySQL'
            $s = '  ' + $Bo
            for ($i = 0; $i -lt $word.Length; $i++) { $s += $Grad[[int]($i * 15 / 8)] + $word[$i] }
            Out ($s + "$Rs`n")
        } else {
            Write-Host '  EasyMySQL' -ForegroundColor Magenta
        }
        Write-Host ''
        if ($VT) { Out "  ${Fg}MariaDB server and database manager$Rs  $Sh$Dot$Rs  ${Mute}SQL editor $Dot ER diagrams $Dot backups $Dot AI$Rs`n" }
        else { Write-Host '  MariaDB server and database manager' -NoNewline; Write-Host "  -  SQL editor, ER diagrams, backups, AI" -ForegroundColor DarkGray }
        Write-Host ''
    }

    # Herunterladen mit eigenem Balken (Prozent, Groesse, Tempo) auf einer Zeile
    function Download([string]$Url, [string]$Target, [double]$Expected) {
        $req = [Net.HttpWebRequest]::Create($Url)
        $req.UserAgent = 'EasyMySQL-install/2'
        $req.Timeout = 30000
        $req.ReadWriteTimeout = 60000
        $resp = $req.GetResponse()
        $total = if ($resp.ContentLength -gt 0) { [double]$resp.ContentLength } else { $Expected }
        $in = $resp.GetResponseStream()
        $out = [IO.File]::Create($Target)
        $width = if ($Cols -lt 72) { 14 } else { 28 }
        $done = 0.0
        $watch = [Diagnostics.Stopwatch]::StartNew()
        if ($VT) { Cursor $false }
        try {
            $buf = New-Object byte[] 262144
            $last = -1000
            while (($n = $in.Read($buf, 0, $buf.Length)) -gt 0) {
                $out.Write($buf, 0, $n)
                $done += $n
                if ($VT -and $watch.ElapsedMilliseconds - $last -ge 100) {
                    $last = $watch.ElapsedMilliseconds
                    $pct = if ($total -gt 0) { [Math]::Min(1.0, $done / $total) } else { 0 }
                    $speed = if ($watch.Elapsed.TotalSeconds -gt 0) { $done / $watch.Elapsed.TotalSeconds } else { 0 }
                    Out ("`r  $Acc$Arrow$Rs $Bo$Fg{0,-10}$Rs {1} $Fg{2,3:N0}%$Rs  $Mute{3} / {4}  {5}/s$Rs$E[K" -f 'Download', (Bar $pct $width), ($pct * 100), (Size $done), (Size $total), (Size $speed))
                }
            }
        } finally {
            $out.Close(); $in.Close(); $resp.Close()
            if ($VT) { ClearLine; Cursor $true }
        }
        $secs = '{0:N1}' -f $watch.Elapsed.TotalSeconds
        if ($VT) { Done 'Download' ("{0} $Mute{1} in {2} s$Rs" -f (Bar 1 $width), (Size $done), $secs) }
        else { Done 'Download' ('{0} in {1} s' -f (Size $done), $secs) }
    }

    function Installed-Exe {
        foreach ($root in 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall', 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall') {
            $k = Join-Path $root $AppId
            if (Test-Path $k) {
                $loc = (Get-ItemProperty $k).InstallLocation
                if ($loc -and (Test-Path (Join-Path $loc 'EasyMySQL.exe'))) { return (Join-Path $loc 'EasyMySQL.exe') }
            }
        }
        $default = Join-Path $env:ProgramFiles 'EasyMySQL\EasyMySQL.exe'
        if (Test-Path $default) { return $default }
        return $null
    }

    # Version einer vorhandenen EasyMySQL.exe aus den Dateiinformationen (startet das Programm nicht)
    function Exe-Version([string]$Path) {
        if (-not $Path -or -not (Test-Path $Path)) { return $null }
        try {
            $v = (Get-Item $Path).VersionInfo.ProductVersion
            if ($v -match '(\d+\.\d+\.\d+)') { return $Matches[1] }
        } catch {}
        return $null
    }

    function Add-UserPath([string[]]$Dirs) {
        $current = [Environment]::GetEnvironmentVariable('Path', 'User')
        $parts = @($current -split ';' | Where-Object { $_ })
        $changed = $false
        foreach ($d in $Dirs) {
            if ($parts -notcontains $d) { $parts += $d; $changed = $true }
            if (($env:Path -split ';') -notcontains $d) { $env:Path = "$env:Path;$d" }
        }
        if ($changed) { [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User') }
    }

    # Rahmen um die Zusammenfassung
    function Card([string]$Title, [string[][]]$Rows) {
        $w = 56
        $rule = $HLine * $w
        Write-Host ''
        if ($VT) {
            Out "  $Sh$TL$rule$TR$Rs`n"
            Out ("  $Sh$VLine$Rs  $Bo$Grn{0,-$($w - 2)}$Rs$Sh$VLine$Rs`n" -f $Title)
            Out ("  $Sh$VLine$Rs{0}$Sh$VLine$Rs`n" -f (' ' * $w))
            foreach ($r in $Rows) {
                Out ("  $Sh$VLine$Rs  $Acc{0,-22}$Rs$Mute{1,-$($w - 24)}$Rs$Sh$VLine$Rs`n" -f $r[0], $r[1])
            }
            Out "  $Sh$BL$rule$BR$Rs`n"
        } else {
            Write-Host "  $Title" -ForegroundColor Green
            foreach ($r in $Rows) { Write-Host ('    {0,-22}' -f $r[0]) -ForegroundColor DarkYellow -NoNewline; Write-Host $r[1] }
        }
    }

    $oldEncoding = [Console]::OutputEncoding
    $tmp = $null
    try {
        try { [Console]::OutputEncoding = [Text.Encoding]::UTF8 } catch {}
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        Banner

        # 1. System -------------------------------------------------------------------------------
        if (-not [Environment]::Is64BitOperatingSystem) { Fail 'EasyMySQL needs 64-bit Windows.' }
        $os = [Environment]::OSVersion.Version
        if ($os.Major -lt 10) { Fail "EasyMySQL needs Windows 10 or 11 (this is $os)." }
        $name = if ($os.Build -ge 22000) { 'Windows 11' } else { 'Windows 10' }
        $mode = if ($Portable) { 'portable, for this user only' } else { 'installer, for all users' }
        Done 'System' "$name $Dot 64-bit $Dot build $($os.Build) $Mute$Dot $mode$Rs"
        $existing = if ($Portable) { Join-Path $env:LOCALAPPDATA 'Programs\EasyMySQL\EasyMySQL.exe' } else { Installed-Exe }
        $old = Exe-Version $existing

        # 2. Release ------------------------------------------------------------------------------
        $job = [Net.WebClient]::new()
        $job.Headers['User-Agent'] = 'EasyMySQL-install/2'
        $job.Headers['Accept'] = 'application/json'
        $job.Encoding = [Text.Encoding]::UTF8
        $task = $job.DownloadStringTaskAsync("$Site/api/release")
        Spin 'Version' "asking $($Site.Replace('https://', '')) for the newest release" { -not $task.IsCompleted }
        if ($task.IsFaulted) { Fail ("The release information could not be loaded: " + $task.Exception.GetBaseException().Message) }
        $rel = $task.Result | ConvertFrom-Json
        $kind = if ($Portable) { 'portable' } else { 'setup' }
        $entry = $rel.files.PSObject.Properties[$kind]
        $file = if ($entry) { $entry.Value } else { $null }
        if (-not $file -or -not $file.sha256) { Fail "The current release has no $kind package with a checksum. Try again in a few minutes." }
        $mdb = if ($rel.PSObject.Properties['mariadb'] -and $rel.mariadb) { " $Dot MariaDB $($rel.mariadb)" } else { '' }
        $sz = " $Dot $(Size $file.size)"
        if ($old -and $old -ne $rel.version) { Done 'Version' "EasyMySQL $old $Acc$Arrow$Rs $Bo$($rel.version)$Rs$Mute$mdb$sz$Rs" }
        elseif ($old) { Done 'Version' "EasyMySQL $Bo$($rel.version)$Rs$Mute$mdb$sz $Dot reinstalling$Rs" }
        else { Done 'Version' "EasyMySQL $Bo$($rel.version)$Rs$Mute$mdb$sz$Rs" }

        # 3. Download -----------------------------------------------------------------------------
        $tmp = Join-Path ([IO.Path]::GetTempPath()) ("easymysql-" + [Guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $tmp | Out-Null
        $target = Join-Path $tmp $file.name
        Download "$Site$($file.url)" $target ([double]$file.size)

        # 4. Verify -------------------------------------------------------------------------------
        $hash = (Get-FileHash -Algorithm SHA256 -Path $target).Hash.ToLower()
        if ($hash -ne $file.sha256.ToLower()) {
            Fail "Checksum mismatch - the download was deleted. Expected $($file.sha256), got $hash."
        }
        Done 'Checksum' ("SHA-256 verified $Mute$Dot {0}$Ell{1}$Rs" -f $hash.Substring(0, 12), $hash.Substring(52))

        # 5. Install ------------------------------------------------------------------------------
        if ($Portable) {
            $dir = Join-Path $env:LOCALAPPDATA 'Programs\EasyMySQL'
            Get-Process -Name EasyMySQL -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dir*" } | ForEach-Object {
                Warn 'Install' 'closing the running EasyMySQL'
                $_.CloseMainWindow() | Out-Null; Start-Sleep -Seconds 3; if (-not $_.HasExited) { $_.Kill() }
            }
            $unpack = Join-Path $tmp 'unpack'
            # .NET statt Expand-Archive: schneller und ohne Fortschrittsanzeige, die das Layout zerreisst
            Add-Type -AssemblyName System.IO.Compression.FileSystem
            [IO.Compression.ZipFile]::ExtractToDirectory($target, $unpack)
            $src = Join-Path $unpack 'EasyMySQL'
            if (-not (Test-Path (Join-Path $src 'EasyMySQL.exe'))) { Fail 'The archive does not contain EasyMySQL.exe.' }
            New-Item -ItemType Directory -Force -Path $dir | Out-Null
            Copy-Item -Path (Join-Path $src '*') -Destination $dir -Recurse -Force
            $exe = Join-Path $dir 'EasyMySQL.exe'
            Add-UserPath @($dir, (Join-Path $dir 'mariadb\bin'))
            $menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'EasyMySQL.lnk'
            $shell = New-Object -ComObject WScript.Shell
            $lnk = $shell.CreateShortcut($menu); $lnk.TargetPath = $exe; $lnk.WorkingDirectory = $dir; $lnk.Save()
            Done 'Install' (Short $dir)
            Detail "Start menu entry $Dot PATH for this user (easymysql, mysql)"
        } else {
            Detail 'Windows asks for administrator rights now ...'
            try {
                $p = Start-Process -FilePath $target -ArgumentList '/SP- /VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Verb RunAs -PassThru
            } catch {
                Fail 'Administrator rights were not granted. Run again with $env:EASYMYSQL_PORTABLE="1" to install without them.'
            }
            Spin 'Install' 'running the setup (MariaDB, drivers, Start menu, PATH)' { -not $p.WaitForExit(0) }
            $p.WaitForExit()
            if ($p.ExitCode -ne 0) { Fail "The setup ended with code $($p.ExitCode)." }
            $exe = Installed-Exe
            if (-not $exe) { Fail 'The setup finished, but EasyMySQL.exe was not found.' }
            $dir = Split-Path $exe
            foreach ($d in @($dir, (Join-Path $dir 'mariadb\bin'))) { if (($env:Path -split ';') -notcontains $d) { $env:Path = "$env:Path;$d" } }
            Done 'Install' $dir
            Detail "MariaDB $Dot Start menu entry $Dot PATH (easymysql, mysql)"
        }
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
        $tmp = $null

        # AI assistants ---------------------------------------------------------------------------
        $assistants = @(@('claude', 'Claude Code'), @('codex', 'Codex') | Where-Object { Get-Command $_[0] -ErrorAction SilentlyContinue } | ForEach-Object { $_[1] })
        $wantAI = $false
        if ($env:EASYMYSQL_AI -eq '1') { $wantAI = $true }
        elseif ($env:EASYMYSQL_AI -ne '0') {
            if ($assistants.Count -eq 2) {
                Write-Host ''
                Detail ("Found: " + ($assistants -join ', '))
                $wantAI = Ask 'Connect EasyMySQL to your AI assistants (read-only until you allow changes)?' $true
            } elseif (-not $Yes) {
                Write-Host ''
                if ($assistants.Count -gt 0) { Detail ("Found: " + ($assistants -join ', ')) }
                $wantAI = Ask 'Download the AI assistants Claude Code and Codex (about 250 MB) and connect them?' ($assistants.Count -gt 0)
            } elseif ($assistants.Count -gt 0) { $wantAI = $true }
        }
        # install-ai laedt nur, was fehlt (neueste Version mit Pruefsumme), und traegt beide ein
        if ($wantAI) {
            # Aeltere Versionen kennen install-ai noch nicht: dann nur eintragen
            $aiCmd = if ((& $exe --help 2>$null | Out-String) -match 'install-ai') { 'install-ai' } else { 'setup-ai' }
            Write-Host ''
            & $exe $aiCmd | Out-Host
            if ($LASTEXITCODE -eq 0) { Done 'AI' 'Claude Code and Codex are connected' }
            else { Warn 'AI' 'setting up the AI assistants did not fully work (see above)' }
        }

        # Done ------------------------------------------------------------------------------------
        Card ("EasyMySQL {0} is ready  ({1:N0} s)" -f $rel.version, $Started.Elapsed.TotalSeconds) @(
            , @('easymysql', 'open the app, the server starts')
            , @('mysql -u root', 'console while EasyMySQL runs')
            , @('easymysql install-ai', 'connect Claude Code + Codex')
            , @('easymysql mcp', 'MCP server for other AI apps')
        )
        Write-Host ''
        Detail 'Other terminals pick up the new PATH after they are reopened.'
        Write-Host ''
        if (Ask 'Start EasyMySQL now?' $true) { Start-Process -FilePath $exe; Done 'Started' 'EasyMySQL opens in a moment' }
        Write-Host ''
    } catch {
        ClearLine; Cursor $true
        Write-Host ''
        if ($VT) { Out "  $Red$Bo$BadG Installation stopped$Rs`n" } else { Write-Host '  Installation stopped' -ForegroundColor Red }
        Write-Host "    $($_.Exception.Message)"
        Write-Host "    Download by hand: $Site" -ForegroundColor DarkGray
        Write-Host ''
    } finally {
        if ($tmp) { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
        try { [Console]::OutputEncoding = $oldEncoding } catch {}
    }
} @args
