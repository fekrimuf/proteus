# Builds corpus/materials_manifest.json from the Poly Haven public API for the ids listed in corpus/materials/library.json.
# Only CC0 PBR sets at 1k are listed; `pt corpus fetch corpus/materials_manifest.json` then downloads them and writes the ledger.
# Run from the repo root:  powershell -File corpus/tools/make_materials_manifest.ps1
$ErrorActionPreference = 'Stop'
$ua = 'proteus-corpus/0.1 (research; contact via repo owner)'
$lib = Get-Content corpus/materials/library.json -Raw | ConvertFrom-Json
$ids = $lib.materials.PSObject.Properties | ForEach-Object { $_.Value.polyhaven } | Sort-Object -Unique
$maps = [ordered]@{ Diffuse = 'diff'; Rough = 'rough'; nor_gl = 'nor_gl'; Displacement = 'disp'; Metal = 'metal' }
$attr = "Poly Haven (polyhaven.com); CC0 per https://polyhaven.com/license; the API terms ask for a 'Powered by Poly Haven' credit when the API is used in a product"
$assets = @()
foreach ($id in $ids) {
    $f = Invoke-RestMethod -Uri "https://api.polyhaven.com/files/$id" -UserAgent $ua
    $files = @()
    foreach ($k in $maps.Keys) {
        $e = $f.$k
        if ($null -eq $e) { continue }
        $u = $e.'1k'.jpg.url
        if ($null -eq $u) { continue }
        $files += [ordered]@{ path = "polyhaven/materials/$id/$($id)_$($maps[$k])_1k.jpg"; url = $u }
    }
    if ($files.Count -lt 3) { throw "$id has only $($files.Count) maps" }
    $assets += [ordered]@{
        id = "polyhaven/materials/$id"; source = 'polyhaven'; set = 'materials'; category = 'material'; difficulty = 'material'
        license = 'CC0-1.0'; license_url = 'https://creativecommons.org/publicdomain/zero/1.0/'; attribution = $attr
        main = $files[0].path; files = $files
    }
    Write-Host "$id : $($files.Count) maps"
}
$m = [ordered]@{ version = 1; note = 'CC0 PBR material sets for the Blender render (Level 2). Names map to these ids in corpus/materials/library.json.'; assets = $assets }
# UTF-8 without a byte-order mark (the Rust JSON reader rejects one)
[IO.File]::WriteAllText((Join-Path (Get-Location) 'corpus/materials_manifest.json'), ($m | ConvertTo-Json -Depth 8), (New-Object Text.UTF8Encoding($false)))
Write-Host "wrote corpus/materials_manifest.json with $($assets.Count) sets"
