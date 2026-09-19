param(
  [string]$DesktopName,
  [switch]$Development
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$previousTarget = $env:CARGO_TARGET_DIR
Push-Location $root
try {
  $conf = Get-Content -Raw 'src-tauri\tauri.conf.json' | ConvertFrom-Json
  $package = Get-Content -Raw 'package.json' | ConvertFrom-Json
  if ($conf.bundle.active) { throw 'Portable exe only: bundle.active must be false' }
  if ($package.version -ne $conf.version) { throw 'Product versions must match' }
  $commit = (git rev-parse --short HEAD).Trim()
  if ($LASTEXITCODE -ne 0) { throw 'Cannot identify source commit' }
  $dirty = [bool](git status --porcelain)
  if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect source status' }
  if ($dirty -and -not $Development) { throw 'Public build requires a clean checkout. Use -Development for a labelled local test build.' }
  if ($dirty) { $commit += '-dirty' }
  if (-not (Test-Path -LiteralPath 'node_modules\@tauri-apps\cli\tauri.js')) {
    throw 'Run npm ci first (build does not install or update dependencies)'
  }
  $env:CARGO_TARGET_DIR = Join-Path $root 'src-tauri\target'
  & node 'node_modules\@tauri-apps\cli\tauri.js' build --ci --no-bundle -- --locked --offline
  if ($LASTEXITCODE -ne 0) { throw 'Build failed; no artifact was copied' }
  # Only the Cargo package target is accepted; never fall back to an old named exe.
  $exe = Join-Path $env:CARGO_TARGET_DIR 'release\doubaoime-darkmode.exe'
  $item = Get-Item -LiteralPath $exe
  $binaryVersion = $item.VersionInfo.ProductVersion
  if ($binaryVersion -ne $conf.version) { throw "Built product version mismatch: $binaryVersion" }
  $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
  $flavor = if ($Development) { 'test' } else { 'release' }
  $name = "DoubaoIME Darkmode-$($conf.version)-$flavor-$commit-$stamp.exe"
  $dist = Join-Path $root 'dist'
  New-Item -ItemType Directory -Force -Path $dist | Out-Null
  $out = Join-Path $dist $name
  [System.IO.File]::Copy($exe, $out, $false)
  if (-not $DesktopName) { $DesktopName = $name }
  if ([System.IO.Path]::GetFileName($DesktopName) -ne $DesktopName -or -not $DesktopName.EndsWith('.exe')) {
    throw 'DesktopName must be a filename ending in .exe'
  }
  $desktop = Join-Path ([Environment]::GetFolderPath('Desktop')) $DesktopName
  # Never overwrite an existing desktop build, including the user's working version.
  [System.IO.File]::Copy($out, $desktop, $false)
  $hash = (Get-FileHash -LiteralPath $out -Algorithm SHA256).Hash
  if ((Get-FileHash -LiteralPath $desktop -Algorithm SHA256).Hash -ne $hash) { throw 'Copy hash mismatch' }
  $metadata = [ordered]@{
    path = $out; desktop = $desktop; version = $conf.version
    commit = $commit; flavor = $flavor; size = $item.Length; sha256 = $hash
    created = (Get-Date).ToString('o')
  }
  $metadata | ConvertTo-Json | Set-Content -LiteralPath ($out + '.json') -Encoding UTF8
  $metadata | ConvertTo-Json
} finally {
  $env:CARGO_TARGET_DIR = $previousTarget
  Pop-Location
}
