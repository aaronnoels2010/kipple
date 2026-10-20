# SPDX-License-Identifier: GPL-3.0-or-later
#
# Spike S9 on Windows (CI runner, admin): home (C:), NTFS / FAT32 / exFAT virtual disks,
# and an SMB share from localhost, both as a mapped drive letter and as a UNC path.
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

cargo build --release --quiet
New-Item -ItemType Directory -Force out | Out-Null

function New-TestVhd($fs, $letter) {
    $file = Join-Path $env:RUNNER_TEMP "k-$fs.vhdx"
    $script = @"
create vdisk file="$file" maximum=256 type=expandable
select vdisk file="$file"
attach vdisk
create partition primary
format fs=$fs quick label=K$fs
assign letter=$letter
"@
    $tmp = Join-Path $env:RUNNER_TEMP "diskpart-$fs.txt"
    Set-Content -Path $tmp -Value $script -Encoding ascii
    diskpart /s $tmp | Out-Null
}
New-TestVhd "ntfs" "N"
New-TestVhd "fat32" "R"
New-TestVhd "exfat" "X"

New-Item -ItemType Directory -Force C:\kshare | Out-Null
New-SmbShare -Name kshare -Path C:\kshare -FullAccess Everyone | Out-Null
net use S: \\localhost\kshare | Out-Null

$header = "## Windows $([System.Environment]::OSVersion.VersionString) · $(Get-Date -Format 'yyyy-MM-dd HH:mm')`n"
$table = & .\target\release\kipple-spike-trashprobe.exe `
    "home=$env:USERPROFILE" "ntfs-vhd=N:\" "fat32-vhd=R:\" "exfat-vhd=X:\" `
    "smb-mapped-drive=S:\" "smb-unc=\\localhost\kshare"
$out = $header + "`n" + ($table -join "`n")
Set-Content -Path out\results-windows.md -Value $out -Encoding utf8
$out
exit 0
