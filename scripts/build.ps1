param([ValidateSet('build','check','test','run')][string]$Action = 'build', [switch]$Debug, [string[]]$RunArgs = @())
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$ws = [IO.Path]::GetFullPath((Join-Path $root '..'))
# windres (Windows resources: icon, version info) fails on paths with spaces, such as
# "Neramit Cut": build through a space-free junction instead.
if ($root.Contains(' ')) {
    $links = Join-Path $ws '.toolchains\links'
    $link = Join-Path $links ((Split-Path $root -Leaf) -replace ' ', '')
    if (!(Test-Path $link)) {
        New-Item -ItemType Directory -Force $links | Out-Null
        New-Item -ItemType Junction -Path $link -Target $root | Out-Null
    }
    $root = $link
}
$env:CARGO_HOME = Join-Path $ws '.toolchains\rust\cargo'
$env:RUSTUP_HOME = Join-Path $ws '.toolchains\rust\rustup'
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu'
$bin = Join-Path $env:CARGO_HOME 'bin'
$tools = Join-Path $ws '.toolchains\binutils'
$env:PATH = "$bin;$(Join-Path $tools 'mingw64\bin');$env:PATH"
$env:WINDRES = Join-Path $tools 'windres-wrapper.exe'
$env:AR = Join-Path $tools 'mingw64\bin\ar.exe'
$sysroot = (& (Join-Path $bin 'rustc.exe') --print sysroot).Trim()
$env:DLLTOOL = Join-Path $sysroot 'lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained\dlltool.exe'
$cargoAction = if ($Action -eq 'run') { 'build' } else { $Action }
$a = @($cargoAction, '--manifest-path', (Join-Path $root 'Cargo.toml'), '--offline')
if (!$Debug -and $Action -ne 'check') { $a += '--release' }
$ErrorActionPreference = 'Continue'
& (Join-Path $bin 'cargo.exe') @a 2>&1 | ForEach-Object { "$_" }
$code = $LASTEXITCODE
$ErrorActionPreference = 'Stop'
if ($code -ne 0) { Write-Output "cargo $Action failed ($code)"; exit $code }
