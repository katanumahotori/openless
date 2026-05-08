# Release build for OpenLess fork.
# Produces:
#   - openless-all\app\src-tauri\target\release\openless.exe
#   - bundle\msi\OpenLess_*.msi
#   - bundle\nsis\OpenLess_*-setup.exe
$env:PATH = 'C:\Users\katan\.cargo\bin;' + $env:PATH
Set-Location 'C:\Users\katan\Obsidian\90_tools\openless\openless-all\app'
& 'C:\Program Files\nodejs\npm.cmd' run tauri build 2>&1 | Tee-Object -FilePath 'C:\Users\katan\Obsidian\90_tools\openless\tauri_build.log'
'BUILD_DONE'
