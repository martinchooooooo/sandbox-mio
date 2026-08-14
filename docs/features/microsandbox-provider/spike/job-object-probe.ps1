# Spike probe: does Windows Job Object membership break a real `msb create`?
#
# The e2e harness launches the app through `build.rs`'s `run()`, which calls `win_job::assign`, so
# the app (and therefore every `msb` it spawns) is a job member. `src-tauri/src/mockctl/mod.rs`
# records hands-on that `msb create` fails under `tauri dev` for exactly this reason. This probe
# reproduces the condition directly: it creates a bare job object (no JOB_OBJECT_LIMIT_BREAKAWAY_OK
# — absent by default, which is the whole point), assigns *this* PowerShell process to it, and then
# runs a real `msb create`. Any child inherits the membership.
#
# Deliberately no SetInformationJobObject call: KILL_ON_JOB_CLOSE is irrelevant to breakaway and
# would kill this process on exit. Creating the job is enough to deny breakaway.
#
# Usage:  powershell -NoProfile -File job-object-probe.ps1
# Leaves nothing behind: the sandbox it creates is removed at the end regardless of outcome.

$ErrorActionPreference = 'Continue'

Add-Type -Namespace Probe -Name Job -MemberDefinition @'
[DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
public static extern IntPtr CreateJobObjectW(IntPtr sa, string name);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
[DllImport("kernel32.dll")]
public static extern IntPtr GetCurrentProcess();
'@

$job = [Probe.Job]::CreateJobObjectW([IntPtr]::Zero, $null)
if ($job -eq [IntPtr]::Zero) { Write-Output "FAIL: CreateJobObject failed"; exit 2 }

$assigned = [Probe.Job]::AssignProcessToJobObject($job, [Probe.Job]::GetCurrentProcess())
Write-Output "job assigned to current process: $assigned"
if (-not $assigned) { Write-Output "FAIL: could not assign (already in a job that denies nesting?)"; exit 2 }

$name = "probe-job-$(Get-Random -Maximum 99999)"
# Joined and parsed in two steps: PowerShell 5.1's ConvertFrom-Json can hand a wrapped single
# object back when fed a multi-line array through the pipeline, which loses `reference`.
$json = (& msb image ls --format json 2>$null) -join "`n"
$image = (ConvertFrom-Json $json)[0].reference
if (-not $image) { Write-Output "FAIL: no image in the msb store to create from"; exit 2 }
Write-Output "creating sandbox '$name' from image '$image' INSIDE the job..."

# `MsbProvider::create_instance`'s exact argv, workspace volume included. Creating with a bare
# `msb create <image> --name <n>` instead would fail outside the job too, on a *different* error
# (`invalid config: workdir does not exist in guest: /root/work`), which would make the A/B against
# an out-of-job control meaningless.
& msb volume create "$name-ws" --kind disk --size 20G 2>&1 | Out-Null
$mount = "${name}-ws:/root/work:kind=disk,size=20G"
$out = & msb create $image --name $name --mount-named $mount 2>&1
$code = $LASTEXITCODE
Write-Output "--- msb create exit=$code ---"
$out | ForEach-Object { Write-Output "  $_" }

if ($code -eq 0) {
  Write-Output "RESULT: msb create SUCCEEDED inside a job object - job membership is NOT a blocker"
} else {
  Write-Output "RESULT: msb create FAILED inside a job object - job membership IS a blocker"
}

Write-Output "--- cleanup ---"
& msb rm --force $name 2>&1 | ForEach-Object { Write-Output "  $_" }
& msb volume rm "$name-ws" 2>&1 | Out-Null
Write-Output "cleanup done"
