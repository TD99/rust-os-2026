$ErrorActionPreference = "Stop"

$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
$Dist = Join-Path $Root "dist"
$Efi = Join-Path $Root "target\x86_64-unknown-uefi\release\rustos-poc.efi"
$RawImage = Join-Path $Dist "rustos-poc.img"
$VmdkImage = Join-Path $Dist "rustos-poc.vmdk"
$VmxFile = Join-Path $Dist "rustos-poc.vmx"

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)] [string] $Command,
        [Parameter(Mandatory = $true)] [string[]] $Arguments
    )

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $Command $($Arguments -join ' ')"
    }
}

function Write-Vmx {
    param([Parameter(Mandatory = $true)] [string] $Path)

    $Config = @'
.encoding = "UTF-8"
config.version = "8"
virtualHW.version = "17"
virtualHW.productCompatibility = "hosted"
displayName = "RustOS POC"
guestOS = "other-64"
firmware = "efi"
efi.secureBoot.enabled = "FALSE"
memsize = "512"
numvcpus = "1"
cpuid.coresPerSocket = "1"
scsi0.present = "TRUE"
scsi0.virtualDev = "lsilogic"
scsi0:0.present = "TRUE"
scsi0:0.fileName = "rustos-poc.vmdk"
scsi0:0.deviceType = "disk"
floppy0.present = "FALSE"
ide1:0.present = "FALSE"
ethernet0.present = "FALSE"
sound.present = "FALSE"
usb.present = "FALSE"
svga.autodetect = "TRUE"
checkpoint.vmState = ""
'@

    Set-Content -LiteralPath $Path -Value $Config -Encoding ascii
}

Push-Location $Root
try {
    Invoke-Checked "rustup" @("target", "add", "x86_64-unknown-uefi")
    Invoke-Checked "cargo" @("build", "-p", "rustos-poc", "--release", "--target", "x86_64-unknown-uefi")
    Invoke-Checked "cargo" @("run", "-p", "image-builder", "--", "$Efi", "$RawImage")

    $QemuImg = Get-Command qemu-img -ErrorAction SilentlyContinue
    if ($QemuImg) {
        Invoke-Checked $QemuImg.Source @("convert", "-f", "raw", "-O", "vmdk", "$RawImage", "$VmdkImage")
        Write-Vmx $VmxFile
        "Built VMware disk: $VmdkImage"
        "Built VMware config: $VmxFile"
    } else {
        "qemu-img was not found. Built raw boot disk only: $RawImage"
    }
} finally {
    Pop-Location
}
