# ask_grok.ps1 — Quick Grok caller for Antigravity
# Usage: .\ask_grok.ps1 "your question here"
# Usage: .\ask_grok.ps1 -Mode research "topic"
# Usage: .\ask_grok.ps1 -Mode think "problem"

param(
    [Parameter(Position=0, Mandatory=$true)]
    [string]$Prompt,
    
    [ValidateSet("chat","research","think","review","brainstorm")]
    [string]$Mode = "chat",
    
    [string]$Model = "grok-4.20-0309-non-reasoning"
)

# Map legacy models to active models on local grok2api
if ($Model -eq "grok-4-heavy" -or $Model -eq "grok-4" -or $Model -eq "heavy" -or $Model -eq "expert") {
    $Model = "grok-4.20-0309-non-reasoning"
} elseif ($Model -eq "grok-3" -or $Model -eq "grok-3-mini" -or $Model -eq "grok-3-thinking" -or $Model -eq "mini" -or $Model -eq "fast" -or $Model -eq "grok-4-fast") {
    $Model = "grok-4.20-fast"
}

$systemPrompts = @{
    "chat" = "You are Grok, an expert AI assistant. Answer concisely and precisely."
    "research" = "You are Grok Research Engine. Provide deep analytical research with evidence, trade-offs, and actionable recommendations. Use structured headers."
    "think" = "You are Grok Thinking Engine. Apply first principles thinking. Evaluate 2-3 options, give ONE clear recommendation with rationale. Be decisive."
    "review" = "You are Grok Code Review Engine. Find bugs, security issues, performance problems. For each: describe issue, show fix, explain why. End with APPROVE/NEEDS CHANGES/BLOCK."
    "brainstorm" = "You are Grok Brainstorm Engine. Generate Bold Ideas, Quick Wins, and Long-Term Seeds. Each idea: summary, why it matters, first action step."
}

$body = @{
    model = $Model
    messages = @(
        @{ role = "system"; content = $systemPrompts[$Mode] }
        @{ role = "user"; content = $Prompt }
    )
    stream = $false
} | ConvertTo-Json -Depth 5

try {
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($body)
    $grokBase = if ($env:GROK_API_BASE) { $env:GROK_API_BASE } else { "http://127.0.0.1:8000" }
    $response = Invoke-RestMethod -Uri "$grokBase/v1/chat/completions" `
        -Method Post `
        -ContentType "application/json; charset=utf-8" `
        -Body $bodyBytes `
        -TimeoutSec 180
    
    $answer = $response.choices[0].message.content
    Write-Output $answer
} catch {
    Write-Error "Grok API error: $_"
}
