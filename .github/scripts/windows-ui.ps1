# Native Windows-Oberflaeche: Java-Datei oeffnen, Autofix per Tastatur ausloesen und speichern.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Fenster {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int command);
}
'@
$testordner = Join-Path $env:RUNNER_TEMP 'easymysql-ui'
New-Item -ItemType Directory -Force $testordner | Out-Null
$datei = Join-Path $testordner 'Main.java'
@'
public class Main {
    public static void main(String[] args) {
        System.out.printLn("GUI smoke")
    }
}
'@ | Set-Content -Encoding utf8 $datei
$programm = (Resolve-Path 'target/debug/easymysql.exe').Path
$start = [System.Diagnostics.ProcessStartInfo]::new($programm)
$start.Arguments = '"' + $datei + '"'
$start.WorkingDirectory = $testordner
$start.UseShellExecute = $false
$start.Environment['LOCALAPPDATA'] = $testordner
$prozess = [System.Diagnostics.Process]::Start($start)
try {
    $frist = (Get-Date).AddSeconds(60)
    do {
        Start-Sleep -Milliseconds 500
        $prozess.Refresh()
        if ($prozess.HasExited) { throw "EasyMySQL wurde beendet: $($prozess.ExitCode)" }
    } while ($prozess.MainWindowHandle -eq 0 -and (Get-Date) -lt $frist)
    if ($prozess.MainWindowHandle -eq 0) { throw 'Kein natives Fenster geoeffnet.' }
    [Fenster]::ShowWindow($prozess.MainWindowHandle, 9) | Out-Null
    if (-not [Fenster]::SetForegroundWindow($prozess.MainWindowHandle)) { throw 'Fenster konnte nicht fokussiert werden.' }
    Start-Sleep -Seconds 3
    [System.Windows.Forms.SendKeys]::SendWait('{ESC}')
    [System.Windows.Forms.SendKeys]::SendWait('^.')
    Start-Sleep -Seconds 2
    [System.Windows.Forms.SendKeys]::SendWait('^s')
    $frist = (Get-Date).AddSeconds(20)
    do {
        Start-Sleep -Milliseconds 500
        $inhalt = Get-Content -Raw $datei
    } while (-not $inhalt.Contains('System.out.println("GUI smoke");') -and (Get-Date) -lt $frist)
    if (-not $inhalt.Contains('System.out.println("GUI smoke");')) { throw "Autofix/Speichern fehlgeschlagen: $inhalt" }
    $prozess.Refresh()
    if ($prozess.HasExited) { throw 'Fenster nach Autofix abgestuerzt.' }
} finally {
    $bereich = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bild = [System.Drawing.Bitmap]::new($bereich.Width, $bereich.Height)
    $grafik = [System.Drawing.Graphics]::FromImage($bild)
    try {
        $grafik.CopyFromScreen($bereich.Location, [System.Drawing.Point]::Empty, $bereich.Size)
        $bild.Save((Join-Path $testordner 'windows-ui.png'))
    } finally { $grafik.Dispose(); $bild.Dispose() }
    if (-not $prozess.HasExited) { $prozess.Kill(); $prozess.WaitForExit(10000) | Out-Null }
}
