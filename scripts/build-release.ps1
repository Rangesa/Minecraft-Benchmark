$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot

$adapterJar = Join-Path $projectRoot "build\libs\mcbench-1.0.jar"
& .\gradlew.bat build --no-daemon
if ($LASTEXITCODE -ne 0) {
    throw "Gradle Adapter build failed with exit code $LASTEXITCODE"
}
if (-not (Test-Path -LiteralPath $adapterJar -PathType Leaf)) {
    throw "Gradle did not produce the Fabric Adapter JAR: $adapterJar"
}

$env:MINEBENCH_ADAPTER_JAR = $adapterJar
& cargo build -p minebench-launcher --release
$cargoExitCode = $LASTEXITCODE
Remove-Item Env:MINEBENCH_ADAPTER_JAR -ErrorAction SilentlyContinue
if ($cargoExitCode -ne 0) {
    throw "Rust launcher build failed with exit code $cargoExitCode"
}

$releaseExe = Join-Path $projectRoot "target\release\minebench.exe"
if (-not (Test-Path -LiteralPath $releaseExe -PathType Leaf)) {
    throw "Rust release executable was not produced: $releaseExe"
}

$distDir = Join-Path $projectRoot "dist"
New-Item -ItemType Directory -Force -Path $distDir | Out-Null
$distributionExe = Join-Path $distDir "MineBench.exe"
Copy-Item -LiteralPath $releaseExe -Destination $distributionExe -Force

$hash = Get-FileHash -LiteralPath $distributionExe -Algorithm SHA256
$hash.Hash | Set-Content -LiteralPath "$distributionExe.sha256" -Encoding ascii -NoNewline
Write-Output "MineBench.exe: $distributionExe"
Write-Output "SHA-256: $($hash.Hash)"
