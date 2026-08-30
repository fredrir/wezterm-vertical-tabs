# Locates the wez-vtabs backend: explicit path, cached download, GitHub release, or cargo build.
$ErrorActionPreference = "Stop"
$name = "wez-vtabs"
$data = Join-Path $env:LOCALAPPDATA "wez-vtabs"
$target = if ($env:VTABS_TARGET) { $env:VTABS_TARGET } else { "x86_64-pc-windows-msvc" }
$version = if ($env:VTABS_VERSION) { $env:VTABS_VERSION } else { "dev" }

if ($env:VTABS_BIN -and (Test-Path $env:VTABS_BIN)) { & $env:VTABS_BIN; exit $LASTEXITCODE }

$bin = Join-Path $data "bin\$name-$target-$version.exe"
if (Test-Path $bin) { & $bin; exit $LASTEXITCODE }
New-Item -ItemType Directory -Force -Path (Split-Path $bin) | Out-Null

if ($version -ne "dev" -and $env:VTABS_REPO) {
  $url = "https://github.com/$($env:VTABS_REPO)/releases/download/v$version/$name-$target.exe"
  Write-Host "downloading $url"
  try {
    Invoke-WebRequest -Uri $url -OutFile "$bin.tmp"
    Move-Item -Force "$bin.tmp" $bin
    & $bin; exit $LASTEXITCODE
  } catch { Write-Host "download failed" }
}

if ($env:VTABS_BUILD -ne "0" -and $env:VTABS_SRC -and (Get-Command cargo -ErrorAction SilentlyContinue)) {
  Write-Host "building backend"
  cargo build --release --manifest-path (Join-Path $env:VTABS_SRC "Cargo.toml") --target-dir (Join-Path $data "target")
  if ($LASTEXITCODE -eq 0) {
    Copy-Item (Join-Path $data "target\release\$name.exe") $bin
    & $bin; exit $LASTEXITCODE
  }
  Write-Host "build failed"
}

Write-Host "backend not found`ninstall cargo or set backend.path"
while ($true) { Start-Sleep -Seconds 3600 }
