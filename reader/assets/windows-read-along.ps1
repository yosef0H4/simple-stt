param(
    [Parameter(Mandatory=$true)][ValidateSet('Select','Inspect','Capture','Advance')][string]$Mode,
    [string]$TargetJson,
    [string]$OutputPath,
    [string]$Action,
    [string]$Key,
    [int]$X = -1,
    [int]$Y = -1
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ReadAlongNative {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint processId);
  [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern IntPtr PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; public POINT(int x,int y){X=x;Y=y;} }
}
'@

function Read-Target {
    if ([string]::IsNullOrWhiteSpace($TargetJson)) { throw 'TargetJson is required.' }
    return $TargetJson | ConvertFrom-Json
}

function Get-Window($target) {
    $handle = [IntPtr]::new([Int64]$target.hwnd)
    if (-not [ReadAlongNative]::IsWindow($handle)) { throw 'The selected window was closed.' }
    if ([ReadAlongNative]::IsIconic($handle)) { throw 'The selected window is minimized.' }
    $processId = [uint32]0
    [void][ReadAlongNative]::GetWindowThreadProcessId($handle, [ref]$processId)
    if ($processId -ne [uint32]$target.pid) { throw 'The selected window identity changed.' }
    $bounds = New-Object ReadAlongNative+RECT
    if (-not [ReadAlongNative]::GetWindowRect($handle, [ref]$bounds)) { throw 'Could not read selected window bounds.' }
    $w = $bounds.Right - $bounds.Left; $h = $bounds.Bottom - $bounds.Top
    if ($target.x -lt 0 -or $target.y -lt 0 -or $target.width -le 0 -or $target.height -le 0 -or
        ([int64]$target.x + $target.width) -gt $w -or ([int64]$target.y + $target.height) -gt $h) {
        throw 'The saved region no longer fits the selected window.'
    }
    return @{ Handle=$handle; Pid=$processId; Left=$bounds.Left; Top=$bounds.Top; Width=$w; Height=$h }
}

function Save-Region($target) {
    $live = Get-Window $target
    $bitmap = [System.Drawing.Bitmap]::new($live.Width, $live.Height)
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $dc = $graphics.GetHdc()
        try {
            if (-not [ReadAlongNative]::PrintWindow($live.Handle, $dc, 2)) { throw 'The selected window could not be captured.' }
        }
        finally { $graphics.ReleaseHdc($dc); $graphics.Dispose() }
        $cropRect = [System.Drawing.Rectangle]::new([int]$target.x, [int]$target.y, [int]$target.width, [int]$target.height)
        $crop = $bitmap.Clone($cropRect, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try { $crop.Save($OutputPath, [System.Drawing.Imaging.ImageFormat]::Png) }
        finally { $crop.Dispose() }
    } finally { $bitmap.Dispose() }
}

if ($Mode -eq 'Select') {
    $bounds = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $form = New-Object System.Windows.Forms.Form
    $form.FormBorderStyle = 'None'; $form.StartPosition = 'Manual'; $form.Bounds = $bounds
    $form.TopMost = $true; $form.ShowInTaskbar = $false; $form.KeyPreview = $true
    $form.Opacity = 0.16; $form.BackColor = [System.Drawing.Color]::Black
    $form.Cursor = [System.Windows.Forms.Cursors]::Cross
    $script:anchor = $null; $script:drag = $null; $script:done = $false; $script:screenRect = $null
    $form.Add_MouseDown({ if ($_.Button -eq [System.Windows.Forms.MouseButtons]::Left) { $script:anchor=$_.Location; $script:drag=$true } })
    $form.Add_MouseMove({ if ($script:drag) { $form.Invalidate() } })
    $form.Add_Paint({
        if ($script:drag -and $script:anchor) {
            $p = $form.PointToClient([System.Windows.Forms.Cursor]::Position)
            $r = [System.Drawing.Rectangle]::FromLTRB([Math]::Min($script:anchor.X,$p.X),[Math]::Min($script:anchor.Y,$p.Y),[Math]::Max($script:anchor.X,$p.X),[Math]::Max($script:anchor.Y,$p.Y))
            $_.Graphics.DrawRectangle([System.Drawing.Pens]::LimeGreen, $r)
        }
    })
    $form.Add_MouseUp({
        if ($_.Button -eq [System.Windows.Forms.MouseButtons]::Left -and $script:anchor) {
            $p = $form.PointToClient([System.Windows.Forms.Cursor]::Position)
            $left=[Math]::Min($script:anchor.X,$p.X); $top=[Math]::Min($script:anchor.Y,$p.Y)
            $width=[Math]::Abs($script:anchor.X-$p.X); $height=[Math]::Abs($script:anchor.Y-$p.Y)
            if ($width -ge 2 -and $height -ge 2) {
                $script:screenRect = @{ x=($bounds.Left+$left); y=($bounds.Top+$top); width=$width; height=$height }
                $script:done=$true
            }
            $script:drag=$false; $form.Close()
        }
    })
    $form.Add_KeyDown({ if ($_.KeyCode -eq [System.Windows.Forms.Keys]::Escape) { $form.Close() } })
    [void]$form.ShowDialog()
    if (-not $script:done) { exit 2 }
    # Resolve ownership after the picker has closed so WindowFromPoint cannot
    # return the picker itself. Refuse the desktop or an invalid target.
    $rect = $script:screenRect
    $screen = [ReadAlongNative+POINT]::new([int]($rect.x+$rect.width/2),[int]($rect.y+$rect.height/2))
    $hwnd = [ReadAlongNative]::WindowFromPoint($screen)
    $wr = New-Object ReadAlongNative+RECT
    if ($hwnd -eq [IntPtr]::Zero -or -not [ReadAlongNative]::GetWindowRect($hwnd,[ref]$wr)) { exit 2 }
    $processId = [uint32]0
    [void][ReadAlongNative]::GetWindowThreadProcessId($hwnd, [ref]$processId)
    $script:result = @{ hwnd=$hwnd.ToInt64(); pid=$processId; x=($rect.x-$wr.Left); y=($rect.y-$wr.Top); width=$rect.width; height=$rect.height }
    $script:result | ConvertTo-Json -Compress
    exit 0
}

$target = Read-Target
$live = Get-Window $target
if ($Mode -eq 'Inspect') {
    [pscustomobject]@{ left=$live.Left; top=$live.Top; width=$live.Width; height=$live.Height; active=([ReadAlongNative]::GetForegroundWindow() -eq [ReadAlongNative]::GetAncestor($live.Handle,2)) } | ConvertTo-Json -Compress
    exit 0
}
if ($Mode -eq 'Capture') { Save-Region $target; exit 0 }
if ($Mode -eq 'Advance') {
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
}
