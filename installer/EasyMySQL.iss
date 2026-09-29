; EasyMySQL Setup (Inno Setup 6)
; Wird von der GitHub-Action gebaut:  ISCC /DAppVersion=1.0.0 installer\EasyMySQL.iss
; Erwartet:
;   ..\target\release\easymysql.exe      (cargo build --release)
;   ..\target\easymysql.ico              (von build.rs erzeugt)
;   ..\build\mariadb\...                 (entpacktes MariaDB-ZIP)
;   ..\build\redist\vc_redist.x64.exe
;   ..\build\redist\mariadb-connector-odbc.msi

#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif
#define HaveOdbc FileExists(AddBackslash(SourcePath) + "..\build\redist\mariadb-connector-odbc.msi")
#define HaveVcRedist FileExists(AddBackslash(SourcePath) + "..\build\redist\vc_redist.x64.exe")

#ifndef MariaDBVersion
  #define MariaDBVersion "11.8"
#endif

[Setup]
AppId={{6F3C2A51-8E4D-4B7A-9C1E-EA5E3A7D2B10}
AppName=EasyMySQL
AppVersion={#AppVersion}
AppVerName=EasyMySQL {#AppVersion}
AppPublisher=EasyMySQL
DefaultDirName={autopf}\EasyMySQL
DefaultGroupName=EasyMySQL
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename=EasyMySQL-Setup-{#AppVersion}
SetupIconFile=..\target\easymysql.ico
UninstallDisplayIcon={app}\EasyMySQL.exe
UninstallDisplayName=EasyMySQL {#AppVersion} (MariaDB {#MariaDBVersion})
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
ChangesEnvironment=yes
WizardStyle=modern
CloseApplications=no
MinVersion=10.0

[Languages]
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Tasks]
Name: "desktopicon"; Description: "Desktop-Verknüpfung erstellen"; GroupDescription: "Zusätzliche Verknüpfungen:"
#if HaveOdbc
Name: "odbc"; Description: "MariaDB ODBC-Treiber installieren und Datenquelle ""EasyMySQL"" anlegen (für Excel, Access, LibreOffice ...)"; GroupDescription: "Treiber:"
#endif

[Dirs]
; Gemeinsamer Datenordner (kurzer Pfad ohne Umlaute), für alle Benutzer beschreibbar.
Name: "{commonappdata}\EasyMySQL"; Permissions: users-modify; Flags: uninsneveruninstall

[Files]
Source: "..\target\release\easymysql.exe"; DestDir: "{app}"; DestName: "EasyMySQL.exe"; Flags: ignoreversion
Source: "..\build\mariadb\*"; DestDir: "{app}\mariadb"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\README.md"; DestDir: "{app}"; DestName: "LIESMICH.md"; Flags: ignoreversion
#if HaveVcRedist
Source: "..\build\redist\vc_redist.x64.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall
#endif
#if HaveOdbc
Source: "..\build\redist\mariadb-connector-odbc.msi"; DestDir: "{tmp}"; Flags: deleteafterinstall; Tasks: odbc
#endif

[Icons]
Name: "{autoprograms}\EasyMySQL"; Filename: "{app}\EasyMySQL.exe"
Name: "{autoprograms}\MariaDB-Konsole (mysql -u root)"; Filename: "{cmd}"; Parameters: "/k mysql -u root"; WorkingDir: "{userdocs}"; IconFilename: "{app}\EasyMySQL.exe"
Name: "{autodesktop}\EasyMySQL"; Filename: "{app}\EasyMySQL.exe"; Tasks: desktopicon

[Registry]
; mysql, mysqldump, mariadb ... in der Eingabeaufforderung verfügbar machen
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}\mariadb\bin"; Check: NeedsAddPath(ExpandConstant('{app}\mariadb\bin'))
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"; ValueType: string; ValueName: "EASYMYSQL_HOME"; ValueData: "{app}"; Flags: uninsdeletevalue
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"; ValueType: string; ValueName: "MARIADB_HOME"; ValueData: "{app}\mariadb"; Flags: uninsdeletevalue

[Run]
#if HaveVcRedist
Filename: "{tmp}\vc_redist.x64.exe"; Parameters: "/install /quiet /norestart"; StatusMsg: "Visual C++ Laufzeitbibliotheken werden installiert ..."; Flags: waituntilterminated
#endif
#if HaveOdbc
Filename: "msiexec.exe"; Parameters: "/i ""{tmp}\mariadb-connector-odbc.msi"" /qn /norestart"; StatusMsg: "MariaDB ODBC-Treiber wird installiert ..."; Tasks: odbc; Flags: waituntilterminated
Filename: "{sys}\odbcconf.exe"; Parameters: "/a {{CONFIGSYSDSN ""MariaDB ODBC 3.2 Driver"" ""DSN=EasyMySQL|DESCRIPTION=EasyMySQL (lokaler MariaDB-Server)|SERVER=127.0.0.1|PORT=3306|USER=root""}"; StatusMsg: "ODBC-Datenquelle wird angelegt ..."; Tasks: odbc; Flags: runhidden waituntilterminated
#endif
Filename: "{app}\EasyMySQL.exe"; Description: "EasyMySQL jetzt starten"; Flags: postinstall nowait skipifsilent runasoriginaluser

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM EasyMySQL.exe"; Flags: runhidden waituntilterminated; RunOnceId: "StopApp"
Filename: "{app}\mariadb\bin\mariadb-admin.exe"; Parameters: "-u root --connect-timeout=3 shutdown"; Flags: runhidden waituntilterminated skipifdoesntexist; RunOnceId: "StopServer"

[Code]
const
  EnvKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

function NeedsAddPath(Param: string): Boolean;
var
  OrigPath: string;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE, EnvKey, 'Path', OrigPath) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Uppercase(Param) + ';', ';' + Uppercase(OrigPath) + ';') = 0;
end;

procedure RemovePath(Dir: string);
var
  Path, Upper, Needle: string;
  P: Integer;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE, EnvKey, 'Path', Path) then
    exit;
  Path := ';' + Path + ';';
  Upper := Uppercase(Path);
  Needle := ';' + Uppercase(Dir) + ';';
  P := Pos(Needle, Upper);
  while P > 0 do
  begin
    Delete(Path, P, Length(Dir) + 1);
    Upper := Uppercase(Path);
    P := Pos(Needle, Upper);
  end;
  Path := Copy(Path, 2, Length(Path) - 2);
  RegWriteExpandStringValue(HKEY_LOCAL_MACHINE, EnvKey, 'Path', Path);
end;

// Laufende Version beenden, damit Dateien ersetzt werden können (Update)
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Rc: Integer;
  Admin: string;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM EasyMySQL.exe', '', SW_HIDE, ewWaitUntilTerminated, Rc);
  Admin := ExpandConstant('{app}\mariadb\bin\mariadb-admin.exe');
  if FileExists(Admin) then
  begin
    Exec(Admin, '-u root --connect-timeout=3 shutdown', '', SW_HIDE, ewWaitUntilTerminated, Rc);
    Sleep(1500);
  end;
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    RemovePath(ExpandConstant('{app}\mariadb\bin'));
    MsgBox('EasyMySQL wurde entfernt.' + #13#10 + #13#10 +
      'Ihre Datenbanken wurden NICHT gelöscht. Sie liegen weiterhin in:' + #13#10 +
      ExpandConstant('{commonappdata}\EasyMySQL'), mbInformation, MB_OK);
  end;
end;
