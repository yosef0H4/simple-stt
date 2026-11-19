#ifndef SimpleSttVersion
  #error "Pass /DSimpleSttVersion from Cargo.toml through the distribution builder"
#endif
[Setup]
AppId={{D638F724-8CC9-4D12-9E63-BEC9FA0D29E4}
AppName=simple-stt
AppVersion={#SimpleSttVersion}
AppPublisher=simple-stt
DefaultDirName={localappdata}\Programs\simple-stt
DefaultGroupName=simple-stt
DisableProgramGroupPage=yes
DisableDirPage=no
UsePreviousAppDir=no
UsePreviousGroup=no
OutputDir=dist
OutputBaseFilename=simple-stt-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
UninstallDisplayName=simple-stt
UninstallDisplayIcon={app}\runtime\icons\simple-stt.ico
SetupIconFile=simple-stt-portable\runtime\icons\simple-stt.ico
LicenseFile=simple-stt-portable\LICENSE

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked
Name: "startup"; Description: "Launch simple-stt when I sign in"; GroupDescription: "Startup options:"; Flags: unchecked

[InstallDelete]
Type: filesandordirs; Name: "{app}\ahk"
Type: files; Name: "{app}\runtime\simple-stt.exe"
Type: files; Name: "{app}\simple-stt.cmd"

[Files]
Source: "simple-stt-portable\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\simple-stt"; Filename: "{app}\simple-stt.cmd"; WorkingDir: "{app}"; IconFilename: "{app}\runtime\icons\simple-stt.ico"
Name: "{autodesktop}\simple-stt"; Filename: "{app}\simple-stt.cmd"; WorkingDir: "{app}"; IconFilename: "{app}\runtime\icons\simple-stt.ico"; Tasks: desktopicon
Name: "{userstartup}\simple-stt"; Filename: "{app}\simple-stt.cmd"; WorkingDir: "{app}"; IconFilename: "{app}\runtime\icons\simple-stt.ico"; Tasks: startup

[Run]
Filename: "{app}\simple-stt.cmd"; WorkingDir: "{app}"; Description: "Launch simple-stt"; Flags: postinstall nowait skipifsilent
