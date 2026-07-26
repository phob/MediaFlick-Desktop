# Capture a PNG of the running app window over the CEF remote-debugging port.
# Development aid: start the app with `--remote-debugging-port 9222` first.
#
#   pwsh scripts/cdp-screenshot.ps1 -Path shot.png
param(
    [Parameter(Mandatory = $true)][string]$Path,
    [int]$Port = 9222,
    [switch]$FullPage,
    [int]$TimeoutSeconds = 60
)

$ErrorActionPreference = 'Stop'

$targets = (Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$Port/json/list").Content | ConvertFrom-Json
$page = $targets | Where-Object { $_.type -eq 'page' } | Select-Object -First 1
if (-not $page) { throw "no page target on port $Port" }

$ws = New-Object System.Net.WebSockets.ClientWebSocket
$ws.ConnectAsync([Uri]$page.webSocketDebuggerUrl, [Threading.CancellationToken]::None).Wait()
try {
    $params = @{ format = 'png' }
    if ($FullPage) { $params.captureBeyondViewport = $true }
    $payload = @{ id = 1; method = 'Page.captureScreenshot'; params = $params } |
        ConvertTo-Json -Depth 10 -Compress
    $bytes = [Text.Encoding]::UTF8.GetBytes($payload)
    $ws.SendAsync((New-Object ArraySegment[byte] -ArgumentList @(, $bytes)), 'Text', $true,
        [Threading.CancellationToken]::None).Wait()

    $buffer = New-Object byte[] 1048576
    $segment = New-Object ArraySegment[byte] -ArgumentList @(, $buffer)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $sb = New-Object Text.StringBuilder
        do {
            $chunk = $ws.ReceiveAsync($segment, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
            [void]$sb.Append([Text.Encoding]::UTF8.GetString($buffer, 0, $chunk.Count))
        } while (-not $chunk.EndOfMessage)
        $message = $sb.ToString() | ConvertFrom-Json
        if ($message.id -eq 1) {
            if ($message.error) { throw $message.error.message }
            [IO.File]::WriteAllBytes((Resolve-Path -LiteralPath (Split-Path -Parent $Path)).Path +
                [IO.Path]::DirectorySeparatorChar + (Split-Path -Leaf $Path),
                [Convert]::FromBase64String($message.result.data))
            Write-Output $Path
            return
        }
    }
    throw "timed out waiting for the screenshot"
}
finally {
    $ws.Dispose()
}
