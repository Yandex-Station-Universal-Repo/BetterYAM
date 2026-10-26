[CmdletBinding()]
param(
    [string]$Version = "latest",
    [string]$Prefix = (Join-Path $env:LOCALAPPDATA "YAM"),
    [string]$Serial = "",
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
$daemonPort = 8099

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

# ---------- device discovery ----------

function Get-StationDevices {
    $list = @()
    foreach ($line in @(& adb devices 2>$null | Select-Object -Skip 1)) {
        $f = ([string]$line).Trim() -split "\s+"
        if ($f.Length -lt 2) { continue }
        if ($f[1] -ne "device") {
            if ($f[1] -in @("unauthorized", "offline")) {
                Write-Warning "Skipping $($f[0]): state is '$($f[1])'"
            }
            continue
        }
        $id = $f[0]
        $arch = (& adb -s $id shell uname -m 2>$null | Out-String).Trim()
        if ($arch -notmatch "aarch64|arm") { continue }
        $name = (& adb -s $id shell uname -n 2>$null | Out-String).Trim()
        $model = Remove-StationDeviceId $name
        $list += [PSCustomObject]@{
            Serial    = $id
            Model     = $(if ($model) { $model } else { "unknown" })
            Transport = $(if ($id -match ':\d+$') { "network" } else { "USB" })
        }
    }
    return $list
}

function Select-Station {
    $devices = @(Get-StationDevices)

    if ($Serial) {
        $picked = $devices | Where-Object { $_.Serial -eq $Serial } | Select-Object -First 1
        if (-not $picked) { throw "Device '$Serial' not found among connected stations" }
        return $picked
    }

    if ($devices.Count -eq 0) { return $null }
    if ($devices.Count -eq 1) { return $devices[0] }

    Write-Host "Several devices found. Select station:"
    for ($i = 0; $i -lt $devices.Count; $i++) {
        Write-Host ("  {0}) {1}  [{2}]  {3}" -f ($i + 1), $devices[$i].Model, $devices[$i].Transport, $devices[$i].Serial)
    }
    while ($true) {
        $selection = Read-Host "Number"
        $number = 0
        if ([int]::TryParse($selection, [ref]$number) -and $number -ge 1 -and $number -le $devices.Count) {
            return $devices[$number - 1]
        }
        Write-Host "Enter a number from 1 to $($devices.Count)"
    }
}

function Get-StationIp([string]$Id) {
    # 1) adb over network: IP is already in the serial
    if ($Id -match '^(\d{1,3}(?:\.\d{1,3}){3}):\d+$') { return $Matches[1] }

    # 2) ask the device itself
    $route = (& adb -s $Id shell "ip route get 1.1.1.1" 2>$null | Out-String)
    if ($route -match "\bsrc\s+(\d{1,3}(?:\.\d{1,3}){3})") { return $Matches[1] }

    $wlan = (& adb -s $Id shell "ip -4 addr show wlan0" 2>$null | Out-String)
    if ($wlan -match "\binet\s+(\d{1,3}(?:\.\d{1,3}){3})") { return $Matches[1] }

    $any = (& adb -s $Id shell "ip -4 addr" 2>$null | Out-String)
    foreach ($m in [regex]::Matches($any, "\binet\s+(\d{1,3}(?:\.\d{1,3}){3})")) {
        $candidate = $m.Groups[1].Value
        if (-not $candidate.StartsWith("127.")) { return $candidate }
    }

    # 3) no network on the device: caller falls back to USB (adb forward)
    return $null
}

function Get-ReleaseAsset([string]$Name) {
    $path = Join-Path $tmp $Name
    Invoke-WebRequest -Headers $headers -Uri "$baseUrl/$Name" -OutFile $path
    return $path
}

# ---------- install ----------

try {
    if (-not (Get-Command adb -ErrorAction SilentlyContinue)) {
        throw "adb is required to deploy BetterYAMd to the station"
    }

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

    # pick device
    $station = Select-Station
    $stationIp = $null
    $useForward = $false

    if ($station) {
        $deviceSerial = $station.Serial
        Write-Host "Using $($station.Model) [$($station.Transport)] $deviceSerial"
        $stationIp = Get-StationIp $deviceSerial
        if (-not $stationIp) { $useForward = $true }
    } else {
        Write-Host "No station found via adb (connect it over USB or enter its IP)."
        $manualIp = (Read-Host "Station IP address").Trim()
        if ($manualIp -notmatch '^\d{1,3}(\.\d{1,3}){3}$') {
            throw "No station found: connect it over USB or provide a valid IP address"
        }
        & adb connect "$manualIp`:5555" | Out-Host
        $deviceSerial = "$manualIp`:5555"
        & adb -s $deviceSerial get-state | Out-Null
        $stationIp = $manualIp
    }

    $stationHostname = (& adb -s $deviceSerial shell uname -n 2>$null | Out-String).Trim()
    if (-not $stationHostname) { throw "Could not determine station model using uname -n" }
    $stationModel = Remove-StationDeviceId $stationHostname
    $modelId = ($stationModel.ToLowerInvariant() -replace '[^a-z0-9_-]', '-' -replace '-+', '-').Trim('-')
    if (-not $modelId) { throw "Station model did not produce a valid daemon ID" }
    $manifestMatches = @($releaseManifest.devices | Where-Object {
            $_.models -contains $modelId -or ($_.models.Count -eq 0 -and $_.device -eq $modelId)
        })
    if ($manifestMatches.Count -ne 1) { throw "No unique BetterYAMd manifest entry for model '$stationModel' ($modelId)" }
    $deviceEntry = $manifestMatches[0]
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
    & adb -s $deviceSerial shell "mkdir -p $devicePrefix/bin $devicePrefix/data" | Out-Null
    & adb -s $deviceSerial push $daemonBinary "$devicePrefix/bin/betteryamd.new" | Out-Null
    $startCommand = 'chmod 0755 /data/quazar/betteryam/bin/betteryamd.new; if [ -f /data/quazar/betteryam/betteryamd.pid ] && kill -0 $(cat /data/quazar/betteryam/betteryamd.pid) 2>/dev/null; then kill $(cat /data/quazar/betteryam/betteryamd.pid); fi; mv /data/quazar/betteryam/bin/betteryamd.new /data/quazar/betteryam/bin/betteryamd; start-stop-daemon --start --background --make-pidfile --pidfile /data/quazar/betteryam/betteryamd.pid --startas /data/quazar/betteryam/bin/betteryamd -- --data-dir /data/quazar/betteryam/data'
    & adb -s $deviceSerial shell $startCommand | Out-Null

    # address the CLI will use
    if ($useForward) {
        & adb -s $deviceSerial forward "tcp:$daemonPort" "tcp:$daemonPort" | Out-Null
        $yamUrl = "http://127.0.0.1:$daemonPort"
        $forwardLine = "adb -s `"$deviceSerial`" forward tcp:$daemonPort tcp:$daemonPort >nul 2>&1`r`n"
    } else {
        $yamUrl = "http://$stationIp`:$daemonPort"
        $forwardLine = ""
    }

    [Environment]::SetEnvironmentVariable("YAM_URL", $yamUrl, "User")
    $env:YAM_URL = $yamUrl

    $wrapper = "@echo off`r`n" +
        $forwardLine +
        "if not defined YAM_URL set `"YAM_URL=$yamUrl`"`r`n" +
        "`"%~dp0byam-bin.exe`" %*`r`n"
    Set-Content -Encoding ascii -Path $yamCommand -Value $wrapper
    Copy-Item -Force $yamCommand $byamCommand

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not (($userPath -split ";") -contains $binDir)) {
        [Environment]::SetEnvironmentVariable("Path", (($userPath, $binDir | Where-Object { $_ }) -join ";"), "User")
    }
    $env:Path = "$binDir;$env:Path"

    $where = if ($useForward) { "$deviceSerial (USB, adb forward)" } else { $stationIp }
    Write-Host "BetterYAM $tag installed in $Prefix and BetterYAMd ($($deviceEntry.filepath)) for $stationModel deployed to $where"
    Write-Host "Use byam list or byam --url $yamUrl list (yam.cmd is also available)"
} finally {
    if (Test-Path $tmp) { Remove-Item -Force -Recurse $tmp }
}