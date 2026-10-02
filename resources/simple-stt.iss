[Setup]
AppId={{D638F724-8CC9-4D12-9E63-BEC9FA0D29E4}
AppName=simple-stt
AppVersion=0.3.0
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
Name: "downloadmodel"; Description: "Download the English Q8 speech model during install (~178 MB)"; GroupDescription: "Speech model:"
Name: "installarabic"; Description: "Download the Arabic Q8 speech model during install (~159 MB)"; GroupDescription: "Speech model:"; Flags: unchecked

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
Source: "https://huggingface.co/mudler/parakeet-cpp-gguf/resolve/main/tdt_ctc-110m-q8_0.gguf"; DestDir: "{app}\runtime\external\parakeet-runtime\models"; DestName: "tdt_ctc-110m-q8_0.gguf"; ExternalSize: 177796224; Hash: "614feee3a990cf0e672b0314f4da0c80ae8da9094507f5ccb7c42e43b5fc5a12"; Flags: external download ignoreversion; Tasks: downloadmodel; Check: RecommendedModelNeedsDownload
Source: "https://huggingface.co/yosef0H4/lemura-arabic-asr-lite-GGUF/resolve/main/lemura-arabic-asr-lite-q8_0.gguf"; DestDir: "{app}\runtime\external\parakeet-runtime\models"; DestName: "lemura-arabic-asr-lite-q8_0.gguf"; ExternalSize: 158617056; Hash: "b0aa3f0f316551a45bbd76d1ac7674102a3b221d5fd8c4cd6cac1c2bc4ccce86"; Flags: external download ignoreversion; Tasks: installarabic; Check: ArabicModelNeedsDownload

[Icons]
Name: "{group}\simple-stt"; Filename: "{app}\simple-stt.cmd"; WorkingDir: "{app}"
Name: "{autodesktop}\simple-stt"; Filename: "{app}\simple-stt.cmd"; WorkingDir: "{app}"; Tasks: desktopicon
Name: "{userstartup}\simple-stt"; Filename: "{app}\simple-stt.cmd"; WorkingDir: "{app}"; Tasks: startup

[Run]
Filename: "{app}\simple-stt.cmd"; WorkingDir: "{app}"; Description: "Launch simple-stt"; Flags: postinstall nowait skipifsilent

[Code]
const
  RecommendedModelSHA256 = '614feee3a990cf0e672b0314f4da0c80ae8da9094507f5ccb7c42e43b5fc5a12';

function RecommendedModelNeedsDownload: Boolean;
var
  ModelPath: String;
begin
  ModelPath := ExpandConstant('{app}\runtime\external\parakeet-runtime\models\tdt_ctc-110m-q8_0.gguf');
  if not FileExists(ModelPath) then
    Result := True
  else
    Result := not SameText(GetSHA256OfFile(ModelPath), RecommendedModelSHA256);
end;

function ArabicModelNeedsDownload: Boolean;
var
  ModelPath: String;
begin
  ModelPath := ExpandConstant('{app}\runtime\external\parakeet-runtime\models\lemura-arabic-asr-lite-q8_0.gguf');
  if not FileExists(ModelPath) then
    Result := True
  else
    Result := not SameText(GetSHA256OfFile(ModelPath), 'b0aa3f0f316551a45bbd76d1ac7674102a3b221d5fd8c4cd6cac1c2bc4ccce86');
end;
