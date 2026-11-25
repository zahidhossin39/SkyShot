<#
Release Shutter on every platform: bump the version, check, commit, tag and push. GitHub
Actions (.github/workflows/release.yml) then builds Windows, macOS and Linux into a DRAFT
release with one latest.json; this script waits for it, checks it, and publishes.

  powershell -File scripts\release.ps1 -Version 0.3.0 -Notes "One-line summary" -NotesFile notes.md
  add -DryRun to stop after the checks (nothing committed, pushed or published)
  test build without releasing: gh workflow run release.yml   (installers land as run artifacts)

latest.json is what every installed copy reads to update. A wrong version, url or signature
breaks updates for everyone, so the draft is checked before it goes public and the served
file is checked after.

Needs the repo secret TAURI_SIGNING_PRIVATE_KEY = contents of ~/.tauri/shutter.key (the key
every earlier release was signed with).
#>
param(
  [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
  [Parameter(Mandatory)][string]$Notes,      # one line, shown in the in-app update
  [Parameter(Mandatory)][string]$NotesFile,  # Markdown body of the GitHub release
  [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
$Repo = 'zahidhossin39/shutter'
$Tag = "v$Version"
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
if (-not (Test-Path $NotesFile)) { throw "notes file missing: $NotesFile" }
if ($Notes -match '[\r\n"]') { throw 'Notes must be one line without double quotes' }
if ((git rev-parse --abbrev-ref HEAD) -ne 'main') { throw 'release from main' }
if (git status --porcelain) { throw 'working tree not clean: commit first' }
if (git tag -l $Tag) { throw "tag $Tag already exists" }
if (-not ((gh secret list --repo $Repo) -match '^TAURI_SIGNING_PRIVATE_KEY\s')) {
  throw "repo secret TAURI_SIGNING_PRIVATE_KEY missing (gh secret set TAURI_SIGNING_PRIVATE_KEY --repo $Repo < ~/.tauri/shutter.key)"
}

# --- version bump (all five places)
Edit 'src-tauri/tauri.conf.json' '"version": "\d+\.\d+\.\d+"' "`"version`": `"$Version`""
Edit 'src-tauri/Cargo.toml' '^version = "\d+\.\d+\.\d+"' "version = `"$Version`""
Edit 'src-tauri/Cargo.lock' '(name = "screenshot-app"\r?\nversion = )"\d+\.\d+\.\d+"' "`${1}`"$Version`""
Run "npm version $Version --no-git-tag-version --allow-same-version"   # package.json + lock
Run 'npm run check'

if ($DryRun) {
  git checkout -- package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
  Write-Host 'dry run: checks passed, version bump reverted, nothing committed, pushed or published'
  exit 0
}

# --- commit, tag (its message is the in-app note), push: the tag starts the build
Run 'git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json'
Run "git commit -q -m `"Version $Version`""
Run "git tag -a $Tag -m `"$Notes`""
Run 'git push origin main'
Run "git push origin $Tag"

# --- wait for the build
Write-Host 'waiting for the release build (about 15-25 min)...'
$runId = $null
for ($i = 0; $i -lt 30 -and -not $runId; $i++) {
  Start-Sleep 10
  $runId = gh run list --repo $Repo --workflow release.yml --branch $Tag --limit 1 --json databaseId -q '.[0].databaseId'
}
if (-not $runId) { throw "no release build started for $Tag" }
Run "gh run watch $runId --repo $Repo --exit-status --interval 60"

# --- check the draft before anyone can update from it
$tmp = Join-Path ([IO.Path]::GetTempPath()) "shutter-$Tag"
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
Run "gh release download $Tag --repo $Repo --pattern latest.json --dir `"$tmp`""
$latest = Get-Content (Join-Path $tmp 'latest.json') -Raw | ConvertFrom-Json
if ($latest.version -ne $Version) { throw "draft latest.json says $($latest.version), not $Version" }
foreach ($p in 'windows-x86_64', 'darwin-aarch64', 'darwin-x86_64', 'linux-x86_64') {
  $entry = $latest.platforms.$p
  if (-not $entry -or -not $entry.signature -or $entry.url -notmatch "/download/$Tag/") { throw "latest.json: $p missing or wrong" }
}
Write-Host 'draft latest.json covers Windows, macOS (both chips) and Linux'

# --- publish, then make sure the updater url serves this version
Run "gh release edit $Tag --repo $Repo --notes-file `"$NotesFile`" --draft=false --latest"
Start-Sleep 5
$served = Invoke-RestMethod "https://github.com/$Repo/releases/latest/download/latest.json"
if ($served.version -ne $Version) { throw "latest.json is serving $($served.version), not $Version" }
Write-Host "released ${Tag}: https://github.com/$Repo/releases/tag/$Tag"
