param([ValidateSet('build','check','test')][string]$Action = 'build')
# Builds installer\ (NeramitCut-Setup.exe). Setup packs target\release\NeramitCut.exe:
# run build.ps1 build first.
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$ws = [IO.Path]::GetFullPath((Join-Path $root '..'))
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
$a = @($Action, '--manifest-path', (Join-Path $root 'installer\Cargo.toml'), '--offline', '--release')
$ErrorActionPreference = 'Continue'
& (Join-Path $bin 'cargo.exe') @a 2>&1 | ForEach-Object { "$_" }
$code = $LASTEXITCODE
$ErrorActionPreference = 'Stop'
if ($code -ne 0) { Write-Output "cargo $Action failed ($code)"; exit $code }
