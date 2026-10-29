# check-windows.ps1 (M8, D-59): checks that Cairn can run on this Windows computer.
#
#   powershell -ExecutionPolicy Bypass -File check-windows.ps1
#
# Run it from the unzipped Cairn folder. It prints a plain-text report, one line per item:
#   PASS  fine      WARN  works, but look at it      FAIL  something is wrong      INFO  for the record
# Paste the whole report back. It takes a few minutes: the self-test runs twice (the first start is when
# Microsoft Defender scans the files) and a Cairn window opens for a few seconds for the scripted UI check.
# Do not click in that window.
#
# It changes nothing outside a throw-away folder in %TEMP% (deleted at the end) and one test entry in
# Credential Manager (deleted at once). Cairn's window keeps its web view cache where it always does.
# This file is plain ASCII on purpose (PowerShell 5.1 reads a script without a byte-order mark as ANSI).

param(
    [switch]$SkipUi      # skip the scripted UI check (opens a window)
)

$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'

$Here = $null
if ($MyInvocation.MyCommand.Path) { $Here = Split-Path -Parent $MyInvocation.MyCommand.Path }
if (-not $Here) { $Here = (Get-Location).Path }
$Here = $Here.TrimEnd('\')
$Exe = Join-Path $Here 'cairn.exe'
$Tmp = Join-Path $env:TEMP ('cairn-check-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$null = New-Item -ItemType Directory -Path $Tmp -Force

$script:Counts = @{ PASS = 0; WARN = 0; FAIL = 0; INFO = 0 }

function Hide-User([string]$s) {
    # keep the report free of the user name where it is easy to do
    if ($env:USERPROFILE) { $s = $s.Replace($env:USERPROFILE, '%USERPROFILE%') }
    return $s
}

function Add-Result([string]$Level, [string]$Text) {
    $Text = (Hide-User $Text) -replace '[\r\n]+', ' | '
    $script:Counts[$Level] = $script:Counts[$Level] + 1
    Write-Host ('{0,-5} {1}' -f $Level, $Text)
}

function Set-EnvVars([hashtable]$Vars) {
    foreach ($k in $Vars.Keys) { [Environment]::SetEnvironmentVariable($k, $Vars[$k], 'Process') }
}

function Format-Seconds([double]$ms) { return ('{0:N1} s' -f ($ms / 1000.0)) }

# Starts a program and waits at most $TimeoutSec seconds. Returns an object with ExitCode, TimedOut and Ms.
# Only the process started here is ever stopped.
function Invoke-Timed([string]$File, [string[]]$Arguments, [int]$TimeoutSec) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    if ($Arguments -and $Arguments.Count -gt 0) {
        $p = Start-Process -FilePath $File -ArgumentList $Arguments -WorkingDirectory $Here -PassThru
    } else {
        $p = Start-Process -FilePath $File -WorkingDirectory $Here -PassThru
    }
    $null = $p.Handle     # keeps the handle, so ExitCode is available after the exit
    $done = $p.WaitForExit($TimeoutSec * 1000)
    $timedOut = -not $done
    if ($timedOut) { try { $p.Kill(); $null = $p.WaitForExit(10000) } catch { } }
    $sw.Stop()
    $code = $null
    try { $code = $p.ExitCode } catch { }
    return New-Object psobject -Property @{ ExitCode = $code; TimedOut = $timedOut; Ms = $sw.Elapsed.TotalMilliseconds }
}

function Read-Lines([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return @() }
    return @(Get-Content -LiteralPath $Path -Encoding UTF8 | Where-Object { $_ -and $_.Trim() })
}

function Test-Under([string]$Path, [string]$Root) {
    if (-not $Root) { return $false }
    $r = $Root.TrimEnd('\') + '\'
    return ($Path.TrimEnd('\') + '\').StartsWith($r, [StringComparison]::OrdinalIgnoreCase)
}

Write-Host '=============================================================='
Write-Host ('Cairn check on Windows, ' + (Get-Date -Format 'yyyy-MM-dd HH:mm'))
Write-Host ('PowerShell ' + $PSVersionTable.PSVersion.ToString())
Write-Host '=============================================================='

# ---------------------------------------------------------------- the folder
Write-Host ''
Write-Host '--- the Cairn folder'
Add-Result 'INFO' ("folder: $Here")
$haveExe = Test-Path -LiteralPath $Exe
$haveDataBefore = Test-Path -LiteralPath (Join-Path $Here 'data')
# A lite zip has no models folder: search by meaning needs the model, which Settings > Search adds (NFR-INS-1b, D-82).
$lite = -not (Test-Path -LiteralPath (Join-Path $Here 'models'))
if ($lite) { Add-Result 'INFO' 'lite: no search model bundled' }
$required = @('cairn.exe', 'sidecar\python\python.exe', 'NOTICE', 'ui-check.js')
if (-not $lite) {
    $required += @('models\embeddinggemma-300m\tokenizer.json', 'models\embeddinggemma-300m\onnx\model_q4.onnx',
        'models\embeddinggemma-300m\onnx\model_q4.onnx_data')
}
$missing = @($required | Where-Object { -not (Test-Path -LiteralPath (Join-Path $Here $_)) })
if ($missing.Count -eq 0) { Add-Result 'PASS' 'the files Cairn needs are all there' }
else { Add-Result 'FAIL' ('missing from the folder: ' + ($missing -join ', ')) }

# every file, with the long-path prefix so that deep paths can be listed even when long paths are off
$files = @()
$listError = $false
try {
    $files = @(Get-ChildItem -LiteralPath ('\\?\' + $Here) -Recurse -File -Force -ErrorAction Stop)
} catch { $listError = $true }
if ($listError) { Add-Result 'WARN' 'could not list every file in the folder (path too long, or access denied)' }
$prefixLen = 4 + $Here.Length + 1
$deepest = 0
$deepestName = ''
$totalBytes = 0
foreach ($f in $files) {
    $rel = $f.FullName.Length - $prefixLen
    if ($rel -gt $deepest) { $deepest = $rel; $deepestName = $f.FullName.Substring($prefixLen) }
    $totalBytes += $f.Length
}
Add-Result 'INFO' ('{0} files, {1:N0} MB; the deepest file is {2} characters inside the folder' -f $files.Count, ($totalBytes / 1MB), $deepest)

# ---------------------------------------------------------------- system
Write-Host ''
Write-Host '--- system'
$os = $null
try { $os = Get-CimInstance Win32_OperatingSystem } catch { }
$build = 0
if ($os) { $build = [int]$os.BuildNumber }
if ($os) {
    Add-Result 'INFO' ('{0}, version {1}, build {2}' -f $os.Caption, $os.Version, $os.BuildNumber)
    if ($build -ge 22000) { Add-Result 'PASS' 'Windows 11' } else { Add-Result 'WARN' 'this is not Windows 11 (Cairn is built for Windows 11)' }
} else {
    Add-Result 'WARN' 'could not read the Windows version'
}
$arch = $env:PROCESSOR_ARCHITECTURE
if ($env:PROCESSOR_ARCHITEW6432) { $arch = $env:PROCESSOR_ARCHITEW6432 }
if ($arch -eq 'AMD64') { Add-Result 'PASS' 'architecture x64 (AMD64)' }
else { Add-Result 'WARN' ("architecture $arch (Cairn is built for x64; Windows on ARM can run it through emulation, slowly)") }
try {
    $root = [IO.Path]::GetPathRoot($Here)
    $free = (New-Object IO.DriveInfo($root)).AvailableFreeSpace
    if ($free -ge 3GB) { Add-Result 'PASS' ('free space on {0}: {1:N1} GB' -f $root, ($free / 1GB)) }
    else { Add-Result 'WARN' ('free space on {0}: {1:N1} GB (Cairn needs about 1 GB plus your data)' -f $root, ($free / 1GB)) }
    $troot = [IO.Path]::GetPathRoot($env:TEMP)
    if ($troot -ne $root) {
        $tfree = (New-Object IO.DriveInfo($troot)).AvailableFreeSpace
        Add-Result 'INFO' ('free space on {0} (TEMP): {1:N1} GB' -f $troot, ($tfree / 1GB))
    }
} catch { Add-Result 'WARN' ("could not read the free disk space: $($_.Exception.Message)") }

# ---------------------------------------------------------------- long paths
Write-Host ''
Write-Host '--- long paths'
$longOn = $null
try { $longOn = (Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name LongPathsEnabled -ErrorAction Stop).LongPathsEnabled } catch { }
$deepestTotal = $Here.Length + 1 + $deepest
if ($longOn -eq 1) {
    Add-Result 'PASS' ("long paths are switched on (LongPathsEnabled = 1); the deepest file is $deepestTotal characters")
} else {
    $adminCmd = "New-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name LongPathsEnabled -Value 1 -PropertyType DWORD -Force"
    if ($deepestTotal -ge 260) {
        Add-Result 'WARN' ("long paths are off and the deepest file is $deepestTotal characters (the limit is 259): " + $deepestName)
        Add-Result 'INFO' ("switch long paths on: open PowerShell as administrator, run: $adminCmd  - then restart Windows. Or unzip to a shorter folder, e.g. C:\Cairn")
    } else {
        Add-Result 'INFO' ("long paths are off, but this folder is short enough: the deepest file is $deepestTotal characters (limit 259). A longer folder name may need them: $adminCmd (as administrator)")
    }
}
if ($Here.StartsWith('\\')) { Add-Result 'WARN' 'the folder is on a network share; the database needs a local disk' }
if ($Here -match '[^\x00-\x7F]') { Add-Result 'WARN' 'the folder path has non-ASCII characters (accents, Chinese); if anything fails to start, move Cairn to a plain path such as C:\Cairn' }

# ---------------------------------------------------------------- OneDrive
Write-Host ''
Write-Host '--- OneDrive'
$inOneDrive = $false
foreach ($name in @('OneDrive', 'OneDriveCommercial', 'OneDriveConsumer')) {
    $v = [Environment]::GetEnvironmentVariable($name)
    if ($v -and (Test-Under $Here $v)) { $inOneDrive = $true; Add-Result 'WARN' ("the folder is inside OneDrive ($name): move the Cairn folder outside OneDrive (for example C:\Users\<you>\Cairn). OneDrive syncs and locks the database files.") }
}
if (-not $inOneDrive -and $Here -match '\\OneDrive[^\\]*\\') {
    $inOneDrive = $true
    Add-Result 'WARN' 'the folder path contains a OneDrive folder: move the Cairn folder outside OneDrive (for example C:\Users\<you>\Cairn)'
}
if (-not $inOneDrive) { Add-Result 'PASS' 'the folder is not inside OneDrive' }

# ---------------------------------------------------------------- WebView2, fonts
Write-Host ''
Write-Host '--- WebView2 and fonts'
$wv = $null
$guid = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
foreach ($key in @("HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$guid", "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$guid", "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$guid")) {
    if (-not $wv) {
        try { $pv = (Get-ItemProperty -Path $key -Name pv -ErrorAction Stop).pv; if ($pv -and $pv -ne '0.0.0.0') { $wv = $pv } } catch { }
    }
}
if ($wv) { Add-Result 'PASS' ("WebView2 runtime $wv") }
else { Add-Result 'FAIL' 'the WebView2 runtime is not installed (Cairn cannot show its window; install the Evergreen runtime from Microsoft)' }

$yahei = $false
if (Test-Path -LiteralPath (Join-Path $env:windir 'Fonts\msyh.ttc')) { $yahei = $true }
if (-not $yahei) {
    try {
        $fonts = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts' -ErrorAction Stop
        if ($fonts.PSObject.Properties | Where-Object { $_.Name -like 'Microsoft YaHei*' }) { $yahei = $true }
    } catch { }
}
if ($yahei) { Add-Result 'PASS' 'the font Microsoft YaHei is installed' }
else { Add-Result 'WARN' 'the font Microsoft YaHei is not installed; Chinese text will use another font' }

# ---------------------------------------------------------------- Smart App Control, downloaded-file mark
Write-Host ''
Write-Host '--- Smart App Control and the downloaded-file mark'
$sac = $null
try { $sac = (Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy' -Name VerifiedAndReputablePolicyState -ErrorAction Stop).VerifiedAndReputablePolicyState } catch { }
if ($sac -eq 0) { Add-Result 'PASS' 'Smart App Control is off' }
elseif ($sac -eq 1) { Add-Result 'FAIL' 'Smart App Control is ON: it blocks programs that are not signed, and "Run anyway" does not help. Turn it off in Windows Security > App and browser control > Smart App Control (it cannot be switched on again without reinstalling Windows)' }
elseif ($sac -eq 2) { Add-Result 'WARN' 'Smart App Control is in evaluation mode: it may block cairn.exe (not signed). If it does, turn it off in Windows Security > App and browser control > Smart App Control' }
else { Add-Result 'INFO' 'Smart App Control: not reported on this Windows' }

$marked = @()
$sample = @('cairn.exe', 'sidecar\python\python.exe', 'sidecar\python\python312.dll', 'ui-check.js', 'check-windows.ps1', 'NOTICE', 'README.txt')
if (-not $lite) { $sample += @('models\embeddinggemma-300m\onnx\model_q4.onnx_data') }
$sample += @($files | Where-Object { $_.Extension -eq '.pyd' } | Select-Object -First 5 | ForEach-Object { $_.FullName.Substring($prefixLen) })
foreach ($rel in $sample) {
    $path = '\\?\' + (Join-Path $Here $rel)
    if (Test-Path -LiteralPath $path) {
        $zone = $null
        try { $zone = Get-Item -LiteralPath $path -Stream 'Zone.Identifier' -ErrorAction SilentlyContinue } catch { }
        if ($zone) { $marked += $rel }
    }
}
if ($marked.Count -eq 0) { Add-Result 'PASS' ('no downloaded-file mark (Zone.Identifier) on the ' + $sample.Count + ' files checked') }
else {
    Add-Result 'WARN' ('{0} of {1} files checked carry the downloaded-file mark (e.g. {2}); Windows may warn or Defender may scan harder' -f $marked.Count, $sample.Count, $marked[0])
    Add-Result 'INFO' ("clear it: Get-ChildItem -LiteralPath '$Here' -Recurse -Force | Unblock-File   (better: right-click the zip > Properties > Unblock, before unzipping)")
}

# ---------------------------------------------------------------- self-test
Write-Host ''
Write-Host '--- self-test (cairn.exe --self-test, twice)'
$selfOk = $false
if (-not $haveExe) {
    Add-Result 'FAIL' 'cairn.exe is missing: the self-test and the UI check were not run'
} else {
    $times = @()
    for ($n = 1; $n -le 2; $n++) {
        $out = Join-Path $Tmp "selftest$n.json"
        $dataDir = Join-Path $Tmp "data-selftest$n"
        Set-EnvVars @{ CAIRN_SELF_TEST_OUT = $out; CAIRN_DATA_DIR = $dataDir }
        $run = $null
        try { $run = Invoke-Timed $Exe @('--self-test') 300 } catch { Add-Result 'FAIL' ("self-test $n could not start cairn.exe: $($_.Exception.Message)") }
        Set-EnvVars @{ CAIRN_SELF_TEST_OUT = $null; CAIRN_DATA_DIR = $null }
        if (-not $run) { continue }
        $label = if ($n -eq 1) { 'first run (Defender scans the files)' } else { 'second run' }
        $times += $run.Ms
        $lines = Read-Lines $out
        if ($run.TimedOut) { Add-Result 'FAIL' ("self-test $n ($label) did not finish within 300 s and was stopped"); continue }
        if ($lines.Count -eq 0) { Add-Result 'FAIL' ("self-test $n ($label): exit code $($run.ExitCode), and no result file was written (CAIRN_SELF_TEST_OUT)"); continue }
        $json = $null
        try { $json = $lines[$lines.Count - 1] | ConvertFrom-Json } catch { }
        if (-not $json) { Add-Result 'FAIL' ("self-test $n ($label): the result is not JSON: " + $lines[$lines.Count - 1]); continue }
        if ($run.ExitCode -eq 0 -and $json.ok -eq $true) {
            Add-Result 'PASS' ("self-test $n ($label): ok in " + (Format-Seconds $run.Ms))
            if ($n -eq 1) { $selfOk = $true }
        } else {
            Add-Result 'FAIL' ("self-test $n ($label): exit code $($run.ExitCode), in " + (Format-Seconds $run.Ms))
        }
        Add-Result 'INFO' ("self-test $n result: " + $lines[$lines.Count - 1])
        if ($json.python_path -and -not (Test-Under ([string]$json.python_path) $Here)) {
            Add-Result 'WARN' ('the self-test used a Python outside this folder: ' + $json.python_path)
        }
    }
    if ($times.Count -eq 2) {
        if ($times[1] -gt 30000) { Add-Result 'WARN' ('the second self-test took ' + (Format-Seconds $times[1]) + ' (more than 30 s)') }
        Add-Result 'INFO' ('timing: first ' + (Format-Seconds $times[0]) + ', second ' + (Format-Seconds $times[1]))
    }
}

# ---------------------------------------------------------------- Credential Manager
Write-Host ''
Write-Host '--- Credential Manager (a throw-away entry, deleted at once)'
$credType = @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CairnCheckCred {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct CREDENTIAL {
        public UInt32 Flags;
        public UInt32 Type;
        public string TargetName;
        public string Comment;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
        public UInt32 CredentialBlobSize;
        public IntPtr CredentialBlob;
        public UInt32 Persist;
        public UInt32 AttributeCount;
        public IntPtr Attributes;
        public string TargetAlias;
        public string UserName;
    }
    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool CredWriteW(ref CREDENTIAL credential, UInt32 flags);
    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool CredReadW(string target, UInt32 type, UInt32 flags, out IntPtr credential);
    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool CredDeleteW(string target, UInt32 type, UInt32 flags);
    [DllImport("advapi32.dll")]
    static extern void CredFree(IntPtr buffer);

    // generic credential (type 1), kept on this computer (persist 2): what the keyring crate writes
    public static int Write(string target, string user, string secret) {
        byte[] blob = Encoding.UTF8.GetBytes(secret);
        IntPtr mem = Marshal.AllocHGlobal(blob.Length);
        try {
            Marshal.Copy(blob, 0, mem, blob.Length);
            CREDENTIAL c = new CREDENTIAL();
            c.Type = 1;
            c.TargetName = target;
            c.UserName = user;
            c.CredentialBlobSize = (UInt32)blob.Length;
            c.CredentialBlob = mem;
            c.Persist = 2;
            if (CredWriteW(ref c, 0)) return 0;
            return Marshal.GetLastWin32Error();
        } finally {
            Marshal.FreeHGlobal(mem);
        }
    }
    public static string Read(string target, out int error) {
        IntPtr p;
        error = 0;
        if (!CredReadW(target, 1, 0, out p)) { error = Marshal.GetLastWin32Error(); return null; }
        try {
            CREDENTIAL c = (CREDENTIAL)Marshal.PtrToStructure(p, typeof(CREDENTIAL));
            byte[] blob = new byte[c.CredentialBlobSize];
            if (c.CredentialBlobSize > 0) Marshal.Copy(c.CredentialBlob, blob, 0, blob.Length);
            return Encoding.UTF8.GetString(blob);
        } finally {
            CredFree(p);
        }
    }
    public static int Delete(string target) {
        if (CredDeleteW(target, 1, 0)) return 0;
        return Marshal.GetLastWin32Error();
    }
}
'@
$credReady = $false
try {
    if (-not ('CairnCheckCred' -as [type])) { Add-Type -TypeDefinition $credType -ErrorAction Stop }
    $credReady = $true
} catch { Add-Result 'WARN' ("could not set up the Credential Manager test (PowerShell language mode?): $($_.Exception.Message)") }
if ($credReady) {
    $target = 'cairn-check-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
    $secret = 'check-' + [guid]::NewGuid().ToString('N')
    $written = $false
    try {
        $err = [CairnCheckCred]::Write($target, 'cairn-check', $secret)
        if ($err -eq 0) { $written = $true; Add-Result 'PASS' 'Credential Manager: saved a test entry' }
        else { Add-Result 'FAIL' "Credential Manager: could not save an entry (Windows error $err)" }
        if ($written) {
            $rerr = 0
            $back = [CairnCheckCred]::Read($target, [ref]$rerr)
            if ($back -eq $secret) { Add-Result 'PASS' 'Credential Manager: read the entry back' }
            else { Add-Result 'FAIL' "Credential Manager: could not read the entry back (Windows error $rerr)" }
        }
    } catch { Add-Result 'FAIL' ("Credential Manager: $($_.Exception.Message)") }
    finally {
        if ($written) {
            $derr = [CairnCheckCred]::Delete($target)
            if ($derr -eq 0) { Add-Result 'PASS' 'Credential Manager: deleted the test entry' }
            else { Add-Result 'FAIL' "Credential Manager: could not delete the test entry '$target' (Windows error $derr); remove it in Control Panel > Credential Manager" }
        }
    }
}

# ---------------------------------------------------------------- az
Write-Host ''
Write-Host '--- Azure CLI (for the Assistant with Azure sign-in)'
$az = Get-Command az -ErrorAction SilentlyContinue
if (-not $az) {
    Add-Result 'WARN' 'az (Azure CLI) was not found on the PATH; Azure sign-in in Cairn needs it (install it, then run: az login)'
} else {
    Add-Result 'PASS' ('az found: ' + $az.Source)
    $ok = $false
    try {
        & az account show --output none 2>$null
        $ok = ($LASTEXITCODE -eq 0)
    } catch { $ok = $false }
    if ($ok) { Add-Result 'PASS' 'az is signed in (az account show succeeded)' }
    else { Add-Result 'WARN' 'az is not signed in (az account show failed); run: az login' }
}

# ===================================================================================================
# BEGIN CHECKS OF AGENT B (Windows behaviour in the Rust code: data folder, sidecar start, process handling)
# Helpers you can use: Add-Result 'PASS'|'WARN'|'FAIL'|'INFO' 'text'; $Here (the Cairn folder), $Exe (cairn.exe),
# $Tmp (the throw-away folder, deleted at the end), Invoke-Timed, Read-Lines, Set-EnvVars @{ NAME = value }.
# ===================================================================================================
Write-Host ''
Write-Host '--- Windows behaviour (where Python, the model and the data go; the AI workers)'

# 1. The self-test used the Python and the model inside this folder, and wrote a clean line. A lite zip's
#    self-test reports no model: model_dir, dims and embed_ms are null (D-82).
$stFile = Join-Path $Tmp 'selftest2.json'
if (-not (Test-Path -LiteralPath $stFile)) { $stFile = Join-Path $Tmp 'selftest1.json' }
if (Test-Path -LiteralPath $stFile) {
    $stRaw = [IO.File]::ReadAllText($stFile)
    $st = $null
    try { $st = (($stRaw.Trim() -split "`n")[-1]) | ConvertFrom-Json } catch { }
    if ($st) {
        $py = ([string]$st.python_path).Replace('/', '\')
        $md = ([string]$st.model_dir).Replace('/', '\')
        if (($py -like '*\sidecar\python\python.exe') -and (Test-Under $py $Here)) { Add-Result 'PASS' 'Python is the bundled one (sidecar\python\python.exe)' }
        else { Add-Result 'FAIL' ('Python is not the bundled one: ' + $py) }
        if ($lite) {
            if (($null -eq $st.model_dir) -and ($null -eq $st.dims) -and ($null -eq $st.embed_ms)) { Add-Result 'PASS' 'lite: the self-test reports no search model, as expected' }
            else { Add-Result 'FAIL' ('lite: the self-test should report no search model, but reports ' + $md + ', ' + $st.dims + ' numbers') }
        } else {
            if (($md -like '*\models\embeddinggemma-300m') -and (Test-Under $md $Here)) { Add-Result 'PASS' 'the search model is the bundled one (models\embeddinggemma-300m)' }
            else { Add-Result 'FAIL' ('the search model is not the bundled one: ' + $md) }
            if ($st.dims -eq 256) { Add-Result 'PASS' 'a search vector has 256 numbers' }
            else { Add-Result 'FAIL' ("a search vector has $($st.dims) numbers, expected 256") }
        }
        if ($stRaw.TrimEnd("`r", "`n").Contains("`r")) { Add-Result 'WARN' 'the self-test line contains a carriage return' }
    } else { Add-Result 'FAIL' 'the self-test result could not be read as JSON' }
} else { Add-Result 'INFO' 'no self-test result to look at (see the self-test above)' }

# 2. A normal end leaves no AI worker behind: the self-tests above started the workers and ended.
$left = @(Get-CimInstance Win32_Process -Filter "Name = 'python.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.ExecutablePath -and (Test-Under $_.ExecutablePath $Here) })
if ($left.Count -eq 0) { Add-Result 'PASS' 'no AI worker is left running after the self-tests ended' }
else { Add-Result 'FAIL' ("$($left.Count) AI worker(s) from this folder are still running after the self-tests (is Cairn open? close it before the check)") }

# 3. The portable data folder (D-56): a copy of cairn.exe in the throw-away folder, started without
#    CAIRN_DATA_DIR, must create data\ with the database, logs and webview storage next to itself,
#    and nothing in the user profile.
if ($haveExe) {
    $port = Join-Path $Tmp 'portable'
    $null = New-Item -ItemType Directory -Path $port -Force
    Copy-Item -LiteralPath $Exe -Destination (Join-Path $port 'cairn.exe')
    $watch = @(
        (Join-Path $env:APPDATA 'cairn'), (Join-Path $env:APPDATA 'app.cairn.desktop'),
        (Join-Path $env:LOCALAPPDATA 'cairn'), (Join-Path $env:LOCALAPPDATA 'app.cairn.desktop')
    )
    $before = @{}
    foreach ($w in $watch) { $before[$w] = Test-Path -LiteralPath $w }
    Set-EnvVars @{ CAIRN_AUTOMATION = '1'; CAIRN_EXIT_AFTER_SECS = '20' }
    $run = $null
    try { $run = Invoke-Timed (Join-Path $port 'cairn.exe') @() 90 } catch { Add-Result 'FAIL' ("portable: could not start the copy of cairn.exe: $($_.Exception.Message)") }
    Set-EnvVars @{ CAIRN_AUTOMATION = $null; CAIRN_EXIT_AFTER_SECS = $null }
    if ($run) {
        $pd = Join-Path $port 'data'
        foreach ($item in @('cairn.sqlite', 'logs', 'webview')) {
            if (Test-Path -LiteralPath (Join-Path $pd $item)) { Add-Result 'PASS' ("portable: data\$item was created next to cairn.exe") }
            else { Add-Result 'FAIL' ("portable: data\$item was not created next to cairn.exe") }
        }
        foreach ($w in $watch) {
            if ($before[$w]) { Add-Result 'INFO' ("portable: $w already existed before the check, so it was not judged") }
            elseif (Test-Path -LiteralPath $w) { Add-Result 'FAIL' ("portable: Cairn created $w outside its own folder") }
        }
        if ($run.TimedOut) { Add-Result 'WARN' 'portable: the copy did not close by itself within 90 s and was stopped' }
    }
}

# 4. The AI workers have no console window, and end with Cairn even when Cairn is killed (the Job
#    Object). A search by meaning starts the embed worker; then cairn.exe is stopped by force.
#    A lite zip has no model, so the search may start no worker: that is noted, not failed (D-82).
if ($haveExe -and $selfOk) {
    $jobJs = Join-Path $Tmp 'worker-check.js'
    $jobOut = Join-Path $Tmp 'worker-check.out'
    [IO.File]::WriteAllText($jobJs, @'
(async () => {
  const inv = window.__TAURI_INTERNALS__.invoke
  const rep = (l) => inv('automation_report', { line: l }).catch(() => {})
  try {
    await inv('search_hybrid', { query: { text: 'pump maintenance check' } })
    await rep('WORKERS STARTED')
  } catch (e) {
    await rep('FAIL search by meaning: ' + JSON.stringify(e))
  }
})()
'@)
    Set-EnvVars @{
        CAIRN_AUTOMATION = '1'; CAIRN_DATA_DIR = (Join-Path $Tmp 'data-workers'); CAIRN_DEV_SCRIPT_FILE = $jobJs
        CAIRN_DEV_SCRIPT_AFTER = 'load'; CAIRN_AUTOMATION_OUT = $jobOut; CAIRN_EXIT_AFTER_SECS = '180'
    }
    $app = $null
    try { $app = Start-Process -FilePath $Exe -WorkingDirectory $Here -PassThru } catch { Add-Result 'FAIL' ("workers: could not start cairn.exe: $($_.Exception.Message)") }
    Set-EnvVars @{ CAIRN_AUTOMATION = $null; CAIRN_DATA_DIR = $null; CAIRN_DEV_SCRIPT_FILE = $null; CAIRN_DEV_SCRIPT_AFTER = $null; CAIRN_AUTOMATION_OUT = $null; CAIRN_EXIT_AFTER_SECS = $null }
    if ($app) {
        $started = $false
        for ($i = 0; $i -lt 150; $i++) {
            $got = Read-Lines $jobOut
            if ($got -contains 'WORKERS STARTED') { $started = $true; break }
            if (@($got | Where-Object { $_ -like 'FAIL*' }).Count -gt 0) { break }
            if ($app.HasExited) { break }
            Start-Sleep -Seconds 1
        }
        $workers = @(Get-CimInstance Win32_Process -Filter "Name = 'python.exe'" -ErrorAction SilentlyContinue |
            Where-Object { $_.ParentProcessId -eq $app.Id })
        if ($lite -and ((-not $started) -or ($workers.Count -eq 0))) {
            Add-Result 'INFO' ('workers: lite: the search started no AI worker without the search model, so the Job Object check was not run (the full zip runs it): ' + ((Read-Lines $jobOut) -join ' | '))
        } elseif (-not $started) {
            Add-Result 'FAIL' ('workers: the search by meaning did not finish: ' + ((Read-Lines $jobOut) -join ' | '))
        } elseif ($workers.Count -eq 0) {
            Add-Result 'FAIL' 'workers: no AI worker was found under cairn.exe'
        } else {
            Add-Result 'PASS' ("workers: $($workers.Count) AI worker(s) running under cairn.exe")
            $ids = @($workers | ForEach-Object { $_.ProcessId })
            # A console window belongs to a conhost.exe started for the worker.
            $consoles = @(Get-CimInstance Win32_Process -Filter "Name = 'conhost.exe'" -ErrorAction SilentlyContinue |
                Where-Object { $ids -contains $_.ParentProcessId })
            if ($consoles.Count -eq 0) { Add-Result 'PASS' 'workers: no console window (no conhost.exe under the workers)' }
            else { Add-Result 'FAIL' "workers: $($consoles.Count) console window(s) belong to the AI workers" }
            try { Stop-Process -Id $app.Id -Force -ErrorAction Stop } catch { }
            $still = $ids
            for ($i = 0; $i -lt 10; $i++) {
                Start-Sleep -Milliseconds 500
                $still = @($ids | Where-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
                if ($still.Count -eq 0) { break }
            }
            if ($still.Count -eq 0) { Add-Result 'PASS' 'workers: they ended when Cairn was killed (Job Object)' }
            else {
                Add-Result 'FAIL' "workers: $($still.Count) AI worker(s) were still running 5 s after Cairn was killed; stopping them"
                foreach ($id in $still) { try { Stop-Process -Id $id -Force -ErrorAction Stop } catch { } }
            }
        }
        if (-not $app.HasExited) { try { Stop-Process -Id $app.Id -Force -ErrorAction Stop } catch { } }
    }
} elseif ($haveExe) {
    Add-Result 'INFO' 'workers: not checked, because the first self-test failed'
}

# ===================================================================================================
# END CHECKS OF AGENT B
# ===================================================================================================

# ---------------------------------------------------------------- scripted UI check
Write-Host ''
Write-Host '--- scripted UI check (a Cairn window opens for a few seconds; do not click in it)'
if ($SkipUi) {
    Add-Result 'INFO' 'skipped (-SkipUi)'
} elseif (-not $haveExe) {
    Add-Result 'FAIL' 'cairn.exe is missing: no UI check'
} elseif (-not (Test-Path -LiteralPath (Join-Path $Here 'ui-check.js'))) {
    Add-Result 'FAIL' 'ui-check.js is missing: no UI check'
} else {
    $uiOut = Join-Path $Tmp 'ui-check.out'
    $uiData = Join-Path $Tmp 'data-ui'
    Set-EnvVars @{
        CAIRN_AUTOMATION = '1'
        CAIRN_DATA_DIR = $uiData
        CAIRN_DEV_SCRIPT_FILE = (Join-Path $Here 'ui-check.js')
        CAIRN_DEV_SCRIPT_AFTER = 'load'
        CAIRN_AUTOMATION_OUT = $uiOut
        CAIRN_EXIT_AFTER_SECS = '120'
    }
    $run = $null
    try { $run = Invoke-Timed $Exe @() 180 } catch { Add-Result 'FAIL' ("could not start cairn.exe: $($_.Exception.Message)") }
    Set-EnvVars @{ CAIRN_AUTOMATION = $null; CAIRN_DATA_DIR = $null; CAIRN_DEV_SCRIPT_FILE = $null; CAIRN_DEV_SCRIPT_AFTER = $null; CAIRN_AUTOMATION_OUT = $null; CAIRN_EXIT_AFTER_SECS = $null }
    if ($run) {
        $lines = Read-Lines $uiOut
        $done = $false
        foreach ($l in $lines) {
            if ($l -eq 'DONE') { $done = $true; continue }
            if ($l -match '^(PASS|FAIL|INFO|WARN)\s+(.*)$') { Add-Result $Matches[1] ('ui: ' + $Matches[2]) }
            else { Add-Result 'INFO' ('ui: ' + $l) }
        }
        if ($lines.Count -eq 0) { Add-Result 'FAIL' "ui: the window reported nothing (exit code $($run.ExitCode)); the script did not run, or the window did not open" }
        elseif (-not $done) { Add-Result 'FAIL' 'ui: the script did not reach its end (DONE)' }
        if ($run.TimedOut) { Add-Result 'WARN' 'ui: Cairn was still open after 180 s and was stopped' }
        Add-Result 'INFO' ('ui check took ' + (Format-Seconds $run.Ms) + ', exit code ' + $run.ExitCode)
    }
}

# ---------------------------------------------------------------- after
Write-Host ''
Write-Host '--- afterwards'
if ((-not $haveDataBefore) -and (Test-Path -LiteralPath (Join-Path $Here 'data'))) {
    Add-Result 'WARN' 'the checks created a data folder inside the Cairn folder (CAIRN_DATA_DIR was not used?); delete it before real use'
}
for ($i = 0; $i -lt 5; $i++) {
    try { Remove-Item -LiteralPath $Tmp -Recurse -Force -ErrorAction Stop; break } catch { Start-Sleep -Seconds 1 }
}
if (Test-Path -LiteralPath $Tmp) { Add-Result 'INFO' ("could not delete the throw-away folder $Tmp; you can delete it") }
else { Add-Result 'INFO' 'the throw-away folder in TEMP was deleted' }

Write-Host ''
Write-Host '=============================================================='
Write-Host ('SUMMARY: {0} PASS, {1} WARN, {2} FAIL' -f $script:Counts['PASS'], $script:Counts['WARN'], $script:Counts['FAIL'])
Write-Host 'Paste everything above back (select all in this window, then copy).'
Write-Host '=============================================================='
if ($script:Counts['FAIL'] -gt 0) { exit 1 } else { exit 0 }
