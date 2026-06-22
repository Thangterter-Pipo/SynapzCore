# generate_skills_index.ps1
# Scan tat ca SKILL.md tu cac skill roots cua Antigravity -> data/skills.json
# Chay: powershell -ExecutionPolicy Bypass -File scripts\generate_skills_index.ps1

$ErrorActionPreference = 'SilentlyContinue'

$roots = @(
    @{ path = "$env:USERPROFILE\.gemini\config\skills"; label = 'Gemini Config' },
    @{ path = "E:\AGT_Brain\.claude\skills";            label = 'GitNexus' }
)

function Parse-Frontmatter($file) {
    $lines = Get-Content -LiteralPath $file -ErrorAction SilentlyContinue
    if (-not $lines -or $lines[0].Trim() -ne '---') { return $null }
    $fm = @{}
    for ($i = 1; $i -lt $lines.Count; $i++) {
        $l = $lines[$i]
        if ($l.Trim() -eq '---') { break }
        if ($l -match '^\s*([A-Za-z_]+)\s*:\s*(.*)$') {
            $key = $matches[1].ToLower()
            $val = $matches[2].Trim().Trim('"').Trim("'")
            $fm[$key] = $val
        }
    }
    return $fm
}

$skills = @()
foreach ($root in $roots) {
    if (-not (Test-Path $root.path)) { continue }
    $files = Get-ChildItem -LiteralPath $root.path -Filter SKILL.md -Recurse -ErrorAction SilentlyContinue
    foreach ($f in $files) {
        $fm = Parse-Frontmatter $f.FullName
        $name = if ($fm.name) { $fm.name } else { Split-Path (Split-Path $f.FullName -Parent) -Leaf }
        $desc = if ($fm.description) { $fm.description } else { '' }
        # Group: neu nam trong everything-claude-code thi label rieng
        $group = $root.label
        if ($f.FullName -match 'everything-claude-code') { $group = 'Everything Claude Code (ECC)' }
        elseif ($f.FullName -match '\\config\\skills\\([^\\]+)\\SKILL.md$') { $group = 'Core Skills' }
        $origin = if ($fm.origin) { $fm.origin } else { '' }
        $skills += [PSCustomObject]@{
            name        = $name
            description = $desc
            origin      = $origin
            group       = $group
            path        = $f.FullName
        }
    }
}

$skills = $skills | Sort-Object group, name
$out = [PSCustomObject]@{
    generated_at = (Get-Date).ToString('o')
    total        = $skills.Count
    skills       = $skills
}

$dest = "E:\AGT_Brain\data\skills.json"
$out | ConvertTo-Json -Depth 5 | Out-File -LiteralPath $dest -Encoding utf8
Write-Host "OK: $($skills.Count) skills -> $dest"
