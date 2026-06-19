# hosts/dverse-ca/hardware-configuration.nix
#
# STUB. Replace this entire file with the real hardware-configuration.nix for
# your target machine — generate it on the host with:
#
#     nixos-generate-config --show-hardware-config > hardware-configuration.nix
#
# The values below are intentionally non-functional placeholders so the flake
# evaluates; building a system from them will not produce a bootable machine.
#
# The original deployment (Contabo VPS) used a qemu-guest profile with a GRUB
# install on /dev/sda and an ext4 root mounted by UUID. Adapt accordingly.

{ lib, modulesPath, ... }:
{
  imports = [ (modulesPath + "/profiles/qemu-guest.nix") ];

  boot.loader.grub.device = lib.mkDefault "/dev/sda";
  boot.initrd.availableKernelModules = [
    "ata_piix"
    "uhci_hcd"
    "virtio_pci"
    "virtio_scsi"
    "sd_mod"
    "sr_mod"
  ];
  boot.initrd.kernelModules = [ ];
  boot.kernelModules = [ ];
  boot.extraModulePackages = [ ];

  fileSystems."/" = lib.mkDefault {
    # Replace with your actual root device UUID.
    device = "/dev/disk/by-label/nixos";
    fsType = "ext4";
  };

  swapDevices = [ ];

  nixpkgs.hostPlatform = lib.mkDefault "x86_64-linux";
}
