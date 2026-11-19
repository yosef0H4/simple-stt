param(
    [Parameter(Mandatory=$true)][string]$TargetJson,
    [string]$Action,
    [string]$Key,
    [int]$X = -1,
    [int]$Y = -1
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ReadAlongNative {
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint processId);
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; public POINT(int x,int y){X=x;Y=y;} }
  [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern IntPtr PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
}
'@
[void][ReadAlongNative]::SetProcessDpiAwarenessContext([IntPtr]::new(-4))

function Read-Target {
    if ([string]::IsNullOrWhiteSpace($TargetJson)) { throw 'TargetJson is required.' }
    return $TargetJson | ConvertFrom-Json
}

function Get-Window($target) {
    $handle = [IntPtr]::new([Int64]$target.hwnd)
    if (-not [ReadAlongNative]::IsWindow($handle)) { throw 'The selected window was closed.' }
    $minimized = [ReadAlongNative]::IsIconic($handle)
    $processId = [uint32]0
    [void][ReadAlongNative]::GetWindowThreadProcessId($handle, [ref]$processId)
    if ($processId -ne [uint32]$target.pid) { throw 'The selected window identity changed.' }
    $bounds = New-Object ReadAlongNative+RECT
    if (-not [ReadAlongNative]::GetWindowRect($handle, [ref]$bounds)) { throw 'Could not read selected window bounds.' }
    $w = $bounds.Right - $bounds.Left; $h = $bounds.Bottom - $bounds.Top
    if (-not $minimized -and ($target.x -lt 0 -or $target.y -lt 0 -or $target.width -le 0 -or $target.height -le 0 -or
        ([int64]$target.x + $target.width) -gt $w -or ([int64]$target.y + $target.height) -gt $h)) {
        throw 'The saved region no longer fits the selected window.'
    }
    return @{ Handle=$handle; Left=$bounds.Left; Top=$bounds.Top; Minimized=$minimized }
}

$target = Read-Target
$live = Get-Window $target
if ($live.Minimized -or [ReadAlongNative]::GetForegroundWindow() -ne [ReadAlongNative]::GetAncestor($live.Handle,2)) { exit 3 }
if ($Action -eq 'key') {
    $vk = [int][Enum]::Parse([System.Windows.Forms.Keys], $Key, $true)
    [void][ReadAlongNative]::PostMessage($live.Handle, 0x0100, [IntPtr]$vk, [IntPtr]::Zero)
    [void][ReadAlongNative]::PostMessage($live.Handle, 0x0101, [IntPtr]$vk, [IntPtr]::Zero)
    exit 0
}
if ($Action -eq 'click') {
    if ($X -lt 0 -or $Y -lt 0 -or $X -ge $target.width -or $Y -ge $target.height) { throw 'Click point is outside the selected region.' }
    $p = [ReadAlongNative+POINT]::new($live.Left+[int]$target.x+$X, $live.Top+[int]$target.y+$Y)
    if (-not [ReadAlongNative]::ScreenToClient($live.Handle,[ref]$p)) { throw 'Could not map click to selected window.' }
    $lp = (($p.Y -band 0xffff) -shl 16) -bor ($p.X -band 0xffff)
    [void][ReadAlongNative]::PostMessage($live.Handle, 0x0201, [IntPtr]1, [IntPtr]$lp)
    [void][ReadAlongNative]::PostMessage($live.Handle, 0x0202, [IntPtr]0, [IntPtr]$lp)
    exit 0
}
throw 'Action must be key or click.'
