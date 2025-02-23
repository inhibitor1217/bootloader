use std::{io::Read, path::Path, process::Command};

const QEMU_ARGS: &[&str] = &[
    "-device",
    "isa-debug-exit,iobase=0xf4,iosize=0x04",
    "-serial",
    "stdio",
    "-display",
    "none",
    "--no-reboot",
];
const SEPARATOR: &str = "\n____________________________________\n";

pub fn run_test_kernel(kernel_binary_path: &str) {
    run_test_kernel_with_ramdisk(kernel_binary_path, None)
}

pub fn run_test_kernel_with_ramdisk(kernel_binary_path: &str, ramdisk_path: Option<&Path>) {
    let kernel_path = Path::new(kernel_binary_path);

    #[cfg(feature = "uefi")]
    {
        // create a GPT disk image for UEFI booting
        let gpt_path = kernel_path.with_extension("gpt");
        let mut uefi_builder = bootloader::UefiBoot::new(kernel_path);
        // Set ramdisk for test, if supplied.
        if let Some(rdp) = ramdisk_path {
            uefi_builder.set_ramdisk(rdp);
        }
        uefi_builder.create_disk_image(&gpt_path).unwrap();

        // create a TFTP folder with the kernel executable and UEFI bootloader for
        // UEFI PXE booting
        let tftp_path = kernel_path.with_extension("tftp");
        uefi_builder.create_pxe_tftp_folder(&tftp_path).unwrap();

        run_test_kernel_on_uefi(&gpt_path);
        run_test_kernel_on_uefi_pxe(&tftp_path);
    }

    #[cfg(feature = "bios")]
    {
        // create an MBR disk image for legacy BIOS booting
        let mbr_path = kernel_path.with_extension("mbr");
        let mut bios_builder = bootloader::BiosBoot::new(kernel_path);
        // Set ramdisk for test, if supplied.
        if let Some(rdp) = ramdisk_path {
            bios_builder.set_ramdisk(rdp);
        }
        bios_builder.create_disk_image(&mbr_path).unwrap();

        run_test_kernel_on_bios(&mbr_path);
    }
}

#[cfg(feature = "uefi")]
pub fn run_test_kernel_on_uefi(out_gpt_path: &Path) {
    let ovmf_pure_efi = ovmf_prebuilt::ovmf_pure_efi();
    let args = [
        "-bios",
        ovmf_pure_efi.to_str().unwrap(),
        "-drive",
        &format!("format=raw,file={}", out_gpt_path.display()),
    ];
    run_qemu(args);
}

#[cfg(feature = "bios")]
pub fn run_test_kernel_on_bios(out_mbr_path: &Path) {
    let args = [
        "-drive",
        &(format!("format=raw,file={}", out_mbr_path.display())),
    ];
    run_qemu(args);
}

#[cfg(feature = "uefi")]
pub fn run_test_kernel_on_uefi_pxe(out_tftp_path: &Path) {
    let ovmf_pure_efi = ovmf_prebuilt::ovmf_pure_efi();
    let args = [
        "-netdev",
        &format!(
            "user,id=net0,net=192.168.17.0/24,tftp={},bootfile=bootloader,id=net0",
            out_tftp_path.display()
        ),
        "-device",
        "virtio-net-pci,netdev=net0",
        "-bios",
        ovmf_pure_efi.to_str().unwrap(),
    ];
    run_qemu(args);
}

fn run_qemu<'a, A>(args: A)
where
    A: IntoIterator<Item = &'a str>,
{
    use std::process::Stdio;

    let mut run_cmd = Command::new("qemu-system-x86_64");
    run_cmd.args(args);
    run_cmd.args(QEMU_ARGS);
    let run_cmd_str = format!("{run_cmd:?}");

    run_cmd.stdout(Stdio::piped());
    run_cmd.stderr(Stdio::piped());

    let mut child = run_cmd.spawn().unwrap();

    let child_stdout = child.stdout.take().unwrap();
    let mut child_stderr = child.stderr.take().unwrap();

    let copy_stdout = std::thread::spawn(move || {
        let print_cmd = format!("\nRunning {run_cmd_str}\n\n").into_bytes();
        let mut output = print_cmd.chain(child_stdout).chain(SEPARATOR.as_bytes());
        std::io::copy(
            &mut output,
            &mut strip_ansi_escapes::Writer::new(std::io::stdout()),
        )
    });
    let copy_stderr = std::thread::spawn(move || {
        std::io::copy(
            &mut child_stderr,
            &mut strip_ansi_escapes::Writer::new(std::io::stderr()),
        )
    });

    let exit_status = child.wait().unwrap();
    match exit_status.code() {
        Some(33) => {}                     // success
        Some(35) => panic!("Test failed"), // success
        other => panic!("Test failed with unexpected exit code `{other:?}`"),
    }

    copy_stdout.join().unwrap().unwrap();
    copy_stderr.join().unwrap().unwrap();
}
