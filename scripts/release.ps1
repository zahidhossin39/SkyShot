<#
Release Shutter: bump the version, check, build the signed installer, write latest.json,
commit, push and publish the GitHub release that installed copies update from.

  powershell -File scripts\release.ps1 -Version 0.2.2 -Notes "One-line summary" -NotesFile notes.md
  add -DryRun to stop after the build (nothing committed, pushed or published)

latest.json is what every installed copy reads to update. A wrong version, url or signature
breaks updates for everyone, so this script builds it from the files it just made and checks
it is served after publishing.
#>
param(
  [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
  [Parameter(Mandatory)][string]$Notes,      # one line, shown in the in-app update
  [Parameter(Mandatory)][string]$NotesFile,  # Markdown body of the GitHub release
  [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
$Repo = 'zahidhossin39/shutter'
$Root = Split-Path $PSScriptRoot -Parent
Set-Location $Root
$Utf8 = New-Object Text.UTF8Encoding $false   # no BOM: serde and cargo reject one
function Run($cmd) { Write-Host "> $cmd"; cmd /c $cmd; if ($LASTEXITCODE) { throw "failed: $cmd" } }
function Edit($path, $pattern, $replacement) {
  $p = Join-Path $Root $path
  $text = [IO.File]::ReadAllText($p)
  $new = [regex]::new($pattern, 'Multiline').Replace($text, $replacement, 1)
  if ($new -eq $text) { throw "version not found in $path" }
  [IO.File]::WriteAllText($p, $new, $Utf8)
}

# --- preconditions
$keyPath = Join-Path $HOME '.tauri\shutter.key'
if (-not (Test-Path $keyPath)) { throw "signing key missing: $keyPath (without it installed copies can't update)" }
if (-not (Test-Path $NotesFile)) { throw "notes file missing: $NotesFile" }
if ((git rev-parse --abbrev-ref HEAD) -ne 'main') { throw 'release from main' }
if (git status --porcelain) { throw 'working tree not clean: commit first' }
if (git tag -l "v$Version") { throw "tag v$Version already exists" }

# --- version bump (all five places)
Edit 'src-tauri/tauri.conf.json' '"version": "\d+\.\d+\.\d+"' "`"version`": `"$Version`""
Edit 'src-tauri/Cargo.toml' '^version = "\d+\.\d+\.\d+"' "version = `"$Version`""
Edit 'src-tauri/Cargo.lock' '(name = "screenshot-app"\r?\nversion = )"\d+\.\d+\.\d+"' "`${1}`"$Version`""
Run "npm version $Version --no-git-tag-version --allow-same-version"   # package.json + lock

# --- check, then build signed
Run 'npm run check'
$env:TAURI_SIGNING_PRIVATE_KEY = [IO.File]::ReadAllText($keyPath)
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''
Run 'npm run tauri build'
$exe = "src-tauri\target\release\bundle\nsis\Shutter_${Version}_x64-setup.exe"
$sig = "$exe.sig"
if (-not (Test-Path $exe) -or -not (Test-Path $sig)) { throw "build did not produce $exe and .sig" }

# --- latest.json from the files just built
$latest = [ordered]@{
  version  = $Version
  notes    = $Notes
  pub_date = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
  platforms = @{ 'windows-x86_64' = [ordered]@{
    signature = [IO.File]::ReadAllText((Join-Path $Root $sig)).Trim()
    url       = "https://github.com/$Repo/releases/download/v$Version/Shutter_${Version}_x64-setup.exe"
  } }
}
$latestPath = Join-Path $Root 'src-tauri\target\release\bundle\nsis\latest.json'
[IO.File]::WriteAllText($latestPath, ($latest | ConvertTo-Json -Depth 5), $Utf8)
Write-Host "built $exe ($([math]::Round((Get-Item $exe).Length / 1MB, 2)) MB) and latest.json"

if ($DryRun) {
  git checkout -- package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
  Write-Host 'dry run: version bump reverted, nothing committed, pushed or published'
  exit 0
}

# --- commit, push, publish
Run 'git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json'
Run "git commit -q -m `"Version $Version`""
Run 'git push origin main'
Run "gh release create v$Version `"$exe`" `"$sig`" `"$latestPath`" --repo $Repo --target main --title `"Shutter $Version`" --notes-file `"$NotesFile`""

# --- the updater reads this exact url; make sure it now says this version
Start-Sleep 3
$served = Invoke-RestMethod "https://github.com/$Repo/releases/latest/download/latest.json"
if ($served.version -ne $Version) { throw "latest.json is serving $($served.version), not $Version" }
Write-Host "released v$Version: https://github.com/$Repo/releases/tag/v$Version"
