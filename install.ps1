# Nexus one-line installer for Windows (works with Smart App Control on):
#   iwr -useb https://raw.githubusercontent.com/GameBitBan/Nexus/main/install.ps1 | iex
# installs Python if needed (winget, per-user), downloads the latest Nexus release from GitHub,
# and runs its installer: Start Menu + Desktop shortcuts, listed in Settings > Apps for uninstalling.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Repo = 'GameBitBan/Nexus'

function Say($t) { Write-Host "  $t" }
Write-Host ""
Write-Host "  nexus installer" -ForegroundColor White
Write-Host ""

function Get-Python {
    $cands = @()
    $py = Get-Command py -ErrorAction SilentlyContinue
    if ($py) { $cands += , @($py.Source, '-3') }
    $p = Get-Command python -ErrorAction SilentlyContinue
    if ($p -and $p.Source -notlike '*WindowsApps*') { $cands += , @($p.Source) }
    $cands += , @("$env:LOCALAPPDATA\Programs\Python\Python313\python.exe")
    $cands += , @("$env:LOCALAPPDATA\Programs\Python\Python312\python.exe")
    foreach ($c in $cands) {
        if (-not (Test-Path $c[0])) { continue }
        try {
            $exe = & $c[0] @($c[1..9]) -c "import sys; print(sys.executable if sys.version_info >= (3, 10) else '')" 2>$null
            if ($exe -and (Test-Path $exe.Trim())) { return $exe.Trim() }
        } catch { }
    }
    return $null
}

$python = Get-Python
if (-not $python) {
    Say "Python 3.10+ not found - installing Python 3.12 for your user (winget)..."
    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        Say "winget isn't available. Install Python from https://www.python.org/downloads/ (tick 'Add python.exe to PATH'), then run this again."
        Start-Process "https://www.python.org/downloads/"
        return
    }
    winget install -e --id Python.Python.3.12 --scope user --silent --accept-package-agreements --accept-source-agreements | Out-Null
    $env:Path = [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + [Environment]::GetEnvironmentVariable('Path', 'Machine')
    $python = Get-Python
    if (-not $python) { Say "Python installed, but this window can't see it yet. Open a new PowerShell window and run the command again."; return }
}
Say "python: $python"

Say "finding the latest Nexus release..."
$rel = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ 'User-Agent' = 'nexus-installer' }
$asset = $rel.assets | Where-Object { $_.name -eq 'nexus.zip' } | Select-Object -First 1
if (-not $asset) { Say "the latest release has no nexus.zip attached."; return }
Say "downloading Nexus $($rel.tag_name)..."
$tmp = Join-Path $env:TEMP ("nexus-install-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
$zip = Join-Path $tmp 'nexus.zip'
Invoke-WebRequest $asset.browser_download_url -OutFile $zip -UseBasicParsing
Expand-Archive $zip -DestinationPath $tmp -Force
Get-ChildItem $tmp -Recurse | Unblock-File

Say "installing (python packages can take a minute the first time)..."
& $python (Join-Path $tmp 'Nexus\Install Nexus.pyw') --yes
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
Write-Host ""
Say "done - Nexus is in your Start Menu and on your Desktop. Right-click the icon > Pin to taskbar."
Write-Host ""
