$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

foreach ($key in 'AZURE_CLIENT_ID', 'AZURE_TENANT_ID', 'AZURE_CLIENT_SECRET', 'AZURE_SIGNING_ENDPOINT', 'AZURE_SIGNING_ACCOUNT', 'AZURE_CERTIFICATE_PROFILE') {
    if (-not [Environment]::GetEnvironmentVariable($key)) {
        throw "Missing signing configuration: $key"
    }
}

$sdkBin = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
$signTool = Get-ChildItem "$sdkBin\10.*\x64\signtool.exe" |
    Sort-Object { [version]$_.Directory.Parent.Name } -Descending |
    Select-Object -First 1
if (-not $signTool) { throw "Cannot find the Windows SDK x64 SignTool in $sdkBin" }

$metadata = Join-Path $env:RUNNER_TEMP 'artifact-signing.json'
@{
    Endpoint = $env:AZURE_SIGNING_ENDPOINT
    CodeSigningAccountName = $env:AZURE_SIGNING_ACCOUNT
    CertificateProfileName = $env:AZURE_CERTIFICATE_PROFILE
} | ConvertTo-Json | Set-Content $metadata

$client = Join-Path $env:RUNNER_TEMP 'Microsoft.ArtifactSigning.Client/bin/x64/Azure.CodeSigning.Dlib.dll'
$signArgs = @('sign', '/fd', 'SHA256', '/tr', 'http://timestamp.acs.microsoft.com', '/td', 'SHA256', '/dlib', $client, '/dmdf', $metadata)
$release = "target/$env:TARGET/release"
$cli = "$release/diffkin.exe"
& $signTool.FullName @signArgs $cli

# Tauri invokes signCommand on the patched desktop EXE before packing it,
# then on the NSIS installer. Use an argument array to preserve SDK paths.
$config = Join-Path $env:RUNNER_TEMP 'tauri-signing.json'
$signCommand = @{ cmd = $signTool.FullName; args = $signArgs + '%1' }
@{ bundle = @{ windows = @{ signCommand = $signCommand } } } |
    ConvertTo-Json -Depth 5 | Set-Content $config
pnpm desktop bundle --target $env:TARGET --bundles nsis --config $config
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$installers = @(Get-ChildItem "$release/bundle/nsis/*.exe" -File)
if ($installers.Count -ne 1) { throw "Expected one NSIS installer, found $($installers.Count)" }
& $signTool.FullName verify /pa /all /tw $cli $installers[0].FullName
