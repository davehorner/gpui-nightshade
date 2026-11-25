param([switch]$VerifyOnly)
$ErrorActionPreference = 'Stop'
$pins = @{
    'nightshade' = '630c74c973f1f1e047123f2fb3090be5a26ba6b9'
    'zed' = '2664a0aa1eae5dabf8f8273c03f88d53061ca7dc'
    'gpui-component' = '82f1c1dc1f2970d93f1adfe10cbd772516c21ce7'
}
if (!$VerifyOnly) {
    git submodule update --init --depth 1
    if ($LASTEXITCODE) { throw 'upstream checkout failed' }
}
foreach ($entry in $pins.GetEnumerator()) {
    $actual = git -C "upstream/$($entry.Key)" rev-parse HEAD
    if ($actual -ne $entry.Value) { throw "Wrong $($entry.Key) revision: $actual" }
}
if (!$VerifyOnly) {
    foreach ($patch in @('0001-external-output.patch', '0002-embedded-viewer-core.patch', '0003-deferred-spawn-selection.patch', '0004-mcp-alignment.patch', '0005-mcp-transform-batches.patch', '0006-frontend-camera-access.patch', '0007-dx12-indirect-builtins.patch', '0008-mcp-operation-tracking.patch')) {
        $patchPath = "../../patches/nightshade/$patch"
        git -C upstream/nightshade apply --check $patchPath 2>$null
        if ($LASTEXITCODE -eq 0) {git -C upstream/nightshade apply $patchPath}
        else {
            git -C upstream/nightshade apply --reverse --check $patchPath
            if ($LASTEXITCODE) {throw "Nightshade patch $patch conflicts with local changes"}
        }
    }
}
Write-Output 'Upstream revisions verified.'
