# Builds corpus/models_manifest.json from the Poly Haven public API for the model ids in corpus/models.txt (one id per line).
# Only CC0 glTF models at 1k texture size are listed; `pt corpus fetch corpus/models_manifest.json` downloads them and writes the ledger.
# Run from the repo root:  powershell -File corpus/tools/make_models_manifest.ps1
$ErrorActionPreference = 'Stop'
$ua = 'proteus-corpus/0.1 (research; contact via repo owner)'
$ids = Get-Content corpus/models.txt | ForEach-Object { $_.Trim() } | Where-Object { $_ -and -not $_.StartsWith('#') }
$attr = "Poly Haven (polyhaven.com); CC0 per https://polyhaven.com/license; the API terms ask for a 'Powered by Poly Haven' credit when the API is used in a product"
$assets = @()
foreach ($id in $ids) {
    $f = Invoke-RestMethod -Uri "https://api.polyhaven.com/files/$id" -UserAgent $ua
    $g = $f.gltf.'1k'.gltf
    if ($null -eq $g) { throw "$id has no 1k glTF" }
    $info = Invoke-RestMethod -Uri "https://api.polyhaven.com/info/$id" -UserAgent $ua
    $files = @([ordered]@{ path = "polyhaven/models/$id/$($id)_1k.gltf"; url = $g.url })
    foreach ($p in $g.include.PSObject.Properties) {
        $files += [ordered]@{ path = "polyhaven/models/$id/$($p.Name)"; url = $p.Value.url }
    }
    $assets += [ordered]@{
        id = "polyhaven/models/$id"; source = 'polyhaven'; set = 'models'; category = ($info.categories -join ','); difficulty = 'asset'
        license = 'CC0-1.0'; license_url = 'https://creativecommons.org/publicdomain/zero/1.0/'; attribution = $attr
        main = $files[0].path; files = $files
    }
    Write-Host "$id : $($files.Count) files, tris=$($info.polycount)"
}
$m = [ordered]@{ version = 1; note = 'CC0 Poly Haven models used as world assets (import_asset). Ids listed in corpus/models.txt.'; assets = $assets }
[IO.File]::WriteAllText((Join-Path (Get-Location) 'corpus/models_manifest.json'), ($m | ConvertTo-Json -Depth 8), (New-Object Text.UTF8Encoding($false)))
Write-Host "wrote corpus/models_manifest.json with $($assets.Count) models"
