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
#
# The script downloads the newest release from mysql.benjaminberger.at, checks its SHA-256 checksum
# and runs the regular setup silently. It never clears the terminal, so everything stays in the scrollback.

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
    $Steps = 5

    # --- Output -----------------------------------------------------------------------------------
    function Say([string]$Text, [string]$Color = 'Gray') { Write-Host $Text -ForegroundColor $Color }
    function Step([int]$N, [string]$Title) {
        Write-Host ''
        Write-Host ('  {0}/{1}  ' -f $N, $Steps) -ForegroundColor DarkGray -NoNewline
        Write-Host $Title -ForegroundColor White
    }
    function Ok([string]$Text) { Write-Host '        + ' -ForegroundColor Green -NoNewline; Write-Host $Text }
    function Info([string]$Text) { Write-Host '        . ' -ForegroundColor DarkGray -NoNewline; Write-Host $Text -ForegroundColor Gray }
    function Warn([string]$Text) { Write-Host '        ! ' -ForegroundColor Yellow -NoNewline; Write-Host $Text -ForegroundColor Yellow }
    function Fail([string]$Text) { throw $Text }
    function Ask([string]$Question, [bool]$Default = $true) {
        if ($Yes) { return $true }
        if (-not $Interactive) { return $Default }
        $hint = if ($Default) { '[Y/n]' } else { '[y/N]' }
        Write-Host '        ? ' -ForegroundColor Cyan -NoNewline
        $a = Read-Host "$Question $hint"
        if ([string]::IsNullOrWhiteSpace($a)) { return $Default }
        return $a.Trim().ToLower().StartsWith('y') -or $a.Trim().ToLower().StartsWith('j')
    }
    function Size([double]$Bytes) { '{0:N1} MB' -f ($Bytes / 1MB) }

    function Banner {
        Write-Host ''
        Write-Host '     .-~~~~~~-.   ' -ForegroundColor DarkYellow -NoNewline; Write-Host 'EasyMySQL' -ForegroundColor White
        Write-Host '    (~~~~~~~~~~)  ' -ForegroundColor DarkYellow -NoNewline; Write-Host 'MariaDB server and database manager' -ForegroundColor Gray
        Write-Host '    |~~~~~~~~~~|  ' -ForegroundColor DarkYellow -NoNewline; Write-Host 'SQL editor, ER diagrams, backups, AI assistants' -ForegroundColor DarkGray
        Write-Host '    (__________)  ' -ForegroundColor DarkYellow -NoNewline; Write-Host $Site.Replace('https://', '') -ForegroundColor DarkGray
    }

    # Download with a progress bar on a single line (no flickering, nothing lost in the scrollback)
    function Download([string]$Url, [string]$Target, [double]$Expected) {
        $req = [Net.HttpWebRequest]::Create($Url)
        $req.UserAgent = 'EasyMySQL-install/1'
        $req.Timeout = 30000
        $resp = $req.GetResponse()
        $total = if ($resp.ContentLength -gt 0) { [double]$resp.ContentLength } else { $Expected }
        $in = $resp.GetResponseStream()
        $out = [IO.File]::Create($Target)
        try {
            $buf = New-Object byte[] 262144
            $done = 0.0
            $watch = [Diagnostics.Stopwatch]::StartNew()
            $last = -1000
            while (($n = $in.Read($buf, 0, $buf.Length)) -gt 0) {
                $out.Write($buf, 0, $n)
                $done += $n
                if ($watch.ElapsedMilliseconds - $last -ge 120) {
                    $last = $watch.ElapsedMilliseconds
                    $pct = if ($total -gt 0) { [Math]::Min(1.0, $done / $total) } else { 0 }
                    $width = 28
                    $fill = [int][Math]::Round($pct * $width)
                    $speed = if ($watch.Elapsed.TotalSeconds -gt 0) { $done / $watch.Elapsed.TotalSeconds } else { 0 }
                    $bar = ('#' * $fill) + ('-' * ($width - $fill))
                    Write-Host ("`r        [{0}] {1,3:N0}%  {2} / {3}  {4}/s   " -f $bar, ($pct * 100), (Size $done), (Size $total), (Size $speed)) -NoNewline -ForegroundColor DarkYellow
                }
            }
            $bar = '#' * 28
            Write-Host ("`r        [{0}] 100%  {1} in {2:N1} s              " -f $bar, (Size $done), $watch.Elapsed.TotalSeconds) -ForegroundColor DarkYellow
        } finally {
            $out.Close(); $in.Close(); $resp.Close()
        }
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

    $oldEncoding = [Console]::OutputEncoding
    try {
        try { [Console]::OutputEncoding = [Text.Encoding]::UTF8 } catch {}
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        Banner

        # 1. System -------------------------------------------------------------------------------
        Step 1 'Checking this PC'
        if (-not [Environment]::Is64BitOperatingSystem) { Fail 'EasyMySQL needs 64-bit Windows.' }
        $os = [Environment]::OSVersion.Version
        if ($os.Major -lt 10) { Fail "EasyMySQL needs Windows 10 or 11 (this is $os)." }
        $name = if ($os.Build -ge 22000) { 'Windows 11' } else { 'Windows 10' }
        Ok "$name, 64-bit (build $($os.Build))"
        $mode = if ($Portable) { 'portable, for this user only' } else { 'installer, for all users (asks for administrator rights)' }
        Info "Mode: $mode"
        $existing = Installed-Exe
        if ($existing -and -not $Portable) { Info "Already installed: $existing (will be updated)" }

        # 2. Release ------------------------------------------------------------------------------
        Step 2 'Finding the newest version'
        $rel = Invoke-RestMethod -Uri "$Site/api/release" -Headers @{ 'User-Agent' = 'EasyMySQL-install/1' } -TimeoutSec 30
        $kind = if ($Portable) { 'portable' } else { 'setup' }
        $entry = $rel.files.PSObject.Properties[$kind]
        $file = if ($entry) { $entry.Value } else { $null }
        if (-not $file -or -not $file.sha256) { Fail "The current release has no $kind package with a checksum. Try again in a few minutes." }
        $mdb = if ($rel.mariadb) { ", MariaDB $($rel.mariadb)" } else { '' }
        Ok "EasyMySQL $($rel.version)$mdb"
        Info "$($file.name), $(Size $file.size)"

        # 3. Download -----------------------------------------------------------------------------
        Step 3 'Downloading'
        $tmp = Join-Path ([IO.Path]::GetTempPath()) ("easymysql-" + [Guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $tmp | Out-Null
        $target = Join-Path $tmp $file.name
        Download "$Site$($file.url)" $target ([double]$file.size)

        # 4. Verify -------------------------------------------------------------------------------
        Step 4 'Verifying the SHA-256 checksum'
        $hash = (Get-FileHash -Algorithm SHA256 -Path $target).Hash.ToLower()
        if ($hash -ne $file.sha256.ToLower()) {
            Remove-Item -Recurse -Force $tmp
            Fail "Checksum mismatch - the download was deleted. Expected $($file.sha256), got $hash."
        }
        Ok $hash

        # 5. Install ------------------------------------------------------------------------------
        Step 5 'Installing'
        if ($Portable) {
            $dir = Join-Path $env:LOCALAPPDATA 'Programs\EasyMySQL'
            Get-Process -Name EasyMySQL -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dir*" } | ForEach-Object {
                Warn 'Closing the running EasyMySQL'
                $_.CloseMainWindow() | Out-Null; Start-Sleep -Seconds 3; if (-not $_.HasExited) { $_.Kill() }
            }
            $unpack = Join-Path $tmp 'unpack'
            Expand-Archive -Path $target -DestinationPath $unpack -Force
            $src = Join-Path $unpack 'EasyMySQL'
            if (-not (Test-Path (Join-Path $src 'EasyMySQL.exe'))) { Fail 'The archive does not contain EasyMySQL.exe.' }
            New-Item -ItemType Directory -Force -Path $dir | Out-Null
            Copy-Item -Path (Join-Path $src '*') -Destination $dir -Recurse -Force
            $exe = Join-Path $dir 'EasyMySQL.exe'
            Add-UserPath @($dir, (Join-Path $dir 'mariadb\bin'))
            $menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'EasyMySQL.lnk'
            $sh = New-Object -ComObject WScript.Shell
            $lnk = $sh.CreateShortcut($menu); $lnk.TargetPath = $exe; $lnk.WorkingDirectory = $dir; $lnk.Save()
            Ok "Installed to $dir"
            Ok 'Start menu entry and PATH (easymysql, mysql) for this user'
        } else {
            Info 'Windows asks for administrator rights now ...'
            try {
                $p = Start-Process -FilePath $target -ArgumentList '/SP- /VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Verb RunAs -Wait -PassThru
            } catch {
                Fail 'Administrator rights were not granted. Run again with $env:EASYMYSQL_PORTABLE="1" to install without them.'
            }
            if ($p.ExitCode -ne 0) { Fail "The setup ended with code $($p.ExitCode)." }
            $exe = Installed-Exe
            if (-not $exe) { Fail 'The setup finished, but EasyMySQL.exe was not found.' }
            $dir = Split-Path $exe
            foreach ($d in @($dir, (Join-Path $dir 'mariadb\bin'))) { if (($env:Path -split ';') -notcontains $d) { $env:Path = "$env:Path;$d" } }
            Ok "Installed to $dir"
            Ok 'MariaDB, Start menu entry, PATH (easymysql, mysql)'
        }
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue

        # AI assistants ---------------------------------------------------------------------------
        $assistants = @(@('claude', 'Claude Code'), @('codex', 'Codex') | Where-Object { Get-Command $_[0] -ErrorAction SilentlyContinue } | ForEach-Object { $_[1] })
        $wantAI = $false
        if ($env:EASYMYSQL_AI -eq '1') { $wantAI = $true }
        elseif ($env:EASYMYSQL_AI -ne '0') {
            Write-Host ''
            if ($assistants.Count -eq 2) {
                Info ("Found: " + ($assistants -join ', '))
                $wantAI = Ask 'Connect EasyMySQL to your AI assistants (read-only until you allow changes)?' $true
            } elseif (-not $Yes) {
                if ($assistants.Count -gt 0) { Info ("Found: " + ($assistants -join ', ')) }
                $wantAI = Ask 'Download the AI assistants Claude Code and Codex (about 250 MB) and connect them?' ($assistants.Count -gt 0)
            } elseif ($assistants.Count -gt 0) { $wantAI = $true }
        }
        # install-ai laedt nur, was fehlt (neueste Version mit Pruefsumme), und traegt beide ein
        if ($wantAI) {
            # Aeltere Versionen kennen install-ai noch nicht: dann nur eintragen
            $aiCmd = if ((& $exe --help 2>$null | Out-String) -match 'install-ai') { 'install-ai' } else { 'setup-ai' }
            & $exe $aiCmd | Out-Host
        }

        # Done ------------------------------------------------------------------------------------
        Write-Host ''
        Write-Host '  Done. ' -ForegroundColor Green -NoNewline
        Write-Host "EasyMySQL $($rel.version) is installed." -ForegroundColor White
        Write-Host ''
        Write-Host '    easymysql            ' -ForegroundColor DarkYellow -NoNewline; Write-Host 'open the app (the server starts with it)'
        Write-Host '    mysql -u root        ' -ForegroundColor DarkYellow -NoNewline; Write-Host 'MariaDB console, while EasyMySQL runs'
        Write-Host '    easymysql install-ai ' -ForegroundColor DarkYellow -NoNewline; Write-Host 'download and connect Claude Code and Codex'
        Write-Host '    easymysql mcp        ' -ForegroundColor DarkYellow -NoNewline; Write-Host 'MCP server for any other AI client'
        Write-Host ''
        Write-Host '  Other terminals pick up the new PATH after they are reopened.' -ForegroundColor DarkGray
        if (Ask 'Start EasyMySQL now?' $true) { Start-Process -FilePath $exe }
        Write-Host ''
    } catch {
        Write-Host ''
        Write-Host '  Installation stopped: ' -ForegroundColor Red -NoNewline
        Write-Host $_.Exception.Message
        Write-Host "  Download by hand: $Site" -ForegroundColor DarkGray
        Write-Host ''
    } finally {
        try { [Console]::OutputEncoding = $oldEncoding } catch {}
    }
} @args
