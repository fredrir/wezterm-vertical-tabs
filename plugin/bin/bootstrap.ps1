param([Parameter(ValueFromRemainingArguments = $true)] $Passthru)
# Locates the wez-vtabs backend: explicit path, cached download, verified GitHub release, or cargo build.
$ErrorActionPreference = "Stop"
$name = "wez-vtabs"
$data = Join-Path $env:LOCALAPPDATA "wez-vtabs"
$target = if ($env:VTABS_TARGET) { $env:VTABS_TARGET } else { "x86_64-pc-windows-msvc" }
$version = if ($env:VTABS_VERSION) { $env:VTABS_VERSION } else { "dev" }
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

if ($target -notmatch '^[A-Za-z0-9._-]+$' -or $version -notmatch '^[A-Za-z0-9._-]+$') {
  Write-Host "invalid VTABS_TARGET or VTABS_VERSION"; exit 1
}
if ($env:VTABS_BIN -and (Test-Path $env:VTABS_BIN)) { & $env:VTABS_BIN; exit $LASTEXITCODE }

$bin = Join-Path $data "bin\$name-$target-$version.exe"
if (Test-Path $bin) { & $bin @Passthru; exit $LASTEXITCODE }
New-Item -ItemType Directory -Force -Path (Split-Path $bin) | Out-Null

if ($version -ne "dev" -and $env:VTABS_REPO) {
  $base = "https://github.com/$($env:VTABS_REPO)/releases/download/v$version"
  $tmp = "$bin.$PID.tmp"
  Write-Host "downloading $base/$name-$target.exe"
  try {
    Invoke-WebRequest -Uri "$base/$name-$target.exe" -OutFile $tmp
    $sums = Invoke-WebRequest -Uri "$base/SHA256SUMS" | Select-Object -ExpandProperty Content
    $expected = ($sums -split "`n" | Where-Object { $_ -match " $name-$target.exe$" }) -replace ' .*', ''
    $actual = (Get-FileHash -Algorithm SHA256 $tmp).Hash.ToLower()
    if ($expected -and $expected.Trim() -eq $actual) {
      Move-Item -Force $tmp $bin
      & $bin @Passthru; exit $LASTEXITCODE
    }
    Write-Host "checksum mismatch"
  } catch { Write-Host "download failed" }
  Remove-Item -Force -ErrorAction SilentlyContinue $tmp
}

if ($env:VTABS_BUILD -ne "0" -and $env:VTABS_SRC -and (Get-Command cargo -ErrorAction SilentlyContinue)) {
  Write-Host "building backend"
  cargo build --release --manifest-path (Join-Path $env:VTABS_SRC "Cargo.toml") --target-dir (Join-Path $data "target")
  if ($LASTEXITCODE -eq 0) {
    Copy-Item (Join-Path $data "target\release\$name.exe") $bin
    & $bin @Passthru; exit $LASTEXITCODE
  }
  Write-Host "build failed"
}

Write-Host "backend not found`ninstall cargo or set backend.path, then press Enter to retry"
Read-Host | Out-Null
& $PSCommandPath @Passthru; exit $LASTEXITCODE
