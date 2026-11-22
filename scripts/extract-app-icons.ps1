# Extrait l'icône haute résolution (256 px) d'applications installées vers resources/icons/apps/*.png
#
# Usage :
#   powershell -ExecutionPolicy Bypass -File scripts\extract-app-icons.ps1
#   powershell -ExecutionPolicy Bypass -File scripts\extract-app-icons.ps1 -Exe "C:\chemin\vers\appli.exe" -Name mon-appli
#
# Les logos restent la propriété de leurs éditeurs : usage personnel sur ton boîtier uniquement.

param(
    [string]$Exe,
    [string]$Name,
    [int]$Size = 256,
    [string]$OutDir = (Join-Path $PSScriptRoot '..\resources\icons\apps')
)

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class IconExtractor {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern uint PrivateExtractIcons(string file, int index, int cx, int cy,
        IntPtr[] icons, uint[] ids, uint count, uint flags);
    [DllImport("user32.dll")]
    public static extern bool DestroyIcon(IntPtr handle);
}
"@

New-Item -ItemType Directory -Force $OutDir | Out-Null

function Export-AppIcon([string]$Path, [string]$OutName) {
    if (-not (Test-Path $Path)) { Write-Host "  absent : $OutName ($Path)"; return }
    $handles = [IntPtr[]]::new(1)
    $ids = [UInt32[]]::new(1)
    $n = [IconExtractor]::PrivateExtractIcons($Path, 0, $Size, $Size, $handles, $ids, 1, 0)
    if ($n -lt 1 -or $handles[0] -eq [IntPtr]::Zero) { Write-Warning "Pas d'icône dans $Path"; return }
    try {
        $icon = [System.Drawing.Icon]::FromHandle($handles[0])
        $bmp = $icon.ToBitmap()
        $dest = Join-Path $OutDir "$OutName.png"
        $bmp.Save($dest, [System.Drawing.Imaging.ImageFormat]::Png)
        Write-Host "  ok     : $OutName -> $dest ($($bmp.Width)x$($bmp.Height))"
        $bmp.Dispose(); $icon.Dispose()
    } finally {
        [IconExtractor]::DestroyIcon($handles[0]) | Out-Null
    }
}

if ($Exe) {
    if (-not $Name) { $Name = [IO.Path]::GetFileNameWithoutExtension($Exe).ToLower() }
    Export-AppIcon $Exe $Name
    return
}

# Applications trouvées sur ce PC (ajoute les tiennes ici).
$apps = [ordered]@{
    'obs'          = "$env:ProgramFiles\obs-studio\bin\64bit\obs64.exe"
    'discord'      = (Get-ChildItem "$env:LOCALAPPDATA\Discord*\app-*\Discord.exe" -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1).FullName
    'spotify'      = "$env:APPDATA\Spotify\Spotify.exe"
    'streamerbot'  = "$env:USERPROFILE\Documents\Streamer.bot\Streamer.bot.exe"
    'steam'        = "${env:ProgramFiles(x86)}\Steam\steam.exe"
    'epic-games'   = "$env:ProgramFiles\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"
    'edge'         = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
}

Write-Host "Extraction vers $OutDir"
foreach ($entry in $apps.GetEnumerator()) {
    if ($entry.Value) { Export-AppIcon $entry.Value $entry.Key } else { Write-Host "  absent : $($entry.Key)" }
}
