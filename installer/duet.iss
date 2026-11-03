; duet's Windows installer (Inno Setup 6.7 or later). scripts\release.ps1 runs every step:
;
;   cargo build --release -p duet-app -p duet-cli
;   python scripts\third-party-notices.py dist\THIRD_PARTY_NOTICES.md
;   "%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe" installer\duet.iss
;
; Reads the version from target\release\duet.exe (its version block comes from Cargo.toml) and
; writes dist\duet-<version>-setup-x64.exe. Pictures: installer\make-art.py. The setup follows the
; Windows light or dark setting.

#define AppExe "duet.exe"
#define BinDir "..\target\release"
#define AppVersion GetVersionNumbersString(AddBackslash(BinDir) + AppExe)
; GetVersionNumbersString gives 0.1.0.0; the release name uses the three-part version
#define ShortVersion Copy(AppVersion, 1, RPos(".", AppVersion) - 1)
#define RepoUrl "https://github.com/ball46/duet"
; A test build can install side by side with a real one: ISCC /DAppName=duet-test /DAppGuid=<another GUID>
#ifndef AppName
  #define AppName "duet"
#endif
#ifndef AppGuid
  #define AppGuid "F2E1350D-7680-4731-B9BC-1D5599BB450C"
#endif
#ifndef WizardMode
  #define WizardMode "dynamic"
#endif

[Setup]
AppId={{{#AppGuid}}
AppName={#AppName}
AppVersion={#ShortVersion}
AppVerName={#AppName} {#ShortVersion}
AppPublisher=ball46
AppPublisherURL={#RepoUrl}
AppSupportURL={#RepoUrl}/issues
AppUpdatesURL={#RepoUrl}/releases
AppCopyright=Copyright (c) 2026 ball46. MIT License.
AppComments=Compare folders and files: text, tables, pictures and bytes. Fast and offline.
VersionInfoVersion={#AppVersion}
VersionInfoDescription=duet setup
VersionInfoProductName=duet

; Per-user by default (no administrator prompt), the same folder scripts\install.ps1 uses;
; the first page lets the user pick "all users" instead.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog commandline
DefaultDirName={autopf}\{#AppName}
DisableDirPage=auto
DisableProgramGroupPage=yes
UsePreviousAppDir=yes

; x64 Windows; Windows 11 on Arm runs it through its x64 emulation. duet.exe needs icuuc.dll,
; which Windows ships from Windows 10 version 1903 (build 18362).
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.18362

WizardStyle=modern {#WizardMode} windows11 hidebevels
; A forced-dark preview (/DWizardMode=dark /DPreviewDark) uses the standard image directives, so it
; swaps in the dark pictures that "dynamic" shows on a dark Windows.
#ifdef PreviewDark
WizardImageFile=art\wizard-dark-202.png,art\wizard-dark-269.png,art\wizard-dark-336.png,art\wizard-dark-404.png,art\wizard-dark-505.png
#else
WizardImageFile=art\wizard-light-202.png,art\wizard-light-269.png,art\wizard-light-336.png,art\wizard-light-404.png,art\wizard-light-505.png
#endif
WizardImageFileDynamicDark=art\wizard-dark-202.png,art\wizard-dark-269.png,art\wizard-dark-336.png,art\wizard-dark-404.png,art\wizard-dark-505.png
WizardSmallImageFile=art\small-58.png,art\small-77.png,art\small-97.png,art\small-116.png,art\small-124.png,art\small-143.png,art\small-159.png
WizardSmallImageFileDynamicDark=art\small-58.png,art\small-77.png,art\small-97.png,art\small-116.png,art\small-124.png,art\small-143.png,art\small-159.png
DisableWelcomePage=no
SetupIconFile=..\crates\duet-app\assets\duet.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
LicenseFile=..\LICENSE
ShowLanguageDialog=auto
LanguageDetectionMethod=uilanguage

; A running duet is closed through Restart Manager before its files are replaced.
CloseApplications=yes
RestartApplications=no
ChangesEnvironment=yes

OutputDir=..\dist
OutputBaseFilename={#AppName}-{#ShortVersion}-setup-x64
Compression=lzma2/ultra64
SolidCompression=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "th"; MessagesFile: "compiler:Languages\Thai.isl"

[Messages]
en.WelcomeLabel1=Welcome to duet
en.WelcomeLabel2=duet compares folders and files side by side: text, tables, pictures and raw bytes, with 3-way merge and sync.%n%nIt runs fully offline, opens big files fast and reads Thai correctly.%n%nThis will install [name/ver] on your computer.
th.WelcomeLabel1=ยินดีต้อนรับสู่ duet
th.WelcomeLabel2=duet เทียบโฟลเดอร์และไฟล์แบบวางคู่กัน ทั้งข้อความ ตาราง รูปภาพ และข้อมูลไบต์ มีรวมสามทางและซิงก์โฟลเดอร์ด้วย%n%nทำงานแบบออฟไลน์ทั้งหมด เปิดไฟล์ใหญ่ได้เร็ว และแสดงภาษาไทยได้ถูกต้อง%n%nโปรแกรมนี้จะติดตั้ง [name/ver] ลงในเครื่องของคุณ
en.FinishedHeadingLabel=duet is ready
th.FinishedHeadingLabel=ติดตั้ง duet เรียบร้อยแล้ว

[CustomMessages]
en.AddToPath=Add duet-cli to PATH (to compare from a terminal)
th.AddToPath=เพิ่ม duet-cli ลงใน PATH (สำหรับสั่งเทียบจาก terminal)
en.ExtraTasks=More:
th.ExtraTasks=เพิ่มเติม:
en.OpenGuide=Report a problem or ask for a feature on GitHub
th.OpenGuide=แจ้งปัญหาหรือขอฟีเจอร์ใหม่บน GitHub

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "addtopath"; Description: "{cm:AddToPath}"; GroupDescription: "{cm:ExtraTasks}"; Flags: unchecked

[Files]
Source: "{#BinDir}\duet.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\duet-cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
; written by scripts\third-party-notices.py before the installer is built
Source: "..\dist\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "Compare folders and files"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "Compare folders and files"; Tasks: desktopicon

[Registry]
; duet-cli on PATH: the current user's PATH, or the machine's when installed for all users
Root: HKA; Subkey: "{code:EnvKey}"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: addtopath; Check: PathMissing(ExpandConstant('{app}'))

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,duet}"; Flags: nowait postinstall skipifsilent
Filename: "{#RepoUrl}/issues/new/choose"; Description: "{cm:OpenGuide}"; Flags: shellexec nowait postinstall skipifsilent unchecked

[Code]
function EnvKey(Param: String): String;
begin
  if IsAdminInstallMode then
    Result := 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
  else
    Result := 'Environment';
end;

function EnvRoot: Integer;
begin
  if IsAdminInstallMode then
    Result := HKEY_LOCAL_MACHINE
  else
    Result := HKEY_CURRENT_USER;
end;

{ True when Dir is not already one of PATH's entries (case and a trailing backslash ignored). }
function PathMissing(Dir: String): Boolean;
var
  Path: String;
begin
  if not RegQueryStringValue(EnvRoot, EnvKey(''), 'Path', Path) then
    Path := '';
  Result := Pos(';' + Uppercase(RemoveBackslashUnlessRoot(Dir)) + ';',
                ';' + Uppercase(Path) + ';') = 0;
end;

{ Uninstall takes the install folder back out of PATH, leaving every other entry as it was. }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Path, Dir, Entry, Kept: String;
  P: Integer;
  Removed: Boolean;
begin
  if CurUninstallStep <> usPostUninstall then
    Exit;
  if not RegQueryStringValue(EnvRoot, EnvKey(''), 'Path', Path) then
    Exit;
  Dir := Uppercase(RemoveBackslashUnlessRoot(ExpandConstant('{app}')));
  Kept := '';
  Removed := False;
  while Path <> '' do
  begin
    P := Pos(';', Path);
    if P = 0 then
    begin
      Entry := Path;
      Path := '';
    end
    else
    begin
      Entry := Copy(Path, 1, P - 1);
      Delete(Path, 1, P);
    end;
    if (Entry <> '') and (Uppercase(RemoveBackslashUnlessRoot(Entry)) = Dir) then
      Removed := True
    else if Entry <> '' then
    begin
      if Kept <> '' then
        Kept := Kept + ';';
      Kept := Kept + Entry;
    end;
  end;
  if Removed then
    RegWriteExpandStringValue(EnvRoot, EnvKey(''), 'Path', Kept);
end;
