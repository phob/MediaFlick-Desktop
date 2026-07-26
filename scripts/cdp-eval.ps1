# Evaluate a JavaScript expression inside the running app window over the CEF
# remote-debugging port. Development aid: start the app with
# `--remote-debugging-port 9222` first.
#
#   pwsh scripts/cdp-eval.ps1 -Expression 'document.title'
param(
    [Parameter(Mandatory = $true)][string]$Expression,
    [int]$Port = 9222,
    [int]$TimeoutSeconds = 60
)

$ErrorActionPreference = 'Stop'

$targets = (Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$Port/json/list").Content | ConvertFrom-Json
$page = $targets | Where-Object { $_.type -eq 'page' } | Select-Object -First 1
if (-not $page) { throw "no page target on port $Port" }

$ws = New-Object System.Net.WebSockets.ClientWebSocket
$ws.ConnectAsync([Uri]$page.webSocketDebuggerUrl, [Threading.CancellationToken]::None).Wait()
try {
    $payload = @{
        id     = 1
        method = 'Runtime.evaluate'
        params = @{ expression = $Expression; awaitPromise = $true; returnByValue = $true }
    } | ConvertTo-Json -Depth 10 -Compress
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
            if ($message.result.exceptionDetails) {
                throw ($message.result.exceptionDetails | ConvertTo-Json -Depth 8)
            }
            return $message.result.result.value
        }
    }
    throw "timed out waiting for the evaluation result"
}
finally {
    $ws.Dispose()
}
