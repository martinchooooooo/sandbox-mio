# S4 headless (docs/features/host-tool-ssh-access/spike/s2-path.md): end a host ssh client's session
# abruptly and watch what the exec transport leaves in the container.
# Modes: KillSsh (Stop-Process ssh.exe), KillProxy (Stop-Process the podman.exe ProxyCommand child),
# SuspendSsh (freeze ssh.exe so the exec pipe stays open but nothing answers -- a hung client).
# -Config is an ssh_config holding one Host block shaped like the app's alias (see s2-path.md, Setup).
# Use -Seconds 150 for SuspendSsh: the ClientAlive reap took 110-123 s, not the nominal 45 s.
param(
  [ValidateSet('KillSsh', 'KillProxy', 'SuspendSsh')][string]$Mode = 'KillSsh',
  [Parameter(Mandatory)][string]$Name,
  [Parameter(Mandatory)][string]$Config,
  [int]$Seconds = 90
)
Add-Type -Namespace Spike -Name Nt -MemberDefinition @'
[DllImport("ntdll.dll")] public static extern int NtSuspendProcess(System.IntPtr h);
'@
function Get-Proxy { Get-CimInstance Win32_Process -Filter "Name='podman.exe'" | Where-Object { $_.CommandLine -match "exec -i $Name mio-sshd" } }
function Show-State($label) {
  $t = [int]((Get-Date) - $script:start).TotalSeconds
  $procs = & podman --connection podman-machine-default exec $Name sh -c 'pgrep -a sshd; pgrep -a sleep'
  "[t+{0,3}s] {1} | container: {2} | host proxy podman.exe: {3}" -f $t, $label, (($procs | ForEach-Object { $_ }) -join ' ; '), @(Get-Proxy).Count
}
$script:start = Get-Date
$p = Start-Process -FilePath ssh -ArgumentList @('-F', "`"$Config`"", $Name, 'sleep 600') -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 5
Show-State "mode=$Mode, session open (ssh.exe pid $($p.Id))"
switch ($Mode) {
  'KillSsh'    { Stop-Process -Id $p.Id -Force }
  'KillProxy'  { Get-Proxy | ForEach-Object { Stop-Process -Id $_.ProcessId -Force } }
  'SuspendSsh' {
    $rc = [Spike.Nt]::NtSuspendProcess($p.Handle)
    $threads = (Get-Process -Id $p.Id).Threads | Group-Object WaitReason | ForEach-Object { "$($_.Name)=$($_.Count)" }
    "suspend status=$rc ; ssh.exe thread wait reasons: $($threads -join ',')"
  }
}
$script:start = Get-Date
Show-State 'client ended'
while (((Get-Date) - $script:start).TotalSeconds -lt $Seconds) {
  Start-Sleep -Seconds 10
  Show-State 'poll'
}
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
