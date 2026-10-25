[CmdletBinding()]
param(
    [string]$Version = "latest",
    [string]$Prefix = (Join-Path $env:LOCALAPPDATA "YAM"),
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"
$repo = "Yandex-Station-Universal-Repo/BetterYAM"
$headers = @{ "User-Agent" = "YAM-installer"; "Accept" = "application/vnd.github+json" }

if ($Version -eq "latest") {
    $release = Invoke-RestMethod -Headers $headers -Uri "https://api.github.com/repos/$repo/releases/latest"
    $tag = $release.tag_name
} else {
    $tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
}

$baseUrl = "https://github.com/$repo/releases/download/$tag"
$binDir = Join-Path $Prefix "bin"
$byamBinary = Join-Path $binDir "byam-bin.exe"
$byamCommand = Join-Path $binDir "byam.cmd"
$yamCommand = Join-Path $binDir "yam.cmd"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("yam-" + [guid]::NewGuid())

function Remove-StationDeviceId([string]$Model) {
    return ($Model -creplace '[-_][A-Z0-9]{4}$', '')
}

if ($Uninstall) {
    if (Test-Path $Prefix) { Remove-Item -Force -Recurse $Prefix }
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $updatedPath = (($userPath -split ";") | Where-Object { $_ -and $_ -ne $binDir }) -join ";"
    [Environment]::SetEnvironmentVariable("Path", $updatedPath, "User")
    [Environment]::SetEnvironmentVariable("YAM_URL", $null, "User")
    Write-Host "YAM CLI removed from $Prefix"
    return
}

function Select-StationSerial {
    $devices = @(& adb devices | Select-Object -Skip 1 | ForEach-Object {
            $fields = $_ -split "\s+"
            if ($fields.Length -ge 2 -and $fields[1] -eq "device") {
                $serial = $fields[0]
                $arch = & adb -s $serial shell uname -m 2>$null
                if ($arch -match "aarch64|arm64|armv7l|armv7") {
                    $model = Remove-StationDeviceId ((& adb -s $serial shell uname -n 2>$null).Trim())
                    [PSCustomObject]@{ Serial = $serial; Model = $(if ($model) { $model } else { "unknown" }) }
                }
            }
        })
    if ($devices.Count -eq 0) { return $null }
    if ($devices.Count -eq 1) { return $devices[0].Serial }
    Write-Host "Select station:"
    for ($i = 0; $i -lt $devices.Count; $i++) {
        Write-Host ("  {0}) {1} ({2})" -f ($i + 1), $devices[$i].Model, $devices[$i].Serial)
    }
    $selection = Read-Host "Number"
    $number = 0
    if (-not [int]::TryParse($selection, [ref]$number) -or $number -lt 1 -or $number -gt $devices.Count) {
        throw "Invalid station selection"
    }
    return $devices[$number - 1].Serial
}

function Find-StationIp {
    $adb = Get-Command adb -ErrorAction SilentlyContinue
    if ($adb) {
        $serial = Select-StationSerial
        if ($serial) {
            $script:yamStationSerial = $serial
            $route = & adb -s $serial shell "ip route get 1.1.1.1" 2>$null
            if ($route -match "\bsrc\s+(\d{1,3}(?:\.\d{1,3}){3})") { return $Matches[1] }
            $address = & adb -s $serial shell "ip -4 addr show wlan0" 2>$null
            if ($address -match "\binet\s+(\d{1,3}(?:\.\d{1,3}){3})") { return $Matches[1] }
        }
    }
    return Read-Host "Station IP address"
}

function Get-ReleaseAsset([string]$Name) {
    $path = Join-Path $tmp $Name
    Invoke-WebRequest -Headers $headers -Uri "$baseUrl/$Name" -OutFile $path
    return $path
}

try {
    New-Item -ItemType Directory -Force -Path $tmp, $binDir | Out-Null
    $checksums = Get-ReleaseAsset "SHA256SUMS"
    $manifestFile = Get-ReleaseAsset "manifest.json"
    $releaseManifest = Get-Content -Raw $manifestFile | ConvertFrom-Json
    $downloads = @{}
    foreach ($name in @("byam-x86_64-pc-windows-msvc.exe")) {
        $download = Get-ReleaseAsset $name
        $expected = ((Get-Content $checksums | Where-Object { $_ -match ("\s" + [regex]::Escape($name) + "$") }) -split "\s+")[0]
        if (-not $expected) { throw "No SHA-256 checksum for $name" }
        $actual = (Get-FileHash -Algorithm SHA256 $download).Hash.ToLowerInvariant()
        if ($actual -ne $expected.ToLowerInvariant()) { throw "Checksum mismatch for $name" }
        $downloads[$name] = $download
    }
    Copy-Item -Force $downloads["byam-x86_64-pc-windows-msvc.exe"] $byamBinary

    $stationIp = Find-StationIp
    if (-not $stationIp) { throw "Station IP address is required" }
    $adb = Get-Command adb -ErrorAction SilentlyContinue
    if (-not $adb) { throw "adb is required to deploy BetterYAMd to the station" }
    $serial = $script:yamStationSerial
    if (-not $serial) {
        & adb connect "$stationIp`:5555" | Out-Host
        $serial = "$stationIp`:5555"
        & adb -s $serial get-state | Out-Null
    }
    $stationHostname = (& adb -s $serial shell uname -n 2>$null).Trim()
    if (-not $stationHostname) { throw "Could not determine station model using uname -n" }
    $stationModel = Remove-StationDeviceId $stationHostname
    $modelId = ($stationModel.ToLowerInvariant() -replace '[^a-z0-9_-]', '-' -replace '-+', '-').Trim('-')
    if (-not $modelId) { throw "Station model did not produce a valid daemon ID" }
    $matches = @($releaseManifest.devices | Where-Object {
            $_.models -contains $modelId -or ($_.models.Count -eq 0 -and $_.device -eq $modelId)
        })
    if ($matches.Count -ne 1) { throw "No unique BetterYAMd manifest entry for model '$stationModel' ($modelId)" }
    $deviceEntry = $matches[0]
    $daemonAsset = $deviceEntry.asset
    $daemonSha256 = $deviceEntry.sha256sum
    if (-not $daemonAsset -or -not $daemonSha256 -or -not $deviceEntry.filepath) {
        throw "Incomplete release manifest entry for model '$stationModel'"
    }
    $listedSha256 = ((Get-Content $checksums | Where-Object {
                $_ -match ("\s" + [regex]::Escape($daemonAsset) + "$")
            }) -split "\s+")[0]
    if ($listedSha256 -ne $daemonSha256) { throw "Manifest checksum mismatch for $daemonAsset" }
    try {
        $daemonBinary = Get-ReleaseAsset $daemonAsset
    } catch {
        throw "No BetterYAMd release asset for model '$stationModel' ($daemonAsset)"
    }
    $actual = (Get-FileHash -Algorithm SHA256 $daemonBinary).Hash.ToLowerInvariant()
    if ($actual -ne $daemonSha256.ToLowerInvariant()) { throw "Checksum mismatch for $daemonAsset" }
    $devicePrefix = "/data/quazar/betteryam"
    & adb -s $serial shell "mkdir -p $devicePrefix/bin $devicePrefix/data" | Out-Null
    & adb -s $serial push $daemonBinary "$devicePrefix/bin/betteryamd.new" | Out-Null
    $startCommand = 'chmod 0755 /data/quazar/betteryam/bin/betteryamd.new; if [ -f /data/quazar/betteryam/betteryamd.pid ] && kill -0 $(cat /data/quazar/betteryam/betteryamd.pid) 2>/dev/null; then kill $(cat /data/quazar/betteryam/betteryamd.pid); fi; mv /data/quazar/betteryam/bin/betteryamd.new /data/quazar/betteryam/bin/betteryamd; start-stop-daemon --start --background --make-pidfile --pidfile /data/quazar/betteryam/betteryamd.pid --startas /data/quazar/betteryam/bin/betteryamd -- --data-dir /data/quazar/betteryam/data'
    & adb -s $serial shell $startCommand | Out-Null

    $yamUrl = "http://$stationIp`:8099"
    [Environment]::SetEnvironmentVariable("YAM_URL", $yamUrl, "User")
    $env:YAM_URL = $yamUrl
    @"
@echo off
if not defined YAM_URL set "YAM_URL=$yamUrl"
"%~dp0byam-bin.exe" %*
"@ | Set-Content -Encoding ascii $yamCommand
    Copy-Item -Force $yamCommand $byamCommand

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not (($userPath -split ";") -contains $binDir)) {
        [Environment]::SetEnvironmentVariable("Path", (($userPath, $binDir | Where-Object { $_ }) -join ";"), "User")
    }
    $env:Path = "$binDir;$env:Path"
    Write-Host "BetterYAM $tag installed in $Prefix and BetterYAMd ($($deviceEntry.filepath)) for $stationModel deployed to $stationIp"
    Write-Host "Use byam list or byam --url $yamUrl list (yam.cmd is also available)"
} finally {
    if (Test-Path $tmp) { Remove-Item -Force -Recurse $tmp }
}
