[Setup]
AppId={{D638F724-8CC9-4D12-9E63-BEC9FA0D29E4}
AppName=simple-stt
AppVersion=0.2.5
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
LicenseFile=simple-stt-portable\LICENSE

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked
Name: "startup"; Description: "Launch simple-stt when I sign in"; GroupDescription: "Startup options:"; Flags: unchecked
Name: "downloadmodel"; Description: "Download the recommended speech model during install (~268 MB)"; GroupDescription: "Speech model:"

[InstallDelete]
Type: filesandordirs; Name: "{app}\ahk"
Type: files; Name: "{app}\runtime\simple-stt.exe"
Type: files; Name: "{app}\simple-stt.cmd"
Type: filesandordirs; Name: "{app}\models"
Type: files; Name: "{userprograms}\simple-stt\simple-stt.lnk"
Type: files; Name: "{autodesktop}\simple-stt.lnk"
Type: files; Name: "{userstartup}\simple-stt.lnk"
Type: files; Name: "{userstartup}\Simple STT.lnk"

[Files]
Source: "simple-stt-portable\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\simple-stt"; Filename: "{app}\simple-stt.cmd"; WorkingDir: "{app}"
Name: "{autodesktop}\simple-stt"; Filename: "{app}\simple-stt.cmd"; WorkingDir: "{app}"; Tasks: desktopicon
Name: "{userstartup}\simple-stt"; Filename: "{app}\simple-stt.cmd"; WorkingDir: "{app}"; Tasks: startup

[Run]
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""try {{ $ErrorActionPreference='Stop'; $dir='{app}\runtime\external\parakeet-runtime\parakeet-windows-cuda\models'; New-Item -ItemType Directory -Force -Path $dir | Out-Null; $out=Join-Path $dir 'tdt_ctc-110m-f16.gguf'; if (!(Test-Path -LiteralPath $out)) {{ Invoke-WebRequest -Uri 'https://huggingface.co/mudler/parakeet-cpp-gguf/resolve/main/tdt_ctc-110m-f16.gguf' -OutFile $out }} }} catch {{ Write-Output $_ }}; exit 0"""; StatusMsg: "Downloading recommended speech model..."; Flags: runhidden waituntilterminated; Tasks: downloadmodel
Filename: "{app}\simple-stt.cmd"; WorkingDir: "{app}"; Description: "Launch simple-stt"; Flags: postinstall nowait skipifsilent
