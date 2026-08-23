# Powers the displays off, to exercise the WM's display power handling
# without waiting for the Windows idle timeout.
#
# Usage: .\display-off.ps1 [-Seconds 20]
#
# This sends the DPMS blank that the idle timeout also sends. On many GPUs
# the real idle timeout additionally drops the video link, so confirm a fix
# against the true path too:
#
#   powercfg /change monitor-timeout-ac 1   # then idle for a minute
#   powercfg /change monitor-timeout-ac <original>
#
# Move the mouse to wake the displays early.

param([int]$Seconds = 20)

$signature = @'
[DllImport("user32.dll")]
public static extern int SendMessage(int hWnd, int hMsg, int wParam, int lParam);
'@

$user32 = Add-Type -MemberDefinition $signature -Name PowerHelper `
  -Namespace Win32 -PassThru

$HWND_BROADCAST = 0xFFFF
$WM_SYSCOMMAND = 0x0112
$SC_MONITORPOWER = 0xF170
$MONITOR_OFF = 2

Write-Host "Powering displays off for $Seconds seconds..."
Start-Sleep -Seconds 2

$user32::SendMessage(
  $HWND_BROADCAST, $WM_SYSCOMMAND, $SC_MONITORPOWER, $MONITOR_OFF) | Out-Null

Start-Sleep -Seconds $Seconds
Write-Host "Done. Move the mouse if the displays are still off."
