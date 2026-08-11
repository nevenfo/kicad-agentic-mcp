//! What the machine offers a local model: GPU, RAM, CPU cores, and which
//! local inference backends are on `PATH`.
//!
//! Every probe degrades to `None` / an empty `Vec` rather than a guessed
//! value (plan.md, Phase H step 2: "aucune sonde ne doit paniquer ni bloquer
//! si l'outil est absent"). The router that will read this (later step, not
//! built here) has to be able to trust that `None` means "not probed", never
//! "zero" — 0 MiB VRAM and "VRAM unknown" are different facts.

use std::process::Command;

/// One GPU. VRAM fields are `None` when the probe that found the GPU's name
/// could not also read its memory (e.g. the WMI fallback, which sees a
/// display adapter but not its memory counters).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GpuInfo {
    /// Adapter name as the probe reported it.
    pub name: String,
    /// Total VRAM in MiB, if the probe could read it.
    pub vram_total_mib: Option<u64>,
    /// Free VRAM in MiB, if the probe could read it. `nvidia-smi` reports
    /// this live; the WMI fallback cannot, so it is always `None` there.
    pub vram_free_mib: Option<u64>,
}

/// A local inference backend this probe knows how to look for by executable
/// name. Presence on `PATH` is not the same as "running" — this is a
/// capability probe, not a liveness check, so it does not open a socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalBackend {
    /// LM Studio's CLI (`lms`), which can also serve an OpenAI-compatible API.
    LmStudio,
    /// A standalone `llama.cpp` server binary.
    LlamaCppServer,
    /// Ollama, which also exposes an OpenAI-compatible surface.
    Ollama,
}

impl LocalBackend {
    fn executable_stems(self) -> &'static [&'static str] {
        match self {
            Self::LmStudio => &["lms"],
            Self::LlamaCppServer => &["llama-server"],
            Self::Ollama => &["ollama"],
        }
    }

    /// All backends this probe recognises.
    const ALL: [Self; 3] = [Self::LmStudio, Self::LlamaCppServer, Self::Ollama];
}

/// Everything [`probe`] could learn about the machine.
#[derive(Debug, Clone, Default)]
pub struct HardwareProfile {
    /// GPUs found. Empty means none found, not "no GPU present" — a probe
    /// tool being absent looks the same as a probe tool finding nothing.
    pub gpus: Vec<GpuInfo>,
    /// Total system RAM in bytes, if the probe succeeded.
    pub ram_total_bytes: Option<u64>,
    /// Logical CPU cores, if the platform could report it.
    pub cpu_cores_logical: Option<usize>,
    /// Local backends found on `PATH`.
    pub backends: Vec<LocalBackend>,
}

/// Probe the current machine. Never panics: every sub-probe catches its own
/// failure (missing tool, non-zero exit, unparsable output) and degrades to
/// an empty/`None` result instead of propagating an error, because there is
/// no caller-actionable difference between "nvidia-smi is not installed" and
/// "this machine has no NVIDIA GPU" — both mean "report nothing", not "crash
/// the process that was only trying to look".
#[must_use]
pub fn probe() -> HardwareProfile {
    HardwareProfile {
        gpus: probe_gpus(),
        ram_total_bytes: probe_ram_bytes(),
        cpu_cores_logical: std::thread::available_parallelism()
            .ok()
            .map(std::num::NonZeroUsize::get),
        backends: probe_backends(&path_dirs()),
    }
}

fn probe_gpus() -> Vec<GpuInfo> {
    let via_nvidia = probe_gpus_via_nvidia_smi("nvidia-smi");
    if !via_nvidia.is_empty() {
        return via_nvidia;
    }
    probe_gpus_via_wmi()
}

/// Runs `nvidia-smi` under an injectable command name so tests can point it
/// at a binary that does not exist and assert the "no GPU" path without
/// depending on whether the test machine actually has an NVIDIA card.
fn probe_gpus_via_nvidia_smi(command: &str) -> Vec<GpuInfo> {
    let output = Command::new(command)
        .args([
            "--query-gpu=name,memory.total,memory.free",
            "--format=csv,noheader,nounits",
        ])
        .output();
    let Ok(output) = output else {
        return Vec::new(); // tool absent: not a panic, not a guess.
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_nvidia_smi_line)
        .collect()
}

fn parse_nvidia_smi_line(line: &str) -> Option<GpuInfo> {
    let mut parts = line.split(',').map(str::trim);
    let name = parts.next()?.to_string();
    if name.is_empty() {
        return None;
    }
    let vram_total_mib = parts.next().and_then(|s| s.parse().ok());
    let vram_free_mib = parts.next().and_then(|s| s.parse().ok());
    Some(GpuInfo {
        name,
        vram_total_mib,
        vram_free_mib,
    })
}

/// Fallback for machines without `nvidia-smi` (no NVIDIA GPU, or a GPU from
/// another vendor): asks Windows for display adapter names only. No VRAM —
/// `Win32_VideoController.AdapterRAM` is a 32-bit field that misreports on
/// modern cards, so it is not read rather than reported wrong.
fn probe_gpus_via_wmi() -> Vec<GpuInfo> {
    probe_gpus_via_wmi_command("powershell")
}

fn probe_gpus_via_wmi_command(command: &str) -> Vec<GpuInfo> {
    let output = Command::new(command)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance Win32_VideoController).Name",
        ])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|name| GpuInfo {
            name: name.to_string(),
            vram_total_mib: None,
            vram_free_mib: None,
        })
        .collect()
}

fn probe_ram_bytes() -> Option<u64> {
    probe_ram_bytes_via("powershell")
}

/// Injectable command name for the same reason as [`probe_gpus_via_nvidia_smi`].
fn probe_ram_bytes_via(command: &str) -> Option<u64> {
    let output = Command::new(command)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn path_dirs() -> Vec<std::path::PathBuf> {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default()
}

fn probe_backends(dirs: &[std::path::PathBuf]) -> Vec<LocalBackend> {
    LocalBackend::ALL
        .into_iter()
        .filter(|backend| backend_on_path(*backend, dirs))
        .collect()
}

fn backend_on_path(backend: LocalBackend, dirs: &[std::path::PathBuf]) -> bool {
    let extensions: &[&str] = if cfg!(windows) {
        &["exe", "cmd", "bat"]
    } else {
        &[""]
    };
    backend.executable_stems().iter().any(|stem| {
        dirs.iter().any(|dir| {
            extensions.iter().any(|ext| {
                let candidate = if ext.is_empty() {
                    dir.join(stem)
                } else {
                    dir.join(format!("{stem}.{ext}"))
                };
                candidate.is_file()
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_normal_nvidia_smi_line() {
        let gpu = parse_nvidia_smi_line("NVIDIA GeForce RTX 5080, 16303, 15012").unwrap();
        assert_eq!(gpu.name, "NVIDIA GeForce RTX 5080");
        assert_eq!(gpu.vram_total_mib, Some(16303));
        assert_eq!(gpu.vram_free_mib, Some(15012));
    }

    #[test]
    fn blank_line_is_not_a_gpu() {
        assert!(parse_nvidia_smi_line("").is_none());
        assert!(parse_nvidia_smi_line(",,").is_none());
    }

    /// The required "machine without a GPU" case: point the nvidia-smi probe
    /// at a binary that cannot exist. Must return empty, must not panic —
    /// independent of whatever GPU the machine actually running this test
    /// has, so CI on a GPU box and CI on a headless box both exercise it.
    #[test]
    fn absent_nvidia_smi_yields_empty_not_panic() {
        let gpus = probe_gpus_via_nvidia_smi("nvidia-smi-does-not-exist-on-this-machine-xyz");
        assert!(gpus.is_empty());
    }

    #[test]
    fn absent_powershell_yields_none_ram_not_panic() {
        let ram = probe_ram_bytes_via("powershell-does-not-exist-on-this-machine-xyz");
        assert!(ram.is_none());
    }

    #[test]
    fn absent_wmi_command_yields_empty_gpu_fallback_not_panic() {
        let gpus = probe_gpus_via_wmi_command("powershell-does-not-exist-on-this-machine-xyz");
        assert!(gpus.is_empty());
    }

    #[test]
    fn unknown_backend_is_absent_not_a_panic() {
        let dirs = vec![std::path::PathBuf::from(
            "C:\\this\\path\\does\\not\\exist\\on\\this\\machine\\xyz",
        )];
        assert!(!backend_on_path(LocalBackend::LlamaCppServer, &dirs));
        assert_eq!(probe_backends(&dirs), Vec::new());
    }

    /// The full probe, end to end, on whatever machine runs the test suite.
    /// Not gated on any specific hardware being present: every field is
    /// `Option`/`Vec` and the only assertion is "did not panic, and cores is
    /// either unknown or a plausible small positive number".
    #[test]
    fn full_probe_never_panics() {
        let profile = probe();
        if let Some(cores) = profile.cpu_cores_logical {
            assert!(cores > 0 && cores < 4096);
        }
    }
}
