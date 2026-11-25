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
  [switch]$DryRun,
  [switch]$Resume   # the tag is already pushed (an earlier run stopped): wait, check, publish
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
if ($Resume -and -not (git tag -l $Tag)) { throw "-Resume: tag $Tag doesn't exist" }
if (-not $Resume) {
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
}

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
$latestPath = Join-Path $tmp 'latest.json'
$latest = Get-Content $latestPath -Raw | ConvertFrom-Json
if ($latest.version -ne $Version) { throw "draft latest.json says $($latest.version), not $Version" }
# A draft's latest.json points at API asset urls (download links don't exist until it's
# public). Rewrite them to the normal download links every earlier release used.
$assets = (gh release view $Tag --repo $Repo --json assets | ConvertFrom-Json).assets
$nameOf = @{}; foreach ($a in $assets) { $nameOf[$a.apiUrl] = $a.name }
foreach ($prop in $latest.platforms.PSObject.Properties) {
  $name = $nameOf[$prop.Value.url]
  if ($name) { $prop.Value.url = "https://github.com/$Repo/releases/download/$Tag/$name" }
}
[IO.File]::WriteAllText($latestPath, ($latest | ConvertTo-Json -Depth 5), $Utf8)
foreach ($p in 'windows-x86_64', 'darwin-aarch64', 'darwin-x86_64', 'linux-x86_64') {
  $entry = $latest.platforms.$p
  if (-not $entry -or -not $entry.signature -or $entry.url -notmatch "/download/$Tag/") { throw "latest.json: $p missing or wrong" }
}
Run "gh release upload $Tag `"$latestPath`" --repo $Repo --clobber"
Write-Host 'draft latest.json covers Windows, macOS (both chips) and Linux, with download links'

# --- publish, then make sure the updater url serves this version
Run "gh release edit $Tag --repo $Repo --notes-file `"$NotesFile`" --draft=false --latest"
Start-Sleep 5
$served = Invoke-RestMethod "https://github.com/$Repo/releases/latest/download/latest.json"
if ($served.version -ne $Version) { throw "latest.json is serving $($served.version), not $Version" }
Write-Host "released ${Tag}: https://github.com/$Repo/releases/tag/$Tag"

# --- Homebrew: `brew install --cask zahidhossin39/tap/shutter` reads Casks/shutter.rb in our tap
$Tap = 'zahidhossin39/homebrew-tap'
Run "gh release download $Tag --repo $Repo --pattern *.dmg --dir `"$tmp`" --clobber"
$dmg = Get-ChildItem $tmp -Filter *.dmg | Select-Object -First 1
if (-not $dmg) { throw 'no .dmg in the release' }
$sha = (Get-FileHash $dmg.FullName -Algorithm SHA256).Hash.ToLower()
$dmgName = $dmg.Name -replace [regex]::Escape($Version), '#{version}'
$cask = @"
cask "shutter" do
  version "$Version"
  sha256 "$sha"

  url "https://github.com/$Repo/releases/download/v#{version}/$dmgName"
  name "Shutter"
  desc "Screenshot tool with blur, OCR, annotations and a drag-out shelf"
  homepage "https://github.com/$Repo"

  auto_updates true
  depends_on macos: ">= :catalina"

  app "Shutter.app"

  # Not notarized yet: without this, macOS blocks the first launch.
  postflight do
    system_command "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "#{appdir}/Shutter.app"]
  end

  zap trash: [
    "~/Library/Application Support/com.zaid-hossain.screenshot-app",
    "~/Library/Caches/com.zaid-hossain.screenshot-app",
    "~/Library/WebKit/com.zaid-hossain.screenshot-app",
  ]
end
"@
cmd /c "gh repo view $Tap >nul 2>nul"
if ($LASTEXITCODE) { Run "gh repo create $Tap --public --description `"Homebrew tap for Shutter`"" }
$existing = cmd /c "gh api repos/$Tap/contents/Casks/shutter.rb -q .sha 2>nul"
$body = @{ message = "shutter $Version"; content = [Convert]::ToBase64String($Utf8.GetBytes($cask)) }
if ($existing) { $body.sha = "$existing".Trim() }
$bodyPath = Join-Path $tmp 'cask.json'
[IO.File]::WriteAllText($bodyPath, ($body | ConvertTo-Json), $Utf8)
Run "gh api -X PUT repos/$Tap/contents/Casks/shutter.rb --input `"$bodyPath`" --silent"
Write-Host "Homebrew cask updated: brew install --cask zahidhossin39/tap/shutter"
