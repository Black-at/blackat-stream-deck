"""Fabrique l'installeur Windows (.msi) de Soomfon Deck avec WiX Toolset 3.14.

    python installer/make-msi.py --wix <dossier contenant candle.exe et light.exe>

Résultat : target/installer/Soomfon-Deck-<version>.msi

Ce que fait le .msi (installation pour l'utilisateur courant, sans droits administrateur) :
  - copie le Stream Deck (soomfon-deck.exe), l'éditeur, le launcher Blackat et les icônes dans
    %LOCALAPPDATA%\\Programs\\Blackat (même dossier que la restauration d'un pack) ;
  - crée les raccourcis du menu Démarrer « Soomfon-Deck » (éditeur) et « Blackat » (launcher) :
    taper « soomfon-deck » dans la recherche de Windows ouvre l'éditeur ;
  - inscrit le Stream Deck au démarrage de Windows (désactivable dans l'éditeur ou le
    Gestionnaire des tâches) et le lance à la fin de l'installation ;
  - désinstallation propre depuis Paramètres → Applications.
Une nouvelle version remplace l'ancienne (même code de mise à niveau).
"""

import argparse
import re
import subprocess
import sys
import uuid
from pathlib import Path
from xml.sax.saxutils import quoteattr

ROOT = Path(__file__).resolve().parent.parent
RELEASE = ROOT / "target" / "release"
OUT = ROOT / "target" / "installer"
# Ne jamais changer : c'est ce qui permet à une nouvelle version de remplacer l'ancienne.
UPGRADE_CODE = "{6B1E2C0A-6F4D-4E59-9C3B-2A7D5B8E41C7}"
EXES = ["soomfon-deck.exe", "soomfon-editor.exe", "blackat-launcher.exe"]
REG = r"Software\Blackat\SoomfonDeck"


def version() -> str:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    return re.search(r'^version\s*=\s*"([^"]+)"', text, re.M).group(1)


def ident(prefix: str, rel: str) -> str:
    """Identifiant WiX stable (72 caractères max, lettres/chiffres/_/.)."""
    clean = re.sub(r"[^A-Za-z0-9_.]", "_", rel)
    return f"{prefix}_{uuid.uuid5(uuid.NAMESPACE_URL, rel).hex[:12]}_{clean[-40:]}"


def component(cid: str, dir_id: str, files: list[Path], extra: str = "") -> str:
    """Composant « par utilisateur » : clé de registre HKCU comme chemin clé (règles ICE)."""
    lines = [f'<Component Id="{cid}" Guid="{{{uuid.uuid5(uuid.NAMESPACE_URL, cid)}}}">']
    lines.append(f'  <RegistryValue Root="HKCU" Key="{REG}\\Components" Name="{cid}" Type="integer" Value="1" KeyPath="yes"/>')
    lines.append(f'  <RemoveFolder Id="rm_{cid}" Directory="{dir_id}" On="uninstall"/>')
    for f in files:
        fid = ident("f", str(f.relative_to(ROOT)))
        lines.append(f"  <File Id={quoteattr(fid)} Source={quoteattr(str(f))}/>")
    if extra:
        lines.append(extra)
    lines.append("</Component>")
    return "\n".join(lines)


def build_wxs(ver: str) -> str:
    icons = ROOT / "resources" / "icons"
    dirs, comps, refs = [], [], []

    def walk(path: Path, dir_id: str) -> str:
        """Arborescence des icônes : un <Directory> et un composant par dossier."""
        files = sorted(p for p in path.iterdir() if p.is_file())
        cid = ident("c", str(path.relative_to(ROOT)))
        comps.append(component(cid, dir_id, files))
        refs.append(cid)
        children = []
        # « perso » = images personnelles de l'utilisateur : jamais dans l'installeur (il est public).
        for sub in sorted(p for p in path.iterdir() if p.is_dir() and p.name != "perso"):
            sub_id = ident("d", str(sub.relative_to(ROOT)))
            children.append(f'<Directory Id="{sub_id}" Name={quoteattr(sub.name)}>{walk(sub, sub_id)}</Directory>')
        return "".join(children)

    icon_tree = walk(icons, "IconsDir")

    run_value = '"[INSTALLDIR]soomfon-deck.exe"'
    app_extra = "\n".join([
        # Raccourcis du menu Démarrer : c'est ce que trouve la recherche de Windows.
        '  <Shortcut Id="ScEditor" Directory="ProgramMenuFolder" Name="Soomfon-Deck" '
        'Description="Éditeur du Stream Deck SOOMFON (Blackat)" Target="[INSTALLDIR]soomfon-editor.exe" '
        'WorkingDirectory="INSTALLDIR" Icon="blackat.ico"/>',
        '  <Shortcut Id="ScLauncher" Directory="ProgramMenuFolder" Name="Blackat" '
        'Description="Lance OBS, Streamer.bot et le Stream Deck" Target="[INSTALLDIR]blackat-launcher.exe" '
        'WorkingDirectory="INSTALLDIR" Icon="blackat.ico"/>',
    ])
    # blackat.ico : icône des raccourcis créés hors installeur (Bureau, restauration d'un pack).
    app_files = [RELEASE / e for e in EXES] + [ROOT / "launcher" / "ui" / "blackat.ico"]
    app_comp = component("c_app", "INSTALLDIR", app_files, app_extra)
    autostart = (
        '<Component Id="c_autostart" Guid="{2F7C1B9E-0A3D-4C58-8E61-7B4F2D9A1C35}">\n'
        f'  <RegistryValue Root="HKCU" Key="Software\\Microsoft\\Windows\\CurrentVersion\\Run" Name="Soomfon Deck" '
        f'Type="string" Value={quoteattr(run_value)} KeyPath="yes"/>\n'
        "</Component>"
    )
    dirs_comp = (
        '<Component Id="c_dirs" Guid="{8D3A5E21-4B7C-4F90-A1D6-3C9E7B2F5A48}">\n'
        f'  <RegistryValue Root="HKCU" Key="{REG}" Name="InstallDir" Type="string" Value="[INSTALLDIR]" KeyPath="yes"/>\n'
        '  <RemoveFolder Id="rm_programs" Directory="ProgramsDir" On="uninstall"/>\n'
        '  <RemoveFolder Id="rm_res" Directory="ResDir" On="uninstall"/>\n'
        "</Component>"
    )

    return f"""<?xml version="1.0" encoding="utf-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi" xmlns:util="http://schemas.microsoft.com/wix/UtilExtension">
  <Product Id="*" Name="Soomfon Deck (Blackat)" Language="1036" Codepage="1252" Version="{ver}"
           Manufacturer="Blackat" UpgradeCode="{UPGRADE_CODE}">
    <Package InstallerVersion="500" Compressed="yes" InstallScope="perUser" InstallPrivileges="limited"
             Languages="1036" SummaryCodepage="1252" Description="Stream Deck SOOMFON : démon, éditeur et launcher Blackat"/>
    <MajorUpgrade AllowSameVersionUpgrades="yes" Schedule="afterInstallInitialize"
                  DowngradeErrorMessage="Une version plus récente de Soomfon Deck est déjà installée."/>
    <MediaTemplate EmbedCab="yes" CompressionLevel="high"/>
    <Icon Id="blackat.ico" SourceFile="{ROOT / 'launcher' / 'ui' / 'blackat.ico'}"/>
    <Property Id="ARPPRODUCTICON" Value="blackat.ico"/>
    <Property Id="ARPNOMODIFY" Value="1"/>

    <!-- Le Stream Deck en cours d'exécution verrouille son .exe : on l'arrête avant la copie. -->
    <SetProperty Id="WixQuietExecCmdLine" Value='"[SystemFolder]taskkill.exe" /F /IM soomfon-deck.exe'
                 Before="StopDaemon" Sequence="execute"/>
    <CustomAction Id="StopDaemon" BinaryKey="WixCA" DllEntry="WixQuietExec" Execute="immediate" Return="ignore"/>
    <!-- Lancé à la fin (sans fenêtre) : les touches s'allument aussitôt. -->
    <CustomAction Id="StartDaemon" Directory="INSTALLDIR" ExeCommand='"[INSTALLDIR]soomfon-deck.exe"' Return="asyncNoWait"/>
    <InstallExecuteSequence>
      <Custom Action="StopDaemon" Before="InstallValidate"/>
      <Custom Action="StartDaemon" After="InstallFinalize">NOT (REMOVE="ALL")</Custom>
    </InstallExecuteSequence>

    <Directory Id="TARGETDIR" Name="SourceDir">
      <Directory Id="ProgramMenuFolder"/>
      <Directory Id="LocalAppDataFolder">
        <Directory Id="ProgramsDir" Name="Programs">
          <Directory Id="INSTALLDIR" Name="Blackat">
            <Directory Id="ResDir" Name="resources">
              <Directory Id="IconsDir" Name="icons">{icon_tree}</Directory>
            </Directory>
          </Directory>
        </Directory>
      </Directory>
    </Directory>

    <DirectoryRef Id="INSTALLDIR">
{app_comp}
{autostart}
{dirs_comp}
    </DirectoryRef>
{"".join(f'''
    <DirectoryRef Id="{c.split('Directory="')[1].split('"')[0]}">
{c}
    </DirectoryRef>''' for c in comps)}

    <Feature Id="Main" Title="Soomfon Deck" Level="1">
      <ComponentRef Id="c_app"/>
      <ComponentRef Id="c_autostart"/>
      <ComponentRef Id="c_dirs"/>
{chr(10).join(f'      <ComponentRef Id="{r}"/>' for r in refs)}
    </Feature>
  </Product>
</Wix>
"""


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--wix", required=True, type=Path, help="dossier de WiX 3.14 (candle.exe, light.exe)")
    args = ap.parse_args()
    missing = [e for e in EXES if not (RELEASE / e).is_file()]
    if missing:
        print("Compile d'abord : cargo build --release --workspace (manquant : " + ", ".join(missing) + ")")
        return 1
    ver = version()
    OUT.mkdir(parents=True, exist_ok=True)
    wxs = OUT / "soomfon-deck.wxs"
    wxs.write_text(build_wxs(ver), encoding="utf-8")
    obj = OUT / "soomfon-deck.wixobj"
    msi = OUT / f"Soomfon-Deck-{ver}.msi"
    run = lambda cmd: subprocess.run(cmd, check=True)
    run([str(args.wix / "candle.exe"), "-nologo", "-ext", "WixUtilExtension", "-out", str(obj), str(wxs)])
    run([str(args.wix / "light.exe"), "-nologo", "-ext", "WixUtilExtension", "-cultures:fr-FR", "-sice:ICE91", "-sice:ICE61",
         "-spdb", "-out", str(msi), str(obj)])
    print(f"OK : {msi}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
