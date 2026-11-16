param([Parameter(Mandatory=$true)][string]$OutputPath)
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$bounds = [System.Windows.Forms.SystemInformation]::VirtualScreen
$form = New-Object System.Windows.Forms.Form
$form.FormBorderStyle = [System.Windows.Forms.FormBorderStyle]::None
$form.StartPosition = [System.Windows.Forms.FormStartPosition]::Manual
$form.Bounds = $bounds
$form.TopMost = $true
$form.ShowInTaskbar = $false
$form.KeyPreview = $true
$form.Opacity = 0.18
$form.BackColor = [System.Drawing.Color]::Black
$form.Cursor = [System.Windows.Forms.Cursors]::Cross
$script:origin = $null
$script:selection = $null
$script:dragging = $false
$form.Add_MouseDown({
    if ($_.Button -eq [System.Windows.Forms.MouseButtons]::Left) {
        $script:origin = $_.Location
        $script:selection = [System.Drawing.Rectangle]::new($script:origin.X, $script:origin.Y, 0, 0)
        $script:dragging = $true
    }
})
$form.Add_MouseMove({
    if ($script:dragging) {
        $x = [Math]::Min($script:origin.X, $_.X)
        $y = [Math]::Min($script:origin.Y, $_.Y)
        $w = [Math]::Abs($_.X - $script:origin.X)
        $h = [Math]::Abs($_.Y - $script:origin.Y)
        $script:selection = [System.Drawing.Rectangle]::new($x, $y, $w, $h)
        $form.Invalidate()
    }
})
$form.Add_Paint({
    if ($script:dragging -and $script:selection -ne $null) {
        $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::LimeGreen, 2)
        $_.Graphics.DrawRectangle($pen, $script:selection)
        $pen.Dispose()
    }
})
$form.Add_MouseUp({
    if ($_.Button -eq [System.Windows.Forms.MouseButtons]::Left) {
        $script:dragging = $false
        $form.Close()
    }
})
$form.Add_KeyDown({
    if ($_.KeyCode -eq [System.Windows.Forms.Keys]::Escape) { $form.Close() }
})
[void]$form.ShowDialog()
if ($script:selection -eq $null -or $script:selection.Width -lt 2 -or $script:selection.Height -lt 2) { exit 2 }
$captureBounds = [System.Drawing.Rectangle]::new(
    ($bounds.Left + $script:selection.X), ($bounds.Top + $script:selection.Y),
    $script:selection.Width, $script:selection.Height)
$bitmap = [System.Drawing.Bitmap]::new($captureBounds.Width, $captureBounds.Height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($captureBounds.Location, [System.Drawing.Point]::Empty, $captureBounds.Size)
$bitmap.Save($OutputPath, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
